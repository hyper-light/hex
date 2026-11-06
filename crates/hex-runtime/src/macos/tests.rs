//! The box-user backend end to end, without root (HEX-MAC-16): a `MacRuntime` in box-user mode, driven
//! through the one `Runtime` trait, reaches a real daemon over a real Unix socket, and the daemon's
//! privileged spawn/arm/kill are a stand-in (no setuid, no ledger — those are measured as root by
//! `bench/native/macos/boxlab.sh`). It exercises the node side: mode detection, create→acquire a pool
//! user, exec→start as that user (stdio by `SCM_RIGHTS`), inspect, list, delete, and that inspect reports
//! box-user membership.

use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader};
use std::num::NonZeroU64;
use std::os::fd::{AsFd, OwnedFd};
use std::process::{Child, Command, Stdio as Piped};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use hex_api::{
    Bounds, BoxId, BoxSpec, BoxState, ClassSpec, Exit, Membership, Proc, Program, Refusal, Runtime,
    Stdio,
};
use hex_core::helper::START_FDS;

use super::MacRuntime;
use super::helper_daemon::{self, Daemon, Member, Privileged};

/// A stand-in for the daemon's privileged backend: it tracks a box's members in memory instead of
/// spawning or arming real processes, so the node→daemon path runs without root or side effects.
#[derive(Default)]
struct Sim {
    inner: Arc<Mutex<SimState>>,
}

#[derive(Default)]
struct SimState {
    next: i32,
    members: Vec<Member>,
}

impl Privileged for Sim {
    fn spawn(
        &self,
        _uid: u32,
        _program: &Program,
        fds: [OwnedFd; 3],
        _leader: bool,
        _cap_mib: u32,
        _nproc: u32,
    ) -> io::Result<i32> {
        drop(fds);
        let mut s = self.inner.lock().expect("sim lock");
        s.next += 1;
        let pid = 2000 + s.next;
        s.members.push(Member { pid, footprint: 0 });
        Ok(pid)
    }

    fn watch(&self, pid: i32) -> io::Result<Box<dyn Proc>> {
        Ok(Box::new(SimProc {
            pid,
            state: Arc::clone(&self.inner),
        }))
    }

    fn arm(&self, _pid: i32, _cap_mib: u32) -> io::Result<()> {
        Ok(())
    }

    fn list(&self, _uid: u32) -> io::Result<Vec<Member>> {
        Ok(self.inner.lock().expect("sim lock").members.clone())
    }

    fn kill(&self, _uid: u32, signal: i32) -> io::Result<usize> {
        let mut s = self.inner.lock().expect("sim lock");
        let n = s.members.len();
        if signal == 9 {
            s.members.clear();
        }
        Ok(n)
    }
}

/// The stand-in's watch: a program has ended (by KILL, the stand-in's only end) once it is no member.
struct SimProc {
    pid: i32,
    state: Arc<Mutex<SimState>>,
}

impl Proc for SimProc {
    fn pid(&self) -> u32 {
        u32::try_from(self.pid).unwrap_or(0)
    }

    fn try_wait(&mut self) -> Result<Option<Exit>, Refusal> {
        let s = self.state.lock().expect("sim lock");
        if s.members.iter().any(|m| m.pid == self.pid) {
            Ok(None)
        } else {
            Ok(Some(Exit::Signal(9)))
        }
    }

    fn wait_for(&mut self, _timeout: Duration) -> Result<Option<Exit>, Refusal> {
        self.try_wait()
    }
}

/// HEX-MAC-16: a box-user MacRuntime drives a box through the helper over a real socket.
#[test]
fn box_user_mode_routes_through_the_helper() {
    let dir = tempfile::tempdir().expect("temp dir");
    let sock = dir.path().join("helper.sock");
    // Bind before connecting, so the node's connect can't race the listener.
    let listener = helper_daemon::bind(&sock).expect("bind the helper socket");
    let server = thread::spawn(move || {
        let daemon = Daemon::new(Sim::default(), vec![7000, 7001], 64);
        let _ = daemon.serve_listener(listener.as_fd());
    });

    let rate = NonZeroU64::new(256 << 30).expect("nonzero");
    let rt = MacRuntime::detect(rate, rate, &sock).expect("detect box-user mode");
    let id = BoxId::new("b1").expect("box id");

    let spec = BoxSpec {
        id: id.clone(),
        bounds: Bounds {
            memory_max: Some(256 << 20),
            ..Bounds::default()
        },
        class: ClassSpec::Process,
    };
    let created = rt.create(spec).expect("create");
    assert_eq!(created.membership, Some(Membership::BoxUser));
    assert_eq!(created.state, BoxState::Empty);

    let program = Program {
        args: vec!["/bin/true".to_owned()],
        env: vec![],
        cwd: None,
        user: None,
    };
    let proc = rt
        .exec(
            &id,
            program,
            Stdio {
                stdin: None,
                stdout: None,
                stderr: None,
            },
        )
        .expect("exec");
    assert!(proc.pid() > 2000, "the sim's pid came back");

    let info = rt.inspect(&id).expect("inspect");
    assert_eq!(info.membership, Some(Membership::BoxUser));
    assert_eq!(info.state, BoxState::Running);
    assert_eq!(rt.list().expect("list").len(), 1);

    rt.delete(&id).expect("delete");
    assert!(matches!(rt.inspect(&id), Err(Refusal::NoSuchBox(_))));

    drop(rt);
    // The daemon thread parks on accept once the node disconnects; it ends with the test process.
    drop(server);
}

/// A backend whose programs are real but children of a shell of their own, never of this process: as a
/// box-user program is the root helper's child, not the node's. Each spawn starts
/// `sh -c '"$@" & echo $!; wait $!; echo $?'`, reads the program's pid, and a thread reads its status.
#[derive(Default)]
struct Shells {
    ends: Mutex<BTreeMap<i32, mpsc::Receiver<Exit>>>,
    live: Mutex<Vec<i32>>,
}

struct ShellProc {
    pid: i32,
    end: mpsc::Receiver<Exit>,
}

impl Proc for ShellProc {
    fn pid(&self) -> u32 {
        u32::try_from(self.pid).unwrap_or(0)
    }

    fn try_wait(&mut self) -> Result<Option<Exit>, Refusal> {
        Ok(self.end.try_recv().ok())
    }

    /// The helper asks only once the node saw the program's exit, and the real reap then waits for the
    /// zombie (`proc::BoxProc`); so does this, up to a bound past any test's run.
    fn wait_for(&mut self, _timeout: Duration) -> Result<Option<Exit>, Refusal> {
        Ok(self.end.recv_timeout(Duration::from_secs(30)).ok())
    }
}

impl Privileged for Shells {
    fn spawn(
        &self,
        _uid: u32,
        program: &Program,
        _fds: [OwnedFd; START_FDS],
        _leader: bool,
        _cap_mib: u32,
        _nproc: u32,
    ) -> io::Result<i32> {
        let mut shell: Child = Command::new("/bin/sh")
            .args([
                "-c",
                "\"$@\" </dev/null >/dev/null 2>&1 & echo $!; wait $!; echo $?",
                "sh",
            ])
            .args(&program.args)
            .stdout(Piped::piped())
            .spawn()?;
        let mut lines = BufReader::new(shell.stdout.take().expect("piped")).lines();
        let pid: i32 = lines
            .next()
            .transpose()?
            .and_then(|l| l.trim().parse().ok())
            .ok_or_else(|| io::Error::other("the shell named no pid"))?;
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let status: i32 = lines
                .next()
                .and_then(Result::ok)
                .and_then(|l| l.trim().parse().ok())
                .unwrap_or(-1);
            let _ = shell.wait();
            let _ = tx.send(if status > 128 {
                Exit::Signal(status - 128)
            } else {
                Exit::Code(status)
            });
        });
        self.ends.lock().unwrap().insert(pid, rx);
        self.live.lock().unwrap().push(pid);
        Ok(pid)
    }

    fn watch(&self, pid: i32) -> io::Result<Box<dyn Proc>> {
        let end = self
            .ends
            .lock()
            .unwrap()
            .remove(&pid)
            .ok_or_else(|| io::Error::other("no such program"))?;
        Ok(Box::new(ShellProc { pid, end }))
    }

    fn arm(&self, _pid: i32, _cap_mib: u32) -> io::Result<()> {
        Ok(())
    }

    fn list(&self, _uid: u32) -> io::Result<Vec<Member>> {
        Ok(Vec::new())
    }

    fn kill(&self, _uid: u32, _signal: i32) -> io::Result<usize> {
        Ok(0)
    }
}

/// HEX-RT-3, AC-T7.1.1 (box-user mode, end to end): a node in box-user mode, through a daemon on a real
/// socket, starts programs that are another process's children, as the root helper's are. Do run one that
/// sleeps, then one that exits 3; expect the first not ended while it runs and both ends as they were,
/// never an end the node made up because it couldn't reap a program it doesn't parent.
#[test]
fn a_box_user_programs_end_is_its_own_not_unknown() {
    let dir = tempfile::tempdir().expect("temp dir");
    let sock = dir.path().join("helper.sock");
    let listener = helper_daemon::bind(&sock).expect("bind");
    thread::spawn(move || {
        let daemon = Daemon::new(Shells::default(), vec![7000], 64);
        let _ = daemon.serve_listener(listener.as_fd());
    });
    let rt = MacRuntime::detect(
        NonZeroU64::new(1 << 40).unwrap(),
        NonZeroU64::new(1_000_000).unwrap(),
        &sock,
    )
    .expect("a box-user runtime");
    let id = BoxId::new("bu-end").unwrap();
    rt.create(BoxSpec {
        id: id.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Process,
    })
    .expect("create");
    let sh = |script: &str| Program {
        args: vec!["/bin/sh".to_owned(), "-c".to_owned(), script.to_owned()],
        env: vec![],
        cwd: None,
        user: None,
    };
    let mut sleeper = rt.exec(&id, sh("sleep 0.5"), Stdio::null()).expect("exec");
    assert_eq!(
        sleeper.try_wait().unwrap(),
        None,
        "a running program read as ended"
    );
    assert_eq!(
        sleeper.wait_for(Duration::from_secs(10)).unwrap(),
        Some(Exit::Code(0))
    );
    let mut three = rt.exec(&id, sh("exit 3"), Stdio::null()).expect("exec");
    assert_eq!(
        three.wait_for(Duration::from_secs(10)).unwrap(),
        Some(Exit::Code(3))
    );
}

/// A box-user program runs as its pool user, which the unprivileged node may not signal, and XNU attaches
/// `NOTE_EXITSTATUS` only for a parent or one that may (`kern_event.c:1116-1133`, `EACCES` else), so the
/// node's watch of it asks for `NOTE_EXIT` alone (HEX-MAC-16, HEX-RT-3). launchd (pid 1) stands in for a
/// process this one may not signal. Do watch it with the members' status watch, then for its end alone;
/// expect the first refused (unless root) and the second taken, launchd not ended.
#[test]
fn a_process_this_one_may_not_signal_is_watched_for_its_end_alone() {
    let kq = super::sys::Kqueue::new().expect("kqueue");
    if !rustix::process::geteuid().is_root() {
        let refused = kq
            .watch(&[1], 0)
            .expect_err("a status watch of launchd taken unprivileged");
        assert_eq!(refused.raw_os_error(), Some(libc::EACCES));
    }
    assert!(!kq.watch_end(1).expect("an end watch of launchd"));
}

/// The box helper's default process cap is the platform's per-uid ceiling, read at run time (Ada's rule,
/// 2026-11-06: no picked defaults; `kern.maxprocperuid`, `bsd/kern/kern_resource.c:169-170`). Do read it;
/// expect a positive count no larger than the kernel's whole process table (`kern.maxproc`).
#[test]
fn the_per_uid_process_ceiling_is_read_from_the_kernel() {
    let per_uid = super::sys::max_processes_per_uid().expect("kern.maxprocperuid");
    let all = super::sys::max_processes().expect("kern.maxproc");
    assert!(per_uid > 0 && per_uid <= all, "{per_uid} of {all}");
}

/// Ada's rule (2026-11-06): no picked number stands in for the kernel's. A process or user listing is sized
/// by `kern.maxproc`; when that can't be read, the listing is refused naming it (CLAUDE.md §3), never sized
/// by a guess (the 4096 step 1's class and the helper fell back to). Do size a listing from a failed read
/// and from a read of 7; expect an error naming kern.maxproc, and room for 7.
#[test]
fn a_listing_is_sized_by_kern_maxproc_or_refused() {
    let failed = super::sys::listing_from(Err(std::io::Error::from_raw_os_error(libc::ENOENT)));
    let refused = failed.expect_err("a listing sized by a guess");
    assert!(refused.to_string().contains("kern.maxproc"), "{refused}");
    let sized = super::sys::listing_from(Ok(7)).expect("a listing for 7");
    assert!(sized.capacity() >= 7 && sized.is_empty());
}
