//! A program started in a box, watched for its end. macOS has no pidfd, so a [`BoxProc`] waits on its
//! child through a kqueue `EVFILT_PROC`/`NOTE_EXIT` (the fact, never a wall-clock guess, CLAUDE.md §4) and
//! reaps it with `waitpid`. The node is the child's parent (posix_spawn makes a direct child), so its pid
//! can't be confused with another process's until this reaps it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use hex_api::{BoxId, Exit, Proc, Refusal, Signal};
use rustix::process::{Pid, WaitId, WaitIdOptions};

use super::helper_client::Helper;
use super::process::Members;
use super::sys::{Event, Kqueue};

/// A program the node spawned into a box, by pid.
pub struct BoxProc {
    pid: i32,
    kq: Kqueue,
    /// The process box this program runs in (none for a box-user box, whose helper counts its ends), so reaping a kill at the fatal cap counts it on the box at once
    /// (native plan step 1's `_ext` backstop) — before a caller that waited on this reads the box's usage.
    members: Option<Members>,
    exit: Option<Exit>,
    /// Whether its end is under way: the kernel refused its watch as already exiting (`ESRCH`), or a wait
    /// took its exit event. Its reap then waits for the zombie the same exit path makes.
    exiting: bool,
    events: Vec<Event>,
}

impl BoxProc {
    /// Watches child `pid` for its end, in process box `members` (none for a box-user box). It must be this process's child and not yet
    /// reaped, so its pid is this program's until [`BoxProc`] reaps it.
    pub fn watch(pid: i32, members: Option<Members>) -> Result<Self, Refusal> {
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
        // Peek first (NOWAIT): the zombie's lifetime peak is readable only until the reap, and it tells a cap
        // kill from another hand's SIGKILL (State::killed_by_cap).
        let peak = match rustix::process::waitid(WaitId::Pid(pid), options | WaitIdOptions::NOWAIT)
        {
            Ok(Some(_)) => super::sys::rusage(self.pid).ok().flatten().map(|r| r.peak),
            _ => None,
        };
        match rustix::process::waitid(WaitId::Pid(pid), options) {
            Ok(Some(status)) => {
                let exit = exit_of(&status);
                // A SIGKILL of a box member is the kernel's fatal cap (or memory pressure on it): tell the
                // box before this reap returns, so the count is there when a caller that waited reads
                // usage. The box ignores it unless the member was armed and it wasn't the box's own kill.
                let sigkill = matches!(exit, Exit::Signal(s) if s == Signal::KILL.number());
                self.exit = Some(exit);
                if let Some(members) = &self.members {
                    members.ended(self.pid, sigkill, peak)?;
                }
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

    /// Takes the exit event its watch holds, if any, then reaps: at once either way, since an exit event
    /// means the zombie is being made on the same exit path.
    fn try_wait(&mut self) -> Result<Option<Exit>, Refusal> {
        if !self.exiting && self.exit.is_none() {
            self.kq
                .wait(None, Some(Duration::ZERO), &mut self.events)
                .map_err(|e| Refusal::os("waiting on a program", e))?;
            self.exiting = ended_in(&self.events, self.pid);
        }
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
            self.exiting = self.exiting || ended_in(&self.events, self.pid);
        }
    }
}

/// Whether `events` carry `pid`'s end: its exit, or a fork coalesced with it.
fn ended_in(events: &[Event], pid: i32) -> bool {
    events.iter().any(|e| match e {
        Event::Exit { pid: p, .. }
        | Event::Fork {
            pid: p,
            exited: true,
            ..
        } => *p == pid,
        _ => false,
    })
}

/// A program the box helper started in a box-user box (native plan step 2). The helper is its parent, so
/// only the helper can reap it: `waitid` answers `ECHILD` to any other caller
/// (`bsd/kern/kern_exit.c:3194-3361`), which a [`BoxProc`] would read as an end at once. Any process may
/// watch another's `NOTE_EXIT` (`filt_procattach` checks only `NOTE_EXITSTATUS`,
/// `bsd/kern/kern_event.c:1116-1133`), so the node waits on the fact on a kqueue of its own, then asks the
/// helper for the end it reaped. A watch attached once the program is past its `NOTE_EXIT` is refused
/// (`ESRCH`): the exit path hides it from `proc_find` (`P_REF_DEAD`, `kern_exit.c:2127-2129`, in
/// `proc_prepareexit`) before `proc_exit` posts the event (:2561), so no end is missed between the two.
pub struct UserProc {
    pid: i32,
    box_id: BoxId,
    helper: Arc<Helper>,
    kq: Kqueue,
    exit: Option<Exit>,
    /// Whether its end is under way: its exit event was taken, or its watch was refused as exiting.
    exiting: bool,
    events: Vec<Event>,
}

impl UserProc {
    /// Watches `pid`, which the helper just started in `box_id`, for its end.
    pub fn watch(pid: i32, box_id: BoxId, helper: Arc<Helper>) -> Result<Self, Refusal> {
        let kq = Kqueue::new().map_err(|e| Refusal::os("making a program's kqueue", e))?;
        // NOTE_EXIT alone: the node isn't its parent, and XNU refuses another's NOTE_EXITSTATUS.
        let exiting = kq
            .watch_end(pid)
            .map_err(|e| Refusal::os("watching a program", e))?;
        Ok(Self {
            pid,
            box_id,
            helper,
            kq,
            exit: None,
            exiting,
            events: Vec::with_capacity(1),
        })
    }

    /// Its end from the helper once it is under way; `None` before.
    fn ask(&mut self) -> Result<Option<Exit>, Refusal> {
        if self.exit.is_some() || !self.exiting {
            return Ok(self.exit);
        }
        let exit = match self.helper.wait(&self.box_id, self.pid) {
            Ok(Some(exit)) => exit,
            Ok(None) => {
                return Err(Refusal::Broken(format!(
                    "the helper holds program {} as running after its exit event",
                    self.pid
                )));
            }
            // Its box and the ends it kept were dropped (a delete long past): no one holds its end now.
            Err(Refusal::NoSuchBox(_)) => Exit::Unknown,
            Err(e) => return Err(e),
        };
        self.exit = Some(exit);
        Ok(self.exit)
    }

    /// Takes the events waiting on its kqueue for at most `timeout`, noting its end among them.
    fn take(&mut self, timeout: Duration) -> Result<(), Refusal> {
        self.kq
            .wait(None, Some(timeout), &mut self.events)
            .map_err(|e| Refusal::os("waiting on a program", e))?;
        self.exiting = self.exiting || ended_in(&self.events, self.pid);
        Ok(())
    }
}

impl Proc for UserProc {
    fn pid(&self) -> u32 {
        u32::try_from(self.pid).unwrap_or(0)
    }

    fn try_wait(&mut self) -> Result<Option<Exit>, Refusal> {
        if !self.exiting {
            self.take(Duration::ZERO)?;
        }
        self.ask()
    }

    fn wait_for(&mut self, timeout: Duration) -> Result<Option<Exit>, Refusal> {
        let deadline = Instant::now().checked_add(timeout);
        loop {
            if let Some(exit) = self.ask()? {
                return Ok(Some(exit));
            }
            let left = match deadline {
                Some(d) => d.saturating_duration_since(Instant::now()),
                None => timeout,
            };
            if left.is_zero() {
                return Ok(None);
            }
            self.take(left)?;
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
