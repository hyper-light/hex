//! AC-T7.4.1: a CPU-only Firecracker microVM through the real `hex node` and its `firecracker`
//! class: the native lane's smoke boot before any GPU is bound (no device, no network).
//! Explicit inputs only (`HEX_NATIVE_FC_SMOKE`, a JSON file); run alone, `--ignored`, inside a
//! cgroup delegated to this binary (`HEX_DELEGATED`) whose outer scope bounds the whole process.
//! Missing hardware or configuration fails by name; nothing here enables hardware.
#![cfg(target_os = "linux")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio as HostStdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use hex_api::{
    Bounds, BoxId, BoxSpec, BoxState, CapacityResources, Class, ClassSpec, Exit, Network, OciSpec,
    Program, Refusal, Rootfs, Runtime, Stdio,
};
use hex_client::Client;
use serde_json::Value;

struct Inputs {
    value: Value,
    wait: Duration,
}

impl Inputs {
    fn load() -> Self {
        assert!(
            std::env::var_os("HEX_DELEGATED").is_some(),
            "run inside a cgroup delegated to this binary (HEX_DELEGATED)"
        );
        let path = std::env::var_os("HEX_NATIVE_FC_SMOKE").expect("HEX_NATIVE_FC_SMOKE inputs");
        let value: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let wait = Duration::from_millis(value["wait_ms"].as_u64().unwrap());
        assert!(!wait.is_zero());
        // A VM's create boots it: the client's learned create bound (twice the recorded slowest,
        // an OCI box's) would refuse it first, so every wait here measures and each call that
        // makes a VM carries this test's explicit deadline instead.
        hex_core::patience::measure_all();
        Self { value, wait }
    }

    fn path(&self, field: &str) -> PathBuf {
        PathBuf::from(
            self.value[field]
                .as_str()
                .unwrap_or_else(|| panic!("input {field}")),
        )
    }
}

/// This binary's own cgroup, made a parent: it moves into a leaf, as node_socket does (once).
fn own_cgroup() -> PathBuf {
    let line = fs::read_to_string("/proc/self/cgroup").unwrap();
    let own = Path::new("/sys/fs/cgroup").join(line.trim().strip_prefix("0::/").unwrap());
    if own.file_name().is_some_and(|name| name == "hex-fc-smoke") {
        return own.parent().unwrap().to_path_buf();
    }
    let leaf = own.join("hex-fc-smoke");
    fs::create_dir_all(&leaf).unwrap();
    fs::write(leaf.join("cgroup.procs"), std::process::id().to_string()).unwrap();
    own
}

/// A `hex node` with the firecracker class, ended by its own PID when dropped.
struct Node {
    child: Child,
    socket: PathBuf,
    state: PathBuf,
}

impl Node {
    fn start(inputs: &Inputs, own: &Path, name: &str, config: &Path) -> Self {
        let dir = inputs.path("work").join(name);
        assert!(!dir.exists(), "exclusive node directory {dir:?}");
        fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("node.sock");
        let state = dir.join("state");
        let begun = Instant::now();
        let mut child = Command::new(inputs.path("hex"))
            .arg("node")
            .arg("--socket")
            .arg(&socket)
            .arg("--state")
            .arg(&state)
            .arg("--box-root")
            .arg(own.join(format!("hex-fc-{name}")))
            .env("HEX_FIRECRACKER_CONFIG", config)
            .stdin(HostStdio::null())
            .stdout(HostStdio::piped())
            .stderr(fs::File::create(dir.join("node.stderr")).unwrap())
            .spawn()
            .unwrap();
        // The node's own "ready <socket>" line is the fact waited on; the deadline only guards it.
        let (tx, rx) = mpsc::channel();
        let stdout = child.stdout.take().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let want = format!("ready {}", socket.display());
        let until = begun + inputs.wait;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(line) if line == want => break,
                Ok(line) => eprintln!("node {name}: {line}"),
                Err(error) => panic!(
                    "node {name} not ready: {error}; stderr: {}",
                    fs::read_to_string(dir.join("node.stderr")).unwrap_or_default()
                ),
            }
        }
        eprintln!(
            "SMOKE node {name} ready after {:?} (pid {})",
            begun.elapsed(),
            child.id()
        );
        Self {
            child,
            socket,
            state,
        }
    }

    fn client(&self) -> Client {
        Client::connect(&self.socket).unwrap()
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        let pid = self.child.id();
        if self.child.try_wait().unwrap().is_none() {
            self.child.kill().unwrap();
        }
        let status = self.child.wait().unwrap();
        eprintln!("SMOKE node pid {pid} ended: {status}");
    }
}

/// The box's spec: the class's payload, with its admitted charge from the backend's own cost.
fn spec(inputs: &Inputs, config: &Path, id: &BoxId, init: &[&str]) -> BoxSpec {
    let config = hex_runtime::firecracker::Config::from_file(config).unwrap();
    let cpu_milli = inputs.value["cpu_milli"].as_u64().unwrap();
    let mem_bytes = inputs.value["memory_bytes"].as_u64().unwrap();
    let allocation = config
        .cost
        .allocate(CapacityResources {
            cpu_milli,
            mem_bytes,
            disk_bytes: 0,
        })
        .unwrap();
    BoxSpec {
        id: id.clone(),
        bounds: Bounds {
            cpu_max_milli: Some(u32::try_from(cpu_milli).unwrap()),
            memory_max: Some(mem_bytes),
            ..Bounds::default()
        },
        class: ClassSpec::Firecracker(Box::new(OciSpec {
            rootfs: Rootfs::Dir(inputs.path("rootfs")),
            init: Program::of(init),
            mounts: Vec::new(),
            hostname: None,
            network: Network::Private,
            hosts: Vec::new(),
        })),
        owner: None,
        devices: Vec::new(),
        allocation: Some(allocation),
        route: None,
    }
}

/// The live VMM of the VM whose state directory is `dir`: its PID and start time, read from /proc
/// (observation only; the node alone stops it).
fn vmm(binary: &Path, dir: &Path) -> Vec<(u32, u64)> {
    let mut found = Vec::new();
    for entry in fs::read_dir("/proc").unwrap() {
        let entry = entry.unwrap();
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(cmdline) = fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let words: Vec<_> = cmdline.split(|b| *b == 0).collect();
        let exe = words
            .first()
            .map(|w| Path::new(std::str::from_utf8(w).unwrap_or("")));
        let names_dir = words
            .iter()
            .any(|w| std::str::from_utf8(w).is_ok_and(|w| w.starts_with(&*dir.to_string_lossy())));
        if exe == Some(binary) && names_dir {
            let stat = fs::read_to_string(entry.path().join("stat")).unwrap();
            let start = stat
                .rsplit(") ")
                .next()
                .unwrap()
                .split(' ')
                .nth(19)
                .unwrap();
            found.push((pid, start.parse().unwrap()));
        }
    }
    found
}

fn alive(pid: u32, start: u64) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| {
            stat.rsplit(") ")
                .next()
                .and_then(|rest| rest.split(' ').nth(19))
                .map(|s| s.parse::<u64>().ok() == Some(start))
        })
        .unwrap_or(false)
}

/// The host's TUN/TAP interfaces (those with `tun_flags`): what a Firecracker network would add.
/// Other workloads' interfaces (a container's veth) come and go on a shared host.
fn taps() -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir("/sys/class/net")
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|path| path.join("tun_flags").exists())
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Every file named `<id>.json` under the node's state: its records of the box (store.rs).
fn records(state: &Path, id: &BoxId) -> Vec<PathBuf> {
    let name = format!("{id}.json");
    let mut found = Vec::new();
    let mut pending = vec![state.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                pending.push(path);
            } else if entry.file_name().to_string_lossy() == name {
                found.push(path);
            }
        }
    }
    found
}

/// The one VM directory a single box leaves in the backend's state while it runs.
fn vm_dir(config: &Path) -> PathBuf {
    let config = hex_runtime::firecracker::Config::from_file(config).unwrap();
    let mut dirs: Vec<_> = fs::read_dir(&config.state_dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    assert_eq!(dirs.len(), 1, "one VM directory: {dirs:?}");
    dirs.pop().unwrap()
}

fn state_dir_entries(config: &Path) -> Vec<PathBuf> {
    let config = hex_runtime::firecracker::Config::from_file(config).unwrap();
    fs::read_dir(&config.state_dir)
        .map(|entries| entries.map(|e| e.unwrap().path()).collect())
        .unwrap_or_default()
}

/// AC-T7.4.1: do boot one CPU-only Firecracker VM through the real node and adapter, run a
/// program in its box and delete it; expect readiness, the program's exit and output, a guest
/// that lists no PCI GPU function and was given no GPU words, and then no VMM process, no VM
/// state (its vsock and API sockets), no new TUN/TAP interface and no record of the box left.
#[test]
#[ignore = "native: KVM, a delegated cgroup and explicit pinned inputs (HEX_NATIVE_FC_SMOKE)"]
fn native_firecracker_cpu_box_runs_and_leaves_nothing() {
    let inputs = Inputs::load();
    let own = own_cgroup();
    let config = inputs.path("config");
    let binary = hex_runtime::firecracker::Config::from_file(&config)
        .unwrap()
        .binary;
    let before = taps();
    let node = Node::start(&inputs, &own, "a", &config);
    let client = node.client();
    assert!(
        client.classes().contains(&Class::Firecracker),
        "{:?}",
        client.classes()
    );
    let id = BoxId::new("fc-smoke").unwrap();
    let begun = Instant::now();
    let info = client
        .create_before(
            spec(&inputs, &config, &id, &["/probe", "--hold"]),
            Instant::now() + inputs.wait,
        )
        .unwrap();
    eprintln!(
        "SMOKE create (boot to ready, guest box made) {:?}",
        begun.elapsed()
    );
    assert_eq!(info.class, Class::Firecracker);
    if client.inspect(&id).unwrap().state == BoxState::Created {
        client.start(&id).unwrap();
    }
    let dir = vm_dir(&config);
    let running = vmm(&binary, &dir);
    assert_eq!(running.len(), 1, "one live VMM for the box: {running:?}");
    let boot = fs::read_to_string(dir.join("config.json")).unwrap();
    let boot: Value = serde_json::from_str(&boot).unwrap();
    let boot = boot["boot-source"]["boot_args"]
        .as_str()
        .unwrap()
        .to_owned();
    eprintln!(
        "SMOKE VMM pid {} start {}; boot_args {boot:?}",
        running[0].0, running[0].1
    );
    assert!(
        !boot.contains("hex.gpu"),
        "a VM given no GPU carries no GPU words"
    );

    let challenge = inputs.value["challenge"].as_str().unwrap();
    let begun = Instant::now();
    let (mut out, writer) = std::io::pipe().unwrap();
    let mut proc = client
        .exec(
            &id,
            Program::of(&["/probe", challenge]),
            Stdio {
                stdout: Some(writer),
                ..Stdio::null()
            },
        )
        .unwrap();
    // The program's output to its end, which comes when it ends; the outer scope bounds a hang.
    let mut said = String::new();
    out.read_to_string(&mut said).unwrap();
    let exit = proc.wait_for(inputs.wait).unwrap();
    eprintln!("SMOKE exec {:?}: {exit:?} {said:?}", begun.elapsed());
    assert_eq!(exit, Some(Exit::Code(0)));
    let mut lines = said.lines();
    assert_eq!(lines.next(), Some(challenge));
    let pci = lines.next().unwrap();
    assert!(
        pci.starts_with("pci-functions ") && pci.ends_with(" pci-gpu-functions 0"),
        "{pci}"
    );
    drop(proc);

    let begun = Instant::now();
    client.delete(&id).unwrap();
    eprintln!("SMOKE delete {:?}", begun.elapsed());
    assert!(client.list().unwrap().is_empty(), "the node lists no box");
    let (pid, start) = running[0];
    assert!(!alive(pid, start), "the VMM {pid} is gone");
    assert!(
        vmm(&binary, &dir).is_empty(),
        "no VMM names the VM's directory"
    );
    assert!(
        !dir.exists(),
        "the VM's state (vsock, API and readiness sockets) is gone"
    );
    assert!(
        state_dir_entries(&config).is_empty(),
        "{:?}",
        state_dir_entries(&config)
    );
    assert_eq!(taps(), before, "no TUN/TAP interface was left");
    drop(client);
    let state = node.state.clone();
    drop(node);
    let left = records(&state, &id);
    assert!(
        left.is_empty(),
        "the node's records of the box are closed: {left:?}"
    );
    eprintln!(
        "SMOKE cleanup: VMM {pid} absent, {dir:?} absent, TUN/TAP unchanged ({} before), no {id}.json under {state:?}",
        before.len()
    );
}

/// AC-T7.4.1: do create a box on a node whose configured kernel is not a kernel, so its VMM ends
/// before the guest is ready; expect the create refused Broken with the end of what the VMM wrote,
/// and nothing of the box left.
#[test]
#[ignore = "native: KVM, a delegated cgroup and explicit pinned inputs (HEX_NATIVE_FC_SMOKE)"]
fn native_firecracker_vmm_ending_before_readiness_is_refused_with_its_output() {
    let inputs = Inputs::load();
    let own = own_cgroup();
    let config = inputs.path("broken_config");
    let binary = hex_runtime::firecracker::Config::from_file(&config)
        .unwrap()
        .binary;
    let node = Node::start(&inputs, &own, "b", &config);
    let client = node.client();
    let id = BoxId::new("fc-smoke-broken").unwrap();
    let begun = Instant::now();
    let refusal = client
        .create_before(
            spec(&inputs, &config, &id, &["/probe", "--hold"]),
            Instant::now() + inputs.wait,
        )
        .unwrap_err();
    eprintln!("SMOKE refused after {:?}: {refusal:?}", begun.elapsed());
    let Refusal::Broken(why) = &refusal else {
        panic!("{refusal:?}")
    };
    let prefix = "Firecracker ended before guest readiness; its last output:\n";
    assert!(
        why.starts_with(prefix) && why.len() > prefix.len(),
        "{why:?}"
    );
    assert!(client.list().unwrap().is_empty(), "the node lists no box");
    assert!(
        state_dir_entries(&config).is_empty(),
        "{:?}",
        state_dir_entries(&config)
    );
    let config_text = hex_runtime::firecracker::Config::from_file(&config).unwrap();
    assert!(
        vmm(&binary, &config_text.state_dir).is_empty(),
        "no VMM names the backend's state"
    );
    drop(client);
    let state = node.state.clone();
    drop(node);
    assert!(records(&state, &id).is_empty(), "no record of the box");
}
