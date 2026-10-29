//! The node agent's local socket over a live agent (decision 3 of docs/design/runtime-architecture.md;
//! AC-T7.1.1): `hex node` runs as a process of its own, and this test reaches it only through its
//! socket, with `hex-client`'s `Runtime`. "do X, expect Y" (§6):
//! - HEX-SOCK-9: connect, create a `Bundle` box, start it, exec `/usr/bin/true` with its stdout a pipe
//!   this test holds, read the output, and wait on the `Proc` for `Code(0)`, all over one socket;
//! - the hex-api conformance suite, every case, over the socket, on `oci` and on `process` boxes, and on
//!   `oci` boxes each with a network namespace of its own: as root they take the agent's pooled ones,
//!   rootless they make theirs on the create's path (decision 4; goal 4 plan step 4);
//! - HEX-SOCK-10: a box whose bundle makes a network namespace of its own is counted, as root as one
//!   taken from the agent's pool, rootless as one made on its create's path, and the counts cross the
//!   socket (decision 4; goal 4 plan steps 2 and 4);
//! - HEX-SOCK-6, the disconnect rule: a caller that goes away while its exec'd programs run (its
//!   connection closes with no release, as when its process dies) has each program ended and reaped,
//!   and counted in its box, which inspect reports; nothing it started is left running.
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
use hex_api::{Bounds, BoxId, BoxSpec, BoxState, Class, ClassSpec, Exit, Program, Runtime, Stdio};
use hex_client::Client;
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
    _dir: tempfile::TempDir,
}

impl Agent {
    fn start(exe: &str, cache: &Path, box_root: PathBuf) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("agent-")
            .tempdir_in(cache)
            .unwrap();
        let socket = dir.path().join("node.sock");
        let mut child = Command::new(exe)
            .arg("node")
            .arg("--socket")
            .arg(&socket)
            .arg("--state")
            .arg(dir.path().join("state"))
            .arg("--box-root")
            .arg(&box_root)
            .stdout(Std::piped())
            .spawn()
            .expect("start hex node");
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
            _dir: dir,
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
  "linux": {{"namespaces": [{namespaces}]{mappings}}}
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
    disconnect(&lab, &agent, &client);
    assert!(client.list().unwrap().is_empty(), "a box is left");
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
