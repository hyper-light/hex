//! `endwait <rounds>`: how soon a completion port hears that a program ended, two ways (native plan step 8,
//! the lead's measurement on the decision "the Windows node, on one completion port"):
//! - `pool`: `RegisterWaitForSingleObject` on the process handle, its thread-pool callback posting to the
//!   port (`PostQueuedCompletionStatus`): a pool thread's hop on every end;
//! - `packet`: a wait completion packet (`NtCreateWaitCompletionPacket`, `NtAssociateWaitCompletionPacket`,
//!   ntdll) associated with the process handle and the port, which the kernel queues to the port itself
//!   when the handle is signalled: no hop. Undocumented in the SDK; the Windows thread pool and Go's runtime
//!   use it.
//! Each round, for each method in turn (the order alternating by round), the node's thread waits on the
//! port while a second thread, 2 ms later, stamps t0 and ends the watched object: a program of this probe's
//! (`linger`) by `TerminateProcess` (`process`), or an event by `SetEvent` (`event`, the hop alone, without a
//! process's teardown). The time is t0 to the port's dequeue. Prints a CSV row per notice: round, method,
//! kind, microseconds, and the machine's CPU load over the round.

use std::ffi::c_void;
use std::os::windows::io::AsRawHandle;
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::IO::{
    CreateIoCompletionPort, GetQueuedCompletionStatusEx, OVERLAPPED_ENTRY,
    PostQueuedCompletionStatus,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, INFINITE, RegisterWaitForSingleObject, SetEvent, TerminateProcess,
    UnregisterWaitEx, WT_EXECUTEONLYONCE,
};

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateWaitCompletionPacket(handle: *mut HANDLE, access: u32, attributes: *const c_void) -> i32;
    fn NtAssociateWaitCompletionPacket(
        packet: HANDLE,
        port: HANDLE,
        target: HANDLE,
        key: *const c_void,
        apc: *const c_void,
        status: i32,
        information: usize,
        already: *mut u8,
    ) -> i32;
}

/// GENERIC_ALL, as Go's runtime opens its packets.
const GENERIC_ALL: u32 = 0x1000_0000;
const KEY: usize = 7;

struct Port(HANDLE);
unsafe impl Send for Port {}
unsafe impl Sync for Port {}

fn port() -> Port {
    // SAFETY: a new port, no file.
    let h = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, null_mut(), 0, 1) };
    assert!(!h.is_null());
    Port(h)
}

/// Blocks on the port until one completion; returns when it was dequeued.
fn dequeue(p: &Port) -> Instant {
    let mut e = [OVERLAPPED_ENTRY { lpCompletionKey: 0, lpOverlapped: null_mut(), Internal: 0, dwNumberOfBytesTransferred: 0 }];
    let mut n = 0u32;
    // SAFETY: one entry of room.
    let ok = unsafe { GetQueuedCompletionStatusEx(p.0, e.as_mut_ptr(), 1, &mut n, 10_000, 0) };
    let at = Instant::now();
    assert!(ok != 0 && n == 1 && e[0].lpCompletionKey == KEY, "no completion in 10 s");
    at
}

struct Ctx {
    port: HANDLE,
}

unsafe extern "system" fn posted(ctx: *mut c_void, _timed_out: bool) {
    // SAFETY: ctx is the Ctx `watch_pool` made, alive until its wait is unregistered.
    let c = unsafe { &*(ctx as *const Ctx) };
    // SAFETY: the port is alive for the round.
    unsafe { PostQueuedCompletionStatus(c.port, 0, KEY, null()) };
}

/// Arms the method's watch of `target` on `port`; returns what disarms it once the notice came.
fn arm(method: &str, p: &Port, target: HANDLE) -> Box<dyn FnOnce()> {
    match method {
        "pool" => {
            let ctx = Box::into_raw(Box::new(Ctx { port: p.0 }));
            let mut wait: HANDLE = null_mut();
            // SAFETY: target and ctx outlive the wait, unregistered below before ctx is freed.
            let ok = unsafe {
                RegisterWaitForSingleObject(&mut wait, target, Some(posted), ctx as *const c_void, INFINITE, WT_EXECUTEONLYONCE)
            };
            assert!(ok != 0);
            let wait = wait as usize;
            let ctx = ctx as usize;
            Box::new(move || {
                // SAFETY: the wait registered above, waited out (INVALID_HANDLE_VALUE) before ctx goes.
                unsafe { UnregisterWaitEx(wait as HANDLE, INVALID_HANDLE_VALUE) };
                // SAFETY: made by Box::into_raw above, freed once.
                drop(unsafe { Box::from_raw(ctx as *mut Ctx) });
            })
        }
        _ => {
            let mut packet: HANDLE = null_mut();
            // SAFETY: the handle is written; no attributes.
            let st = unsafe { NtCreateWaitCompletionPacket(&mut packet, GENERIC_ALL, null()) };
            assert!(st >= 0, "NtCreateWaitCompletionPacket {st:#x}");
            let mut already = 0u8;
            // SAFETY: the packet, the port and the target are live handles; the key is posted back.
            let st = unsafe {
                NtAssociateWaitCompletionPacket(packet, p.0, target, KEY as *const c_void, null(), 0, 0, &mut already)
            };
            assert!(st >= 0, "NtAssociateWaitCompletionPacket {st:#x}");
            let packet = packet as usize;
            Box::new(move || {
                // SAFETY: the packet made above, its one completion dequeued.
                unsafe { CloseHandle(packet as HANDLE) };
            })
        }
    }
}

pub fn endwait(rounds: usize) {
    let exe = std::env::current_exe().unwrap();
    let p = Arc::new(port());
    let base = Instant::now();
    let t0 = Arc::new(AtomicU64::new(0));
    println!("round,method,kind,us,load");
    let mut samples = std::collections::VecDeque::new();
    for r in 0..rounds {
        let order: [&str; 2] = if r % 2 == 0 { ["pool", "packet"] } else { ["packet", "pool"] };
        let mut rows = Vec::new();
        for method in order {
            for kind in ["process", "event"] {
                let mut child = None;
                let target: HANDLE = match kind {
                    "process" => {
                        let c = std::process::Command::new(&exe).arg("linger").spawn().unwrap();
                        let h = c.as_raw_handle() as HANDLE;
                        child = Some(c);
                        h
                    }
                    // SAFETY: an unnamed manual-reset event, unsignalled.
                    _ => unsafe { CreateEventW(null(), 1, 0, null()) },
                };
                let disarm = arm(method, &p, target);
                let (t, target_u) = (Arc::clone(&t0), target as usize);
                let ender = std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(2));
                    t.store(base.elapsed().as_nanos() as u64, Ordering::SeqCst);
                    // SAFETY: the target is alive until this thread is joined.
                    unsafe {
                        if kind == "process" {
                            TerminateProcess(target_u as HANDLE, 1);
                        } else {
                            SetEvent(target_u as HANDLE);
                        }
                    }
                });
                let at = dequeue(&p);
                ender.join().unwrap();
                let us = (at.duration_since(base).as_nanos() as u64).saturating_sub(t0.load(Ordering::SeqCst)) as f64 / 1000.0;
                disarm();
                if let Some(mut c) = child {
                    let _ = c.wait();
                } else {
                    // SAFETY: the event made above.
                    unsafe { CloseHandle(target) };
                }
                rows.push((method, kind, us));
            }
        }
        let load = super::load_over_last_second(&mut samples);
        for (method, kind, us) in rows {
            println!("{r},{method},{kind},{us:.1},{load:.1}");
        }
    }
}
