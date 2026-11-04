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
//!   and counted in its box, which inspect reports; nothing it started is left running;
//! - T-T7.1.2 (goal 5 extended step 6): kill the agent while a box of an owner holds a slot and another
//!   box's slot is idle, with a box of no owner beside them, and restart it: the idle slot is removed,
//!   said so on stderr, the live boxes are taken back, the owner's in its recorded slot, and once they're
//!   deleted nothing is left under the oci boxes' root;
//! - T-T7.1.2 (goal 5 plan step 2; goal 5 extended step 5): kill the agent at each point of a create
//!   (in a debug build, at its cgroup made and at its bundle rendered, where the agent armed by
//!   `hex node --crash-at` kills itself; its cgroup and its container's state directory made; its
//!   init cloned; inside its one record write; after it), its boxes' records durable and not, and
//!   restart it: the box killed mid-create is cleaned up by what it left, the one made is taken back
//!   and deletes, and nothing of either is left; and a box whose own spec names the crash point, on an
//!   agent that armed none, is made whole, the agent living on.
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

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio as Std};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hex_api::conformance::{self, Act, Fixture};
use hex_api::{
    Bounds, BoxId, BoxOwner, BoxSpec, BoxState, Class, ClassSpec, Exit, Program, Refusal, Runtime,
    Stdio,
};
#[cfg(debug_assertions)]
use hex_api::{Network, OciSpec, Rootfs};
use hex_client::Client;
use hex_core::record::{BoxRecord, Phase};
use hex_core::wire::{self, Answer, Call, Request, Streams};
use hex_node::OCI_BOXES;

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
    /// The agent's stderr, drained by a thread as it is written so the pipe never fills: what it says
    /// about the boxes it recovered, which a crash case reads to see why one was removed.
    errors: Arc<Mutex<String>>,
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
        Self::armed(exe, dir, box_root, runtime, None)
    }

    /// As [`Agent::over`], armed at a debug build's crash point (`hex node --crash-at`) when one's given.
    fn armed(
        exe: &str,
        dir: &Path,
        box_root: PathBuf,
        runtime: Option<&Path>,
        crash_at: Option<&str>,
    ) -> Self {
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
            .stdout(Std::piped())
            .stderr(Std::piped());
        if let Some(point) = crash_at {
            command.arg("--crash-at").arg(point);
        }
        if let Some(runtime) = runtime {
            command.env("XDG_RUNTIME_DIR", runtime);
        }
        let mut child = command.spawn().expect("start hex node");
        // Drain stderr as it comes so the pipe never fills and blocks the agent, and keep what it says
        // for a crash case to read (the boxes it recovered, and why each was removed).
        let errors = Arc::new(Mutex::new(String::new()));
        let stderr = child.stderr.take().unwrap();
        let sink = Arc::clone(&errors);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                sink.lock().unwrap().push_str(&line);
                line.clear();
            }
        });
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
            errors,
        }
    }

    /// What the agent has said on stderr so far: the boxes recovery removed, and why each
    /// ([`Agent::errors`]).
    fn errors(&self) -> String {
        self.errors.lock().unwrap().clone()
    }

    fn client(&self) -> Client {
        Client::connect(&self.socket).expect("connect to hex node")
    }

    fn pid(&self) -> i32 {
        i32::try_from(self.child.id()).unwrap()
    }

    /// Waits for the agent to end, and how it ended.
    #[cfg(debug_assertions)]
    fn ended(&mut self) -> std::process::ExitStatus {
        self.child.wait().unwrap()
    }

    /// Whether the agent still runs.
    #[cfg(debug_assertions)]
    fn runs(&mut self) -> bool {
        self.child.try_wait().unwrap().is_none()
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

    /// A bundle whose container runs `args` holding Docker's default capabilities (no CAP_SYS_ADMIN), so
    /// its box's cgroup may be handed to its owner's next box (goal 5 extended, step 6).
    fn bundle_owned(&self, args: &[String]) -> PathBuf {
        let dir = self.bundle(args);
        let caps = r#"["CAP_CHOWN","CAP_DAC_OVERRIDE","CAP_FSETID","CAP_FOWNER","CAP_MKNOD","CAP_NET_RAW","CAP_SETGID","CAP_SETUID","CAP_SETFCAP","CAP_SETPCAP","CAP_NET_BIND_SERVICE","CAP_SYS_CHROOT","CAP_KILL","CAP_AUDIT_WRITE"]"#;
        let config = std::fs::read_to_string(dir.join("config.json")).unwrap();
        let config = config.replace(
            r#""noNewPrivileges": true"#,
            &format!(
                r#""capabilities": {{"bounding": {caps}, "effective": {caps}, "permitted": {caps}}},
    "noNewPrivileges": true"#
            ),
        );
        std::fs::write(dir.join("config.json"), config).unwrap();
        dir
    }

    /// An `OciSpec` the node renders a bundle for: the lab's rootfs, the host's network, an init that
    /// waits.
    #[cfg(debug_assertions)]
    fn spec(&self) -> OciSpec {
        OciSpec {
            rootfs: Rootfs::Dir(self.rootfs.clone()),
            init: program(&Self::act(&Act::Wait)),
            mounts: Vec::new(),
            hostname: None,
            network: Network::Host,
            hosts: Vec::new(),
        }
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
            owner: None,
        }
    }

    fn init(&self, id: BoxId, act: &Act) -> BoxSpec {
        BoxSpec {
            id,
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(self.0.bundle_with(&Lab::act(act), self.1)),
            owner: None,
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
            owner: None,
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
    a_crash_with_idle_cgroups_and_live_boxes_leaves_nothing(&lab, own, exe);
    crash_at_each_point_of_a_create(&lab, own, exe);
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
        owner: None,
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
            owner: None,
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
            owner: None,
        })
        .unwrap();
    client.start(&oci).unwrap();
    client
        .create(BoxSpec {
            id: process.clone(),
            bounds: Bounds::default(),
            class: ClassSpec::Process,
            owner: None,
        })
        .unwrap();
    let init = procs(&agent.box_root.join(OCI_BOXES), &oci);
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
        procs(&agent.box_root.join(OCI_BOXES), &oci).len() == 2
            && procs(&agent.box_root, &process).len() == 1
    });
    drop(caller);
    for id in [&oci, &process] {
        wait_for(&format!("{id} to count its abandoned program"), || {
            client.inspect(id).unwrap().abandoned == 1
        });
    }
    wait_for("the programs to be ended", || {
        procs(&agent.box_root.join(OCI_BOXES), &oci) == init
            && procs(&agent.box_root, &process).is_empty()
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
/// durable: on tmpfs, as the node keeps it in its runtime directory, the records beside it and not
/// fsynced; or on disk (an `XDG_RUNTIME_DIR` on disk, as a rootless node with none keeps it in its
/// state directory), every record in the state directory and fsynced.
#[derive(Clone, Copy, Debug)]
enum Kept {
    Tmpfs,
    Disk,
}

/// Where in a create a crash case kills the agent. The box's record is written once, last (goal 5
/// extended step 5), so before it the agent leaves only parts at the paths the box's id fixes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum At {
    /// Its cgroup made, nothing else of it: the agent armed to kill itself there (a debug build's crash
    /// point), the box rendered from an `OciSpec`.
    #[cfg(debug_assertions)]
    CgroupMade,
    /// Its bundle rendered after its cgroup, its container's state directory not made: likewise.
    #[cfg(debug_assertions)]
    BundleRendered,
    /// Its cgroup and its container's state directory made: the create held reading its bundle's
    /// config.json, a FIFO no one has opened.
    StateDir,
    /// Its init cloned, waiting at the exec FIFO, and its container's state written: the create held in
    /// its createRuntime hook.
    Init,
    /// In the record write that marks it ready, a durable record's (so one written to a temp file and
    /// renamed): the box made, the write held opening its temp file, a FIFO no one has opened. A
    /// non-durable record's first write takes no temp ([`At::TornRecord`]).
    Ready,
    /// In the record write that marks it ready, a non-durable record's: the box made, the agent armed
    /// to kill itself between opening the record's file and writing it (a debug build's crash point),
    /// leaving a torn record. The first write to a non-durable record goes straight to its file, so no
    /// FIFO can hold it there; a release build can't arm the point, so it has no such case.
    #[cfg(debug_assertions)]
    TornRecord,
    /// After it: the create done.
    Made,
}

impl At {
    /// The crash point the agent is armed at to die at this point, as `hex node --crash-at` names it.
    fn armed(self) -> Option<&'static str> {
        match self {
            #[cfg(debug_assertions)]
            Self::CgroupMade => Some("cgroup-made"),
            #[cfg(debug_assertions)]
            Self::BundleRendered => Some("bundle-rendered"),
            #[cfg(debug_assertions)]
            Self::TornRecord => Some("record-first-write"),
            Self::StateDir | Self::Init | Self::Ready | Self::Made => None,
        }
    }
}

/// T-T7.1.2 (goal 5 plan step 2; goal 5 extended step 5): kill the node agent at each point of a
/// create, with its boxes' records kept on tmpfs and on disk, then restart it. In a debug build the
/// agent is armed to kill itself once the box's cgroup is made, once its bundle is rendered, and —
/// where its record is non-durable, written once straight to its file — between that file's open and
/// the write, leaving a torn record: no file it opens can hold it at any of these. Elsewhere it is held
/// at a point by a FIFO no one has opened, or by the box's createRuntime hook, and killed once the
/// kernel shows it waiting there: with the cgroup and the container's state directory made; with the
/// init cloned and the container's state written; inside a durable record's write, opening the temp
/// file it renames; and after the create. Restarted, the agent removes the box killed mid-create by
/// what it finds of it — a torn record it reads as unreadable, and says so — and takes back the one
/// made, which then deletes, so at every point nothing of the box is left: no record in either place,
/// no temp file, no cgroup, no container state, no bundle.
fn crash_at_each_point_of_a_create(lab: &Lab, own: &Path, exe: &str) {
    for kept in [Kept::Tmpfs, Kept::Disk] {
        let mut points = Vec::new();
        #[cfg(debug_assertions)]
        points.extend([At::CgroupMade, At::BundleRendered]);
        points.extend([At::StateDir, At::Init]);
        // The record write that marks a box ready is held by where its record is kept: a durable
        // record (on disk) is written to a temp file and renamed, so a FIFO holds the agent opening the
        // temp ([`At::Ready`]); a non-durable record (on tmpfs) is written once, straight to its file,
        // so only a debug build's crash point holds the agent there ([`At::TornRecord`]).
        match kept {
            Kept::Tmpfs => {
                #[cfg(debug_assertions)]
                points.push(At::TornRecord);
            }
            Kept::Disk => points.push(At::Ready),
        }
        points.push(At::Made);
        for at in &points {
            crash_at(lab, own, exe, kept, *at);
        }
    }
    #[cfg(debug_assertions)]
    a_box_cannot_arm_the_agents_crash_point(lab, own, exe);
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
    let cgroup = own.join(&name).join(OCI_BOXES).join(id.as_str());
    let rendered = state.join("bundles").join(id.as_str());
    let (hooked, go) = (dir.path().join("hooked"), dir.path().join("go"));
    // Keep a writer open until the case ends, including unwinding: an orphaned hook reads EOF
    // after an assertion failure. A polled release file can disappear with the TempDir before
    // the hook observes it, leaving the hook and this test's output pipe alive forever.
    let mut hook_release = None;
    let class = match at {
        #[cfg(debug_assertions)]
        At::CgroupMade | At::BundleRendered => ClassSpec::Oci(Box::new(lab.spec())),
        At::StateDir => {
            let bundle = dir.path().join("bundle");
            std::fs::create_dir(&bundle).unwrap();
            mkfifo(&bundle.join("config.json"));
            ClassSpec::Bundle(bundle)
        }
        At::Init | At::Ready => {
            mkfifo(&go);
            hook_release = Some(
                std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&go)
                    .unwrap(),
            );
            // The create waits in its createRuntime hook, its container made, until this test
            // writes a line, or closes the writer on an assertion failure.
            let script = crash_hook_script(&hooked, &go);
            let hooks = format!(
                r#",
  "hooks": {{"createRuntime": [{{"path": "/bin/sh", "args": ["sh", "-c", {script:?}], "env": ["PATH=/usr/bin:/bin"]}}]}}"#
            );
            ClassSpec::Bundle(lab.bundle_hooked(&Lab::act(&Act::Wait), false, &hooks))
        }
        // Armed to crash at the record write, so it creates fully, like a box made; a bundle box's
        // record names only its container's state (on tmpfs here), so it is non-durable.
        #[cfg(debug_assertions)]
        At::TornRecord => ClassSpec::Bundle(lab.bundle(&Lab::act(&Act::Wait))),
        At::Made => ClassSpec::Bundle(lab.bundle(&Lab::act(&Act::Wait))),
    };
    let spec = BoxSpec {
        id: id.clone(),
        bounds: Bounds::default(),
        class,
        owner: None,
    };
    #[cfg_attr(not(debug_assertions), allow(unused_mut))]
    let mut agent = Agent::armed(
        exe,
        dir.path(),
        own.join(&name),
        runtime.as_deref(),
        at.armed(),
    );
    if on_tmpfs(&oci_root) != matches!(kept, Kept::Tmpfs) {
        eprintln!("SKIP {name}: {} isn't where it's kept", oci_root.display());
        return;
    }
    // The record's place, now the agent has made its state directory: the state directory for a
    // durable one, else the node's own beside its containers' state.
    let records = match kept {
        Kept::Tmpfs => hex_node::records_dir(&oci_root, &state).unwrap(),
        Kept::Disk => state.clone(),
    };
    // A non-durable record is kept in its shard of the runtime record directory (goal 5 extended, step 6).
    let (record, temp) = match kept {
        Kept::Tmpfs => (
            hex_node::store::runtime_record(&records, &id),
            hex_node::store::runtime_temp(&records, &id),
        ),
        Kept::Disk => (
            records.join(format!("{id}.json")),
            records.join(format!(".{id}.tmp")),
        ),
    };
    let socket = agent.socket.clone();
    let create = std::thread::spawn(move || Client::connect(&socket).and_then(|c| c.create(spec)));
    match at {
        #[cfg(debug_assertions)]
        At::CgroupMade | At::BundleRendered | At::TornRecord => {
            use std::os::unix::process::ExitStatusExt;
            let ended = agent.ended();
            assert_eq!(
                ended.signal(),
                Some(9),
                "{name}: the agent killed itself at its crash point: {ended:?}"
            );
        }
        At::StateDir => {}
        At::Init => wait_for("the create's hook to hold it", || hooked.exists()),
        At::Ready => {
            wait_for("the create's hook to hold it", || hooked.exists());
            mkfifo(&temp);
            hook_release.as_mut().unwrap().write_all(b"\n").unwrap();
        }
        At::Made => wait_for("the create", || create.is_finished()),
    }
    if matches!(at, At::StateDir | At::Ready) {
        wait_for("the agent to wait opening a FIFO", || {
            waits_opening_a_fifo(agent.pid())
        });
    }
    // What the kill leaves, as the point says: a record only once the box is made, its container's
    // state directory from the runtime's create on, its state written from its init's clone on.
    let containers = || containers_of(&oci_root, &id);
    // A record counts as written only once it decodes whole: a crash between a non-durable record's
    // open and its write leaves a torn file, which reads as no record (At::TornRecord).
    let written = || {
        std::fs::read(&record)
            .ok()
            .and_then(|bytes| BoxRecord::decode(&bytes).ok())
            .map(|record| record.phase)
    };
    let made = if at == At::Made {
        Some(Phase::Ready)
    } else {
        None
    };
    assert_eq!(written(), made, "{name}: the record at the kill");
    // The container's state directory is made by every point but the two before it (the cgroup made,
    // the bundle rendered); the record write that marks the box ready comes after it is all made.
    let state_dir_made = match at {
        #[cfg(debug_assertions)]
        At::CgroupMade | At::BundleRendered => false,
        _ => true,
    };
    assert_eq!(
        (containers().len(), cgroup.exists()),
        (usize::from(state_dir_made), true),
        "{name}: (container states, the cgroup) at the kill"
    );
    #[cfg(debug_assertions)]
    assert_eq!(
        rendered.exists(),
        at == At::BundleRendered,
        "{name}: the bundle at the kill"
    );
    let state_written = containers()
        .first()
        .is_some_and(|c| c.join("state.json").exists());
    assert_eq!(
        state_written,
        state_dir_made && at != At::StateDir,
        "{name}: the container's state at the kill"
    );
    drop(agent);
    if at == At::Init {
        // The hook outlives the agent it ran under; let it end.
        hook_release.as_mut().unwrap().write_all(b"\n").unwrap();
    }
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
    // A torn first write leaves a record that can't be read; recovery removed its box and said so
    // (hex_core::recovery's Why::Unreadable). The reason is the point of the case.
    #[cfg(debug_assertions)]
    if at == At::TornRecord {
        wait_for("recovery to report the box removed as unreadable", || {
            agent
                .errors()
                .lines()
                .any(|l| l.contains(id.as_str()) && l.contains("couldn't be read"))
        });
    }
    assert!(client.list().unwrap().is_empty(), "{name}: a box is left");
    for place in [&state, &records] {
        assert!(
            !place.join(format!("{id}.json")).exists(),
            "{name}: its record is left in {}",
            place.display()
        );
    }
    assert!(!temp.exists(), "{name}: its record's temp file is left");
    assert!(
        containers().is_empty(),
        "{name}: its container's state is left"
    );
    assert!(!cgroup.exists(), "{name}: its cgroup is left");
    assert!(!rendered.exists(), "{name}: its bundle is left");
    eprintln!(
        "{name}: killed with the record {made:?}, its cgroup made, its container's state directory {}made and its state {}written; after the restart {}, nothing left",
        if state_dir_made { "" } else { "not " },
        if state_written { "" } else { "not " },
        if at == At::Made {
            "taken back, then deleted"
        } else {
            "cleaned up"
        }
    );
}

fn crash_hook_script(hooked: &Path, go: &Path) -> String {
    format!(
        "touch {} && read -r release < {}",
        hooked.display(),
        go.display()
    )
}

/// T-T7.1.2's fixture: a killed node's hook still waits for release, and ends even when its test
/// unwinds without releasing it. No release filename can disappear before a polling hook sees it.
#[test]
fn a_crash_hook_waits_for_a_line_and_ends_when_its_writer_closes() {
    use std::os::unix::fs::OpenOptionsExt;

    struct Hook(Child);
    impl Drop for Hook {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    for release in [true, false] {
        let dir = tempfile::tempdir_in(cache_dir()).unwrap();
        let go = dir.path().join("go");
        let ready = dir.path().join("ready");
        mkfifo(&go);
        let mut hook = Hook(
            Command::new("/bin/sh")
                .arg("-c")
                .arg(crash_hook_script(&ready, &go))
                .stdout(Std::null())
                .stderr(Std::null())
                .spawn()
                .unwrap(),
        );
        let mut writer = None;
        wait_for("the hook to open its release pipe", || {
            assert!(
                hook.0.try_wait().unwrap().is_none(),
                "the crash hook ended before its pipe was released"
            );
            match std::fs::OpenOptions::new()
                .write(true)
                .custom_flags(i32::try_from(rustix::fs::OFlags::NONBLOCK.bits()).unwrap())
                .open(&go)
            {
                Ok(f) => {
                    writer = Some(f);
                    true
                }
                // Opening a FIFO's writer without blocking succeeds only once a reader opened it.
                Err(e) if e.raw_os_error() == Some(rustix::io::Errno::NXIO.raw_os_error()) => false,
                Err(e) => panic!("opening the release pipe: {e}"),
            }
        });
        if release {
            writer.as_mut().unwrap().write_all(b"\n").unwrap();
        }
        drop(writer);
        wait_for("the hook to end after release or writer close", || {
            hook.0.try_wait().unwrap().is_some()
        });
    }
}

/// T-T7.1.2's crash point is the agent's alone to arm (`hex node --crash-at`, from its own arguments):
/// an agent started without it makes a box whose own spec names the point, in its init's environment as
/// a variable might carry it, whole, and lives on. Debug builds only, where the point exists to be proven
/// inert.
#[cfg(debug_assertions)]
fn a_box_cannot_arm_the_agents_crash_point(lab: &Lab, own: &Path, exe: &str) {
    let dir = tempfile::Builder::new()
        .prefix("crash-unarmed-")
        .tempdir_in(lab.bundles.path())
        .unwrap();
    let mut agent = Agent::over(exe, dir.path(), own.join("crash-unarmed"), None);
    let client = agent.client();
    let id = BoxId::new("names-the-crash-point").unwrap();
    let mut spec = lab.spec();
    for (name, point) in [
        ("HEX_CRASH_AT", "cgroup-made"),
        ("CRASH_AT", "bundle-rendered"),
    ] {
        spec.init.env.push((name.to_owned(), point.to_owned()));
    }
    let made = client
        .create(BoxSpec {
            id: id.clone(),
            bounds: Bounds::default(),
            class: ClassSpec::Oci(Box::new(spec)),
            owner: None,
        })
        .expect("a box naming the crash point is made");
    assert_eq!(made.state, BoxState::Created);
    assert!(agent.runs(), "the agent lives on");
    client.delete(&id).unwrap();
    eprintln!(
        "a box naming the crash point in its own spec: made whole, the unarmed agent living on"
    );
}

/// The state directories under `oci_root` of box `id`'s container, whatever node's: each is named
/// `<node key>-<box id>`, the key 16 hex digits.
/// The cgroups under the oci boxes' root `oci` that are slots ([`hex_core::reuse::slot_of`]).
fn slots_under(oci: &Path) -> std::collections::BTreeSet<String> {
    std::fs::read_dir(oci)
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .filter(|name| hex_core::reuse::slot_of(name).is_some())
        .collect()
}

/// Whether a process here holds the mount namespace `ns` (its `mnt:[inode]` link): as its own, or by a
/// descriptor. A process whose `/proc` entries this user can't read is passed over.
fn mount_namespace_held(ns: &str) -> bool {
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter(|p| {
            p.file_name()
                .to_str()
                .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
        })
        .any(|p| {
            let links = |dir: PathBuf| {
                std::fs::read_dir(dir)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter_map(|fd| std::fs::read_link(fd.path()).ok())
                    .collect::<Vec<_>>()
            };
            std::fs::read_link(p.path().join("ns/mnt")).is_ok_and(|own| own.to_string_lossy() == ns)
                || links(p.path().join("fd"))
                    .iter()
                    .any(|held| held.to_string_lossy() == ns)
        })
}

/// Goal 5 extended, step 6 (T-T7.1.2). Do: over the socket, make and start boxes A and B of owner O and a
/// box U of none, delete A (its slot goes idle), SIGKILL the agent, and start it again over the same
/// state. Expect: the restarted agent removes A's idle slot and says so; takes B back in its recorded
/// slot and U by its id, both running, B's usage read through its slot; and once both are deleted
/// nothing is left under the oci boxes' root, no box is listed, and no process holds any of the three
/// boxes' mount namespaces, which the agent held by descriptor when it was killed (the lead's condition
/// for the held mount namespaces, 2026-11-04T09:18Z).
fn a_crash_with_idle_cgroups_and_live_boxes_leaves_nothing(lab: &Lab, own: &Path, exe: &str) {
    let dir = tempfile::Builder::new()
        .prefix("agent-reuse-")
        .tempdir_in(lab.bundles.path())
        .unwrap();
    let box_root = own.join("hex-sock-reuse");
    let mut agent = Agent::over(exe, dir.path(), box_root.clone(), None);
    let client = agent.client();
    let o = BoxOwner::named("owner-crash").unwrap();
    let wait = Lab::act(&Act::Wait);
    let id = |name: &str| BoxId::new(name).unwrap();
    for (name, owner) in [
        ("crash-a", Some(&o)),
        ("crash-b", Some(&o)),
        ("crash-u", None),
    ] {
        client
            .create(BoxSpec {
                id: id(name),
                bounds: Bounds::default(),
                class: ClassSpec::Bundle(lab.bundle_owned(&wait)),
                owner: owner.cloned(),
            })
            .unwrap();
        client.start(&id(name)).unwrap();
    }
    let oci = box_root.join(OCI_BOXES);
    let slots = slots_under(&oci);
    assert_eq!(slots.len(), 2, "A and B each hold a slot: {slots:?}");
    let namespaces: Vec<String> = slots
        .iter()
        .map(String::as_str)
        .chain(["crash-u"])
        .flat_map(|cg| {
            std::fs::read_to_string(oci.join(cg).join("cgroup.procs"))
                .unwrap_or_default()
                .lines()
                .filter_map(|pid| std::fs::read_link(format!("/proc/{pid}/ns/mnt")).ok())
                .map(|ns| ns.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        namespaces.len() >= 3,
        "each box's init's mount namespace: {namespaces:?}"
    );
    assert!(
        oci.join("crash-u").is_dir(),
        "U's cgroup is its own, named by its id"
    );
    client.delete(&id("crash-a")).unwrap();
    assert_eq!(
        slots_under(&oci),
        slots,
        "A's slot is kept idle for its owner"
    );
    drop(client);
    agent.child.kill().unwrap();
    let ended = agent.child.wait().unwrap();
    assert_eq!(
        std::os::unix::process::ExitStatusExt::signal(&ended),
        Some(9)
    );
    drop(agent);
    let again = Agent::over(exe, dir.path(), box_root.clone(), None);
    wait_for(
        "the restarted agent to say it removed the idle slot",
        || again.errors().contains("removed idle cgroup"),
    );
    let left = slots_under(&oci);
    assert_eq!(
        left.len(),
        1,
        "A's idle slot is removed, B's kept: {left:?}"
    );
    let client = again.client();
    for name in ["crash-b", "crash-u"] {
        let info = client.inspect(&id(name)).unwrap();
        assert_eq!(
            info.state,
            BoxState::Running,
            "{name} is taken back running"
        );
    }
    client.usage(&id("crash-b")).unwrap();
    for name in ["crash-b", "crash-u"] {
        client.delete(&id(name)).unwrap();
    }
    let names: Vec<String> = std::fs::read_dir(&oci)
        .unwrap()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .collect();
    assert!(
        names.is_empty(),
        "nothing is left under the oci boxes' root: {names:?}"
    );
    assert!(client.list().unwrap().is_empty(), "no box is listed");
    wait_for("the boxes' mount namespaces gone", || {
        namespaces.iter().all(|ns| !mount_namespace_held(ns))
    });
    eprintln!(
        "T-T7.1.2, step 6: idle slot removed on restart ({}), B taken back in {left:?}, nothing left",
        again
            .errors()
            .lines()
            .filter(|l| l.contains("idle cgroup"))
            .collect::<Vec<_>>()
            .join("; ")
    );
}

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
