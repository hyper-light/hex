//! The system calls the OCI runtime makes that Rust can't make safely (CLAUDE.md §12 names this
//! file). The runtime re-execs itself for the container init (`hex __oci-init`), so the process that
//! calls [`fork`] is single-threaded — the fork-in-a-threaded-process hazard does not apply here,
//! and the child runs ordinary Rust, not only async-signal-safe calls.

#![allow(unsafe_code)]

use crate::linux::oci::OciError;

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

/// Whether process `pid` exists (sending signal 0), so `state` can tell a live container from a
/// stopped one without reaping it.
pub fn alive(pid: i32) -> bool {
    // SAFETY: kill(pid, 0) checks for the process's existence and delivers no signal; it writes
    // nothing. Any error (ESRCH, EPERM) is turned into a bool.
    unsafe { libc::kill(pid, 0) == 0 }
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
pub fn set_keepcaps(keep: bool) -> Result<(), OciError> {
    prctl2(
        libc::PR_SET_KEEPCAPS,
        libc::c_ulong::from(u8::from(keep)),
        "PR_SET_KEEPCAPS",
    )
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
pub fn capbset_drop(cap: u32) -> Result<(), OciError> {
    prctl2(
        libc::PR_CAPBSET_DROP,
        libc::c_ulong::from(cap),
        "PR_CAPBSET_DROP",
    )
}

/// Clears the process's ambient capability set (`PR_CAP_AMBIENT_CLEAR_ALL`).
pub fn ambient_clear() -> Result<(), OciError> {
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
    if r == -1 {
        Err(OciError::os(
            "PR_CAP_AMBIENT_CLEAR_ALL",
            std::io::Error::last_os_error(),
        ))
    } else {
        Ok(())
    }
}

/// Raises capability number `cap` in the process's ambient set (`PR_CAP_AMBIENT_RAISE`).
pub fn ambient_raise(cap: u32) -> Result<(), OciError> {
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
    if r == -1 {
        Err(OciError::os(
            "PR_CAP_AMBIENT_RAISE",
            std::io::Error::last_os_error(),
        ))
    } else {
        Ok(())
    }
}

fn prctl2(option: libc::c_int, arg2: libc::c_ulong, what: &'static str) -> Result<(), OciError> {
    // SAFETY: prctl with a scalar option and one scalar argument (here a capability number); it
    // takes no pointers and writes nothing. The return value is checked.
    let r = unsafe { libc::prctl(option, arg2, 0, 0, 0) };
    if r == -1 {
        Err(OciError::os(what, std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

// `bpf(2)`'s commands and the device program's type and attach point (include/uapi/linux/bpf.h).
const BPF_PROG_LOAD: libc::c_long = 5;
const BPF_PROG_ATTACH: libc::c_long = 8;
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
