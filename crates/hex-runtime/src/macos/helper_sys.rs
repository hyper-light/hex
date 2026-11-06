//! The macOS box helper daemon's privileged syscalls (native plan step 2): the ones that need root and
//! that Rust can't make safely. CLAUDE.md §12 names this file. Each `unsafe` block states the invariant
//! that makes it sound. The daemon's membership reads and kills ([`super::helper_daemon::members`],
//! [`super::helper_daemon::kill_user`]) need no privilege and aren't here; this is [`RealPriv`], the real
//! [`super::helper_daemon::Privileged`] backend, used when the daemon runs as root.
//!
//! - **Starting a box's program as its pool user** ([`RealPriv::spawn`]): the daemon forks and, in the
//!   child, drops to the box user (`setgid`/`setuid`), opens a session for a leader (`setsid`), caps the
//!   user's processes (`setrlimit(RLIMIT_NPROC)`), puts the three stdio descriptors on 0/1/2 (`dup2`) and
//!   closes the rest, then `execve`s the program — every call async-signal-safe, as the Linux runtime's
//!   `pre_exec` is, since between fork and exec a multi-threaded process may touch no lock. `posix_spawn`
//!   can't change uid, so this is a fork, not a spawn.
//! - **Arming a member's footprint and routing its exception** ([`RealPriv::arm`]): `task_for_pid` (root)
//!   for the member's task, `task_set_phys_footprint_limit` for its ledger limit, and
//!   `task_set_exception_ports(EXC_MASK_RESOURCE)` so a crossing is delivered to the daemon's port, not
//!   crash reporting. The limit is non-fatal, so the kernel notifies rather than kills, and the daemon
//!   re-apportions or ends the box (Ada's call, 2026-11-01).
//! - **The daemon's exception port** ([`RealPriv::new`]): a Mach receive right the crossings arrive on.
//!
//! libc binds `fork`/`setgid`/`setuid`/`setsid`/`setrlimit`/`dup2`/`execve`/`_exit`; the Mach routines
//! (`task_for_pid`, `task_set_phys_footprint_limit`, `task_set_exception_ports`, `mach_port_allocate`,
//! `mach_port_insert_right`) are declared here as `unsafe extern "C"`, as `sys.rs` declares the ones it
//! calls. The spawn is validated as root on the Mac Studio (the arming window and the exception delivery
//! are bench/native/macos's to measure); its POSIX half mirrors the Linux runtime's proven path.

// The box helper daemon's one place `unsafe` is allowed (CLAUDE.md §12); every block carries its SAFETY
// comment.
#![allow(unsafe_code)]

use std::ffi::{CString, c_char, c_int, c_uint};
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, OwnedFd};

use hex_api::{Proc, Program};

use super::helper_daemon::Privileged;
use super::sys::Spawn;

/// Mach types we name (`mach/port.h`, `mach/kern_return.h`): a port name and a kernel return are both
/// 32-bit, a task is a port.
type MachPort = c_uint;
type KernReturn = c_int;

// `mach_task_self()` and the Mach routines the arm uses, declared as libc declares the ones it binds.
// SAFETY: each is declared as `<mach/task.h>`/`<mach/mach_port.h>` declare it; they are kernel traps that
// read their scalar arguments and the one port/limit each takes, and write their out-parameter.
unsafe extern "C" {
    fn mach_task_self() -> MachPort;
    fn task_for_pid(target: MachPort, pid: c_int, task: *mut MachPort) -> KernReturn;
    fn task_set_phys_footprint_limit(
        task: MachPort,
        new_limit_mb: c_int,
        old_limit_mb: *mut c_int,
    ) -> KernReturn;
    fn task_set_exception_ports(
        task: MachPort,
        exception_mask: c_uint,
        new_port: MachPort,
        behavior: c_int,
        flavor: c_int,
    ) -> KernReturn;
    fn mach_port_allocate(task: MachPort, right: c_int, name: *mut MachPort) -> KernReturn;
    fn mach_port_insert_right(
        task: MachPort,
        name: MachPort,
        poly: MachPort,
        poly_type: c_uint,
    ) -> KernReturn;
    fn mach_msg(
        msg: *mut MsgHeader,
        option: c_int,
        send_size: c_uint,
        rcv_size: c_uint,
        rcv_name: MachPort,
        timeout: c_uint,
        notify: MachPort,
    ) -> KernReturn;
    fn mach_port_deallocate(task: MachPort, name: MachPort) -> KernReturn;
}

const KERN_SUCCESS: KernReturn = 0;
/// `EXC_MASK_RESOURCE` = `1 << EXC_RESOURCE` (`mach/exception_types.h`: `EXC_RESOURCE` is 11).
const EXC_MASK_RESOURCE: c_uint = 1 << 11;
/// `EXCEPTION_DEFAULT | MACH_EXCEPTION_CODES` (`mach/exception_types.h`): the behavior the daemon's
/// handler reads, carrying 64-bit codes (the footprint limit crossed).
const EXCEPTION_DEFAULT: c_int = 1;
const MACH_EXCEPTION_CODES: c_int = 0x8000_0000_u32.cast_signed();
/// `THREAD_STATE_NONE` (`mach/thread_status.h`): the handler takes no thread state.
const THREAD_STATE_NONE: c_int = 0;
/// `MACH_PORT_RIGHT_RECEIVE` (`mach/port.h`) and `MACH_MSG_TYPE_MAKE_SEND` (`mach/message.h`).
const MACH_PORT_RIGHT_RECEIVE: c_int = 1;
const MACH_MSG_TYPE_MAKE_SEND: c_uint = 20;
/// `mach_msg` options (`mach/message.h`): receive a message, send one, and time the receive out.
const MACH_RCV_MSG: c_int = 2;
const MACH_SEND_MSG: c_int = 1;
const MACH_RCV_TIMEOUT: c_int = 0x100;
/// `MACH_RCV_TIMED_OUT` (`mach/message.h`): the receive's timeout elapsed with no message.
const MACH_RCV_TIMED_OUT: KernReturn = 0x1000_4003;
/// The remote-bits mask of `msgh_bits` (`MACH_MSGH_BITS_REMOTE_MASK`), carried back on the reply.
const MACH_MSGH_BITS_REMOTE_MASK: u32 = 0x1f;

/// A Mach message header (`mach/message.h`).
#[repr(C)]
#[derive(Clone, Copy)]
struct MsgHeader {
    bits: u32,
    size: c_uint,
    remote: MachPort,
    local: MachPort,
    voucher: MachPort,
    id: c_int,
}

/// A `mach_msg_port_descriptor_t` (12 bytes on LP64): a port name, then the size and the
/// pad/disposition/type bits, which the daemon reads only to deallocate the right it carries.
#[repr(C)]
#[derive(Clone, Copy)]
struct PortDescriptor {
    name: MachPort,
    pad_size: c_uint,
    pad_bits: c_uint,
}

/// An `EXC_RESOURCE` message as delivered with `EXCEPTION_DEFAULT | MACH_EXCEPTION_CODES`: the thread and
/// task send rights, then the exception and its 64-bit codes, then room for the trailer. The daemon reads
/// the header to reply and the descriptors to free the rights; the crossing itself is the signal (which
/// box crossed is found by re-reading each box, not from the message — a non-fatal crossing arrives via a
/// corpse task, whose pid isn't the member's).
#[repr(C)]
struct ExcMessage {
    header: MsgHeader,
    descriptor_count: c_uint,
    thread: PortDescriptor,
    task: PortDescriptor,
    ndr: [u8; 8],
    exception: c_int,
    code_count: c_uint,
    code: [i64; 2],
    trailer: [u8; 68],
}

/// The reply to an exception message: `KERN_SUCCESS`, so the kernel's handler path completes.
#[repr(C)]
struct ExcReply {
    header: MsgHeader,
    ndr: [u8; 8],
    ret_code: KernReturn,
}

/// The real privileged backend, run as root by the daemon. Holds the receive right that a member's
/// `EXC_RESOURCE` is routed to; a send right to it is set as each member's exception port.
pub struct RealPriv {
    /// The daemon's exception receive port.
    recv: MachPort,
}

impl RealPriv {
    /// Allocates the daemon's exception port (a receive right, with a send right made for it). Root.
    pub fn new() -> io::Result<Self> {
        let mut recv: MachPort = 0;
        // SAFETY: mach_port_allocate writes one port name into `recv` for the given right on our own task.
        let kr =
            unsafe { mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_RECEIVE, &raw mut recv) };
        if kr != KERN_SUCCESS {
            return Err(io::Error::other(format!("mach_port_allocate: {kr}")));
        }
        // SAFETY: inserts a send right for the receive right we just made on our own task.
        let kr = unsafe {
            mach_port_insert_right(mach_task_self(), recv, recv, MACH_MSG_TYPE_MAKE_SEND)
        };
        if kr != KERN_SUCCESS {
            return Err(io::Error::other(format!("mach_port_insert_right: {kr}")));
        }
        Ok(Self { recv })
    }
}

impl Privileged for RealPriv {
    fn spawn(
        &self,
        uid: u32,
        program: &Program,
        fds: [OwnedFd; 3],
        leader: bool,
        _cap_mib: u32,
        nproc: u32,
    ) -> io::Result<i32> {
        // Build everything the child's execve needs before the fork, so the child path allocates nothing
        // and touches no lock (async-signal-safe only). The daemon supplies the stdio and the user; the
        // program's path/argv/environment/cwd are the shared `Parts`.
        let parts = super::program_parts(program).map_err(|e| io::Error::other(e.to_string()))?;
        let spawn = Spawn {
            paths: parts.paths,
            argv: parts.argv,
            envp: parts.envp,
            cwd: parts.cwd,
            stdio: fds,
            // A fork and execve, not posix_spawn, so no spawn-time jetsam cap: the daemon arms the
            // member's ledger limit itself (`arm`).
            jetsam_mib: None,
            // The default under the helper's own hard limit, as a process box's is under its node's.
            nofile: Some(super::default_nofile()),
        };
        fork_exec_as(uid, &spawn, leader, nproc)
    }

    fn arm(&self, pid: i32, cap_mib: u32) -> io::Result<()> {
        let mut task: MachPort = 0;
        // SAFETY: task_for_pid writes the task port of `pid` into `task`; root may name another task's.
        let kr = unsafe { task_for_pid(mach_task_self(), pid, &raw mut task) };
        if kr != KERN_SUCCESS {
            return Err(io::Error::other(format!("task_for_pid({pid}): {kr}")));
        }
        let limit = c_int::try_from(cap_mib).unwrap_or(c_int::MAX);
        let mut old: c_int = 0;
        // SAFETY: sets the task's physical-footprint ledger limit (MiB); writes the old limit to `old`.
        let kr = unsafe { task_set_phys_footprint_limit(task, limit, &raw mut old) };
        if kr != KERN_SUCCESS {
            return Err(io::Error::other(format!(
                "task_set_phys_footprint_limit({pid}, {cap_mib}): {kr}"
            )));
        }
        // SAFETY: routes the task's EXC_RESOURCE to the daemon's port; reads no memory past the scalars.
        let kr = unsafe {
            task_set_exception_ports(
                task,
                EXC_MASK_RESOURCE,
                self.recv,
                EXCEPTION_DEFAULT | MACH_EXCEPTION_CODES,
                THREAD_STATE_NONE,
            )
        };
        if kr != KERN_SUCCESS {
            return Err(io::Error::other(format!(
                "task_set_exception_ports({pid}): {kr}"
            )));
        }
        Ok(())
    }

    /// The daemon forked `pid`, so it is its parent: the process class's own watch and reap serve it.
    fn watch(&self, pid: i32) -> io::Result<Box<dyn Proc>> {
        super::proc::BoxProc::watch(pid, None)
            .map(|p| Box::new(p) as Box<dyn Proc>)
            .map_err(|e| io::Error::other(e.to_string()))
    }

    /// Waits up to `timeout_ms` for a member's `EXC_RESOURCE` on the daemon's port; `Ok(true)` when one
    /// arrived (a crossing: the daemon re-apportions every box), `Ok(false)` on timeout. The message's
    /// carried thread and task send rights are freed, and the kernel's handler is replied `KERN_SUCCESS`.
    fn next_crossing(&self, timeout_ms: u32) -> io::Result<bool> {
        let mut msg = MaybeUninit::<ExcMessage>::zeroed();
        let size = u32::try_from(size_of::<ExcMessage>()).unwrap_or(u32::MAX);
        // SAFETY: mach_msg receives at most `size` bytes into the ExcMessage buffer, which is that size and
        // alive for the call, on the daemon's own receive port; it writes only the message.
        let kr = unsafe {
            mach_msg(
                msg.as_mut_ptr().cast::<MsgHeader>(),
                MACH_RCV_MSG | MACH_RCV_TIMEOUT,
                0,
                size,
                self.recv,
                timeout_ms,
                0,
            )
        };
        if kr == MACH_RCV_TIMED_OUT {
            return Ok(false);
        }
        if kr != KERN_SUCCESS {
            return Err(io::Error::other(format!("mach_msg receive: {kr}")));
        }
        // SAFETY: the kernel filled the message on success; every field is plain data.
        let msg = unsafe { msg.assume_init() };
        // Free the thread and task send rights the message carried, so they don't leak.
        for (i, desc) in [msg.thread, msg.task].into_iter().enumerate() {
            if u32::try_from(i).unwrap_or(u32::MAX) < msg.descriptor_count && desc.name != 0 {
                // SAFETY: deallocates a send right the message gave this task; a bad name is a no-op error.
                unsafe {
                    mach_port_deallocate(mach_task_self(), desc.name);
                }
            }
        }
        // Reply KERN_SUCCESS so the kernel's exception path completes (the id + 100 is the reply id).
        let mut reply = ExcReply {
            header: MsgHeader {
                bits: msg.header.bits & MACH_MSGH_BITS_REMOTE_MASK,
                size: u32::try_from(size_of::<ExcReply>()).unwrap_or(u32::MAX),
                remote: msg.header.remote,
                local: 0,
                voucher: 0,
                id: msg.header.id.saturating_add(100),
            },
            ndr: msg.ndr,
            ret_code: KERN_SUCCESS,
        };
        // SAFETY: sends one ExcReply (its own size) to the remote port the request named; no receive.
        let _ = unsafe {
            mach_msg(
                &raw mut reply.header,
                MACH_SEND_MSG,
                reply.header.size,
                0,
                0,
                0,
                0,
            )
        };
        Ok(true)
    }
}

/// Forks and, in the child, drops to user `uid`, caps the user's processes at `nproc` and the program's
/// descriptors at `spawn`'s limit, puts `fds` on 0/1/2, opens a session when `leader`, and execs
/// `spawn`'s program (its first resolvable path). Returns
/// the child's pid. Every call in the child is async-signal-safe (raw libc), and the child allocates
/// nothing: a multi-threaded process may hold a lock in another thread across the fork, so the child uses
/// only what was prepared before it (`spawn`, `fds`), exactly as the Linux runtime's `pre_exec` does.
fn fork_exec_as(uid: u32, spawn: &Spawn, leader: bool, nproc: u32) -> io::Result<i32> {
    let argv = nul_terminated(&spawn.argv);
    let envp = nul_terminated(&spawn.envp);
    let stdio = [
        spawn.stdio[0].as_raw_fd(),
        spawn.stdio[1].as_raw_fd(),
        spawn.stdio[2].as_raw_fd(),
    ];
    let cwd = spawn.cwd.clone();
    let nofile = spawn.nofile.map(|n| libc::rlimit {
        rlim_cur: n.soft,
        rlim_max: n.hard,
    });
    // SAFETY: fork in a process that re-execs its child at once; the child calls only async-signal-safe
    // libc functions on the argv/envp/paths/fds prepared above, and _exit on any failure, so it never
    // runs Rust destructors or touches a lock another thread may hold.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(io::Error::last_os_error());
    }
    if pid == 0 {
        // Child. From here only async-signal-safe calls, ending in execve or _exit.
        // SAFETY: each call acts on this child alone with the values prepared before the fork.
        unsafe {
            if leader && libc::setsid() < 0 {
                libc::_exit(127);
            }
            let rl = libc::rlimit {
                rlim_cur: u64::from(nproc),
                rlim_max: u64::from(nproc),
            };
            if libc::setrlimit(libc::RLIMIT_NPROC, &raw const rl) < 0 {
                libc::_exit(127);
            }
            if let Some(limit) = &nofile
                && libc::setrlimit(libc::RLIMIT_NOFILE, limit) < 0
            {
                libc::_exit(127);
            }
            if libc::setgid(uid) < 0 || libc::setuid(uid) < 0 {
                libc::_exit(127);
            }
            // stdin/stdout/stderr onto 0/1/2 (three fixed targets, no index cast).
            if libc::dup2(stdio[0], 0) < 0
                || libc::dup2(stdio[1], 1) < 0
                || libc::dup2(stdio[2], 2) < 0
            {
                libc::_exit(127);
            }
            // Close every descriptor past stdio, so only the three cross (no leak into the box). macOS
            // has no closefrom(2) in libc, so close 3..table size — a counted bound (CLAUDE.md §2), and
            // close ignores a descriptor that isn't open.
            let max = libc::getdtablesize();
            let mut fd = 3;
            while fd < max {
                libc::close(fd);
                fd = fd.saturating_add(1);
            }
            if let Some(cwd) = &cwd
                && libc::chdir(cwd.as_ptr()) < 0
            {
                libc::_exit(127);
            }
            for path in &spawn.paths {
                libc::execve(path.as_ptr(), argv.as_ptr(), envp.as_ptr());
                // execve only returns on failure; try the next path, as execvp does.
            }
            libc::_exit(127);
        }
    }
    Ok(pid)
}

/// `strings` as the NULL-terminated `*const` pointer list `execve` takes; it borrows them (alive across
/// the call).
fn nul_terminated(strings: &[CString]) -> Vec<*const c_char> {
    strings
        .iter()
        .map(|s| s.as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect()
}
