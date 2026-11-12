//! AC-T7.4.1 (ROOT item 3, the CPU-held scope's exec'd member): a managed scope's member that its watcher
//! holds execs between the hold's token capture and a later signal of that hold. XNU gives a process a new
//! audit-token pid version at exec, so the token names an image the member no longer runs. Each case drives
//! the watcher's own steps in their order (`apply`'s hold, `run_one`'s continue for a read, `stop_again`) on
//! a real member, the scope's group leader, and expects the same process (pid and birth) signalled, held as
//! before, and its scope still Running. And one that ends while held (its zombie unreaped, as a scope's
//! controller's stays until finalize) is let go of, never taken for a process outside the scope.
use super::scope_completion_tests::{Birth, unobserved_box};
use super::*;
use std::io::{BufRead as _, Write as _};
use std::os::unix::process::CommandExt as _;
use std::process::{Child, ChildStdin, Command, Stdio};

/// Longest a test waits for a fact it needs: a bound, so a member that never makes it true fails the
/// test instead of hanging it.
const PATIENCE: Duration = Duration::from_secs(30);

/// A held member: a shell that says it runs, and given a line execs a shell that says so and then waits on
/// its own stdin.
struct Held {
    child: Child,
    stdin: ChildStdin,
    said: std::sync::mpsc::Receiver<String>,
    boxed: ProcessBox,
    pid: i32,
    started: u64,
}

impl Held {
    /// The member started in a process group of its own, its scope's box built over it, and, once its
    /// first shell says it runs (past its own start's exec), the watcher's hold applied: stopped, its token
    /// captured from that shell.
    fn new() -> Self {
        let mut child = Command::new("/bin/sh")
            .args([
                "-c",
                "echo ready; read line; exec /bin/sh -c 'echo execd; read more'",
            ])
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, said) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                if sender.send(line).is_err() {
                    return;
                }
            }
        });
        assert_eq!(said.recv_timeout(PATIENCE).unwrap(), "ready");
        let pid = i32::try_from(child.id()).unwrap();
        let started = sys::rusage(pid).unwrap().unwrap().started;
        let boxed = unobserved_box(&Birth {
            pid,
            started,
            session: 0,
        });
        let member = Member {
            pid: child.id(),
            started,
            footprint: 0,
            life: Life::Running,
            armed: false,
        };
        assert_eq!(boxed.state.apply(&[child.id()], &[], &[member]).unwrap(), 1);
        let held = Self {
            child,
            stdin,
            said,
            boxed,
            pid,
            started,
        };
        held.wait_until(sys::Run::Stopped);
        held
    }

    #[track_caller]
    fn wait_until(&self, run: sys::Run) {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let now = sys::run_state(self.pid).unwrap();
            if now == Some(run) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the member never reached {run:?}: {now:?}"
            );
            std::thread::park_timeout(Duration::from_millis(1));
        }
    }

    /// The watcher's group listing, with room for every process this kernel allows, as its own has.
    fn listing() -> Vec<i32> {
        storage(sys::max_processes().unwrap()).unwrap()
    }

    /// Its line, sent while it is held, then the watcher's continue for a read: it execs, and says so.
    fn exec_in_its_read(&mut self) {
        self.stdin.write_all(b"go\n").unwrap();
        let mut pids = Self::listing();
        let run = self
            .boxed
            .state
            .run_one(self.pid, &mut pids, 0)
            .unwrap()
            .expect("the held member runs for the read");
        assert_eq!(run.pid, self.pid);
        assert_eq!(self.said.recv_timeout(PATIENCE).unwrap(), "execd");
    }

    /// The same process, held by the watcher, and its scope still Running (not Uncertain).
    fn still_held(&self) {
        assert_eq!(
            sys::rusage(self.pid).unwrap().map(|usage| usage.started),
            Some(self.started),
            "the member isn't the same process"
        );
        assert!(self.boxed.state.held().unwrap().stopped.contains(&self.pid));
        let scope = self.boxed.state.vm.as_ref().unwrap();
        assert!(
            scope.watchable(self.boxed.state.id.as_ref()).unwrap(),
            "the scope left Running"
        );
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The member execs while it runs for its read; the watcher then stops it again with the hold's token:
/// expect it stopped, the same process, still held, and the scope Running.
#[test]
fn a_held_member_that_execs_in_its_read_is_stopped_again() {
    let mut held = Held::new();
    held.exec_in_its_read();
    let again = held.boxed.state.stop_again(held.pid);
    assert!(again.is_ok(), "{again:?}");
    held.wait_until(sys::Run::Stopped);
    held.still_held();
}

/// The member execs in its read and its stop lands only after the exec (as a stop posted while the
/// kernel runs an exec does): the watcher's next continue uses the hold's token. Expect it continued,
/// the same process, still held, and the scope Running.
#[test]
fn a_held_member_stopped_after_its_exec_is_continued_again() {
    let mut held = Held::new();
    held.exec_in_its_read();
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(held.pid).unwrap(),
        rustix::process::Signal::STOP,
    )
    .unwrap();
    held.wait_until(sys::Run::Stopped);
    let mut pids = Held::listing();
    let run = held.boxed.state.run_one(held.pid, &mut pids, 0);
    assert!(run.is_ok(), "{:?}", run.map(|ran| ran.map(|ran| ran.pid)));
    held.wait_until(sys::Run::Live);
    held.still_held();
}

/// The held member ends (killed; its zombie unreaped, its parent this test, as the helper keeps a scope's
/// controller until finalize) and the watcher's read continues the held: expect no run, the member no
/// longer held, and the scope Running. A zombie is no process group's (`getpgid` finds no process), so a
/// signal fenced on the group would take it for a process outside the scope.
#[test]
fn a_held_member_that_ended_is_no_longer_held() {
    let mut held = Held::new();
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(held.pid).unwrap(),
        rustix::process::Signal::KILL,
    )
    .unwrap();
    // Its zombie: proc_pid_rusage reads one (proc_pidinfo's run state finds none).
    let deadline = Instant::now() + PATIENCE;
    while !sys::rusage(held.pid)
        .unwrap()
        .is_some_and(|usage| usage.ended && usage.started == held.started)
    {
        assert!(Instant::now() < deadline, "the member never ended");
        std::thread::park_timeout(Duration::from_millis(1));
    }
    let mut pids = Held::listing();
    let run = held.boxed.state.run_one(held.pid, &mut pids, 0);
    assert!(
        matches!(run, Ok(None)),
        "{:?}",
        run.map(|ran| ran.map(|ran| ran.pid))
    );
    assert!(!held.boxed.state.held().unwrap().stopped.contains(&held.pid));
    let scope = held.boxed.state.vm.as_ref().unwrap();
    assert!(scope.watchable(held.boxed.state.id.as_ref()).unwrap());
}
