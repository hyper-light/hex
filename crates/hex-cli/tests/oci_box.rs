//! Decision 2's seam half (docs/design/runtime-architecture.md; AC-T7.1.1, T-T7.1.2): through
//! `hex_node::Node`, the node agent is each `oci` box's parent, holds its init's pidfd and records
//! how the init ended. "do X, expect Y" (§6):
//! - a crash: SIGKILL an agent while its box runs, restart it, end the init: the restarted agent
//!   reads the box running, then empty, and nothing of it is left;
//! - create a box: its init's parent is the agent;
//! - a box whose program exits 7: inspect reports `Code(7)` and the init's pid is gone (reaped);
//! - SIGKILL a box's init: inspect reports `Signal(9)`;
//! - past the init budget: create is refused with `Exhausted`, naming it, and leaves nothing;
//! - the hex-api conformance suite on the `oci` class, its exit-report cases included;
//! - decision 4's network namespaces (goal 4 plan step 4): a box whose bundle makes a network
//!   namespace of its own runs in one no other box has had, loopback up. HEX-NETNS-9: from a node whose
//!   pool holds none (bound 0) it is made on the create's path, counted so. HEX-NETNS-8, as root: two
//!   boxes take one each from the node's pool, and the node keeps no descriptor of either.
//!   HEX-NETNS-10, rootless: made on the path in the box's own user namespace, which owns it, nothing
//!   taken;
//! - a box's process orphaned to the agent, once ended, is reaped by the sweep: no zombie is kept;
//! - goal 5 plan step 2: 31 boxes are made at once while another box's durable record write is held
//!   open, none waiting on it, and none of their records synced with their container state on tmpfs;
//!   two nodes with state directories of their own each make and start a box of the same id, two
//!   containers, and deleting one leaves the other running; a box made before the container state
//!   moved, its record naming `<state>/oci/<id>`, is taken back, inspected and deleted by today's node.
//!
//! Rootless where the caller isn't root, in a delegated cgroup v2 subtree, as oci_seccomp.rs; where
//! neither is possible it prints SKIP and why rather than pass falsely. The crash runs first, before
//! this process opens any node: an agent's orphans go to its nearest subreaper, and this process
//! becomes one when it opens a node, so a restart in a process that never was the init's ancestor is
//! the real one's.
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
use std::process::{Command, Stdio as Std};
use std::time::{Duration, Instant};

use hex_api::conformance::{self, Act, Fixture};
use hex_api::{
    Bounds, BoxId, BoxInfo, BoxSpec, BoxState, ClassSpec, Exit, Network, OciSpec, Refusal, Rootfs,
    Runtime,
};
use hex_core::record::{BoxRecord, Phase, ProcId};
use hex_node::Node;
use hex_node::store::Store;
use hex_runtime::linux::cgroup::BoxRoot;
use hex_runtime::linux::disks;
use hex_runtime::linux::oci::run::Runtime as OciRuntime;

const TEST: &str = "oci_boxes_report_their_inits_exit_through_the_seam";
/// Set in the re-exec'd copy of this test that plays the agent the crash kills.
const AGENT_ENV: &str = "HEX_OCI_BOX_AGENT";

#[test]
fn oci_boxes_report_their_inits_exit_through_the_seam() {
    let exe = env!("CARGO_BIN_EXE_hex").to_owned();
    let cache = cache_dir();
    if let Some(dirs) = std::env::var_os(AGENT_ENV) {
        play_the_agent(Path::new(&dirs), &exe);
    }
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

/// Where one run keeps its bundles, records and box root.
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
            .prefix("ocibox-")
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

    /// A bundle whose init runs `args`, in a fresh directory.
    fn bundle(&self, args: &[String]) -> PathBuf {
        self.bundle_with(args, false)
    }

    /// A bundle whose init runs `args`, with a network namespace of its own when `network` is set,
    /// its sysfs mounted at `/sys` to read it by.
    fn bundle_with(&self, args: &[String], network: bool) -> PathBuf {
        let dir = tempfile::Builder::new()
            .prefix("bundle-")
            .tempdir_in(self.bundles.path())
            .expect("bundle dir")
            .keep();
        std::fs::write(dir.join("config.json"), self.config(args, network)).expect("config.json");
        dir
    }

    fn config(&self, args: &[String], network: bool) -> String {
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
        let mut sys = String::new();
        if network {
            namespaces.push_str(r#",{"type":"network"}"#);
            sys.push_str(
                r#",
    {"destination": "/sys", "type": "sysfs", "source": "sysfs", "options": ["nosuid","noexec","nodev","ro"]}"#,
            );
        }
        let args = json_args(args);
        format!(
            r#"{{
  "ociVersion": "1.3.0",
  "root": {{"path": "{rootfs}", "readonly": false}},
  "process": {{
    "cwd": "/",
    "args": {args},
    "env": ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"],
    "user": {{"uid": 0, "gid": 0}},
    "noNewPrivileges": true
  }},
  "mounts": [
    {{"destination": "/proc", "type": "proc", "source": "proc"}},
    {{"destination": "/dev", "type": "tmpfs", "source": "tmpfs", "options": ["nosuid","mode=755","size=65536k"]}},
    {{"destination": "/tmp", "type": "tmpfs", "source": "tmpfs", "options": ["nosuid","nodev"]}},
    {{"destination": "/usr", "type": "bind", "source": "/usr", "options": ["rbind","ro"]}},
    {{"destination": "/hex", "type": "bind", "source": "{exe}", "options": ["bind","ro"]}}{sys}
  ],
  "linux": {{"namespaces": [{namespaces}]{mappings}}}
}}"#,
            rootfs = self.rootfs.display(),
            exe = self.exe,
        )
    }

    /// A box whose init runs `args`.
    fn running(&self, id: BoxId, args: &[String]) -> BoxSpec {
        BoxSpec {
            id,
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(self.bundle(args)),
        }
    }

    /// A box whose init waits, in a network namespace of its own.
    fn networked(&self, id: BoxId) -> BoxSpec {
        BoxSpec {
            id,
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(self.bundle_with(&Self::act(&Act::Wait), true)),
        }
    }

    /// The `hex selftest-box` program for `act`, as the container sees the binary (`/hex`).
    fn act(act: &Act) -> Vec<String> {
        let mut args = vec!["/hex".to_owned(), "selftest-box".to_owned()];
        match act {
            Act::Say(words) => args.extend(["say".to_owned(), (*words).to_owned()]),
            Act::Fail(code) => args.extend(["fail".to_owned(), code.to_string()]),
            Act::Wait => args.push("wait".to_owned()),
            Act::Hog(bytes) => args.extend(["hog".to_owned(), bytes.to_string()]),
            Act::Spawn(n) => args.extend(["spawn".to_owned(), n.to_string()]),
            Act::Echo => args.push("echo".to_owned()),
        }
        args
    }

    fn node(&self, box_root: &Path, state: &Path) -> Node {
        Node::open(box_root, state, state)
            .expect("open a node")
            .with_oci_helper(PathBuf::from(&self.exe))
    }
}

impl Fixture for Lab {
    fn spec(&self, id: BoxId, bounds: Bounds) -> BoxSpec {
        BoxSpec {
            id,
            bounds,
            class: ClassSpec::Bundle(self.bundle(&Self::act(&Act::Wait))),
        }
    }

    fn init(&self, id: BoxId, act: &Act) -> BoxSpec {
        BoxSpec {
            id,
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(self.bundle(&Self::act(act))),
        }
    }

    fn program(&self, act: &Act) -> hex_api::Program {
        let args = Self::act(act);
        hex_api::Program::of(&args.iter().map(String::as_str).collect::<Vec<_>>())
    }
}

fn exercise(own: &Path, cache: &Path, exe: &str) {
    let lab = Lab::new(cache, exe);
    // First, while this process is no node's and so no subreaper; it also finds out whether a
    // container runs here at all (rootless needs unprivileged user namespaces).
    if let Err(e) = crash_and_restart(&lab, own) {
        eprintln!("SKIP: rootless OCI unavailable here ({e})");
        return;
    }
    parent_and_exit_code(&lab, own);
    killed_init(&lab, own);
    budget(&lab, own);
    conformance_on_oci(&lab, own);
    network_namespaces(&lab, own);
    program_alone(&lab, own);
    exec_reads_no_state(&lab, own);
    orphans_are_reaped(&lab, own);
    held_record_write(&lab, own);
    two_nodes_one_box_id(&lab, own);
    a_box_made_before_the_move_is_adopted(&lab, own);
}

/// The agent the crash kills: a node in this re-exec'd process makes and starts a box whose init
/// waits, says `ready <agent pid> <its container's state dir>` on stdout, and waits to be killed.
fn play_the_agent(dirs: &Path, exe: &str) -> ! {
    let lab_rootfs = dirs.join("rootfs");
    let box_root = PathBuf::from(std::fs::read_to_string(dirs.join("box_root")).unwrap());
    let state = dirs.join("state");
    let bundle = dirs.join("bundle");
    let node = Node::open(&box_root, &state, &state)
        .expect("agent: open a node")
        .with_oci_helper(PathBuf::from(exe));
    let id = BoxId::new("crash").unwrap();
    assert!(lab_rootfs.exists());
    let made = node.create(BoxSpec {
        id: id.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Bundle(bundle),
    });
    match made {
        // Waits to be killed either way, as the harness ends it.
        Err(e) => println!("unavailable {e}"),
        Ok(_) => {
            node.start(&id).expect("agent: start");
            println!(
                "ready {} {}",
                std::process::id(),
                node.container_dir(&id).display()
            );
        }
    }
    loop {
        std::thread::park();
    }
}

/// T-T7.1.2 crash injection: SIGKILL the agent while its box runs; a restarted agent, not the
/// init's parent, reads the box running while the init runs and empty once it has ended (an exit it
/// couldn't collect: `Unknown`), and nothing of the box is left. `Err` when no container can be
/// made here.
fn crash_and_restart(lab: &Lab, own: &Path) -> Result<(), String> {
    let dirs = tempfile::Builder::new()
        .prefix("crash-")
        .tempdir_in(lab.bundles.path())
        .unwrap();
    let box_root = own.join("hex-oci-crash");
    let state = dirs.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(
        dirs.path().join("box_root"),
        box_root.to_string_lossy().as_bytes(),
    )
    .unwrap();
    std::os::unix::fs::symlink(&lab.rootfs, dirs.path().join("rootfs")).unwrap();
    let bundle = dirs.path().join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    std::fs::write(
        bundle.join("config.json"),
        lab.config(&Lab::act(&Act::Wait), false),
    )
    .unwrap();

    let mut agent = Command::new(std::env::current_exe().unwrap())
        .args([TEST, "--exact", "--nocapture", "--test-threads=1"])
        .env(AGENT_ENV, dirs.path())
        .stdout(Std::piped())
        .spawn()
        .expect("start the agent");
    // libtest writes `test <name> ... ` before the test's own output, on the same line.
    let mut out = BufReader::new(agent.stdout.take().unwrap());
    let (agent_pid, container): (i32, PathBuf) = loop {
        let mut line = String::new();
        assert!(
            out.read_line(&mut line).unwrap() > 0,
            "the agent ended before it was ready"
        );
        if let Some((_, ready)) = line.split_once("ready ") {
            let (pid, dir) = ready.trim().split_once(' ').unwrap();
            break (pid.parse().unwrap(), PathBuf::from(dir));
        }
        if let Some((_, why)) = line.split_once("unavailable ") {
            agent.kill().unwrap();
            agent.wait().unwrap();
            return Err(why.trim().to_owned());
        }
    };
    let init = init_pid(&container);
    assert_eq!(parent_of(init), agent_pid, "the init is the agent's child");

    agent.kill().unwrap();
    agent.wait().unwrap();
    // Orphaned again: its new parent is the agent's nearest subreaper, not this process.
    wait_for(|| parent_of(init) != agent_pid, "the init to be reparented");

    let restarted = lab.node(&box_root, &state);
    let id = BoxId::new("crash").unwrap();
    let summary = restarted.recover().expect("recover");
    assert!(summary.adopted.contains(&id), "not adopted: {summary:?}");
    let running = restarted.inspect(&id).unwrap();
    assert_eq!(
        (running.state, running.exit),
        (BoxState::Running, None),
        "while its init runs"
    );

    kill(init, 9);
    let ended = wait_exit(&restarted, &id);
    assert_eq!(
        (ended.state, ended.exit),
        (BoxState::Empty, Some(Exit::Unknown)),
        "once its init has ended"
    );
    wait_for(
        || !Path::new(&format!("/proc/{init}")).exists(),
        "the ended init to be reaped by its parent",
    );
    restarted.delete(&id).expect("delete");
    assert!(!box_root.join("crash").exists(), "the box's cgroup is left");
    assert!(restarted.list().unwrap().is_empty(), "a box is left");
    Ok(())
}

/// Decision 2: once create returns, the init is the agent's child; its program's exit 7 is what
/// inspect reports, and the init is reaped (its pid gone).
fn parent_and_exit_code(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-exit"), state.path());
    let id = BoxId::new("exit7").unwrap();
    let info = node
        .create(lab.running(id.clone(), &strings(&["/bin/sh", "-c", "exit 7"])))
        .expect("create");
    assert_eq!((info.state, info.exit), (BoxState::Created, None));
    let init = init_pid(&node.container_dir(&id));
    assert_eq!(
        parent_of(init),
        i32::try_from(std::process::id()).unwrap(),
        "the init's parent is the agent"
    );
    node.start(&id).expect("start");
    let ended = wait_exit(&node, &id);
    assert_eq!(
        (ended.state, ended.exit),
        (BoxState::Empty, Some(Exit::Code(7)))
    );
    assert!(
        !Path::new(&format!("/proc/{init}")).exists(),
        "the ended init is reaped"
    );
    node.delete(&id).expect("delete");
}

/// SIGKILL a running box's init: inspect reports signal 9, and no zombie of it remains.
fn killed_init(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-kill"), state.path());
    let id = BoxId::new("killed").unwrap();
    node.create(lab.running(id.clone(), &Lab::act(&Act::Wait)))
        .expect("create");
    node.start(&id).expect("start");
    let init = init_pid(&node.container_dir(&id));
    kill(init, 9);
    let ended = wait_exit(&node, &id);
    assert_eq!(
        (ended.state, ended.exit),
        (BoxState::Empty, Some(Exit::Signal(9)))
    );
    assert!(
        !Path::new(&format!("/proc/{init}")).exists(),
        "no zombie of it remains"
    );
    node.delete(&id).expect("delete");
}

/// Past the init budget create is refused, typed and naming it, and makes nothing; a delete gives
/// the place back.
fn budget(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let box_root = own.join("hex-oci-budget");
    let node = lab
        .node(&box_root, state.path())
        .with_init_budget(1)
        .unwrap();
    let wait = Lab::act(&Act::Wait);
    let first = BoxId::new("first").unwrap();
    node.create(lab.running(first.clone(), &wait))
        .expect("the first create fits the budget");
    let second = BoxId::new("second").unwrap();
    let refused = node.create(lab.running(second.clone(), &wait));
    assert_eq!(
        refused,
        Err(Refusal::Exhausted {
            what: "container init pidfds",
            bound: 1
        })
    );
    assert_eq!(
        node.inspect(&second),
        Err(Refusal::NoSuchBox(second.clone()))
    );
    assert!(
        !box_root.join("second").exists(),
        "a refused create made a cgroup"
    );
    assert!(
        !node.container_dir(&second).exists(),
        "a refused create made state"
    );
    node.delete(&first).expect("delete");
    node.create(lab.running(second.clone(), &wait))
        .expect("a delete gives its place back");
    node.delete(&second).expect("delete");
}

/// AC-T7.1.1: the `oci` class through the node holds the runtime contract, HEX-RT-11 and 12 (the
/// init's exit) included.
fn conformance_on_oci(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-conf"), state.path());
    let failures = conformance::run(&node, lab);
    assert!(failures.is_empty(), "conformance failures: {failures:#?}");
}

/// Decision 4 (goal 4 plan step 4): each box whose bundle makes a network namespace of its own runs in
/// a namespace no other box has had, loopback up (its flags, read in the box from the namespace's
/// sysfs, are IFF_UP | IFF_LOOPBACK), and the node counts where the namespace came from. Namespace
/// inode numbers are compared while every box named holds its own, since the kernel reuses a freed
/// one's.
/// - HEX-NETNS-9: a node whose pool holds none (bound 0) makes a box's namespace on the create's path
///   and counts it so: made on the path 1, nothing made ahead or taken.
/// - HEX-NETNS-10, rootless, the same node: the namespace is made in the box's own user namespace,
///   which owns it, as the box's init mounting its sysfs shows (a sysfs mount takes `CAP_SYS_ADMIN` in
///   the user namespace that owns the mounter's network namespace, `kobj_ns_current_may_mount`).
/// - HEX-NETNS-8, as root: once a node's pool holds two, two boxes take one each, their namespaces
///   differ from each other's, from the first box's and from the host's, and the node keeps no
///   descriptor of either: the pool's copy is dropped once handed over.
fn network_namespaces(lab: &Lab, own: &Path) {
    let host = netns_of(std::process::id());
    let dry_state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let dry = lab
        .node(&own.join("hex-oci-netns-dry"), dry_state.path())
        .with_netns_bound(0)
        .unwrap();
    let first = BoxId::new("netns-dry").unwrap();
    dry.create(lab.networked(first.clone())).expect("create");
    dry.start(&first).expect("start");
    let made = netns_of(init_pid(&dry.container_dir(&first)));
    assert_ne!(made, host, "the box's network namespace is its own");
    assert_eq!(loopback_flags(&dry, &first), "0x9", "its loopback is up");
    let counts = dry.netns().unwrap();
    assert_eq!(
        (counts.made_ahead, counts.taken, counts.made_on_path),
        (0, 0, 1),
        "(made ahead, taken, made on the path)"
    );
    if lab.rootless {
        dry.delete(&first).expect("delete");
        return;
    }

    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-netns"), state.path());
    wait_for(
        || {
            let c = node.netns().unwrap();
            c.made_ahead >= c.taken + c.discarded + 2
        },
        "the node's pool to hold two namespaces",
    );
    let mut seen = vec![host, made];
    let mut taken = Vec::new();
    for name in ["netns-a", "netns-b"] {
        let id = BoxId::new(name).unwrap();
        node.create(lab.networked(id.clone())).expect("create");
        node.start(&id).expect("start");
        let ns = netns_of(init_pid(&node.container_dir(&id)));
        assert!(!seen.contains(&ns), "{name}'s {ns} is no other's: {seen:?}");
        assert!(
            !held_by_this_process(&ns),
            "the node keeps no descriptor of {name}'s {ns}"
        );
        assert_eq!(loopback_flags(&node, &id), "0x9", "{name}'s loopback is up");
        seen.push(ns);
        taken.push(id);
    }
    let counts = node.netns().unwrap();
    assert_eq!(
        (counts.taken, counts.made_on_path),
        (2, 0),
        "(taken, made on the path)"
    );
    for id in &taken {
        node.delete(id).expect("delete");
    }
    dry.delete(&first).expect("delete");
}

/// The network namespace process `pid` is in, as its `/proc` link names it (`net:[<inode>]`).
fn netns_of(pid: impl std::fmt::Display) -> String {
    std::fs::read_link(format!("/proc/{pid}/ns/net"))
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

/// Whether this process, the node agent, holds a descriptor of network namespace `ns`.
fn held_by_this_process(ns: &str) -> bool {
    std::fs::read_dir("/proc/self/fd").unwrap().any(|fd| {
        std::fs::read_link(fd.unwrap().path()).is_ok_and(|link| link.to_string_lossy() == ns)
    })
}

/// Loopback's flags in box `id`'s network namespace, read in the box.
fn loopback_flags(rt: &dyn Runtime, id: &BoxId) -> String {
    let (mut out, w) = std::io::pipe().unwrap();
    let mut proc = rt
        .exec(
            id,
            hex_api::Program::of(&["/usr/bin/cat", "/sys/class/net/lo/flags"]),
            hex_api::Stdio {
                stdout: Some(w),
                ..hex_api::Stdio::null()
            },
        )
        .expect("exec cat");
    let mut flags = String::new();
    std::io::Read::read_to_string(&mut out, &mut flags).unwrap();
    assert_eq!(
        proc.wait_for(conformance::PATIENCE).unwrap(),
        Some(Exit::Code(0))
    );
    flags.trim().to_owned()
}

/// Goal 3 plan step 2: no process is made per exec but the program. Exec a non-hex `sleep` into a
/// running box through the seam, and find the exec'd program is the agent's own direct child — no
/// `hex __oci-exec` helper stands beside it, as one did before the seed (agent → helper → program; now
/// agent → program) — and that a second exec spawns no new `hex __oci-seed`: the seed is reused, so no
/// `hex` binary is exec'd per call.
fn program_alone(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-alone"), state.path());
    let id = BoxId::new("alone").unwrap();
    // A non-hex, long-lived init and program, so "no hex process beside it" is plain to see.
    node.create(lab.running(id.clone(), &["/usr/bin/sleep".to_owned(), "600".to_owned()]))
        .expect("create");
    node.start(&id).expect("start");
    let agent = std::process::id();

    let mut first = node
        .exec(
            &id,
            hex_api::Program::of(&["/usr/bin/sleep", "300"]),
            hex_api::Stdio::null(),
        )
        .expect("exec a program");
    let pid = first.pid();
    assert_eq!(
        first.try_wait().expect("try_wait"),
        None,
        "the program is still running while we look at it"
    );
    // It is the agent's own child: no `hex __oci-exec` helper stands between the agent and the program.
    // Cloned straight from the agent's thread (as root) it is born so; through a rootless node's seed,
    // the seed reaps its intermediate before the agent is told, so it has reparented by the time `exec`
    // returns.
    assert_eq!(
        ppid(pid),
        Some(agent),
        "the exec'd program's parent is the node agent itself"
    );
    let children = children_of(agent);
    assert!(
        !children.iter().any(|&c| cmdline_has(c, "__oci-exec")),
        "no hex __oci-exec helper runs beside the program"
    );
    // Goal 3 plan step 2's acceptance: no process is made per exec but the program, and no hex binary is
    // exec'd per call. A node that runs as root clones the program from its own thread and keeps no
    // seed; a rootless one hands the exec to a reusable seed. Either way a second exec adds to the agent's
    // children its program alone.
    let seeds = |cs: &[u32]| cs.iter().filter(|&&c| cmdline_has(c, "__oci-seed")).count();
    if lab.rootless {
        assert!(
            seeds(&children) >= 1,
            "a rootless node's exec is served by a reusable seed"
        );
    } else {
        assert_eq!(
            seeds(&children),
            0,
            "a node that runs as root needs no seed"
        );
    }
    let before: std::collections::BTreeSet<u32> = children.into_iter().collect();
    let second = node
        .exec(
            &id,
            hex_api::Program::of(&["/usr/bin/sleep", "300"]),
            hex_api::Stdio::null(),
        )
        .expect("a second exec");
    let after: std::collections::BTreeSet<u32> = children_of(agent).into_iter().collect();
    assert_eq!(
        after.difference(&before).copied().collect::<Vec<_>>(),
        [second.pid()],
        "the second exec added no process but its program: no helper, no seed, no hex binary exec'd"
    );
    assert_eq!(
        ppid(second.pid()),
        Some(agent),
        "the second program is the agent's child too"
    );

    node.delete(&id).expect("delete ends the programs");
    // Each program was a member of the box cgroup, so delete's cgroup.kill reached it: reap it (the
    // caller's own Proc, as HEX-RT-7 does) and find it ended by a signal, no zombie left.
    for mut p in [first, second] {
        let exit = p
            .wait_for(Duration::from_secs(5))
            .expect("wait for a deleted program")
            .expect("the program ended with the box");
        assert!(
            matches!(exit, Exit::Signal(_)),
            "delete ended the program by a signal: {exit:?}"
        );
    }
}

/// Goal 3 plan step 6: an exec into a running box reads nothing of the box's from disk; what it needs
/// of the container is kept with the init the agent watches. Move the container's state file away, exec
/// `true` into the box, and find it ran and exited 0; with the file back, the box deletes as usual.
fn exec_reads_no_state(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-nostate"), state.path());
    let id = BoxId::new("nostate").unwrap();
    node.create(lab.running(id.clone(), &Lab::act(&Act::Wait)))
        .expect("create");
    node.start(&id).expect("start");
    let file = node.container_dir(&id).join("state.json");
    let aside = node.container_dir(&id).join("state.json.aside");
    std::fs::rename(&file, &aside).expect("move the container's state away");
    let ran = node
        .exec(
            &id,
            hex_api::Program::of(&["/usr/bin/true"]),
            hex_api::Stdio::null(),
        )
        .and_then(|mut p| p.wait_for(Duration::from_secs(5)));
    std::fs::rename(&aside, &file).expect("put the container's state back");
    assert_eq!(
        ran,
        Ok(Some(Exit::Code(0))),
        "exec ran its program with the container's state file gone"
    );
    node.delete(&id).expect("delete");
}

/// `/proc/<pid>/stat` field 4, the parent pid, read past the parenthesized command name (which may
/// itself hold spaces and parentheses).
fn ppid(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, rest) = stat.rsplit_once(')')?;
    let mut fields = rest.split_whitespace();
    let _state = fields.next()?;
    fields.next()?.parse().ok()
}

/// Whether any argument in `/proc/<pid>/cmdline` (NUL-separated) is exactly `needle`.
fn cmdline_has(pid: u32, needle: &str) -> bool {
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .map(|b| b.split(|&c| c == 0).any(|a| a == needle.as_bytes()))
        .unwrap_or(false)
}

/// The processes whose parent is `agent`, by scanning `/proc`.
fn children_of(agent: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return out;
    };
    for entry in entries.flatten() {
        if let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>()
            && ppid(pid) == Some(agent)
        {
            out.push(pid);
        }
    }
    out
}

/// A box's program forks a child and exits, orphaning the child to the agent (its subreaper); the
/// child waits on a FIFO, so it ends when the test says. Once it has ended the sweep reaps it within
/// a [`hex_node::supervise::SWEEP_PERIOD`]: its pid goes, rather than stay a zombie of the agent.
fn orphans_are_reaped(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-orphan"), state.path());
    let fifo = state.path().join("go");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let id = BoxId::new("orphaner").unwrap();
    node.create(BoxSpec {
        id: id.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Process,
    })
    .unwrap();
    let (mut out, w) = std::io::pipe().unwrap();
    // stdout and stderr go before the FIFO opens (which blocks), so the orphan never holds the pipe
    // the test reads to its end.
    let script = format!("read x >/dev/null 2>&1 < {} & echo $!", fifo.display());
    let mut proc = node
        .exec(
            &id,
            hex_api::Program::of(&["/bin/sh", "-c", &script]),
            hex_api::Stdio {
                stdout: Some(w),
                ..hex_api::Stdio::null()
            },
        )
        .unwrap();
    assert_eq!(
        proc.wait_for(conformance::PATIENCE).unwrap(),
        Some(Exit::Code(0))
    );
    let mut text = String::new();
    std::io::Read::read_to_string(&mut out, &mut text).unwrap();
    let orphan: i32 = text.trim().parse().unwrap();
    assert_eq!(
        parent_of(orphan),
        i32::try_from(std::process::id()).unwrap(),
        "the orphan is the agent's"
    );
    std::fs::write(&fifo, b"go\n").unwrap();
    wait_for(
        || !Path::new(&format!("/proc/{orphan}")).exists(),
        "the ended orphan to be reaped",
    );
    node.delete(&id).unwrap();
}

/// Goal 5 plan step 2: no create waits on another box's record write, its fsyncs included. 32 boxes
/// are created at once from 32 threads; then 31 more while a box with a durable record (an `OciSpec`
/// box, its rendered bundle on disk) is held inside its first record write: its temp file is a FIFO no
/// one has opened, so the write can't go on until this test reads it. All 31 are made while it's held,
/// and with the node's container state on tmpfs none of their records is synced. Let go, the held
/// write fails at its fsync (a FIFO can't be synced), which shows it was a durable one. Both runs'
/// latencies are printed: the 31 beside the held write against the 32 with none.
fn held_record_write(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-held"), state.path());
    let wait = Lab::act(&Act::Wait);
    let specs = |prefix: &str, n: usize| -> Vec<BoxSpec> {
        (0..n)
            .map(|i| lab.running(BoxId::new(&format!("{prefix}{i}")).unwrap(), &wait))
            .collect()
    };
    // Each create timed on a thread of its own, all started at once; their latencies in the order they
    // came, a create that didn't come within the patience missing.
    let create_all = |specs: Vec<BoxSpec>| -> Vec<Duration> {
        let n = specs.len();
        let (done, made) = std::sync::mpsc::channel();
        std::thread::scope(|s| {
            for spec in specs {
                let (node, done) = (&node, done.clone());
                s.spawn(move || {
                    let t = Instant::now();
                    let made = node.create(spec).map(|_| t.elapsed());
                    let _ = done.send(made);
                });
            }
            (0..n)
                .map_while(|_| made.recv_timeout(conformance::PATIENCE).ok())
                .map(|m| m.expect("create"))
                .collect()
        })
    };

    let free = specs("free", 32);
    let ids: Vec<BoxId> = free.iter().map(|s| s.id.clone()).collect();
    let alone = create_all(free);
    assert_eq!(alone.len(), 32, "32 creates at once, none held");
    for id in &ids {
        node.delete(id).unwrap();
    }

    let fifo = state.path().join(".held.tmp");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let held = BoxId::new("held").unwrap();
    let held_spec = BoxSpec {
        id: held.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Oci(Box::new(OciSpec {
            rootfs: Rootfs::Dir(lab.rootfs.clone()),
            init: hex_api::Program::of(&["/hex", "selftest-box", "wait"]),
            mounts: Vec::new(),
            hostname: None,
            network: Network::Host,
            hosts: Vec::new(),
        })),
    };
    let busy = specs("busy", 31);
    let ids: Vec<BoxId> = busy.iter().map(|s| s.id.clone()).collect();
    let synced = node.record_syncs();
    // Nothing here panics before the held write is let go, so a create that waited on it ends too and
    // the scope's joins return.
    let (beside, synced_beside, held_made) = std::thread::scope(|s| {
        let holder = s.spawn(|| node.create(held_spec));
        wait_for(
            blocked_opening_a_fifo,
            "the held create to block in its record write",
        );
        let (done, made) = std::sync::mpsc::channel();
        for spec in busy {
            let (node, done) = (&node, done.clone());
            s.spawn(move || {
                let t = Instant::now();
                let made = node.create(spec).map(|_| t.elapsed());
                let _ = done.send(made);
            });
        }
        let beside: Vec<Result<Duration, Refusal>> = (0..31)
            .map_while(|_| made.recv_timeout(conformance::PATIENCE).ok())
            .collect();
        let synced_beside = node.record_syncs();
        // Let the held write go on: its open completes, and it writes the record into the FIFO.
        let mut held_bytes = Vec::new();
        let read = std::fs::File::open(&fifo).and_then(|mut f| f.read_to_end(&mut held_bytes));
        (beside, synced_beside, (read, holder.join().unwrap()))
    });
    let (read, held_made) = held_made;
    read.unwrap();
    std::fs::remove_file(&fifo).unwrap();
    assert_eq!(
        beside.len(),
        31,
        "every create beside the held write was made while it was held"
    );
    let beside: Vec<Duration> = beside.into_iter().map(|m| m.expect("create")).collect();
    assert!(
        matches!(
            &held_made,
            Err(Refusal::Os {
                op: "flushing a record",
                ..
            })
        ),
        "the held write, let go, failed at its fsync: {held_made:?}"
    );
    if on_tmpfs(node.container_dir(&held).parent().unwrap()) {
        assert_eq!(
            synced_beside, synced,
            "the 31 boxes' records, their container state on tmpfs alone, were not synced"
        );
    } else {
        eprintln!(
            "held_record_write: the node's container state isn't on tmpfs here, so its boxes' records are synced too"
        );
    }
    let ms = |d: &Duration| d.as_secs_f64() * 1e3;
    let (alone, beside) = (sorted_ms(&alone), sorted_ms(&beside));
    eprintln!(
        "held_record_write: create p50 {:.3} ms, max {:.3} ms with 32 at once; p50 {:.3} ms, max {:.3} ms with 31 beside a held durable write",
        ms(&alone[alone.len() / 2]),
        ms(&alone[alone.len() - 1]),
        ms(&beside[beside.len() / 2]),
        ms(&beside[beside.len() - 1]),
    );
    for id in &ids {
        node.delete(id).unwrap();
    }
    assert_eq!(node.inspect(&held), Err(Refusal::NoSuchBox(held.clone())));
    assert!(node.list().unwrap().is_empty(), "a box is left");
}

/// Goal 5 plan step 2: two nodes on one host, each with a state directory of its own, keep their
/// containers' state in the one root under the runtime directory; a box of the same id in each gets a
/// container of its own, named for its node. Both are made and start, each its own init; deleting one
/// leaves the other running, its state where it was.
fn two_nodes_one_box_id(lab: &Lab, own: &Path) {
    let (state_a, state_b) = (
        tempfile::tempdir_in(lab.bundles.path()).unwrap(),
        tempfile::tempdir_in(lab.bundles.path()).unwrap(),
    );
    let a = lab.node(&own.join("hex-oci-two-a"), state_a.path());
    let b = lab.node(&own.join("hex-oci-two-b"), state_b.path());
    let id = BoxId::new("same").unwrap();
    let wait = Lab::act(&Act::Wait);
    a.create(lab.running(id.clone(), &wait))
        .expect("create in a");
    b.create(lab.running(id.clone(), &wait))
        .expect("create in b");
    let (in_a, in_b) = (a.container_dir(&id), b.container_dir(&id));
    assert_ne!(in_a.file_name(), in_b.file_name(), "two containers' names");
    if in_a.parent() != in_b.parent() {
        eprintln!(
            "two_nodes_one_box_id: no runtime directory here, so each node keeps its containers in its state directory"
        );
    }
    for node in [&a, &b] {
        node.start(&id).expect("start");
        assert_eq!(node.inspect(&id).unwrap().state, BoxState::Running);
    }
    assert_ne!(init_pid(&in_a), init_pid(&in_b), "two inits");
    a.delete(&id).expect("delete in a");
    assert!(!in_a.exists(), "a's container state is left");
    assert_eq!(
        b.inspect(&id).unwrap().state,
        BoxState::Running,
        "b's box runs on"
    );
    assert!(
        in_b.join("state.json").exists(),
        "b's container state is kept"
    );
    b.delete(&id).expect("delete in b");
    assert!(!in_b.exists(), "b's container state is left");
}

/// T-T7.1.2 across goal 5 plan step 2's upgrade: a node before cff3699 kept a box's container state in
/// its state directory, `<state>/oci/<id>`, and the box's record names that directory. A box made so,
/// its init waiting, is taken back by a node of today restarted on the same state directory, which reads
/// its state where the record names it (created, not empty with an exit no one saw) and deletes it,
/// leaving nothing of it.
fn a_box_made_before_the_move_is_adopted(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let box_root = own.join("hex-oci-legacy");
    let id = BoxId::new("legacy").unwrap();
    let bundle = lab.bundle(&Lab::act(&Act::Wait));
    // The box as the node before cff3699 made it: its cgroup under the box root with its bounds, its
    // container under `<state>/oci` named by its id, and its record naming that directory.
    let root = BoxRoot::open(&box_root, disks::under(state.path()).unwrap()).unwrap();
    let cgroup = root.create(&id, &Bounds::default()).unwrap();
    let _cleanup = Killing(&root, id.clone());
    let before = state.path().join("oci");
    let oci = OciRuntime::with_helper(before.clone(), PathBuf::from(&lab.exe));
    oci.create_in_cgroup(id.as_str(), &bundle, None, cgroup, None)
        .unwrap();
    let made = oci.state(id.as_str()).unwrap();
    let mut record = BoxRecord::new(
        BoxSpec {
            id: id.clone(),
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(bundle),
        },
        Phase::Ready,
        vec![before.join(id.as_str())],
    );
    record.init = Some(ProcId {
        pid: u32::try_from(made.pid).unwrap(),
        start_ticks: made.start_time.unwrap(),
    });
    Store::open(state.path()).unwrap().put(&record).unwrap();

    let node = lab.node(&box_root, state.path());
    assert_ne!(
        node.container_dir(&id),
        before.join(id.as_str()),
        "today's node keeps its containers elsewhere"
    );
    let summary = node.recover().expect("recover");
    assert!(summary.adopted.contains(&id), "not adopted: {summary:?}");
    let info = node.inspect(&id).unwrap();
    assert_eq!(
        (info.state, info.exit),
        (BoxState::Created, None),
        "its state, read where its record names it"
    );
    node.delete(&id).expect("delete");
    assert!(
        !before.join(id.as_str()).exists(),
        "its container state is left"
    );
    assert!(!box_root.join(id.as_str()).exists(), "its cgroup is left");
    assert_eq!(node.inspect(&id), Err(Refusal::NoSuchBox(id.clone())));
}

/// Kills whatever is left in a box's cgroup when dropped, so a check that fails leaves no container
/// running, holding this test's output open.
struct Killing<'a>(&'a BoxRoot, BoxId);

impl Drop for Killing<'_> {
    fn drop(&mut self) {
        if let Some(cg) = self.0.get(&self.1) {
            let _ = cg.kill();
        }
    }
}

fn sorted_ms(latencies: &[Duration]) -> Vec<Duration> {
    let mut sorted = latencies.to_vec();
    sorted.sort_unstable();
    sorted
}

/// Whether a thread of this process is blocked opening a FIFO no one has opened the other end of: the
/// kernel shows it waiting in `wait_for_partner` (fs/pipe.c), or in `fifo_open` where that's inlined.
fn blocked_opening_a_fifo() -> bool {
    std::fs::read_dir("/proc/self/task")
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|task| std::fs::read_to_string(task.path().join("wchan")).ok())
        .any(|wchan| matches!(wchan.trim(), "wait_for_partner" | "fifo_open"))
}

/// Whether `dir` lies on tmpfs (include/uapi/linux/magic.h, `TMPFS_MAGIC`).
fn on_tmpfs(dir: &Path) -> bool {
    rustix::fs::statfs(dir).is_ok_and(|fs| i128::from(fs.f_type) == 0x0102_1994)
}

/// Polls inspect until the box reports an exit: the fact the test needs, bounded so a runtime that
/// never reports one fails rather than hangs.
fn wait_exit(rt: &dyn Runtime, id: &BoxId) -> BoxInfo {
    let mut last = None;
    wait_for(
        || {
            let info = rt.inspect(id).unwrap();
            let done = info.exit.is_some();
            last = Some(info);
            done
        },
        "the box to report its init's exit",
    );
    last.unwrap()
}

fn wait_for(mut cond: impl FnMut() -> bool, what: &str) {
    let deadline = Instant::now() + conformance::PATIENCE;
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

/// The init's pid, from the state (`"pid":<n>`) in its container's state dir.
fn init_pid(container: &Path) -> i32 {
    let text = std::fs::read_to_string(container.join("state.json")).unwrap();
    let (_, after) = text.split_once("\"pid\":").unwrap();
    after
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| (*a).to_owned()).collect()
}

/// Field 4 of `/proc/<pid>/stat`; 0 once it's gone.
fn parent_of(pid: i32) -> i32 {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| {
            s.rsplit_once(')')
                .and_then(|(_, rest)| rest.split_whitespace().nth(1).map(str::to_owned))
        })
        .and_then(|p| p.parse().ok())
        .unwrap_or(0)
}

fn kill(pid: i32, sig: i32) {
    let status = Command::new("kill")
        .args([format!("-{sig}"), pid.to_string()])
        .status()
        .unwrap();
    assert!(status.success(), "kill -{sig} {pid}");
}

/// `args` as a JSON array: Debug quoting is JSON's for the plain ASCII the tests use.
fn json_args(args: &[String]) -> String {
    assert!(args.iter().all(|a| a.is_ascii() && !a.contains('\\')));
    format!("{args:?}")
}

/// A rootfs of the host's `/usr`, bound in by each bundle, with the host's top-level links into it
/// (`/bin`, `/lib64`, …) so the loader and `/bin/sh` resolve as on the host.
fn build_rootfs(rootfs: &Path) {
    for d in ["proc", "dev", "sys", "tmp", "usr"] {
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

/// This process's cgroup directory under /sys/fs/cgroup.
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
/// oci_seccomp.rs does; returns the own cgroup dir, or why it isn't ours to subdivide.
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
    let cache = PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".cache/hex-t7/ocibox");
    std::fs::create_dir_all(&cache).expect("cache dir");
    cache
}
