//! The system calls Hex makes that Rust can't make safely (CLAUDE.md §12 names this file). Each
//! `unsafe` block states the invariant that makes it sound.

// The one place `unsafe` is allowed (CLAUDE.md §12); every block below carries its SAFETY comment.
#![allow(unsafe_code)]

use std::os::fd::OwnedFd;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::Arc;

/// Ignores SIGINT and SIGTERM in this process, so a wrapper (`hex box run`) outlives the signals
/// its caller sends the whole process group and stays alive to clean the box up; the boxed child,
/// in the same group, still receives them. Best-effort: a failure leaves the default disposition.
pub fn ignore_int_term() {
    for sig in [libc::SIGINT, libc::SIGTERM] {
        // SAFETY: signal(2) with SIG_IGN and a valid signal number; it installs a disposition and
        // writes nothing of ours. The return value isn't needed (best-effort).
        unsafe {
            libc::signal(sig, libc::SIG_IGN);
        }
    }
}

/// Makes `cmd`'s child join the cgroup whose `cgroup.procs` is open as `procs` before it executes,
/// so the program and everything it starts are in the box from their first instruction.
pub fn join_cgroup_on_exec(cmd: &mut Command, procs: Arc<OwnedFd>) {
    // SAFETY: the closure runs in the forked child before exec, where only async-signal-safe calls
    // are allowed. It makes one write(2) on a descriptor opened before the fork (kept open by the
    // `Arc` the closure owns), allocates nothing, and turns a failure into the errno std reports.
    unsafe {
        cmd.pre_exec(move || match rustix::io::write(&*procs, b"0") {
            Ok(1) => Ok(()),
            Ok(_) => Err(std::io::Error::from(rustix::io::Errno::IO)),
            Err(e) => Err(std::io::Error::from(e)),
        });
    }
}
