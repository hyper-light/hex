//! Goal 5 extended, step 6 (decision 5, "The cgroup lock"; docs/research/runtime/goal5-cgroup-lock.md,
//! f19-f50, f61-f65, f81, f82): fewer holds of the kernel's host-wide `cgroup_mutex` per box, through the
//! `Runtime` seam (`hex_node::Node`). "do X, expect Y" (CLAUDE.md §6), each case naming the plan step's
//! test it serves:
//! - test 1, how a delete ends a box: an `oci` box whose init made a pid namespace of its own, a program
//!   exec'd in it, is deleted: expect it ended through the init's pidfd, with no write to its
//!   `cgroup.kill` (an inotify watch on the file sees none), its cgroup gone and the program reaped, its
//!   end read as SIGKILL. A box with a process from outside its namespace moved into its cgroup: expect
//!   the delete to fall back to `cgroup.kill` (the watch sees the write), counted, and the outsider
//!   killed. A `process` box: expect `cgroup.kill` (the watch sees it), counted;
//! - test 2, a cgroup reused within one owner: box A of owner O burns CPU, holds 64 MiB, has a program
//!   OOM-killed and forks refused, and is deleted; box B of owner O is created: expect B in A's slot, B's
//!   reported CPU under 1% of A's, its peak under A's and its event counts its own (0) while the cgroup's
//!   own files still hold A's, and every bound B sets in force, every one it doesn't at its default;
//! - test 3, each guard: after A of owner O is deleted, a sub-cgroup made in its cgroup, its
//!   `memory.oom.group` set, a live process moved in, or 64 MiB left charged by A: expect B of O a fresh
//!   cgroup, A's removed and the refusal counted by its reason. A killed through `cgroup.kill` (a signal
//!   KILL to the box) and deleted: expect its cgroup removed, not kept, counted, and B of O a fresh one
//!   that runs: Linux 6.19 kills every task later cloned into a cgroup `cgroup.kill` was written to
//!   (its `kill_seq`, kernel/cgroup/cgroup.c:6702-6705 and :6941). B of another owner: never A's
//!   cgroup, a fresh one, counted; whether A's is then removed or kept for O is Ada's call (the lead,
//!   2026-11-02T18:23Z), so this asserts neither. A of no owner, holding CAP_SYS_ADMIN, or whose spec
//!   gives no capability sets (it keeps its creator's, all of them): a cgroup of its own, removed with
//!   it, counted; B holding CAP_SYS_ADMIN beside its owner's idle cgroup: a cgroup of its own, counted,
//!   the idle one left.
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

use std::io::Read;
use std::mem::MaybeUninit;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use hex_api::conformance::{Act, PATIENCE};
use hex_api::{Bounds, BoxId, BoxOwner, BoxSpec, ClassSpec, Exit, Program, Runtime, Stdio};
use hex_core::reuse::Refused;
use hex_node::Node;
use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::fs::inotify;
use rustix::process::{Pid, PidfdFlags};

const TEST: &str = "a_boxs_cgroup_is_ended_without_the_lock_and_reused_within_its_owner";

/// Docker's default capability set (moby's `oci/caps/defaults.go`, as crates/hex-core/src/bundle.rs
/// renders it): no CAP_SYS_ADMIN, so a box holding these can't change its cgroup.
const DOCKER_CAPS: &str = r#"["CAP_CHOWN","CAP_DAC_OVERRIDE","CAP_FSETID","CAP_FOWNER","CAP_MKNOD","CAP_NET_RAW","CAP_SETGID","CAP_SETUID","CAP_SETFCAP","CAP_SETPCAP","CAP_NET_BIND_SERVICE","CAP_SYS_CHROOT","CAP_KILL","CAP_AUDIT_WRITE"]"#;

#[test]
fn a_boxs_cgroup_is_ended_without_the_lock_and_reused_within_its_owner() {
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

/// What a box's process may hold.
#[derive(Clone, Copy)]
enum Caps {
    /// Docker's defaults, given in every set.
    Docker,
    /// Docker's defaults and CAP_SYS_ADMIN.
    SysAdmin,
    /// No capability sets given, so the process keeps its creator's: all of them.
    Unset,
}

/// Where one run keeps its bundles and box roots.
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
            .prefix("ocireuse-")
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

    /// A bundle whose init waits, its process holding `caps`.
    fn bundle(&self, caps: Caps) -> PathBuf {
        self.bundle_running(caps, &["/hex", "selftest-box", "wait"])
    }

    /// A bundle whose init runs `args`, its process holding `caps`.
    fn bundle_running(&self, caps: Caps, args: &[&str]) -> PathBuf {
        let dir = tempfile::Builder::new()
            .prefix("bundle-")
            .tempdir_in(self.bundles.path())
            .expect("bundle dir")
            .keep();
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
        let capabilities = match caps {
            Caps::Docker => Some(DOCKER_CAPS.to_owned()),
            Caps::SysAdmin => Some(DOCKER_CAPS.replace(']', r#","CAP_SYS_ADMIN"]"#)),
            Caps::Unset => None,
        }
        .map(|set| {
            format!(
                r#""capabilities": {{"bounding": {set}, "effective": {set}, "permitted": {set}}},
    "#
            )
        })
        .unwrap_or_default();
        // A container in a user namespace holding CAP_SYS_ADMIN needs a network namespace of its own,
        // or its create is refused (beda8a9); one given no capability sets holds it too.
        let namespaces = match caps {
            Caps::SysAdmin | Caps::Unset => format!(r#"{namespaces},{{"type":"network"}}"#),
            Caps::Docker => namespaces,
        };
        let config = format!(
            r#"{{
  "ociVersion": "1.3.0",
  "root": {{"path": "{rootfs}", "readonly": false}},
  "process": {{
    "cwd": "/",
    "args": {args:?},
    "env": ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"],
    "user": {{"uid": 0, "gid": 0}},
    {capabilities}"noNewPrivileges": true
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

    /// An `oci` box of `owner` (none when `None`) whose init waits, holding `caps`, bounded by `bounds`.
    fn oci(&self, id: &str, owner: Option<&BoxOwner>, caps: Caps, bounds: Bounds) -> BoxSpec {
        BoxSpec {
            id: BoxId::new(id).unwrap(),
            bounds,
            class: ClassSpec::Bundle(self.bundle(caps)),
            owner: owner.cloned(),
        }
    }

    fn node(&self, box_root: &Path) -> (Node, tempfile::TempDir) {
        let state = tempfile::tempdir_in(self.bundles.path()).unwrap();
        let node = Node::open(box_root, state.path(), state.path())
            .expect("open a node")
            .with_oci_helper(PathBuf::from(&self.exe));
        (node, state)
    }
}

/// The `hex selftest-box` program for `act`, as the container sees the binary (`/hex`).
fn selftest(act: &Act) -> Program {
    let mut args = vec!["/hex", "selftest-box"];
    let n;
    match act {
        Act::Wait => args.push("wait"),
        Act::Hog(bytes) => {
            n = bytes.to_string();
            args.extend(["hog", n.as_str()]);
        }
        Act::Spawn(count) => {
            n = count.to_string();
            args.extend(["spawn", n.as_str()]);
        }
        _ => panic!("no such act here"),
    }
    Program::of(&args)
}

/// A process's pidfd, which reads ready once it has ended, whoever reaps it: the node agent in this
/// process sweeps up ended processes of its boxes, so a test's own child may be reaped before the test
/// waits for it.
fn pidfd(pid: u32) -> OwnedFd {
    rustix::process::pidfd_open(
        Pid::from_raw(i32::try_from(pid).unwrap()).unwrap(),
        PidfdFlags::empty(),
    )
    .expect("pidfd_open")
}

/// Whether the process `fd` names has ended.
fn has_ended(fd: &OwnedFd) -> bool {
    let mut fds = [PollFd::new(fd, PollFlags::IN)];
    let now = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    rustix::event::poll(&mut fds, Some(&now)).unwrap() > 0
}

/// Runs `program` in box `id` to its end.
fn run(node: &Node, id: &BoxId, program: Program) -> Exit {
    let mut p = node.exec(id, program, Stdio::null()).expect("exec");
    p.wait_for(PATIENCE)
        .expect("wait")
        .expect("the program ended in time")
}

fn exercise(own: &Path, cache: &Path, exe: &str) {
    let lab = Lab::new(cache, exe);
    // Whether a container runs here at all (rootless needs unprivileged user namespaces).
    {
        let (node, _state) = lab.node(&own.join("hex-reuse-probe"));
        let spec = lab.oci("probe", None, Caps::Docker, Bounds::default());
        if let Err(e) = node.create(spec) {
            eprintln!("SKIP: rootless OCI unavailable here ({e})");
            return;
        }
        node.delete(&BoxId::new("probe").unwrap()).unwrap();
    }
    a_delete_ends_a_box_through_its_inits_pidfd(&lab, own);
    a_box_of_one_owner_is_given_the_cgroup_its_earlier_box_left(&lab, own);
    each_guard_gives_a_fresh_cgroup_and_counts_why(&lab, own);
}

/// A watch on a cgroup's directory for writes to its `cgroup.kill`: every write to a file in a watched
/// directory is reported with the file's name (vfs_write's fsnotify_modify; inotify(7), IN_MODIFY), so
/// the test knows whether a delete wrote it independently of the node's own counts. The directory is
/// watched since `cgroup.kill` itself is write-only, and inotify_add_watch(2) needs read access to what
/// it watches.
/// Where this user has no inotify instance left (EMFILE: `fs.inotify.max_user_instances` is per user, and a
/// root lab's root shares the host's root, whose services hold them), it says so on stderr and checks
/// nothing: the node's own counts are then the case's evidence, and the rootless runs keep this check.
struct KillWatch(Option<OwnedFd>);

impl KillWatch {
    fn on(cgroup: &Path) -> Self {
        let fd = match inotify::init(inotify::CreateFlags::CLOEXEC | inotify::CreateFlags::NONBLOCK)
        {
            Ok(fd) => fd,
            Err(rustix::io::Errno::MFILE) => {
                eprintln!(
                    "NOTE: no inotify instance left for this user here (EMFILE): cgroup.kill writes go unwatched, the node's counts stand alone"
                );
                return Self(None);
            }
            Err(e) => panic!("inotify: {e}"),
        };
        inotify::add_watch(&fd, cgroup, inotify::WatchFlags::MODIFY)
            .expect("watch the cgroup's directory");
        Self(Some(fd))
    }

    /// Whether `cgroup.kill` was written since the watch began; `None` where nothing watched it.
    fn written(&self) -> Option<bool> {
        let fd = self.0.as_ref()?;
        let mut buf = [MaybeUninit::<u8>::uninit(); 4096];
        let mut events = inotify::Reader::new(fd, &mut buf);
        loop {
            match events.next() {
                Ok(e)
                    if e.events().contains(inotify::ReadFlags::MODIFY)
                        && e.file_name()
                            .is_some_and(|n| n.to_bytes() == b"cgroup.kill") =>
                {
                    return Some(true);
                }
                Ok(_) => {}
                Err(rustix::io::Errno::AGAIN) => return Some(false),
                Err(e) => panic!("reading the cgroup's watch: {e}"),
            }
        }
    }
}

/// Test 1. Do: delete an `oci` box whose init has a pid namespace of its own, with a program exec'd in
/// it; delete one with an outside process moved into its cgroup; delete a `process` box running a
/// program. Expect: the first ended through its init's pidfd with no `cgroup.kill` write, its cgroup gone
/// and its program reaped, ended by SIGKILL; the second falls back to `cgroup.kill`, counted, its
/// outsider killed; the third ends by `cgroup.kill`, counted.
fn a_delete_ends_a_box_through_its_inits_pidfd(lab: &Lab, own: &Path) {
    let (node, _state) = lab.node(&own.join("hex-reuse-end"));
    // The first: a program exec'd in it, still running at the delete.
    let id = BoxId::new("ends").unwrap();
    node.create(lab.oci("ends", None, Caps::Docker, Bounds::default()))
        .unwrap();
    node.start(&id).unwrap();
    let mut program = node.exec(&id, selftest(&Act::Wait), Stdio::null()).unwrap();
    let pid = program.pid();
    let cg = node.cgroup_dir(&id).expect("the box's cgroup");
    let watch = KillWatch::on(&cg);
    let before = node.kills();
    node.delete(&id).unwrap();
    let after = node.kills();
    assert_eq!(
        (after.through_init, after.fell_back, after.cgroup_kill),
        (
            before.through_init + 1,
            before.fell_back,
            before.cgroup_kill
        ),
        "the delete ended it through its init's pidfd: {before:?} -> {after:?}"
    );
    assert_ne!(
        watch.written(),
        Some(true),
        "the delete wrote the box's cgroup.kill"
    );
    assert!(!cg.exists(), "the box's cgroup is gone");
    assert_eq!(
        program.wait_for(PATIENCE).unwrap(),
        Some(Exit::Signal(9)),
        "the exec'd program ended with its namespace"
    );
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "the exec'd program was reaped: no zombie of it"
    );
    eprintln!("test 1, own pid namespace: ended through the init's pidfd, no cgroup.kill write");

    // The second: a process outside the box's pid namespace in its cgroup.
    let id = BoxId::new("outsider").unwrap();
    node.create(lab.oci("outsider", None, Caps::Docker, Bounds::default()))
        .unwrap();
    node.start(&id).unwrap();
    let cg = node.cgroup_dir(&id).expect("the box's cgroup");
    let mut outside = Command::new("sleep").arg("600").spawn().expect("sleep");
    let outside_fd = pidfd(outside.id());
    std::fs::write(cg.join("cgroup.procs"), outside.id().to_string())
        .expect("move a process from outside into the box's cgroup");
    let watch = KillWatch::on(&cg);
    let before = node.kills();
    node.delete(&id).unwrap();
    let after = node.kills();
    assert_eq!(
        (after.through_init, after.fell_back, after.cgroup_kill),
        (
            before.through_init,
            before.fell_back + 1,
            before.cgroup_kill
        ),
        "the delete fell back to cgroup.kill: {before:?} -> {after:?}"
    );
    assert_ne!(
        watch.written(),
        Some(false),
        "the fallback wrote no cgroup.kill"
    );
    wait_for(|| has_ended(&outside_fd), "the outsider killed");
    // Reaped here unless the node's sweep reaped it first.
    let _ = outside.wait();
    assert!(!cg.exists(), "the box's cgroup is gone");
    eprintln!("test 1, outsider: fell back to cgroup.kill, counted, the outsider killed");

    // The third: a process box.
    let id = BoxId::new("process").unwrap();
    node.create(BoxSpec {
        id: id.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Process,
        owner: None,
    })
    .unwrap();
    let mut program = node
        .exec(&id, Program::of(&["sleep", "600"]), Stdio::null())
        .unwrap();
    let cg = node.cgroup_dir(&id).expect("the box's cgroup");
    let watch = KillWatch::on(&cg);
    let before = node.kills();
    node.delete(&id).unwrap();
    let after = node.kills();
    assert_eq!(
        (after.through_init, after.fell_back, after.cgroup_kill),
        (
            before.through_init,
            before.fell_back,
            before.cgroup_kill + 1
        ),
        "a process box is ended by cgroup.kill: {before:?} -> {after:?}"
    );
    assert_ne!(
        watch.written(),
        Some(false),
        "the delete wrote no cgroup.kill for the process box"
    );
    assert_eq!(program.wait_for(PATIENCE).unwrap(), Some(Exit::Signal(9)));
    eprintln!("test 1, process box: ended by cgroup.kill, counted");

    // The fourth: an oci box whose init ended on its own and was reaped before the delete, its exit read.
    let id = BoxId::new("ended").unwrap();
    node.create(BoxSpec {
        id: id.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Bundle(
            lab.bundle_running(Caps::Docker, &["/hex", "selftest-box", "fail", "0"]),
        ),
        owner: None,
    })
    .unwrap();
    node.start(&id).unwrap();
    wait_for(
        || {
            node.inspect(&id)
                .is_ok_and(|info| info.exit == Some(Exit::Code(0)))
        },
        "the init's exit, read by inspect",
    );
    let cg = node.cgroup_dir(&id).expect("the box's cgroup");
    let watch = KillWatch::on(&cg);
    let before = node.kills();
    node.delete(&id).unwrap();
    let after = node.kills();
    assert_eq!(
        (after.through_init, after.fell_back, after.cgroup_kill),
        (
            before.through_init + 1,
            before.fell_back,
            before.cgroup_kill
        ),
        "a box whose init had ended needs no kill: {before:?} -> {after:?}"
    );
    assert_ne!(
        watch.written(),
        Some(true),
        "the delete wrote the box's cgroup.kill"
    );
    assert!(!cg.exists(), "the box's cgroup is gone");
    eprintln!("test 1, init ended before the delete: no kill, no cgroup.kill write");
}

/// A cgroup file's value, trimmed.
fn read(cg: &Path, file: &str) -> String {
    std::fs::read_to_string(cg.join(file))
        .unwrap_or_else(|e| panic!("{}/{file}: {e}", cg.display()))
        .trim()
        .to_owned()
}

/// The value of `key` in a flat-keyed cgroup file.
fn flat(cg: &Path, file: &str, key: &str) -> u64 {
    hex_core::cgroup::flat(&read(cg, file), key).unwrap_or(0)
}

/// Test 2. Do: box A of owner O burns CPU, holds 64 MiB, has a program OOM-killed and forks refused, and
/// is deleted; then box B of owner O is created and started. Expect: B is given A's slot; B's reported
/// CPU is under 1% of A's, its peak under half A's, its OOM kills and refused forks 0, while the
/// cgroup's own files still count A's; B's memory bound and the default of the pids bound A set and B
/// doesn't are in force.
fn a_box_of_one_owner_is_given_the_cgroup_its_earlier_box_left(lab: &Lab, own: &Path) {
    let (node, _state) = lab.node(&own.join("hex-reuse-ab"));
    let o = BoxOwner::named("owner-o").unwrap();
    let a = BoxId::new("a").unwrap();
    let a_bounds = Bounds {
        memory_max: Some(128 << 20),
        pids_max: Some(32),
        ..Bounds::default()
    };
    node.create(lab.oci("a", Some(&o), Caps::Docker, a_bounds))
        .unwrap();
    node.start(&a).unwrap();
    let slot = node.cgroup_dir(&a).expect("A's cgroup");
    let name = slot.file_name().unwrap().to_str().unwrap().to_owned();
    assert!(
        hex_core::reuse::slot_of(&name).is_some(),
        "an owned box's cgroup is a slot: {name}"
    );
    let burn = Program::of(&[
        "/bin/sh",
        "-c",
        "head -c 3000000000 /dev/zero | sha256sum >/dev/null",
    ]);
    assert_eq!(run(&node, &a, burn), Exit::Code(0));
    assert_eq!(run(&node, &a, selftest(&Act::Hog(64 << 20))), Exit::Code(0));
    assert_eq!(
        run(&node, &a, selftest(&Act::Hog(512 << 20))),
        Exit::Signal(9),
        "a program past A's memory bound is OOM-killed"
    );
    assert_ne!(
        run(&node, &a, selftest(&Act::Spawn(64))),
        Exit::Code(0),
        "a program past A's process bound fails"
    );
    let ua = node.usage(&a).unwrap();
    assert!(
        ua.cpu_usec > 1_000_000
            && ua.memory_peak_bytes >= 64 << 20
            && ua.oom_kills >= 1
            && ua.pids_refused >= 1,
        "A used what the test made it use: {ua:?}"
    );
    node.delete(&a).unwrap();
    assert!(slot.exists(), "A's cgroup is kept idle for its owner");
    let stat = read(&slot, "memory.stat");
    let kinds: Vec<String> = [
        "anon",
        "file",
        "kernel",
        "shmem",
        "file_dirty",
        "slab",
        "percpu",
        "sock",
    ]
    .iter()
    .map(|k| format!("{k} {}", hex_core::cgroup::flat(&stat, k).unwrap_or(0)))
    .collect();
    eprintln!(
        "test 2: A left {} bytes charged in its idle cgroup ({})",
        read(&slot, "memory.current"),
        kinds.join(", ")
    );
    let before = node.cgroup_reuse().unwrap();
    let b = BoxId::new("b").unwrap();
    node.create(lab.oci(
        "b",
        Some(&o),
        Caps::Docker,
        Bounds {
            memory_max: Some(256 << 20),
            ..Bounds::default()
        },
    ))
    .unwrap();
    let after = node.cgroup_reuse().unwrap();
    assert_eq!(
        node.cgroup_dir(&b).as_deref(),
        Some(slot.as_path()),
        "B is given A's slot: {after:?}"
    );
    assert_eq!(after.pool.handed_on, before.pool.handed_on + 1);
    node.start(&b).unwrap();
    let ub = node.usage(&b).unwrap();
    assert!(
        ub.cpu_usec * 100 < ua.cpu_usec,
        "B's CPU {} us is under 1% of A's {} us",
        ub.cpu_usec,
        ua.cpu_usec
    );
    assert!(
        ub.memory_peak_bytes < ua.memory_peak_bytes / 2,
        "B's peak {} is its own, not A's {}",
        ub.memory_peak_bytes,
        ua.memory_peak_bytes
    );
    assert_eq!(
        (ub.oom_kills, ub.pids_refused),
        (0, 0),
        "B's events: {ub:?}"
    );
    // The cgroup's own counters run on from A's: what B is told is a delta, not a fresh cgroup's.
    assert!(flat(&slot, "memory.events", "oom_kill") >= 1);
    assert!(flat(&slot, "pids.events", "max") >= 1);
    assert!(flat(&slot, "cpu.stat", "usage_usec") >= ua.cpu_usec);
    // Every bound written again: B's own, and A's pids bound back at its default.
    assert_eq!(read(&slot, "memory.max"), (256u64 << 20).to_string());
    assert_eq!(read(&slot, "pids.max"), "max");
    eprintln!(
        "test 2: B in A's slot, CPU {} us against A's {}, peak {} against {}, events 0",
        ub.cpu_usec, ua.cpu_usec, ub.memory_peak_bytes, ua.memory_peak_bytes
    );
    node.delete(&b).unwrap();
    node.release_idle().unwrap();
    assert!(!slot.exists(), "released, no idle cgroup is left");
}

/// Waits for `cond`, a fact, at most [`PATIENCE`].
fn wait_for(mut cond: impl FnMut() -> bool, what: &str) {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

/// One state guard's case: A of owner O made, started, given `work`, deleted; `leave` done to its idle
/// cgroup; B of O created. Expect B a fresh cgroup, A's removed, `reason` counted once more.
fn a_state_guard(
    lab: &Lab,
    node: &Node,
    case: &str,
    work: Option<Program>,
    leave: impl FnOnce(&Path),
    reason: Refused,
) {
    let o = BoxOwner::named("owner-guard").unwrap();
    let a = BoxId::new(&format!("{case}-a")).unwrap();
    node.create(lab.oci(
        &format!("{case}-a"),
        Some(&o),
        Caps::Docker,
        Bounds::default(),
    ))
    .unwrap();
    node.start(&a).unwrap();
    if let Some(work) = work {
        assert_eq!(run(node, &a, work), Exit::Code(0), "{case}: A's work");
    }
    let slot = node.cgroup_dir(&a).unwrap();
    node.delete(&a).unwrap();
    assert!(slot.exists(), "{case}: A's cgroup is idle");
    leave(&slot);
    let before = node.cgroup_reuse().unwrap();
    let b = BoxId::new(&format!("{case}-b")).unwrap();
    node.create(lab.oci(
        &format!("{case}-b"),
        Some(&o),
        Caps::Docker,
        Bounds::default(),
    ))
    .unwrap();
    let after = node.cgroup_reuse().unwrap();
    assert_ne!(
        node.cgroup_dir(&b).as_deref(),
        Some(slot.as_path()),
        "{case}: B is given a fresh cgroup"
    );
    assert_eq!(
        after.pool.refused(reason),
        before.pool.refused(reason) + 1,
        "{case}: counted as {}: {after:?}",
        reason.name()
    );
    wait_for(|| !slot.exists(), &format!("{case}: A's cgroup removed"));
    node.delete(&b).unwrap();
    eprintln!(
        "test 3, {case}: B fresh, A's removed, counted {}",
        reason.name()
    );
}

/// Test 3. Do: for each guard, leave A's idle cgroup unclean, or make A or B a box that may not share
/// one. Expect: each B a fresh cgroup, each refusal counted by its reason, A's removed where it was left
/// unclean or never kept, and an idle cgroup handed to its own owner's box alone.
fn each_guard_gives_a_fresh_cgroup_and_counts_why(lab: &Lab, own: &Path) {
    let (node, _state) = lab.node(&own.join("hex-reuse-guard"));
    a_state_guard(
        lab,
        &node,
        "sub",
        None,
        |slot| std::fs::create_dir(slot.join("left")).unwrap(),
        Refused::Descendants,
    );
    a_state_guard(
        lab,
        &node,
        "oomgroup",
        None,
        |slot| std::fs::write(slot.join("memory.oom.group"), "1").unwrap(),
        Refused::OomGroup,
    );
    let mut outside = Command::new("sleep").arg("600").spawn().expect("sleep");
    let pid = outside.id();
    let outside_fd = pidfd(pid);
    a_state_guard(
        lab,
        &node,
        "live",
        None,
        |slot| std::fs::write(slot.join("cgroup.procs"), pid.to_string()).unwrap(),
        Refused::Populated,
    );
    wait_for(
        || has_ended(&outside_fd),
        "the process left in A's cgroup ended with it",
    );
    let _ = outside.wait();
    let big = lab.rootfs.join("big");
    a_state_guard(
        lab,
        &node,
        "charged",
        Some(Program::of(&[
            "/bin/sh",
            "-c",
            "head -c 67108864 /dev/zero > /big",
        ])),
        |slot| {
            let charged: u64 = read(slot, "memory.current").parse().unwrap();
            assert!(charged >= 60 << 20, "A left {charged} bytes charged");
        },
        Refused::Charged,
    );
    std::fs::remove_file(&big).unwrap();

    // A killed through cgroup.kill: its cgroup is never kept, since a task cloned into it later dies.
    let o = BoxOwner::named("owner-guard").unwrap();
    let a = BoxId::new("killed-a").unwrap();
    node.create(lab.oci("killed-a", Some(&o), Caps::Docker, Bounds::default()))
        .unwrap();
    node.start(&a).unwrap();
    let slot = node.cgroup_dir(&a).unwrap();
    node.signal(&a, hex_api::Signal::KILL).unwrap();
    wait_for(
        || {
            node.inspect(&a)
                .is_ok_and(|info| info.exit == Some(Exit::Signal(9)))
        },
        "A's init killed",
    );
    let before = node.cgroup_reuse().unwrap();
    node.delete(&a).unwrap();
    let after = node.cgroup_reuse().unwrap();
    assert!(
        !slot.exists(),
        "a cgroup cgroup.kill was written to is removed, not kept: {after:?}"
    );
    assert_eq!(
        after.discarded_killed,
        before.discarded_killed + 1,
        "counted: {after:?}"
    );
    let b = BoxId::new("killed-b").unwrap();
    node.create(lab.oci("killed-b", Some(&o), Caps::Docker, Bounds::default()))
        .unwrap();
    assert_ne!(
        node.cgroup_dir(&b).as_deref(),
        Some(slot.as_path()),
        "B is given a fresh cgroup"
    );
    node.start(&b).unwrap();
    assert_eq!(
        run(&node, &b, selftest(&Act::Hog(1 << 20))),
        Exit::Code(0),
        "B's init and its programs run"
    );
    node.delete(&b).unwrap();
    eprintln!("test 3, killed: A's cgroup removed, not kept, counted; B fresh, and runs");

    // B of another owner: never A's, a fresh cgroup, counted. What becomes of A's idle cgroup then is
    // Ada's call (the plan's "A's is removed" against keeping it for O; the lead, 2026-11-02T18:23Z), so
    // nothing here asserts either.
    let o = BoxOwner::named("owner-o").unwrap();
    let p = BoxOwner::named("owner-p").unwrap();
    node.create(lab.oci("other-a", Some(&o), Caps::Docker, Bounds::default()))
        .unwrap();
    let slot = node.cgroup_dir(&BoxId::new("other-a").unwrap()).unwrap();
    node.delete(&BoxId::new("other-a").unwrap()).unwrap();
    let before = node.cgroup_reuse().unwrap();
    node.create(lab.oci("other-b", Some(&p), Caps::Docker, Bounds::default()))
        .unwrap();
    let after = node.cgroup_reuse().unwrap();
    assert_ne!(
        node.cgroup_dir(&BoxId::new("other-b").unwrap()).as_deref(),
        Some(slot.as_path()),
        "another owner's box is never given A's cgroup"
    );
    assert_eq!(
        after.pool.refused(Refused::Owner),
        before.pool.refused(Refused::Owner) + 1
    );
    eprintln!("test 3, another owner: B fresh, counted");
    // O's next box, so O has an idle cgroup for the CAP_SYS_ADMIN case below once it's deleted.
    node.create(lab.oci("other-c", Some(&o), Caps::Docker, Bounds::default()))
        .unwrap();
    for id in ["other-b", "other-c"] {
        node.delete(&BoxId::new(id).unwrap()).unwrap();
    }

    // A of no owner, A holding CAP_SYS_ADMIN, and A whose spec gives no capability sets: a cgroup of
    // its own, gone with it.
    for (case, owner, caps, reason) in [
        ("noowner", None, Caps::Docker, Refused::NoOwner),
        ("sysadmin", Some(&o), Caps::SysAdmin, Refused::Privileged),
        ("unset", Some(&o), Caps::Unset, Refused::Privileged),
    ] {
        let a = BoxId::new(&format!("{case}-a")).unwrap();
        let before = node.cgroup_reuse().unwrap();
        node.create(lab.oci(&format!("{case}-a"), owner, caps, Bounds::default()))
            .unwrap();
        let after = node.cgroup_reuse().unwrap();
        assert_eq!(
            after.pool.refused(reason),
            before.pool.refused(reason) + 1,
            "{case}: counted as {}",
            reason.name()
        );
        let cg = node.cgroup_dir(&a).unwrap();
        assert_eq!(
            cg.file_name().unwrap().to_str().unwrap(),
            a.as_str(),
            "{case}: A's cgroup is its own, named by its id"
        );
        node.delete(&a).unwrap();
        assert!(!cg.exists(), "{case}: A's cgroup is removed with it");
        eprintln!("test 3, A {case}: a cgroup of its own, removed with it, counted");
    }
    // B holding CAP_SYS_ADMIN beside its owner's idle cgroup (other-c's): never handed it.
    let before = node.cgroup_reuse().unwrap();
    assert!(
        before.pool.idle > 0,
        "its owner has an idle cgroup: {before:?}"
    );
    node.create(lab.oci("sysadmin-b", Some(&o), Caps::SysAdmin, Bounds::default()))
        .unwrap();
    let after = node.cgroup_reuse().unwrap();
    assert_eq!(
        after.pool.refused(Refused::Privileged),
        before.pool.refused(Refused::Privileged) + 1
    );
    assert_eq!(
        after.pool.idle, before.pool.idle,
        "the idle cgroup stays its owner's"
    );
    node.delete(&BoxId::new("sysadmin-b").unwrap()).unwrap();
    eprintln!("test 3, B sysadmin: a cgroup of its own, counted, the idle one left");
    node.release_idle().unwrap();
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
    let mut own = String::new();
    std::fs::File::open("/proc/self/cgroup")
        .and_then(|mut f| f.read_to_string(&mut own))
        .expect("/proc/self/cgroup");
    let rel = own
        .lines()
        .find_map(|l| l.strip_prefix("0::"))
        .expect("a 0:: cgroup line")
        .trim_start_matches('/');
    PathBuf::from("/sys/fs/cgroup").join(rel)
}

/// Moves this process into a leaf of its own cgroup and confirms every controller is delegated, as
/// oci_box.rs does; returns the own cgroup dir, or why it isn't ours to subdivide.
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
        PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".cache/hex-t7/ocireuse");
    std::fs::create_dir_all(&cache).expect("cache dir");
    cache
}
