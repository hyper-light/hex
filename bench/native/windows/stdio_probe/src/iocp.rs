//! `iocp`: what the Windows node's completion port design takes of Windows' AF_UNIX and doesn't know yet
//! (docs/design/runtime.md, "the Windows node, on one completion port", its *to measure* items):
//! - `accept`: an overlapped `AcceptEx` on an AF_UNIX listener completes on the port when a client
//!   connects, and the socket it took reads its peer's pid (`SIO_AF_UNIX_GETPEERPID`) once its accept
//!   context is updated;
//! - `zero-read`: a zero-byte overlapped `WSARecv` stays pending while nothing is waiting, completes when
//!   bytes come (and takes none of them), and completes when the peer ends;
//! - `send`: when an overlapped `WSASend` of more than the stream buffers completes, against a peer that
//!   reads nothing until a delay, then reads it all.
//!
//! Each prints `iocp <check>: PASS|FAIL <what was seen>`; the times are what one run saw, not a bench.

use std::io::{Read, Write};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use shards_afunix::{UnixListener, UnixStream};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_TIMEOUT};
use windows_sys::Win32::Networking::WinSock::{
    AF_UNIX, AcceptEx, INVALID_SOCKET, SO_UPDATE_ACCEPT_CONTEXT, SOCK_STREAM, SOCKADDR_UN, SOCKET,
    SOL_SOCKET, WSA_FLAG_NO_HANDLE_INHERIT, WSA_FLAG_OVERLAPPED, WSA_IO_PENDING, WSABUF, WSAGetLastError,
    WSAIoctl, WSARecv, WSASend, WSASocketW, closesocket, recv, setsockopt,
};
use windows_sys::Win32::System::IO::{
    CreateIoCompletionPort, GetQueuedCompletionStatusEx, OVERLAPPED, OVERLAPPED_ENTRY,
};

use super::SIO_AF_UNIX_GETPEERPID;

const LISTENER: usize = 1;
const CONN: usize = 2;
/// AcceptEx's address room: an address and 16 bytes, for each end (AcceptEx's documentation).
const ADDR: u32 = (size_of::<SOCKADDR_UN>() + 16) as u32;

struct Port(HANDLE);

impl Port {
    fn new() -> Port {
        // SAFETY: a new port, no file.
        let h = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, null_mut(), 0, 1) };
        assert!(
            !h.is_null(),
            "CreateIoCompletionPort: {}",
            std::io::Error::last_os_error()
        );
        Port(h)
    }

    fn add(&self, s: SOCKET, key: usize) {
        // SAFETY: an overlapped socket this probe owns, on this port.
        let h = unsafe { CreateIoCompletionPort(s as HANDLE, self.0, key, 0) };
        assert!(
            !h.is_null(),
            "associating a socket: {}",
            std::io::Error::last_os_error()
        );
    }

    /// One completion within `within`: its key and bytes, or None.
    fn next(&self, within: Duration) -> Option<(usize, u32, *mut OVERLAPPED)> {
        let mut e = [OVERLAPPED_ENTRY {
            lpCompletionKey: 0,
            lpOverlapped: null_mut(),
            Internal: 0,
            dwNumberOfBytesTransferred: 0,
        }];
        let mut n = 0u32;
        let ms = u32::try_from(within.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: one entry of room, the count written.
        let ok = unsafe { GetQueuedCompletionStatusEx(self.0, e.as_mut_ptr(), 1, &mut n, ms, 0) };
        if ok == 0 {
            let err = std::io::Error::last_os_error();
            assert!(
                err.raw_os_error() == Some(WAIT_TIMEOUT as i32),
                "GetQueuedCompletionStatusEx: {err}"
            );
            return None;
        }
        (n == 1).then(|| {
            (
                e[0].lpCompletionKey,
                e[0].dwNumberOfBytesTransferred,
                e[0].lpOverlapped,
            )
        })
    }
}

impl Drop for Port {
    fn drop(&mut self) {
        // SAFETY: the port this value made.
        unsafe { CloseHandle(self.0) };
    }
}

fn socket() -> SOCKET {
    // SAFETY: a new AF_UNIX stream socket.
    let s = unsafe {
        WSASocketW(
            i32::from(AF_UNIX),
            SOCK_STREAM,
            0,
            null(),
            0,
            WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT,
        )
    };
    assert!(
        s != INVALID_SOCKET,
        "WSASocketW: {}",
        std::io::Error::last_os_error()
    );
    s
}

fn wsa() -> i32 {
    // SAFETY: reads this thread's last Winsock error.
    unsafe { WSAGetLastError() }
}

fn peer_pid(s: SOCKET) -> Result<u32, i32> {
    let mut pid = 0u32;
    let mut ret = 0u32;
    // SAFETY: pid (4 bytes) is written.
    let rc = unsafe {
        WSAIoctl(
            s,
            SIO_AF_UNIX_GETPEERPID,
            null(),
            0,
            (&raw mut pid).cast(),
            4,
            &mut ret,
            null_mut(),
            None,
        )
    };
    if rc == 0 { Ok(pid) } else { Err(wsa()) }
}

/// A zero-byte overlapped receive on `s`: pending, or completed at once (an error code otherwise).
fn zero_recv(s: SOCKET, ov: &mut OVERLAPPED) -> Result<bool, i32> {
    *ov = unsafe { std::mem::zeroed() };
    let mut buf = WSABUF {
        len: 0,
        buf: null_mut(),
    };
    let mut flags = 0u32;
    // SAFETY: an empty buffer, and an OVERLAPPED that outlives the receive (the caller's, waited out).
    let rc = unsafe { WSARecv(s, &mut buf, 1, null_mut(), &mut flags, ov, None) };
    match rc {
        0 => Ok(false),
        _ if wsa() == WSA_IO_PENDING => Ok(true),
        _ => Err(wsa()),
    }
}

pub fn iocp() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("hex-iocp-probe-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    let port = Port::new();
    port.add(listener.raw() as SOCKET, LISTENER);

    // accept: AcceptEx pending, then a client, then its completion.
    let accepted = socket();
    let mut out = vec![0u8; (2 * ADDR) as usize];
    let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
    let mut got = 0u32;
    // SAFETY: room for both addresses and no data; the OVERLAPPED and buffer outlive the accept.
    let ok = unsafe {
        AcceptEx(
            listener.raw() as SOCKET,
            accepted,
            out.as_mut_ptr().cast(),
            0,
            ADDR,
            ADDR,
            &mut got,
            &mut ov,
        )
    };
    let pending = ok == 0 && wsa() == WSA_IO_PENDING;
    if ok == 0 && !pending {
        println!("iocp accept: FAIL AcceptEx refused at once, WSA error {}", wsa());
        return;
    }
    let early = port.next(Duration::from_millis(200));
    let client = spawn_client(&path);
    let t = Instant::now();
    let done = early.or_else(|| port.next(Duration::from_secs(5)));
    let listener_raw = listener.raw() as SOCKET;
    // SAFETY: the listener's value, for SO_UPDATE_ACCEPT_CONTEXT on the socket AcceptEx took it into.
    let upd = unsafe {
        setsockopt(
            accepted,
            SOL_SOCKET,
            SO_UPDATE_ACCEPT_CONTEXT,
            (&raw const listener_raw).cast(),
            size_of::<SOCKET>() as i32,
        )
    };
    let pid = peer_pid(accepted);
    match (early, done) {
        (None, Some((LISTENER, _, _))) => println!(
            "iocp accept: PASS pending until a client connected, completed {:?} after; update context rc {upd}; peer pid {pid:?} (the client is {})",
            t.elapsed(),
            client.pid
        ),
        (Some(_), _) => println!("iocp accept: FAIL completed before any client connected"),
        _ => println!("iocp accept: FAIL no completion within 5 s of a connect ({done:?})"),
    }
    port.add(accepted, CONN);

    // zero-read: pending while nothing waits, completes when bytes come, takes none; completes at the end.
    let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
    let pend = zero_recv(accepted, &mut ov);
    let quiet = port.next(Duration::from_millis(300));
    client.tell(b"s");
    let t = Instant::now();
    let woke = port.next(Duration::from_secs(5));
    let mut b = [0u8; 64];
    let n = recv_now(accepted, &mut b);
    match (pend, quiet, woke) {
        (Ok(true), None, Some((CONN, 0, _))) if n == Ok(10) => println!(
            "iocp zero-read data: PASS pending while quiet, completed with 0 bytes {:?} after the peer wrote, all 10 bytes then read",
            t.elapsed()
        ),
        _ => println!(
            "iocp zero-read data: FAIL pend {pend:?} quiet {} woke {woke:?} then read {n:?}",
            quiet.is_some()
        ),
    }
    let pend = zero_recv(accepted, &mut ov);
    let quiet = port.next(Duration::from_millis(300));
    client.tell(b"e");
    let t = Instant::now();
    let woke = port.next(Duration::from_secs(5));
    let n = recv_now(accepted, &mut b);
    match (pend, quiet, woke) {
        (Ok(true), None, Some((CONN, 0, _))) if n == Ok(0) => println!(
            "iocp zero-read end: PASS pending while quiet, completed {:?} after the peer ended, the read then 0 (its end)",
            t.elapsed()
        ),
        _ => println!(
            "iocp zero-read end: FAIL pend {pend:?} quiet {} woke {woke:?} then read {n:?}",
            quiet.is_some()
        ),
    }
    let ended = client.wait();
    // SAFETY: the socket AcceptEx took, its operations all completed.
    unsafe { closesocket(accepted) };

    // send: 8 MiB in one overlapped send to a peer that reads after 500 ms.
    let accepted = socket();
    let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
    // SAFETY: as above.
    let ok = unsafe {
        AcceptEx(
            listener.raw() as SOCKET,
            accepted,
            out.as_mut_ptr().cast(),
            0,
            ADDR,
            ADDR,
            &mut got,
            &mut ov,
        )
    };
    assert!(ok != 0 || wsa() == WSA_IO_PENDING, "AcceptEx: {}", wsa());
    let reader = spawn_reader(&path);
    assert!(
        matches!(port.next(Duration::from_secs(5)), Some((LISTENER, _, _))),
        "second accept"
    );
    port.add(accepted, CONN);
    let size = 8usize << 20;
    let data = vec![0x5au8; size];
    let mut buf = WSABUF {
        len: size as u32,
        buf: data.as_ptr().cast_mut(),
    };
    let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
    let t = Instant::now();
    // SAFETY: data and the OVERLAPPED outlive the send, which is waited out below.
    let rc = unsafe { WSASend(accepted, &mut buf, 1, null_mut(), 0, &mut ov, None) };
    let state = if rc == 0 {
        "completed at once".to_owned()
    } else if wsa() == WSA_IO_PENDING {
        "pending".to_owned()
    } else {
        format!("refused {}", wsa())
    };
    let done = port.next(Duration::from_secs(10));
    let took = t.elapsed();
    let read = reader.wait();
    match done {
        Some((CONN, n, _)) if n as usize == size => println!(
            "iocp send: PASS 8 MiB in one overlapped send ({state}), completed whole {took:?} after it was made (the peer began reading at 500 ms); the peer read {read}"
        ),
        _ => println!("iocp send: FAIL ({state}) completion {done:?} after {took:?}; the peer read {read}"),
    }
    // SAFETY: as above.
    unsafe { closesocket(accepted) };
    let _ = ended;
    let _ = std::fs::remove_file(&path);
}

/// A plain (not overlapped) receive, so no completion is posted to the port: the zero-byte receive's
/// completion said something is waiting, so it doesn't block.
fn recv_now(s: SOCKET, b: &mut [u8]) -> Result<usize, i32> {
    // SAFETY: b's own length.
    let n = unsafe { recv(s, b.as_mut_ptr(), b.len() as i32, 0) };
    usize::try_from(n).map_err(|_| wsa())
}

/// The probe's client, in a thread: connects, then on each word sent it writes 10 bytes (`s`) or ends (`e`).
struct Client {
    pid: u32,
    tell: std::sync::mpsc::Sender<u8>,
    done: std::thread::JoinHandle<()>,
}

impl Client {
    fn tell(&self, w: &[u8]) {
        for b in w {
            self.tell.send(*b).unwrap();
        }
    }
    fn wait(self) {
        drop(self.tell);
        self.done.join().unwrap();
    }
}

fn spawn_client(path: &Path) -> Client {
    let (tell, words) = std::sync::mpsc::channel::<u8>();
    let path = path.to_owned();
    let done = std::thread::spawn(move || {
        let mut s = UnixStream::connect(&path).unwrap();
        for w in words {
            match w {
                b's' => s.write_all(&[7u8; 10]).unwrap(),
                _ => return,
            }
        }
    });
    Client {
        pid: std::process::id(),
        tell,
        done,
    }
}

struct Reader(std::thread::JoinHandle<usize>);

impl Reader {
    fn wait(self) -> usize {
        self.0.join().unwrap()
    }
}

fn spawn_reader(path: &Path) -> Reader {
    let path = path.to_owned();
    Reader(std::thread::spawn(move || {
        let mut s = UnixStream::connect(&path).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        let mut all = Vec::new();
        let mut b = vec![0u8; 1 << 20];
        while all.len() < 8 << 20 {
            match s.read(&mut b) {
                Ok(0) | Err(_) => break,
                Ok(n) => all.extend_from_slice(&b[..n]),
            }
        }
        all.len()
    }))
}
