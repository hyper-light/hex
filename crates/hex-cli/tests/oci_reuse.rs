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
//!   own files still hold A's, and every bound B sets in force, every one it doesn't at its default.
//!   With it, `cpu.max` (the lead's ruling, 2026-11-03T01:19Z): B of O bounded otherwise than A, then C of
//!   O bounded as B: expect the file written for B and left for C, holding their values each time;
//! - test 3, each guard: after A of owner O is deleted, a sub-cgroup made in its cgroup, its
//!   `memory.oom.group` set, a live process moved in, or 64 MiB left charged by A: expect B of O a fresh
//!   cgroup, A's removed and the refusal counted by its reason. A killed through `cgroup.kill` (a signal
//!   KILL to the box) and deleted: expect its cgroup removed, not kept, counted, and B of O a fresh one
//!   that runs: Linux 6.19 kills every task later cloned into a cgroup `cgroup.kill` was written to
//!   (its `kill_seq`, kernel/cgroup/cgroup.c:6702-6705 and :6941). B of another owner: a fresh cgroup,
//!   counted, and A's kept in O's pool, then handed to O's next box (Ada's call, 2026-11-03: keep, not
//!   the plan's "A's is removed"). A of no owner, holding CAP_SYS_ADMIN, or whose spec
//!   gives no capability sets (it keeps its creator's, all of them): a cgroup of its own, removed with
//!   it, counted; B holding CAP_SYS_ADMIN beside its owner's idle cgroup: a cgroup of its own, counted,
//!   the idle one left;
//! - the network namespace, reused within one owner (Ada's call, 2026-11-03; decision 5), root only: A of
//!   O, which could change nothing of its namespace, deleted; B of O: expect A's namespace, counted, its
//!   loopback up and A's counters still in it. A's namespace left holding a TIME_WAIT connection, an open
//!   socket, an address or a route: expect B fresh and the refusal counted by its reason, the open socket's
//!   check counted as reading the protocols; one holding a socket never hashed: expect the protocols read,
//!   counted, and the namespace kept. A holding
//!   CAP_NET_ADMIN, with `/proc/sys` writable, or of no owner: never kept, counted. Boxes of another owner P
//!   beside O's idle namespace: each fresh, counted, never O's; then O's next box gets O's and P's next box
//!   P's oldest. B of O given a namespace the node checked: expect B to see the node's netlink socket in it and
//!   its send to that socket refused, its send to the kernel gone, the socket's queue and drops unchanged, and
//!   the namespace kept again at its next check.
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
use hex_core::netns_reuse::Refused as NetRefused;
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
    /// Docker's defaults and CAP_NET_ADMIN.
    NetAdmin,
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
        self.bundle_with(caps, args, false, false)
    }

    /// A bundle whose init runs `args`, its process holding `caps`, with a network namespace of its own
    /// when `network` and `/proc/sys` read-only when `readonly_sys`.
    fn bundle_with(&self, caps: Caps, args: &[&str], network: bool, readonly_sys: bool) -> PathBuf {
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
            Caps::NetAdmin => Some(DOCKER_CAPS.replace(']', r#","CAP_NET_ADMIN"]"#)),
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
            Caps::Docker | Caps::NetAdmin if network => {
                format!(r#"{namespaces},{{"type":"network"}}"#)
            }
            Caps::Docker | Caps::NetAdmin => namespaces,
        };
        let readonly = if readonly_sys {
            r#","readonlyPaths": ["/proc/sys"]"#
        } else {
            ""
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
  "linux": {{"namespaces": [{namespaces}]{mappings}{readonly}}}
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
    a_reused_cgroups_cpu_max_is_written_only_when_its_values_differ(&lab, own);
    each_guard_gives_a_fresh_cgroup_and_counts_why(&lab, own);
    a_boxs_network_namespace_is_reused_within_its_owner_when_clean(&lab, own);
    a_box_can_not_send_to_the_socket_the_check_keeps_in_its_namespace(&lab, own);
    a_kept_namespace_holds_its_proc_net_files_and_they_close_with_it(&lab, own);
}

/// A watch on a cgroup's directory for writes to one of its files (`cgroup.kill`, `cpu.max`): every
/// write to a file in a watched directory is reported with the file's name (vfs_write's
/// fsnotify_modify; inotify(7), IN_MODIFY), so the test knows whether the node wrote it independently of
/// the node's own counts. The directory is watched since `cgroup.kill` itself is write-only, and
/// inotify_add_watch(2) needs read access to what it watches.
/// Where this user has no inotify instance left (EMFILE: `fs.inotify.max_user_instances` is per user, and a
/// root lab's root shares the host's root, whose services hold them), it says so on stderr and checks
/// nothing: the node's own counts and the file's contents are then the case's evidence, and the rootless
/// runs keep this check.
struct WriteWatch {
    fd: Option<OwnedFd>,
    file: &'static str,
}

impl WriteWatch {
    fn on(cgroup: &Path, file: &'static str) -> Self {
        let fd = match inotify::init(inotify::CreateFlags::CLOEXEC | inotify::CreateFlags::NONBLOCK)
        {
            Ok(fd) => fd,
            Err(rustix::io::Errno::MFILE) => {
                eprintln!(
                    "NOTE: no inotify instance left for this user here (EMFILE): {file} writes go unwatched, the node's counts stand alone"
                );
                return Self { fd: None, file };
            }
            Err(e) => panic!("inotify: {e}"),
        };
        inotify::add_watch(&fd, cgroup, inotify::WatchFlags::MODIFY)
            .expect("watch the cgroup's directory");
        Self { fd: Some(fd), file }
    }

    /// Whether the file was written since the watch began; `None` where nothing watched it.
    fn written(&self) -> Option<bool> {
        let fd = self.fd.as_ref()?;
        let mut buf = [MaybeUninit::<u8>::uninit(); 4096];
        let mut events = inotify::Reader::new(fd, &mut buf);
        loop {
            match events.next() {
                Ok(e)
                    if e.events().contains(inotify::ReadFlags::MODIFY)
                        && e.file_name()
                            .is_some_and(|n| n.to_bytes() == self.file.as_bytes()) =>
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
    let watch = WriteWatch::on(&cg, "cgroup.kill");
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
    let watch = WriteWatch::on(&cg, "cgroup.kill");
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
    let watch = WriteWatch::on(&cg, "cgroup.kill");
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
    let watch = WriteWatch::on(&cg, "cgroup.kill");
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

/// With test 2, the lead's ruling on `cpu.max` (2026-11-03T01:19Z): a reused cgroup's `cpu.max` is
/// written again when the next box's values differ from what it holds, and left when they're the same,
/// compared as values (`hex_core::cgroup::CpuMax`). Do: A of owner O, bounded to half a CPU, is deleted;
/// B of O, bounded to a quarter, is created; B is deleted, and C of O, bounded to a quarter too, is
/// created. Expect: B and C each given A's slot, the file holding a quarter's values after each, and a
/// write to it seen for B and none for C.
fn a_reused_cgroups_cpu_max_is_written_only_when_its_values_differ(lab: &Lab, own: &Path) {
    let (node, _state) = lab.node(&own.join("hex-reuse-cpu"));
    let o = BoxOwner::named("owner-cpu").unwrap();
    let bounded = |milli| Bounds {
        cpu_max_milli: Some(milli),
        ..Bounds::default()
    };
    let a = BoxId::new("cpu-a").unwrap();
    node.create(lab.oci("cpu-a", Some(&o), Caps::Docker, bounded(500)))
        .unwrap();
    let slot = node.cgroup_dir(&a).unwrap();
    let held = || std::fs::read_to_string(slot.join("cpu.max")).unwrap();
    assert_eq!(held().trim(), "50000 100000", "A's half a CPU");
    node.delete(&a).unwrap();
    for (id, written) in [("cpu-b", true), ("cpu-c", false)] {
        let watch = WriteWatch::on(&slot, "cpu.max");
        let next = BoxId::new(id).unwrap();
        node.create(lab.oci(id, Some(&o), Caps::Docker, bounded(250)))
            .unwrap();
        assert_eq!(
            node.cgroup_dir(&next).as_deref(),
            Some(slot.as_path()),
            "{id} is given A's slot"
        );
        assert_eq!(held().trim(), "25000 100000", "{id}'s quarter of a CPU");
        if let Some(seen) = watch.written() {
            assert_eq!(
                seen, written,
                "{id}: cpu.max is written only when its values differ"
            );
        }
        node.delete(&next).unwrap();
    }
    eprintln!("cpu.max: written for B (a half to a quarter), left for C (a quarter to a quarter)");
    node.release_idle().unwrap();
}

/// Goal 5 extended, step 6 (Ada's call of 2026-11-03): a box's network namespace handed to its owner's next
/// box when the box couldn't have changed it and the check finds nothing of it left. Root only: only a root
/// node keeps namespaces (a rootless node's containers make theirs in user namespaces of their own). What a
/// box could leave in its namespace is done here from outside, by this process (root) on a thread that joins
/// it, while the box is alive: it is the namespace's state at the delete that the check reads, whoever made
/// it. Do, expect:
/// - A of O, eligible (Docker's capabilities, `/proc/sys` read-only), deleted; B of O: B is given A's
///   namespace (the same inode), counted handed on, and its loopback is up;
/// - the facts a namespace can't be reset of, handed on: a UDP datagram A's namespace sent is still in its
///   counters for B, and B can tell it was A's (the same inode);
/// - each guard, B fresh (another inode) and the reason counted once more: a TCP connection in TIME_WAIT,
///   a socket still open (held from outside), an address added, a route added;
/// - A holding CAP_NET_ADMIN, A with `/proc/sys` writable, A of no owner: never kept, each counted;
/// - B of another owner: fresh, counted Owner, A's kept for O and handed to O's next box.
fn a_boxs_network_namespace_is_reused_within_its_owner_when_clean(lab: &Lab, own: &Path) {
    if lab.rootless {
        eprintln!("SKIP netns reuse: a rootless node keeps no network namespaces");
        return;
    }
    let (node, _state) = lab.node(&own.join("hex-reuse-netns"));
    let o = BoxOwner::named("owner-net").unwrap();
    let wait = ["/hex", "selftest-box", "wait"];
    let make = |id: &str, owner: Option<&BoxOwner>, caps: Caps, readonly_sys: bool| BoxSpec {
        id: BoxId::new(id).unwrap(),
        bounds: Bounds::default(),
        class: ClassSpec::Bundle(lab.bundle_with(caps, &wait, true, readonly_sys)),
        owner: owner.cloned(),
    };
    let counts = || {
        node.netns_reuse()
            .unwrap()
            .expect("a root node keeps namespaces")
    };
    let netns_of = |id: &str| -> PathBuf {
        let cg = node.cgroup_dir(&BoxId::new(id).unwrap()).unwrap();
        let procs = std::fs::read_to_string(cg.join("cgroup.procs")).unwrap();
        let pid = procs.lines().next().expect("the box's init").to_owned();
        std::fs::read_link(format!("/proc/{pid}/ns/net")).unwrap()
    };
    let init_of = |id: &str| -> String {
        let cg = node.cgroup_dir(&BoxId::new(id).unwrap()).unwrap();
        let procs = std::fs::read_to_string(cg.join("cgroup.procs")).unwrap();
        procs.lines().next().expect("the box's init").to_owned()
    };
    // A namespace's inode is its identity only while it lives: one freed may have its number given to the next
    // one made. So each comparison holds the earlier box's namespace open until it's made.
    let hold = |id: &str| std::fs::File::open(format!("/proc/{}/ns/net", init_of(id))).unwrap();
    // The node's checker thread rules on each namespace given back, in turn: the fact to wait on is its
    // ruling on every one given back so far, the one just given back among them.
    let ruled = || {
        wait_for(
            || {
                let c = counts();
                c.checked + c.unchecked == c.given_back
            },
            "the checker's ruling on every namespace given back",
        );
        counts()
    };

    // Handed on: B gets A's namespace, and its loopback is up.
    node.create(make("net-a", Some(&o), Caps::Docker, true))
        .unwrap();
    let a_ns = netns_of("net-a");
    let a_held = hold("net-a");
    in_netns(&init_of("net-a"), || {
        let s = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        s.send_to(b"x", s.local_addr().unwrap()).unwrap();
    });
    let before = ruled();
    node.delete(&BoxId::new("net-a").unwrap()).unwrap();
    let after = ruled();
    assert_eq!(after.given_back, before.given_back + 1, "{after:?}");
    assert_eq!(
        after.pool.idle,
        before.pool.idle + 1,
        "A's namespace is kept for O: {after:?}"
    );
    let before = counts();
    node.create(make("net-b", Some(&o), Caps::Docker, true))
        .unwrap();
    assert_eq!(
        netns_of("net-b"),
        a_ns,
        "B is given A's namespace, and can tell"
    );
    assert_eq!(counts().pool.handed_on, before.pool.handed_on + 1);
    let snmp = std::fs::read_to_string(format!("/proc/{}/net/snmp", init_of("net-b"))).unwrap();
    assert!(
        udp_out(&snmp) >= 1,
        "A's datagram is still in the namespace's counters for B: {snmp}"
    );
    in_netns(&init_of("net-b"), || {
        std::net::TcpListener::bind("127.0.0.1:0").expect("B's loopback is up");
    });
    drop(a_held);
    eprintln!("netns: B given A's namespace, counted, its loopback up, A's counters still in it");
    node.delete(&BoxId::new("net-b").unwrap()).unwrap();
    ruled();
    node.release_idle().unwrap();

    // Each guard: what A's namespace holds at its delete refuses it, by why, and B gets a fresh one.
    let mut held: Vec<std::net::UdpSocket> = Vec::new();
    for (case, reason) in [
        ("timewait", NetRefused::TimeWait),
        ("socket", NetRefused::Sockets),
        ("address", NetRefused::Addresses),
        ("route", NetRefused::Routes),
    ] {
        let a = format!("net-{case}-a");
        node.create(make(&a, Some(&o), Caps::Docker, true)).unwrap();
        let a_ns = netns_of(&a);
        let a_held = hold(&a);
        let pid = init_of(&a);
        match case {
            "timewait" => in_netns(&pid, time_wait),
            "socket" => {
                let (tx, rx) = std::sync::mpsc::channel();
                in_netns(&pid, || {
                    tx.send(std::net::UdpSocket::bind("127.0.0.1:0").unwrap())
                        .unwrap()
                });
                held.push(rx.recv().unwrap());
            }
            "address" => in_netns(&pid, || rtnetlink(new_address())),
            _ => in_netns(&pid, || rtnetlink(new_route())),
        }
        let before = ruled();
        node.delete(&BoxId::new(&a).unwrap()).unwrap();
        let after = ruled();
        assert_eq!(
            after.pool.refused(reason),
            before.pool.refused(reason) + 1,
            "{case}: counted as {}: {after:?}",
            reason.name()
        );
        if case == "socket" {
            assert_eq!(
                after.protocols,
                before.protocols + 1,
                "{case}: a socket past the check's own reads the protocols, counted: {after:?}"
            );
        }
        assert_eq!(after.pool.idle, before.pool.idle, "{case}: not kept");
        let b = format!("net-{case}-b");
        node.create(make(&b, Some(&o), Caps::Docker, true)).unwrap();
        assert_ne!(netns_of(&b), a_ns, "{case}: B is given a fresh namespace");
        drop(a_held);
        node.delete(&BoxId::new(&b).unwrap()).unwrap();
        ruled();
        node.release_idle().unwrap();
        eprintln!("netns, {case}: A's refused, counted, B fresh");
    }
    drop(held);

    // A socket never hashed (a UDP socket never bound, held outside) is counted by sockstat and by no protocol:
    // the check reads the protocols, counted, and keeps the namespace, as decision 5 says it crosses within the
    // owner (the lead's conditions for the check's own socket, 2026-11-03T17:36Z).
    node.create(make("net-unhashed-a", Some(&o), Caps::Docker, true))
        .unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    in_netns(&init_of("net-unhashed-a"), || {
        tx.send(
            rustix::net::socket(
                rustix::net::AddressFamily::INET,
                rustix::net::SocketType::DGRAM,
                None,
            )
            .unwrap(),
        )
        .unwrap()
    });
    let unbound = rx.recv().unwrap();
    let before = ruled();
    node.delete(&BoxId::new("net-unhashed-a").unwrap()).unwrap();
    let after = ruled();
    assert_eq!(
        after.protocols,
        before.protocols + 1,
        "unhashed: the protocols read, counted: {after:?}"
    );
    assert_eq!(
        after.pool.idle,
        before.pool.idle + 1,
        "unhashed: in no protocol's count, so kept for O: {after:?}"
    );
    drop(unbound);
    node.release_idle().unwrap();
    eprintln!("netns, a socket never hashed: the protocols read, counted, the namespace kept");

    // Never kept: a box that could have changed its namespace, or that names no owner.
    for (case, owner, caps, readonly_sys, reason) in [
        (
            "netadmin",
            Some(&o),
            Caps::NetAdmin,
            true,
            NetRefused::NetAdmin,
        ),
        (
            "procsys",
            Some(&o),
            Caps::Docker,
            false,
            NetRefused::ProcSysWritable,
        ),
        ("noowner", None, Caps::Docker, true, NetRefused::NoOwner),
    ] {
        let before = ruled();
        let a = format!("net-{case}");
        node.create(make(&a, owner, caps, readonly_sys)).unwrap();
        assert_eq!(
            counts().pool.refused(reason),
            before.pool.refused(reason) + 1,
            "{case}: counted as {}",
            reason.name()
        );
        node.delete(&BoxId::new(&a).unwrap()).unwrap();
        let after = counts();
        assert_eq!(after.given_back, before.given_back, "{case}: never kept");
        assert_eq!(after.pool.idle, before.pool.idle, "{case}: never kept");
        eprintln!("netns, {case}: a namespace of its own, never kept, counted");
    }

    // Another owner's boxes: each fresh, counted, never O's; and each owner's idle namespaces go to that
    // owner's next box alone, oldest first (Ada's call; the lead, 2026-11-03T06:12Z: a namespace never
    // reaches another owner's box). Every namespace compared is alive, kept idle or a live box's.
    let p = BoxOwner::named("owner-net-p").unwrap();
    node.create(make("net-o-a", Some(&o), Caps::Docker, true))
        .unwrap();
    let a_ns = netns_of("net-o-a");
    node.delete(&BoxId::new("net-o-a").unwrap()).unwrap();
    let before = ruled();
    assert_eq!(
        before.pool.idle, 1,
        "A's namespace is kept for O: {before:?}"
    );
    let mut p_ns = Vec::new();
    for i in 0..3 {
        let id = format!("net-p-{i}");
        node.create(make(&id, Some(&p), Caps::Docker, true))
            .unwrap();
        let ns = netns_of(&id);
        assert_ne!(ns, a_ns, "P's box {i} never gets O's");
        p_ns.push(ns);
    }
    assert_eq!(
        counts().pool.refused(NetRefused::Owner),
        before.pool.refused(NetRefused::Owner) + 3,
        "each of P's boxes counted beside O's idle one"
    );
    for i in 0..3 {
        node.delete(&BoxId::new(&format!("net-p-{i}")).unwrap())
            .unwrap();
    }
    let after = ruled();
    assert_eq!(
        after.pool.idle, 4,
        "O's one and P's three, each kept for its owner: {after:?}"
    );
    node.create(make("net-o-c", Some(&o), Caps::Docker, true))
        .unwrap();
    assert_eq!(
        netns_of("net-o-c"),
        a_ns,
        "O's next box gets O's, never one of P's"
    );
    node.create(make("net-p-3", Some(&p), Caps::Docker, true))
        .unwrap();
    assert_eq!(netns_of("net-p-3"), p_ns[0], "P's next box gets P's oldest");
    eprintln!(
        "netns, another owner: P's boxes fresh, counted, never O's; each owner's next box given its own owner's"
    );
    for id in ["net-o-c", "net-p-3"] {
        node.delete(&BoxId::new(id).unwrap()).unwrap();
    }
    ruled();
    node.release_idle().unwrap();
}

/// Goal 5 extended, step 6 (a kept namespace's `/proc/net` files, so a later check needn't enter it; the lead's
/// conditions: closed on every path, counted; CLAUDE.md §2). Do: keep one owner's namespace, reuse it, leave an
/// address in it, a socket in the next, release what's idle, give one past the owner's bound, then let the node go
/// with a box's namespace held and recover over its state, counting the files open in this process by the names
/// their links show, and all its descriptors at the end. Expect: the first clean check enters and opens the four;
/// each later one comes to its verdict through them, a dirty one's too, but a socket past its own, which enters; a
/// box's namespace keeps them; a namespace refused, released or put out by the bound closes them, and so does the
/// node's end; a node recovered holds none; and every descriptor is back to the count before the first node.
fn a_kept_namespace_holds_its_proc_net_files_and_they_close_with_it(lab: &Lab, own: &Path) {
    if lab.rootless {
        eprintln!("SKIP held files: a rootless node keeps no network namespaces");
        return;
    }
    // Its sockstat and dev, and each iptables file its host's modules make.
    let files = 2 + ["ip_tables_names", "ip6_tables_names"]
        .iter()
        .filter(|table| Path::new("/proc/net").join(table).exists())
        .count();
    let open_fds = || std::fs::read_dir("/proc/self/fd").unwrap().count();
    let box_root = own.join("hex-reuse-files");
    let (first_node, _first_state) = lab.node(&box_root);
    drop(first_node);
    let before_all = open_fds();
    let (node, state) = lab.node(&box_root);
    let o = BoxOwner::named("owner-files").unwrap();
    let wait = ["/hex", "selftest-box", "wait"];
    let make = |id: &str| BoxSpec {
        id: BoxId::new(id).unwrap(),
        bounds: Bounds::default(),
        class: ClassSpec::Bundle(lab.bundle_with(Caps::Docker, &wait, true, true)),
        owner: Some(o.clone()),
    };
    let counts = || {
        node.netns_reuse()
            .unwrap()
            .expect("a root node keeps namespaces")
    };
    let ruled = || {
        wait_for(
            || {
                let c = counts();
                c.checked + c.unchecked == c.given_back
            },
            "the checker's ruling on every namespace given back",
        );
        counts()
    };
    let init_of = |id: &str| -> String {
        let cg = node.cgroup_dir(&BoxId::new(id).unwrap()).unwrap();
        let procs = std::fs::read_to_string(cg.join("cgroup.procs")).unwrap();
        procs.lines().next().expect("the box's init").to_owned()
    };
    let delete = |id: &str| node.delete(&BoxId::new(id).unwrap()).unwrap();
    // A socket counts until it's freed, after a grace period, so a namespace's count may stand past the check's
    // own for a while, and a check then enters for the protocols. Waited out where a case asks for the files.
    let settled = |id: &str| {
        let sockstat = format!("/proc/{}/net/sockstat", init_of(id));
        wait_for(
            || {
                std::fs::read_to_string(&sockstat)
                    .unwrap()
                    .starts_with("sockets: used 1\n")
            },
            "the namespace's count down to the check's own socket",
        );
    };
    // A file of `/proc/thread-self/net` opened links to its name under the thread's directory.
    let held = || {
        std::fs::read_dir("/proc/self/fd")
            .unwrap()
            .filter_map(|fd| std::fs::read_link(fd.ok()?.path()).ok())
            .filter(|link| {
                let link = link.to_string_lossy();
                ["sockstat", "dev", "ip_tables_names", "ip6_tables_names"]
                    .iter()
                    .any(|name| link.ends_with(&format!("/net/{name}")))
            })
            .count()
    };
    assert_eq!(held(), 0, "no namespace kept yet");

    node.create(make("files-a")).unwrap();
    let start = ruled();
    delete("files-a");
    let first = ruled();
    assert_eq!(first.pool.idle, start.pool.idle + 1, "{first:?}");
    assert_eq!(
        first.through_files, start.through_files,
        "the first check enters: {first:?}"
    );
    assert_eq!(first.files_failed, 0, "{first:?}");
    assert_eq!(held(), files, "the clean namespace holds its files");

    node.create(make("files-b")).unwrap();
    assert_eq!(held(), files, "a box's namespace keeps them while it lives");
    settled("files-b");
    delete("files-b");
    let second = ruled();
    assert_eq!(
        second.through_files,
        first.through_files + 1,
        "through its files: {second:?}"
    );
    assert_eq!(second.pool.idle, first.pool.idle, "kept again: {second:?}");
    assert_eq!(held(), files);

    node.create(make("files-c")).unwrap();
    in_netns(&init_of("files-c"), || rtnetlink(new_address()));
    settled("files-c");
    delete("files-c");
    let third = ruled();
    assert_eq!(
        third.through_files,
        second.through_files + 1,
        "through its files: {third:?}"
    );
    assert_eq!(
        third.pool.refused(NetRefused::Addresses),
        second.pool.refused(NetRefused::Addresses) + 1,
        "an address the files' path refuses: {third:?}"
    );
    assert_eq!(held(), 0, "a namespace refused closes them");

    node.create(make("files-d")).unwrap();
    delete("files-d");
    ruled();
    node.create(make("files-e")).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    in_netns(&init_of("files-e"), || {
        tx.send(std::net::UdpSocket::bind("127.0.0.1:0").unwrap())
            .unwrap()
    });
    let socket = rx.recv().unwrap();
    let before = ruled();
    delete("files-e");
    let after = ruled();
    assert_eq!(
        after.through_files, before.through_files,
        "a socket past its own enters: {after:?}"
    );
    assert_eq!(after.protocols, before.protocols + 1, "{after:?}");
    assert_eq!(
        after.pool.refused(NetRefused::Sockets),
        before.pool.refused(NetRefused::Sockets) + 1,
        "{after:?}"
    );
    drop(socket);
    assert_eq!(held(), 0, "refused by way of entering, it closes them too");

    node.create(make("files-f")).unwrap();
    delete("files-f");
    ruled();
    assert_eq!(held(), files);
    node.release_idle().unwrap();
    wait_for(|| held() == 0, "the files of a namespace released closing");

    // One past the owner's bound, each namespace with its files: the one put out closes its own.
    let many: Vec<String> = (0..=hex_core::netns_reuse::PER_OWNER)
        .map(|n| format!("files-many-{n}"))
        .collect();
    for id in &many {
        node.create(make(id)).unwrap();
    }
    for id in &many {
        delete(id);
    }
    let filled = ruled();
    assert_eq!(
        held(),
        files * hex_core::netns_reuse::PER_OWNER,
        "the namespace the owner's bound put out closed its files: {filled:?}"
    );
    node.release_idle().unwrap();
    wait_for(|| held() == 0, "the bound's namespaces released closing");
    assert_eq!(counts().files_failed, 0);

    // The node let go with a box's namespace held, its files among what it holds: they close with it, the box runs
    // on, and a node recovered over the same state holds none of them again.
    node.create(make("files-g")).unwrap();
    delete("files-g");
    ruled();
    node.create(make("files-h")).unwrap();
    assert_eq!(held(), files, "the box's namespace holds them");
    drop(node);
    wait_for(|| held() == 0, "the files closing with the node");
    let restarted = Node::open(&box_root, state.path(), state.path())
        .expect("reopen the node")
        .with_oci_helper(PathBuf::from(&lab.exe));
    restarted.recover().expect("recover");
    assert_eq!(held(), 0, "a node recovered holds no namespace's files");
    restarted.delete(&BoxId::new("files-h").unwrap()).unwrap();
    assert_eq!(held(), 0);
    drop(restarted);
    wait_for(
        || open_fds() <= before_all,
        "every descriptor the nodes held closing",
    );
    eprintln!(
        "netns: a kept namespace's files held across a reuse, closed when refused, released, put out or let go"
    );
}

/// Goal 5 extended, step 6 (the check's own socket, kept in each namespace it checks; the lead's conditions,
/// 2026-11-03T17:36Z): a box given a namespace the node checked sees the node's netlink socket in it and can't
/// send to it. Root only, as the namespace's reuse. Do: A of O deleted, its namespace checked and kept; B of O
/// given it; from B, an rtnetlink request to the route socket with a portid in B's namespace, then one to the
/// kernel; B deleted. Expect: B's `/proc/net/netlink` lists one route socket with a portid, this process's (the
/// node agent's) by its inode; B's send to it refused with EPERM, since NETLINK_ROUTE takes a message to another
/// socket only from one with CAP_NET_ADMIN over the namespace's owner (Linux v6.19 net/core/rtnetlink.c:7038;
/// net/netlink/af_netlink.c:1844-1846, :904-908), and B's send to the kernel gone (non-vacuity); the socket's
/// queue and drops 0 before and after; and the namespace kept again at its next check.
fn a_box_can_not_send_to_the_socket_the_check_keeps_in_its_namespace(lab: &Lab, own: &Path) {
    if lab.rootless {
        eprintln!("SKIP netns kept socket: a rootless node keeps no network namespaces");
        return;
    }
    let (node, _state) = lab.node(&own.join("hex-reuse-netsock"));
    let o = BoxOwner::named("owner-netsock").unwrap();
    let wait = ["/hex", "selftest-box", "wait"];
    let make = |id: &str| BoxSpec {
        id: BoxId::new(id).unwrap(),
        bounds: Bounds::default(),
        class: ClassSpec::Bundle(lab.bundle_with(Caps::Docker, &wait, true, true)),
        owner: Some(o.clone()),
    };
    let counts = || {
        node.netns_reuse()
            .unwrap()
            .expect("a root node keeps namespaces")
    };
    let ruled = || {
        wait_for(
            || {
                let c = counts();
                c.checked + c.unchecked == c.given_back
            },
            "the checker's ruling on every namespace given back",
        );
        counts()
    };
    node.create(make("sock-a")).unwrap();
    node.delete(&BoxId::new("sock-a").unwrap()).unwrap();
    let kept = ruled();
    assert_eq!(
        kept.pool.idle, 1,
        "A's namespace is checked and kept for O: {kept:?}"
    );
    node.create(make("sock-b")).unwrap();
    assert_eq!(
        counts().pool.handed_on,
        kept.pool.handed_on + 1,
        "B is given A's namespace"
    );
    let b = BoxId::new("sock-b").unwrap();
    let procs = std::fs::read_to_string(node.cgroup_dir(&b).unwrap().join("cgroup.procs")).unwrap();
    let init = procs.lines().next().expect("B's init").to_owned();
    let route_sockets = || -> Vec<NetlinkSocket> {
        netlink_sockets(&init)
            .into_iter()
            .filter(|s| s.protocol == 0 && s.portid != 0)
            .collect()
    };
    let before = route_sockets();
    assert_eq!(
        before.len(),
        1,
        "B's namespace holds one route socket with a portid: {before:?}"
    );
    let socket = before[0];
    let theirs = format!("socket:[{}]", socket.inode);
    assert!(
        std::fs::read_dir("/proc/self/fd").unwrap().any(|fd| {
            std::fs::read_link(fd.unwrap().path()).is_ok_and(|to| to.as_os_str() == theirs.as_str())
        }),
        "the socket at portid {} is this process's, the node agent's",
        socket.portid
    );
    assert_eq!((socket.rmem, socket.drops), (0, 0), "{socket:?}");
    let portid = socket.portid.to_string();
    assert_eq!(
        run(
            &node,
            &b,
            Program::of(&["/hex", "selftest-box", "netlink-to", &portid])
        ),
        Exit::Code(1),
        "B's send to the node's socket is refused, EPERM"
    );
    assert_eq!(
        run(
            &node,
            &b,
            Program::of(&["/hex", "selftest-box", "netlink-to", "0"])
        ),
        Exit::Code(0),
        "B's send to the kernel goes"
    );
    assert_eq!(
        route_sockets(),
        before,
        "the node's socket as it was: nothing queued, nothing dropped"
    );
    node.delete(&b).unwrap();
    let after = ruled();
    assert_eq!(
        after.pool.idle, 1,
        "the namespace passes its next check, kept for O: {after:?}"
    );
    eprintln!(
        "netns, kept socket: B sees the node's at portid {portid}, its send to it refused (EPERM), its send to the kernel gone, the socket unchanged, the namespace kept again"
    );
    node.release_idle().unwrap();
}

/// A netlink socket as `/proc/<pid>/net/netlink` lists it, in that process's network namespace (Linux v6.19
/// net/netlink/af_netlink.c:2703, `netlink_native_seq_show`: sk, Eth, Pid, Groups, Rmem, Wmem, Dump, Locks, Drops,
/// Inode): its protocol, its portid, the bytes queued for it to read, its drops and its inode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NetlinkSocket {
    protocol: u32,
    portid: u32,
    rmem: u64,
    drops: u64,
    inode: u64,
}

fn netlink_sockets(pid: &str) -> Vec<NetlinkSocket> {
    std::fs::read_to_string(format!("/proc/{pid}/net/netlink"))
        .unwrap()
        .lines()
        .skip(1)
        .map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            NetlinkSocket {
                protocol: f[1].parse().unwrap(),
                portid: f[2].parse().unwrap(),
                rmem: f[4].parse().unwrap(),
                drops: f[8].parse().unwrap(),
                inode: f[9].parse().unwrap(),
            }
        })
        .collect()
}

/// Runs `f` on a thread of its own in process `pid`'s network namespace, and waits for it.
fn in_netns(pid: &str, f: impl FnOnce() + Send) {
    let ns = std::fs::File::open(format!("/proc/{pid}/ns/net")).unwrap();
    std::thread::scope(|scope| {
        scope
            .spawn(move || {
                rustix::thread::move_into_link_name_space(
                    std::os::fd::AsFd::as_fd(&ns),
                    Some(rustix::thread::LinkNameSpaceType::Network),
                )
                .unwrap();
                f();
            })
            .join()
            .unwrap();
    });
}

/// A TCP connection on loopback whose client closes first, so it lingers in TIME_WAIT
/// (include/net/tcp.h:141, 60 s).
fn time_wait() {
    use std::io::Read as _;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let client = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (mut server, _) = listener.accept().unwrap();
    client.shutdown(std::net::Shutdown::Both).unwrap();
    drop(client);
    // The server reads the client's end, then closes its own: the client's side is left in TIME_WAIT.
    let mut sink = Vec::new();
    let _ = server.read_to_end(&mut sink);
    drop(server);
}

/// Udp's OutDatagrams in a `/proc/net/snmp`: its "Udp:" lines, names then values.
fn udp_out(snmp: &str) -> u64 {
    let mut udp = snmp.lines().filter(|l| l.starts_with("Udp:"));
    let (names, values) = (udp.next().unwrap(), udp.next().unwrap());
    let at = names
        .split_whitespace()
        .position(|n| n == "OutDatagrams")
        .unwrap();
    values.split_whitespace().nth(at).unwrap().parse().unwrap()
}

/// Sends one rtnetlink request on a socket of the calling thread's namespace and wants its ack.
fn rtnetlink(request: Vec<u8>) {
    use rustix::net::{AddressFamily, RecvFlags, SendFlags, SocketFlags, SocketType};
    let fd = rustix::net::socket_with(
        AddressFamily::NETLINK,
        SocketType::RAW,
        SocketFlags::CLOEXEC,
        None,
    )
    .unwrap();
    rustix::net::send(&fd, &request, SendFlags::empty()).unwrap();
    let mut answer = [0u8; 1024];
    let (got, _) = rustix::net::recv(&fd, &mut answer, RecvFlags::empty()).unwrap();
    let error = i32::from_ne_bytes(answer[16..20].try_into().unwrap());
    assert!(got >= 20 && error == 0, "the kernel refused it: {error}");
}

/// A netlink request of `kind` (`NLM_F_REQUEST | NLM_F_ACK | NLM_F_CREATE | NLM_F_EXCL`) with `body`.
fn nl_request(kind: u16, body: &[u8]) -> Vec<u8> {
    let mut m = Vec::new();
    m.extend_from_slice(&u32::try_from(16 + body.len()).unwrap().to_ne_bytes());
    m.extend_from_slice(&kind.to_ne_bytes());
    m.extend_from_slice(&(0x1u16 | 0x4 | 0x400 | 0x200).to_ne_bytes());
    m.extend_from_slice(&1u32.to_ne_bytes());
    m.extend_from_slice(&0u32.to_ne_bytes());
    m.extend_from_slice(body);
    m
}

fn nl_attr(kind: u16, value: &[u8]) -> Vec<u8> {
    let mut a = Vec::new();
    a.extend_from_slice(&u16::try_from(4 + value.len()).unwrap().to_ne_bytes());
    a.extend_from_slice(&kind.to_ne_bytes());
    a.extend_from_slice(value);
    while a.len() % 4 != 0 {
        a.push(0);
    }
    a
}

/// `RTM_NEWADDR` (20): 10.11.12.13/32 on loopback (interface 1), `IFA_LOCAL` (2) and `IFA_ADDRESS` (1).
fn new_address() -> Vec<u8> {
    let mut body = vec![2u8, 32, 0, 0];
    body.extend_from_slice(&1u32.to_ne_bytes());
    body.extend(nl_attr(2, &[10, 11, 12, 13]));
    body.extend(nl_attr(1, &[10, 11, 12, 13]));
    nl_request(20, &body)
}

/// `RTM_NEWROUTE` (24): 10.20.0.0/16 out of loopback in the main table (254), unicast, link scope.
fn new_route() -> Vec<u8> {
    // struct rtmsg: family, dst_len, src_len, tos, table, protocol (static, 4), scope (link, 253),
    // type (unicast, 1), flags.
    let mut body = vec![2u8, 16, 0, 0, 254, 4, 253, 1];
    body.extend_from_slice(&0u32.to_ne_bytes());
    body.extend(nl_attr(1, &[10, 20, 0, 0]));
    body.extend(nl_attr(4, &1u32.to_ne_bytes()));
    nl_request(24, &body)
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

    // B of another owner: never A's, a fresh cgroup, counted, and A's kept in O's pool for O's next box
    // (Ada's call, 2026-11-03: keep, not the plan's "A's is removed").
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
    assert!(slot.exists(), "A's cgroup is kept, not removed");
    assert_eq!(
        after.pool.idle, before.pool.idle,
        "A's stays idle in O's pool"
    );
    node.create(lab.oci("other-c", Some(&o), Caps::Docker, Bounds::default()))
        .unwrap();
    assert_eq!(
        node.cgroup_dir(&BoxId::new("other-c").unwrap()).as_deref(),
        Some(slot.as_path()),
        "A's cgroup goes to its own owner's next box"
    );
    eprintln!("test 3, another owner: B fresh, counted, A's kept for O and handed to O's next box");
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
