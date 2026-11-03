//! The system calls Hex makes that Rust can't make safely (CLAUDE.md §12 names this file). Each
//! `unsafe` block states the invariant that makes it sound.

// The one place `unsafe` is allowed (CLAUDE.md §12); every block below carries its SAFETY comment.
#![allow(unsafe_code)]

use std::ffi::{CString, c_char, c_int};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::Arc;

use rustix::pipe::PipeFlags;
use rustix::process::{WaitId, WaitIdOptions};

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

/// Makes `cmd`'s child join the cgroup whose `cgroup.procs` is open as `procs` before it executes,
/// so the program and everything it starts are in the box from their first instruction, and mark every
/// descriptor from 3 up close-on-exec ([`mark_cloexec_from_3`]).
pub fn join_cgroup_on_exec(cmd: &mut Command, procs: Arc<OwnedFd>) {
    // SAFETY: the closure runs in the forked child before exec, where only async-signal-safe calls
    // are allowed. It makes one write(2) on a descriptor opened before the fork (kept open by the
    // `Arc` the closure owns) and the system calls of mark_cloexec_from_3, allocates nothing, and
    // turns a failure into the errno std reports.
    unsafe {
        cmd.pre_exec(move || {
            match rustix::io::write(&*procs, b"0") {
                Ok(1) => {}
                Ok(_) => return Err(std::io::Error::from(rustix::io::Errno::IO)),
                Err(e) => return Err(std::io::Error::from(e)),
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

/// `CLONE_PIDFD` (Linux 5.2, `include/uapi/linux/sched.h`): the kernel writes a pidfd for the child
/// where `clone_args.pidfd` points.
const CLONE_PIDFD: u64 = 0x1000;

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
}

impl Spawn {
    /// `stdio` is moved to descriptors at 3 or above where it isn't there already.
    pub fn new(
        paths: Vec<CString>,
        argv: Vec<CString>,
        envp: Vec<CString>,
        cwd: Option<CString>,
        stdio: [OwnedFd; 3],
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

/// Clones `spawn`'s program straight into the cgroup open as `cgroup`, with `clone3(CLONE_INTO_CGROUP |
/// CLONE_PIDFD)`: in the box from its first instruction, without the `cgroup.procs` write a migration
/// makes under the kernel's cgroup locks (docs/design/runtime-architecture.md, decision 7). Returns
/// once the program has executed; an exec that fails comes back as its errno, its child reaped.
pub fn clone_into_cgroup(cgroup: BorrowedFd<'_>, spawn: Spawn) -> std::io::Result<Cloned> {
    let argv = pointers(&spawn.argv);
    let envp = pointers(&spawn.envp);
    // The child writes its errno here when it can't execute; close-on-exec, so it reads end-of-file
    // once the program runs.
    let (err_r, err_w) = rustix::pipe::pipe_with(PipeFlags::CLOEXEC)?;
    let mut pidfd: c_int = -1;
    let args = CloneArgs {
        flags: CLONE_INTO_CGROUP | CLONE_PIDFD,
        pidfd: u64::try_from((&raw mut pidfd).addr())
            .map_err(|_| std::io::Error::other("a pointer wider than 64 bits"))?,
        child_tid: 0,
        parent_tid: 0,
        exit_signal: u64::try_from(libc::SIGCHLD)
            .map_err(|_| std::io::Error::other("SIGCHLD is negative"))?,
        stack: 0,
        stack_size: 0,
        tls: 0,
        set_tid: 0,
        set_tid_size: 0,
        cgroup: u64::try_from(cgroup.as_raw_fd())
            .map_err(|_| std::io::Error::other("a negative file descriptor"))?,
    };
    // SAFETY: clone3(2) with a valid pointer to a fully initialized `CloneArgs` and its exact size,
    // which the kernel reads, and in `pidfd` the address of a live `c_int` it writes the child's pidfd
    // to. With no stack given the child goes on with a copy of this stack, as after fork(2), and no
    // flag shares memory, files or signal handlers with the parent. The child is a copy of a possibly
    // multi-threaded process, so it runs only `child`, which makes async-signal-safe calls alone on
    // memory made before the clone. The return value is checked.
    let r = unsafe {
        libc::syscall(
            libc::SYS_clone3,
            &args as *const CloneArgs,
            std::mem::size_of::<CloneArgs>(),
        )
    };
    match r {
        -1 => {
            let e = std::io::Error::last_os_error();
            match e.raw_os_error() {
                Some(libc::ENOSYS) | Some(libc::E2BIG) => Ok(Cloned::Unsupported(spawn)),
                _ => Err(e),
            }
        }
        // SAFETY: this is the clone's child; `child` never returns.
        0 => unsafe { child(&spawn, &argv, &envp, err_w.as_raw_fd()) },
        child_pid => {
            drop(err_w);
            // SAFETY: the clone succeeded, so the kernel made `pidfd` for the child and nothing else
            // in this process owns it.
            let pidfd = unsafe { OwnedFd::from_raw_fd(pidfd) };
            let pid = u32::try_from(child_pid)
                .map_err(|_| std::io::Error::other("clone3 returned a pid out of range"))?;
            let mut errno = [0u8; 4];
            let got = match read_fully(&err_r, &mut errno) {
                Ok(got) => got,
                Err(e) => {
                    // The clone succeeded but reading its errno pipe failed unexpectedly, so the
                    // child's state is unknown: end it and reap it rather than leak it (CLAUDE.md §2),
                    // then report the read failure. A SIGKILL to a child that has already exited is a
                    // harmless ESRCH; the pidfd reap can't take a recycled pid.
                    reap_cloned(child_pid, pidfd.as_fd());
                    return Err(e);
                }
            };
            match got {
                0 => Ok(Cloned::Started { pid, pidfd }),
                n => {
                    // The child has written why and exits; reap it, so its pid isn't left a zombie.
                    let _ = rustix::process::waitid(
                        WaitId::PidFd(pidfd.as_fd()),
                        WaitIdOptions::EXITED,
                    );
                    Err(if n == errno.len() {
                        std::io::Error::from_raw_os_error(c_int::from_ne_bytes(errno))
                    } else {
                        std::io::Error::other("the program's child sent a short error")
                    })
                }
            }
        }
    }
}

/// Ends a cloned child and reaps it, so a spawn that fails after the clone leaves neither a zombie
/// nor a running workload (CLAUDE.md §2). The child is unreaped, so its pid can't have been recycled;
/// the reap waits on the pidfd, which names the child itself.
fn reap_cloned(child_pid: i64, pidfd: BorrowedFd<'_>) {
    if let Ok(pid) = c_int::try_from(child_pid) {
        // SAFETY: kill(2) with the child's pid and SIGKILL; no pointers, writes nothing. The return
        // value isn't needed — a child that has already exited gives ESRCH, which the reap below
        // handles; a pid out of `c_int` range can't be a real one, so the kill is skipped.
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
    let _ = rustix::process::waitid(WaitId::PidFd(pidfd), WaitIdOptions::EXITED);
}

/// `strs` as the null-terminated array of pointers execve(2) takes; `strs` must outlive it.
fn pointers(strs: &[CString]) -> Vec<*const c_char> {
    strs.iter()
        .map(|s| s.as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect()
}

/// Reads until `buf` is full or end-of-file; how many bytes it read.
fn read_fully(fd: &OwnedFd, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut got = 0;
    while let Some(rest) = buf.get_mut(got..) {
        if rest.is_empty() {
            break;
        }
        match rustix::io::read(fd, rest) {
            Ok(0) => break,
            Ok(n) => {
                got = got
                    .checked_add(n)
                    .ok_or_else(|| std::io::Error::other("a read overflowed"))?
            }
            Err(rustix::io::Errno::INTR) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(got)
}

/// The clone's child: stdio onto 0, 1 and 2, the signal state std's `Command` leaves a child (an empty
/// mask, SIGPIPE back to its default, which Rust ignores), the working folder, then each path through
/// execve(2), as execvp(3) tries them. Async-signal-safe calls only, on memory made before the clone.
/// It never returns: it executes, or writes its errno to `err` and `_exit`s.
///
/// # Safety
/// Only in the child of [`clone_into_cgroup`]'s clone, with `argv` and `envp` built by [`pointers`].
unsafe fn child(spawn: &Spawn, argv: &[*const c_char], envp: &[*const c_char], err: RawFd) -> ! {
    /// Writes `e` to `err` and exits: async-signal-safe calls alone.
    fn fail(err: RawFd, e: c_int) -> ! {
        let bytes = e.to_ne_bytes();
        // SAFETY: write(2) of a local buffer to an open descriptor, then _exit(2); both
        // async-signal-safe, with valid arguments.
        unsafe {
            libc::write(err, bytes.as_ptr().cast(), bytes.len());
            libc::_exit(127)
        }
    }
    // SAFETY: __errno_location returns this thread's errno, which the child has as a copy.
    let errno = || unsafe { *libc::__errno_location() };
    for (target, fd) in (0..).zip(&spawn.stdio) {
        // SAFETY: dup2(2) of an open descriptor onto 0, 1 or 2, which clears close-on-exec there.
        if unsafe { libc::dup2(fd.as_raw_fd(), target) } == -1 {
            fail(err, errno());
        }
    }
    if let Err(e) = mark_cloexec_from_3() {
        fail(err, e.raw_os_error());
    }
    // SAFETY: `sigset_t` is plain data, initialized by sigemptyset before anything reads it; then
    // pthread_sigmask and signal(2), both async-signal-safe, with valid arguments.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        let r = libc::pthread_sigmask(libc::SIG_SETMASK, &set, std::ptr::null_mut());
        if r != 0 {
            fail(err, r);
        }
        if libc::signal(libc::SIGPIPE, libc::SIG_DFL) == libc::SIG_ERR {
            fail(err, errno());
        }
    }
    if let Some(cwd) = &spawn.cwd {
        // SAFETY: chdir(2) with a valid C string.
        if unsafe { libc::chdir(cwd.as_ptr()) } == -1 {
            fail(err, errno());
        }
    }
    let mut last = libc::ENOENT;
    let mut denied = false;
    for path in &spawn.paths {
        // SAFETY: execve(2) with a valid C string and null-terminated arrays of valid C strings,
        // which outlive the call; it returns only on failure.
        unsafe { libc::execve(path.as_ptr(), argv.as_ptr(), envp.as_ptr()) };
        match errno() {
            // As execvp(3): a path that isn't there sends it on to the next; one it may not run is
            // reported if nothing after it runs.
            e @ (libc::ENOENT | libc::ENOTDIR) => last = e,
            libc::EACCES => denied = true,
            e => fail(err, e),
        }
    }
    fail(err, if denied { libc::EACCES } else { last })
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
