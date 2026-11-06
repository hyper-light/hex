//! The Windows calls the `process` class makes that Rust can't make safely (CLAUDE.md §12 names this
//! file): `windows-sys` binds them only as `unsafe` foreign functions, and std's own job-object and
//! `PROC_THREAD_ATTRIBUTE_JOB_LIST`/`_HANDLE_LIST` process-creation support is unstable. Each `unsafe`
//! block states the invariant that makes it sound. A box is a job object: this file creates one, sets
//! its limits, starts a member suspended inside it (`PROC_THREAD_ATTRIBUTE_JOB_LIST`, so nothing runs
//! outside it) and resumes it, reads the job's accounting and its guaranteed memory-limit violation,
//! and ends the job. Names, structures and flags are Windows' own (`windows-sys`;
//! docs/research/runtime/windows-job-objects.md).

// The Windows class's one place `unsafe` is allowed (CLAUDE.md §12); every block below carries its
// SAFETY comment.
#![allow(unsafe_code)]

use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_MORE_DATA, ERROR_NOT_ENOUGH_QUOTA, GetLastError, HANDLE,
    HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, JOB_OBJECT_CPU_RATE_CONTROL_ENABLE, JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
    JOB_OBJECT_CPU_RATE_CONTROL_WEIGHT_BASED, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_JOB_MEMORY, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_AND_IO_ACCOUNTING_INFORMATION, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOBOBJECT_LIMIT_VIOLATION_INFORMATION,
    JOBOBJECT_NOTIFICATION_LIMIT_INFORMATION, JobObjectBasicAndIoAccountingInformation,
    JobObjectBasicProcessIdList, JobObjectCpuRateControlInformation,
    JobObjectExtendedLimitInformation, JobObjectLimitViolationInformation,
    JobObjectNotificationLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    ALL_PROCESSOR_GROUPS, CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
    GetActiveProcessorCount, GetExitCodeProcess, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_JOB_LIST, PROCESS_INFORMATION, ResumeThread, STARTF_USESTDHANDLES,
    STARTUPINFOEXW, STARTUPINFOW, WaitForSingleObject,
};

use hex_core::job::Cpu;

/// Windows' active processor count across every processor group (not one group's 64-at-most), the
/// denominator the CPU hard cap maps against ([`super::job`]).
pub fn active_processors() -> u32 {
    // SAFETY: GetActiveProcessorCount reads nothing and returns a count; ALL_PROCESSOR_GROUPS is the
    // documented argument for every group. A 0 return (no information) is treated as one processor.
    let n = unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) };
    n.max(1)
}

/// The users folder (`FOLDERID_UserProfiles`, `%SystemDrive%\\Users` unless moved), read from its known
/// folder as Windows resolves it, never a literal: the node guest's default share
/// (docs/design/shards-guest.md §4a).
pub fn user_profiles() -> io::Result<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_UserProfiles, SHGetKnownFolderPath};
    let mut path: windows_sys::core::PWSTR = null_mut();
    // SAFETY: FOLDERID_UserProfiles is a static GUID read for the call's length; flags 0 and a null
    // token ask for the folder as the calling user sees it; `path` is a valid place for the callee to
    // store a CoTaskMemAlloc'd, NUL-terminated wide string, which it does on success and may on
    // failure, so it is freed below either way.
    let hr = unsafe { SHGetKnownFolderPath(&FOLDERID_UserProfiles, 0, null_mut(), &mut path) };
    let read = if hr >= 0 && !path.is_null() {
        let mut len = 0usize;
        // SAFETY: on success `path` is NUL-terminated, so each read up to and at the NUL is within
        // the string the callee allocated; the walk stops at the NUL.
        while unsafe { *path.add(len) } != 0 {
            len = len.saturating_add(1);
        }
        // SAFETY: `path` holds `len` initialised u16s before its NUL (counted above), alive until
        // the CoTaskMemFree below.
        let wide = unsafe { std::slice::from_raw_parts(path, len) };
        Ok(std::path::PathBuf::from(std::ffi::OsString::from_wide(
            wide,
        )))
    } else {
        Err(io::Error::from_raw_os_error(hr))
    };
    // SAFETY: `path` is null or the callee's CoTaskMemAlloc'd string, freed once here and not used
    // after; CoTaskMemFree takes null.
    unsafe { CoTaskMemFree(path.cast_const().cast()) };
    read
}

/// The temp folder, as Windows resolves it for this process (`GetTempPath2W`: `TMP`, `TEMP`, the
/// user's profile, then the Windows folder), the node guest's other default share.
pub fn temp_folder() -> io::Result<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Storage::FileSystem::GetTempPath2W;
    // MAX_PATH + 1, the most GetTempPath2W writes (its documentation: "The maximum possible return
    // value is MAX_PATH+1 (261)").
    let mut buf = [0u16; 261];
    let size = u32::try_from(buf.len()).unwrap_or(0);
    // SAFETY: `buf` is a writable array of `size` u16s, alive for the call; GetTempPath2W writes at
    // most that many, NUL included, and returns how many it wrote before the NUL, 0 on failure.
    let n = unsafe { GetTempPath2W(size, buf.as_mut_ptr()) };
    let n = usize::try_from(n).unwrap_or(usize::MAX);
    match buf.get(..n) {
        Some(wide) if n > 0 => Ok(std::path::PathBuf::from(std::ffi::OsString::from_wide(
            wide,
        ))),
        _ => Err(last_error()),
    }
}

fn last_error() -> io::Error {
    io::Error::from_raw_os_error(
        // SAFETY: GetLastError reads the calling thread's own last-error value and returns it.
        i32::try_from(unsafe { GetLastError() }).unwrap_or(-1),
    )
}

/// Whether `err` is the refusal Windows returns when a `CreateProcess` would exceed a job's
/// active-process limit (`ERROR_NOT_ENOUGH_QUOTA`): the box is full.
pub fn is_process_limit(err: &io::Error) -> bool {
    err.raw_os_error() == i32::try_from(ERROR_NOT_ENOUGH_QUOTA).ok()
}

/// An owned Windows handle, closed when dropped. `HANDLE` is a raw pointer, so this is the one owner;
/// it is only made from a handle this process owns (a job, a completion port, a process or thread this
/// process created). Sendable: a handle is a process-wide value, not tied to a thread.
#[derive(Debug)]
pub struct Handle(HANDLE);

// SAFETY: a Windows HANDLE is valid in any thread of the process; nothing thread-local is held.
unsafe impl Send for Handle {}
// SAFETY: the kernel serializes handle operations; sharing `&Handle` races only in the kernel, which is
// sound. Hex shares one only behind its own lock.
unsafe impl Sync for Handle {}

impl Handle {
    /// The raw handle, for a call that borrows it (it stays owned by `self`).
    pub fn raw(&self) -> HANDLE {
        self.0
    }

    /// The handle borrowed, for a call that takes a `BorrowedHandle`.
    pub fn borrowed(&self) -> std::os::windows::io::BorrowedHandle<'_> {
        // SAFETY: self owns a valid handle for at least the borrow's life (Handle's invariant).
        unsafe { std::os::windows::io::BorrowedHandle::borrow_raw(self.0) }
    }

    /// Takes ownership of a raw handle this process owns.
    ///
    /// # Safety
    /// `h` must be a valid handle this process owns and no other `Handle` owns.
    pub(crate) unsafe fn own(h: HANDLE) -> Handle {
        Handle(h)
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            // SAFETY: self owns a valid handle (Handle's invariant); closing it once is sound, and Drop
            // runs once.
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// A member started suspended in its box's job.
pub struct Started {
    pub process: Handle,
    pub thread: Handle,
    pub pid: u32,
}

/// What a box's job holds for its members: the fully-built command line, environment block and working
/// directory as wide, NUL-handled strings, and the three standard handles the member inherits (each the
/// NUL device where the caller gave none). Built by the shell ([`super::mod`]); consumed by [`create`].
pub struct Spawn {
    /// The command line, UTF-16, NUL-terminated.
    pub command_line: Vec<u16>,
    /// The environment block: NUL-separated `name=value`, double-NUL-terminated, sorted (Windows reads a
    /// sorted block); empty but for one trailing NUL when the member gets an empty environment.
    pub environment: Vec<u16>,
    /// The working directory, UTF-16 NUL-terminated, or `None` for the node's.
    pub cwd: Option<Vec<u16>>,
    /// stdin, stdout, stderr: the handles the member inherits as its standard streams.
    pub stdio: [OwnedHandle; 3],
}

/// What a box's job enforces, as [`create`] sets it on the job.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub memory: Option<u64>,
    pub processes: Option<u32>,
    pub cpu: Option<Cpu>,
    /// The memory the job notifies at: a guaranteed `NotificationLimit` below the hard `memory` cap, so
    /// the box learns it reached its bound through the class Windows guarantees, never a lossy message
    /// (CLAUDE.md §3). `None` when the box has no memory bound.
    pub memory_notify: Option<u64>,
}

/// A box's job: created, limited, its member started inside it, its accounting read, and ended. The
/// handle is closed on drop; with kill-on-close set, that ends every process still in the job.
pub struct Job(Handle);

impl Job {
    /// A new, empty job object.
    pub fn create() -> io::Result<Job> {
        // SAFETY: CreateJobObjectW with null attributes and null name makes an unnamed job and returns
        // its handle, or null on failure.
        let h = unsafe { CreateJobObjectW(null(), null()) };
        if h.is_null() {
            return Err(last_error());
        }
        // SAFETY: h is a job handle this process just created and nothing else owns.
        Ok(Job(unsafe { Handle::own(h) }))
    }

    /// The job's handle, borrowed.
    pub fn borrowed(&self) -> std::os::windows::io::BorrowedHandle<'_> {
        self.0.borrowed()
    }

    pub fn raw(&self) -> HANDLE {
        self.0.raw()
    }

    /// Sets the job's limits: a hard commit cap, an active-process cap, kill-on-close (so closing the
    /// last handle ends the tree), the CPU weight or hard cap, and the guaranteed memory notification
    /// threshold. Breakaway is never allowed (neither `BREAKAWAY_OK` nor `SILENT_BREAKAWAY_OK` is set),
    /// so no member can leave the job.
    pub fn configure(&self, limits: &Limits) -> io::Result<()> {
        let mut ext: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed_struct();
        let mut flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if let Some(bytes) = limits.memory {
            flags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
            ext.JobMemoryLimit = usize::try_from(bytes).unwrap_or(usize::MAX);
        }
        if let Some(n) = limits.processes {
            flags |= JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
            ext.BasicLimitInformation.ActiveProcessLimit = n;
        }
        ext.BasicLimitInformation.LimitFlags = flags;
        self.set(JobObjectExtendedLimitInformation, &ext)?;
        if let Some(cpu) = limits.cpu {
            let mut rate: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION = zeroed_struct();
            match cpu {
                Cpu::Weight(w) => {
                    rate.ControlFlags = JOB_OBJECT_CPU_RATE_CONTROL_ENABLE
                        | JOB_OBJECT_CPU_RATE_CONTROL_WEIGHT_BASED;
                    rate.Anonymous.Weight = w;
                }
                Cpu::Cap(r) => {
                    rate.ControlFlags =
                        JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP;
                    rate.Anonymous.CpuRate = r;
                }
            }
            self.set(JobObjectCpuRateControlInformation, &rate)?;
        }
        if let Some(notify) = limits.memory_notify {
            let mut n: JOBOBJECT_NOTIFICATION_LIMIT_INFORMATION = zeroed_struct();
            n.LimitFlags = JOB_OBJECT_LIMIT_JOB_MEMORY;
            n.JobMemoryLimit = notify;
            self.set(JobObjectNotificationLimitInformation, &n)?;
        }
        Ok(())
    }

    fn set<T>(&self, class: i32, value: &T) -> io::Result<()> {
        let len = u32::try_from(size_of::<T>()).unwrap_or(u32::MAX);
        // SAFETY: SetInformationJobObject reads `len` bytes from `value` (one T, alive for the call) and
        // sets them on the job self owns. `class` names the structure T is.
        if unsafe { SetInformationJobObject(self.0.raw(), class, (value as *const T).cast(), len) }
            == 0
        {
            return Err(last_error());
        }
        Ok(())
    }

    fn query<T>(&self, class: i32) -> io::Result<T> {
        let mut value: T = zeroed_struct();
        let len = u32::try_from(size_of::<T>()).unwrap_or(u32::MAX);
        // SAFETY: QueryInformationJobObject writes at most `len` bytes (one T) into `value`, which is
        // zeroed and alive for the call; `class` names the structure T is.
        if unsafe {
            QueryInformationJobObject(
                self.0.raw(),
                class,
                (&mut value as *mut T).cast(),
                len,
                null_mut(),
            )
        } == 0
        {
            return Err(last_error());
        }
        Ok(value)
    }

    /// The job's peak committed memory, CPU time (user + kernel, in 100 ns ticks) and I/O bytes. All
    /// authoritative reads, never a lossy message (CLAUDE.md §3). The live process count is
    /// [`Job::live_processes`]; the job's accounting `ActiveProcesses` is not used, as it lingers while
    /// a terminated member's handle is held.
    pub fn accounting(&self) -> io::Result<Accounting> {
        let a: JOBOBJECT_BASIC_AND_IO_ACCOUNTING_INFORMATION =
            self.query(JobObjectBasicAndIoAccountingInformation)?;
        let ext: JOBOBJECT_EXTENDED_LIMIT_INFORMATION =
            self.query(JobObjectExtendedLimitInformation)?;
        let ticks = |t: i64| u64::try_from(t).unwrap_or(0);
        Ok(Accounting {
            peak_memory: u64::try_from(ext.PeakJobMemoryUsed).unwrap_or(u64::MAX),
            cpu_100ns: ticks(a.BasicInfo.TotalUserTime)
                .saturating_add(ticks(a.BasicInfo.TotalKernelTime)),
            read_bytes: a.IoInfo.ReadTransferCount,
            write_bytes: a.IoInfo.WriteTransferCount,
        })
    }

    /// How many processes are live in the job right now (`JobObjectBasicProcessIdList`'s
    /// `NumberOfProcessIdsInList`). This counts running processes, not the job's accounting
    /// `ActiveProcesses`, which lingers while a terminated member's handle is still held — so a box
    /// whose member has exited reads as empty here even while the node still holds its process handle.
    /// The list is sized from the job itself: a first read with room for one id learns the job's
    /// `NumberOfAssignedProcesses` (filled in even when the ids don't fit, `ERROR_MORE_DATA`), the second
    /// reads that many. Ids that joined between the two reads don't fit; the box is running with at least
    /// the ids listed, which is all a caller needs (CLAUDE.md §2: the read is bounded by the job's own
    /// count, not a picked one).
    pub fn live_processes(&self) -> io::Result<u64> {
        let header = size_of::<u32>().saturating_mul(2);
        let first = self.process_ids(header.saturating_add(size_of::<usize>()))?;
        if first.listed >= first.assigned {
            return Ok(u64::from(first.listed));
        }
        let ids = usize::try_from(first.assigned).unwrap_or(usize::MAX);
        let second =
            self.process_ids(header.saturating_add(ids.saturating_mul(size_of::<usize>())))?;
        Ok(u64::from(second.listed))
    }

    /// One read of the job's process id list into a buffer of `len` bytes: its two counts, the ids
    /// themselves unread. `ERROR_MORE_DATA` (the ids don't all fit) still fills both counts.
    fn process_ids(&self, len: usize) -> io::Result<IdCounts> {
        // usize-aligned, as the list's ids are; the counts are its first 8 bytes.
        let mut buf = vec![0usize; len.div_ceil(size_of::<usize>())];
        let bytes = u32::try_from(buf.len().saturating_mul(size_of::<usize>()))
            .map_err(|_| io::Error::other("a job's process id list past 4 GiB"))?;
        // SAFETY: QueryInformationJobObject writes at most `bytes` bytes into `buf`, alive and that long
        // for the call, for the job self owns; the class names the structure, whose header is two u32s.
        let ok = unsafe {
            QueryInformationJobObject(
                self.0.raw(),
                JobObjectBasicProcessIdList,
                buf.as_mut_ptr().cast(),
                bytes,
                null_mut(),
            )
        };
        if ok == 0 {
            let err = last_error();
            if err.raw_os_error() != i32::try_from(ERROR_MORE_DATA).ok() {
                return Err(err);
            }
        }
        // The header's 8 bytes, whatever usize's width.
        let head: Vec<u8> = buf.iter().flat_map(|w| w.to_ne_bytes()).take(8).collect();
        let count = |at: usize| {
            head.get(at..at.saturating_add(4))
                .and_then(|b| <[u8; 4]>::try_from(b).ok())
                .map_or(0, u32::from_ne_bytes)
        };
        Ok(IdCounts {
            assigned: count(0),
            listed: count(4),
        })
    }

    /// Whether the job's memory notification limit has been violated since this was last read: the
    /// guaranteed accounting of the guaranteed notification class. Reading it re-arms the notification.
    pub fn memory_violated(&self) -> io::Result<bool> {
        let v: JOBOBJECT_LIMIT_VIOLATION_INFORMATION =
            self.query(JobObjectLimitViolationInformation)?;
        Ok(v.ViolationLimitFlags & JOB_OBJECT_LIMIT_JOB_MEMORY != 0)
    }

    /// Ends every process in the job with `code`, at once. Idempotent: an already-empty job is fine.
    pub fn terminate(&self, code: u32) -> io::Result<()> {
        // SAFETY: self owns a valid job handle; TerminateJobObject ends its processes and returns.
        if unsafe { TerminateJobObject(self.0.raw(), code) } == 0 {
            return Err(last_error());
        }
        Ok(())
    }
}

/// A job process id list's two counts (`JOBOBJECT_BASIC_PROCESS_ID_LIST`'s header).
struct IdCounts {
    /// `NumberOfAssignedProcesses`: the ids the list would hold.
    assigned: u32,
    /// `NumberOfProcessIdsInList`: the ids it holds.
    listed: u32,
}

/// A job's resource accounting, read whole.
#[derive(Debug, Clone, Copy, Default)]
pub struct Accounting {
    pub peak_memory: u64,
    pub cpu_100ns: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
}

/// Starts `spawn` suspended in `jobs` (its box's job, and the node's where there is one), so it is a
/// member of them from its creation and nothing runs outside them (`PROC_THREAD_ATTRIBUTE_JOB_LIST`).
/// Only its three standard handles are inherited (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`). The caller
/// resumes it with [`resume`].
pub fn create(spawn: &mut Spawn, jobs: &[HANDLE]) -> io::Result<Started> {
    // The three standard handles, made inheritable, as a contiguous array for the handle-list attribute.
    let handles: [HANDLE; 3] = [
        spawn.stdio[0].as_raw_handle() as HANDLE,
        spawn.stdio[1].as_raw_handle() as HANDLE,
        spawn.stdio[2].as_raw_handle() as HANDLE,
    ];
    for h in handles {
        // SAFETY: each handle is owned by spawn.stdio for the call; marking it inheritable is sound.
        if unsafe { SetHandleInformation(h, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) } == 0 {
            return Err(last_error());
        }
    }

    // Two attributes: the inherited-handle list and the job list.
    let mut size = 0usize;
    // SAFETY: the first call, with a null list, only writes the required byte size into `size`; it is
    // expected to fail with ERROR_INSUFFICIENT_BUFFER, which we ignore in favour of `size`.
    unsafe { InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut size) };
    if size == 0 {
        return Err(io::Error::other("attribute list size is zero"));
    }
    let mut buf = vec![0u8; size];
    let list = buf.as_mut_ptr().cast::<core::ffi::c_void>();
    // SAFETY: `list` points at `size` writable bytes (buf), the size the call above asked for, for 2
    // attributes.
    if unsafe { InitializeProcThreadAttributeList(list, 2, 0, &mut size) } == 0 {
        return Err(last_error());
    }
    let started = create_in(spawn, jobs, &handles, list);
    // SAFETY: `list` was initialized above; deleting it frees its internal allocations once, and buf
    // (its backing bytes) is still alive and dropped after.
    unsafe { DeleteProcThreadAttributeList(list) };
    started
}

/// The body of [`create`] once the attribute list is allocated, so the list is always deleted.
fn create_in(
    spawn: &mut Spawn,
    jobs: &[HANDLE],
    handles: &[HANDLE; 3],
    list: LPPROC_THREAD_ATTRIBUTE_LIST,
) -> io::Result<Started> {
    let handles_bytes = u32::try_from(size_of::<HANDLE>().saturating_mul(3)).unwrap_or(u32::MAX);
    // SAFETY: the handle-list attribute points at `handles` (3 HANDLEs), alive for this call; the size
    // matches. The list has room for 2 attributes.
    if unsafe {
        windows_sys::Win32::System::Threading::UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            handles.as_ptr().cast(),
            handles_bytes as usize,
            null_mut(),
            null(),
        )
    } == 0
    {
        return Err(last_error());
    }
    if !jobs.is_empty() {
        let jobs_bytes = size_of::<HANDLE>().saturating_mul(jobs.len());
        // SAFETY: the job-list attribute points at `jobs` (jobs.len() HANDLEs), alive for this call; the
        // size matches.
        if unsafe {
            windows_sys::Win32::System::Threading::UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                jobs.as_ptr().cast(),
                jobs_bytes,
                null_mut(),
                null(),
            )
        } == 0
        {
            return Err(last_error());
        }
    }

    let mut si: STARTUPINFOEXW = zeroed_struct();
    si.StartupInfo.cb = u32::try_from(size_of::<STARTUPINFOEXW>()).unwrap_or(0);
    si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    si.StartupInfo.hStdInput = handles[0];
    si.StartupInfo.hStdOutput = handles[1];
    si.StartupInfo.hStdError = handles[2];
    si.lpAttributeList = list;

    let cwd = spawn.cwd.as_ref().map_or(null(), |c| c.as_ptr());
    let mut pi: PROCESS_INFORMATION = zeroed_struct();
    // SAFETY: command_line is a NUL-terminated writable UTF-16 buffer (CreateProcessW may modify it);
    // environment is a double-NUL-terminated UTF-16 block; cwd is NUL-terminated or null; si and pi are
    // valid and alive for the call. bInheritHandles is TRUE because only the listed handles are
    // inherited (the handle-list attribute). The process is created suspended and in its jobs.
    let ok = unsafe {
        CreateProcessW(
            null(),
            spawn.command_line.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_SUSPENDED
                | CREATE_UNICODE_ENVIRONMENT
                | CREATE_NO_WINDOW
                | EXTENDED_STARTUPINFO_PRESENT,
            spawn.environment.as_ptr().cast(),
            cwd,
            (&si as *const STARTUPINFOEXW).cast::<STARTUPINFOW>(),
            &mut pi,
        )
    };
    if ok == 0 {
        return Err(last_error());
    }
    // SAFETY: CreateProcessW succeeded, so pi holds a process handle this process now owns.
    let process = unsafe { Handle::own(pi.hProcess) };
    // SAFETY: and a thread handle this process now owns.
    let thread = unsafe { Handle::own(pi.hThread) };
    Ok(Started {
        process,
        thread,
        pid: pi.dwProcessId,
    })
}

/// Lets a suspended member run (its one initial thread).
pub fn resume(thread: &Handle) -> io::Result<()> {
    // SAFETY: thread is a valid, owned thread handle. ResumeThread returns the previous suspend count,
    // or u32::MAX on failure.
    if unsafe { ResumeThread(thread.raw()) } == u32::MAX {
        return Err(last_error());
    }
    Ok(())
}

/// Whether `process` ended within `timeout_ms` (`u32::MAX` is forever).
pub fn wait(process: &Handle, timeout_ms: u32) -> io::Result<bool> {
    // SAFETY: process is a valid, owned process handle; WaitForSingleObject waits on it.
    match unsafe { WaitForSingleObject(process.raw(), timeout_ms) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        _ => Err(last_error()),
    }
}

/// `process`'s exit code, once it has ended (`STILL_ACTIVE`, 259, while it runs).
pub fn exit_code(process: &Handle) -> io::Result<u32> {
    let mut code = 0u32;
    // SAFETY: GetExitCodeProcess writes one u32 into `code`, alive for the call, for the owned handle.
    if unsafe { GetExitCodeProcess(process.raw(), &mut code) } == 0 {
        return Err(last_error());
    }
    Ok(code)
}

/// A zeroed `T` of a plain Windows structure (every field an integer, pointer or union of them, for
/// which all-zero is a valid, inert value). Used for the job-info and startup structures this file
/// fills before a call.
fn zeroed_struct<T>() -> T {
    // SAFETY: T is a `#[repr(C)]` Windows structure of integers, pointers and unions of them; all-zero
    // is a valid bit pattern (null pointers, zero counts, empty flags). Only such types are passed here.
    unsafe { zeroed() }
}
