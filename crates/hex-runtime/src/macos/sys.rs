//! The macOS calls the `process` class makes that Rust can't make safely (CLAUDE.md §12 names this
//! file): libc binds them only as `unsafe` foreign functions and rustix wraps none of them. Each `unsafe`
//! block states the invariant that makes it sound. What rustix does wrap safely (`kqueue`, `kill`,
//! `waitid`, `waitpid`, `fcntl`) is called through it, outside this file.

// The macOS class's one place `unsafe` is allowed (CLAUDE.md §12); every block below carries its SAFETY
// comment.
#![allow(unsafe_code)]

use std::ffi::{CString, c_char, c_int, c_void};
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::time::Duration;

/// Mach absolute time's units (mach_timebase_info(3)): a tick is `numer / denom` nanoseconds, 125/3 on
/// Apple silicon and 1/1 on Intel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timebase {
    numer: u32,
    denom: u32,
}

impl Timebase {
    // libc marks mach_timebase_info and its struct fields deprecated in favour of the mach2 crate, which
    // binds them identically; Hex takes them from libc, which it already links.
    #[allow(deprecated)]
    pub fn get() -> io::Result<Self> {
        let mut info = libc::mach_timebase_info { numer: 0, denom: 0 };
        // SAFETY: mach_timebase_info writes the two fields of the struct it is given, which outlives the call.
        let kr = unsafe { libc::mach_timebase_info(&raw mut info) };
        if kr != 0 || info.numer == 0 || info.denom == 0 {
            return Err(io::Error::other(format!(
                "mach_timebase_info: {kr} ({}/{})",
                info.numer, info.denom
            )));
        }
        Ok(Self {
            numer: info.numer,
            denom: info.denom,
        })
    }

    /// `ticks` of Mach absolute time in nanoseconds, saturating past 2^64 (584 years).
    pub fn nanos(&self, ticks: u64) -> u64 {
        u128::from(ticks)
            .checked_mul(u128::from(self.numer))
            .and_then(|n| n.checked_div(u128::from(self.denom)))
            .map_or(u64::MAX, |n| u64::try_from(n).unwrap_or(u64::MAX))
    }
}

/// A process's use as `proc_pid_rusage(RUSAGE_INFO_V4)` reads it (`libproc.h`). Times are Mach absolute
/// time ([`Timebase`]): XNU fills them from the task's `rm_time_mach` (`osfmk/kern/task.c:6811`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rusage {
    /// When it started: with its pid, which process it is.
    pub started: u64,
    /// It has ended and waits to be reaped. XNU keeps an ended process's use, its footprint zeroed
    /// (`bsd/kern/kern_exit.c:2075-2080`), and reads a live one's exit time as 0
    /// (`bsd/kern/kern_resource.c:3455-3462`).
    pub ended: bool,
    pub footprint: u64,
    /// The highest footprint it reached.
    pub peak: u64,
    /// CPU it used itself, user and system.
    pub cpu: u64,
    /// CPU it ran its own code with (user time): one whose user time rose over a run had a core in it.
    /// Stopped, its time is whole: XNU totals a thread's time as it leaves its core.
    pub user: u64,
    /// CPU its reaped children used.
    pub child_cpu: u64,
    pub read_bytes: u64,
    pub written_bytes: u64,
}

/// Process `pid`'s use, or `None` when no process has that pid (`ESRCH`).
pub fn rusage(pid: i32) -> io::Result<Option<Rusage>> {
    let mut ri = MaybeUninit::<libc::rusage_info_v4>::zeroed();
    // SAFETY: for RUSAGE_INFO_V4 proc_pid_rusage writes at most one rusage_info_v4 into the buffer it is
    // given, which is one, alive for the call. The C prototype takes `rusage_info_t *`, a pointer to the
    // buffer, as libc declares it.
    let r = unsafe {
        libc::proc_pid_rusage(
            pid,
            libc::RUSAGE_INFO_V4,
            ri.as_mut_ptr().cast::<libc::rusage_info_t>(),
        )
    };
    if r != 0 {
        let e = io::Error::last_os_error();
        return match e.raw_os_error() {
            Some(libc::ESRCH) => Ok(None),
            _ => Err(e),
        };
    }
    // SAFETY: zeroed above and written by the kernel; every field is an integer, so any bytes are valid.
    let ri = unsafe { ri.assume_init() };
    Ok(Some(Rusage {
        started: ri.ri_proc_start_abstime,
        ended: ri.ri_proc_exit_abstime != 0,
        footprint: ri.ri_phys_footprint,
        peak: ri.ri_lifetime_max_phys_footprint,
        cpu: ri.ri_user_time.saturating_add(ri.ri_system_time),
        user: ri.ri_user_time,
        child_cpu: ri
            .ri_child_user_time
            .saturating_add(ri.ri_child_system_time),
        read_bytes: ri.ri_diskio_bytesread,
        written_bytes: ri.ri_diskio_byteswritten,
    }))
}

/// Where a process is in its life, as `proc_pidinfo(PROC_PIDTBSDINFO)` reads it: XNU copies the process's
/// `p_stat` into `pbi_status` (`bsd/kern/proc_info.c`), whose values are `sys/proc.h`'s (`SSTOP` 4, `SZOMB`
/// 5), and a stop signal sets `SSTOP` (`bsd/kern/kern_sig.c`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Run {
    /// Stopped by a signal: it runs again only once continued.
    Stopped,
    /// Ended, waiting to be reaped.
    Ended,
    /// Being made, runnable or asleep: each can run.
    Live,
}

/// Process `pid`'s run state, or `None` when no process has that pid (`ESRCH`).
pub fn run_state(pid: i32) -> io::Result<Option<Run>> {
    let mut info = MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = c_int::try_from(std::mem::size_of::<libc::proc_bsdinfo>())
        .map_err(|_| io::Error::other("a proc_bsdinfo past an int's bytes"))?;
    // SAFETY: for PROC_PIDTBSDINFO proc_pidinfo writes at most `size` bytes, one proc_bsdinfo, into the
    // buffer it is given, which is one proc_bsdinfo, alive for the call.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast::<c_void>(),
            size,
        )
    };
    if n <= 0 {
        let e = io::Error::last_os_error();
        return match e.raw_os_error() {
            Some(libc::ESRCH) => Ok(None),
            _ => Err(e),
        };
    }
    if n != size {
        return Err(io::Error::other(format!(
            "proc_pidinfo(PROC_PIDTBSDINFO) of {pid} wrote {n} of {size} bytes"
        )));
    }
    // SAFETY: zeroed above and written whole by the kernel; every field is an integer or an array of them,
    // so any bytes are valid.
    let info = unsafe { info.assume_init() };
    Ok(Some(match info.pbi_status {
        libc::SSTOP => Run::Stopped,
        libc::SZOMB => Run::Ended,
        _ => Run::Live,
    }))
}

/// What a process's threads are doing, as `proc_pidinfo(PROC_PIDTASKINFO)` reads them: XNU counts each
/// thread whose state has `TH_RUN`, on a core or waiting for one (`fill_taskprocinfo`,
/// `osfmk/kern/bsd_kern.c`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Task {
    /// A thread of it runs or waits for a core; with none, each is asleep or stopped.
    pub runnable: bool,
}

/// Process `pid`'s threads, or `None` when no process has that pid (`ESRCH`).
pub fn task(pid: i32) -> io::Result<Option<Task>> {
    let mut info = MaybeUninit::<libc::proc_taskinfo>::zeroed();
    let size = c_int::try_from(std::mem::size_of::<libc::proc_taskinfo>())
        .map_err(|_| io::Error::other("a proc_taskinfo past an int's bytes"))?;
    // SAFETY: for PROC_PIDTASKINFO proc_pidinfo writes at most `size` bytes, one proc_taskinfo, into the
    // buffer it is given, which is one proc_taskinfo, alive for the call.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTASKINFO,
            0,
            info.as_mut_ptr().cast::<c_void>(),
            size,
        )
    };
    if n <= 0 {
        let e = io::Error::last_os_error();
        return match e.raw_os_error() {
            Some(libc::ESRCH) => Ok(None),
            _ => Err(e),
        };
    }
    if n != size {
        return Err(io::Error::other(format!(
            "proc_pidinfo(PROC_PIDTASKINFO) of {pid} wrote {n} of {size} bytes"
        )));
    }
    // SAFETY: zeroed above and written whole by the kernel; every field is an integer, so any bytes are valid.
    let info = unsafe { info.assume_init() };
    Ok(Some(Task {
        runnable: info.pti_numrunning > 0,
    }))
}

/// `PROC_PGRP_ONLY` (`sys/proc_info.h:52`): `proc_listpids` lists one process group.
const PROC_PGRP_ONLY: u32 = 2;
/// `PROC_UID_ONLY` (`sys/proc_info.h:54`): `proc_listpids` lists one user's processes. A box's user,
/// the only marker a member inherits and can't drop (native-platforms.md §3), so this finds every member
/// — including one that daemonized (`setsid`) out of its process group (the box helper's listing, step 2).
const PROC_UID_ONLY: u32 = 4;

/// The members of process group `pgid`, written over `pids` up to its capacity, its zombies included
/// (XNU walks `zombproc` after `allproc`, `bsd/kern/proc_info.c:470-473`). Returns whether the listing may
/// have been cut short: it filled `pids`.
pub fn group(pgid: i32, pids: &mut Vec<i32>) -> io::Result<bool> {
    let group = u32::try_from(pgid)
        .map_err(|_| io::Error::other(format!("{pgid} isn't a process group")))?;
    list_pids(PROC_PGRP_ONLY, group, pids)
}

/// The processes of user `uid`, written over `pids` up to its capacity (the box helper's membership
/// listing, step 2); returns whether the listing may have been cut short.
pub fn user_pids(uid: u32, pids: &mut Vec<i32>) -> io::Result<bool> {
    list_pids(PROC_UID_ONLY, uid, pids)
}

/// `proc_listpids` for one `flavor` and its `arg`, written over `pids` up to its capacity.
fn list_pids(flavor: u32, arg: u32, pids: &mut Vec<i32>) -> io::Result<bool> {
    pids.clear();
    let room = pids.capacity();
    let bytes = room
        .checked_mul(size_of::<i32>())
        .and_then(|b| c_int::try_from(b).ok())
        .ok_or_else(|| {
            io::Error::other("a pid listing's buffer is past what proc_listpids takes")
        })?;
    // SAFETY: proc_listpids writes at most `bytes` bytes of pids into the buffer it is given, which has
    // room for `room` i32s: exactly `bytes`.
    let n = unsafe { libc::proc_listpids(flavor, arg, pids.as_mut_ptr().cast::<c_void>(), bytes) };
    let written = usize::try_from(n).map_err(|_| io::Error::last_os_error())?;
    let count = written.checked_div(size_of::<i32>()).unwrap_or(0).min(room);
    // SAFETY: the kernel wrote the first `count` pids, and `count` is at most the capacity.
    unsafe { pids.set_len(count) };
    Ok(count == room)
}

/// This process's CPU so far, user and system over all its threads (`getrusage(RUSAGE_SELF)`): the macOS
/// node's own cost, for the class bench to charge a box's watcher (bench/native/macos) and for
/// observability (CLAUDE.md §10).
pub fn process_cpu() -> io::Result<Duration> {
    let mut ru = MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: getrusage writes one `rusage` into the buffer given, which is zeroed and outlives the call.
    let r = unsafe { libc::getrusage(libc::RUSAGE_SELF, ru.as_mut_ptr()) };
    if r != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: zeroed and written by the kernel on success; every field is an integer, so any bytes are valid.
    let ru = unsafe { ru.assume_init() };
    let secs = |tv: libc::timeval| {
        let s = u64::try_from(tv.tv_sec).unwrap_or(0);
        let us = u64::try_from(tv.tv_usec).unwrap_or(0);
        Duration::from_secs(s).saturating_add(Duration::from_micros(us))
    };
    Ok(secs(ru.ru_utime).saturating_add(secs(ru.ru_stime)))
}

/// The system's 1-minute load average (`getloadavg(3)`), for a bench to record the host's load with its
/// figures (CLAUDE.md §7) and for observability (§10); `None` if the kernel won't say.
pub fn loadavg() -> Option<f64> {
    let mut avg = [0f64; 3];
    // SAFETY: getloadavg writes at most `avg.len()` doubles into the buffer given, which has 3.
    let n = unsafe { libc::getloadavg(avg.as_mut_ptr(), 3) };
    if n >= 1 { avg.first().copied() } else { None }
}

/// The most processes this kernel holds at once (`kern.maxproc`): no box can list more members.
pub fn max_processes() -> io::Result<usize> {
    sysctl_count(c"kern.maxproc")
}

/// An empty process listing with room for every process the kernel holds (`kern.maxproc`), so one
/// listing call fills it; refused naming `kern.maxproc` when it can't be read, never sized by a guess.
pub fn listing() -> io::Result<Vec<i32>> {
    listing_from(max_processes())
}

/// A listing sized from a read of `kern.maxproc` ([`listing`] with the read given, so its refusal is
/// testable).
pub fn listing_from(max: io::Result<usize>) -> io::Result<Vec<i32>> {
    let max = max.map_err(|e| io::Error::new(e.kind(), format!("reading kern.maxproc: {e}")))?;
    Ok(Vec::with_capacity(max))
}

/// The most processes one uid may hold (`kern.maxprocperuid`, `bsd/kern/kern_resource.c:169-170`): a
/// box-user box's process cap when its spec names none, each box having a uid of its own.
pub fn max_processes_per_uid() -> io::Result<usize> {
    sysctl_count(c"kern.maxprocperuid")
}

/// The most descriptors one process may open (`kern.maxfilesperproc`): what caps its `RLIMIT_NOFILE`.
pub fn max_files_per_process() -> io::Result<usize> {
    sysctl_count(c"kern.maxfilesperproc")
}

/// The most threads one process may run (`kern.num_taskthreads`).
pub fn max_threads_per_task() -> io::Result<usize> {
    sysctl_count(c"kern.num_taskthreads")
}

/// A kernel count read by name (an `int` sysctl), as read now: an administrator may change it.
fn sysctl_count(name: &std::ffi::CStr) -> io::Result<usize> {
    let mut v: c_int = 0;
    let mut len = size_of::<c_int>();
    // SAFETY: sysctlbyname reads the NUL-terminated name and writes at most `len` bytes, one c_int, into `v`,
    // and the new length into `len`; all three outlive the call, and nothing is set (newp null).
    let r = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&raw mut v).cast::<c_void>(),
            &raw mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if r != 0 {
        return Err(io::Error::last_os_error());
    }
    usize::try_from(v)
        .map_err(|_| io::Error::other(format!("{} reads {v}", name.to_string_lossy())))
}

/// A numeric sysctl (`hw.memsize`, `hw.physicalcpu`, `kern.hv_support`, `vm.page_free_count`), whether
/// the kernel keeps it in 4 bytes or 8: the node's facts (`hex-node`'s `facts`).
pub fn sysctl_number(name: &std::ffi::CStr) -> io::Result<u64> {
    let mut v = [0u8; 8];
    let mut len = v.len();
    // SAFETY: sysctlbyname reads the NUL-terminated name and writes at most `len` bytes (8) into `v`,
    // and the length it wrote into `len`; all three outlive the call, and nothing is set (newp null).
    let r = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            v.as_mut_ptr().cast::<c_void>(),
            &raw mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if r != 0 {
        return Err(io::Error::last_os_error());
    }
    match (len, v) {
        (4, [a, b, c, d, ..]) => Ok(u64::from(u32::from_ne_bytes([a, b, c, d]))),
        (8, v) => Ok(u64::from_ne_bytes(v)),
        _ => Err(io::Error::other(format!(
            "{} is {len} bytes, not a number",
            name.to_string_lossy()
        ))),
    }
}

/// The most bytes execve takes of a program's arguments and environment, their pointers included
/// (`kern.argmax`, XNU's `NCARGS`: bsd/kern/kern_exec.c:578, :6546-6549; docs/research/runtime/exec-size-bounds.md).
pub fn arg_max() -> io::Result<usize> {
    sysctl_count(c"kern.argmax")
}

/// The QoS a box's processes run at from their first instruction, as its declared CPU weight asks (a box's
/// CPU share follows its `cpu.weight` on Linux): passed to every process they start (XNU's task policy
/// copies the clamp to a forked child, `osfmk/kern/task_policy.c:746`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clamp {
    /// No clamp: the default weight, the QoS the node's own programs run at.
    Unclamped,
    /// `QOS_CLASS_BACKGROUND` for a weight under the default, which also puts the box in darwin
    /// background (lowest CPU priority, I/O throttled; `task_policy.c:946-950`).
    Background,
}

/// `QOS_CLASS_BACKGROUND` (`sys/qos.h`, macOS 26 SDK).
const QOS_CLASS_BACKGROUND: u32 = 0x09;

// SAFETY: declared as `pthread/spawn.h` (macOS 26 SDK, line 64) declares it: it sets the spawn attribute's
// QoS clamp and returns 0 or an errno.
unsafe extern "C" {
    fn posix_spawnattr_set_qos_class_np(attr: *mut libc::posix_spawnattr_t, qos: u32) -> c_int;
}

// SAFETY: declared as `spawn_private.h` declares it ("All memory limits are in MiB."): it writes a
// per-process jetsam memory limit into the spawn attribute and returns 0 or an errno. The symbol is
// exported from libsystem_kernel.dylib (`_posix_spawnattr_setjetsam_ext`); the macOS 26 CommandLineTools
// SDK ships neither its prototype nor the flags, so Hex declares them (the `_np` form is absent). Rust
// can express neither the foreign call nor the raw attribute pointer safely.
unsafe extern "C" {
    fn posix_spawnattr_setjetsam_ext(
        attr: *mut libc::posix_spawnattr_t,
        flags: i16,
        priority: c_int,
        memlimit_active: c_int,
        memlimit_inactive: c_int,
    ) -> c_int;
}

/// Jetsam spawn-attribute flags (xnu `bsd/sys/spawn_internal.h`, `psa_jetsam_flags`): SET marks the
/// attribute as carrying jetsam settings; ACTIVE_FATAL and INACTIVE_FATAL make the per-process footprint
/// limit fatal whether the process is active or inactive, so the kernel SIGKILLs it the instant it
/// crosses the limit — 0 overshoot, rootless (bench/native/macos/jetsam_probe.c, record
/// jetsam-probe-20261101T2052Z.txt).
const POSIX_SPAWN_JETSAM_SET: u16 = 0x8000;
const POSIX_SPAWN_JETSAM_MEMLIMIT_ACTIVE_FATAL: u16 = 0x04;
const POSIX_SPAWN_JETSAM_MEMLIMIT_INACTIVE_FATAL: u16 = 0x08;

/// A middling jetsam band for a box member. A *fatal* per-process limit kills the process when it crosses
/// the limit regardless of band; the band only orders kills under genuine system memory pressure. 40 is
/// the band the fatal kill was confirmed with (bench/native/macos/jetsam_probe.c; step 2's
/// boxuser_probe.c record boxuser-rootless-e532df3).
const JETSAM_PRIORITY: c_int = 40;

/// Arms `attr` with a fatal per-process memory cap of `mib` MiB, active and inactive, so the kernel
/// SIGKILLs the spawned program if its physical footprint crosses the cap (native plan step 1's `_ext`
/// backstop). `attr` is an initialized `posix_spawnattr_t`; `mib` is the box's whole bound in MiB.
fn set_jetsam_cap(attr: *mut libc::posix_spawnattr_t, mib: c_int) -> io::Result<()> {
    let flags = POSIX_SPAWN_JETSAM_SET
        | POSIX_SPAWN_JETSAM_MEMLIMIT_ACTIVE_FATAL
        | POSIX_SPAWN_JETSAM_MEMLIMIT_INACTIVE_FATAL;
    // 0x800c has the sign bit set, past a positive i16, so it is built from its bytes, not a narrowing cast.
    let flags = i16::from_ne_bytes(flags.to_ne_bytes());
    // SAFETY: posix_spawnattr_setjetsam_ext reads the flags, priority and two memory limits given and
    // writes them into the initialized attribute object `attr` points at, which outlives the call. Both
    // limits are the same `mib`, so the cap is fatal whether the process is active or inactive.
    errno(unsafe { posix_spawnattr_setjetsam_ext(attr, flags, JETSAM_PRIORITY, mib, mib) })
}

/// A mebibyte, the jetsam cap's unit.
const MIB: u64 = 1 << 20;

/// The per-process jetsam cap for a box bounded to `memory_max` bytes, in MiB: rounded DOWN, so the cap
/// in bytes never exceeds the bound, and floored to 1 MiB — the SPI's unit is MiB, `Bounds::check`
/// already refuses a bound under 1 MiB, and a 0 MiB cap would mean no limit. `None` when the MiB value is
/// past what the SPI's `int` takes (~2 PiB), where the member is left uncapped and the watcher's own rule
/// bounds the box (never a silent miscap, CLAUDE.md §3).
pub fn jetsam_cap_mib(memory_max: u64) -> Option<c_int> {
    let mib = memory_max.checked_div(MIB).unwrap_or(0).max(1);
    c_int::try_from(mib).ok()
}

/// The cap in bytes the kernel enforces for a box bounded to `memory_max`: [`jetsam_cap_mib`] times a
/// mebibyte. The watcher reads a dead member's peak footprint against this to tell a fatal-cap kill from
/// an ordinary exit (native plan step 1's `_ext` backstop).
pub fn jetsam_cap_bytes(memory_max: u64) -> Option<u64> {
    let mib = jetsam_cap_mib(memory_max)?;
    u64::try_from(mib).ok()?.checked_mul(MIB)
}

/// A program to spawn into a box: every string and descriptor its spawn uses.
pub struct Spawn {
    /// The paths to execute, in order: the program as named when it holds a `/`, else its name in each
    /// `PATH` folder, as execvp(3) tries them.
    pub paths: Vec<CString>,
    pub argv: Vec<CString>,
    pub envp: Vec<CString>,
    pub cwd: Option<CString>,
    /// stdin, stdout and stderr: each is dup2'd onto 0, 1 and 2, and nothing else is inherited.
    pub stdio: [OwnedFd; 3],
    /// The box's whole memory bound in MiB, when it has one: each spawned member is armed with a fatal
    /// per-process cap at it (native plan step 1's `_ext` backstop), so the kernel bounds a single-member
    /// box with no overshoot. `None` leaves the member uncapped (the watcher's rule bounds the box).
    pub jetsam_mib: Option<c_int>,
}

/// An errno as posix_spawn returns it.
fn errno(r: c_int) -> io::Result<()> {
    if r == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(r))
    }
}

/// A `posix_spawnattr_t`, destroyed when dropped.
struct Attr(libc::posix_spawnattr_t);

impl Drop for Attr {
    fn drop(&mut self) {
        // SAFETY: initialized by posix_spawnattr_init and destroyed once, here.
        unsafe {
            libc::posix_spawnattr_destroy(&raw mut self.0);
        }
    }
}

/// A `posix_spawn_file_actions_t`, destroyed when dropped.
struct Actions(libc::posix_spawn_file_actions_t);

impl Drop for Actions {
    fn drop(&mut self) {
        // SAFETY: initialized by posix_spawn_file_actions_init and destroyed once, here.
        unsafe {
            libc::posix_spawn_file_actions_destroy(&raw mut self.0);
        }
    }
}

/// Spawns `s` into process group `pgroup` (0: a group of its own, led by the program) under `clamp`, its
/// signal mask emptied, every signal's disposition at its default, and no descriptor inherited but its
/// stdio (`POSIX_SPAWN_CLOEXEC_DEFAULT`; XNU marks a dup2's target inherited, `bsd/kern/kern_exec.c:3193-3199`).
/// posix_spawn makes the child whole in the kernel, so it is safe from a multi-threaded process. Returns its
/// pid; a path that isn't there, or isn't executable, is skipped for the next, as execvp(3) does.
pub fn spawn(s: &Spawn, pgroup: i32, clamp: Clamp) -> io::Result<i32> {
    let mut attr = Attr(std::ptr::null_mut());
    // SAFETY: posix_spawnattr_init initializes the attribute object it is given; `attr` destroys it once.
    errno(unsafe { libc::posix_spawnattr_init(&raw mut attr.0) })?;
    let flags = libc::POSIX_SPAWN_SETPGROUP
        | libc::POSIX_SPAWN_SETSIGMASK
        | libc::POSIX_SPAWN_SETSIGDEF
        | libc::POSIX_SPAWN_CLOEXEC_DEFAULT;
    let flags =
        i16::try_from(flags).map_err(|_| io::Error::other("posix_spawn flags past a short"))?;
    let mut none: libc::sigset_t = 0;
    let mut all: libc::sigset_t = 0;
    // SAFETY: each call takes the attribute object initialized above and reads or writes only the signal
    // sets given, which outlive the calls.
    unsafe {
        errno(libc::posix_spawnattr_setflags(&raw mut attr.0, flags))?;
        errno(libc::posix_spawnattr_setpgroup(&raw mut attr.0, pgroup))?;
        errno(libc::sigemptyset(&raw mut none))?;
        errno(libc::sigfillset(&raw mut all))?;
        errno(libc::posix_spawnattr_setsigmask(
            &raw mut attr.0,
            &raw const none,
        ))?;
        errno(libc::posix_spawnattr_setsigdefault(
            &raw mut attr.0,
            &raw const all,
        ))?;
        if clamp == Clamp::Background {
            errno(posix_spawnattr_set_qos_class_np(
                &raw mut attr.0,
                QOS_CLASS_BACKGROUND,
            ))?;
        }
    }
    // Arm the box's whole memory bound as a fatal per-process cap, so the kernel bounds this member with
    // no overshoot (native plan step 1's `_ext` backstop). The watcher tracks it as armed.
    if let Some(mib) = s.jetsam_mib {
        set_jetsam_cap(&raw mut attr.0, mib)?;
    }
    let mut actions = Actions(std::ptr::null_mut());
    // SAFETY: initializes the file-actions object it is given; `actions` destroys it once.
    errno(unsafe { libc::posix_spawn_file_actions_init(&raw mut actions.0) })?;
    for (fd, target) in s.stdio.iter().zip(0..) {
        // SAFETY: records a dup2 of a descriptor open for the whole spawn (`s` owns it) onto `target`.
        errno(unsafe {
            libc::posix_spawn_file_actions_adddup2(
                &raw mut actions.0,
                fd.as_fd().as_raw_fd(),
                target,
            )
        })?;
    }
    if let Some(cwd) = &s.cwd {
        // SAFETY: records a chdir to a NUL-terminated path the action list copies. The `_np` form is the one
        // every macOS from 10.15 has; macOS 26 adds the unsuffixed name.
        errno(unsafe {
            libc::posix_spawn_file_actions_addchdir_np(&raw mut actions.0, cwd.as_ptr())
        })?;
    }
    let argv = nul_terminated(&s.argv);
    let envp = nul_terminated(&s.envp);
    let mut refused = None;
    for path in &s.paths {
        let mut pid: libc::pid_t = 0;
        // SAFETY: path, argv and envp are NUL-terminated strings, the lists NULL-terminated, all alive for the
        // call; the attribute and action objects were initialized above. posix_spawn writes the pid.
        let r = unsafe {
            libc::posix_spawn(
                &raw mut pid,
                path.as_ptr(),
                &raw const actions.0,
                &raw const attr.0,
                argv.as_ptr(),
                envp.as_ptr(),
            )
        };
        match r {
            0 => return Ok(pid),
            libc::ENOENT | libc::ENOTDIR => {}
            libc::EACCES => refused = Some(r),
            e => return Err(io::Error::from_raw_os_error(e)),
        }
    }
    Err(io::Error::from_raw_os_error(
        refused.unwrap_or(libc::ENOENT),
    ))
}

/// `strings` as the NULL-terminated list of pointers exec takes; it borrows them.
fn nul_terminated(strings: &[CString]) -> Vec<*mut c_char> {
    strings
        .iter()
        .map(|s| s.as_ptr().cast_mut())
        .chain(std::iter::once(std::ptr::null_mut()))
        .collect()
}

/// What woke a [`Kqueue`] wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Its timer fired.
    Timer,
    /// Another thread triggered it.
    User,
    /// Process `pid`, watched with tag `tag`, forked; `exited` when it has also ended since (the two
    /// coalesce into one event until it is taken).
    Fork { pid: i32, tag: u64, exited: bool },
    /// Process `pid`, watched with tag `tag`, ended with wait `status` (`NOTE_EXITSTATUS`). The node is
    /// the member's parent, so the kernel delivers the status in the event: the watcher reads a member's
    /// death — a jetsam fatal-cap `SIGKILL` among them — without racing the member's own reaper.
    Exit { pid: i32, tag: u64, status: i32 },
    /// A descriptor watched with tag `tag` ([`Kqueue::fd`]) has bytes to read, or its peer has gone
    /// (`ended`, `EV_EOF`): the node socket's connections and listener (`hex_node::serve::macos`).
    Readable { tag: u64, ended: bool },
    /// A descriptor watched with tag `tag` has room to write, or its peer has gone (`ended`).
    Writable { tag: u64, ended: bool },
}

/// `NOTE_EXITSTATUS` (`sys/event.h`, macOS 26 SDK): a watched child's exit event carries its wait status
/// in the event's `data`. Valid for a child process (the node spawned the box's members).
const NOTE_EXITSTATUS: u32 = 0x0400_0000;

/// The [`Kqueue`] timer's and user event's idents.
const TIMER: usize = 1;
const USER: usize = 2;

/// Most events one [`Kqueue::wait`] takes: the most one wait met in the bench, 28 (one a member of its
/// largest box, all killed at once; bench/native/macos/records/watcher-readcost-events-a7d13c6-20261106T165609Z.txt),
/// and the [`TIMER`] and [`USER`] events, which can be pending beside them, so one call takes such a box's
/// events (the lead's call 2, 2026-11-06). A watched process's forks and end coalesce into one event until
/// it is taken (XNU sets `EV_CLEAR` on them, `bsd/kern/kern_event.c:1139`), so a larger box's further events
/// wait for the next call, none lost.
pub const EVENTS: usize = 28 + 2;

/// A kqueue (kqueue(2)): timers, a user event other threads trigger, and processes' forks and ends.
#[derive(Debug)]
pub struct Kqueue(OwnedFd);

impl Kqueue {
    /// A kqueue with its user event added (`EVFILT_USER`, cleared as it is read).
    pub fn new() -> io::Result<Self> {
        let kq = Self(rustix::event::kqueue::kqueue().map_err(io::Error::from)?);
        kq.change(&[kevent(
            USER,
            libc::EVFILT_USER,
            libc::EV_ADD | libc::EV_CLEAR,
            0,
            0,
            0,
        )])?;
        Ok(kq)
    }

    /// Wakes a thread waiting on it.
    pub fn trigger(&self) -> io::Result<()> {
        self.change(&[kevent(USER, libc::EVFILT_USER, 0, libc::NOTE_TRIGGER, 0, 0)])
    }

    /// Watches each of `pids` for its forks and its end with the end's wait status (`EVFILT_PROC` with
    /// `NOTE_FORK | NOTE_EXIT | NOTE_EXITSTATUS`, `bsd/kern/kern_event.c:1099-1155`), tagged `tag`. XNU
    /// attaches `NOTE_EXITSTATUS` only for the process's parent (`EACCES` otherwise, :1116-1133), so each
    /// of `pids` must be this process's child (the box's members); [`Kqueue::watch_end`] watches another's.
    /// Returns those that have ended already (`ESRCH`), which can't be watched.
    pub fn watch(&self, pids: &[i32], tag: u64) -> io::Result<Vec<i32>> {
        self.watch_with(
            pids,
            tag,
            libc::NOTE_FORK | libc::NOTE_EXIT | NOTE_EXITSTATUS,
        )
    }

    /// Watches process `pid`, which needn't be this process's child (a box-user program is the helper's),
    /// for its end alone (`NOTE_EXIT`, which XNU attaches to any process), tagged 0. True when it has ended
    /// already (`ESRCH`), so no event will come.
    pub fn watch_end(&self, pid: i32) -> io::Result<bool> {
        Ok(self.watch_with(&[pid], 0, libc::NOTE_EXIT)?.contains(&pid))
    }

    fn watch_with(&self, pids: &[i32], tag: u64, fflags: u32) -> io::Result<Vec<i32>> {
        let mut gone = Vec::new();
        // One registration call per EVENTS processes: the same batch the waits take.
        for chunk in pids.chunks(EVENTS) {
            let changes: Vec<libc::kevent> = chunk
                .iter()
                .filter_map(|pid| usize::try_from(*pid).ok())
                .map(|pid| {
                    kevent(
                        pid,
                        libc::EVFILT_PROC,
                        libc::EV_ADD | libc::EV_RECEIPT,
                        fflags,
                        0,
                        tag,
                    )
                })
                .collect();
            let mut receipts = vec![kevent(0, 0, 0, 0, 0, 0); changes.len()];
            let n = self.kevent(&changes, &mut receipts, Some(Duration::ZERO))?;
            for r in receipts.iter().take(n) {
                match c_int::try_from(r.data).unwrap_or(c_int::MAX) {
                    0 => {}
                    libc::ESRCH => {
                        if let Ok(pid) = i32::try_from(r.ident) {
                            gone.push(pid);
                        }
                    }
                    e => return Err(io::Error::from_raw_os_error(e)),
                }
            }
        }
        Ok(gone)
    }

    /// Watches descriptor `fd` under `tag` for reads (`read`) and room to write (`write`), level-triggered
    /// (`EVFILT_READ` and `EVFILT_WRITE` without `EV_CLEAR`: reported while true, kevent(2)), each filter
    /// added and then enabled or disabled, so changing what `fd` is watched for is this call again. The
    /// caller keeps `fd` open while it is watched; closing it takes it out (kqueue(2)).
    pub fn fd(&self, fd: impl AsFd, tag: u64, read: bool, write: bool) -> io::Result<()> {
        let ident = usize::try_from(fd.as_fd().as_raw_fd())
            .map_err(|_| io::Error::other("a descriptor isn't one"))?;
        let on = |on: bool| {
            libc::EV_ADD
                | if on {
                    libc::EV_ENABLE
                } else {
                    libc::EV_DISABLE
                }
        };
        self.change(&[
            kevent(ident, libc::EVFILT_READ, on(read), 0, 0, tag),
            kevent(ident, libc::EVFILT_WRITE, on(write), 0, 0, tag),
        ])
    }

    /// Stops watching descriptor `fd`, if it was watched.
    pub fn forget_fd(&self, fd: impl AsFd) -> io::Result<()> {
        let Ok(ident) = usize::try_from(fd.as_fd().as_raw_fd()) else {
            return Ok(());
        };
        for filter in [libc::EVFILT_READ, libc::EVFILT_WRITE] {
            match self.change(&[kevent(ident, filter, libc::EV_DELETE, 0, 0, 0)]) {
                Err(e) if e.raw_os_error() == Some(libc::ENOENT) => {}
                other => other?,
            }
        }
        Ok(())
    }

    /// Stops watching process `pid`, if it was watched.
    pub fn unwatch(&self, pid: i32) -> io::Result<()> {
        let Ok(ident) = usize::try_from(pid) else {
            return Ok(());
        };
        match self.change(&[kevent(ident, libc::EVFILT_PROC, libc::EV_DELETE, 0, 0, 0)]) {
            Err(e) if e.raw_os_error() == Some(libc::ENOENT) => Ok(()),
            other => other,
        }
    }

    /// Waits for events, after arming a one-shot timer for `timer` marked `NOTE_CRITICAL` ("system does
    /// minimal timer coalescing", `sys/event.h:308`; a plain sleep ran about six times long,
    /// bench/native/macos/RESULT.md) when one is given, and for at most `timeout` when one is given. Takes
    /// at most [`EVENTS`] events into `out`, which the caller keeps so a wait allocates nothing; more wait
    /// for the next call. A process watched with [`Kqueue::watch`] that forked is reported as a fork even
    /// if it has ended since, so its new member isn't missed.
    pub fn wait(
        &self,
        timer: Option<Duration>,
        timeout: Option<Duration>,
        out: &mut Vec<Event>,
    ) -> io::Result<()> {
        out.clear();
        let armed;
        let changes: &[libc::kevent] = match timer {
            Some(t) => {
                // At least a microsecond, so a timer is never armed for 0, which fires at once.
                let us = t.as_micros().clamp(1, u128::from(u32::MAX));
                armed = [kevent(
                    TIMER,
                    libc::EVFILT_TIMER,
                    libc::EV_ADD | libc::EV_ONESHOT,
                    libc::NOTE_USECONDS | libc::NOTE_CRITICAL,
                    isize::try_from(us).unwrap_or(isize::MAX),
                    0,
                )];
                &armed
            }
            None => &[],
        };
        let mut events = [kevent(0, 0, 0, 0, 0, 0); EVENTS];
        let n = match self.kevent(changes, &mut events, timeout) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => 0,
            other => other?,
        };
        for e in events.iter().take(n) {
            if e.flags & libc::EV_ERROR != 0 {
                return Err(io::Error::from_raw_os_error(
                    c_int::try_from(e.data).unwrap_or(libc::EINVAL),
                ));
            }
            let tag = u64::try_from(e.udata.addr()).unwrap_or(u64::MAX);
            let pid = i32::try_from(e.ident).unwrap_or(0);
            let event = match e.filter {
                libc::EVFILT_TIMER => Event::Timer,
                libc::EVFILT_USER => Event::User,
                libc::EVFILT_PROC if e.fflags & libc::NOTE_FORK != 0 => Event::Fork {
                    pid,
                    tag,
                    exited: e.fflags & libc::NOTE_EXIT != 0,
                },
                // The wait status rides in `data` (NOTE_EXITSTATUS); the terminating signal is its low 7
                // bits, so a jetsam fatal-cap SIGKILL is told apart here without reading the zombie.
                libc::EVFILT_PROC => Event::Exit {
                    pid,
                    tag,
                    status: i32::try_from(e.data).unwrap_or(0),
                },
                libc::EVFILT_READ => Event::Readable {
                    tag,
                    ended: e.flags & libc::EV_EOF != 0,
                },
                libc::EVFILT_WRITE => Event::Writable {
                    tag,
                    ended: e.flags & libc::EV_EOF != 0,
                },
                _ => continue,
            };
            out.push(event);
        }
        Ok(())
    }

    fn change(&self, changes: &[libc::kevent]) -> io::Result<()> {
        self.kevent(changes, &mut [], Some(Duration::ZERO))
            .map(|_| ())
    }

    fn kevent(
        &self,
        changes: &[libc::kevent],
        events: &mut [libc::kevent],
        timeout: Option<Duration>,
    ) -> io::Result<usize> {
        // A timeout no deadline can hold (the clock can't add it to now: a run's wait for its program's end)
        // bounds nothing, so it waits for the event alone, rather than hand the kernel a time it refuses
        // (EINVAL for a timespec past its range).
        let ts = timeout
            .filter(|t| std::time::Instant::now().checked_add(*t).is_some())
            .map(|t| libc::timespec {
                tv_sec: libc::time_t::try_from(t.as_secs()).unwrap_or(libc::time_t::MAX),
                tv_nsec: libc::c_long::from(t.subsec_nanos()),
            });
        let nchanges = c_int::try_from(changes.len())
            .map_err(|_| io::Error::other("too many kevent changes"))?;
        let nevents = c_int::try_from(events.len())
            .map_err(|_| io::Error::other("too many kevent events"))?;
        // SAFETY: kevent reads `nchanges` events from `changes` and writes at most `nevents` into `events`,
        // both slices alive for the call, and reads the timeout if one is given. No event carries a pointer:
        // a tag is a number in `udata`, never dereferenced.
        let n = unsafe {
            libc::kevent(
                self.0.as_raw_fd(),
                changes.as_ptr(),
                nchanges,
                events.as_mut_ptr(),
                nevents,
                ts.as_ref().map_or(std::ptr::null(), std::ptr::from_ref),
            )
        };
        usize::try_from(n).map_err(|_| io::Error::last_os_error())
    }
}

/// The effective user id of the process at the other end of local socket `fd` (getpeereid(3)): who
/// connected to the node's socket, which admits this user and root (`hex_node::serve::macos`).
pub fn peer_euid(fd: impl AsFd) -> io::Result<u32> {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    // SAFETY: getpeereid reads the descriptor and writes one uid_t and one gid_t through the two pointers,
    // each to a local alive for the call.
    let r = unsafe { libc::getpeereid(fd.as_fd().as_raw_fd(), &raw mut uid, &raw mut gid) };
    if r != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(uid)
}

/// An `EV_SET` (`sys/event.h`), its tag carried in `udata` as a number.
fn kevent(
    ident: usize,
    filter: i16,
    flags: u16,
    fflags: u32,
    data: isize,
    tag: u64,
) -> libc::kevent {
    libc::kevent {
        ident,
        filter,
        flags,
        fflags,
        data,
        udata: std::ptr::without_provenance_mut(usize::try_from(tag).unwrap_or(usize::MAX)),
    }
}
