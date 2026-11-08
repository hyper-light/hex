//! A program started in a box, watched for its end. macOS has no pidfd, so a [`BoxProc`] waits on its
//! child through a kqueue `EVFILT_PROC`/`NOTE_EXIT` (the fact, never a wall-clock guess, CLAUDE.md §4) and
//! reaps it with `waitpid`. The node is the child's parent (posix_spawn makes a direct child), so its pid
//! can't be confused with another process's until this reaps it.

use std::time::{Duration, Instant};

use hex_api::{Exit, Proc, Refusal, Signal};
use rustix::process::{Pid, WaitId, WaitIdOptions};

use super::process::Members;
use super::sys::{Event, Kqueue};

/// A program the node spawned into a box, by pid.
pub struct BoxProc {
    pid: i32,
    kq: Kqueue,
    /// The box this program runs in, so reaping a kill at the fatal cap counts it on the box at once
    /// (native plan step 1's `_ext` backstop) — before a caller that waited on this reads the box's usage.
    members: Members,
    exit: Option<Exit>,
    /// Whether its end is under way: the kernel refused its watch as already exiting (`ESRCH`), or a wait
    /// took its exit event. Its reap then waits for the zombie the same exit path makes.
    exiting: bool,
    events: Vec<Event>,
}

impl BoxProc {
    /// Watches child `pid` for its end, in box `members`. It must be this process's child and not yet
    /// reaped, so its pid is this program's until [`BoxProc`] reaps it.
    pub fn watch(pid: i32, members: Members) -> Result<Self, Refusal> {
        let kq = Kqueue::new().map_err(|e| Refusal::os("making a program's kqueue", e))?;
        // Already exiting (or reaped by a box delete that beat this): no exit event will come, so its end is
        // read by a reap that waits for it, or is no longer knowable.
        let exiting = kq
            .watch(&[pid], 0)
            .map_err(|e| Refusal::os("watching a program", e))?
            .contains(&pid);
        Ok(Self {
            pid,
            kq,
            members,
            exit: None,
            exiting,
            events: Vec::with_capacity(1),
        })
    }

    pub fn pid_i32(&self) -> i32 {
        self.pid
    }

    /// Reaps the program if it has ended, caching its exit; `Ok(None)` while it still runs. With `exiting`
    /// (its exit event was delivered) it waits for the reap: XNU posts `NOTE_EXIT` before it makes the child
    /// a zombie (`bsd/kern/kern_exit.c:2594-2595`, `p_stat = SZOMB` at :2664, the same exit path), and the
    /// event fires once, so a reap that found nothing on it would wait out the caller's whole budget. An
    /// end its node can't collect (reaped elsewhere, e.g. by a box delete) is [`Exit::Unknown`].
    fn reap(&mut self, exiting: bool) -> Result<Option<Exit>, Refusal> {
        if self.exit.is_some() {
            return Ok(self.exit);
        }
        let Some(pid) = Pid::from_raw(self.pid) else {
            return Err(Refusal::Invalid {
                what: "pid",
                why: format!("{} isn't a process id", self.pid),
            });
        };
        let options = if exiting {
            WaitIdOptions::EXITED
        } else {
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG
        };
        match rustix::process::waitid(WaitId::Pid(pid), options) {
            Ok(Some(status)) => {
                let exit = exit_of(&status);
                // A SIGKILL of a box member is the kernel's fatal cap (or memory pressure on it): tell the
                // box before this reap returns, so the count is there when a caller that waited reads
                // usage. The box ignores it unless the member was armed and it wasn't the box's own kill.
                let sigkill = matches!(exit, Exit::Signal(s) if s == Signal::KILL.number());
                self.exit = Some(exit);
                self.members.ended(self.pid, sigkill)?;
                Ok(self.exit)
            }
            Ok(None) => Ok(None),
            // No such child: it was reaped elsewhere (a box delete). Its end is no longer knowable here.
            Err(rustix::io::Errno::CHILD) => {
                self.exit = Some(Exit::Unknown);
                Ok(self.exit)
            }
            Err(e) => Err(Refusal::os("waitid", e)),
        }
    }
}

impl Proc for BoxProc {
    fn pid(&self) -> u32 {
        u32::try_from(self.pid).unwrap_or(0)
    }

    fn try_wait(&mut self) -> Result<Option<Exit>, Refusal> {
        self.reap(self.exiting)
    }

    fn wait_for(&mut self, timeout: Duration) -> Result<Option<Exit>, Refusal> {
        let deadline = Instant::now().checked_add(timeout);
        loop {
            if let Some(exit) = self.reap(self.exiting)? {
                return Ok(Some(exit));
            }
            let left = match deadline {
                Some(d) => d.saturating_duration_since(Instant::now()),
                None => timeout,
            };
            if left.is_zero() {
                return Ok(None);
            }
            self.kq
                .wait(None, Some(left), &mut self.events)
                .map_err(|e| Refusal::os("waiting on a program", e))?;
            let me = self.pid;
            self.exiting = self.exiting
                || self.events.iter().any(|e| match e {
                    Event::Exit { pid, .. }
                    | Event::Fork {
                        pid, exited: true, ..
                    } => *pid == me,
                    _ => false,
                });
        }
    }

    /// KILL by pid: the program is this node's child and not yet reaped (a reaped one has its exit cached),
    /// so its pid is still its own, a zombie's at worst.
    fn kill(&mut self) -> Result<(), Refusal> {
        if self.reap(self.exiting)?.is_some() {
            return Ok(());
        }
        let Some(pid) = Pid::from_raw(self.pid) else {
            return Err(Refusal::Invalid {
                what: "pid",
                why: format!("{} isn't a process id", self.pid),
            });
        };
        self.members.killing(self.pid)?;
        match rustix::process::kill_process(pid, rustix::process::Signal::KILL) {
            // ESRCH: reaped elsewhere since (a box delete); it has ended.
            Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
            Err(e) => Err(Refusal::os("kill", e)),
        }
    }
}

/// How a program ended, from its wait status, with any terminating signal carried as Linux's number so a
/// caller reads every OS's boxes alike (CLAUDE.md §8).
fn exit_of(status: &rustix::process::WaitIdStatus) -> Exit {
    if let Some(sig) = status.terminating_signal() {
        Exit::Signal(hex_core::darwin::signal_from_macos(sig).unwrap_or(sig))
    } else if let Some(code) = status.exit_status() {
        Exit::Code(code)
    } else {
        Exit::Unknown
    }
}
