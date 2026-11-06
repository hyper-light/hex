//! The node agent's local socket over a live agent (decision 3 of docs/design/runtime-architecture.md;
//! AC-T7.1.1): `hex node` runs as a process of its own, and this test reaches it only through its
//! socket, with `hex-client`'s `Runtime`. "do X, expect Y" (§6):
//! - HEX-SOCK-9: connect, create a `Bundle` box, start it, exec `/usr/bin/true` with its stdout a pipe
//!   this test holds, read the output, and wait on the `Proc` for `Code(0)`, all over one socket;
//! - the hex-api conformance suite, every case, over the socket, on `oci` and on `process` boxes, and on
//!   `oci` boxes each with a network namespace of its own: as root they take the agent's pooled ones,
//!   rootless they make theirs on the create's path (decision 4; goal 4 plan step 4);
//! - HEX-GUEST-8 (native plan step 5, docs/design/shards-guest.md §3): the conformance suite, every
//!   case, on `oci` and `process` boxes, again through `hex guest relay` and `hex_client::guest`'s
//!   client, the path a macOS or Windows host takes to the node in its Shards guest, Unix sockets
//!   standing in for vsock;
//! - HEX-GUEST-17 (§4a): the conformance suite, every case, on `oci` boxes through
//!   `hex_runtime::guest::WithGuest`, the node's runtime on macOS and Windows, over `shards_api`'s
//!   simulator, whose guest bridges each port to `hex guest relay`: the real link, staging and spec
//!   rewrite, each box's root staged and its copy removed with it;
//! - HEX-SOCK-10: a box whose bundle makes a network namespace of its own is counted, as root as one
//!   taken from the agent's pool, rootless as one made on its create's path, and the counts cross the
//!   socket (decision 4; goal 4 plan steps 2 and 4);
//! - HEX-SOCK-6, the disconnect rule: a caller that goes away while its exec'd programs run (its
//!   connection closes with no release, as when its process dies) has each program ended and reaped,
//!   and counted in its box, which inspect reports; nothing it started is left running;
//! - T-T7.1.2 (goal 5 plan step 2): kill the agent at each record write of a create, its boxes'
//!   records durable and not, and restart it: the box killed mid-create is cleaned up, the one made is
//!   taken back and deletes, and nothing of either is left.
//!
//! Rootless where the caller isn't root, in a delegated cgroup v2 subtree, as oci_box.rs; where neither
//! is possible it prints SKIP and why rather than pass falsely.
#![cfg(target_os = "linux")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio as Std};
use std::time::{Duration, Instant};

use hex_api::conformance::{self, Act, Fixture};
use hex_api::{
    Bounds, BoxId, BoxSpec, BoxState, Class, ClassSpec, Exit, Program, Refusal, Runtime, Stdio,
};
use hex_client::Client;
use hex_core::record::{BoxRecord, Phase};
use hex_core::stream;
use hex_core::wire::{self, Answer, Call, Request, Streams};

const TEST: &str = "the_seam_holds_its_contract_over_the_node_agents_socket";

#[test]
fn the_seam_holds_its_contract_over_the_node_agents_socket() {
    let exe = env!("CARGO_BIN_EXE_hex").to_owned();
    let cache = cache_dir();
    match prepare_cgroup(&cache) {
        Ok(own) => exercise(&own, &cache, &exe),
        Err(direct) if std::env::var_os("HEX_DELEGATED").is_none() => {
            let has_systemd = Command::new("systemd-run")
                .arg("--version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !has_systemd {
                eprintln!(
                    "SKIP: own cgroup isn't ours to subdivide ({direct}) and no systemd --user for a private scope"
                );
                return;
            }
            let status = Command::new("systemd-run")
                .args(["--user", "--scope", "--quiet", "-p", "Delegate=yes"])
                .arg(std::env::current_exe().expect("test binary path"))
                .args([TEST, "--exact", "--nocapture", "--test-threads=1"])
                .env("HEX_DELEGATED", "1")
                .status()
                .expect("run the test in a delegated scope");
            assert!(status.success(), "the delegated re-run failed");
        }
        Err(direct) => {
            eprintln!("SKIP: no usable cgroup v2 delegation ({direct}); run on a cgroup-v2 host")
        }
    }
}

/// A `hex node` process serving its socket, killed when dropped.
struct Agent {
    child: Child,
    socket: PathBuf,
    box_root: PathBuf,
    _dir: Option<tempfile::TempDir>,
}

impl Agent {
    fn start(exe: &str, cache: &Path, box_root: PathBuf) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("agent-")
            .tempdir_in(cache)
            .unwrap();
        let mut agent = Self::over(exe, dir.path(), box_root, None);
        agent._dir = Some(dir);
        agent
    }

    /// A `hex node` whose records are in `dir`'s `state` and whose socket is `dir`'s `node.sock`, its
    /// boxes under `box_root`, with `runtime` its `XDG_RUNTIME_DIR` when given; `dir` is the caller's.
    fn over(exe: &str, dir: &Path, box_root: PathBuf, runtime: Option<&Path>) -> Self {
        let socket = dir.join("node.sock");
        let mut command = Command::new(exe);
        command
            .arg("node")
            .arg("--socket")
            .arg(&socket)
            .arg("--state")
            .arg(dir.join("state"))
            .arg("--box-root")
            .arg(&box_root)
            .stdout(Std::piped());
        if let Some(runtime) = runtime {
            command.env("XDG_RUNTIME_DIR", runtime);
        }
        let mut child = command.spawn().expect("start hex node");
        // The fact a client needs: the agent says it's ready once its socket listens.
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(
            line.trim(),
            format!("ready {}", socket.display()),
            "hex node said {line:?}"
        );
        Self {
            child,
            socket,
            box_root,
            _dir: None,
        }
    }

    fn client(&self) -> Client {
        Client::connect(&self.socket).expect("connect to hex node")
    }

    fn pid(&self) -> i32 {
        i32::try_from(self.child.id()).unwrap()
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Where one run keeps its bundles: a rootfs of the host's `/usr`, and the `hex` binary at `/hex`.
struct Lab {
    exe: String,
    rootfs: PathBuf,
    bundles: tempfile::TempDir,
    rootless: bool,
    uid: u32,
    gid: u32,
}

impl Lab {
    fn new(cache: &Path, exe: &str) -> Self {
        let (uid, gid) = (host_id("-u"), host_id("-g"));
        let bundles = tempfile::Builder::new()
            .prefix("socklab-")
            .tempdir_in(cache)
            .expect("bundles dir");
        let rootfs = bundles.path().join("rootfs");
        build_rootfs(&rootfs);
        Self {
            exe: exe.to_owned(),
            rootfs,
            bundles,
            rootless: uid != 0,
            uid,
            gid,
        }
    }

    fn bundle(&self, args: &[String]) -> PathBuf {
        self.bundle_with(args, false)
    }

    /// A bundle whose container runs `args`, with a network namespace of its own when `network`.
    fn bundle_with(&self, args: &[String], network: bool) -> PathBuf {
        self.bundle_hooked(args, network, "")
    }

    /// As [`Lab::bundle_with`], its config ending with `hooks`: a `"hooks"` member after a comma, or
    /// nothing.
    fn bundle_hooked(&self, args: &[String], network: bool, hooks: &str) -> PathBuf {
        let dir = tempfile::Builder::new()
            .prefix("bundle-")
            .tempdir_in(self.bundles.path())
            .expect("bundle dir")
            .keep();
        let (mut namespaces, mappings) = if self.rootless {
            (
                r#"{"type":"user"},{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"}"#
                    .to_owned(),
                format!(
                    r#","uidMappings":[{{"containerID":0,"hostID":{},"size":1}}],"gidMappings":[{{"containerID":0,"hostID":{},"size":1}}]"#,
                    self.uid, self.gid
                ),
            )
        } else {
            (
                r#"{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"}"#.to_owned(),
                String::new(),
            )
        };
        if network {
            namespaces.push_str(r#",{"type":"network"}"#);
        }
        assert!(args.iter().all(|a| a.is_ascii() && !a.contains('\\')));
        let config = format!(
            r#"{{
  "ociVersion": "1.3.0",
  "root": {{"path": "{rootfs}", "readonly": false}},
  "process": {{
    "cwd": "/",
    "args": {args:?},
    "env": ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"],
    "user": {{"uid": 0, "gid": 0}},
    "noNewPrivileges": true
  }},
  "mounts": [
    {{"destination": "/proc", "type": "proc", "source": "proc"}},
    {{"destination": "/dev", "type": "tmpfs", "source": "tmpfs", "options": ["nosuid","mode=755","size=65536k"]}},
    {{"destination": "/tmp", "type": "tmpfs", "source": "tmpfs", "options": ["nosuid","nodev"]}},
    {{"destination": "/usr", "type": "bind", "source": "/usr", "options": ["rbind","ro"]}},
    {{"destination": "/hex", "type": "bind", "source": "{exe}", "options": ["bind","ro"]}}
  ],
  "linux": {{"namespaces": [{namespaces}]{mappings}}}{hooks}
}}"#,
            rootfs = self.rootfs.display(),
            exe = self.exe,
        );
        std::fs::write(dir.join("config.json"), config).expect("config.json");
        dir
    }

    /// The `hex selftest-box` program for `act`, as a container sees the binary (`/hex`).
    fn act(act: &Act) -> Vec<String> {
        let mut args = vec!["/hex".to_owned(), "selftest-box".to_owned()];
        args.extend(act_args(act));
        args
    }
}

fn act_args(act: &Act) -> Vec<String> {
    match act {
        Act::Say(words) => vec!["say".to_owned(), (*words).to_owned()],
        Act::Fail(code) => vec!["fail".to_owned(), code.to_string()],
        Act::Wait => vec!["wait".to_owned()],
        Act::Hog(bytes) => vec!["hog".to_owned(), bytes.to_string()],
        Act::Spawn(n) => vec!["spawn".to_owned(), n.to_string()],
        Act::Echo => vec!["echo".to_owned()],
    }
}

fn program(args: &[String]) -> Program {
    Program::of(&args.iter().map(String::as_str).collect::<Vec<_>>())
}

/// `oci` boxes from bundles whose init and programs are `hex selftest-box` acts, each with a network
/// namespace of its own when the second field is set: one taken from the agent's pool as root, made on
/// the create's path rootless (decision 4).
struct OciBoxes<'a>(&'a Lab, bool);

impl Fixture for OciBoxes<'_> {
    fn spec(&self, id: BoxId, bounds: Bounds) -> BoxSpec {
        BoxSpec {
            id,
            bounds,
            class: ClassSpec::Bundle(self.0.bundle_with(&Lab::act(&Act::Wait), self.1)),
        }
    }

    fn init(&self, id: BoxId, act: &Act) -> BoxSpec {
        BoxSpec {
            id,
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(self.0.bundle_with(&Lab::act(act), self.1)),
        }
    }

    fn program(&self, act: &Act) -> Program {
        program(&Lab::act(act))
    }
}

/// `process` boxes, whose programs are the host's `hex selftest-box` acts.
struct ProcessBoxes<'a>(&'a str);

impl Fixture for ProcessBoxes<'_> {
    fn spec(&self, id: BoxId, bounds: Bounds) -> BoxSpec {
        BoxSpec {
            id,
            bounds,
            class: ClassSpec::Process,
        }
    }

    fn init(&self, id: BoxId, _act: &Act) -> BoxSpec {
        self.spec(id, Bounds::default())
    }

    fn program(&self, act: &Act) -> Program {
        let mut args = vec![self.0.to_owned(), "selftest-box".to_owned()];
        args.extend(act_args(act));
        program(&args)
    }
}

fn exercise(own: &Path, cache: &Path, exe: &str) {
    let lab = Lab::new(cache, exe);
    let agent = Agent::start(exe, cache, own.join("hex-sock"));
    let client = agent.client();
    assert_eq!(client.classes(), [Class::Process, Class::Oci]);
    if let Err(e) = example(&lab, &client) {
        eprintln!("SKIP: rootless OCI unavailable here ({e})");
        return;
    }
    namespaces_counted(&lab, &client);
    let failures = conformance::run(&client, &OciBoxes(&lab, false));
    assert!(
        failures.is_empty(),
        "oci boxes over the socket: {failures:#?}"
    );
    conformance_with_namespaces(&lab, &client);
    let failures = conformance::run(&client, &ProcessBoxes(exe));
    assert!(
        failures.is_empty(),
        "process boxes over the socket: {failures:#?}"
    );
    through_the_guest_relay(&lab, &agent, exe);
    through_a_simulated_guest(&lab, &agent, exe);
    disconnect(&lab, &agent, &client);
    crash_at_each_record_write(&lab, own, exe);
    assert!(client.list().unwrap().is_empty(), "a box is left");
}

/// The guest's ports as Unix sockets in the relay's directory.
struct GuestPorts(PathBuf);

impl hex_client::guest::Link for GuestPorts {
    fn dial(&self, port: u32) -> Result<Box<dyn hex_client::guest::Duplex>, Refusal> {
        let name = match port {
            stream::CONTROL_PORT => "control.sock",
            stream::STDIO_PORT => "stdio.sock",
            stream::files::FILES_PORT => "files.sock",
            other => panic!("dialed port {other:#x}"),
        };
        Ok(Box::new(
            std::os::unix::net::UnixStream::connect(self.0.join(name)).unwrap(),
        ))
    }
}

/// HEX-GUEST-8: start `hex guest relay` to the agent's socket, connect through it as a host reaches
/// the node in its guest, and run the conformance suite on `oci` and `process` boxes: expect every case
/// to pass, as over the socket itself.
fn through_the_guest_relay(lab: &Lab, agent: &Agent, exe: &str) {
    let dir = tempfile::Builder::new()
        .prefix("relay-")
        .tempdir_in(agent.socket.parent().unwrap())
        .unwrap();
    let mut relay = Command::new(exe)
        .args(["guest", "relay", "--node"])
        .arg(&agent.socket)
        .arg("--unix")
        .arg(dir.path())
        .stdout(Std::piped())
        .spawn()
        .expect("start hex guest relay");
    let mut line = String::new();
    BufReader::new(relay.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line.trim(), "ready", "hex guest relay said {line:?}");
    let guest =
        hex_client::guest::Client::connect(std::sync::Arc::new(GuestPorts(dir.path().to_owned())))
            .expect("connect through the guest relay");
    assert_eq!(guest.classes(), [Class::Process, Class::Oci]);
    let failures = conformance::run(&guest, &OciBoxes(lab, false));
    assert!(
        failures.is_empty(),
        "oci boxes through the guest relay: {failures:#?}"
    );
    let failures = conformance::run(&guest, &ProcessBoxes(exe));
    assert!(
        failures.is_empty(),
        "process boxes through the guest relay: {failures:#?}"
    );
    drop(guest);
    relay.kill().unwrap();
    relay.wait().unwrap();
}

/// Copies `from` into `to` until `from` ends, then ends `to`'s sending half: one direction of a bridged
/// connection, the guest's vsock port to the relay's socket or back.
fn pump(mut from: Box<dyn shards_api::Stream>, to: std::os::unix::net::UnixStream) {
    let mut to_w = to.try_clone().unwrap();
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut from, &mut to_w);
        let _ = to_w.shutdown(std::net::Shutdown::Write);
    });
}

fn pump_back(mut from: std::os::unix::net::UnixStream, mut to: Box<dyn shards_api::Stream>) {
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut from, &mut to);
        let _ = to.shutdown(std::net::Shutdown::Write);
    });
}

/// HEX-GUEST-17: start `hex guest relay` (staging under a folder of the test's) to the agent's socket,
/// boot a simulated Shards guest whose PID 1 bridges each of the channel's ports to it, and run the
/// conformance suite on `oci` boxes through `WithGuest` over that guest, one share `/` seen at `/` (a
/// simulated guest is the host): expect every case to pass, the guest booted once, and no staged
/// copy left once the suite's boxes are deleted.
fn through_a_simulated_guest(lab: &Lab, agent: &Agent, exe: &str) {
    use hex_runtime::guest::shares::Share;
    use hex_runtime::guest::{GuestConfig, GuestRuntime, WithGuest};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let dir = tempfile::Builder::new()
        .prefix("guest-")
        .tempdir_in(agent.socket.parent().unwrap())
        .unwrap();
    let staged = dir.path().join("staged");
    let mut relay = Command::new(exe)
        .args(["guest", "relay", "--node"])
        .arg(&agent.socket)
        .arg("--unix")
        .arg(dir.path())
        .arg("--staged")
        .arg(&staged)
        .stdout(Std::piped())
        .spawn()
        .expect("start hex guest relay");
    let mut line = String::new();
    BufReader::new(relay.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line.trim(), "ready", "hex guest relay said {line:?}");
    let ports = dir.path().to_owned();
    let boots = Arc::new(AtomicUsize::new(0));
    let booted = Arc::clone(&boots);
    let guests = shards_api::sim::SimGuests::new(move |host| {
        booted.fetch_add(1, Ordering::SeqCst);
        for port in [
            stream::CONTROL_PORT,
            stream::STDIO_PORT,
            stream::files::FILES_PORT,
        ] {
            host.listen(port);
        }
        while let Some((port, conn)) = host.accept() {
            let name = match port {
                stream::CONTROL_PORT => "control.sock",
                stream::STDIO_PORT => "stdio.sock",
                _ => "files.sock",
            };
            let relay = std::os::unix::net::UnixStream::connect(ports.join(name)).unwrap();
            pump(conn.try_clone().unwrap(), relay.try_clone().unwrap());
            pump_back(relay, conn);
        }
        shards_api::End::Stopped
    });
    let config = GuestConfig {
        // A simulated guest boots nothing: no binary, kernel or initramfs is read.
        binary: PathBuf::from("/nonexistent/shards"),
        kernel: PathBuf::from("/nonexistent/kernel"),
        initrd: PathBuf::from("/nonexistent/initrd"),
        vcpus: 1,
        memory_mib: 256,
        shares: vec![Share {
            dir: PathBuf::from("/"),
            read_only: false,
            at: "/".to_owned(),
        }],
        staged: staged.to_str().unwrap().to_owned(),
    };
    let node = WithGuest::new(
        hex_api::sim::SimRuntime::new(),
        GuestRuntime::new(Arc::new(guests), config),
    );
    assert_eq!(node.classes(), [Class::Process, Class::Oci]);
    let failures = conformance::run(&node, &OciBoxes(lab, false));
    assert!(
        failures.is_empty(),
        "oci boxes through a simulated Shards guest: {failures:#?}"
    );
    assert_eq!(
        boots.load(Ordering::SeqCst),
        1,
        "the guest booted more than once"
    );
    assert!(node.list().unwrap().is_empty(), "a box is left");
    let left: Vec<_> = std::fs::read_dir(&staged).unwrap().collect();
    assert!(left.is_empty(), "staged copies left: {left:?}");
    drop(node);
    relay.kill().unwrap();
    relay.wait().unwrap();
}

/// HEX-SOCK-9, the plan's example: a `Bundle` box made, started, and `/usr/bin/true` exec'd in it
/// with its stdout a pipe this test holds, its output read and its `Code(0)` waited for, all over one
/// socket. `Err` when no container can be made here (rootless needs unprivileged user namespaces).
fn example(lab: &Lab, client: &Client) -> Result<(), String> {
    let id = BoxId::new("example").unwrap();
    let made = client.create(BoxSpec {
        id: id.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Bundle(lab.bundle(&Lab::act(&Act::Wait))),
    });
    let info = made.map_err(|e| e.to_string())?;
    assert_eq!((info.class, info.state), (Class::Oci, BoxState::Created));
    client.start(&id).unwrap();
    assert_eq!(client.inspect(&id).unwrap().state, BoxState::Running);
    let (mut out, w) = std::io::pipe().unwrap();
    let mut proc = client
        .exec(
            &id,
            Program::of(&["/usr/bin/true"]),
            Stdio {
                stdout: Some(w),
                ..Stdio::null()
            },
        )
        .unwrap();
    let mut said = String::new();
    out.read_to_string(&mut said).unwrap();
    assert_eq!(said, "", "true says nothing");
    assert_eq!(
        proc.wait_for(conformance::PATIENCE).unwrap(),
        Some(Exit::Code(0))
    );
    let (mut out, w) = std::io::pipe().unwrap();
    let mut proc = client
        .exec(
            &id,
            program(&Lab::act(&Act::Say("from the socket"))),
            Stdio {
                stdout: Some(w),
                ..Stdio::null()
            },
        )
        .unwrap();
    let mut said = String::new();
    out.read_to_string(&mut said).unwrap();
    assert_eq!(said, "from the socket\n");
    assert_eq!(
        proc.wait_for(conformance::PATIENCE).unwrap(),
        Some(Exit::Code(0))
    );
    client.delete(&id).unwrap();
    Ok(())
}

/// HEX-SOCK-10 over the live agent (decision 4; goal 4 plan steps 2 and 4): the agent counts each
/// box's network namespace, and the counts cross its socket. A box with a network namespace of its
/// own takes one from a root agent's pool, once the pool holds one, and a rootless agent's makes its
/// own on its create's path; boxes without one (the example's, and every other here) count none.
fn namespaces_counted(lab: &Lab, client: &Client) {
    let before = client.netns().unwrap();
    assert_eq!(
        (before.taken, before.made_on_path),
        (0, 0),
        "the example's box has no network namespace of its own"
    );
    if !lab.rootless {
        wait_for("the agent's pool to hold a namespace", || {
            let c = client.netns().unwrap();
            c.made_ahead > c.taken + c.discarded
        });
    }
    let id = BoxId::new("own-network").unwrap();
    client
        .create(BoxSpec {
            id: id.clone(),
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(lab.bundle_with(&Lab::act(&Act::Wait), true)),
        })
        .unwrap();
    client.delete(&id).unwrap();
    let after = client.netns().unwrap();
    assert_eq!(
        (after.taken, after.made_on_path),
        if lab.rootless { (0, 1) } else { (1, 0) },
        "(taken, made on the path)"
    );
}

/// The conformance suite again over the socket, every `oci` box with a network namespace of its own
/// (decision 4): as root, once the agent's pool holds one, the boxes take theirs from it; rootless, each
/// is made on its create's path and none is taken.
fn conformance_with_namespaces(lab: &Lab, client: &Client) {
    if !lab.rootless {
        wait_for("the agent's pool to hold a namespace", || {
            let c = client.netns().unwrap();
            c.made_ahead > c.taken + c.discarded
        });
    }
    let before = client.netns().unwrap();
    let failures = conformance::run(client, &OciBoxes(lab, true));
    assert!(
        failures.is_empty(),
        "oci boxes with network namespaces of their own over the socket: {failures:#?}"
    );
    let after = client.netns().unwrap();
    if lab.rootless {
        assert_eq!(after.taken, before.taken, "a rootless agent takes none");
        assert!(after.made_on_path > before.made_on_path, "{after:?}");
    } else {
        assert!(
            after.taken > before.taken,
            "the pool served them: {after:?}"
        );
    }
}

/// HEX-SOCK-6 over the live agent: a caller execs a waiting program in an `oci` box and in a `process`
/// box over a connection of its own, then goes away without releasing them. Each program is ended and
/// reaped (no process of it in its box, no zombie of it under the agent), and its box counts it.
fn disconnect(lab: &Lab, agent: &Agent, client: &Client) {
    let oci = BoxId::new("left-oci").unwrap();
    let process = BoxId::new("left-process").unwrap();
    client
        .create(BoxSpec {
            id: oci.clone(),
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(lab.bundle(&Lab::act(&Act::Wait))),
        })
        .unwrap();
    client.start(&oci).unwrap();
    client
        .create(BoxSpec {
            id: process.clone(),
            bounds: Bounds::default(),
            class: ClassSpec::Process,
        })
        .unwrap();
    let init = procs(&agent.box_root, &oci);
    assert_eq!(init.len(), 1, "the oci box holds its init alone: {init:?}");
    let caller = raw(&agent.socket);
    let mut pids = Vec::new();
    for (n, (id, args)) in [
        (oci.clone(), Lab::act(&Act::Wait)),
        (process.clone(), {
            let mut a = vec![lab.exe.clone(), "selftest-box".to_owned()];
            a.extend(act_args(&Act::Wait));
            a
        }),
    ]
    .into_iter()
    .enumerate()
    {
        let n = u64::try_from(n + 1).unwrap();
        let call = Call::Exec {
            id: id.clone(),
            program: program(&args),
            streams: Streams::default(),
        };
        send(
            &caller,
            &wire::request(wire::VERSION, n, &Request::Call(call), wire::MAX_MESSAGE).unwrap(),
        );
        match answer(&caller) {
            (h, Answer::Started { pid }) if h.id == n => pids.push(pid),
            other => panic!("exec in {id}: {other:?}"),
        }
    }
    wait_for("each program to run in its box", || {
        procs(&agent.box_root, &oci).len() == 2 && procs(&agent.box_root, &process).len() == 1
    });
    drop(caller);
    for id in [&oci, &process] {
        wait_for(&format!("{id} to count its abandoned program"), || {
            client.inspect(id).unwrap().abandoned == 1
        });
    }
    wait_for("the programs to be ended", || {
        procs(&agent.box_root, &oci) == init && procs(&agent.box_root, &process).is_empty()
    });
    for pid in &pids {
        wait_for(&format!("program {pid} to be reaped"), || {
            !Path::new(&format!("/proc/{pid}")).exists()
        });
    }
    wait_for("no zombie under the agent", || {
        zombies_of(agent.pid()).is_empty()
    });
    assert_eq!(
        client.inspect(&oci).unwrap().state,
        BoxState::Running,
        "the init runs on"
    );
    client.delete(&oci).unwrap();
    client.delete(&process).unwrap();
}

/// Where a crash case keeps the agent's containers' state, and so whether its boxes' records are
/// durable: on tmpfs, as the node keeps it in its runtime directory, the records not fsynced; or on
/// disk (an `XDG_RUNTIME_DIR` on disk, as a rootless node with none keeps it in its state directory),
/// every record fsynced.
#[derive(Clone, Copy, Debug)]
enum Kept {
    Tmpfs,
    Disk,
}

/// Where in a create a crash case kills the agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum At {
    /// In the record write that marks the box creating, before anything of it is made.
    Creating,
    /// In the record write that marks it ready, its cgroup, container and init made.
    Ready,
    /// After both: the create done.
    Made,
}

/// T-T7.1.2 (goal 5 plan step 2): kill the node agent at each record write of a create, then restart
/// it, with its boxes' records durable and not. The agent is held in a record write by its temp file, a
/// FIFO no one has opened, and killed once the kernel shows it waiting there: in the write that marks
/// the box creating, and in the one that marks it ready (the FIFO made while the box's createRuntime
/// hook holds the create, its container made); and after both. Restarted, the agent cleans up the box
/// killed mid-create and takes back the one made, which then deletes, so at every point nothing of the
/// box is left: no record, temp file, cgroup or container state.
fn crash_at_each_record_write(lab: &Lab, own: &Path, exe: &str) {
    for kept in [Kept::Tmpfs, Kept::Disk] {
        for at in [At::Creating, At::Ready, At::Made] {
            crash_at(lab, own, exe, kept, at);
        }
    }
}

fn crash_at(lab: &Lab, own: &Path, exe: &str, kept: Kept, at: At) {
    let name = format!("crash-{kept:?}-{at:?}").to_lowercase();
    let dir = tempfile::Builder::new()
        .prefix(&format!("{name}-"))
        .tempdir_in(lab.bundles.path())
        .unwrap();
    // The agent's oci root, where the node puts it: a root node's under /run, a rootless node's under
    // its runtime directory, which this case may set on disk.
    let (runtime, oci_root) = match (kept, lab.rootless) {
        (Kept::Tmpfs, false) => (None, PathBuf::from("/run/hex-node")),
        (Kept::Tmpfs, true) => match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(xdg) => (None, PathBuf::from(xdg).join("hex-node")),
            None => {
                eprintln!("SKIP {name}: no XDG_RUNTIME_DIR");
                return;
            }
        },
        (Kept::Disk, true) => {
            let run = dir.path().join("run");
            std::fs::create_dir(&run).unwrap();
            let oci_root = run.join("hex-node");
            (Some(run), oci_root)
        }
        (Kept::Disk, false) => {
            eprintln!("SKIP {name}: a root node keeps its containers' state under /run");
            return;
        }
    };
    let id = BoxId::new(&name).unwrap();
    let state = dir.path().join("state");
    let record = state.join(format!("{id}.json"));
    let temp = state.join(format!(".{id}.tmp"));
    let cgroup = own.join(&name).join(id.as_str());
    let (hooked, go) = (dir.path().join("hooked"), dir.path().join("go"));
    let bundle = if at == At::Ready {
        // The create waits in its createRuntime hook, its container made, until this test makes `go`.
        let script = format!(
            "touch {} && until [ -e {} ]; do sleep 0.01; done",
            hooked.display(),
            go.display()
        );
        let hooks = format!(
            r#",
  "hooks": {{"createRuntime": [{{"path": "/bin/sh", "args": ["sh", "-c", {script:?}], "env": ["PATH=/usr/bin:/bin"]}}]}}"#
        );
        lab.bundle_hooked(&Lab::act(&Act::Wait), false, &hooks)
    } else {
        lab.bundle(&Lab::act(&Act::Wait))
    };
    let spec = BoxSpec {
        id: id.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Bundle(bundle),
    };
    let agent = Agent::over(exe, dir.path(), own.join(&name), runtime.as_deref());
    if on_tmpfs(&oci_root) != matches!(kept, Kept::Tmpfs) {
        eprintln!("SKIP {name}: {} isn't where it's kept", oci_root.display());
        return;
    }
    // Made once the agent has recovered, which removes a temp file it finds.
    if at == At::Creating {
        mkfifo(&temp);
    }
    let socket = agent.socket.clone();
    let create = std::thread::spawn(move || Client::connect(&socket).and_then(|c| c.create(spec)));
    match at {
        At::Creating => {}
        At::Ready => {
            wait_for("the create's hook to hold it", || hooked.exists());
            mkfifo(&temp);
            std::fs::write(&go, b"").unwrap();
        }
        At::Made => {
            wait_for("the create", || create.is_finished());
        }
    }
    if at != At::Made {
        wait_for("the agent to wait in its record write", || {
            waits_opening_a_fifo(agent.pid())
        });
    }
    // What the kill leaves, as the point says.
    let phase = || {
        std::fs::read(&record)
            .ok()
            .map(|bytes| BoxRecord::decode(&bytes).unwrap().phase)
    };
    let (written, made) = match at {
        At::Creating => (None, 0),
        At::Ready => (Some(Phase::Creating), 1),
        At::Made => (Some(Phase::Ready), 1),
    };
    assert_eq!(phase(), written, "{name}: the record at the kill");
    assert_eq!(
        (
            containers_of(&oci_root, &id).len(),
            usize::from(cgroup.exists())
        ),
        (made, made),
        "{name}: (container states, cgroups) at the kill"
    );
    drop(agent);
    let created = create.join().unwrap();
    assert_eq!(
        created.is_ok(),
        at == At::Made,
        "{name}: the create: {created:?}"
    );

    let agent = Agent::over(exe, dir.path(), own.join(&name), runtime.as_deref());
    let client = agent.client();
    if at == At::Made {
        let info = client.inspect(&id).expect("the box made is taken back");
        assert_eq!(info.state, BoxState::Created, "{name}: as it was");
        client.delete(&id).unwrap();
    } else {
        assert_eq!(
            client.inspect(&id),
            Err(Refusal::NoSuchBox(id.clone())),
            "{name}: the box killed mid-create is cleaned up"
        );
    }
    assert!(client.list().unwrap().is_empty(), "{name}: a box is left");
    assert!(!record.exists(), "{name}: its record is left");
    assert!(!temp.exists(), "{name}: its record's temp file is left");
    assert!(
        containers_of(&oci_root, &id).is_empty(),
        "{name}: its container's state is left"
    );
    assert!(!cgroup.exists(), "{name}: its cgroup is left");
    eprintln!(
        "{name}: killed with the record {written:?} and {made} container made; after the restart {}, nothing left",
        if at == At::Made {
            "taken back, then deleted"
        } else {
            "cleaned up"
        }
    );
}

/// The state directories under `oci_root` of box `id`'s container, whatever node's: each is named
/// `<node key>-<box id>`, the key 16 hex digits.
fn containers_of(oci_root: &Path, id: &BoxId) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(oci_root) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.split_once('-'))
                .is_some_and(|(key, rest)| {
                    key.len() == 16
                        && key.bytes().all(|b| b.is_ascii_hexdigit())
                        && rest == id.as_str()
                })
        })
        .collect()
}

/// Whether a thread of process `pid` waits to open a FIFO no one has opened the other end of: the
/// kernel shows it in `wait_for_partner` (fs/pipe.c), or in `fifo_open` where that's inlined.
fn waits_opening_a_fifo(pid: i32) -> bool {
    std::fs::read_dir(format!("/proc/{pid}/task")).is_ok_and(|tasks| {
        tasks
            .filter_map(Result::ok)
            .filter_map(|task| std::fs::read_to_string(task.path().join("wchan")).ok())
            .any(|wchan| matches!(wchan.trim(), "wait_for_partner" | "fifo_open"))
    })
}

fn mkfifo(path: &Path) {
    assert!(Command::new("mkfifo").arg(path).status().unwrap().success());
}

/// Whether `dir` lies on tmpfs (include/uapi/linux/magic.h, `TMPFS_MAGIC`).
fn on_tmpfs(dir: &Path) -> bool {
    rustix::fs::statfs(dir).is_ok_and(|fs| i128::from(fs.f_type) == 0x0102_1994)
}

/// A connection that sends exactly what it is given, and goes away as a dead process's does: no
/// release is sent for what it started.
fn raw(socket: &Path) -> std::os::fd::OwnedFd {
    use rustix::net::{AddressFamily, SocketAddrUnix, SocketFlags, SocketType};
    let fd = rustix::net::socket_with(
        AddressFamily::UNIX,
        SocketType::SEQPACKET,
        SocketFlags::CLOEXEC,
        None,
    )
    .unwrap();
    rustix::net::connect(&fd, &SocketAddrUnix::new(socket).unwrap()).unwrap();
    fd
}

fn send(fd: &std::os::fd::OwnedFd, bytes: &[u8]) {
    rustix::net::send(fd, bytes, rustix::net::SendFlags::NOSIGNAL).unwrap();
}

fn answer(fd: &std::os::fd::OwnedFd) -> (wire::Header, Answer) {
    let mut ready = [rustix::event::PollFd::new(fd, rustix::event::PollFlags::IN)];
    let ts = rustix::event::Timespec {
        tv_sec: 60,
        tv_nsec: 0,
    };
    assert_eq!(
        rustix::event::poll(&mut ready, Some(&ts)).unwrap(),
        1,
        "no answer came"
    );
    let mut buf = vec![0u8; wire::MAX_MESSAGE];
    let (n, _) = rustix::net::recv(fd, &mut buf, rustix::net::RecvFlags::empty()).unwrap();
    let msg = wire::Received {
        bytes: &buf[..n],
        size: n,
        descriptors: 0,
        descriptors_cut: false,
    };
    wire::read_answer(&msg, wire::MAX_MESSAGE).unwrap()
}

/// The processes in a box's cgroup (an ended one leaves it as it ends).
fn procs(box_root: &Path, id: &BoxId) -> Vec<String> {
    std::fs::read_to_string(box_root.join(id.as_str()).join("cgroup.procs"))
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

/// The agent's children that have ended and aren't reaped.
fn zombies_of(agent: i32) -> Vec<String> {
    let mut out = Vec::new();
    for task in std::fs::read_dir(format!("/proc/{agent}/task"))
        .unwrap()
        .flatten()
    {
        let children = std::fs::read_to_string(task.path().join("children")).unwrap_or_default();
        for child in children.split_whitespace() {
            let stat = std::fs::read_to_string(format!("/proc/{child}/stat")).unwrap_or_default();
            if stat
                .rsplit_once(')')
                .is_some_and(|(_, rest)| rest.trim_start().starts_with('Z'))
            {
                out.push(child.to_owned());
            }
        }
    }
    out
}

fn wait_for(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + conformance::PATIENCE;
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

/// A rootfs of the host's `/usr`, bound in by each bundle, with the host's top-level links into it.
fn build_rootfs(rootfs: &Path) {
    for d in ["proc", "dev", "tmp", "usr"] {
        std::fs::create_dir_all(rootfs.join(d)).unwrap();
    }
    std::fs::File::create(rootfs.join("hex")).unwrap();
    for link in ["bin", "sbin", "lib", "lib64"] {
        if let Ok(target) = std::fs::read_link(Path::new("/").join(link)) {
            let target = target.to_string_lossy().trim_start_matches('/').to_owned();
            std::os::unix::fs::symlink(target, rootfs.join(link)).unwrap();
        }
    }
}

fn host_id(flag: &str) -> u32 {
    let out = Command::new("id").arg(flag).output().expect("id");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("id is a number")
}

fn own_cgroup() -> PathBuf {
    let own = std::fs::read_to_string("/proc/self/cgroup").expect("/proc/self/cgroup");
    let rel = own
        .lines()
        .find_map(|l| l.strip_prefix("0::"))
        .expect("a 0:: cgroup line")
        .trim_start_matches('/');
    PathBuf::from("/sys/fs/cgroup").join(rel)
}

/// Moves this process into a leaf of its own cgroup and confirms every controller is delegated, as
/// oci_box.rs does; the agent this test starts inherits the leaf, and makes its boxes beside it.
fn prepare_cgroup(disk: &Path) -> Result<PathBuf, String> {
    let own = own_cgroup();
    let leaf = own.join("hex-agent");
    std::fs::create_dir_all(&leaf).map_err(|e| format!("making a leaf for the agent: {e}"))?;
    std::fs::write(leaf.join("cgroup.procs"), format!("{}", std::process::id()))
        .map_err(|e| format!("moving self into a leaf: {e}"))?;
    let probe = hex_runtime::linux::cgroup::BoxRoot::open(
        &own.join("hex-probe"),
        hex_runtime::linux::disks::under(disk).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;
    let missing: Vec<&str> = hex_core::cgroup::CONTROLLERS
        .into_iter()
        .filter(|c| !probe.controllers().contains(*c))
        .collect();
    let _ = std::fs::remove_dir(own.join("hex-probe"));
    if missing.is_empty() {
        Ok(own)
    } else {
        Err(format!(
            "{} can't hand down the {} controllers",
            own.display(),
            missing.join(", ")
        ))
    }
}

fn cache_dir() -> PathBuf {
    let cache =
        PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".cache/hex-t7/socklab");
    std::fs::create_dir_all(&cache).expect("cache dir");
    cache
}
