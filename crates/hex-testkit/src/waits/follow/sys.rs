//! A thread as a party, as each OS shows another thread of the process: Linux in procfs, macOS through
//! `proc_pidinfo`, which needs its one FFI call here.

#![cfg_attr(target_os = "macos", allow(unsafe_code))]

use hex_core::patience::Party;

/// A thread of this process, as its OS names it: Linux's tid, macOS's 64-bit thread id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Id(u64);

/// The calling thread's id: `/proc/thread-self` names `<pid>/task/<tid>`.
#[cfg(target_os = "linux")]
pub fn current() -> std::io::Result<Id> {
    let link = std::fs::read_link("/proc/thread-self")?;
    link.file_name()
        .and_then(|tid| tid.to_str())
        .and_then(|tid| tid.parse().ok())
        .map(Id)
        .ok_or_else(|| {
            std::io::Error::other(format!(
                "/proc/thread-self names no thread: {}",
                link.display()
            ))
        })
}

/// Thread `id` as a party, from `/proc/self/task/<tid>/stat`: gone or a zombie, ended; running or runnable (`R`),
/// live; otherwise alive, its CPU time in nanoseconds from `schedstat` where the kernel keeps it, else in clock ticks
/// from `stat`'s utime and stime (fields 14 and 15), one source for the thread's whole life. One the OS shows no state
/// of is followed by its life alone.
#[cfg(target_os = "linux")]
pub fn party(id: Id) -> Party {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/self/task/{}/stat", id.0)) else {
        return Party::Ended;
    };
    // The command name is in parentheses and may hold anything: the fields after it are read past its last ')'.
    let Some((_, fields)) = stat.rsplit_once(')') else {
        return Party::Live;
    };
    let fields: Vec<&str> = fields.split_whitespace().collect();
    match fields.first().copied() {
        Some("Z" | "X" | "x") => return Party::Ended,
        Some("R") | None => return Party::Live,
        Some(_) => {}
    }
    let nanos = std::fs::read_to_string(format!("/proc/self/task/{}/schedstat", id.0))
        .ok()
        .and_then(|s| s.split_whitespace().next().and_then(|n| n.parse().ok()));
    // After the ')': state is field 3, so utime (14) and stime (15) are the 12th and 13th.
    let ticks = || {
        let utime: u64 = fields.get(11)?.parse().ok()?;
        let stime: u64 = fields.get(12)?.parse().ok()?;
        utime.checked_add(stime)
    };
    match nanos.or_else(ticks) {
        Some(progress) => Party::Alive { progress },
        None => Party::Live,
    }
}

/// The calling thread's id, as `pthread_threadid_np` gives it.
#[cfg(target_os = "macos")]
pub fn current() -> std::io::Result<Id> {
    let mut id: u64 = 0;
    // SAFETY: with no thread named (0), pthread_threadid_np writes the calling thread's id to `id`, a valid u64.
    let failed = unsafe { libc::pthread_threadid_np(0, &mut id) };
    if failed != 0 {
        return Err(std::io::Error::from_raw_os_error(failed));
    }
    Ok(Id(id))
}

/// XNU's flavor reading a thread by its 64-bit id (`sys/proc_info.h`), which libc doesn't name.
#[cfg(target_os = "macos")]
const PROC_PIDTHREADID64INFO: libc::c_int = 15;
/// A thread running or runnable (`mach/thread_info.h`).
#[cfg(target_os = "macos")]
const TH_STATE_RUNNING: i32 = 1;

/// Thread `id` as a party, from `proc_pidinfo(PROC_PIDTHREADID64INFO)`: no such thread, ended; running or runnable,
/// live; otherwise alive, its user and system time in nanoseconds. A short answer is followed by its life alone.
#[cfg(target_os = "macos")]
pub fn party(id: Id) -> Party {
    let mut info = std::mem::MaybeUninit::<libc::proc_threadinfo>::zeroed();
    let Ok(size) = libc::c_int::try_from(std::mem::size_of::<libc::proc_threadinfo>()) else {
        return Party::Live;
    };
    // SAFETY: proc_pidinfo writes at most `size` bytes, one proc_threadinfo, into `info`, which holds that many; the
    // flavor reads this process's thread whose 64-bit id is `id.0`.
    let wrote = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            PROC_PIDTHREADID64INFO,
            id.0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if wrote <= 0 {
        return Party::Ended;
    }
    if wrote != size {
        return Party::Live;
    }
    // SAFETY: proc_pidinfo wrote the whole struct (`wrote == size`), and it was zeroed before, every field plain data.
    let info = unsafe { info.assume_init() };
    if info.pth_run_state == TH_STATE_RUNNING {
        return Party::Live;
    }
    match info.pth_user_time.checked_add(info.pth_system_time) {
        Some(progress) => Party::Alive { progress },
        None => Party::Live,
    }
}
