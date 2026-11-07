//! macOS's event sources and local socket for the node's socket core (native plan step 8b), in safe calls
//! only (no new `unsafe`, CLAUDE.md §12):
//! - the socket is AF_UNIX `SOCK_STREAM`, as macOS has no `SOCK_SEQPACKET` local socket (it refuses one,
//!   EPROTONOSUPPORT, measured on studio-3, macOS 26.2, 2026-11-07), so each message is framed
//!   ([`hex_core::stream`]) and the core gets them whole;
//! - an exec's stdio comes as descriptors (`SCM_RIGHTS`) with its message's first byte. XNU's stream
//!   receive stops before a record that carries control data once it has copied any (measured on studio-3:
//!   two framed messages each with descriptors, then one without, came as three reads' worth, the
//!   descriptors with the read that began at their message), so a read that brings descriptors begins at
//!   the message they belong to; descriptors that come while a frame is under way are misplaced, and that
//!   message is refused;
//! - a connection is watched by poll(2), level-triggered as the core asks; a program's end by kqueue's
//!   `EVFILT_PROC` (`hex_runtime::macos::sys::Kqueue`, its safe wrapper) on one thread, which wakes the poll
//!   through a pipe; [`Os::wake`] writes the same pipe;
//! - a message the stream takes only part of is finished here when the socket has room again, and until
//!   then the core's next sends would block, so to the core every send is whole or nothing;
//! - only this user or root can connect: the socket's file is this user's, mode 0600, and XNU checks write
//!   permission on it for a connect (`bsd/kern/uipc_usrreq.c`, `unp_connect`: `vnode_authorize` with
//!   `KAUTH_VNODE_WRITE_DATA`), the rule Linux's peer check applies.

use std::collections::{BTreeMap, VecDeque};
use std::io::IoSliceMut;
use std::mem::MaybeUninit;
use std::os::fd::OwnedFd;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use hex_api::{Refusal, Stdio};
use hex_core::stream::{self, Deframer};
use hex_core::wire::{self, Streams};
use hex_runtime::macos::sys::{self, Kqueue};
use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::fs::OFlags;
use rustix::io::{Errno, FdFlags};
use rustix::net::{
    AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags, SendFlags,
    Shutdown, SocketAddrUnix, SocketType,
};
use rustix::process::{Pid, Signal};

use super::{Accepted, EVENTS, Event, Interest, Os, Peer, Recv, Sent, Unwatched};

/// Connections waiting to be accepted: the I/O thread accepts them as they come.
const BACKLOG: i32 = 64;
/// A frame's prefix, its body's length (`hex_core::stream`'s `u32`).
const PREFIX: usize = size_of::<u32>();

/// One connection: its socket, and the framing between its bytes and the core's messages.
pub struct Stream {
    fd: OwnedFd,
    framing: Mutex<Framing>,
}

struct Framing {
    deframer: Deframer,
    /// Whole messages read and not yet taken by the core, each with what came with it.
    ready: VecDeque<Message>,
    /// Descriptors that came at a message's first byte, for that message.
    passed: Vec<OwnedFd>,
    /// Descriptors came while a frame was under way, or more than a message takes.
    cut: bool,
    /// The rest of a message the stream took only part of.
    unsent: Vec<u8>,
    /// The stream can't be read further (an oversized frame): the next read is its end.
    broken: bool,
}

struct Message {
    bytes: Vec<u8>,
    /// Its declared size, when it was past the bound and not kept.
    size: usize,
    passed: Vec<OwnedFd>,
    cut: bool,
}

/// What the I/O thread watches beside the sockets.
#[derive(Default)]
struct Watching {
    accepting: bool,
    listening: bool,
    conns: BTreeMap<u64, (Arc<Stream>, Interest)>,
}

/// What the kqueue thread and [`Os::wake`] leave for the next wait.
#[derive(Default)]
struct Woken {
    wake: bool,
    /// Tokens of watched programs that have ended.
    ended: VecDeque<u64>,
}

/// The listener, the wake pipe, the kqueue that watches programs and its thread.
pub struct MacOs {
    listener: OwnedFd,
    wake_r: OwnedFd,
    wake_w: Arc<OwnedFd>,
    watching: Mutex<Watching>,
    woken: Arc<Mutex<Woken>>,
    kq: Arc<Kqueue>,
    stopping: Arc<AtomicBool>,
    procs: Mutex<Option<JoinHandle<()>>>,
}

fn poisoned<T>(_: std::sync::PoisonError<T>) -> Refusal {
    Refusal::Broken("a socket lock was poisoned".to_owned())
}

/// Descriptors made close-on-exec and non-blocking: macOS's `socket`, `accept` and `pipe` take no flags.
fn own(fd: &OwnedFd, what: &'static str) -> Result<(), Refusal> {
    rustix::io::fcntl_setfd(fd, FdFlags::CLOEXEC).map_err(|e| Refusal::os(what, e))?;
    let flags = rustix::fs::fcntl_getfl(fd).map_err(|e| Refusal::os(what, e))?;
    rustix::fs::fcntl_setfl(fd, flags | OFlags::NONBLOCK).map_err(|e| Refusal::os(what, e))
}

impl MacOs {
    /// A socket at `path`, readable and writable by this user alone, and the thread that watches
    /// programs' ends. A socket left at `path` by an agent that has ended is replaced; one an agent
    /// still serves, or a file that isn't a socket, is refused.
    pub fn bind(path: &Path) -> Result<Self, Refusal> {
        super::unix::clear(path, SocketType::STREAM)?;
        let addr = SocketAddrUnix::new(path).map_err(|e| Refusal::Invalid {
            what: "socket path",
            why: format!("{}: {e}", path.display()),
        })?;
        let listener = rustix::net::socket(AddressFamily::UNIX, SocketType::STREAM, None)
            .map_err(|e| Refusal::os("making the node agent's socket", e))?;
        own(&listener, "making the node agent's socket")?;
        rustix::net::bind(&listener, &addr).map_err(|e| Refusal::Os {
            op: "binding the node agent's socket",
            why: format!("{}: {e}", path.display()),
        })?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| Refusal::os("restricting the node agent's socket to its user", e))?;
        rustix::net::listen(&listener, BACKLOG)
            .map_err(|e| Refusal::os("listening on the node agent's socket", e))?;
        let (wake_r, wake_w) =
            rustix::pipe::pipe().map_err(|e| Refusal::os("making the socket's wake pipe", e))?;
        own(&wake_r, "making the socket's wake pipe")?;
        own(&wake_w, "making the socket's wake pipe")?;
        let kq = Arc::new(Kqueue::new().map_err(|e| Refusal::os("making the socket's kqueue", e))?);
        let os = Self {
            listener,
            wake_r,
            wake_w: Arc::new(wake_w),
            watching: Mutex::new(Watching {
                listening: true,
                ..Watching::default()
            }),
            woken: Arc::new(Mutex::new(Woken::default())),
            kq,
            stopping: Arc::new(AtomicBool::new(false)),
            procs: Mutex::new(None),
        };
        let thread = {
            let (kq, woken, wake_w, stopping) = (
                Arc::clone(&os.kq),
                Arc::clone(&os.woken),
                Arc::clone(&os.wake_w),
                Arc::clone(&os.stopping),
            );
            std::thread::Builder::new()
                .name("hex-socket-procs".to_owned())
                .spawn(move || watch_ends(&kq, &woken, &wake_w, &stopping))
                .map_err(|e| Refusal::os("starting the socket's program watch", e))?
        };
        *os.procs.lock().map_err(poisoned)? = Some(thread);
        Ok(os)
    }

    fn watching(&self) -> Result<MutexGuard<'_, Watching>, Refusal> {
        self.watching.lock().map_err(poisoned)
    }
}

/// The kqueue thread: each watched program's end goes to the next wait, which the pipe wakes.
fn watch_ends(kq: &Kqueue, woken: &Mutex<Woken>, wake_w: &OwnedFd, stopping: &AtomicBool) {
    let mut events = Vec::with_capacity(sys::EVENTS);
    while !stopping.load(Ordering::SeqCst) {
        if kq.wait(None, None, &mut events).is_err() {
            return;
        }
        let ended: Vec<u64> = events
            .iter()
            .filter_map(|e| match e {
                sys::Event::Exit { tag, .. } => Some(*tag),
                _ => None,
            })
            .collect();
        if ended.is_empty() {
            continue;
        }
        let Ok(mut w) = woken.lock() else {
            return;
        };
        w.ended.extend(ended);
        drop(w);
        let _ = rustix::io::write(wake_w, &[1]);
    }
}

impl Drop for MacOs {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        let _ = self.kq.trigger();
        if let Some(t) = self.procs.get_mut().ok().and_then(Option::take) {
            let _ = t.join();
        }
    }
}

/// A watched program: its pid, and when it started, which with the pid names it.
pub struct Watch {
    pid: i32,
    started: u64,
}

impl Os for MacOs {
    type Conn = Arc<Stream>;
    type Watch = Watch;
    type Passed = OwnedFd;

    fn wait(&self, events: &mut Vec<Event>, timeout: Option<Duration>) -> Result<(), Refusal> {
        let (listening, conns): (bool, Vec<(u64, Arc<Stream>, Interest)>) = {
            let w = self.watching()?;
            (
                w.listening && w.accepting,
                w.conns
                    .iter()
                    .map(|(t, (s, i))| (*t, Arc::clone(s), *i))
                    .collect(),
            )
        };
        // What is ready without a poll: messages read whole and not taken, a wake, programs' ends.
        let now = self.ready_now(&conns, events)?;
        let mut fds = Vec::with_capacity(conns.len().saturating_add(2));
        fds.push(PollFd::new(&self.wake_r, PollFlags::IN));
        if listening {
            fds.push(PollFd::new(&self.listener, PollFlags::IN));
        }
        for (_, s, interest) in &conns {
            let unsent = !s.framing.lock().map_err(poisoned)?.unsent.is_empty();
            let mut flags = match interest {
                Interest::Read => PollFlags::IN,
                Interest::Write => PollFlags::OUT,
            };
            if unsent {
                flags |= PollFlags::OUT;
            }
            fds.push(PollFd::new(&s.fd, flags));
        }
        let ts = if now {
            Some(Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            })
        } else {
            timeout.map(|left| Timespec {
                tv_sec: i64::try_from(left.as_secs()).unwrap_or(i64::MAX),
                tv_nsec: i64::from(left.subsec_nanos()),
            })
        };
        match rustix::event::poll(&mut fds, ts.as_ref()) {
            Ok(_) | Err(Errno::INTR) => {}
            Err(e) => return Err(Refusal::os("waiting on the socket's poll set", e)),
        }
        let revents: Vec<PollFlags> = fds.iter().map(PollFd::revents).collect();
        let mut at = revents.iter();
        if at.next().is_some_and(|r| r.contains(PollFlags::IN)) {
            let mut sink = [0u8; 64];
            while matches!(rustix::io::read(&self.wake_r, &mut sink), Ok(n) if n > 0) {}
            if !now {
                self.ready_now(&conns, events)?;
            }
        }
        if listening && at.next().is_some_and(|r| r.contains(PollFlags::IN)) {
            events.push(Event::Listener);
        }
        for ((token, s, interest), r) in conns.iter().zip(at) {
            let hung_up = r.intersects(PollFlags::HUP | PollFlags::ERR);
            let mut writable = false;
            if r.contains(PollFlags::OUT) {
                writable = finish_unsent(s)? && matches!(interest, Interest::Write);
            }
            let readable = matches!(interest, Interest::Read) && r.contains(PollFlags::IN);
            if (readable || writable || hung_up)
                && !events
                    .iter()
                    .any(|e| matches!(e, Event::Ready { token: t, .. } if t == token))
            {
                events.push(Event::Ready {
                    token: *token,
                    readable,
                    writable,
                    hung_up,
                });
            }
            if events.len() >= EVENTS {
                break;
            }
        }
        Ok(())
    }

    fn wake(&self) -> Result<(), Refusal> {
        self.woken.lock().map_err(poisoned)?.wake = true;
        match rustix::io::write(self.wake_w.as_ref(), &[1]) {
            Ok(_) | Err(Errno::AGAIN) => Ok(()),
            Err(e) => Err(Refusal::os("waking the socket's thread", e)),
        }
    }

    fn accept(&self) -> Accepted<Arc<Stream>> {
        loop {
            return match rustix::net::accept(&self.listener) {
                Ok(fd) => match own(&fd, "accepting on the node agent's socket").and_then(|()| {
                    rustix::net::sockopt::set_socket_nosigpipe(&fd, true)
                        .map_err(|e| Refusal::os("accepting on the node agent's socket", e))
                }) {
                    Ok(()) => Accepted::Conn(Arc::new(Stream {
                        fd,
                        framing: Mutex::new(Framing {
                            deframer: Deframer::new(wire::MAX_MESSAGE),
                            ready: VecDeque::new(),
                            passed: Vec::new(),
                            cut: false,
                            unsent: Vec::new(),
                            broken: false,
                        }),
                    })),
                    Err(e) => Accepted::Failed(e),
                },
                Err(Errno::AGAIN) => Accepted::None,
                Err(Errno::INTR | Errno::CONNABORTED) => continue,
                Err(Errno::MFILE | Errno::NFILE | Errno::NOBUFS | Errno::NOMEM) => Accepted::Spent,
                Err(e) => Accepted::Failed(Refusal::os("accepting on the node agent's socket", e)),
            };
        }
    }

    fn accepting(&self, on: bool) -> Result<(), Refusal> {
        self.watching()?.accepting = on;
        Ok(())
    }

    fn stop_listening(&self) {
        if let Ok(mut w) = self.watching.lock() {
            w.listening = false;
        }
    }

    fn peer(&self, _conn: &Arc<Stream>) -> Peer {
        // The socket's file admits only this user and root (the module's note).
        Peer::Allowed
    }

    fn watch(&self, conn: &Arc<Stream>, token: u64, interest: Interest) -> Result<(), Refusal> {
        self.watching()?
            .conns
            .insert(token, (Arc::clone(conn), interest));
        Ok(())
    }

    fn rewatch(&self, conn: &Arc<Stream>, token: u64, interest: Interest) -> Result<(), Refusal> {
        self.watch(conn, token, interest)
    }

    fn unwatch(&self, conn: &Arc<Stream>) {
        if let Ok(mut w) = self.watching.lock() {
            w.conns.retain(|_, (s, _)| !Arc::ptr_eq(s, conn));
        }
    }

    fn recv(&self, conn: &Arc<Stream>, buf: &mut [u8]) -> Recv<OwnedFd> {
        let Ok(mut f) = conn.framing.lock() else {
            return Recv::Gone;
        };
        loop {
            if let Some(m) = f.ready.pop_front() {
                let n = m.bytes.len().min(buf.len());
                if let (Some(into), Some(from)) = (buf.get_mut(..n), m.bytes.get(..n)) {
                    into.copy_from_slice(from);
                }
                return Recv::Message {
                    size: m.size,
                    got: n,
                    passed: m.passed,
                    cut: m.cut,
                };
            }
            if f.broken {
                return Recv::Gone;
            }
            match read_some(&conn.fd, &mut f) {
                Ok(true) => {}
                // Its end: a stream holds no empty message (each frame has its prefix), so a read of
                // nothing, after every whole message it sent, is its client gone.
                Ok(false) => return Recv::Gone,
                Err(Errno::AGAIN) => return Recv::WouldBlock,
                Err(Errno::CONNRESET) => return Recv::Reset,
                Err(_) => return Recv::Gone,
            }
        }
    }

    fn send(&self, conn: &Arc<Stream>, bytes: &[u8]) -> Sent {
        let Ok(mut f) = conn.framing.lock() else {
            return Sent::Gone;
        };
        if !f.unsent.is_empty() {
            return Sent::WouldBlock;
        }
        let Ok(framed) = stream::frame(bytes, wire::MAX_MESSAGE) else {
            return Sent::Gone;
        };
        loop {
            return match rustix::net::send(&conn.fd, &framed, SendFlags::DONTWAIT) {
                Ok(n) if n >= framed.len() => Sent::Sent,
                Ok(n) => {
                    // Taken in part: the rest goes when the socket has room ([`finish_unsent`]).
                    f.unsent = framed.get(n..).unwrap_or_default().to_vec();
                    Sent::Sent
                }
                Err(Errno::INTR) => continue,
                Err(Errno::AGAIN) => Sent::WouldBlock,
                Err(_) => Sent::Gone,
            };
        }
    }

    fn shutdown(&self, conn: &Arc<Stream>) {
        let _ = rustix::net::shutdown(&conn.fd, Shutdown::Both);
    }

    fn hung_up(&self, conn: &Arc<Stream>) -> bool {
        let mut fds = [PollFd::new(&conn.fd, PollFlags::HUP)];
        let zero = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        match rustix::event::poll(&mut fds, Some(&zero)) {
            Ok(_) => fds
                .first()
                .is_some_and(|f| f.revents().intersects(PollFlags::HUP | PollFlags::ERR)),
            Err(_) => true,
        }
    }

    fn watch_program(&self, pid: u32, token: u64) -> Result<Watch, Unwatched> {
        let unwatched = |why: String| Unwatched {
            why: Refusal::Os {
                op: "watching a started program",
                why: format!(
                    "{why}: it runs on unwatched by the socket, and the node's sweep reaps it"
                ),
            },
            killed: false,
        };
        let pid = i32::try_from(pid).map_err(|_| unwatched(format!("pid {pid} isn't one")))?;
        let started = match sys::rusage(pid) {
            Ok(Some(r)) => r.started,
            Ok(None) => 0,
            Err(e) => return Err(unwatched(e.to_string())),
        };
        let gone = self
            .kq
            .watch(&[pid], token)
            .map_err(|e| unwatched(e.to_string()))?;
        if gone.contains(&pid) {
            // Ended before it could be watched: its end is the next wait's.
            if let Ok(mut w) = self.woken.lock() {
                w.ended.push_back(token);
            }
            let _ = rustix::io::write(self.wake_w.as_ref(), &[1]);
        }
        Ok(Watch { pid, started })
    }

    fn kill(&self, watch: &Watch) {
        // Only the process that pid named when it was watched: one given the pid since started later.
        // An ended program not yet reaped keeps its pid and start time, and a kill of it does nothing.
        if let Ok(Some(r)) = sys::rusage(watch.pid)
            && r.started == watch.started
            && let Some(pid) = Pid::from_raw(watch.pid)
        {
            let _ = rustix::process::kill_process(pid, Signal::KILL);
        }
    }

    fn unwatch_program(&self, watch: Watch) {
        let _ = self.kq.unwatch(watch.pid);
    }

    fn stdio(&self, streams: Streams, passed: Vec<OwnedFd>) -> Result<Stdio, Refusal> {
        super::unix::stdio_of(streams, passed)
    }
}

impl MacOs {
    /// Puts in `events` what is ready without a poll: a wake, programs' ends, and connections with
    /// messages read whole and not taken. Whether there was any.
    fn ready_now(
        &self,
        conns: &[(u64, Arc<Stream>, Interest)],
        events: &mut Vec<Event>,
    ) -> Result<bool, Refusal> {
        let before = events.len();
        {
            let mut w = self.woken.lock().map_err(poisoned)?;
            if std::mem::take(&mut w.wake) {
                events.push(Event::Wake);
            }
            while events.len() < EVENTS {
                let Some(token) = w.ended.pop_front() else {
                    break;
                };
                events.push(Event::Ready {
                    token,
                    readable: true,
                    writable: false,
                    hung_up: false,
                });
            }
        }
        for (token, s, interest) in conns {
            if events.len() >= EVENTS {
                break;
            }
            let held = s.framing.lock().map_err(poisoned)?;
            if matches!(interest, Interest::Read) && (!held.ready.is_empty() || held.broken) {
                events.push(Event::Ready {
                    token: *token,
                    readable: true,
                    writable: false,
                    hung_up: false,
                });
            }
        }
        Ok(events.len() > before)
    }
}

/// Sends what the stream took only part of, as far as it takes now. Whether all of it is out.
fn finish_unsent(s: &Stream) -> Result<bool, Refusal> {
    let mut f = s.framing.lock().map_err(poisoned)?;
    while !f.unsent.is_empty() {
        match rustix::net::send(&s.fd, &f.unsent, SendFlags::DONTWAIT) {
            Ok(n) => {
                let len = f.unsent.len();
                f.unsent.drain(..n.min(len));
            }
            Err(Errno::INTR) => {}
            Err(Errno::AGAIN) => return Ok(false),
            Err(_) => {
                // Its client is gone: the hang-up closes it.
                f.unsent.clear();
            }
        }
    }
    Ok(true)
}

/// Reads what the stream has and frames it into whole messages. `Ok(false)` at its end.
fn read_some(fd: &OwnedFd, f: &mut Framing) -> Result<bool, Errno> {
    let mut bytes = vec![0u8; stream::MAX_FRAME];
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(wire::MAX_FDS))];
    let msg = loop {
        let mut control = RecvAncillaryBuffer::new(&mut space);
        match rustix::net::recvmsg(
            fd,
            &mut [IoSliceMut::new(&mut bytes)],
            &mut control,
            RecvFlags::DONTWAIT,
        ) {
            Ok(msg) => {
                let fds: Vec<OwnedFd> = control
                    .drain()
                    .filter_map(|m| match m {
                        RecvAncillaryMessage::ScmRights(fds) => Some(fds),
                        _ => None,
                    })
                    .flatten()
                    .collect();
                break (msg, fds);
            }
            Err(Errno::INTR) => continue,
            Err(e) => return Err(e),
        }
    };
    let (msg, fds) = msg;
    if msg.bytes == 0 && fds.is_empty() {
        return Ok(false);
    }
    for fd in &fds {
        // A descriptor that came is this process's now: close-on-exec, as every other.
        let _ = rustix::io::fcntl_setfd(fd, FdFlags::CLOEXEC);
    }
    if !fds.is_empty() {
        // Descriptors begin a message; ones that come while a frame is under way are misplaced.
        if f.deframer.in_frame() || !f.passed.is_empty() {
            f.cut = true;
        }
        f.passed.extend(fds);
    }
    if msg.flags.contains(ReturnFlags::CTRUNC) {
        f.cut = true;
    }
    let mut input = bytes.get(..msg.bytes).unwrap_or_default();
    while !input.is_empty() {
        let at_start = !f.deframer.in_frame();
        match f.deframer.take(input) {
            Ok((took, done)) => {
                input = input.get(took..).unwrap_or_default();
                if let Some(message) = done {
                    let passed = std::mem::take(&mut f.passed);
                    let cut = std::mem::take(&mut f.cut);
                    f.ready.push_back(Message {
                        size: message.len(),
                        bytes: message,
                        passed,
                        cut,
                    });
                }
            }
            Err(wire::Fault::Oversized { bytes, .. }) => {
                // Its size is said, with its header where this read holds it, so the core refuses it by
                // name to the request it is; the stream can't be read past it.
                let header = if at_start {
                    input
                        .get(PREFIX..)
                        .unwrap_or_default()
                        .iter()
                        .take(wire::HEADER_BYTES)
                        .copied()
                        .collect()
                } else {
                    Vec::new()
                };
                f.ready.push_back(Message {
                    bytes: header,
                    size: bytes,
                    passed: std::mem::take(&mut f.passed),
                    cut: false,
                });
                f.broken = true;
                return Ok(true);
            }
            Err(_) => {
                f.broken = true;
                return Ok(true);
            }
        }
    }
    Ok(true)
}
