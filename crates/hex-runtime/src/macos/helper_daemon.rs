//! The macOS box helper daemon (native plan step 2). A box runs as a user of its own from a pool the
//! installer makes; the privileged launchd daemon starts each box's programs as that user, arms their
//! physical-footprint ledger limits and catches their Mach exceptions, and ends a box by killing every
//! process of its user. The node drives it over the [`hex_core::helper`] protocol.
//!
//! This module is the daemon's membership and bookkeeping: the live members of a box's pool user and
//! their footprints (`proc_listpids(PROC_UID_ONLY)` through [`super::sys::user_pids`]), whether the user
//! is empty (delete frees the user back to the pool only once it is), and killing every process of the
//! user. A box's process count is the kernel's — `RLIMIT_NPROC` of its user, set when the leader starts.
//! The privileged spawn and the ledger/exception arming are [`super::sys`]'s (`unsafe`, CLAUDE.md §12);
//! the apportionment is [`hex_core::boxuser`].

use std::collections::{BTreeMap, BTreeSet};
use std::io;

use hex_core::boxuser::{self, Bound, Use};
use hex_core::darwin;
use hex_core::helper::State;

use super::sys;

/// One live member of a box's user: its pid and its physical footprint in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Member {
    pub pid: i32,
    pub footprint: u64,
}

/// The box user's live members, each with its footprint, through `proc_listpids(PROC_UID_ONLY)` and
/// `proc_pid_rusage`. Finds every process of the user — including one that daemonized (`setsid`) out of
/// its process group, since a user id is inherited and can't be dropped unprivileged
/// (docs/research/runtime/native-platforms.md §3). A process gone since it was listed (`ESRCH`) or ended
/// and awaiting reap is left out; its footprint would be 0.
pub fn members(uid: u32) -> io::Result<Vec<Member>> {
    let mut pids = Vec::with_capacity(sys::max_processes().unwrap_or(4096));
    sys::user_pids(uid, &mut pids)?;
    let mut out = Vec::with_capacity(pids.len());
    for &pid in &pids {
        if let Some(r) = sys::rusage(pid)?
            && !r.ended
        {
            out.push(Member {
                pid,
                footprint: r.footprint,
            });
        }
    }
    Ok(out)
}

/// Whether the box's user has any live process. Delete frees the user back to the pool only once this is
/// true (no process of the user is left).
pub fn user_empty(uid: u32) -> io::Result<bool> {
    Ok(members(uid)?.is_empty())
}

/// Sends `signal` (a Linux signal number, mapped to macOS's as the rest of Hex maps them) to every
/// process of the box's user. Returns how many it signalled. Delete sends `KILL` then waits for
/// [`user_empty`]; [`hex_core::helper::Request::Signal`] sends the caller's signal.
pub fn kill_user(uid: u32, linux_signal: i32) -> io::Result<usize> {
    let macos = darwin::signal_to_macos(linux_signal)
        .ok_or_else(|| io::Error::other(format!("signal {linux_signal} has no macOS number")))?;
    let sig = rustix::process::Signal::from_named_raw(macos)
        .ok_or_else(|| io::Error::other(format!("{macos} isn't a signal this kernel names")))?;
    let mut pids = Vec::with_capacity(sys::max_processes().unwrap_or(4096));
    sys::user_pids(uid, &mut pids)?;
    let mut signalled = 0usize;
    for &pid in &pids {
        if let Some(p) = rustix::process::Pid::from_raw(pid) {
            match rustix::process::kill_process(p, sig) {
                Ok(()) => signalled = signalled.saturating_add(1),
                // Gone between the listing and the kill: not an error.
                Err(rustix::io::Errno::SRCH) => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    Ok(signalled)
}

/// What the daemon must do to a box's kernel limits after a read of its members: arm some at new ledger
/// limits, or end the box because its tree reached the bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// (Re)arm each `(pid, limit)`: a member new since the last read, or one whose apportioned share
    /// changed. A member whose share is unchanged keeps its current kernel limit and isn't re-armed.
    Arm(Vec<(i32, u64)>),
    /// The members' footprints reached what may be apportioned: end the box (its footprint and bound).
    End { footprint: u64, bound: u64 },
}

/// One box the helper manages: its pool user, its bound and reserve, and the ledger limit each member is
/// armed at now. The daemon apportions the bound across the box's members ([`hex_core::boxuser`]) and
/// arms the kernel limits ([`super::sys`], privileged); this tracks what is armed so a read only re-arms
/// what changed, and keeps the box's peak footprint and how often a member crossed its share.
#[derive(Debug, Clone)]
pub struct Tracked {
    uid: u32,
    bound: Bound,
    /// Each live member's pid and the ledger limit it is armed at.
    armed: BTreeMap<i32, u64>,
    peak: u64,
    crossings: u64,
    ended_over: bool,
}

impl Tracked {
    /// A box on pool user `uid`, bounded at `bound` bytes with `reserve` bytes held back for the fork
    /// arming window (`hex_core::boxuser::Bound`).
    pub fn new(uid: u32, bound: u64, reserve: u64) -> Self {
        Self {
            uid,
            bound: Bound {
                limit: bound,
                reserve,
            },
            armed: BTreeMap::new(),
            peak: 0,
            crossings: 0,
            ended_over: false,
        }
    }

    pub fn uid(&self) -> u32 {
        self.uid
    }

    /// Apportions the bound across `members` (their footprints now) and returns what to arm: the members
    /// new or whose share changed. Forgets members gone since the last read. Ends the box if the tree
    /// reached the apportionable bound (HEX-MAC-14).
    pub fn reapportion(&mut self, members: &[Member]) -> Plan {
        let footprint = members
            .iter()
            .fold(0u64, |s, m| s.saturating_add(m.footprint));
        self.peak = self.peak.max(footprint);
        let uses: Vec<Use> = members
            .iter()
            .filter_map(|m| {
                u32::try_from(m.pid).ok().map(|pid| Use {
                    pid,
                    footprint: m.footprint,
                })
            })
            .collect();
        match boxuser::apportion(self.bound, &uses) {
            boxuser::Plan::End { footprint, bound } => {
                self.ended_over = true;
                Plan::End { footprint, bound }
            }
            boxuser::Plan::Arm(shares) => {
                // Forget members gone since the last read, so `armed` tracks only the live ones.
                let live: BTreeSet<i32> = members.iter().map(|m| m.pid).collect();
                self.armed.retain(|pid, _| live.contains(pid));
                let mut to_arm = Vec::new();
                for s in shares {
                    let pid = i32::try_from(s.pid).unwrap_or(-1);
                    if self.armed.get(&pid) != Some(&s.limit) {
                        self.armed.insert(pid, s.limit);
                        to_arm.push((pid, s.limit));
                    }
                }
                Plan::Arm(to_arm)
            }
        }
    }

    /// Records that a member crossed its ledger share (an `EXC_RESOURCE` the helper caught); the next
    /// [`Tracked::reapportion`] decides whether to rebalance or end.
    pub fn crossed(&mut self) {
        self.crossings = self.crossings.saturating_add(1);
    }

    /// What the box holds now, for an inspect answer.
    pub fn state(&self, members: &[Member]) -> State {
        let footprint = members
            .iter()
            .fold(0u64, |s, m| s.saturating_add(m.footprint));
        State {
            members: u32::try_from(members.len()).unwrap_or(u32::MAX),
            footprint,
            peak: self.peak.max(footprint),
            processes: u32::try_from(members.len()).unwrap_or(u32::MAX),
            crossings: self.crossings,
            ended_over: self.ended_over,
        }
    }
}

#[cfg(test)]
mod tests;
