//! The candidates for an exec's stdio between a Windows node and its caller (native plan step 8), over
//! Windows' AF_UNIX, which passes no handles:
//! - `framed`: the stream framed over the caller's own connection (length-prefixed chunks).
//! - `conn`: a connection per stream: the caller opens a second connection with a one-time token, and the
//!   node takes it only from the caller's pid (SIO_AF_UNIX_GETPEERPID).
//! - `pipe`: a named pipe per stream, first instance only, remote clients refused, taken only from the
//!   caller's pid (GetNamedPipeClientProcessId).
//! - `dup`: an anonymous pipe whose read end the node duplicates into the caller's process (DuplicateHandle),
//!   naming the handle to it over the connection.
//! `bench` runs the node and one caller, which asks for a stream of `bytes` from each candidate in turn, the
//! order rotated each round, and prints a CSV row per stream: the time to its first byte and to its last,
//! and the machine's CPU load over the round. Each stream is read to the bytes asked for, never to its end,
//! which afunix.sys doesn't always deliver (`receive`). `refuse` runs a second process of the caller's user against
//! the two candidates that have a node-facing door (`conn`, `pipe`), and says whether the node refused it.
//! A probe, outside Hex's workspace: its `unsafe` is the Win32 calls only.

use std::fs::File;
use std::io::{Read, Write};
use std::os::windows::io::FromRawHandle;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::ptr::{null, null_mut};
use std::time::Instant;

mod endwait;
mod iocp;
mod other;
mod peerclose;

use shards_afunix::{UnixListener, UnixStream};
use windows_sys::Win32::Foundation::{
    CloseHandle, DUPLICATE_CLOSE_SOURCE, DUPLICATE_SAME_ACCESS, DuplicateHandle, FILETIME, GENERIC_READ,
    HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Networking::WinSock::{SOCKET, WSAIoctl};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FlushFileBuffers, OPEN_EXISTING, PIPE_ACCESS_OUTBOUND,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, CreatePipe, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetSystemTimes, OpenProcess, PROCESS_DUP_HANDLE};

/// SIO_AF_UNIX_GETPEERPID: _WSAIOR(IOC_VENDOR, 256), afunix.h.
const SIO_AF_UNIX_GETPEERPID: u32 = 0x5800_0100;
const CHUNK: usize = 64 << 10;
const CANDIDATES: [&str; 4] = ["framed", "conn", "pipe", "dup"];

fn main() {
    shards_afunix::startup().expect("winsock");
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("bench") => bench(arg(&args, 1, 300), arg(&args, 2, 4 << 20)),
        Some("caller") => caller(Path::new(&args[1]), arg(&args, 2, 300), arg(&args, 3, 4 << 20)),
        Some("refuse") => refuse(),
        Some("iocp") => iocp::iocp(),
        Some("intrude") => intrude(&args[1], &args[2], &args[3], args.get(4).map(String::as_str)),
        Some("refuse-as") => other::refuse_as(&args[1], args.get(2).map(String::as_str)),
        Some("knock") => other::knock(&args[1], &args[2]),
        Some("endwait") => endwait::endwait(arg(&args, 1, 300)),
        Some("peerclose") => peerclose::peerclose(
            arg(&args, 1, 299),
            std::time::Duration::from_millis(arg(&args, 2, 1000) as u64),
        ),
        Some("peerclose-caller") => peerclose::caller(
            Path::new(&args[1]),
            arg(&args, 2, 299),
            std::time::Duration::from_millis(arg(&args, 3, 2000) as u64),
        ),
        Some("linger") => loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        },
        _ => eprintln!("usage: stdio_probe bench <rounds> <bytes> | refuse | iocp | refuse-as <user> [node socket] (password on stdin) | endwait <rounds> | peerclose <rounds> [watch_ms]"),
    }
}

fn arg(args: &[String], i: usize, default: usize) -> usize {
    args.get(i).and_then(|a| a.parse().ok()).unwrap_or(default)
}

fn socket_path() -> PathBuf {
    std::env::temp_dir().join(format!("hex-stdio-probe-{}.sock", std::process::id()))
}

fn spawn(args: &[&str]) -> Child {
    Command::new(std::env::current_exe().unwrap()).args(args).stdout(Stdio::inherit()).spawn().unwrap()
}

/// The node: serves one caller's requests until it closes its connection.
fn bench(rounds: usize, bytes: usize) {
    let path = socket_path();
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    let mut child = spawn(&["caller", path.to_str().unwrap(), &rounds.to_string(), &bytes.to_string()]);
    let (mut ctl, ()) = listener.accept().unwrap();
    let caller = peer_pid(&ctl);
    let payload = vec![0x5au8; CHUNK];
    let mut n = 0u32;
    while let Some((c, size)) = request(&mut ctl) {
        serve(&listener, &mut ctl, caller, c, size, &payload, &mut n, None);
    }
    child.wait().unwrap();
    let _ = std::fs::remove_file(&path);
}

/// A request on the control connection: the candidate and the stream's size.
fn request(ctl: &mut UnixStream) -> Option<(u8, usize)> {
    let mut b = [0u8; 9];
    ctl.read_exact(&mut b).ok()?;
    Some((b[0], u64::from_le_bytes(b[1..9].try_into().unwrap()) as usize))
}

/// One stream of `size` bytes to the caller through candidate `c`. `intruder`, in `refuse`, is told the
/// door (token or pipe name) before the caller is, so it tries first. Returns whether the node refused a
/// connection from a pid other than the caller's.
#[allow(clippy::too_many_arguments)]
fn serve(
    listener: &UnixListener,
    ctl: &mut UnixStream,
    caller: u32,
    c: u8,
    size: usize,
    payload: &[u8],
    n: &mut u32,
    intruder: Option<&dyn Fn(&str) -> Box<dyn Waits>>,
) -> bool {
    let mut refused = false;
    match CANDIDATES[c as usize] {
        "framed" => {
            let mut left = size;
            while left > 0 {
                let k = left.min(payload.len());
                ctl.write_all(&(k as u32).to_le_bytes()).unwrap();
                ctl.write_all(&payload[..k]).unwrap();
                left -= k;
            }
            ctl.write_all(&0u32.to_le_bytes()).unwrap();
        }
        "conn" => {
            *n += 1;
            let token = format!("{:016x}{:08x}", std::process::id() as u64 * 7919 + *n as u64, *n);
            // In `refuse`, the intruder gets the token first and its connection is taken before the
            // caller is told it, so the node's check is what stands between it and the stream.
            if let Some(f) = intruder {
                let mut ch = f(&token);
                let (mut s, ()) = listener.accept().unwrap();
                let mut got = [0u8; 24];
                let presented = s.read_exact(&mut got).is_ok() && got == token.as_bytes();
                refused = presented && peer_pid(&s) != caller;
                drop(s);
                ch.wait();
            }
            send_str(ctl, &token);
            loop {
                let (mut s, ()) = listener.accept().unwrap();
                let mut got = [0u8; 24];
                let ok = s.read_exact(&mut got).is_ok();
                if ok && got == token.as_bytes() && peer_pid(&s) == caller {
                    write_n(&mut s, size, payload);
                    break;
                }
            }
        }
        "pipe" => {
            *n += 1;
            let name = format!(r"\\.\pipe\hex-stdio-probe-{}-{}", std::process::id(), *n);
            let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
            let mut tried = intruder.is_none();
            loop {
                // SAFETY: wide is a NUL-terminated name alive for the call; null security takes the default.
                let h = unsafe {
                    CreateNamedPipeW(
                        wide.as_ptr(),
                        PIPE_ACCESS_OUTBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
                        PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                        1,
                        CHUNK as u32,
                        CHUNK as u32,
                        0,
                        null(),
                    )
                };
                assert!(h != INVALID_HANDLE_VALUE, "CreateNamedPipeW: {}", std::io::Error::last_os_error());
                // The pipe exists now: in `refuse` the intruder is told its name first, else the caller.
                let mut intruding = None;
                if tried {
                    send_str(ctl, &name);
                } else {
                    intruding = intruder.map(|f| f(&name));
                    tried = true;
                }
                // SAFETY: h is the pipe just made; a blocking connect with no OVERLAPPED.
                unsafe { ConnectNamedPipe(h, null_mut()) };
                let mut client = 0u32;
                // SAFETY: h is a connected pipe; client is written.
                unsafe { GetNamedPipeClientProcessId(h, &mut client) };
                if client == caller {
                    // SAFETY: h is owned here; File takes it over and closes it.
                    let mut f = unsafe { File::from_raw_handle(h as _) };
                    write_n(&mut f, size, payload);
                    // SAFETY: h is still open (owned by f); flushed, then closed by f, so the caller reads
                    // every byte and then the end (a disconnect would discard what it hasn't read).
                    unsafe { FlushFileBuffers(h) };
                    break;
                }
                refused = true;
                // SAFETY: h is the pipe made above; the other process's connection is dropped and the pipe closed.
                unsafe {
                    DisconnectNamedPipe(h);
                    CloseHandle(h);
                }
                if let Some(ch) = intruding.as_mut() {
                    ch.wait();
                }
            }
        }
        "dup" => {
            let (mut r, mut w): (HANDLE, HANDLE) = (null_mut(), null_mut());
            // SAFETY: r and w are written by CreatePipe; default security, a 64 KiB buffer.
            assert!(unsafe { CreatePipe(&mut r, &mut w, null(), CHUNK as u32) } != 0);
            // SAFETY: opens the caller's process for duplicating a handle into it.
            let proc = unsafe { OpenProcess(PROCESS_DUP_HANDLE, 0, caller) };
            assert!(!proc.is_null(), "OpenProcess: {}", std::io::Error::last_os_error());
            let mut theirs: HANDLE = null_mut();
            // SAFETY: r is ours and is closed by the duplication; theirs is the caller's handle value.
            let ok = unsafe {
                DuplicateHandle(
                    GetCurrentProcess(),
                    r,
                    proc,
                    &mut theirs,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS | DUPLICATE_CLOSE_SOURCE,
                )
            };
            assert!(ok != 0, "DuplicateHandle: {}", std::io::Error::last_os_error());
            // SAFETY: proc was opened above.
            unsafe { CloseHandle(proc) };
            ctl.write_all(&(theirs as usize as u64).to_le_bytes()).unwrap();
            // SAFETY: w is ours; File takes it over and closes it, which ends the caller's read.
            let mut f = unsafe { File::from_raw_handle(w as _) };
            write_n(&mut f, size, payload);
        }
        _ => unreachable!(),
    }
    refused
}

fn write_n(w: &mut impl Write, size: usize, payload: &[u8]) {
    let mut left = size;
    while left > 0 {
        let k = left.min(payload.len());
        w.write_all(&payload[..k]).unwrap();
        left -= k;
    }
}

fn send_str(ctl: &mut UnixStream, s: &str) {
    ctl.write_all(&(s.len() as u32).to_le_bytes()).unwrap();
    ctl.write_all(s.as_bytes()).unwrap();
}

fn recv_str(ctl: &mut UnixStream) -> String {
    let mut l = [0u8; 4];
    ctl.read_exact(&mut l).unwrap();
    let mut b = vec![0u8; u32::from_le_bytes(l) as usize];
    ctl.read_exact(&mut b).unwrap();
    String::from_utf8(b).unwrap()
}

/// The pid of a connected AF_UNIX socket's peer.
fn peer_pid(s: &UnixStream) -> u32 {
    let mut pid = 0u32;
    let mut ret = 0u32;
    // SAFETY: s is a connected socket; pid (4 bytes) is written.
    let rc = unsafe {
        WSAIoctl(
            s.raw() as SOCKET,
            SIO_AF_UNIX_GETPEERPID,
            null(),
            0,
            (&mut pid as *mut u32).cast(),
            4,
            &mut ret,
            null_mut(),
            None,
        )
    };
    assert!(rc == 0, "SIO_AF_UNIX_GETPEERPID: {}", std::io::Error::last_os_error());
    pid
}

/// The machine's CPU time, busy and all, from GetSystemTimes (100 ns ticks).
fn cpu() -> (u64, u64) {
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut idle, mut kernel, mut user) = (zero, zero, zero);
    // SAFETY: the three FILETIMEs are written.
    unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
    let t = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
    let all = t(kernel) + t(user);
    (all - t(idle), all)
}

/// The machine's CPU load (percent) over the last second at the call: `samples` keeps the CPU-time readings
/// back to the newest at least a second old (a round is milliseconds, finer than the ~15 ms tick
/// GetSystemTimes counts in).
fn load_over_last_second(samples: &mut std::collections::VecDeque<(Instant, u64, u64)>) -> f64 {
    let (b1, a1) = cpu();
    let now = Instant::now();
    samples.push_back((now, b1, a1));
    while samples.len() > 1 && now.duration_since(samples[1].0).as_secs_f64() >= 1.0 {
        samples.pop_front();
    }
    let (_, b0, a0) = samples[0];
    100.0 * (b1 - b0) as f64 / (a1 - a0).max(1) as f64
}

/// The caller: `rounds` rounds, each a stream from every candidate, the order rotated by the round.
fn caller(path: &Path, rounds: usize, bytes: usize) {
    let mut ctl = UnixStream::connect(path).unwrap();
    println!("round,candidate,first_us,last_us,mib_per_s,load_pct_last_s");
    let mut buf = vec![0u8; CHUNK];
    // The load over the last second at each round's end (a round is milliseconds, finer than the ~15 ms
    // tick GetSystemTimes counts in): samples kept back to the newest at least a second old.
    let mut samples: std::collections::VecDeque<(Instant, u64, u64)> = std::collections::VecDeque::new();
    for r in 0..rounds {
        let mut rows = Vec::new();
        for k in 0..CANDIDATES.len() {
            let c = (r + k) % CANDIDATES.len();
            let t0 = Instant::now();
            ctl.write_all(&[c as u8]).unwrap();
            ctl.write_all(&(bytes as u64).to_le_bytes()).unwrap();
            let (first, last, got) = receive(&mut ctl, path, c, bytes, &mut buf, t0);
            assert_eq!(got, bytes, "{} gave {got} of {bytes}", CANDIDATES[c]);
            rows.push((c, first, last));
        }
        let load = load_over_last_second(&mut samples);
        for (c, first, last) in rows {
            let span = (last - first).as_secs_f64().max(1e-9);
            println!(
                "{r},{},{},{},{:.1},{load:.1}",
                CANDIDATES[c],
                first.as_micros(),
                last.as_micros(),
                (bytes as f64 / (1 << 20) as f64) / span
            );
        }
    }
}

/// Reads one stream of the `want` bytes the caller asked for: when its first byte came, when its last did,
/// and how many came. No stream's end is waited for: a `conn` stream's end is its connection's graceful
/// close, which afunix.sys now and then never delivers to a read in another process (the hostile-frame
/// finding of docs/design/runtime.md; ~1 in 100 `conn` streams on a quiet jupiter, 2026-11-09, each with
/// its bytes all in), so a round that waited for it hung.
fn receive(
    ctl: &mut UnixStream,
    path: &Path,
    c: usize,
    want: usize,
    buf: &mut [u8],
    t0: Instant,
) -> (std::time::Duration, std::time::Duration, usize) {
    let mut first = None;
    let mut got = 0usize;
    let mut drain = |r: &mut dyn Read, first: &mut Option<std::time::Duration>, got: &mut usize| {
        while *got < want {
            let room = buf.len().min(want - *got);
            let k = r.read(&mut buf[..room]).unwrap();
            if k == 0 {
                break;
            }
            first.get_or_insert_with(|| t0.elapsed());
            *got += k;
        }
    };
    let last = match CANDIDATES[c] {
        "framed" => {
            let mut last = None;
            loop {
                let mut l = [0u8; 4];
                ctl.read_exact(&mut l).unwrap();
                let k = u32::from_le_bytes(l) as usize;
                if k == 0 {
                    // The end, a frame on the control connection: read, as the protocol has it, after the
                    // last byte's time is taken.
                    break;
                }
                ctl.read_exact(&mut buf[..k]).unwrap();
                first.get_or_insert_with(|| t0.elapsed());
                got += k;
                if got >= want {
                    last.get_or_insert_with(|| t0.elapsed());
                }
            }
            last.unwrap_or_else(|| t0.elapsed())
        }
        "conn" => {
            let token = recv_str(ctl);
            let mut s = UnixStream::connect(path).unwrap();
            s.write_all(token.as_bytes()).unwrap();
            drain(&mut s, &mut first, &mut got);
            t0.elapsed()
        }
        "pipe" => {
            let name = recv_str(ctl);
            let mut f = open_pipe(&name).unwrap();
            drain(&mut f, &mut first, &mut got);
            t0.elapsed()
        }
        "dup" => {
            let mut h = [0u8; 8];
            ctl.read_exact(&mut h).unwrap();
            // SAFETY: the node duplicated this handle into this process for this read; File owns it now.
            let mut f = unsafe { File::from_raw_handle(u64::from_le_bytes(h) as usize as _) };
            drain(&mut f, &mut first, &mut got);
            t0.elapsed()
        }
        _ => unreachable!(),
    };
    (first.unwrap_or_default(), last, got)
}

fn open_pipe(name: &str) -> std::io::Result<File> {
    let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
    // SAFETY: wide is a NUL-terminated name alive for the call.
    let h = unsafe { CreateFileW(wide.as_ptr(), GENERIC_READ, 0, null(), OPEN_EXISTING, 0, null_mut()) };
    if h == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: h was just opened and is owned by the File.
    Ok(unsafe { File::from_raw_handle(h as _) })
}

/// The refusal check: for `conn` and `pipe`, a second process of the caller's user is given the door
/// (the token or the pipe's name) before the caller, and tries it first. The node must refuse it and still
/// serve the caller.
fn refuse() {
    let path = socket_path();
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    let me = std::process::id();
    let payload = vec![0x5au8; CHUNK];
    let mut n = 0u32;
    for c in [1u8, 2u8] {
        // This process is the caller, on a connection of its own.
        let mut mine = UnixStream::connect(&path).unwrap();
        let (mut ctl, ()) = listener.accept().unwrap();
        assert_eq!(peer_pid(&ctl), me);
        let p = path.to_str().unwrap().to_owned();
        let name = CANDIDATES[c as usize];
        let intruder = move |door: &str| -> Box<dyn Waits> { Box::new(spawn(&["intrude", name, &p, door])) };
        let served = std::thread::scope(|sc| {
            let h = sc.spawn(|| {
                let mut buf = vec![0u8; CHUNK];
                receive(&mut mine, &path, c as usize, 1 << 20, &mut buf, Instant::now()).2
            });
            let refused = serve(&listener, &mut ctl, me, c, 1 << 20, &payload, &mut n, Some(&intruder));
            (refused, h.join().unwrap())
        });
        println!(
            "refuse {name}: a second process of the caller's user {} ; the caller got {} of {} bytes",
            if served.0 { "was REFUSED" } else { "WAS NOT REFUSED" },
            served.1,
            1 << 20
        );
    }
    let _ = std::fs::remove_file(&path);
}

/// An intruder the node waits out: a child of this user, or one started as another user.
pub trait Waits {
    fn wait(&mut self);
}

impl Waits for Child {
    fn wait(&mut self) {
        let _ = Child::wait(self);
    }
}

/// A process other than the caller, given a door: it tries to read the stream, and says how many bytes it
/// got on stderr, or in `result` where given (one started as another user has no stderr here).
fn intrude(candidate: &str, path: &str, door: &str, result: Option<&str>) {
    let mut buf = [0u8; 4096];
    let got = match candidate {
        "conn" => UnixStream::connect(path)
            .and_then(|mut s| {
                s.write_all(door.as_bytes())?;
                s.read(&mut buf)
            })
            .unwrap_or(0),
        "pipe" => open_pipe(door).and_then(|mut f| f.read(&mut buf)).unwrap_or(0),
        _ => 0,
    };
    let line = format!("intruder {candidate}: read {got} bytes");
    match result {
        Some(r) => std::fs::write(r, &line).unwrap(),
        None => eprintln!("{line}"),
    }
}
