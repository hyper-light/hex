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

use std::io;

use hex_core::darwin;

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

#[cfg(test)]
mod tests;
