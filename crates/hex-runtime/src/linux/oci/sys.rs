//! The system calls the OCI runtime makes that Rust can't make safely (CLAUDE.md §12 names this
//! file). The runtime re-execs itself for the container init (`hex __oci-init`), so the process that
//! calls [`fork`] is single-threaded — the fork-in-a-threaded-process hazard does not apply here,
//! and the child runs ordinary Rust, not only async-signal-safe calls.

#![allow(unsafe_code)]

use crate::linux::oci::{Failure, OciError};
use crate::linux::sys::{CLONE_INTO_CGROUP, CloneArgs};

/// What [`fork`] returns.
pub enum Fork {
    /// The parent; holds the child's pid.
    Parent(i32),
    /// The child.
    Child,
}

/// Forks. The caller must be single-threaded (the OCI init is, having just re-exec'd).
pub fn fork() -> Result<Fork, OciError> {
    // SAFETY: fork(2) with no arguments. The one real constraint — that a forked child of a
    // multi-threaded process may touch only async-signal-safe state before exec — does not apply:
    // every caller (`hex __oci-init`) has just re-exec'd and is single-threaded, so the child owns a
    // faithful copy of the address space and runs ordinary code. The return value is checked.
    let pid = unsafe { libc::fork() };
    match pid {
        -1 => Err(OciError::os("fork", std::io::Error::last_os_error())),
        0 => Ok(Fork::Child),
        child => Ok(Fork::Parent(child)),
    }
}

/// Forks into a cgroup: `clone3` with `CLONE_INTO_CGROUP`, so the child is born in the cgroup that
/// the open directory `cgroup` is, and nothing migrates it there. A migration takes the cgroup
/// threadgroup lock, whose writer can wait out a whole RCU grace period (docs/design/runtime.md); a
/// birth in the cgroup takes no such lock. `None` when this kernel or sandbox has no such clone:
/// `ENOSYS` (before Linux 5.3, or a seccomp profile like Docker's default, which answers `clone3`
/// so) or `E2BIG` (before 5.7, which doesn't know the `cgroup` field). The caller must be
/// single-threaded, as for [`fork`].
pub fn fork_into_cgroup(cgroup: std::os::fd::BorrowedFd<'_>) -> Result<Option<Fork>, OciError> {
    use std::os::fd::AsRawFd;
    let fd = u64::try_from(cgroup.as_raw_fd())
        .map_err(|_| OciError::os("clone3 into a cgroup", "a negative file descriptor"))?;
    let exit_signal = u64::try_from(libc::SIGCHLD)
        .map_err(|_| OciError::os("clone3 into a cgroup", "SIGCHLD is negative"))?;
    let args = CloneArgs {
        flags: CLONE_INTO_CGROUP,
        pidfd: 0,
        child_tid: 0,
        parent_tid: 0,
        exit_signal,
        stack: 0,
        stack_size: 0,
        tls: 0,
        set_tid: 0,
        set_tid_size: 0,
        cgroup: fd,
    };
    // SAFETY: clone3(2) with a valid pointer to a fully initialized `CloneArgs` and its exact size,
    // which the kernel only reads. With no stack given, the child goes on with a copy of the caller's
    // stack, as after fork(2), and no flag shares anything with the parent (no CLONE_VM, CLONE_FILES
    // or CLONE_THREAD), so the child owns a faithful copy of the address space. fork's one
    // constraint, that the child of a multi-threaded process may touch only async-signal-safe state,
    // doesn't apply: every caller has re-exec'd and is single-threaded, so no other thread held a lock
    // across the clone, and Hex registers no atfork handler for libc's fork to have run. The return
    // value is checked.
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
                Some(libc::ENOSYS) | Some(libc::E2BIG) => Ok(None),
                _ => Err(OciError::os("clone3 into the container's cgroup", e)),
            }
        }
        0 => Ok(Some(Fork::Child)),
        child => Ok(Some(Fork::Parent(i32::try_from(child).map_err(|_| {
            OciError::os("clone3 into a cgroup", "a pid out of range")
        })?))),
    }
}

/// `CLONE_PIDFD` (Linux 5.2, `include/uapi/linux/sched.h`): the kernel writes a pidfd for the child
/// where `clone_args.pidfd` points. libc's constant is the same value, written here to match
/// [`crate::linux::sys`]'s own.
const CLONE_PIDFD: u64 = 0x1000;

/// Forks into a cgroup with a pidfd for the child: `clone3(CLONE_INTO_CGROUP | CLONE_PIDFD)`, so the
/// child is born in the cgroup that the open directory `cgroup` is, and the parent is handed a pidfd
/// that it (or a process it sends it to) watches for the child's end. Like [`fork_into_cgroup`] but for
/// the exec seed (`seed.rs`), whose intermediate hands the pidfd to the node agent rather than waiting
/// on the child itself. `None` when this kernel or sandbox has no clone into a cgroup (`ENOSYS` or
/// `E2BIG`, as [`fork_into_cgroup`]); the caller falls back to [`fork`] and a migration. The caller
/// must be single-threaded, as for [`fork`].
pub fn fork_into_cgroup_pidfd(
    cgroup: std::os::fd::BorrowedFd<'_>,
) -> Result<Option<(Fork, Option<std::os::fd::OwnedFd>)>, OciError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let fd = u64::try_from(cgroup.as_raw_fd())
        .map_err(|_| OciError::os("clone3 into a cgroup", "a negative file descriptor"))?;
    let exit_signal = u64::try_from(libc::SIGCHLD)
        .map_err(|_| OciError::os("clone3 into a cgroup", "SIGCHLD is negative"))?;
    let mut pidfd: libc::c_int = -1;
    let pidfd_ptr = u64::try_from((&raw mut pidfd).addr())
        .map_err(|_| OciError::os("clone3 into a cgroup", "a pointer wider than 64 bits"))?;
    let args = CloneArgs {
        flags: CLONE_INTO_CGROUP | CLONE_PIDFD,
        pidfd: pidfd_ptr,
        child_tid: 0,
        parent_tid: 0,
        exit_signal,
        stack: 0,
        stack_size: 0,
        tls: 0,
        set_tid: 0,
        set_tid_size: 0,
        cgroup: fd,
    };
    // SAFETY: clone3(2) with a valid pointer to a fully initialized `CloneArgs` and its exact size,
    // which the kernel only reads, and in `pidfd` the address of a live `c_int` it writes the child's
    // pidfd to. With no stack given, the child goes on with a copy of the caller's stack, as after
    // fork(2), and no flag shares memory, files or signal handlers with the parent. fork's one
    // constraint, that the child of a multi-threaded process may touch only async-signal-safe state,
    // doesn't apply: every caller has re-exec'd (the seed) and is single-threaded. The return value is
    // checked.
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
                Some(libc::ENOSYS) | Some(libc::E2BIG) => Ok(None),
                _ => Err(OciError::os("clone3 into the container's cgroup", e)),
            }
        }
        0 => Ok(Some((Fork::Child, None))),
        child => {
            // SAFETY: the clone succeeded, so the kernel made `pidfd` for the child and nothing else
            // in this process owns it.
            let owned = unsafe { std::os::fd::OwnedFd::from_raw_fd(pidfd) };
            let pid = i32::try_from(child)
                .map_err(|_| OciError::os("clone3 into a cgroup", "a pid out of range"))?;
            Ok(Some((Fork::Parent(pid), Some(owned))))
        }
    }
}

/// `CLONE_PARENT` (`include/uapi/linux/sched.h`): the child's parent is the caller's parent.
const CLONE_PARENT: u64 = 0x8000;

/// Clones a sibling: `clone3(CLONE_PARENT | CLONE_PIDFD)`, with `CLONE_INTO_CGROUP` when `cgroup` is
/// given, so the child's parent is the caller's parent (the exec seed's: the node agent), it is born in
/// the cgroup the open directory `cgroup` is, and the caller is handed its pidfd. `exit_signal` is 0
/// because `clone3` refuses `CLONE_PARENT` with any other (`clone3_args_valid` in kernel/fork.c): the
/// child takes the caller's own exit signal instead (`copy_process`), which for a seed the agent
/// spawned is `SIGCHLD`. `None` when this kernel or sandbox has no `clone3` or no clone into a cgroup
/// (`ENOSYS` or `E2BIG`, as [`fork_into_cgroup`]); the seed then takes its intermediate path. The caller
/// must be single-threaded, as for [`fork`].
pub fn clone_sibling(
    cgroup: Option<std::os::fd::BorrowedFd<'_>>,
) -> Result<Option<Sibling>, OciError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let (flags, cgroup) = match cgroup {
        Some(dir) => (
            CLONE_PARENT | CLONE_PIDFD | CLONE_INTO_CGROUP,
            u64::try_from(dir.as_raw_fd())
                .map_err(|_| OciError::os("clone3 a sibling", "a negative file descriptor"))?,
        ),
        None => (CLONE_PARENT | CLONE_PIDFD, 0),
    };
    let mut pidfd: libc::c_int = -1;
    let pidfd_ptr = u64::try_from((&raw mut pidfd).addr())
        .map_err(|_| OciError::os("clone3 a sibling", "a pointer wider than 64 bits"))?;
    let args = CloneArgs {
        flags,
        pidfd: pidfd_ptr,
        child_tid: 0,
        parent_tid: 0,
        exit_signal: 0,
        stack: 0,
        stack_size: 0,
        tls: 0,
        set_tid: 0,
        set_tid_size: 0,
        cgroup,
    };
    // SAFETY: clone3(2) with a valid pointer to a fully initialized `CloneArgs` and its exact size,
    // which the kernel only reads, and in `pidfd` the address of a live `c_int` it writes the child's
    // pidfd to. With no stack given, the child goes on with a copy of the caller's stack, as after
    // fork(2), and no flag shares memory, files or signal handlers with the caller. fork's one
    // constraint, that the child of a multi-threaded process may touch only async-signal-safe state,
    // doesn't apply: the caller, the seed, re-exec'd and is single-threaded. The return value is
    // checked.
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
                Some(libc::ENOSYS) | Some(libc::E2BIG) => Ok(None),
                _ => Err(OciError::os("clone3 the program into the container", e)),
            }
        }
        0 => Ok(Some(Sibling::Child)),
        child => {
            // SAFETY: the clone succeeded, so the kernel made `pidfd` for the child and nothing else
            // in this process owns it.
            let pidfd = unsafe { std::os::fd::OwnedFd::from_raw_fd(pidfd) };
            let pid = i32::try_from(child)
                .map_err(|_| OciError::os("clone3 a sibling", "a pid out of range"))?;
            Ok(Some(Sibling::Parent { pid, pidfd }))
        }
    }
}

/// What [`clone_sibling`] returns in each process.
pub enum Sibling {
    /// The caller: the sibling's pid and its pidfd.
    Parent {
        pid: i32,
        pidfd: std::os::fd::OwnedFd,
    },
    /// The sibling.
    Child,
}

/// The signal state a program starts with, as std's `Command` leaves a child: an empty signal mask and
/// SIGPIPE at its default (Rust's runtime ignores it, and an ignored signal stays ignored across execve).
/// Async-signal-safe calls only, for the seed's clone's child, which allocates nothing.
pub fn default_signals() -> Result<(), rustix::io::Errno> {
    // SAFETY: `sigset_t` is plain data, initialized by sigemptyset before anything reads it; then
    // pthread_sigmask and signal(2), both async-signal-safe, with valid arguments.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        let r = libc::pthread_sigmask(libc::SIG_SETMASK, &set, std::ptr::null_mut());
        if r != 0 {
            return Err(rustix::io::Errno::from_raw_os_error(r));
        }
        if libc::signal(libc::SIGPIPE, libc::SIG_DFL) == libc::SIG_ERR {
            return Err(last_errno());
        }
    }
    Ok(())
}

/// execve(2) of each of `paths` in turn, as execvp(3) tries them, with `argv` and `envp` the
/// null-terminated arrays execve takes. Returns only when none ran: the errno that ended it, `EACCES`
/// if a path it may not run came before paths that aren't there, else the last.
pub fn execve_each(
    paths: &[std::ffi::CString],
    argv: &[*const libc::c_char],
    envp: &[*const libc::c_char],
) -> i32 {
    // SAFETY: __errno_location returns this thread's errno.
    let errno = || unsafe { *libc::__errno_location() };
    let mut last = libc::ENOENT;
    let mut denied = false;
    for path in paths {
        // SAFETY: execve(2) with a valid C string and null-terminated arrays of valid C strings (the
        // caller's `argv`/`envp`, built over strings that outlive the call); it returns only on failure.
        unsafe { libc::execve(path.as_ptr(), argv.as_ptr(), envp.as_ptr()) };
        match errno() {
            e @ (libc::ENOENT | libc::ENOTDIR) => last = e,
            libc::EACCES => denied = true,
            e => return e,
        }
    }
    if denied { libc::EACCES } else { last }
}

/// `CLONE_VM` and `CLONE_VFORK` (`include/uapi/linux/sched.h`): the child shares the caller's memory,
/// and the caller's thread waits until the child has exec'd or exited.
const CLONE_VM: u64 = 0x100;
const CLONE_VFORK: u64 = 0x4000;

/// The stack a vfork-style clone's child runs on ([`clone_vfork`]): 256 KiB, mapped once per thread that
/// clones and reused, since the child is done with it by the time the clone returns. Its lowest page is a
/// guard, inaccessible, so an overflow faults rather than writing into the parent's memory. The child's
/// frames are shallow (the joins, the credential and capability calls, execve); 256 KiB is a stated
/// bound well above them, not a tuning.
pub struct ChildStack {
    base: *mut libc::c_void,
    len: usize,
}

const CHILD_STACK_BYTES: usize = 256 << 10;

impl ChildStack {
    pub fn map() -> Result<Self, OciError> {
        let page = page_size()?;
        // SAFETY: an anonymous private mapping at an address the kernel picks: it aliases nothing. The
        // result is checked; the guard page is made inaccessible before anything uses the mapping.
        let base = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                CHILD_STACK_BYTES,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_STACK,
                -1,
                0,
            )
        };
        if base == libc::MAP_FAILED {
            return Err(OciError::os(
                "mapping a child's stack",
                std::io::Error::last_os_error(),
            ));
        }
        let stack = Self {
            base,
            len: CHILD_STACK_BYTES,
        };
        // SAFETY: the first page of the mapping just made, which nothing references.
        if unsafe { libc::mprotect(base, page, libc::PROT_NONE) } != 0 {
            return Err(OciError::os(
                "guarding a child's stack",
                std::io::Error::last_os_error(),
            ));
        }
        Ok(stack)
    }
}

impl Drop for ChildStack {
    fn drop(&mut self) {
        // SAFETY: the mapping `map` made, which no clone's child uses any more (each clone returned).
        unsafe {
            libc::munmap(self.base, self.len);
        }
    }
}

/// The size of a page.
fn page_size() -> Result<usize, OciError> {
    // SAFETY: sysconf(3) takes a constant and reads nothing of ours.
    let n = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    usize::try_from(n).map_err(|_| OciError::os("the page size", "sysconf gave none"))
}

/// Every signal blocked on the calling thread until dropped, which restores the mask it had. A vfork-style
/// clone's child inherits the mask, so it runs none of the process's signal handlers before its execve
/// (which resets them), as posix_spawn blocks them.
pub struct Blocked {
    old: libc::sigset_t,
}

pub fn block_signals() -> Result<Blocked, OciError> {
    // SAFETY: `sigset_t` is plain data, filled by sigfillset before pthread_sigmask reads it, and `old`
    // is written by pthread_sigmask; both calls take valid pointers. Checked.
    unsafe {
        let mut all: libc::sigset_t = std::mem::zeroed();
        let mut old: libc::sigset_t = std::mem::zeroed();
        libc::sigfillset(&mut all);
        let r = libc::pthread_sigmask(libc::SIG_BLOCK, &all, &mut old);
        if r != 0 {
            return Err(OciError::os(
                "blocking signals for a clone",
                std::io::Error::from_raw_os_error(r),
            ));
        }
        Ok(Blocked { old })
    }
}

impl Drop for Blocked {
    fn drop(&mut self) {
        // SAFETY: the mask `block_signals` saved, a valid sigset_t.
        unsafe {
            libc::pthread_sigmask(libc::SIG_SETMASK, &self.old, std::ptr::null_mut());
        }
    }
}

/// Clones a child that shares this process's memory and runs `child` on `stack`, born in the cgroup the
/// open directory `cgroup` is, with a pidfd for it: `clone3(CLONE_VM | CLONE_VFORK | CLONE_INTO_CGROUP |
/// CLONE_PIDFD)`, as posix_spawn clones. No page of this process is copied for the child, and its execve
/// tears none down; the calling thread waits until the child has exec'd or exited. The caller blocks
/// signals first ([`block_signals`]). `child` makes system calls alone on memory made before the clone,
/// and returns only when one failed (the status to exit with, and what): it shares this process's memory
/// with the process's other threads, so it may take no lock and allocate nothing. `None` on an
/// architecture without the trampoline below, or a kernel or sandbox without clone3 into a cgroup
/// (`ENOSYS`, `E2BIG`).
pub fn clone_vfork<F: Fn() -> (u8, Failure)>(
    cgroup: std::os::fd::BorrowedFd<'_>,
    stack: &ChildStack,
    child: &F,
) -> Result<Option<(i32, std::os::fd::OwnedFd)>, OciError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    /// The child's start, on its own stack: `child`, then its failure reported and its exit.
    extern "C" fn entry<F: Fn() -> (u8, Failure)>(arg: *mut libc::c_void) -> ! {
        // SAFETY: `arg` is `child`, borrowed by the clone below, alive until this child has exec'd or
        // exited, which the vfork makes its parent thread wait for.
        let child = unsafe { &*arg.cast::<F>().cast_const() };
        let (code, failed) = child();
        report(failed);
        exit_child(code)
    }
    let page = page_size()?;
    let fd = u64::try_from(cgroup.as_raw_fd())
        .map_err(|_| OciError::os("clone3 into a cgroup", "a negative file descriptor"))?;
    let mut pidfd: libc::c_int = -1;
    let as_u64 = |v: usize| {
        u64::try_from(v)
            .map_err(|_| OciError::os("clone3 into a cgroup", "a value wider than 64 bits"))
    };
    let args = CloneArgs {
        flags: CLONE_VM | CLONE_VFORK | CLONE_INTO_CGROUP | CLONE_PIDFD,
        pidfd: as_u64((&raw mut pidfd).addr())?,
        child_tid: 0,
        parent_tid: 0,
        exit_signal: u64::try_from(libc::SIGCHLD)
            .map_err(|_| OciError::os("clone3 into a cgroup", "SIGCHLD is negative"))?,
        // The usable stack: above the guard page; the kernel starts the child at its top.
        stack: as_u64(stack.base.addr().checked_add(page).ok_or_else(|| {
            OciError::os("clone3 into a cgroup", "a stack past the address space")
        })?)?,
        stack_size: as_u64(stack.len.checked_sub(page).ok_or_else(|| {
            OciError::os("clone3 into a cgroup", "a stack smaller than its guard")
        })?)?,
        tls: 0,
        set_tid: 0,
        set_tid_size: 0,
        cgroup: fd,
    };
    let entry_fn: extern "C" fn(*mut libc::c_void) -> ! = entry::<F>;
    let arg = std::ptr::from_ref(child).cast_mut().cast::<libc::c_void>();
    let Some(r) = clone3_on_stack(&args, entry_fn, arg) else {
        return Ok(None);
    };
    match r {
        n if n < 0 => match i32::try_from(n.unsigned_abs()).unwrap_or(libc::EIO) {
            libc::ENOSYS | libc::E2BIG => Ok(None),
            e => Err(OciError::os(
                "clone3 the program into the container",
                std::io::Error::from_raw_os_error(e),
            )),
        },
        child_pid => {
            // SAFETY: the clone succeeded, so the kernel made `pidfd` for the child and nothing else in
            // this process owns it.
            let pidfd = unsafe { std::os::fd::OwnedFd::from_raw_fd(pidfd) };
            let pid = i32::try_from(child_pid)
                .map_err(|_| OciError::os("clone3 into a cgroup", "a pid out of range"))?;
            Ok(Some((pid, pidfd)))
        }
    }
}

/// The raw clone3 with the child started on `args.stack` at `entry(arg)`: the kernel's return in the
/// parent (the child's pid, or a negated errno), or `None` on an architecture without this trampoline.
/// The child never returns here: it calls `entry`, which never returns, on the stack the kernel gave it.
#[cfg(target_arch = "x86_64")]
fn clone3_on_stack(
    args: &CloneArgs,
    entry: extern "C" fn(*mut libc::c_void) -> !,
    arg: *mut libc::c_void,
) -> Option<i64> {
    let ret: i64;
    // SAFETY: clone3(2) reads `args` (a fully initialized CloneArgs of the size given) and writes the
    // child's pidfd where `args.pidfd` points. In the parent the syscall returns in rax, with rcx and r11
    // clobbered as declared, and touches nothing else of ours. In the child (rax 0) the kernel has set
    // rsp to the top of the stack `args` names, 16-byte aligned, and kept r12/r13: it calls entry(arg)
    // there, which never returns, so this function's own frame on the parent's stack is never touched
    // by the child. CLONE_VFORK keeps the parent's thread stopped until the child has exec'd or exited.
    unsafe {
        std::arch::asm!(
            "syscall",
            "test rax, rax",
            "jnz 2f",
            "mov rdi, r12",
            "call r13",
            "ud2",
            "2:",
            inlateout("rax") libc::SYS_clone3 => ret,
            in("rdi") std::ptr::from_ref(args),
            in("rsi") std::mem::size_of::<CloneArgs>(),
            in("r12") arg,
            in("r13") entry,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    Some(ret)
}

/// The same trampoline on aarch64: the syscall in x8, its arguments in x0 and x1; in the child (x0 0) the
/// kernel has set sp to the top of the stack `args` names and kept x9/x10, and it branches to entry(arg).
#[cfg(target_arch = "aarch64")]
fn clone3_on_stack(
    args: &CloneArgs,
    entry: extern "C" fn(*mut libc::c_void) -> !,
    arg: *mut libc::c_void,
) -> Option<i64> {
    let ret: i64;
    // SAFETY: as the x86_64 trampoline's: clone3(2) reads `args` and writes the child's pidfd where it
    // points; the parent gets the result in x0 with every other register kept; the child calls
    // entry(arg) on its own stack and never returns, so this frame is never touched by it.
    unsafe {
        std::arch::asm!(
            "svc 0",
            "cbnz x0, 2f",
            "mov x0, x10",
            "blr x9",
            "brk #0x1",
            "2:",
            inlateout("x0") std::ptr::from_ref(args) => ret,
            in("x1") std::mem::size_of::<CloneArgs>(),
            in("x8") libc::SYS_clone3,
            in("x9") entry,
            in("x10") arg,
            options(nostack),
        );
    }
    Some(ret)
}

/// No trampoline here: the seed's own clone path serves.
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn clone3_on_stack(
    _args: &CloneArgs,
    _entry: extern "C" fn(*mut libc::c_void) -> !,
    _arg: *mut libc::c_void,
) -> Option<i64> {
    None
}

/// Writes `hex oci exec: <what>: errno <n>` to stderr with write(2) alone, the number made on the stack:
/// a vfork-style clone's child reports why it couldn't start the program without allocating.
pub fn report(failed: Failure) {
    let mut digits = [0u8; 12];
    let mut at = digits.len();
    let mut n = failed.errno.raw_os_error().unsigned_abs();
    loop {
        at = at.saturating_sub(1);
        if let Some(d) = digits.get_mut(at) {
            *d = b'0'.saturating_add(u8::try_from(n % 10).unwrap_or(0));
        }
        n /= 10;
        if n == 0 || at == 0 {
            break;
        }
    }
    let number = digits.get(at..).unwrap_or_default();
    for part in [
        b"hex oci exec: ".as_slice(),
        failed.what.as_bytes(),
        b": errno ",
        number,
        b"\n",
    ] {
        // SAFETY: write(2) of a live buffer to descriptor 2; async-signal-safe. A short or failed write
        // loses the report, which the program's exit status still carries.
        unsafe {
            libc::write(2, part.as_ptr().cast(), part.len());
        }
    }
}

/// Ends a forked child at once with status `code`, running no `atexit` handler and flushing no
/// buffer: the seed's intermediate and the program it forks must not return up into the seed's serve
/// loop, nor flush the seed's buffers a second time (`_exit(2)`, async-signal-safe). The CLI helpers
/// that re-exec'd first (`hex __oci-exec`, `hex __oci-init`) return their status up instead; this is
/// for a fork that did not re-exec.
pub fn exit_child(code: u8) -> ! {
    // SAFETY: _exit(2) takes an int and never returns; it makes no allocation and touches no shared
    // state, so it is sound in a forked child that may run only async-signal-safe calls.
    unsafe { libc::_exit(i32::from(code)) }
}

/// Signals blocked for this process and read from a `signalfd` instead, so a parent can pass them on to
/// a child it waits on, with no handler: `hex oci exec` forwards the signals a caller ends a step with.
pub struct ForwardedSignals {
    old: libc::sigset_t,
    fd: std::os::fd::OwnedFd,
}

/// Blocks `signals` and opens a `signalfd` that reads them. The caller is single-threaded (it
/// re-exec'd), so the process's mask is this thread's.
pub fn forward_signals(signals: &[i32]) -> Result<ForwardedSignals, OciError> {
    // SAFETY: `sigset_t` is plain data, and sigemptyset initializes it before anything reads it.
    let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
    // SAFETY: sigemptyset(3) on a valid pointer to a local `sigset_t`; the return value is checked.
    if unsafe { libc::sigemptyset(&mut set) } == -1 {
        return Err(OciError::os("sigemptyset", std::io::Error::last_os_error()));
    }
    for &sig in signals {
        // SAFETY: sigaddset(3) on a valid pointer to the initialized set; the return value is checked.
        if unsafe { libc::sigaddset(&mut set, sig) } == -1 {
            return Err(OciError::os("sigaddset", std::io::Error::last_os_error()));
        }
    }
    // SAFETY: as above; sigprocmask fills it with the mask it replaces before anything reads it.
    let mut old: libc::sigset_t = unsafe { std::mem::zeroed() };
    // SAFETY: sigprocmask(2) with valid pointers to two local sets; it writes only `old`. The return
    // value is checked.
    if unsafe { libc::sigprocmask(libc::SIG_BLOCK, &set, &mut old) } == -1 {
        return Err(OciError::os("sigprocmask", std::io::Error::last_os_error()));
    }
    // SAFETY: signalfd(2) with -1 makes a new descriptor reading the signals in `set`, a valid
    // pointer; the return value is checked.
    let fd = unsafe { libc::signalfd(-1, &set, libc::SFD_CLOEXEC) };
    if fd == -1 {
        let e = std::io::Error::last_os_error();
        // SAFETY: sigprocmask(2) restoring the mask just saved; nothing else to undo.
        unsafe { libc::sigprocmask(libc::SIG_SETMASK, &old, std::ptr::null_mut()) };
        return Err(OciError::os("signalfd", e));
    }
    // SAFETY: `fd` is the new descriptor signalfd just returned, owned by nothing else.
    let fd = unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(fd) };
    Ok(ForwardedSignals { old, fd })
}

impl ForwardedSignals {
    /// The `signalfd`, for polling beside the child's pidfd.
    pub fn fd(&self) -> std::os::fd::BorrowedFd<'_> {
        std::os::fd::AsFd::as_fd(&self.fd)
    }

    /// Unblocks the signals again, as they were before: what a forked child does before it execs,
    /// which would otherwise inherit them blocked.
    pub fn restore(&self) -> Result<(), OciError> {
        // SAFETY: sigprocmask(2) setting the mask saved when the signals were blocked, a valid
        // pointer; it writes nothing. The return value is checked.
        if unsafe { libc::sigprocmask(libc::SIG_SETMASK, &self.old, std::ptr::null_mut()) } == -1 {
            return Err(OciError::os("sigprocmask", std::io::Error::last_os_error()));
        }
        Ok(())
    }

    /// The next pending signal's number, read from the `signalfd` (call it once poll says it's
    /// readable).
    pub fn next(&self) -> Result<i32, OciError> {
        let mut info = [0u8; std::mem::size_of::<libc::signalfd_siginfo>()];
        let n = rustix::io::read(&self.fd, &mut info)
            .map_err(|e| OciError::os("reading the signalfd", e))?;
        if n != info.len() {
            return Err(OciError::os(
                "reading the signalfd",
                "a short read of signalfd_siginfo",
            ));
        }
        // `ssi_signo`, the struct's first field, a `u32` (signalfd(2)).
        let signo = info
            .first_chunk::<4>()
            .map(|b| u32::from_ne_bytes(*b))
            .ok_or_else(|| OciError::os("reading the signalfd", "no ssi_signo"))?;
        i32::try_from(signo)
            .map_err(|_| OciError::os("reading the signalfd", "a signal out of range"))
    }
}

/// Reaps child `pid`, waiting for it to end; its exit as a `waitpid` status, or the error.
pub fn waitpid(pid: i32) -> Result<WaitStatus, OciError> {
    let mut status: libc::c_int = 0;
    // SAFETY: waitpid(2) writing through a valid pointer to a local `c_int`. The return value is
    // checked; the status is read only on success.
    let r = unsafe { libc::waitpid(pid, &mut status, 0) };
    if r == -1 {
        return Err(OciError::os("waitpid", std::io::Error::last_os_error()));
    }
    Ok(WaitStatus { raw: status })
}

/// A `waitpid` status.
pub struct WaitStatus {
    raw: libc::c_int,
}

impl WaitStatus {
    /// The exit code, when the child exited normally.
    pub fn exit_code(&self) -> Option<i32> {
        if libc::WIFEXITED(self.raw) {
            Some(libc::WEXITSTATUS(self.raw))
        } else {
            None
        }
    }

    /// The signal that ended the child, when one did.
    pub fn term_signal(&self) -> Option<i32> {
        if libc::WIFSIGNALED(self.raw) {
            Some(libc::WTERMSIG(self.raw))
        } else {
            None
        }
    }
}

/// Sets signal `sig` to its default disposition, unblocks `sig` alone, and raises it on this process,
/// so the process ends the way the signal says: `hex __oci-exec` in seam mode ends by the signal that
/// ended its program, so the node agent's pidfd watch reads that outcome. Only `sig` is unblocked, so
/// another forwarded signal left pending can't be delivered first and misreport. SIGKILL is raised as
/// it is: its disposition can't be changed nor the signal blocked, and signal(2) refuses to try
/// (EINVAL; signal(7)), which would leave the program's SIGKILL reported as an exit of 137. Returns
/// only if the raise didn't end it.
pub fn raise_default(sig: i32) -> Result<(), OciError> {
    if sig != libc::SIGKILL {
        default_and_unblock(sig)?;
    }
    // SAFETY: raise(3) with a valid signal number; no pointers, writes nothing. Checked.
    if unsafe { libc::raise(sig) } != 0 {
        return Err(OciError::os("raise", std::io::Error::last_os_error()));
    }
    Ok(())
}

/// Sets `sig` to its default disposition and unblocks it alone.
fn default_and_unblock(sig: i32) -> Result<(), OciError> {
    // SAFETY: signal(2) with SIG_DFL and a valid signal number installs the default disposition and
    // writes nothing of ours; the return value is checked against SIG_ERR.
    if unsafe { libc::signal(sig, libc::SIG_DFL) } == libc::SIG_ERR {
        return Err(OciError::os(
            "signal SIG_DFL",
            std::io::Error::last_os_error(),
        ));
    }
    // SAFETY: `sigset_t` is plain data; sigemptyset then sigaddset initialize it before it is read.
    let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
    // SAFETY: sigemptyset(3) on a valid pointer to the local set; return checked.
    if unsafe { libc::sigemptyset(&mut set) } == -1 {
        return Err(OciError::os("sigemptyset", std::io::Error::last_os_error()));
    }
    // SAFETY: sigaddset(3) adding `sig` to the initialized local set; return checked.
    if unsafe { libc::sigaddset(&mut set, sig) } == -1 {
        return Err(OciError::os("sigaddset", std::io::Error::last_os_error()));
    }
    // SAFETY: sigprocmask(2) unblocking just `sig` from a valid local set; the old-mask pointer is
    // null, so it writes nothing of ours. Return checked.
    if unsafe { libc::sigprocmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut()) } == -1 {
        return Err(OciError::os(
            "sigprocmask SIG_UNBLOCK",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(())
}

/// Sends signal `sig` to the container: to the process group when `all`, else to the init `pid`.
pub fn kill(pid: i32, sig: i32, all: bool) -> Result<(), OciError> {
    let target = if all {
        pid.checked_neg()
            .ok_or_else(|| OciError::os("kill", "pid has no negation"))?
    } else {
        pid
    };
    // SAFETY: kill(2) with a pid (or negated pid for the group) and a signal number; no pointers,
    // writes nothing. The return value is checked.
    let r = unsafe { libc::kill(target, sig) };
    if r == -1 {
        Err(OciError::os("kill", std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

/// Sets whether the process keeps its permitted capabilities across a uid change
/// (`PR_SET_KEEPCAPS`), so a container can drop to a non-root user and still hold the capabilities
/// its spec grants.
pub fn set_keepcaps(keep: bool) -> Result<(), rustix::io::Errno> {
    prctl2(libc::PR_SET_KEEPCAPS, libc::c_ulong::from(u8::from(keep)))
}

/// Joins the namespace the open `fd` refers to, checking it is of type `nstype` (a `CLONE_NEW*`
/// flag); `setns` fails if the fd's namespace is of another type.
pub fn setns(fd: std::os::fd::BorrowedFd<'_>, nstype: i32) -> Result<(), OciError> {
    use std::os::fd::AsRawFd;
    // SAFETY: setns(2) with a file descriptor and a type flag; it takes no pointers and writes
    // nothing. The return value is checked.
    let r = unsafe { libc::setns(fd.as_raw_fd(), nstype) };
    if r == -1 {
        Err(OciError::os("setns", std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

/// `unshare` of namespace flags. Hex never passes `CLONE_FILES`, the one flag that makes
/// `unshare` unsound (it can detach file descriptors another thread is using); this is the init,
/// single-threaded after its re-exec, and uses only the `NEW*` namespace flags.
pub fn unshare(flags: rustix::thread::UnshareFlags) -> Result<(), OciError> {
    // CLONE_FILES is the one flag that makes `unshare` unsound; clear it defensively (callers only
    // ever pass namespace flags) so the invariant below holds whatever the caller passed.
    let flags = flags.difference(rustix::thread::UnshareFlags::FILES);
    if flags.is_empty() {
        return Ok(());
    }
    // SAFETY: the flags are namespace flags only (CLONE_FILES cleared above), so no file-descriptor
    // table is detached; the invariant `unshare_unsafe` documents holds.
    unsafe { rustix::thread::unshare_unsafe(flags) }.map_err(|e| OciError::os("unshare", e))
}

/// Drops capability number `cap` from the process's bounding set (`PR_CAPBSET_DROP`).
pub fn capbset_drop(cap: u32) -> Result<(), rustix::io::Errno> {
    prctl2(libc::PR_CAPBSET_DROP, libc::c_ulong::from(cap))
}

/// Clears the process's ambient capability set (`PR_CAP_AMBIENT_CLEAR_ALL`).
pub fn ambient_clear() -> Result<(), rustix::io::Errno> {
    // SAFETY: prctl(PR_CAP_AMBIENT, PR_CAP_AMBIENT_CLEAR_ALL, 0, 0, 0) takes no pointers; checked.
    let r = unsafe {
        libc::prctl(
            libc::PR_CAP_AMBIENT,
            libc::PR_CAP_AMBIENT_CLEAR_ALL,
            0,
            0,
            0,
        )
    };
    if r == -1 { Err(last_errno()) } else { Ok(()) }
}

/// Raises capability number `cap` in the process's ambient set (`PR_CAP_AMBIENT_RAISE`).
pub fn ambient_raise(cap: u32) -> Result<(), rustix::io::Errno> {
    // SAFETY: prctl(PR_CAP_AMBIENT, PR_CAP_AMBIENT_RAISE, cap, 0, 0) takes no pointers; checked.
    let r = unsafe {
        libc::prctl(
            libc::PR_CAP_AMBIENT,
            libc::PR_CAP_AMBIENT_RAISE,
            libc::c_ulong::from(cap),
            0,
            0,
        )
    };
    if r == -1 { Err(last_errno()) } else { Ok(()) }
}

fn prctl2(option: libc::c_int, arg2: libc::c_ulong) -> Result<(), rustix::io::Errno> {
    // SAFETY: prctl with a scalar option and one scalar argument (here a capability number); it
    // takes no pointers and writes nothing. The return value is checked.
    let r = unsafe { libc::prctl(option, arg2, 0, 0, 0) };
    if r == -1 { Err(last_errno()) } else { Ok(()) }
}

/// This thread's errno, as plain data: no allocation, so the child of a vfork-style clone may read it.
fn last_errno() -> rustix::io::Errno {
    rustix::io::Errno::from_raw_os_error(
        std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EIO),
    )
}

// `bpf(2)`'s commands and the device program's type and attach point (include/uapi/linux/bpf.h).
const BPF_PROG_LOAD: libc::c_long = 5;
const BPF_PROG_ATTACH: libc::c_long = 8;
const BPF_PROG_GET_FD_BY_ID: libc::c_long = 13;
const BPF_OBJ_GET_INFO_BY_FD: libc::c_long = 15;
const BPF_PROG_TYPE_CGROUP_DEVICE: u32 = 15;
const BPF_CGROUP_DEVICE: u32 = 6;
/// `BPF_F_ALLOW_MULTI` (1 << 1): the program joins whatever the cgroup's ancestors attached, all of
/// which must allow an access, as runc and crun attach theirs.
const BPF_F_ALLOW_MULTI: u32 = 2;
/// Most bytes of the verifier's log kept when it refuses a program: enough for the refusing
/// instruction and its context; the kernel truncates the log at the buffer's end.
const VERIFIER_LOG_BYTES: usize = 65_536;

/// The `union bpf_attr` member `BPF_PROG_LOAD` reads, as the kernel lays it out; fields past
/// `expected_attach_type` are absent, which the kernel takes as zero.
#[repr(C)]
struct ProgLoadAttr {
    prog_type: u32,
    insn_cnt: u32,
    insns: u64,
    license: u64,
    log_level: u32,
    log_size: u32,
    log_buf: u64,
    kern_version: u32,
    prog_flags: u32,
    prog_name: [u8; 16],
    prog_ifindex: u32,
    expected_attach_type: u32,
}

/// The `union bpf_attr` member `BPF_PROG_ATTACH` reads.
#[repr(C)]
struct ProgAttachAttr {
    target_fd: u32,
    attach_bpf_fd: u32,
    attach_type: u32,
    attach_flags: u32,
    replace_bpf_fd: u32,
}

/// An address the kernel reads through, as `bpf_attr`'s `__aligned_u64`.
fn user_ptr<T: ?Sized>(p: *const T) -> Result<u64, OciError> {
    u64::try_from(p.cast::<u8>().expose_provenance())
        .map_err(|_| OciError::os("bpf", "an address wider than 64 bits"))
}

/// Loads a `BPF_PROG_TYPE_CGROUP_DEVICE` program ([`super::resources::compile`]'s output). When the
/// verifier refuses it, the error carries the tail of the verifier's log.
pub fn load_device_program(
    insns: &[super::resources::Insn],
) -> Result<std::os::fd::OwnedFd, OciError> {
    let insn_cnt = u32::try_from(insns.len())
        .map_err(|_| OciError::os("loading the device program", "too many instructions"))?;
    let license = c"MIT";
    // The verifier's log, only asked for when a first load is refused; it lives to the function's end,
    // past every call that may write it.
    let mut log: Vec<u8> = Vec::new();
    let mut attr = ProgLoadAttr {
        prog_type: BPF_PROG_TYPE_CGROUP_DEVICE,
        insn_cnt,
        insns: user_ptr(insns.as_ptr())?,
        license: user_ptr(license.as_ptr())?,
        log_level: 0,
        log_size: 0,
        log_buf: 0,
        kern_version: 0,
        prog_flags: 0,
        prog_name: *b"hex_devices\0\0\0\0\0",
        prog_ifindex: 0,
        expected_attach_type: 0,
    };
    for verbose in [false, true] {
        if verbose {
            log = vec![0u8; VERIFIER_LOG_BYTES];
            attr.log_level = 1;
            attr.log_size = u32::try_from(log.len()).unwrap_or(u32::MAX);
            attr.log_buf = user_ptr(log.as_mut_ptr())?;
        }
        // SAFETY: bpf(BPF_PROG_LOAD) reads `attr` (a live, initialised repr(C) struct of the size
        // passed) and, through it, `insns` (insn_cnt live instructions laid out as struct bpf_insn), the
        // NUL-terminated license, and writes at most log_size bytes into `log`, which outlives the call.
        // The return value is checked before it is owned.
        let r = unsafe {
            libc::syscall(
                libc::SYS_bpf,
                BPF_PROG_LOAD,
                &raw const attr,
                std::mem::size_of::<ProgLoadAttr>(),
            )
        };
        if r >= 0 {
            let fd =
                i32::try_from(r).map_err(|_| OciError::os("bpf", "a descriptor out of range"))?;
            use std::os::fd::FromRawFd;
            // SAFETY: a successful BPF_PROG_LOAD returns a new descriptor no one else holds.
            return Ok(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) });
        }
        let err = std::io::Error::last_os_error();
        if verbose || err.raw_os_error() != Some(libc::EINVAL) {
            let text = String::from_utf8_lossy(&log);
            let tail: String = text
                .trim_end_matches('\0')
                .chars()
                .rev()
                .take(500)
                .collect();
            let tail: String = tail.chars().rev().collect();
            return Err(OciError::os(
                "loading the device program",
                if tail.is_empty() {
                    err.to_string()
                } else {
                    format!("{err}: verifier: {tail}")
                },
            ));
        }
    }
    Err(OciError::os(
        "loading the device program",
        "the verifier refused it",
    ))
}

/// Attaches device program `prog` to the cgroup whose directory is open as `cgroup`.
pub fn attach_device_program(
    cgroup: std::os::fd::BorrowedFd<'_>,
    prog: std::os::fd::BorrowedFd<'_>,
) -> Result<(), OciError> {
    use std::os::fd::AsRawFd;
    let fd = |f: std::os::fd::BorrowedFd<'_>| {
        u32::try_from(f.as_raw_fd()).map_err(|_| OciError::os("bpf", "a negative descriptor"))
    };
    let attr = ProgAttachAttr {
        target_fd: fd(cgroup)?,
        attach_bpf_fd: fd(prog)?,
        attach_type: BPF_CGROUP_DEVICE,
        attach_flags: BPF_F_ALLOW_MULTI,
        replace_bpf_fd: 0,
    };
    // SAFETY: bpf(BPF_PROG_ATTACH) reads `attr`, a live, initialised repr(C) struct of the size
    // passed, holding two descriptors this process has open; it writes nothing of ours.
    let r = unsafe {
        libc::syscall(
            libc::SYS_bpf,
            BPF_PROG_ATTACH,
            &raw const attr,
            std::mem::size_of::<ProgAttachAttr>(),
        )
    };
    if r == 0 {
        Ok(())
    } else {
        Err(OciError::os(
            "attaching the device program",
            std::io::Error::last_os_error(),
        ))
    }
}

/// Loads seccomp filter `program` on the calling thread and leaves no_new_privs as it is: the kernel
/// takes a filter from a thread without no_new_privs when it has CAP_SYS_ADMIN in its user namespace
/// (seccomp(2), `SECCOMP_SET_MODE_FILTER`). [`seccompiler::apply_filter`] sets no_new_privs first,
/// which a spec without `noNewPrivileges` doesn't ask for.
pub fn load_seccomp_filter(filter: &[libc::sock_filter]) -> Result<(), rustix::io::Errno> {
    let len = u16::try_from(filter.len()).map_err(|_| rustix::io::Errno::INVAL)?;
    let prog = libc::sock_fprog {
        len,
        filter: filter.as_ptr().cast_mut(),
    };
    // SAFETY: seccomp(SECCOMP_SET_MODE_FILTER) reads `prog`, a live sock_fprog whose `filter` points at
    // `len` live, initialised sock_filter instructions that outlive the call; the kernel copies them
    // and keeps no pointer into them, and it writes nothing of ours. Checked.
    let r = unsafe {
        libc::syscall(
            libc::SYS_seccomp,
            libc::SECCOMP_SET_MODE_FILTER,
            0,
            &raw const prog,
        )
    };
    if r == 0 { Ok(()) } else { Err(last_errno()) }
}

/// The `union bpf_attr` member `BPF_PROG_GET_FD_BY_ID` reads.
#[repr(C)]
struct GetFdByIdAttr {
    prog_id: u32,
    next_id: u32,
    open_flags: u32,
}

/// The `union bpf_attr` member `BPF_OBJ_GET_INFO_BY_FD` reads.
#[repr(C)]
struct InfoByFdAttr {
    bpf_fd: u32,
    info_len: u32,
    info: u64,
}

/// What the kernel says a loaded program is: the head of `struct bpf_prog_info`
/// (include/uapi/linux/bpf.h) as far as `xlated_prog_len`. The tag is the first 8 bytes of the digest
/// the kernel takes of the instructions it was loaded from (SHA-256 in Linux 7.2: kernel/bpf/core.c,
/// `bpf_prog_calc_tag`): a quick check, not a proof, that two programs are one;
/// [`program_instructions`] is the proof. The two lengths are zero on the way in, which asks the
/// kernel for no instruction dump.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ProgramIdentity {
    pub prog_type: u32,
    pub id: u32,
    pub tag: [u8; 8],
    pub jited_len: u32,
    pub xlated_len: u32,
}

/// The head of `struct bpf_prog_info` through `xlated_prog_insns`: with `xlated_prog_len` and
/// `xlated_prog_insns` set, the kernel dumps the program's translated instructions there.
#[repr(C)]
#[derive(Default)]
struct InfoWithDump {
    head: ProgramIdentity,
    jited_prog_insns: u64,
    xlated_prog_insns: u64,
}

/// Most bytes of translated instructions [`program_instructions`] reads: `BPF_MAXINSNS` (4,096,
/// include/uapi/linux/bpf_common.h) instructions of 8 bytes, which Hex's device programs stay under
/// (`resources::MAX_DEVICE_EXCEPTIONS`).
const MAX_DUMP_BYTES: u32 = 4096 * 8;

/// The identity of the loaded program open as `prog`.
pub fn program_identity(prog: std::os::fd::BorrowedFd<'_>) -> Result<ProgramIdentity, OciError> {
    use std::os::fd::AsRawFd;
    let mut info = ProgramIdentity::default();
    let attr = InfoByFdAttr {
        bpf_fd: u32::try_from(prog.as_raw_fd())
            .map_err(|_| OciError::os("bpf", "a negative descriptor"))?,
        info_len: u32::try_from(std::mem::size_of::<ProgramIdentity>())
            .map_err(|_| OciError::os("bpf", "an info size out of range"))?,
        info: user_ptr((&raw mut info).cast_const())?,
    };
    // SAFETY: bpf(BPF_OBJ_GET_INFO_BY_FD) reads `attr`, a live, initialised repr(C) struct of the size
    // passed, and writes at most `info_len` bytes into `info`, a live repr(C) struct of exactly that
    // size that outlives the call; its zero lengths ask for no dump through any pointer. Checked.
    let r = unsafe {
        libc::syscall(
            libc::SYS_bpf,
            BPF_OBJ_GET_INFO_BY_FD,
            &raw const attr,
            std::mem::size_of::<InfoByFdAttr>(),
        )
    };
    if r == 0 {
        Ok(info)
    } else {
        Err(OciError::os(
            "reading a device program's identity",
            std::io::Error::last_os_error(),
        ))
    }
}

/// The translated instructions of the loaded program open as `prog`, as the kernel dumps them
/// (kernel/bpf/syscall.c, `bpf_prog_get_info_by_fd`), when there are exactly `len` bytes of them.
/// `None` when there aren't, when `len` passes [`MAX_DUMP_BYTES`], or when the kernel dumps none: it
/// withholds them from a caller without CAP_BPF, and from one without raw-dump rights when it blinded
/// the program's constants (clearing the pointer to say so).
pub fn program_instructions(
    prog: std::os::fd::BorrowedFd<'_>,
    len: u32,
) -> Result<Option<Vec<u8>>, OciError> {
    use std::os::fd::AsRawFd;
    if len == 0 || len > MAX_DUMP_BYTES {
        return Ok(None);
    }
    let mut buf = vec![
        0u8;
        usize::try_from(len)
            .map_err(|_| OciError::os("bpf", "a dump length out of range"))?
    ];
    let mut info = InfoWithDump::default();
    info.head.xlated_len = len;
    info.xlated_prog_insns = user_ptr(buf.as_mut_ptr())?;
    let attr = InfoByFdAttr {
        bpf_fd: u32::try_from(prog.as_raw_fd())
            .map_err(|_| OciError::os("bpf", "a negative descriptor"))?,
        info_len: u32::try_from(std::mem::size_of::<InfoWithDump>())
            .map_err(|_| OciError::os("bpf", "an info size out of range"))?,
        info: user_ptr((&raw mut info).cast_const())?,
    };
    // SAFETY: bpf(BPF_OBJ_GET_INFO_BY_FD) reads `attr`, a live, initialised repr(C) struct of the size
    // passed. It writes at most `info_len` bytes into `info`, a live repr(C) struct of exactly that
    // size, and at most `xlated_len` bytes through `xlated_prog_insns` into `buf`, a live buffer of
    // that length; both outlive the call. `jited_len` is zero, so nothing is written through
    // `jited_prog_insns`. Checked.
    let r = unsafe {
        libc::syscall(
            libc::SYS_bpf,
            BPF_OBJ_GET_INFO_BY_FD,
            &raw const attr,
            std::mem::size_of::<InfoByFdAttr>(),
        )
    };
    if r != 0 {
        return Err(OciError::os(
            "reading a device program's instructions",
            std::io::Error::last_os_error(),
        ));
    }
    // The kernel reports the whole translation's length, and a cleared pointer when it withheld it.
    if info.head.xlated_len != len || info.xlated_prog_insns == 0 {
        return Ok(None);
    }
    Ok(Some(buf))
}

/// A descriptor for loaded program `id`, or `None` when no program has that id now. Opening a
/// program by id needs CAP_SYS_ADMIN, which the runtime that loads device programs has.
pub fn program_by_id(id: u32) -> Result<Option<std::os::fd::OwnedFd>, OciError> {
    let attr = GetFdByIdAttr {
        prog_id: id,
        next_id: 0,
        open_flags: 0,
    };
    // SAFETY: bpf(BPF_PROG_GET_FD_BY_ID) reads `attr`, a live, initialised repr(C) struct of the size
    // passed; it writes nothing of ours. The return value is checked before it is owned.
    let r = unsafe {
        libc::syscall(
            libc::SYS_bpf,
            BPF_PROG_GET_FD_BY_ID,
            &raw const attr,
            std::mem::size_of::<GetFdByIdAttr>(),
        )
    };
    if r >= 0 {
        let fd = i32::try_from(r).map_err(|_| OciError::os("bpf", "a descriptor out of range"))?;
        use std::os::fd::FromRawFd;
        // SAFETY: a successful BPF_PROG_GET_FD_BY_ID returns a new descriptor no one else holds.
        return Ok(Some(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) }));
    }
    let err = std::io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::ENOENT) {
        Ok(None)
    } else {
        Err(OciError::os("opening a device program by id", err))
    }
}

/// Owns an inherited file-descriptor number the runtime passed through the environment.
pub fn own_fd(fd: std::os::fd::RawFd) -> Result<std::os::fd::OwnedFd, OciError> {
    use std::os::fd::FromRawFd;
    if fd < 0 {
        return Err(OciError::os("fd", "a negative file descriptor number"));
    }
    // SAFETY: the `create` runtime opened this pipe end and passed its number here with CLOEXEC
    // cleared, having kept it out of every other use, so this process is its sole owner.
    Ok(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) })
}
