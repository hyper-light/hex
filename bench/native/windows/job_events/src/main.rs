//! What a Windows job object enforces and tells, for Hex's `process` class (native plan step 3). Each
//! experiment prints lines starting with its name; members are this program run again as
//! `member <act>`. Run on jupiter: `cargo run --release -- all` (or name experiments: `-- e1 e5`).
//!
//! - e1  the job's peak against its commit limit, a native member committing until refused, assigned
//!       after a suspended start and created in the job (PROC_THREAD_ATTRIBUTE_JOB_LIST)
//! - e2  a process joining a job already at its limit: assigned, or created in it
//! - e3  a member's child started while the job is at its limit
//! - e4  a member's threads started while the job is at its limit
//! - e5  whether a notification limit (the one class Windows guarantees) fires on a refused commit
//! - e6  the active-process limit: what is refused, what the accounting counts, what is sent
//! - e7  TerminateJobObject: the exit code, the grandchild, the accounting, and an assign after it
//! - e8  breakaway from a job that doesn't allow it, and from a nested job that does under one that doesn't
//! - e9  kill-on-close of a nested job: its tree ends, its parent's other members don't
//! - e10 console members: whether a console host joins the job
//! - e11 the CPU weight's share: two jobs on one CPU
//! - e12 the CPU hard cap: a fraction of the whole machine
//! - e13 the node in a job of its own: a member born in it, a box nested under it, no breakaway
//!
//! A probe, not production code: it panics on what it doesn't expect.

#![allow(unsafe_op_in_unsafe_fn, clippy::all)]

use std::ffi::c_void;
use std::io::{BufRead, BufReader, Read, Write};
use std::mem::{size_of, size_of_val, zeroed};
use std::os::windows::io::AsRawHandle;
use std::ptr::{null, null_mut};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
    SetHandleInformation, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::IO::{CreateIoCompletionPort, GetQueuedCompletionStatus, OVERLAPPED};
use windows_sys::Win32::System::JobObjects::*;
use windows_sys::Win32::System::Memory::{MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc};
use windows_sys::Win32::System::SystemServices::{
    JOB_OBJECT_MSG_ABNORMAL_EXIT_PROCESS, JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT,
    JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO, JOB_OBJECT_MSG_EXIT_PROCESS, JOB_OBJECT_MSG_JOB_MEMORY_LIMIT,
    JOB_OBJECT_MSG_NEW_PROCESS, JOB_OBJECT_MSG_NOTIFICATION_LIMIT, JOB_OBJECT_MSG_PROCESS_MEMORY_LIMIT,
};
use windows_sys::Win32::System::Threading::{
    ALL_PROCESSOR_GROUPS, CREATE_BREAKAWAY_FROM_JOB, CREATE_NO_WINDOW, CREATE_SUSPENDED, CreateProcessW,
    CreateThread, DETACHED_PROCESS, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
    GetActiveProcessorCount, GetCurrentProcess, GetExitCodeProcess, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcess, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_JOB_LIST, PROCESS_INFORMATION, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, QueryFullProcessImageNameW, ResumeThread,
    STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject,
};

const MIB: usize = 1 << 20;
const KILL: u32 = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
const STILL_ACTIVE: u32 = 259;
/// The exit code TerminateJobObject is given for "TERM" here: a customer NTSTATUS-shaped value (bit 29
/// set), so a program's own exit code is unlikely to be it.
const TERM_CODE: u32 = 0xE048_000F;

fn last() -> u32 {
    unsafe { GetLastError() }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

struct Job(HANDLE);

impl Job {
    fn new() -> Job {
        let h = unsafe { CreateJobObjectW(null(), null()) };
        assert!(!h.is_null(), "CreateJobObjectW: {}", last());
        Job(h)
    }

    fn set<T>(&self, class: JOBOBJECTINFOCLASS, v: &T) -> Result<(), u32> {
        let ok = unsafe {
            SetInformationJobObject(self.0, class, (v as *const T).cast(), size_of::<T>() as u32)
        };
        if ok == 0 { Err(last()) } else { Ok(()) }
    }

    fn get<T: Copy>(&self, class: JOBOBJECTINFOCLASS) -> Result<T, u32> {
        let mut v: T = unsafe { zeroed() };
        let ok = unsafe {
            QueryInformationJobObject(
                self.0,
                class,
                (&mut v as *mut T).cast(),
                size_of::<T>() as u32,
                null_mut(),
            )
        };
        if ok == 0 { Err(last()) } else { Ok(v) }
    }

    fn limit(&self, flags: u32, memory: usize, processes: u32, affinity: usize) {
        let mut x: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        x.BasicLimitInformation.LimitFlags = flags;
        x.BasicLimitInformation.ActiveProcessLimit = processes;
        x.BasicLimitInformation.Affinity = affinity;
        x.JobMemoryLimit = memory;
        self.set(JobObjectExtendedLimitInformation, &x).expect("extended limits");
    }

    fn ext(&self) -> JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
        self.get(JobObjectExtendedLimitInformation).expect("query extended limits")
    }

    fn acct(&self) -> JOBOBJECT_BASIC_ACCOUNTING_INFORMATION {
        self.get(JobObjectBasicAccountingInformation).expect("query accounting")
    }

    fn assign(&self, process: HANDLE) -> Result<(), u32> {
        if unsafe { AssignProcessToJobObject(self.0, process) } == 0 { Err(last()) } else { Ok(()) }
    }

    fn port(&self, port: &Port, key: usize) {
        let a = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
            CompletionKey: key as *mut c_void,
            CompletionPort: port.0,
        };
        self.set(JobObjectAssociateCompletionPortInformation, &a).expect("associate port");
    }

    fn notify_memory(&self, limit: u64) -> Result<(), u32> {
        let mut n: JOBOBJECT_NOTIFICATION_LIMIT_INFORMATION = unsafe { zeroed() };
        n.LimitFlags = JOB_OBJECT_LIMIT_JOB_MEMORY;
        n.JobMemoryLimit = limit;
        self.set(JobObjectNotificationLimitInformation, &n)
    }

    fn violation(&self) -> Result<JOBOBJECT_LIMIT_VIOLATION_INFORMATION, u32> {
        self.get(JobObjectLimitViolationInformation)
    }

    fn pids(&self) -> Vec<u32> {
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct List {
            assigned: u32,
            listed: u32,
            ids: [usize; 256],
        }
        let l: List = self.get(JobObjectBasicProcessIdList).expect("process id list");
        l.ids[..l.listed as usize].iter().map(|p| *p as u32).collect()
    }

    fn cpu(&self, flags: u32, value: u32) -> Result<(), u32> {
        let mut c: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION = unsafe { zeroed() };
        c.ControlFlags = flags;
        // CpuRate and Weight share the union's slot.
        c.Anonymous.CpuRate = value;
        self.set(JobObjectCpuRateControlInformation, &c)
    }

    fn terminate(&self, code: u32) -> Result<(), u32> {
        if unsafe { TerminateJobObject(self.0, code) } == 0 { Err(last()) } else { Ok(()) }
    }

    fn holds(&self, process: HANDLE) -> bool {
        let mut r = 0;
        let ok = unsafe { IsProcessInJob(process, self.0, &mut r) };
        assert!(ok != 0, "IsProcessInJob: {}", last());
        r != 0
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

struct Port(HANDLE);

impl Port {
    fn new() -> Port {
        let h = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, null_mut(), 0, 1) };
        assert!(!h.is_null(), "CreateIoCompletionPort: {}", last());
        Port(h)
    }

    /// The messages that arrive within `wait`, as (name, key, process id).
    fn drain(&self, wait: Duration) -> Vec<String> {
        let mut out = Vec::new();
        let deadline = Instant::now() + wait;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let (mut msg, mut key, mut ov) = (0u32, 0usize, null_mut::<OVERLAPPED>());
            let ok = unsafe {
                GetQueuedCompletionStatus(self.0, &mut msg, &mut key, &mut ov, left.as_millis() as u32)
            };
            if ok == 0 {
                break;
            }
            out.push(format!("{}:{}", msg_name(msg), ov as usize));
        }
        out
    }
}

impl Drop for Port {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn msg_name(m: u32) -> &'static str {
    match m {
        JOB_OBJECT_MSG_NEW_PROCESS => "NEW",
        JOB_OBJECT_MSG_EXIT_PROCESS => "EXIT",
        JOB_OBJECT_MSG_ABNORMAL_EXIT_PROCESS => "ABNORMAL_EXIT",
        JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO => "ZERO",
        JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT => "ACTIVE_LIMIT",
        JOB_OBJECT_MSG_JOB_MEMORY_LIMIT => "JOB_MEMORY_LIMIT",
        JOB_OBJECT_MSG_PROCESS_MEMORY_LIMIT => "PROCESS_MEMORY_LIMIT",
        JOB_OBJECT_MSG_NOTIFICATION_LIMIT => "NOTIFICATION_LIMIT",
        _ => "OTHER",
    }
}

/// A started process: its handles, and the main thread's while it's suspended.
struct Proc {
    process: HANDLE,
    thread: HANDLE,
    pid: u32,
}

/// CreateProcessW of `cmdline` with `flags`, created in `jobs` (in order) when any are given, with
/// `stdio` as its stdin and stdout/stderr (the only handles it inherits) when given.
fn spawn_raw(cmdline: &str, flags: u32, jobs: &[HANDLE], stdio: Option<(HANDLE, HANDLE)>) -> Result<Proc, u32> {
    let mut si: STARTUPINFOEXW = unsafe { zeroed() };
    si.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    let handles: Vec<HANDLE> = match stdio {
        Some((i, o)) => {
            si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            si.StartupInfo.hStdInput = i;
            si.StartupInfo.hStdOutput = o;
            si.StartupInfo.hStdError = o;
            vec![i, o]
        }
        None => Vec::new(),
    };
    let count = u32::from(!handles.is_empty()) + u32::from(!jobs.is_empty());
    let mut size = 0usize;
    unsafe { InitializeProcThreadAttributeList(null_mut(), count.max(1), 0, &mut size) };
    let mut buf = vec![0u64; size / 8 + 1];
    let list = buf.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
    assert!(
        unsafe { InitializeProcThreadAttributeList(list, count.max(1), 0, &mut size) } != 0,
        "InitializeProcThreadAttributeList: {}",
        last()
    );
    if !handles.is_empty() {
        let ok = unsafe {
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr().cast(),
                size_of_val(handles.as_slice()),
                null_mut(),
                null(),
            )
        };
        assert!(ok != 0, "handle list: {}", last());
    }
    if !jobs.is_empty() {
        let ok = unsafe {
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                jobs.as_ptr().cast(),
                size_of_val(jobs),
                null_mut(),
                null(),
            )
        };
        assert!(ok != 0, "job list: {}", last());
    }
    si.lpAttributeList = list;
    let mut cmd = wide(cmdline);
    let mut pi: PROCESS_INFORMATION = unsafe { zeroed() };
    let ok = unsafe {
        CreateProcessW(
            null(),
            cmd.as_mut_ptr(),
            null(),
            null(),
            i32::from(!handles.is_empty()),
            flags | EXTENDED_STARTUPINFO_PRESENT,
            null(),
            null(),
            (&si as *const STARTUPINFOEXW).cast::<STARTUPINFOW>(),
            &mut pi,
        )
    };
    let err = last();
    unsafe { DeleteProcThreadAttributeList(list) };
    if ok == 0 {
        return Err(err);
    }
    Ok(Proc { process: pi.hProcess, thread: pi.hThread, pid: pi.dwProcessId })
}

fn inheritable(h: HANDLE) {
    assert!(unsafe { SetHandleInformation(h, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) } != 0);
}

fn exe() -> String {
    std::env::current_exe().expect("current exe").display().to_string()
}

fn resume(thread: HANDLE) {
    assert!(unsafe { ResumeThread(thread) } != u32::MAX, "ResumeThread: {}", last());
}

/// How a member is put in its jobs.
enum Place<'a> {
    /// Started suspended, assigned to each job in order, then resumed.
    Assign(&'a [&'a Job]),
    /// Started suspended in the jobs (PROC_THREAD_ATTRIBUTE_JOB_LIST), then resumed.
    JobList(&'a [&'a Job]),
}

/// A member: this program run as `member <act>`, with a pipe to its stdin and one from its stdout.
struct Member {
    p: Proc,
    lines: Receiver<String>,
    stdin: Option<std::io::PipeWriter>,
}

impl Member {
    fn start(act: &str, into: Place<'_>, extra: u32) -> Result<Member, String> {
        let (in_r, in_w) = std::io::pipe().expect("pipe");
        let (out_r, out_w) = std::io::pipe().expect("pipe");
        let (hi, ho) = (in_r.as_raw_handle() as HANDLE, out_w.as_raw_handle() as HANDLE);
        inheritable(hi);
        inheritable(ho);
        let cmdline = format!("\"{}\" member {act}", exe());
        let jobs: Vec<HANDLE> = match &into {
            Place::JobList(js) => js.iter().map(|j| j.0).collect(),
            Place::Assign(_) => Vec::new(),
        };
        let p = spawn_raw(&cmdline, CREATE_SUSPENDED | extra, &jobs, Some((hi, ho)))
            .map_err(|e| format!("create {e}"))?;
        drop(in_r);
        drop(out_w);
        if let Place::Assign(js) = into {
            for j in js {
                if let Err(e) = j.assign(p.process) {
                    unsafe { TerminateProcess(p.process, 1) };
                    return Err(format!("assign {e}"));
                }
            }
        }
        resume(p.thread);
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(out_r).lines() {
                match line {
                    Ok(l) => {
                        if tx.send(l).is_err() {
                            return;
                        }
                    }
                    Err(_) => return,
                }
            }
        });
        Ok(Member { p, lines: rx, stdin: Some(in_w) })
    }

    fn line(&self, wait: Duration) -> String {
        self.lines.recv_timeout(wait).unwrap_or_else(|_| "(no line)".to_owned())
    }

    fn release(&mut self) {
        self.stdin = None;
    }

    fn wait(&self, wait: Duration) -> Option<u32> {
        let r = unsafe { WaitForSingleObject(self.p.process, wait.as_millis() as u32) };
        if r != WAIT_OBJECT_0 {
            return None;
        }
        let mut code = 0u32;
        unsafe { GetExitCodeProcess(self.p.process, &mut code) };
        Some(code)
    }
}

impl Drop for Member {
    fn drop(&mut self) {
        unsafe {
            TerminateProcess(self.p.process, 1);
            CloseHandle(self.p.thread);
            CloseHandle(self.p.process);
        }
    }
}

/// Whether process `pid` still runs (false when it's gone or can't be opened).
fn alive(pid: u32) -> bool {
    let h = unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if h.is_null() {
        return false;
    }
    let r = unsafe { WaitForSingleObject(h, 0) };
    unsafe { CloseHandle(h) };
    r == WAIT_TIMEOUT
}

fn image(pid: u32) -> String {
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if h.is_null() {
        return format!("{pid}:?");
    }
    let mut buf = [0u16; 512];
    let mut n = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut n) };
    unsafe { CloseHandle(h) };
    if ok == 0 {
        return format!("{pid}:?");
    }
    let path = String::from_utf16_lossy(&buf[..n as usize]);
    format!("{pid}:{}", path.rsplit('\\').next().unwrap_or(&path))
}

const LINE: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------- experiments

fn e1() {
    let cap = 4096 * MIB;
    for (limit_mib, step) in [(64usize, MIB), (256, MIB), (256, 64 << 10), (256, 4 * MIB)] {
        for rep in 0..3 {
            let limit = limit_mib * MIB;
            let job = Job::new();
            job.limit(KILL | JOB_OBJECT_LIMIT_JOB_MEMORY, limit, 0, 0);
            let m = Member::start(&format!("commit {step} {cap} exit"), Place::Assign(&[&job]), 0)
                .expect("member");
            let said = m.line(LINE);
            let code = m.wait(LINE);
            let x = job.ext();
            println!(
                "e1 assign limit_mib={limit_mib} step={step} rep={rep} member[{said}] exit={code:?} peak_job={} over={} peak_process={}",
                x.PeakJobMemoryUsed,
                x.PeakJobMemoryUsed as i64 - limit as i64,
                x.PeakProcessMemoryUsed
            );
        }
    }
    for rep in 0..3 {
        let limit = 256 * MIB;
        let job = Job::new();
        job.limit(KILL | JOB_OBJECT_LIMIT_JOB_MEMORY, limit, 0, 0);
        let m = Member::start(&format!("commit {MIB} {cap} exit"), Place::JobList(&[&job]), 0).expect("member");
        let said = m.line(LINE);
        let code = m.wait(LINE);
        let x = job.ext();
        println!(
            "e1 joblist limit_mib=256 step={MIB} rep={rep} member[{said}] exit={code:?} peak_job={} over={} peak_process={}",
            x.PeakJobMemoryUsed,
            x.PeakJobMemoryUsed as i64 - limit as i64,
            x.PeakProcessMemoryUsed
        );
    }
    // What a suspended member's own commit is when it joins: charged to the job unexamined.
    for rep in 0..3 {
        let job = Job::new();
        job.limit(KILL | JOB_OBJECT_LIMIT_JOB_MEMORY, 256 * MIB, 0, 0);
        let p = spawn_raw(&format!("\"{}\" member sleep", exe()), CREATE_SUSPENDED, &[], None).expect("spawn");
        let assigned = job.assign(p.process);
        let at_assign = job.ext().PeakJobMemoryUsed;
        resume(p.thread);
        std::thread::sleep(Duration::from_millis(300));
        let running = job.ext().PeakJobMemoryUsed;
        unsafe { TerminateProcess(p.process, 1) };
        println!("e1 join rep={rep} assign={assigned:?} peak_at_assign={at_assign} peak_after_start={running}");
    }
}

fn e2() {
    let limit = 64 * MIB;
    for rep in 0..3 {
        let job = Job::new();
        job.limit(KILL | JOB_OBJECT_LIMIT_JOB_MEMORY, limit, 0, 0);
        let mut a = Member::start(&format!("commit {MIB} {} hold", 4096 * MIB), Place::Assign(&[&job]), 0).expect("a");
        let said = a.line(LINE);
        let before = job.ext().PeakJobMemoryUsed;
        let b = spawn_raw(&format!("\"{}\" member sleep", exe()), CREATE_SUSPENDED, &[], None).expect("b");
        let assigned = job.assign(b.process);
        let after = job.ext().PeakJobMemoryUsed;
        let c = spawn_raw(&format!("\"{}\" member sleep", exe()), CREATE_SUSPENDED, &[job.0], None);
        let after_list = job.ext().PeakJobMemoryUsed;
        println!(
            "e2 rep={rep} a[{said}] peak_full={before} over_full={} assign_b={assigned:?} peak_after_assign={after} over_after_assign={} create_in_job_c={} peak_after_create={after_list} over_after_create={}",
            before as i64 - limit as i64,
            after as i64 - limit as i64,
            match &c {
                Ok(_) => "ok".to_owned(),
                Err(e) => format!("err {e}"),
            },
            after_list as i64 - limit as i64
        );
        unsafe { TerminateProcess(b.process, 1) };
        if let Ok(c) = c {
            unsafe { TerminateProcess(c.process, 1) };
        }
        a.release();
        let _ = a.wait(LINE);
    }
}

fn e3() {
    let limit = 64 * MIB;
    for rep in 0..3 {
        let job = Job::new();
        job.limit(KILL | JOB_OBJECT_LIMIT_JOB_MEMORY, limit, 0, 0);
        let mut m = Member::start(&format!("commit {MIB} {} child", 4096 * MIB), Place::Assign(&[&job]), 0).expect("m");
        let said = m.line(LINE);
        let child = m.line(LINE);
        let x = job.ext();
        println!(
            "e3 rep={rep} member[{said}] [{child}] peak_job={} over={}",
            x.PeakJobMemoryUsed,
            x.PeakJobMemoryUsed as i64 - limit as i64
        );
        m.release();
        let _ = m.wait(LINE);
    }
}

fn e4() {
    let limit = 64 * MIB;
    for rep in 0..3 {
        let job = Job::new();
        job.limit(KILL | JOB_OBJECT_LIMIT_JOB_MEMORY, limit, 0, 0);
        let mut m = Member::start(&format!("commit {MIB} {} threads 32", 4096 * MIB), Place::Assign(&[&job]), 0).expect("m");
        let said = m.line(LINE);
        let threads = m.line(LINE);
        let x = job.ext();
        println!(
            "e4 rep={rep} member[{said}] [{threads}] peak_job={} over={}",
            x.PeakJobMemoryUsed,
            x.PeakJobMemoryUsed as i64 - limit as i64
        );
        m.release();
        let _ = m.wait(LINE);
    }
}

fn e5() {
    let limit = 64 * MIB;
    let cases: [(&str, Option<usize>, u64); 4] = [
        ("notify_at_hard", Some(limit), limit as u64),
        ("notify_hard_minus_64k", Some(limit), (limit - (64 << 10)) as u64),
        ("notify_hard_minus_1m", Some(limit), (limit - MIB) as u64),
        ("notify_no_hard", None, limit as u64),
    ];
    for (name, hard, notify) in cases {
        for rep in 0..3 {
            let job = Job::new();
            let port = Port::new();
            job.port(&port, 5);
            job.limit(
                KILL | if hard.is_some() { JOB_OBJECT_LIMIT_JOB_MEMORY } else { 0 },
                hard.unwrap_or(0),
                0,
                0,
            );
            let set = job.notify_memory(notify);
            let cap = if hard.is_some() { 4096 * MIB } else { 2 * limit };
            let m = Member::start(&format!("commit {MIB} {cap} exit"), Place::Assign(&[&job]), 0).expect("m");
            let said = m.line(LINE);
            let code = m.wait(LINE);
            let msgs = port.drain(Duration::from_millis(500));
            let v = job.violation();
            let x = job.ext();
            println!(
                "e5 {name} rep={rep} set={set:?} member[{said}] exit={code:?} msgs={msgs:?} violation={} peak_job={} over_hard={}",
                match v {
                    Ok(v) => format!(
                        "flags={:#x} violated={:#x} job_memory={} limit={}",
                        v.LimitFlags, v.ViolationLimitFlags, v.JobMemory, v.JobMemoryLimit
                    ),
                    Err(e) => format!("err {e}"),
                },
                x.PeakJobMemoryUsed,
                x.PeakJobMemoryUsed as i64 - limit as i64
            );
        }
    }
    // Re-arm: the notification is sent once until the violation is queried; a second refused episode.
    let job = Job::new();
    let port = Port::new();
    job.port(&port, 5);
    job.limit(KILL | JOB_OBJECT_LIMIT_JOB_MEMORY, limit, 0, 0);
    job.notify_memory(limit as u64).expect("notify");
    let m1 = Member::start(&format!("commit {MIB} {} exit", 4096 * MIB), Place::Assign(&[&job]), 0).expect("m1");
    let _ = m1.line(LINE);
    let _ = m1.wait(LINE);
    let first = port.drain(Duration::from_millis(500));
    let q1 = job.violation().map(|v| v.ViolationLimitFlags);
    let m2 = Member::start(&format!("commit {MIB} {} exit", 4096 * MIB), Place::Assign(&[&job]), 0).expect("m2");
    let _ = m2.line(LINE);
    let _ = m2.wait(LINE);
    let second = port.drain(Duration::from_millis(500));
    let q2 = job.violation().map(|v| v.ViolationLimitFlags);
    println!("e5 rearm first={first:?} q1={q1:?} second={second:?} q2={q2:?}");
}

fn e6() {
    for (name, extra) in [("plain", 0u32), ("detached", DETACHED_PROCESS)] {
        let job = Job::new();
        let port = Port::new();
        job.port(&port, 6);
        job.limit(KILL | JOB_OBJECT_LIMIT_ACTIVE_PROCESS, 0, 8, 0);
        let mut m = Member::start("spawn 32", Place::Assign(&[&job]), extra).expect("m");
        let said = m.line(LINE);
        let a = job.acct();
        let pids = job.pids();
        // A process from outside, into the full job.
        let b = spawn_raw(&format!("\"{}\" member sleep", exe()), CREATE_SUSPENDED, &[], None).expect("b");
        let assigned = job.assign(b.process);
        let b_code = {
            let r = unsafe { WaitForSingleObject(b.process, 2000) };
            let mut c = 0u32;
            unsafe { GetExitCodeProcess(b.process, &mut c) };
            (r == WAIT_OBJECT_0, c)
        };
        let a2 = job.acct();
        m.release();
        let code = m.wait(LINE);
        let msgs = port.drain(Duration::from_millis(500));
        println!(
            "e6 {name} member[{said}] exit={code:?} acct_total={} active={} terminated={} listed={} assign_outside={assigned:?} outside_ended={:?} after_outside: total={} active={} terminated={} msgs={msgs:?}",
            a.TotalProcesses,
            a.ActiveProcesses,
            a.TotalTerminatedProcesses,
            pids.len(),
            b_code,
            a2.TotalProcesses,
            a2.ActiveProcesses,
            a2.TotalTerminatedProcesses
        );
        unsafe { TerminateProcess(b.process, 1) };
    }
}

fn e7() {
    let job = Job::new();
    let port = Port::new();
    job.port(&port, 7);
    job.limit(KILL, 0, 0, 0);
    let m = Member::start("grandchild", Place::Assign(&[&job]), 0).expect("m");
    let said = m.line(LINE);
    let g: u32 = said.trim_start_matches("grandchild=").parse().unwrap_or(0);
    let before = alive(g);
    let t0 = Instant::now();
    let term = job.terminate(TERM_CODE);
    let code = m.wait(Duration::from_secs(10));
    let waited = t0.elapsed();
    let mut g_gone_after = None;
    for i in 0..1000 {
        if !alive(g) {
            g_gone_after = Some(i);
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let a = job.acct();
    let pids = job.pids();
    let b = spawn_raw(&format!("\"{}\" member sleep", exe()), CREATE_SUSPENDED, &[], None).expect("b");
    let assigned = job.assign(b.process);
    let b_runs = if assigned.is_ok() {
        resume(b.thread);
        std::thread::sleep(Duration::from_millis(200));
        alive(b.pid)
    } else {
        false
    };
    let a2 = job.acct();
    unsafe { TerminateProcess(b.process, 1) };
    let msgs = port.drain(Duration::from_millis(500));
    println!(
        "e7 [{said}] grandchild_alive_before={before} terminate={term:?} member_exit={:?} wait_us={} grandchild_gone_after_ms={g_gone_after:?} acct_while_member_handle_held: total={} active={} terminated={} listed={:?} assign_after_terminate={assigned:?} runs={b_runs} acct2: active={} terminated={} msgs={msgs:?}",
        code.map(|c| format!("{c:#x}")),
        waited.as_micros(),
        a.TotalProcesses,
        a.ActiveProcesses,
        a.TotalTerminatedProcesses,
        pids,
        a2.ActiveProcesses,
        a2.TotalTerminatedProcesses,
    );
}

fn e8() {
    let job = Job::new();
    job.limit(KILL, 0, 0, 0);
    let m = Member::start("breakaway", Place::Assign(&[&job]), 0).expect("m");
    let said = m.line(LINE);
    let _ = m.wait(LINE);
    println!("e8 one_job_no_breakaway [{said}]");
    // A box that forbids breakaway, over a job inside it that allows it: the child leaves the inner job
    // but not the box.
    let outer = Job::new();
    outer.limit(KILL, 0, 0, 0);
    let inner = Job::new();
    inner.limit(KILL | JOB_OBJECT_LIMIT_BREAKAWAY_OK, 0, 0, 0);
    let mut m = Member::start("breakaway_hold", Place::Assign(&[&outer, &inner]), 0).expect("m");
    let said = m.line(LINE);
    let pid: u32 = said
        .split_whitespace()
        .find_map(|w| w.strip_prefix("pid="))
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    let placement = if h.is_null() {
        format!("open err {}", last())
    } else {
        let s = format!("in_outer={} in_inner={}", outer.holds(h), inner.holds(h));
        unsafe { CloseHandle(h) };
        s
    };
    m.release();
    let _ = m.wait(LINE);
    println!("e8 nested_inner_allows [{said}] child {placement}");
}

fn e9() {
    let outer = Job::new();
    outer.limit(KILL, 0, 0, 0);
    let inner = Job::new();
    inner.limit(KILL, 0, 0, 0);
    let m = Member::start("grandchild", Place::Assign(&[&outer, &inner]), 0).expect("m");
    let said = m.line(LINE);
    let g: u32 = said.trim_start_matches("grandchild=").parse().unwrap_or(0);
    let s = Member::start("hold", Place::Assign(&[&outer]), 0).expect("s");
    let before = (alive(m.p.pid), alive(g), alive(s.p.pid));
    drop(inner);
    let code = m.wait(Duration::from_secs(10));
    std::thread::sleep(Duration::from_millis(200));
    println!(
        "e9 [{said}] before(member,grandchild,sibling)={before:?} inner_closed: member_exit={code:?} grandchild_alive={} sibling_alive={} outer_active={}",
        alive(g),
        alive(s.p.pid),
        outer.acct().ActiveProcesses
    );
}

fn e10() {
    for (name, extra) in [("plain", 0u32), ("no_window", CREATE_NO_WINDOW), ("detached", DETACHED_PROCESS)] {
        let job = Job::new();
        job.limit(KILL, 0, 0, 0);
        let mut m = Member::start("hold", Place::Assign(&[&job]), extra).expect("m");
        std::thread::sleep(Duration::from_millis(500));
        let names: Vec<String> = job.pids().into_iter().map(image).collect();
        m.release();
        let _ = m.wait(LINE);
        println!("e10 {name} job_processes={names:?}");
    }
    // A program through cmd.exe, as the lead's probe ran its member.
    let job = Job::new();
    job.limit(KILL, 0, 0, 0);
    let (in_r, in_w) = std::io::pipe().expect("pipe");
    let (out_r, out_w) = std::io::pipe().expect("pipe");
    let (hi, ho) = (in_r.as_raw_handle() as HANDLE, out_w.as_raw_handle() as HANDLE);
    inheritable(hi);
    inheritable(ho);
    let p = spawn_raw(&format!("cmd.exe /c \"\"{}\" member hold\"", exe()), CREATE_SUSPENDED, &[], Some((hi, ho)))
        .expect("cmd");
    drop(in_r);
    drop(out_w);
    job.assign(p.process).expect("assign cmd");
    resume(p.thread);
    std::thread::sleep(Duration::from_millis(800));
    let names: Vec<String> = job.pids().into_iter().map(image).collect();
    drop(in_w);
    drop(out_r);
    unsafe { WaitForSingleObject(p.process, 5000) };
    println!("e10 through_cmd job_processes={names:?}");
}

fn e11() {
    let n = unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) } as usize;
    let cpu = n - 1;
    for (wa, wb) in [(5u32, 5u32), (9, 5), (5, 1), (9, 1), (1, 9), (6, 5), (5, 4), (2, 1)] {
        let ja = Job::new();
        ja.limit(KILL | JOB_OBJECT_LIMIT_AFFINITY, 0, 0, 1 << cpu);
        let sa = ja.cpu(JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_WEIGHT_BASED, wa);
        let jb = Job::new();
        jb.limit(KILL | JOB_OBJECT_LIMIT_AFFINITY, 0, 0, 1 << cpu);
        let sb = jb.cpu(JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_WEIGHT_BASED, wb);
        let ma = Member::start("spin 1", Place::Assign(&[&ja]), 0).expect("a");
        let mb = Member::start("spin 1", Place::Assign(&[&jb]), 0).expect("b");
        let a0 = ja.acct();
        let b0 = jb.acct();
        std::thread::sleep(Duration::from_secs(4));
        let a1 = ja.acct();
        let b1 = jb.acct();
        let ta = (a1.TotalUserTime + a1.TotalKernelTime) - (a0.TotalUserTime + a0.TotalKernelTime);
        let tb = (b1.TotalUserTime + b1.TotalKernelTime) - (b0.TotalUserTime + b0.TotalKernelTime);
        drop(ma);
        drop(mb);
        println!(
            "e11 cpu={cpu} weights={wa}:{wb} set={sa:?}/{sb:?} cpu_ms={}:{} share_a={:.3} ratio={:.3}",
            ta / 10_000,
            tb / 10_000,
            ta as f64 / (ta + tb).max(1) as f64,
            ta as f64 / tb.max(1) as f64
        );
    }
}

fn e12() {
    let n = unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) };
    for milli in [1000u32, 2500, 4000] {
        let rate = (milli * 10 / n).max(1);
        let job = Job::new();
        job.limit(KILL, 0, 0, 0);
        let set = job.cpu(JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP, rate);
        let m = Member::start("spin 8", Place::Assign(&[&job]), 0).expect("m");
        std::thread::sleep(Duration::from_millis(500));
        let a0 = job.acct();
        let t0 = Instant::now();
        std::thread::sleep(Duration::from_secs(4));
        let a1 = job.acct();
        let wall = t0.elapsed();
        drop(m);
        let used = (a1.TotalUserTime + a1.TotalKernelTime) - (a0.TotalUserTime + a0.TotalKernelTime);
        println!(
            "e12 cpus={n} asked_milli={milli} cpu_rate={rate} set={set:?} used_cpus={:.3}",
            used as f64 / 1e7 / wall.as_secs_f64()
        );
    }
}

fn e13() {
    let node = Job::new();
    node.limit(KILL, 0, 0, 0);
    let me = node.assign(unsafe { GetCurrentProcess() });
    let bx = Job::new();
    bx.limit(KILL, 0, 0, 0);
    let p = spawn_raw(&format!("\"{}\" member breakaway", exe()), CREATE_SUSPENDED, &[], None).expect("p");
    let born_in_node = node.holds(p.process);
    let assigned = bx.assign(p.process);
    let in_box = bx.holds(p.process);
    resume(p.thread);
    let r = unsafe { WaitForSingleObject(p.process, 30_000) };
    let mut code = 0u32;
    unsafe { GetExitCodeProcess(p.process, &mut code) };
    println!(
        "e13 node_self_assign={me:?} member_born_in_node={born_in_node} assign_box={assigned:?} in_box={in_box} member_ended={} exit={code}",
        r == WAIT_OBJECT_0
    );
}

/// Does a refused creation show in the job's accounting anywhere (so a refused fork can be counted
/// without a lossy completion message, CLAUDE.md §3)? A member tries to start 32 into a box of 8 and
/// does not stop at the first refusal (~24 refused creations); poll TotalProcesses/Active/Terminated
/// through it. Then 5 assigns of an outside process into the full box, checking the same counters.
fn e14() {
    let job = Job::new();
    let port = Port::new();
    job.port(&port, 14);
    job.limit(KILL | JOB_OBJECT_LIMIT_ACTIVE_PROCESS, 0, 8, 0);
    let mut m = Member::start("spawnall 32", Place::Assign(&[&job]), 0).expect("m");
    let said = m.line(LINE);
    let during = job.acct();
    let mut assigns = Vec::new();
    let mut terminated = Vec::new();
    for _ in 0..5 {
        let b = spawn_raw(&format!("\"{}\" member sleep", exe()), CREATE_SUSPENDED, &[], None).expect("b");
        assigns.push(job.assign(b.process).is_ok());
        terminated.push(job.acct().TotalTerminatedProcesses);
        unsafe { TerminateProcess(b.process, 1) };
    }
    let a = job.acct();
    m.release();
    let _ = m.wait(LINE);
    let after = job.acct();
    println!(
        "e14 member[{said}] limit=8 during: total={} active={} terminated={} after_5_assigns: total={} active={} terminated={} assigns_ok={assigns:?} terminated_seq={terminated:?} after_member_gone: total={} active={} terminated={}",
        during.TotalProcesses, during.ActiveProcesses, during.TotalTerminatedProcesses,
        a.TotalProcesses, a.ActiveProcesses, a.TotalTerminatedProcesses,
        after.TotalProcesses, after.ActiveProcesses, after.TotalTerminatedProcesses
    );
}

/// The real mechanism: a box's job nested under the node's, a member born in the box via
/// PROC_THREAD_ATTRIBUTE_JOB_LIST (suspended, resumed), its fork, then delete (close the box job's
/// handle) ends the box's tree and leaves the node's other members. The node here is a job the probe
/// makes and assigns itself to, as the node agent's job would be.
fn e15() {
    let node = Job::new();
    node.limit(KILL, 0, 0, 0);
    node.assign(unsafe { GetCurrentProcess() }).expect("node self-assign");
    let bx = Job::new();
    bx.limit(KILL, 0, 0, 0);
    // A member created directly in the box job (inherits the node job from this process, its parent).
    let m = Member::start("grandchild", Place::JobList(&[&bx]), 0).expect("m");
    let said = m.line(LINE);
    let g: u32 = said.trim_start_matches("grandchild=").parse().unwrap_or(0);
    let mh = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, 0, m.p.pid) };
    let gh = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, 0, g) };
    let placed = format!(
        "member_in_node={} member_in_box={} grandchild_in_node={} grandchild_in_box={}",
        node.holds(mh), bx.holds(mh), node.holds(gh), bx.holds(gh)
    );
    // A second member in the node but not the box: it must survive the box's delete. The box member's
    // stdin stays open (it blocks in `hold`), so the box's delete is what ends it, not an stdin close.
    let other = Member::start("hold", Place::Assign(&[&node]), 0).expect("other");
    // Delete the box: drop its only handle with kill-on-close set.
    drop(bx);
    std::thread::sleep(Duration::from_millis(400));
    println!(
        "e15 [{said}] {placed} after_delete: member_alive={} grandchild_alive={} other_alive={} node_active={}",
        alive(m.p.pid), alive(g), alive(other.p.pid), node.acct().ActiveProcesses
    );
    unsafe {
        CloseHandle(mh);
        CloseHandle(gh);
    }
}

/// The memory sweep the lead asked for: a member committing at several step sizes into a 256 MiB job,
/// born in the job via JOB_LIST, so everything is charged from its first page. Shows whether the peak's
/// excess over the limit is a bounded constant (a settable margin) or scales with the commit step.
fn e16() {
    let limit = 256 * MIB;
    let cap = 4096 * MIB;
    for step in [64usize << 10, MIB, 4 * MIB, 16 * MIB, 64 * MIB, 256 * MIB] {
        for rep in 0..3 {
            let job = Job::new();
            job.limit(KILL | JOB_OBJECT_LIMIT_JOB_MEMORY, limit, 0, 0);
            let m = Member::start(&format!("commit {step} {cap} exit"), Place::JobList(&[&job]), 0)
                .expect("m");
            let said = m.line(LINE);
            let code = m.wait(LINE);
            let x = job.ext();
            let peak = x.PeakJobMemoryUsed;
            println!(
                "e16 step={step} rep={rep} member[{said}] exit={code:?} peak_job={peak} over={} peak_process={}",
                peak as i64 - limit as i64,
                x.PeakProcessMemoryUsed
            );
        }
    }
}

/// Is the memory notification's violation queryable from the job's accounting WITHOUT an associated
/// completion port (so the box need keep no undrained message queue, CLAUDE.md §2)? Set a notification
/// limit below the hard cap, run a hog, and query JobObjectLimitViolationInformation — once with no
/// port, once with a port for comparison.
fn e17() {
    let limit = 64 * MIB;
    let notify = (limit - 4 * MIB) as u64;
    let cap = 4096 * MIB;
    for with_port in [false, true] {
        for rep in 0..2 {
            let job = Job::new();
            let port = Port::new();
            if with_port {
                job.port(&port, 17);
            }
            job.limit(KILL | JOB_OBJECT_LIMIT_JOB_MEMORY, limit, 0, 0);
            let set = job.notify_memory(notify);
            let m = Member::start(&format!("commit {MIB} {cap} exit"), Place::Assign(&[&job]), 0)
                .expect("m");
            let said = m.line(LINE);
            let _ = m.wait(LINE);
            let v = job.violation();
            let msgs = if with_port { port.drain(Duration::from_millis(300)) } else { Vec::new() };
            println!(
                "e17 with_port={with_port} rep={rep} set={set:?} member[{said}] violated={} msgs={msgs:?}",
                match v {
                    Ok(v) => format!("{:#x} job_memory={}", v.ViolationLimitFlags, v.JobMemory),
                    Err(e) => format!("err {e}"),
                }
            );
        }
    }
}

/// e18: how often a member refused at Hex's commit cap leaves the job's memory notification violated,
/// and when, at Hex's own numbers (a 256 MiB bound, the cap OVERSHOOT_MARGIN under it, the notification
/// NOTIFY_MARGIN under the cap), placed as Hex places members (JOB_LIST). Per rep: the violation read
/// while the refused member still lives (`alive`), then after it ends (`ended`); where a read is 0, it
/// is read again every millisecond for 200 ms (`late_ms`, the first read that saw it, or `never`).
fn e18() {
    let bound = std::env::var("E18_BOUND_MIB").ok().and_then(|b| b.parse::<usize>().ok()).unwrap_or(256) * MIB;
    let assign = std::env::var("E18_PLACE").is_ok_and(|p| p == "assign");
    let limit = bound - 2 * 1_163_264;
    let notify = (limit - 4 * MIB) as u64;
    let cap = 2048 * MIB;
    let reps: usize = std::env::var("E18_REPS").ok().and_then(|r| r.parse().ok()).unwrap_or(200);
    for mode in ["hold", "exit"] {
        let (mut set_alive, mut set_ended, mut late, mut never) = (0, 0, 0, 0);
        for rep in 0..reps {
            let job = Job::new();
            job.limit(KILL | JOB_OBJECT_LIMIT_JOB_MEMORY, limit, 0, 0);
            job.notify_memory(notify).expect("notify");
            let place = if assign { Place::Assign(&[&job]) } else { Place::JobList(&[&job]) };
            let mut m = Member::start(&format!("commit {MIB} {cap} {mode}"), place, 0).expect("m");
            let said = m.line(LINE);
            let peak = job.ext().PeakJobMemoryUsed;
            let mut alive_ms = None;
            let first = if mode == "hold" {
                let mut v = job.violation().map(|v| (v.ViolationLimitFlags, v.JobMemory));
                let held = std::time::Instant::now();
                while held.elapsed() < Duration::from_millis(3000) {
                    if matches!(v, Ok((f, _)) if f & JOB_OBJECT_LIMIT_JOB_MEMORY != 0) {
                        alive_ms = Some(held.elapsed().as_millis());
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                    v = job.violation().map(|v| (v.ViolationLimitFlags, v.JobMemory));
                }
                m.release();
                v
            } else {
                Ok((0, 0))
            };
            let _ = m.wait(LINE);
            let ended = job.violation().map(|v| (v.ViolationLimitFlags, v.JobMemory));
            let seen = |r: &Result<(u32, u64), u32>| matches!(r, Ok((f, _)) if f & JOB_OBJECT_LIMIT_JOB_MEMORY != 0);
            let mut late_ms = None;
            if !seen(&first) && !seen(&ended) {
                for ms in 1..=200u32 {
                    std::thread::sleep(Duration::from_millis(1));
                    if seen(&job.violation().map(|v| (v.ViolationLimitFlags, v.JobMemory))) {
                        late_ms = Some(ms);
                        break;
                    }
                }
            }
            if seen(&first) {
                set_alive += 1;
            } else if seen(&ended) {
                set_ended += 1;
            } else if late_ms.is_some() {
                late += 1;
            } else {
                never += 1;
            }
            if !seen(&first) || !seen(&ended) || late_ms.is_some() || alive_ms.is_some() {
                println!(
                    "e18 {mode} rep={rep} member[{said}] peak={peak} peak_under_notify={} alive={first:?} alive_ms={alive_ms:?} ended={ended:?} late_ms={late_ms:?}",
                    (peak as i64) - (notify as i64)
                );
            }
        }
        println!(
            "e18 {mode} assign={assign} reps={reps} limit={limit} notify={notify} set_while_alive={set_alive} set_only_after_end={set_ended} set_late={late} never={never}"
        );
    }
}

/// e19: the job this process itself runs in, if any (a runner may start its commands in one, and a
/// box's job then nests under it): whether it is in a job, and that job's limits and notification limits.
fn e19() {
    let mut inside = 0i32;
    let ok = unsafe { IsProcessInJob(GetCurrentProcess(), null_mut(), &mut inside) };
    println!("e19 in_a_job={} (ok={ok})", inside != 0);
    if inside == 0 {
        return;
    }
    let mut x: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    let r1 = unsafe {
        QueryInformationJobObject(null_mut(), JobObjectExtendedLimitInformation, (&mut x as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(), size_of_val(&x) as u32, null_mut())
    };
    let mut n: JOBOBJECT_NOTIFICATION_LIMIT_INFORMATION = unsafe { zeroed() };
    let r2 = unsafe {
        QueryInformationJobObject(null_mut(), JobObjectNotificationLimitInformation, (&mut n as *mut JOBOBJECT_NOTIFICATION_LIMIT_INFORMATION).cast(), size_of_val(&n) as u32, null_mut())
    };
    let mut v: JOBOBJECT_LIMIT_VIOLATION_INFORMATION = unsafe { zeroed() };
    let r3 = unsafe {
        QueryInformationJobObject(null_mut(), JobObjectLimitViolationInformation, (&mut v as *mut JOBOBJECT_LIMIT_VIOLATION_INFORMATION).cast(), size_of_val(&v) as u32, null_mut())
    };
    println!(
        "e19 own_job ext(ok={r1}): flags={:#x} job_memory_limit={} process_memory_limit={} active={} | notify(ok={r2}): flags={:#x} job_memory={} | violation(ok={r3}): flags={:#x} violated={:#x} job_memory={}",
        x.BasicLimitInformation.LimitFlags, x.JobMemoryLimit, x.ProcessMemoryLimit, x.BasicLimitInformation.ActiveProcessLimit,
        n.LimitFlags, n.JobMemoryLimit, v.LimitFlags, v.ViolationLimitFlags, v.JobMemory
    );
}

// ---------------------------------------------------------------- members

fn commit_until_refused(step: usize, cap: usize) -> (usize, u32) {
    let mut total = 0usize;
    while total < cap {
        let p = unsafe { VirtualAlloc(null(), step, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) };
        if p.is_null() {
            return (total, last());
        }
        let mut off = 0;
        while off < step {
            unsafe { (p as *mut u8).add(off).write_volatile(1) };
            off += 4096;
        }
        total += step;
    }
    (total, 0)
}

fn hold() -> ! {
    let mut s = String::new();
    let _ = std::io::stdin().read_to_string(&mut s);
    std::process::exit(0)
}

fn say(s: &str) {
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{s}");
    let _ = out.flush();
}

unsafe extern "system" fn parked(_: *mut c_void) -> u32 {
    loop {
        std::thread::park();
    }
}

fn spawn_child(flags: u32) -> Result<Proc, u32> {
    spawn_raw(&format!("\"{}\" member sleep", exe()), flags, &[], None)
}

fn member(args: &[String]) -> ! {
    let act = args.first().map(String::as_str).unwrap_or("");
    let num = |i: usize| args.get(i).and_then(|a| a.parse::<usize>().ok()).unwrap_or(0);
    match act {
        "sleep" => loop {
            std::thread::park();
        },
        "hold" => hold(),
        "commit" => {
            let (total, err) = commit_until_refused(num(1), num(2));
            say(&format!("refused_after={total} err={err}"));
            match args.get(3).map(String::as_str) {
                Some("hold") => hold(),
                Some("threads") => {
                    let (mut ok, mut first_err) = (0, 0);
                    for _ in 0..num(4) {
                        let mut tid = 0u32;
                        let h = unsafe { CreateThread(null(), 0, Some(parked), null(), 0, &mut tid) };
                        if h.is_null() {
                            if first_err == 0 {
                                first_err = last();
                            }
                        } else {
                            ok += 1;
                        }
                    }
                    say(&format!("threads_ok={ok} first_err={first_err}"));
                    hold()
                }
                Some("child") => {
                    match spawn_child(0) {
                        Ok(c) => say(&format!("child=ok pid={}", c.pid)),
                        Err(e) => say(&format!("child=err {e}")),
                    }
                    hold()
                }
                _ => std::process::exit(3),
            }
        }
        "spawn" => {
            let (mut ok, mut first_err) = (0, 0);
            let mut kids = Vec::new();
            for _ in 0..num(1) {
                match spawn_child(0) {
                    Ok(c) => {
                        ok += 1;
                        kids.push(c);
                    }
                    Err(e) => {
                        first_err = e;
                        break;
                    }
                }
            }
            say(&format!("spawned={ok} first_err={first_err}"));
            hold()
        }
        // Like spawn, but keeps trying all N (doesn't stop at the first refusal), so a box of 8 sees
        // ~24 refused creations: whether the job's accounting counts any refusal (e14).
        "spawnall" => {
            let (mut ok, mut refused, mut first_err) = (0, 0, 0);
            let mut kids = Vec::new();
            for _ in 0..num(1) {
                match spawn_child(0) {
                    Ok(c) => {
                        ok += 1;
                        kids.push(c);
                    }
                    Err(e) => {
                        refused += 1;
                        if first_err == 0 {
                            first_err = e;
                        }
                    }
                }
            }
            say(&format!("spawned={ok} refused={refused} first_err={first_err}"));
            hold()
        }
        "breakaway" => {
            match spawn_child(CREATE_BREAKAWAY_FROM_JOB) {
                Ok(c) => {
                    let mut r = 0;
                    unsafe { IsProcessInJob(c.process, null_mut(), &mut r) };
                    unsafe { TerminateProcess(c.process, 1) };
                    say(&format!("breakaway=allowed child_in_a_job={}", r != 0));
                }
                Err(e) => say(&format!("breakaway=refused error={e}")),
            }
            std::process::exit(0)
        }
        "breakaway_hold" => {
            match spawn_child(CREATE_BREAKAWAY_FROM_JOB) {
                Ok(c) => say(&format!("breakaway=allowed pid={}", c.pid)),
                Err(e) => say(&format!("breakaway=refused error={e}")),
            }
            hold()
        }
        "grandchild" => {
            match spawn_child(0) {
                Ok(c) => say(&format!("grandchild={}", c.pid)),
                Err(e) => say(&format!("grandchild=err {e}")),
            }
            hold()
        }
        "spin" => {
            for _ in 1..num(1).max(1) {
                std::thread::spawn(|| {
                    let mut x = 0u64;
                    loop {
                        x = std::hint::black_box(x.wrapping_add(1));
                    }
                });
            }
            let mut x = 0u64;
            loop {
                x = std::hint::black_box(x.wrapping_add(1));
            }
        }
        _ => std::process::exit(2),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("member") {
        member(&args[1..]);
    }
    let all = ["e1", "e2", "e3", "e4", "e5", "e6", "e7", "e8", "e9", "e10", "e11", "e12", "e13", "e14", "e15", "e16", "e17", "e18", "e19"];
    let chosen: Vec<&str> = if args.is_empty() || args.iter().any(|a| a == "all") {
        all.to_vec()
    } else {
        args.iter().map(String::as_str).collect()
    };
    let n = unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) };
    println!("job_events cpus={n}");
    for e in chosen {
        let t = Instant::now();
        match e {
            "e1" => e1(),
            "e2" => e2(),
            "e3" => e3(),
            "e4" => e4(),
            "e5" => e5(),
            "e6" => e6(),
            "e7" => e7(),
            "e8" => e8(),
            "e9" => e9(),
            "e10" => e10(),
            "e11" => e11(),
            "e12" => e12(),
            "e13" => e13(),
            "e14" => e14(),
            "e15" => e15(),
            "e16" => e16(),
            "e17" => e17(),
            "e18" => e18(),
            "e19" => e19(),
            other => println!("unknown experiment {other}"),
        }
        println!("{e} took_ms={}", t.elapsed().as_millis());
    }
}
