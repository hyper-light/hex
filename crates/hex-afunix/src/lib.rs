//! The Windows calls the node agent's socket and its client make that Rust can't make safely (native plan
//! step 8; the
//! decision "the Windows node, on one completion port" in docs/design/runtime.md): one I/O completion
//! port that every source reports on, Windows' AF_UNIX stream sockets (afunix.sys, Windows 10 1803 on),
//! each connection's overlapped receive and send and the listener's overlapped `AcceptEx`, a program's
//! end by a registered wait on its process handle, and who a connection's peer is (its pid by
//! `SIO_AF_UNIX_GETPEERPID`, then its process's user). `windows-sys` binds these as `unsafe` foreign
//! functions; std has no AF_UNIX socket on Windows and no completion port. Each `unsafe` block states the
//! invariant that makes it sound.
//!
//! An overlapped operation's `OVERLAPPED` and buffer are the kernel's from its start until its
//! completion is dequeued. Each is an [`Op`] on the heap, handed to the kernel as a raw pointer and taken
//! back exactly once: from the port when it completes, or at once when the start fails (no completion is
//! queued then). So nothing the kernel may write is freed or moved under it, whatever its connection does
//! meanwhile: closing a socket ends its operations, and their completions still bring their `Op`s back.

// This file's `unsafe` is named in CLAUDE.md §12; every block below carries its SAFETY comment.
#![allow(unsafe_code)]
#![cfg(windows)]

use std::ffi::c_void;
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_OPERATION_ABORTED, FALSE, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::Networking::WinSock::{
    AF_UNIX, AcceptEx, INVALID_SOCKET, SD_BOTH, SO_SNDTIMEO, SO_UPDATE_ACCEPT_CONTEXT, SOCK_STREAM,
    SOCKADDR_UN, SOCKET, SOCKET_ERROR, SOL_SOCKET, WSA_FLAG_NO_HANDLE_INHERIT, WSA_FLAG_OVERLAPPED,
    WSA_IO_PENDING, WSABUF, WSADATA, WSAECONNABORTED, WSAECONNRESET, WSAEMFILE, WSAENOBUFS,
    WSAGetLastError, WSAIoctl, WSARecv, WSASend, WSASocketW, WSAStartup, bind, closesocket,
    connect, listen, recv, send, setsockopt, shutdown,
};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, IsWellKnownSid, TOKEN_QUERY, TOKEN_USER, TokenUser,
    WinLocalSystemSid,
};
use windows_sys::Win32::System::IO::{
    CreateIoCompletionPort, GetQueuedCompletionStatusEx, OVERLAPPED, OVERLAPPED_ENTRY,
    PostQueuedCompletionStatus,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, INFINITE, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, RegisterWaitForSingleObject, TerminateProcess,
    UnregisterWaitEx, WT_EXECUTEONLYONCE,
};

/// `SIO_AF_UNIX_GETPEERPID`: `_WSAIOR(IOC_VENDOR, 256)` (afunix.h), which windows-sys doesn't define.
const SIO_AF_UNIX_GETPEERPID: u32 = 0x5800_0100;
/// The completion key of every socket's operations: which one is in its [`Op`].
const KEY_IO: usize = 0;
/// The completion key [`Port::wake`] posts.
const KEY_WAKE: usize = 1;
/// The completion key a program's end posts; its token is the entry's pointer-sized field.
const KEY_PROGRAM: usize = 2;
/// Room `AcceptEx` takes for each end's address: the address and 16 bytes (AcceptEx's documentation).
const ADDRESS_ROOM: usize = size_of::<SOCKADDR_UN>() + 16;
/// A size as Winsock's `int` lengths take it (every one here is a structure's, far below `i32::MAX`).
fn int_len(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

/// A size as Win32's `DWORD` lengths take it.
fn dword_len(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}
/// Connections waiting for an accept, as Windows bounds it (`SOMAXCONN`): the node takes them as they
/// come, one `AcceptEx` at a time.
const BACKLOG: i32 = 64;

/// Winsock, started once a process (`WSAStartup` is counted; nothing here ends it).
pub fn winsock() -> io::Result<()> {
    static STARTED: OnceLock<i32> = OnceLock::new();
    let r = *STARTED.get_or_init(|| {
        // SAFETY: a WSADATA for the call to fill, alive for it; 2.2 is the version every Windows has.
        let mut data: WSADATA = unsafe { zeroed() };
        // SAFETY: as above.
        unsafe { WSAStartup(0x0202, &raw mut data) }
    });
    if r == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(r))
    }
}

fn wsa_error() -> io::Error {
    // SAFETY: reads this thread's last Winsock error; no arguments.
    io::Error::from_raw_os_error(unsafe { WSAGetLastError() })
}

fn last_error() -> io::Error {
    // SAFETY: reads this thread's last error; no arguments.
    io::Error::from_raw_os_error(unsafe { GetLastError() }.cast_signed())
}

/// Whether a socket call failed for want of sockets or memory: the agent is spent, and accepting waits.
pub fn is_spent(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(WSAEMFILE | WSAENOBUFS))
}

/// Whether a socket call failed because its peer closed with data unread (a reset).
pub fn is_reset(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(WSAECONNRESET | WSAECONNABORTED))
}

fn address(path: &Path) -> io::Result<SOCKADDR_UN> {
    let text = path.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "a socket path that isn't UTF-8",
        )
    })?;
    let mut addr = SOCKADDR_UN {
        sun_family: AF_UNIX,
        sun_path: [0; 108],
    };
    // afunix.sys takes the path as bytes, NUL-terminated within `sun_path`.
    if text.len() >= addr.sun_path.len() || text.contains('\0') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "a socket path of {} bytes, past sockaddr_un's {}",
                text.len(),
                addr.sun_path.len().saturating_sub(1)
            ),
        ));
    }
    for (d, b) in addr.sun_path.iter_mut().zip(text.bytes()) {
        *d = i8::from_ne_bytes([b]);
    }
    Ok(addr)
}

/// An AF_UNIX stream socket, closed as it drops.
#[derive(Debug)]
pub struct Socket(SOCKET);

impl Drop for Socket {
    fn drop(&mut self) {
        // SAFETY: a socket this value owns, closed once; its pending operations end, and their
        // completions bring their own `Op`s back to the port.
        unsafe { closesocket(self.0) };
    }
}

impl Socket {
    /// A new socket: overlapped (so it can be on a port), never inherited by a child.
    fn new() -> io::Result<Socket> {
        winsock()?;
        // SAFETY: no protocol info; the flags are documented ones. The socket returned is owned here.
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
        if s == INVALID_SOCKET {
            return Err(wsa_error());
        }
        Ok(Socket(s))
    }

    /// A listener bound at `path` (a file afunix.sys makes; one there already is refused).
    pub fn listen(path: &Path) -> io::Result<Socket> {
        let addr = address(path)?;
        let s = Socket::new()?;
        // SAFETY: an address of its own length, alive for the call.
        if unsafe {
            bind(
                s.0,
                (&raw const addr).cast(),
                int_len(size_of::<SOCKADDR_UN>()),
            )
        } != 0
        {
            return Err(wsa_error());
        }
        // SAFETY: a bound socket this value owns.
        if unsafe { listen(s.0, BACKLOG) } != 0 {
            return Err(wsa_error());
        }
        Ok(s)
    }

    /// A blocking connection to the listener at `path` (afunix.sys connects at once, or refuses).
    pub fn connect(path: &Path) -> io::Result<Socket> {
        let addr = address(path)?;
        let s = Socket::new()?;
        // SAFETY: an address of its own length, alive for the call.
        if unsafe {
            connect(
                s.0,
                (&raw const addr).cast(),
                int_len(size_of::<SOCKADDR_UN>()),
            )
        } != 0
        {
            return Err(wsa_error());
        }
        Ok(s)
    }

    /// Bounds each blocking send by `patience`: past it, the send fails rather than waits on.
    pub fn set_send_patience(&self, patience: Duration) -> io::Result<()> {
        let ms = u32::try_from(patience.as_millis())
            .unwrap_or(u32::MAX)
            .max(1);
        // SAFETY: a u32 of milliseconds, its own length, as SO_SNDTIMEO takes.
        let rc = unsafe {
            setsockopt(
                self.0,
                SOL_SOCKET,
                SO_SNDTIMEO,
                (&raw const ms).cast(),
                int_len(size_of::<u32>()),
            )
        };
        if rc != 0 {
            return Err(wsa_error());
        }
        Ok(())
    }

    /// Sends what it can of `bytes`, blocking until some goes or its patience ends.
    pub fn send(&self, bytes: &[u8]) -> io::Result<usize> {
        let len = i32::try_from(bytes.len()).unwrap_or(i32::MAX);
        // SAFETY: `len` bytes of `bytes`, alive for the call.
        let n = unsafe { send(self.0, bytes.as_ptr(), len, 0) };
        usize::try_from(n).map_err(|_| wsa_error())
    }

    /// Receives into `buf`, blocking until some comes or the peer ends (0).
    pub fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        let len = i32::try_from(buf.len()).unwrap_or(i32::MAX);
        // SAFETY: `len` writable bytes of `buf`, alive for the call.
        let n = unsafe { recv(self.0, buf.as_mut_ptr(), len, 0) };
        usize::try_from(n).map_err(|_| wsa_error())
    }

    /// Shuts the socket both ways: its peer sees the end, and its own pending receive completes.
    pub fn shutdown(&self) {
        // SAFETY: a socket this value owns.
        unsafe { shutdown(self.0, SD_BOTH) };
    }

    /// The pid of the process that connected (afunix.sys records it at the connect).
    pub fn peer_pid(&self) -> io::Result<u32> {
        let mut pid = 0u32;
        let mut wrote = 0u32;
        // SAFETY: no input; `pid`, 4 bytes, is the output, alive for the call; not overlapped.
        let rc = unsafe {
            WSAIoctl(
                self.0,
                SIO_AF_UNIX_GETPEERPID,
                null(),
                0,
                (&raw mut pid).cast(),
                dword_len(size_of::<u32>()),
                &raw mut wrote,
                null_mut(),
                None,
            )
        };
        if rc != 0 {
            return Err(wsa_error());
        }
        Ok(pid)
    }
}

/// What an [`Op`] is doing.
enum Kind {
    /// An accept into `socket`, its addresses written into the op's buffer.
    Accept {
        socket: Socket,
        listener: SOCKET,
    },
    Recv,
    Send,
}

/// One overlapped operation: its `OVERLAPPED` first (so the kernel's pointer to it is a pointer to the
/// op), its buffer, and what it is for. The kernel's from its start until its completion is dequeued.
#[repr(C)]
struct Op {
    overlapped: OVERLAPPED,
    tag: u64,
    buf: Vec<u8>,
    kind: Kind,
}

impl Op {
    fn new(tag: u64, buf: Vec<u8>, kind: Kind) -> Box<Op> {
        Box::new(Op {
            // SAFETY: an all-zero OVERLAPPED is the documented start of an operation.
            overlapped: unsafe { zeroed() },
            tag,
            buf,
            kind,
        })
    }
}

/// A finished operation, or a wake or program's end, as the port reports it.
pub enum Done {
    /// [`Port::wake`] was called.
    Wake,
    /// The watched program `tag` ended.
    Program(u64),
    /// An accept under `tag` finished: the connection, or why not.
    Accepted { tag: u64, conn: io::Result<Socket> },
    /// A receive under `tag` finished: its buffer, the first `n` bytes read (0: the peer's end).
    Received {
        tag: u64,
        buf: Vec<u8>,
        n: io::Result<usize>,
    },
    /// A send under `tag` finished: its buffer, the first `n` bytes sent.
    Sent {
        tag: u64,
        buf: Vec<u8>,
        n: io::Result<usize>,
    },
}

struct PortHandle(HANDLE);

// SAFETY: a completion port's handle may be used from any thread (it is the documented way threads hand
// each other work), and this value only closes it, once, when the last holder drops.
unsafe impl Send for PortHandle {}
// SAFETY: as above: every call on it is thread-safe in Windows.
unsafe impl Sync for PortHandle {}

impl Drop for PortHandle {
    fn drop(&mut self) {
        // SAFETY: the port this value made, closed once.
        unsafe { CloseHandle(self.0) };
    }
}

/// One I/O completion port, shared by what posts to it (a program watch's callback holds it too).
#[derive(Clone)]
pub struct Port(Arc<PortHandle>);

impl Port {
    pub fn new() -> io::Result<Port> {
        // SAFETY: INVALID_HANDLE_VALUE and no port ask for a new port; one thread runs its completions.
        let h = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, null_mut(), 0, 1) };
        if h.is_null() {
            return Err(last_error());
        }
        Ok(Port(Arc::new(PortHandle(h))))
    }

    /// Puts `socket`'s operations on this port.
    pub fn add(&self, socket: &Socket) -> io::Result<()> {
        // SAFETY: an overlapped socket `socket` owns, and this port; a socket joins one port, once.
        let h = unsafe { CreateIoCompletionPort(socket.0 as HANDLE, self.0.0, KEY_IO, 0) };
        if h.is_null() {
            return Err(last_error());
        }
        Ok(())
    }

    /// Makes the next (or current) [`Port::wait`] return [`Done::Wake`].
    pub fn wake(&self) -> io::Result<()> {
        // SAFETY: this port; a post carries no OVERLAPPED, so nothing is the kernel's.
        if unsafe { PostQueuedCompletionStatus(self.0.0, 0, KEY_WAKE, null()) } == 0 {
            return Err(last_error());
        }
        Ok(())
    }

    /// Waits at most `timeout` (`None`: until one comes) for completions, appending at most `max` to
    /// `done`. A timeout appends none.
    pub fn wait(
        &self,
        done: &mut Vec<Done>,
        timeout: Option<Duration>,
        max: usize,
    ) -> io::Result<()> {
        let room = max.clamp(1, 256);
        let mut entries: Vec<OVERLAPPED_ENTRY> = Vec::with_capacity(room);
        let ms = match timeout {
            None => INFINITE,
            Some(t) => u32::try_from(t.as_millis())
                .unwrap_or(INFINITE.saturating_sub(1))
                .min(INFINITE.saturating_sub(1)),
        };
        let mut got = 0u32;
        let ask = u32::try_from(room).unwrap_or(1);
        // SAFETY: room for `ask` entries, which the call writes the first `got` of; not alertable.
        let ok = unsafe {
            GetQueuedCompletionStatusEx(
                self.0.0,
                entries.as_mut_ptr(),
                ask,
                &raw mut got,
                ms,
                FALSE,
            )
        };
        if ok == 0 {
            let e = last_error();
            return match e.raw_os_error() {
                Some(c) if c == WAIT_TIMEOUT.cast_signed() => Ok(()),
                _ => Err(e),
            };
        }
        let got = usize::try_from(got).unwrap_or(0).min(room);
        // SAFETY: the call initialised the first `got` entries, within the capacity.
        unsafe { entries.set_len(got) };
        for e in entries {
            done.push(match e.lpCompletionKey {
                KEY_WAKE => Done::Wake,
                KEY_PROGRAM => Done::Program(e.lpOverlapped as usize as u64),
                _ => {
                    // SAFETY: on KEY_IO every entry's pointer is an `Op` a start handed the kernel by
                    // `Box::into_raw`, its OVERLAPPED its first field; its completion comes once, so it
                    // is taken back once, here.
                    let op = unsafe { Box::from_raw(e.lpOverlapped.cast::<Op>()) };
                    finished(*op, e.dwNumberOfBytesTransferred)
                }
            });
        }
        Ok(())
    }
}

/// What a dequeued operation did: its status is the OVERLAPPED's (`Internal`, an NTSTATUS), read through
/// the socket's own result call where it failed.
fn finished(op: Op, bytes: u32) -> Done {
    let failed = op.overlapped.Internal != 0;
    let n = if failed {
        Err(
            match u32::try_from(op.overlapped.Internal).unwrap_or(u32::MAX) {
                // STATUS_CANCELLED: its socket closed under it.
                0xC000_0120 => io::Error::from_raw_os_error(ERROR_OPERATION_ABORTED.cast_signed()),
                // STATUS_CONNECTION_RESET, STATUS_CONNECTION_ABORTED, STATUS_LOCAL_DISCONNECT,
                // STATUS_REMOTE_DISCONNECT: its peer reset it.
                0xC000_020D | 0xC000_0241 | 0xC000_013B | 0xC000_013C => {
                    io::Error::from_raw_os_error(WSAECONNRESET)
                }
                other => io::Error::other(format!(
                    "an overlapped socket call ended NTSTATUS {other:#x}"
                )),
            },
        )
    } else {
        Ok(usize::try_from(bytes).unwrap_or(0))
    };
    let Op { tag, buf, kind, .. } = op;
    match kind {
        Kind::Accept { socket, listener } => Done::Accepted {
            tag,
            conn: n.and_then(|_| {
                // The accepted socket takes the listener's properties, so its peer and shutdown work.
                // SAFETY: the listener's value, its own length, as SO_UPDATE_ACCEPT_CONTEXT takes.
                let rc = unsafe {
                    setsockopt(
                        socket.0,
                        SOL_SOCKET,
                        SO_UPDATE_ACCEPT_CONTEXT,
                        (&raw const listener).cast(),
                        int_len(size_of::<SOCKET>()),
                    )
                };
                if rc == SOCKET_ERROR {
                    return Err(wsa_error());
                }
                Ok(socket)
            }),
        },
        Kind::Recv => Done::Received { tag, buf, n },
        Kind::Send => Done::Sent { tag, buf, n },
    }
}

/// Whether a start's failure means it is under way (its completion will come) or done with no completion.
fn started(rc: i32) -> io::Result<()> {
    if rc == 0 {
        return Ok(());
    }
    let e = wsa_error();
    if e.raw_os_error() == Some(WSA_IO_PENDING) {
        Ok(())
    } else {
        Err(e)
    }
}

/// Starts accepting one connection on `listener`, reported on `port` as [`Done::Accepted`] under `tag`.
/// The connection's socket is on `port` already (a socket joins a port once: a second join is refused).
pub fn accept(port: &Port, listener: &Socket, tag: u64) -> io::Result<()> {
    let socket = Socket::new()?;
    port.add(&socket)?;
    let into = socket.0;
    let room = ADDRESS_ROOM.saturating_mul(2);
    let op = Box::into_raw(Op::new(
        tag,
        vec![0u8; room],
        Kind::Accept {
            socket,
            listener: listener.0,
        },
    ));
    let mut got = 0u32;
    // SAFETY: `op` is a live Box the kernel holds until its completion: its OVERLAPPED and its buffer of
    // both addresses' room (no data asked for) stay put, as the op is not touched again until the port
    // returns it. The accepted socket is the op's, unassociated with any connection yet.
    let ok: windows_sys::core::BOOL = unsafe {
        AcceptEx(
            listener.0,
            into,
            (*op).buf.as_mut_ptr().cast(),
            0,
            dword_len(ADDRESS_ROOM),
            dword_len(ADDRESS_ROOM),
            &raw mut got,
            &raw mut (*op).overlapped,
        )
    };
    let r = if ok != 0 {
        Ok(())
    } else {
        started(SOCKET_ERROR)
    };
    if r.is_err() {
        // SAFETY: the start failed, so no completion is queued: the op is this function's again, once.
        drop(unsafe { Box::from_raw(op) });
    }
    r
}

/// Starts a receive of up to `buf.len()` bytes on `socket`, reported as [`Done::Received`] under `tag`.
/// A refused start gives the buffer back.
pub fn receive(socket: &Socket, tag: u64, buf: Vec<u8>) -> Result<(), (io::Error, Vec<u8>)> {
    let op = Box::into_raw(Op::new(tag, buf, Kind::Recv));
    let mut flags = 0u32;
    // SAFETY: `op` is live until its completion (see `accept`); the WSABUF names its own buffer.
    let mut wsabuf = unsafe {
        WSABUF {
            len: u32::try_from((*op).buf.len()).unwrap_or(u32::MAX),
            buf: (*op).buf.as_mut_ptr(),
        }
    };
    // SAFETY: as above: the buffer and OVERLAPPED are the kernel's until the completion, the WSABUF array
    // is read during the call only, and no completion routine is given.
    let rc = unsafe {
        WSARecv(
            socket.0,
            &raw mut wsabuf,
            1,
            null_mut(),
            &raw mut flags,
            &raw mut (*op).overlapped,
            None,
        )
    };
    started(rc).map_err(|e| {
        // SAFETY: the start failed, so no completion is queued: the op is this function's again, once.
        let op = unsafe { Box::from_raw(op) };
        (e, op.buf)
    })
}

/// Starts a send of all of `bytes` on `socket`, reported as [`Done::Sent`] under `tag`. A refused start
/// gives the bytes back.
pub fn send_all(socket: &Socket, tag: u64, bytes: Vec<u8>) -> Result<(), (io::Error, Vec<u8>)> {
    let op = Box::into_raw(Op::new(tag, bytes, Kind::Send));
    // SAFETY: `op` is live until its completion (see `accept`); the WSABUF names its own bytes.
    let mut wsabuf = unsafe {
        WSABUF {
            len: u32::try_from((*op).buf.len()).unwrap_or(u32::MAX),
            buf: (*op).buf.as_mut_ptr(),
        }
    };
    // SAFETY: as in `receive`.
    let rc = unsafe {
        WSASend(
            socket.0,
            &raw mut wsabuf,
            1,
            null_mut(),
            0,
            &raw mut (*op).overlapped,
            None,
        )
    };
    started(rc).map_err(|e| {
        // SAFETY: the start failed, so no completion is queued: the op is this function's again, once.
        let op = unsafe { Box::from_raw(op) };
        (e, op.buf)
    })
}

/// What a program watch's callback needs: the port it posts to, and the token it posts.
struct Waiting {
    port: Port,
    tag: usize,
}

/// A watched program: its process handle (which names it, not its pid, so a kill can't reach a process
/// given the pid since) and the registered wait that posts its end.
pub struct ProgramWatch {
    process: OwnedHandle,
    wait: HANDLE,
    waiting: *mut Waiting,
}

// SAFETY: the wait handle and the callback's context are this value's alone; the context is read only by
// the callback, which `Drop` waits out before freeing it, and the process handle may be used anywhere.
unsafe impl Send for ProgramWatch {}
// SAFETY: as above; `kill` only reads the process handle.
unsafe impl Sync for ProgramWatch {}

/// Called once on a thread-pool thread when the watched process ends.
unsafe extern "system" fn ended(context: *mut c_void, _timed_out: bool) {
    // SAFETY: `context` is the `Waiting` its `ProgramWatch` made, alive until that watch's drop has waited
    // this callback out (UnregisterWaitEx with INVALID_HANDLE_VALUE).
    let waiting = unsafe { &*context.cast::<Waiting>() };
    // SAFETY: the port is alive (the context holds it); the "pointer" posted is the token, which the
    // port's KEY_PROGRAM tells its wait never to take as an `Op`.
    unsafe {
        PostQueuedCompletionStatus(
            waiting.port.0.0,
            0,
            KEY_PROGRAM,
            waiting.tag as *const OVERLAPPED,
        )
    };
}

/// Watches the program `pid` until it ends, reported on `port` as [`Done::Program`] under `tag`. One that
/// ended already is reported at once.
pub fn watch_program(port: &Port, pid: u32, tag: u64) -> io::Result<ProgramWatch> {
    let tag = usize::try_from(tag).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "a token past a pointer's size")
    })?;
    // SAFETY: opens the process `pid` names now for waiting on, ending it, and reading its user; a process
    // stays (as an object) while a handle is open, so this handle names it until it is closed.
    let h = unsafe {
        OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            FALSE,
            pid,
        )
    };
    if h.is_null() {
        return Err(last_error());
    }
    // SAFETY: a handle just opened, owned from here.
    let process = unsafe { OwnedHandle::from_raw_handle(h) };
    let waiting = Box::into_raw(Box::new(Waiting {
        port: port.clone(),
        tag,
    }));
    let mut wait: HANDLE = null_mut();
    // SAFETY: the process handle is alive while the wait is registered (both are this watch's; `Drop`
    // unregisters first); `ended` is called once (WT_EXECUTEONLYONCE) with `waiting`, which lives until
    // the wait is unregistered and its callback done.
    let ok = unsafe {
        RegisterWaitForSingleObject(
            &raw mut wait,
            process.as_raw_handle(),
            Some(ended),
            waiting.cast_const().cast(),
            INFINITE,
            WT_EXECUTEONLYONCE,
        )
    };
    if ok == 0 {
        let e = last_error();
        // SAFETY: no wait was registered, so nothing else holds `waiting`.
        drop(unsafe { Box::from_raw(waiting) });
        return Err(e);
    }
    Ok(ProgramWatch {
        process,
        wait,
        waiting,
    })
}

impl ProgramWatch {
    /// Ends the watched program (its exit code 1, as `TerminateJobObject` ends a box's); one that has
    /// ended already is left as it is.
    pub fn kill(&self) {
        // SAFETY: the process handle this watch owns, opened with PROCESS_TERMINATE; it names the program
        // watched and no other.
        unsafe { TerminateProcess(self.process.as_raw_handle(), 1) };
    }
}

impl Drop for ProgramWatch {
    fn drop(&mut self) {
        // SAFETY: the wait this watch registered, unregistered once; INVALID_HANDLE_VALUE makes the call
        // wait until a callback under way has returned, so `waiting` is no one's after it.
        unsafe { UnregisterWaitEx(self.wait, INVALID_HANDLE_VALUE) };
        // SAFETY: made by `Box::into_raw` in `watch_program`, freed once, after the wait's last callback.
        drop(unsafe { Box::from_raw(self.waiting) });
    }
}

/// A token's user, as `TOKEN_USER` and the SID after it, in a buffer of its own.
fn token_user(process: HANDLE) -> io::Result<Vec<u64>> {
    let mut token: HANDLE = null_mut();
    // SAFETY: a process handle opened with PROCESS_QUERY_LIMITED_INFORMATION (or this process's own); the
    // token handle written is owned below.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut token) } == 0 {
        return Err(last_error());
    }
    // SAFETY: the token just opened, owned from here.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut need = 0u32;
    // SAFETY: a size query: no buffer, the size needed written to `need`.
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            null_mut(),
            0,
            &raw mut need,
        )
    };
    let words = usize::try_from(need)
        .unwrap_or(0)
        .div_ceil(size_of::<u64>())
        .max(size_of::<TOKEN_USER>().div_ceil(size_of::<u64>()));
    // u64s, so the TOKEN_USER at its start is aligned.
    let mut buf = vec![0u64; words];
    let room = u32::try_from(words.saturating_mul(size_of::<u64>())).unwrap_or(u32::MAX);
    // SAFETY: `room` writable bytes of `buf`, aligned for TOKEN_USER.
    let ok = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buf.as_mut_ptr().cast(),
            room,
            &raw mut need,
        )
    };
    if ok == 0 {
        return Err(last_error());
    }
    Ok(buf)
}

/// Whether the process `pid` runs as this process's user or as LocalSystem (the node's peers: this user,
/// or the machine's own account, as Linux's are this user or root).
pub fn same_user_or_system(pid: u32) -> io::Result<bool> {
    // SAFETY: opens `pid` to read its token; owned below.
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid) };
    if h.is_null() {
        return Err(last_error());
    }
    // SAFETY: a handle just opened, owned from here.
    let theirs = unsafe { OwnedHandle::from_raw_handle(h) };
    let theirs = token_user(theirs.as_raw_handle())?;
    // SAFETY: the pseudo-handle of this process, which needs no closing.
    let mine = token_user(unsafe { GetCurrentProcess() })?;
    // SAFETY: each buffer holds a TOKEN_USER at its (u64-aligned) start, whose SID points within it, both
    // written by GetTokenInformation above and alive here.
    let (a, b) = unsafe {
        (
            (*theirs.as_ptr().cast::<TOKEN_USER>()).User.Sid,
            (*mine.as_ptr().cast::<TOKEN_USER>()).User.Sid,
        )
    };
    // SAFETY: two valid SIDs, as above.
    Ok(unsafe { EqualSid(a, b) } != 0 || unsafe { IsWellKnownSid(a, WinLocalSystemSid) } != 0)
}
