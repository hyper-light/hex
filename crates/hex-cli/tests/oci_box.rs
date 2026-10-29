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
//! - a box's process orphaned to the agent, once ended, is reaped by the sweep: no zombie is kept.
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

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio as Std};
use std::time::{Duration, Instant};

use hex_api::conformance::{self, Act, Fixture};
use hex_api::{Bounds, BoxId, BoxInfo, BoxSpec, BoxState, ClassSpec, Exit, Refusal, Runtime};
use hex_node::Node;

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
        let dir = tempfile::Builder::new()
            .prefix("bundle-")
            .tempdir_in(self.bundles.path())
            .expect("bundle dir")
            .keep();
        std::fs::write(dir.join("config.json"), self.config(args)).expect("config.json");
        dir
    }

    fn config(&self, args: &[String]) -> String {
        let (namespaces, mappings) = if self.rootless {
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
    {{"destination": "/hex", "type": "bind", "source": "{exe}", "options": ["bind","ro"]}}
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
    program_alone(&lab, own);
    exec_reads_no_state(&lab, own);
    orphans_are_reaped(&lab, own);
}

/// The agent the crash kills: a node in this re-exec'd process makes and starts a box whose init
/// waits, says `ready <agent pid>` on stdout, and waits to be killed.
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
            println!("ready {}", std::process::id());
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
        lab.config(&Lab::act(&Act::Wait)),
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
    let agent_pid: i32 = loop {
        let mut line = String::new();
        assert!(
            out.read_line(&mut line).unwrap() > 0,
            "the agent ended before it was ready"
        );
        if let Some((_, pid)) = line.split_once("ready ") {
            break pid.trim().parse().unwrap();
        }
        if let Some((_, why)) = line.split_once("unavailable ") {
            agent.kill().unwrap();
            agent.wait().unwrap();
            return Err(why.trim().to_owned());
        }
    };
    let init = init_pid(&state, "crash");
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
    let init = init_pid(state.path(), "exit7");
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
    let init = init_pid(state.path(), "killed");
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
        !state.path().join("oci/second").exists(),
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
    let file = state.path().join("oci/nostate/state.json");
    let aside = state.path().join("state.json.aside");
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

/// The init's pid, from the container's state (`"pid":<n>`).
fn init_pid(state: &Path, id: &str) -> i32 {
    let text = std::fs::read_to_string(state.join("oci").join(id).join("state.json")).unwrap();
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
