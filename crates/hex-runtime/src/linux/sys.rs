//! The system calls Hex makes that Rust can't make safely (CLAUDE.md §12 names this file). Each
//! `unsafe` block states the invariant that makes it sound.

// The one place `unsafe` is allowed (CLAUDE.md §12); every block below carries its SAFETY comment.
#![allow(unsafe_code)]

use std::ffi::{CString, c_char, c_int};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use rustix::process::{Resource, Rlimit, WaitId, WaitIdOptions};

use super::oci::OciError;
use super::oci::sys::{ChildStack, block_signals, clone_vfork, default_signals, execve_each};

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

/// Marks every descriptor from 3 up close-on-exec, so the program this process execs next holds its
/// stdio alone: whatever it inherited by accident, from a window anywhere above it, never reaches foreign
/// code (CVE-2024-21626's class; runc v1.5.2 `libcontainer/init_linux.go:300-306`, crun 1.30.1
/// `src/libcrun/container.c:1096`). `close_range(2)`; where the kernel refuses it (before Linux 5.11, or under
/// a filter) each descriptor `/proc/self/fd` lists, as crun falls back (`src/libcrun/utils.c:1959-1975`); with
/// neither the error, so the caller refuses to run the program. System calls alone on the stack, allocating
/// nothing, so the child of a fork or a vfork-style clone in a multi-threaded process may make them.
pub fn mark_cloexec_from_3() -> rustix::io::Result<()> {
    // SAFETY: close_range(2) with CLOSE_RANGE_CLOEXEC closes nothing: it sets a flag on the descriptors
    // this process has from 3 up, so no descriptor anything in it owns is invalidated.
    let r = unsafe {
        libc::syscall(
            libc::SYS_close_range,
            3_u32,
            u32::MAX,
            libc::CLOSE_RANGE_CLOEXEC,
        )
    };
    if r == 0 {
        return Ok(());
    }
    // SAFETY: __errno_location returns this thread's errno.
    let errno = rustix::io::Errno::from_raw_os_error(unsafe { *libc::__errno_location() });
    match errno {
        rustix::io::Errno::NOSYS | rustix::io::Errno::INVAL | rustix::io::Errno::PERM => {
            mark_cloexec_by_proc()
        }
        e => Err(e),
    }
}

/// [`mark_cloexec_from_3`]'s way where the kernel refuses `close_range`: each descriptor `/proc/self/fd`
/// lists, read onto the stack.
fn mark_cloexec_by_proc() -> rustix::io::Result<()> {
    let dir = rustix::fs::open(
        c"/proc/self/fd",
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )?;
    let mut buf = [std::mem::MaybeUninit::<u8>::uninit(); 4096];
    let mut entries = rustix::fs::RawDir::new(&dir, &mut buf);
    while let Some(entry) = entries.next() {
        let entry = entry?;
        let Some(fd) = entry
            .file_name()
            .to_str()
            .ok()
            .and_then(|name| name.parse::<RawFd>().ok())
        else {
            continue;
        };
        if fd < 3 || fd == dir.as_raw_fd() {
            continue;
        }
        // SAFETY: `fd` was just listed as open here, and is borrowed only for this call: a descriptor
        // closed since is EBADF, and setting the flag on one reused since is what this is for.
        let listed = unsafe { BorrowedFd::borrow_raw(fd) };
        match rustix::io::fcntl_setfd(listed, rustix::io::FdFlags::CLOEXEC) {
            Ok(()) | Err(rustix::io::Errno::BADF) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Makes `cmd`'s child mark every descriptor from 3 up close-on-exec before it executes
/// ([`mark_cloexec_from_3`]): a foreign program the agent starts itself, a hook.
pub fn sweep_on_exec(cmd: &mut Command) {
    // SAFETY: the closure runs in the forked child before exec, where only async-signal-safe calls are
    // allowed. It makes the system calls of mark_cloexec_from_3, allocates nothing, and turns a failure
    // into the errno std reports.
    unsafe {
        cmd.pre_exec(|| mark_cloexec_from_3().map_err(std::io::Error::from));
    }
}

/// Makes `cmd`'s child join the cgroup whose `cgroup.procs` is open as `procs` before it executes, and
/// take the descriptor limit `nofile` where one is given (else it keeps the one it inherits), so the
/// program and everything it starts are in the box, under its limit, from their first instruction; and
/// marks every descriptor from 3 up close-on-exec ([`mark_cloexec_from_3`]).
pub fn join_cgroup_on_exec(cmd: &mut Command, procs: Arc<OwnedFd>, nofile: Option<Rlimit>) {
    // SAFETY: the closure runs in the forked child before exec, where only async-signal-safe calls
    // are allowed. It makes one write(2) on a descriptor opened before the fork (kept open by the
    // `Arc` the closure owns), at most one setrlimit(2) of a value made before it and the system calls of
    // mark_cloexec_from_3, allocates nothing, and turns a failure into the errno std reports.
    unsafe {
        cmd.pre_exec(move || {
            match rustix::io::write(&*procs, b"0") {
                Ok(1) => {}
                Ok(_) => return Err(std::io::Error::from(rustix::io::Errno::IO)),
                Err(e) => return Err(std::io::Error::from(e)),
            }
            if let Some(limit) = nofile {
                rustix::process::setrlimit(Resource::Nofile, limit)
                    .map_err(std::io::Error::from)?;
            }
            mark_cloexec_from_3().map_err(std::io::Error::from)
        });
    }
}

/// `struct clone_args` through its `cgroup` field (`CLONE_ARGS_SIZE_VER2`, 88 bytes), as Linux's
/// `include/uapi/linux/sched.h` lays it out: every field an `__aligned_u64`.
#[repr(C, align(8))]
pub(crate) struct CloneArgs {
    pub(crate) flags: u64,
    pub(crate) pidfd: u64,
    pub(crate) child_tid: u64,
    pub(crate) parent_tid: u64,
    pub(crate) exit_signal: u64,
    pub(crate) stack: u64,
    pub(crate) stack_size: u64,
    pub(crate) tls: u64,
    pub(crate) set_tid: u64,
    pub(crate) set_tid_size: u64,
    pub(crate) cgroup: u64,
}

/// `CLONE_INTO_CGROUP` (Linux 5.7, `include/uapi/linux/sched.h`): the child starts in the cgroup
/// `clone_args.cgroup` names. libc's constant is an `i32` that overflows (libc issue 3584), so it is
/// written here.
pub(crate) const CLONE_INTO_CGROUP: u64 = 0x2_0000_0000;

/// A box's program, ready to clone: every string and descriptor the child uses, made before the
/// clone. The child of a multi-threaded process may make only async-signal-safe calls until it
/// executes (signal-safety(7); fork(2), NOTES), so it allocates nothing.
pub struct Spawn {
    /// The paths to execute, in order: the program as named when it holds a `/`, else its name in each
    /// `PATH` folder, as execvp(3) tries them.
    paths: Vec<CString>,
    argv: Vec<CString>,
    envp: Vec<CString>,
    cwd: Option<CString>,
    /// stdin, stdout and stderr, each at fd 3 or above, so no `dup2` onto 0, 1 or 2 overwrites another.
    stdio: [OwnedFd; 3],
    /// The descriptor limit the program starts under.
    nofile: Rlimit,
}

impl Spawn {
    /// `stdio` is moved to descriptors at 3 or above where it isn't there already.
    pub fn new(
        paths: Vec<CString>,
        argv: Vec<CString>,
        envp: Vec<CString>,
        cwd: Option<CString>,
        stdio: [OwnedFd; 3],
        nofile: Rlimit,
    ) -> std::io::Result<Self> {
        let [a, b, c] = stdio;
        // Each stdio fd ends up at 3 or above and close-on-exec, so the child's `dup2` onto 0, 1 and
        // 2 makes the only copies the workload keeps (dup2 clears close-on-exec on its target) and
        // the original closes on exec rather than leaking in beside it. One already above 2 keeps its
        // number but is marked close-on-exec; one at 0..=2 is moved above 2 (which sets it too).
        let above = |fd: OwnedFd| -> std::io::Result<OwnedFd> {
            if fd.as_raw_fd() > 2 {
                rustix::io::fcntl_setfd(&fd, rustix::io::FdFlags::CLOEXEC)?;
                Ok(fd)
            } else {
                Ok(rustix::io::fcntl_dupfd_cloexec(&fd, 3)?)
            }
        };
        Ok(Self {
            paths,
            argv,
            envp,
            cwd,
            stdio: [above(a)?, above(b)?, above(c)?],
            nofile,
        })
    }

    /// Its stdio back, for a caller that starts the program another way.
    pub fn into_stdio(self) -> [OwnedFd; 3] {
        self.stdio
    }
}

/// What [`clone_into_cgroup`] did.
pub enum Cloned {
    /// The program runs: its pid, and the pidfd the clone made for it.
    Started { pid: u32, pidfd: OwnedFd },
    /// This kernel or sandbox has no clone into a cgroup: `ENOSYS` (before Linux 5.3, or a seccomp
    /// profile like Docker's default, which answers `clone3` so) or `E2BIG` (before 5.7, which doesn't
    /// know the `cgroup` field). The spawn comes back for the caller's fork path.
    Unsupported(Spawn),
}

std::thread_local! {
    /// The stack a process box's vfork-style clone runs its child on, mapped once per agent thread that
    /// clones and reused ([`ChildStack`]).
    static STACK: std::cell::RefCell<Option<ChildStack>> = const { std::cell::RefCell::new(None) };
}

/// Clones `spawn`'s program straight into the cgroup open as `cgroup`, as posix_spawn starts a program:
/// `clone3(CLONE_VM | CLONE_VFORK | CLONE_INTO_CGROUP | CLONE_PIDFD)` from the calling thread
/// ([`clone_vfork`], goal 5's trampoline). The program is in the box from its first instruction, with no
/// `cgroup.procs` write and the kernel's migration locks it takes, and nothing of the node agent is
/// copied for it: the child runs on a stack of its own in the agent's memory until its execve, and this
/// thread waits until then (decision 7 of docs/design/runtime-architecture.md). Returns once the child's
/// execve has replaced its memory, past which the exec can't come back to the caller (the vfork's release,
/// Linux v6.19 fs/exec.c:854, kernel/fork.c:1488); the kernel names the program a moment later
/// (fs/exec.c:1242-1246). One that can't execute comes back as its errno, its child reaped.
pub fn clone_into_cgroup(cgroup: BorrowedFd<'_>, spawn: Spawn) -> std::io::Result<Cloned> {
    let argv = pointers(&spawn.argv);
    let envp = pointers(&spawn.envp);
    // Where the child leaves its errno when no path executes. It shares this memory until it executes
    // or exits, and this thread runs again only then (CLONE_VFORK), so the value is read once the clone
    // has returned. No pipe: a vfork's child and its parent need none to tell an exec from a failure.
    let failed = AtomicI32::new(0);
    let run = || {
        // SAFETY: this closure runs only as the vfork-style clone's child, with `argv` and `envp` built
        // by `pointers` over `spawn`'s strings, which outlive the clone.
        let e = unsafe { child(&spawn, &argv, &envp) };
        failed.store(e, Ordering::Relaxed);
        127
    };
    let other = |e: OciError| std::io::Error::other(e.to_string());
    let blocked = block_signals().map_err(other)?;
    let cloned = STACK.with(|stack| -> Result<_, OciError> {
        let mut stack = stack
            .try_borrow_mut()
            .map_err(|_| OciError::Broken("a thread's child stack is in use".to_owned()))?;
        if stack.is_none() {
            *stack = Some(ChildStack::map()?);
        }
        match stack.as_ref() {
            Some(stack) => clone_vfork(cgroup, stack, &run),
            None => Ok(None),
        }
    });
    drop(blocked);
    let Some((pid, pidfd)) = cloned.map_err(other)? else {
        return Ok(Cloned::Unsupported(spawn));
    };
    match failed.load(Ordering::Relaxed) {
        0 => {
            let pid = u32::try_from(pid)
                .map_err(|_| std::io::Error::other("clone3 returned a pid out of range"))?;
            Ok(Cloned::Started { pid, pidfd })
        }
        e => {
            // The child has exited; reap it, so its pid isn't left a zombie.
            let _ = rustix::process::waitid(WaitId::PidFd(pidfd.as_fd()), WaitIdOptions::EXITED);
            Err(std::io::Error::from_raw_os_error(e))
        }
    }
}

/// `strs` as the null-terminated array of pointers execve(2) takes; `strs` must outlive it.
fn pointers(strs: &[CString]) -> Vec<*const c_char> {
    strs.iter()
        .map(|s| s.as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect()
}

/// The clone's child: stdio onto 0, 1 and 2, every descriptor from 3 up close-on-exec, the signal
/// state std's `Command` leaves a child ([`default_signals`]: an empty mask, SIGPIPE back to its
/// default, which Rust ignores), its descriptor limit, the working folder, then each path through
/// execve(2), as execvp(3) tries them. System calls alone, on memory made before the clone, which it
/// shares with the node agent's other threads: it takes no lock and allocates nothing. Returns only when
/// the program couldn't start: the errno why.
///
/// # Safety
/// Only in the child of [`clone_into_cgroup`]'s clone, with `argv` and `envp` built by [`pointers`].
unsafe fn child(spawn: &Spawn, argv: &[*const c_char], envp: &[*const c_char]) -> c_int {
    // SAFETY: __errno_location returns this thread's errno: the calling thread's, whose memory the child
    // shares while that thread waits in the vfork, so nothing else reads it meanwhile.
    let errno = || unsafe { *libc::__errno_location() };
    for (target, fd) in (0..).zip(&spawn.stdio) {
        // SAFETY: dup2(2) of an open descriptor onto 0, 1 or 2, which clears close-on-exec there.
        if unsafe { libc::dup2(fd.as_raw_fd(), target) } == -1 {
            return errno();
        }
    }
    if let Err(e) = mark_cloexec_from_3() {
        return e.raw_os_error();
    }
    if let Err(e) = default_signals() {
        return e.raw_os_error();
    }
    // The child is a process of its own (no CLONE_THREAD), so the limit is its own, never the agent's.
    if let Err(e) = rustix::process::setrlimit(Resource::Nofile, spawn.nofile) {
        return e.raw_os_error();
    }
    if let Some(cwd) = &spawn.cwd {
        // SAFETY: chdir(2) with a valid C string.
        if unsafe { libc::chdir(cwd.as_ptr()) } == -1 {
            return errno();
        }
    }
    execve_each(&spawn.paths, argv, envp)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Before a foreign execve every descriptor from 3 up is close-on-exec, one opened without the flag
    /// included (CVE-2024-21626's class), by `close_range` and by the walk of `/proc/self/fd` a kernel
    /// that refuses it falls back to. Do: open a descriptor without the flag, mark. Expect the flag on it,
    /// each way.
    #[test]
    fn a_stray_descriptor_is_marked_close_on_exec_either_way() {
        for mark in [mark_cloexec_from_3, mark_cloexec_by_proc] {
            let stray = rustix::fs::open(
                c"/dev/null",
                rustix::fs::OFlags::RDONLY,
                rustix::fs::Mode::empty(),
            )
            .unwrap();
            let cloexec = |fd: &OwnedFd| {
                rustix::io::fcntl_getfd(fd)
                    .unwrap()
                    .contains(rustix::io::FdFlags::CLOEXEC)
            };
            assert!(!cloexec(&stray));
            mark().unwrap();
            assert!(cloexec(&stray));
        }
    }
}
