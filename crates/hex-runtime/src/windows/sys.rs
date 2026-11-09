//! The Windows calls the `process` class makes that Rust can't make safely (CLAUDE.md §12 names this
//! file): `windows-sys` binds them only as `unsafe` foreign functions, and std's own job-object and
//! `PROC_THREAD_ATTRIBUTE_JOB_LIST`/`_HANDLE_LIST` process-creation support is unstable. Each `unsafe`
//! block states the invariant that makes it sound. A box is a job object: this file creates one, sets
//! its limits, starts a member suspended inside it (`PROC_THREAD_ATTRIBUTE_JOB_LIST`, so nothing runs
//! outside it) and resumes it, reads the job's accounting, and ends the job. Names, structures and
//! flags are Windows' own (`windows-sys`; docs/research/runtime/windows-job-objects.md).

// The Windows class's one place `unsafe` is allowed (CLAUDE.md §12); every block below carries its
// SAFETY comment.
#![allow(unsafe_code)]

use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    CloseHandle, DUPLICATE_CLOSE_SOURCE, DUPLICATE_SAME_ACCESS, DuplicateHandle,
    ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NOT_ENOUGH_QUOTA, FILETIME,
    GetLastError, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation,
    WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, JOB_OBJECT_CPU_RATE_CONTROL_ENABLE, JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
    JOB_OBJECT_CPU_RATE_CONTROL_WEIGHT_BASED, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_JOB_MEMORY, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_AND_IO_ACCOUNTING_INFORMATION, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAndIoAccountingInformation,
    JobObjectBasicProcessIdList, JobObjectCpuRateControlInformation,
    JobObjectExtendedLimitInformation, OpenJobObjectW, QueryInformationJobObject,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    ALL_PROCESSOR_GROUPS, CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    CreateEventW, CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
    GetActiveProcessorCount, GetCurrentProcess, GetExitCodeProcess, GetProcessTimes,
    InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcess,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_JOB_LIST, PROCESS_DUP_HANDLE,
    PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, ResumeThread,
    STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW, SetEvent, TerminateProcess,
    WaitForMultipleObjects, WaitForSingleObject,
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

/// The machine's physical memory and how much of it is available, in bytes (`GlobalMemoryStatusEx`):
/// the node's facts (`hex-node`'s `facts`).
pub fn memory() -> io::Result<(u64, u64)> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m: MEMORYSTATUSEX = zeroed_struct();
    m.dwLength = u32::try_from(size_of::<MEMORYSTATUSEX>())
        .map_err(|_| io::Error::other("MEMORYSTATUSEX's size past u32"))?;
    // SAFETY: `m` is a MEMORYSTATUSEX whose dwLength is its size, as the call requires; it outlives the
    // call, which only writes into it.
    let ok = unsafe { GlobalMemoryStatusEx(&raw mut m) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((m.ullTotalPhys, m.ullAvailPhys))
}

/// The size of the volume `dir` is on and the bytes this user may still write there
/// (`GetDiskFreeSpaceExW`): the node's facts.
pub fn disk_space(dir: &std::path::Path) -> io::Result<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let (mut caller, mut total, mut free) = (0u64, 0u64, 0u64);
    // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the call; the three outputs are u64s
    // that outlive it, each written once.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &raw mut caller,
            &raw mut total,
            &raw mut free,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((total, caller))
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
}

/// The rights a node takes on a box's job it opens by name (winnt.h's `JOB_OBJECT_ASSIGN_PROCESS`,
/// `_SET_ATTRIBUTES`, `_QUERY`, `_TERMINATE`; windows-sys has them only under
/// `Win32_System_SystemServices`): start members in it, set its limits, read it, end it.
const JOB_RIGHTS: u32 = 0x0001 | 0x0002 | 0x0004 | 0x0008;

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

    /// A new, empty job object named `name` (in the session's `Local\` namespace), its security the
    /// creator's default: this user and SYSTEM. One of that name held already is refused
    /// (`ERROR_ALREADY_EXISTS`): it is a box a node made before, which [`Job::open`] takes back.
    pub fn create_named(name: &str) -> io::Result<Job> {
        let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
        // SAFETY: null attributes; `wide` is a NUL-terminated name alive for the call. The handle returned
        // (to a new job, or to one of that name already, which GetLastError then says) is owned below.
        let h = unsafe { CreateJobObjectW(null(), wide.as_ptr()) };
        if h.is_null() {
            return Err(last_error());
        }
        let existed = last_error().raw_os_error() == i32::try_from(ERROR_ALREADY_EXISTS).ok();
        // SAFETY: h is a job handle this call returned, owned from here, closed as the value drops.
        let job = Job(unsafe { Handle::own(h) });
        if existed {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("a job named {name} is held already: a box a node made before"),
            ));
        }
        Ok(job)
    }

    /// The job named `name`, if one is: a box a node before this one made, which outlived it. `None` where
    /// no job holds the name (the box's members all ended and its last handle closed, or the machine
    /// restarted).
    pub fn open(name: &str) -> io::Result<Option<Job>> {
        let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
        // SAFETY: `wide` is a NUL-terminated name alive for the call; not inherited. The handle returned
        // is owned below.
        let h = unsafe { OpenJobObjectW(JOB_RIGHTS, 0, wide.as_ptr()) };
        if h.is_null() {
            let e = last_error();
            if e.raw_os_error() == i32::try_from(ERROR_FILE_NOT_FOUND).ok() {
                return Ok(None);
            }
            return Err(e);
        }
        // SAFETY: h is a job handle this call returned, owned from here.
        Ok(Some(Job(unsafe { Handle::own(h) })))
    }

    /// The job's handle, borrowed.
    pub fn borrowed(&self) -> std::os::windows::io::BorrowedHandle<'_> {
        self.0.borrowed()
    }

    pub fn raw(&self) -> HANDLE {
        self.0.raw()
    }

    /// Sets the job's limits: a hard commit cap, an active-process cap, kill-on-close (so closing the
    /// last handle ends the tree), and the CPU weight or hard cap. Breakaway is never allowed (neither
    /// `BREAKAWAY_OK` nor `SILENT_BREAKAWAY_OK` is set), so no member can leave the job.
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

    /// How many processes are live in the job right now: `JobObjectBasicProcessIdList`'s
    /// `NumberOfAssignedProcesses`, which Windows fills in even when the buffer holds fewer ids
    /// (`ERROR_MORE_DATA`; jobobject_basic_process_id_list.md: "The number of process identifiers to be
    /// stored in ProcessIdList"), so one read with room for one id learns the job's whole count at any
    /// size (CLAUDE.md §2: bounded by the job's own count, not a picked one). An ended member leaves the
    /// list though the node still holds its process handle, and this count agrees with the ids listed
    /// (`an_ended_member_whose_handle_is_held_leaves_the_id_list`, run on jupiter), so a box whose member
    /// has exited reads as empty. The job's accounting `ActiveProcesses` is not used.
    pub fn live_processes(&self) -> io::Result<u64> {
        let header = size_of::<u32>().saturating_mul(2);
        self.assigned_processes(header.saturating_add(size_of::<usize>()))
            .map(u64::from)
    }

    /// One read of the job's process id list into a buffer of `len` bytes, for its
    /// `NumberOfAssignedProcesses`. `ERROR_MORE_DATA` (the ids don't all fit) still fills it in.
    fn assigned_processes(&self, len: usize) -> io::Result<u32> {
        #[cfg(test)]
        tests::ID_LIST_READS.with(|n| n.set(n.get() + 1));
        // usize-aligned, as the list's ids are; the count is its first 4 bytes.
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
        let head: Vec<u8> = buf.iter().flat_map(|w| w.to_ne_bytes()).take(4).collect();
        Ok(<[u8; 4]>::try_from(head.as_slice()).map_or(0, u32::from_ne_bytes))
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

/// A copy of `source` (a handle of this process's) made in `into`'s process, with the same access: its
/// value there, which only `into` uses (a node's keeper, `super::keeper`). Not inheritable.
pub fn duplicate_into(source: usize, into: &Handle) -> io::Result<usize> {
    let mut theirs: HANDLE = null_mut();
    // SAFETY: `source` is a handle value of this process, `into` a process handle opened with
    // PROCESS_DUP_HANDLE (owned by the caller for the call); the value written is `into`'s, a number here.
    let ok = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            source as HANDLE,
            into.raw(),
            &raw mut theirs,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if ok == 0 {
        return Err(last_error());
    }
    Ok(theirs as usize)
}

/// A copy, in this process, of the handle `value` of `from`'s process, with the same access, owned here.
pub fn duplicate_from(from: &Handle, value: usize) -> io::Result<Handle> {
    let mut mine: HANDLE = null_mut();
    // SAFETY: `from` is a process handle opened with PROCESS_DUP_HANDLE; `value` names a handle in it (a
    // stale value is refused by the call, ERROR_INVALID_HANDLE, or names whatever handle has it, which the
    // caller checks); the handle written is this process's, owned below.
    let ok = unsafe {
        DuplicateHandle(
            from.raw(),
            value as HANDLE,
            GetCurrentProcess(),
            &raw mut mine,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if ok == 0 {
        return Err(last_error());
    }
    // SAFETY: a handle this call made in this process, owned from here.
    Ok(unsafe { Handle::own(mine) })
}

/// Closes the handle `value` in `into`'s process (`DUPLICATE_CLOSE_SOURCE`, no copy made).
pub fn close_in(into: &Handle, value: usize) -> io::Result<()> {
    // SAFETY: `into` is a process handle opened with PROCESS_DUP_HANDLE; with DUPLICATE_CLOSE_SOURCE and no
    // target the call closes `value` there and makes nothing here.
    let ok = unsafe {
        DuplicateHandle(
            into.raw(),
            value as HANDLE,
            null_mut(),
            null_mut(),
            0,
            0,
            DUPLICATE_CLOSE_SOURCE,
        )
    };
    if ok == 0 {
        return Err(last_error());
    }
    Ok(())
}

/// Process `pid` opened for waiting on, handing handles to and from, and reading its times; `None` where
/// no process has the pid.
pub fn open_process(pid: u32) -> io::Result<Option<Handle>> {
    // SAFETY: opens `pid` with the rights named; the handle returned is owned below.
    let h = unsafe {
        OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_DUP_HANDLE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        )
    };
    if h.is_null() {
        let e = last_error();
        // ERROR_INVALID_PARAMETER: no process has that pid.
        if e.raw_os_error() == Some(87) {
            return Ok(None);
        }
        return Err(e);
    }
    // SAFETY: a handle just opened, owned from here.
    Ok(Some(unsafe { Handle::own(h) }))
}

/// When `process` was created (100 ns ticks since 1601), which with its pid names it: a process given
/// the pid since has another.
pub fn created(process: &Handle) -> io::Result<u64> {
    let (mut c, mut e, mut k, mut u): (FILETIME, FILETIME, FILETIME, FILETIME) = (
        zeroed_struct(),
        zeroed_struct(),
        zeroed_struct(),
        zeroed_struct(),
    );
    // SAFETY: a process handle with PROCESS_QUERY_LIMITED_INFORMATION; four FILETIMEs written.
    let ok = unsafe {
        GetProcessTimes(
            process.raw(),
            &raw mut c,
            &raw mut e,
            &raw mut k,
            &raw mut u,
        )
    };
    if ok == 0 {
        return Err(last_error());
    }
    Ok((u64::from(c.dwHighDateTime) << 32) | u64::from(c.dwLowDateTime))
}

/// A manual-reset event named `name` (NUL-terminated wide, in the session's `Local\\` namespace),
/// unsignalled: made if new, else the one of that name opened. The node and its keeper share it by name so
/// either side makes it.
pub fn named_event(name: &[u16]) -> io::Result<Handle> {
    // SAFETY: no attributes; manual-reset, unsignalled; `name` is NUL-terminated and alive for the call.
    // CreateEventW returns a handle to a new event or to one of that name already, owned below.
    let h = unsafe { CreateEventW(null(), 1, 0, name.as_ptr()) };
    if h.is_null() {
        return Err(last_error());
    }
    // SAFETY: a handle just made or opened, owned from here.
    Ok(unsafe { Handle::own(h) })
}

/// Signals `event`.
pub fn set(event: &Handle) -> io::Result<()> {
    // SAFETY: an event handle this process owns.
    if unsafe { SetEvent(event.raw()) } == 0 {
        return Err(last_error());
    }
    Ok(())
}

/// Waits, without end, for the first of `a` and `b` to be signalled: `true` for `a`.
pub fn first_of(a: &Handle, b: &Handle) -> io::Result<bool> {
    let both = [a.raw(), b.raw()];
    // SAFETY: two live handles, waited on without end for either.
    match unsafe { WaitForMultipleObjects(2, both.as_ptr(), 0, u32::MAX) } {
        WAIT_OBJECT_0 => Ok(true),
        r if r == WAIT_OBJECT_0.saturating_add(1) => Ok(false),
        _ => Err(last_error()),
    }
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

/// Ends `process` alone with exit code `code` (`Proc::kill`: its job and the job's other members run on).
/// A process that has ended already is refused `ERROR_ACCESS_DENIED`; the caller reads its exit to tell.
pub fn terminate_process(process: &Handle, code: u32) -> io::Result<()> {
    // SAFETY: process is a valid, owned process handle; TerminateProcess ends that process and returns.
    if unsafe { TerminateProcess(process.raw(), code) } == 0 {
        return Err(last_error());
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use windows_sys::Win32::System::JobObjects::{
        JOBOBJECT_NOTIFICATION_LIMIT_INFORMATION, JobObjectNotificationLimitInformation,
    };

    thread_local! {
        /// The job id-list reads [`Job::live_processes`] made on this thread.
        pub(super) static ID_LIST_READS: Cell<u32> = const { Cell::new(0) };
    }

    fn nul() -> OwnedHandle {
        OwnedHandle::from(
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("NUL")
                .unwrap(),
        )
    }

    /// The job's id list read whole, with room for `room` ids: its `NumberOfAssignedProcesses`,
    /// `NumberOfProcessIdsInList` and the ids listed.
    fn id_list(job: &Job, room: usize) -> (u32, u32, Vec<usize>) {
        let mut buf = vec![0usize; room + 1];
        let bytes = u32::try_from(buf.len() * size_of::<usize>()).unwrap();
        // SAFETY: QueryInformationJobObject writes at most `bytes` bytes into `buf`, alive and that long
        // for the call; the header is two u32s in the first usize (x86_64), then the ids.
        let ok = unsafe {
            QueryInformationJobObject(
                job.raw(),
                JobObjectBasicProcessIdList,
                buf.as_mut_ptr().cast(),
                bytes,
                null_mut(),
            )
        };
        assert_ne!(ok, 0, "{}", last_error());
        let head = buf[0].to_ne_bytes();
        let assigned = u32::from_ne_bytes(head[..4].try_into().unwrap());
        let listed = u32::from_ne_bytes(head[4..8].try_into().unwrap());
        (assigned, listed, buf[1..=listed as usize].to_vec())
    }

    /// HEX-WIN-1 (native plan step 3): a member that has ended leaves the job's id list, which
    /// `live_processes` counts, though the node still holds its process handle. Do: run `cmd /c exit 0`
    /// in a job, wait for its end, keep its handle, read the list with room for every id. Expect: the
    /// member's id isn't listed, and the list's two counts agree in one read (so
    /// `NumberOfAssignedProcesses`, the count `live_processes` returns, keeps no ended member either).
    /// Another id may be listed: the console host Windows starts for a console program with no console,
    /// which ends after its client.
    #[test]
    fn an_ended_member_whose_handle_is_held_leaves_the_id_list() {
        let job = Job::create().unwrap();
        let system = std::env::var("SystemRoot").unwrap();
        let mut spawn = Spawn {
            command_line: format!("{system}\\System32\\cmd.exe /c exit 0")
                .encode_utf16()
                .chain([0])
                .collect(),
            environment: hex_core::job::environment(&[("SystemRoot".to_owned(), system)]),
            cwd: None,
            stdio: [nul(), nul(), nul()],
        };
        let started = create(&mut spawn, &[job.raw()]).unwrap();
        resume(&started.thread).unwrap();
        assert!(wait(&started.process, u32::MAX).unwrap());
        let (assigned, listed, ids) = id_list(&job, 64);
        assert!(
            !ids.contains(&(started.pid as usize)),
            "ended member {} still listed: {ids:?}",
            started.pid
        );
        assert_eq!(assigned, listed);
        drop(started);
    }

    /// T-T7.1.2 (native plan step 8), the evidence for how a Windows box outlives its node: do make a named
    /// job without kill-on-close, start a member in it, open the job by name while a handle is held, then
    /// close every handle to the job. Expect: the open finds it while held; the member runs on after the
    /// last handle closes; and the name is gone then (Windows' object manager drops a temporary object's
    /// name with its last handle, though its processes still hold the job), so a node that held the only
    /// handle can't be followed by one that opens its box by name.
    #[test]
    fn a_named_jobs_name_lapses_with_its_last_handle_though_its_member_runs() {
        let name = format!("Local\\hex-test-{}-lapse", std::process::id());
        let job = Job::create_named(&name).unwrap();
        // Kill-on-close off for this check alone, so the member's running past the last handle shows the
        // name lapsing and not the job ending it.
        let mut ext: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed_struct();
        ext.BasicLimitInformation.LimitFlags = 0;
        job.set(JobObjectExtendedLimitInformation, &ext).unwrap();
        let system = std::env::var("SystemRoot").unwrap();
        let mut spawn = Spawn {
            command_line: format!("{system}\\System32\\PING.EXE -n 600 127.0.0.1")
                .encode_utf16()
                .chain([0])
                .collect(),
            environment: hex_core::job::environment(&[("SystemRoot".to_owned(), system)]),
            cwd: None,
            stdio: [nul(), nul(), nul()],
        };
        let started = create(&mut spawn, &[job.raw()]).unwrap();
        resume(&started.thread).unwrap();
        let held = Job::open(&name).unwrap();
        assert!(
            held.is_some(),
            "a named job wasn't found by its name while held"
        );
        drop(held);
        drop(job);
        let running = !wait(&started.process, 0).unwrap();
        let after = Job::open(&name).unwrap();
        // No job is left to end it by: it is ended as any process is.
        std::process::Command::new("taskkill")
            .args(["/PID", &started.pid.to_string(), "/F"])
            .output()
            .unwrap();
        assert!(running, "the member ended with the job's last handle");
        assert!(
            after.is_none(),
            "the job was found by its name after its last handle closed"
        );
    }

    /// HEX-WIN-1 (native plan step 3): a box's live count is one read of its job, however many members
    /// it holds. Do: start two members that wait on stdin in a job, count the job's id-list reads one
    /// `live_processes` makes. Expect: one read, and a count of at least the two members (each console
    /// member may bring its console host).
    #[test]
    fn a_live_count_of_many_members_is_one_read() {
        let job = Job::create().unwrap();
        let system = std::env::var("SystemRoot").unwrap();
        let mut writers = Vec::new();
        let mut members = Vec::new();
        for _ in 0..2 {
            let (reader, writer) = std::io::pipe().unwrap();
            writers.push(writer);
            let mut spawn = Spawn {
                command_line: format!("{system}\\System32\\cmd.exe /c set /p x=")
                    .encode_utf16()
                    .chain([0])
                    .collect(),
                environment: hex_core::job::environment(&[(
                    "SystemRoot".to_owned(),
                    system.clone(),
                )]),
                cwd: None,
                stdio: [OwnedHandle::from(reader), nul(), nul()],
            };
            let started = create(&mut spawn, &[job.raw()]).unwrap();
            resume(&started.thread).unwrap();
            members.push(started);
        }
        ID_LIST_READS.with(|n| n.set(0));
        let live = job.live_processes().unwrap();
        let reads = ID_LIST_READS.with(Cell::get);
        drop(writers);
        for m in &members {
            assert!(wait(&m.process, u32::MAX).unwrap());
        }
        assert!(live >= 2, "two waiting members read as {live}");
        assert_eq!(reads, 1, "{reads} id-list reads for one live count");
    }

    /// Not a check on its own: run inside a box by
    /// `a_member_reaching_its_bound_is_counted_though_no_notification_comes`, it commits 1 MiB at a
    /// time until the box refuses (`HEX_TEST_COMMIT_TO_REFUSAL` set); otherwise it does nothing.
    #[test]
    fn commits_until_refused_when_asked() {
        if std::env::var_os("HEX_TEST_COMMIT_TO_REFUSAL").is_none() {
            return;
        }
        let mut held: Vec<Vec<u8>> = Vec::with_capacity(4096);
        while held.len() < 4096 {
            let mut chunk = Vec::new();
            if chunk.try_reserve_exact(1 << 20).is_err() {
                break;
            }
            chunk.resize(1 << 20, 1u8);
            held.push(chunk);
        }
    }

    /// HEX-RT-5 (native plan step 3): a member that reaches its box's memory bound and ends at once is
    /// counted, whether or not the job's memory notification is ever delivered. Under load Windows sets
    /// that notification seconds late, or never once the member's commit has fallen back (bench record
    /// windows-jupiter-20261105.txt entry 21, experiment e18). Do: make a 256 MiB box, clear its job's
    /// notification limit (52b298c's box set one and counted only its violation, so this is the
    /// notification that doesn't come), run a member that commits until the box refuses, wait for its
    /// end, read the box's usage. Expect: oom_kills at least 1, and the peak it reports within the bound.
    #[test]
    fn a_member_reaching_its_bound_is_counted_though_no_notification_comes() {
        let bound = 256u64 << 20;
        let bx = super::super::process::JobBox::create(
            hex_api::BoxId::new("sys-bound").unwrap(),
            &hex_api::Bounds {
                memory_max: Some(bound),
                ..hex_api::Bounds::default()
            },
            std::num::NonZeroU32::MIN,
        )
        .unwrap();
        let off: JOBOBJECT_NOTIFICATION_LIMIT_INFORMATION = zeroed_struct();
        // SAFETY: SetInformationJobObject reads one JOBOBJECT_NOTIFICATION_LIMIT_INFORMATION, alive for
        // the call, and sets it on the box's job, which `bx` owns for the test.
        let ok = unsafe {
            SetInformationJobObject(
                bx.job().as_raw_handle() as HANDLE,
                JobObjectNotificationLimitInformation,
                (&off as *const JOBOBJECT_NOTIFICATION_LIMIT_INFORMATION).cast(),
                u32::try_from(size_of::<JOBOBJECT_NOTIFICATION_LIMIT_INFORMATION>()).unwrap(),
            )
        };
        assert_ne!(ok, 0, "switching the notification off: {}", last_error());
        let exe = std::env::current_exe().unwrap();
        let system = std::env::var("SystemRoot").unwrap();
        let spawn = Spawn {
            command_line: format!(
                "\"{}\" windows::sys::tests::commits_until_refused_when_asked --exact --test-threads=1",
                exe.display()
            )
            .encode_utf16()
            .chain([0])
            .collect(),
            environment: hex_core::job::environment(&[
                ("SystemRoot".to_owned(), system),
                ("HEX_TEST_COMMIT_TO_REFUSAL".to_owned(), "1".to_owned()),
            ]),
            cwd: None,
            stdio: [nul(), nul(), nul()],
        };
        let started = bx.exec(spawn).unwrap();
        assert!(wait(&started.process, u32::MAX).unwrap());
        let usage = bx.usage().unwrap();
        assert!(
            usage.memory_peak_bytes <= bound,
            "the box reports more committed than its bound: {usage:?}"
        );
        assert!(
            usage.oom_kills >= 1,
            "the bound reached isn't counted: {usage:?}"
        );
    }

    /// HEX-WIN-2 (native plan step 3): a box commits up to its memory bound itself, and a commit that
    /// would cross it is refused whole. Windows' job refuses such a commit whole and counts its ask in the
    /// job's peak (`PeakJobMemoryUsed`), so the peak passes the commit limit by the refused ask (bench
    /// record windows-jupiter-20261105.txt entry 22, experiment e20). Do: make a 256 MiB box, run a member
    /// that commits 1 MiB at a time until the box refuses, read the job's raw peak. Expect: past the bound
    /// (the member was refused at the bound, not short of it) by at most the refused 1 MiB ask and the
    /// 131,072 bytes e20 measured beside one.
    #[test]
    fn a_box_commits_up_to_its_bound_and_is_refused_past_it() {
        let bound = 256u64 << 20;
        let bx = super::super::process::JobBox::create(
            hex_api::BoxId::new("sys-up-to-bound").unwrap(),
            &hex_api::Bounds {
                memory_max: Some(bound),
                ..hex_api::Bounds::default()
            },
            std::num::NonZeroU32::MIN,
        )
        .unwrap();
        let exe = std::env::current_exe().unwrap();
        let system = std::env::var("SystemRoot").unwrap();
        let spawn = Spawn {
            command_line: format!(
                "\"{}\" windows::sys::tests::commits_until_refused_when_asked --exact --test-threads=1",
                exe.display()
            )
            .encode_utf16()
            .chain([0])
            .collect(),
            environment: hex_core::job::environment(&[
                ("SystemRoot".to_owned(), system),
                ("HEX_TEST_COMMIT_TO_REFUSAL".to_owned(), "1".to_owned()),
            ]),
            cwd: None,
            stdio: [nul(), nul(), nul()],
        };
        let started = bx.exec(spawn).unwrap();
        assert!(wait(&started.process, u32::MAX).unwrap());
        let mut x: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed_struct();
        // SAFETY: QueryInformationJobObject writes at most one JOBOBJECT_EXTENDED_LIMIT_INFORMATION into
        // `x`, alive for the call, for the box's job, which `bx` owns for the test.
        let ok = unsafe {
            QueryInformationJobObject(
                bx.job().as_raw_handle() as HANDLE,
                JobObjectExtendedLimitInformation,
                (&mut x as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).unwrap(),
                null_mut(),
            )
        };
        assert_ne!(ok, 0, "reading the job's peak: {}", last_error());
        let peak = u64::try_from(x.PeakJobMemoryUsed).unwrap();
        assert!(
            peak > bound,
            "refused short of its bound: peak {peak}, bound {bound}"
        );
        assert!(
            peak <= bound + (1 << 20) + 131_072,
            "past its bound by more than one refused ask: peak {peak}, bound {bound}"
        );
    }
}
