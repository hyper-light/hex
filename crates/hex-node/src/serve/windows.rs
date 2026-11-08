//! Windows' event sources and local socket for the node's socket core (native plan step 8; the decision
//! "the Windows node, on one completion port" in docs/design/runtime.md), in safe calls only: the Win32
//! calls are `hex_afunix`'s (CLAUDE.md §12).
//! - the socket is AF_UNIX `SOCK_STREAM` (afunix.sys has no other kind), so each message is framed
//!   ([`hex_core::stream`]) and the core gets them whole, as on macOS;
//! - one completion port carries every source: the listener's one overlapped `AcceptEx` at a time, each
//!   connection's one overlapped receive and one overlapped send, a program's end (a wait completion packet
//!   on its process handle, which the kernel queues to the port itself), and [`Os::wake`]. The core's event set is level-triggered readiness, so
//!   a connection watched for reads is reported while a message read whole waits or its client has gone,
//!   and one watched for writes while no send of it is under way;
//! - a connection is read one buffer at a time, and only while the core reads it and no message read from
//!   it waits, so a client the core has stopped reading holds one buffer and what it decoded here, no more;
//! - a send hands the kernel the whole framed message and owns nothing after, so to the core every send is
//!   whole or nothing; while one is under way, the next is `WouldBlock`;
//! - only this user or LocalSystem is served: each peer's pid (`SIO_AF_UNIX_GETPEERPID`) names a process
//!   whose user is read and refused by name otherwise, as Linux's `SO_PEERCRED` is;
//! - an exec's stdio isn't carried yet: Windows' AF_UNIX passes no handles, and how it reaches the caller is
//!   step 8's measured decision, so an exec that asks for a stream is refused by name until then.

use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::os::windows::fs::MetadataExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use hex_afunix::{self as socket, Done, Port, ProgramWatch, Socket};
use hex_api::{Refusal, Stdio};
use hex_core::stream::{self, Deframer};
use hex_core::wire::{self, Streams};

use super::{Accepted, EVENTS, Event, Interest, Os, Peer, Recv, Sent, Unwatched};

/// A frame's prefix, its body's length (`hex_core::stream`'s `u32`).
const PREFIX: usize = size_of::<u32>();
/// A [`Stream`]'s token before the core watches it.
const UNWATCHED: u64 = u64::MAX;
/// The listener's accepts' tag on the port.
const LISTENER: u64 = 0;
/// `FILE_ATTRIBUTE_REPARSE_POINT`: afunix.sys's socket file is one (`IO_REPARSE_TAG_AF_UNIX`).
const REPARSE_POINT: u32 = 0x400;

/// What a request carries besides its bytes: nothing on Windows, whose AF_UNIX passes no handles.
pub enum NoHandle {}

/// One connection: its socket, its token once watched, and the framing between its bytes and the core's
/// messages.
pub struct Stream {
    socket: Socket,
    token: AtomicU64,
    state: Mutex<State>,
}

struct State {
    deframer: Deframer,
    /// Whole messages read and not yet taken by the core.
    ready: VecDeque<Message>,
    /// The read buffer, when no receive holds it.
    buf: Option<Vec<u8>>,
    /// A receive is under way.
    receiving: bool,
    /// A send is under way.
    sending: bool,
    /// The core watches it for its requests.
    reading: bool,
    /// Its client ended it, or it failed: nothing more is read.
    ended: bool,
    /// Its client reset it: reported once, as [`Recv::Reset`].
    reset: bool,
    /// The stream can't be read further (an oversized or bad frame): the next read is its end.
    broken: bool,
}

struct Message {
    bytes: Vec<u8>,
    /// Its declared size, when it was past the bound and not kept.
    size: usize,
}

fn poisoned<T>(_: std::sync::PoisonError<T>) -> Refusal {
    Refusal::Broken("a socket lock was poisoned".to_owned())
}

impl Stream {
    fn new(socket: Socket) -> Stream {
        Stream {
            socket,
            token: AtomicU64::new(UNWATCHED),
            state: Mutex::new(State {
                deframer: Deframer::new(wire::MAX_MESSAGE),
                ready: VecDeque::new(),
                buf: Some(vec![0u8; stream::MAX_FRAME]),
                receiving: false,
                sending: false,
                reading: false,
                ended: false,
                reset: false,
                broken: false,
            }),
        }
    }

    /// Starts the next receive if the core reads it, nothing read waits, and none is under way.
    fn read_on(&self, s: &mut State) {
        if !s.reading || s.receiving || s.ended || s.broken || !s.ready.is_empty() {
            return;
        }
        let Some(buf) = s.buf.take() else {
            return;
        };
        match socket::receive(&self.socket, self.token.load(Ordering::SeqCst), buf) {
            Ok(()) => s.receiving = true,
            Err((e, buf)) => {
                s.buf = Some(buf);
                s.reset = socket::is_reset(&e);
                s.ended = true;
            }
        }
    }
}

/// The connections the core watches, by token, and what for.
type Conns = BTreeMap<u64, (Arc<Stream>, Interest)>;

/// The listener's state: whether the core accepts, an accept under way, and what one took.
#[derive(Default)]
struct Listening {
    on: bool,
    accepting: bool,
    taken: VecDeque<io::Result<Socket>>,
}

/// The listener and the completion port that watches it, the connections and the programs.
pub struct Windows {
    listener: Socket,
    port: Port,
    listening: Mutex<Listening>,
    /// The connections the core watches, by token, and what for.
    watching: Mutex<Conns>,
}

impl Windows {
    /// A socket at `path`, and its completion port. A socket left at `path` by an agent that has ended is
    /// replaced; one an agent still serves, or a file that isn't a socket, is refused.
    pub fn bind(path: &Path) -> Result<Self, Refusal> {
        clear(path)?;
        let listener = Socket::listen(path).map_err(|e| Refusal::Os {
            op: "binding the node agent's socket",
            why: format!("{}: {e}", path.display()),
        })?;
        let port =
            Port::new().map_err(|e| Refusal::os("making the socket's completion port", e))?;
        port.add(&listener)
            .map_err(|e| Refusal::os("watching the node agent's socket", e))?;
        Ok(Self {
            listener,
            port,
            listening: Mutex::new(Listening::default()),
            watching: Mutex::new(BTreeMap::new()),
        })
    }

    fn watching(&self) -> Result<MutexGuard<'_, Conns>, Refusal> {
        self.watching.lock().map_err(poisoned)
    }

    fn listening(&self) -> Result<MutexGuard<'_, Listening>, Refusal> {
        self.listening.lock().map_err(poisoned)
    }

    /// Starts an accept if the core accepts and none is under way or waiting to be taken.
    fn accept_on(&self, l: &mut Listening) {
        if !l.on || l.accepting || !l.taken.is_empty() {
            return;
        }
        match socket::accept(&self.port, &self.listener, LISTENER) {
            Ok(()) => l.accepting = true,
            // Reported to the core by its next accept, which pauses on a spent one.
            Err(e) => l.taken.push_back(Err(e)),
        }
    }

    /// Puts in `events` what is ready without a wait. Whether there was any.
    fn ready_now(&self, conns: &Conns, events: &mut Vec<Event>) -> Result<bool, Refusal> {
        let before = events.len();
        {
            let l = self.listening()?;
            if l.on && !l.taken.is_empty() {
                events.push(Event::Listener);
            }
        }
        for (token, (s, interest)) in conns {
            if events.len() >= EVENTS {
                break;
            }
            let st = s.state.lock().map_err(poisoned)?;
            let (readable, writable) = match interest {
                Interest::Read => (!st.ready.is_empty() || st.ended || st.broken, false),
                Interest::Write => (false, !st.sending || st.ended),
            };
            if readable || writable {
                push(events, *token, readable, writable, st.ended);
            }
        }
        Ok(events.len() > before)
    }

    /// The connection watched under `tag` and what for, as the core watches it now: a worker may have
    /// changed it while the wait was under way.
    fn watched(&self, tag: u64) -> Result<Option<(Arc<Stream>, Interest)>, Refusal> {
        Ok(self.watching()?.get(&tag).map(|(s, i)| (Arc::clone(s), *i)))
    }

    /// Takes one completion into the core's events.
    fn take(&self, done: Done, events: &mut Vec<Event>) -> Result<(), Refusal> {
        match done {
            Done::Wake => {
                if !events.contains(&Event::Wake) {
                    events.push(Event::Wake);
                }
            }
            Done::Program(token) => push(events, token, true, false, false),
            Done::Accepted { conn, .. } => {
                let mut l = self.listening()?;
                l.accepting = false;
                l.taken.push_back(conn);
                if l.on && !events.contains(&Event::Listener) {
                    events.push(Event::Listener);
                }
            }
            Done::Received { tag, buf, n } => {
                let Some((conn, interest)) = self.watched(tag)? else {
                    return Ok(());
                };
                let mut st = conn.state.lock().map_err(poisoned)?;
                st.receiving = false;
                match n {
                    Ok(0) => st.ended = true,
                    Ok(n) => frame(&mut st, buf.get(..n).unwrap_or_default()),
                    Err(e) => {
                        st.reset = socket::is_reset(&e);
                        st.ended = true;
                    }
                }
                st.buf = Some(buf);
                conn.read_on(&mut st);
                if matches!(interest, Interest::Read)
                    && (!st.ready.is_empty() || st.ended || st.broken)
                {
                    push(events, tag, true, false, st.ended);
                }
            }
            Done::Sent { tag, buf, n } => {
                let Some((conn, interest)) = self.watched(tag)? else {
                    return Ok(());
                };
                let mut st = conn.state.lock().map_err(poisoned)?;
                st.sending = false;
                match n {
                    Ok(n) if n >= buf.len() => {}
                    Ok(n) => {
                        // Taken in part: the rest goes now, and the core's next send waits for it.
                        let rest = buf.get(n..).unwrap_or_default().to_vec();
                        match socket::send_all(&conn.socket, tag, rest) {
                            Ok(()) => st.sending = true,
                            Err(_) => st.ended = true,
                        }
                    }
                    Err(_) => st.ended = true,
                }
                if matches!(interest, Interest::Write) && (!st.sending || st.ended) {
                    push(events, tag, false, true, st.ended);
                }
            }
        }
        Ok(())
    }
}

/// Adds to `token`'s event in `events`, or appends one.
fn push(events: &mut Vec<Event>, token: u64, readable: bool, writable: bool, hung_up: bool) {
    let room = events.len() < EVENTS;
    match events
        .iter_mut()
        .find(|ev| matches!(ev, Event::Ready { token: t, .. } if *t == token))
    {
        Some(Event::Ready {
            readable: r,
            writable: w,
            hung_up: h,
            ..
        }) => {
            *r |= readable;
            *w |= writable;
            *h |= hung_up;
        }
        _ if room => events.push(Event::Ready {
            token,
            readable,
            writable,
            hung_up,
        }),
        _ => {}
    }
}

/// Frames what a receive read into whole messages.
fn frame(st: &mut State, mut input: &[u8]) {
    while !input.is_empty() {
        let at_start = !st.deframer.in_frame();
        match st.deframer.take(input) {
            Ok((took, done)) => {
                input = input.get(took..).unwrap_or_default();
                if let Some(message) = done {
                    st.ready.push_back(Message {
                        size: message.len(),
                        bytes: message,
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
                st.ready.push_back(Message {
                    bytes: header,
                    size: bytes,
                });
                st.broken = true;
                return;
            }
            Err(_) => {
                st.broken = true;
                return;
            }
        }
    }
}

/// Clears `path` for a new socket: nothing there, or a socket no agent serves (removed). A socket an agent
/// still serves, or a file that isn't a socket, is refused.
fn clear(path: &Path) -> Result<(), Refusal> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(Refusal::Os {
                op: "checking the node agent's socket path",
                why: format!("{}: {e}", path.display()),
            });
        }
    };
    if meta.file_attributes() & REPARSE_POINT == 0 {
        return Err(Refusal::Invalid {
            what: "socket path",
            why: format!("{}: a file that isn't a socket is there", path.display()),
        });
    }
    if Socket::connect(path).is_ok() {
        return Err(Refusal::Invalid {
            what: "socket path",
            why: format!("{}: a node agent serves it already", path.display()),
        });
    }
    std::fs::remove_file(path).map_err(|e| Refusal::Os {
        op: "removing a socket no agent serves",
        why: format!("{}: {e}", path.display()),
    })
}

/// A watched program: the watch on its process handle, which its end is seen through and it is killed
/// through.
pub struct Watch(ProgramWatch);

impl Os for Windows {
    type Conn = Arc<Stream>;
    type Watch = Watch;
    type Passed = NoHandle;

    fn wait(&self, events: &mut Vec<Event>, timeout: Option<Duration>) -> Result<(), Refusal> {
        let conns: Conns = self
            .watching()?
            .iter()
            .map(|(t, (s, i))| (*t, (Arc::clone(s), *i)))
            .collect();
        let now = self.ready_now(&conns, events)?;
        let mut done = Vec::new();
        let room = EVENTS.saturating_sub(events.len()).max(1);
        self.port
            .wait(
                &mut done,
                if now { Some(Duration::ZERO) } else { timeout },
                room,
            )
            .map_err(|e| Refusal::os("waiting on the socket's completion port", e))?;
        for d in done {
            self.take(d, events)?;
        }
        Ok(())
    }

    fn wake(&self) -> Result<(), Refusal> {
        self.port
            .wake()
            .map_err(|e| Refusal::os("waking the socket's thread", e))
    }

    fn accept(&self) -> Accepted<Arc<Stream>> {
        let Ok(mut l) = self.listening.lock() else {
            return Accepted::Failed(Refusal::Broken("a socket lock was poisoned".to_owned()));
        };
        let taken = l.taken.pop_front();
        self.accept_on(&mut l);
        match taken {
            None => Accepted::None,
            // On the port already: an accept's socket joins it before the accept starts.
            Some(Ok(s)) => Accepted::Conn(Arc::new(Stream::new(s))),
            Some(Err(e)) if socket::is_spent(&e) => Accepted::Spent,
            Some(Err(e)) => {
                Accepted::Failed(Refusal::os("accepting on the node agent's socket", e))
            }
        }
    }

    fn accepting(&self, on: bool) -> Result<(), Refusal> {
        let mut l = self.listening()?;
        l.on = on;
        self.accept_on(&mut l);
        Ok(())
    }

    fn stop_listening(&self) {
        if let Ok(mut l) = self.listening.lock() {
            l.on = false;
        }
    }

    fn peer(&self, conn: &Arc<Stream>) -> Peer {
        let Ok(pid) = conn.socket.peer_pid() else {
            return Peer::Gone;
        };
        match socket::same_user_or_system(pid) {
            Ok(true) => Peer::Allowed,
            Ok(false) => Peer::Refused(format!(
                "pid {pid} runs as another user than the node agent's"
            )),
            Err(_) => Peer::Gone,
        }
    }

    fn watch(&self, conn: &Arc<Stream>, token: u64, interest: Interest) -> Result<(), Refusal> {
        conn.token.store(token, Ordering::SeqCst);
        self.watching()?.insert(token, (Arc::clone(conn), interest));
        let mut st = conn.state.lock().map_err(poisoned)?;
        st.reading = matches!(interest, Interest::Read);
        conn.read_on(&mut st);
        let ready = match interest {
            Interest::Read => !st.ready.is_empty() || st.ended || st.broken,
            Interest::Write => !st.sending || st.ended,
        };
        drop(st);
        // What is ready already (a message read, room to send) brings no completion of its own, and a
        // worker may change a watch while the I/O thread waits: a wake makes that wait look again.
        if ready { self.wake() } else { Ok(()) }
    }

    fn rewatch(&self, conn: &Arc<Stream>, token: u64, interest: Interest) -> Result<(), Refusal> {
        self.watch(conn, token, interest)
    }

    fn unwatch(&self, conn: &Arc<Stream>) {
        if let Ok(mut w) = self.watching.lock() {
            w.retain(|_, (s, _)| !Arc::ptr_eq(s, conn));
        }
        if let Ok(mut st) = conn.state.lock() {
            st.reading = false;
        }
    }

    fn recv(&self, conn: &Arc<Stream>, buf: &mut [u8]) -> Recv<NoHandle> {
        let Ok(mut st) = conn.state.lock() else {
            return Recv::Gone;
        };
        if let Some(m) = st.ready.pop_front() {
            let n = m.bytes.len().min(buf.len());
            if let (Some(into), Some(from)) = (buf.get_mut(..n), m.bytes.get(..n)) {
                into.copy_from_slice(from);
            }
            conn.read_on(&mut st);
            return Recv::Message {
                size: m.size,
                got: n,
                passed: Vec::new(),
                cut: false,
            };
        }
        if std::mem::take(&mut st.reset) {
            return Recv::Reset;
        }
        if st.ended || st.broken {
            return Recv::Gone;
        }
        conn.read_on(&mut st);
        Recv::WouldBlock
    }

    /// Ends to pass can't go: Windows' AF_UNIX carries no handles, and no Windows host gives any (its
    /// runtimes refuse streams they'd make by name, before an exec). A message that carries some is never
    /// sent without them: its connection ends, which its client sees.
    fn send(&self, conn: &Arc<Stream>, bytes: &[u8], fds: &[hex_api::End]) -> Sent {
        if !fds.is_empty() {
            conn.socket.shutdown();
            return Sent::Gone;
        }
        let Ok(mut st) = conn.state.lock() else {
            return Sent::Gone;
        };
        if st.ended {
            return Sent::Gone;
        }
        if st.sending {
            return Sent::WouldBlock;
        }
        let Ok(framed) = stream::frame(bytes, wire::MAX_MESSAGE) else {
            return Sent::Gone;
        };
        match socket::send_all(&conn.socket, conn.token.load(Ordering::SeqCst), framed) {
            Ok(()) => {
                st.sending = true;
                Sent::Sent
            }
            Err(_) => Sent::Gone,
        }
    }

    fn shutdown(&self, conn: &Arc<Stream>) {
        conn.socket.shutdown();
    }

    fn hung_up(&self, conn: &Arc<Stream>) -> bool {
        conn.state.lock().map_or(true, |st| st.ended)
    }

    fn watch_program(&self, pid: u32, token: u64) -> Result<Watch, Unwatched> {
        socket::watch_program(&self.port, pid, token)
            .map(Watch)
            .map_err(|e| Unwatched {
                why: Refusal::Os {
                    op: "watching a started program",
                    why: format!(
                        "{e}: it runs on unwatched by the socket, and the node's sweep reaps it"
                    ),
                },
                killed: false,
            })
    }

    fn kill(&self, watch: &Watch) {
        watch.0.kill();
    }

    fn unwatch_program(&self, watch: Watch) {
        drop(watch);
    }

    fn stdio(&self, streams: Streams, passed: Vec<NoHandle>) -> Result<Stdio, Refusal> {
        if let Some(never) = passed.into_iter().next() {
            match never {}
        }
        if streams.count() > 0 {
            return Err(Refusal::Platform {
                what: "an exec's stdio through the node agent's socket",
                platform: std::env::consts::OS,
                why:
                    "Windows' AF_UNIX passes no handles, and the transport that carries an exec's \
                      streams to its caller waits on its measured decision (native plan step 8)"
                        .to_owned(),
            });
        }
        Ok(Stdio::null())
    }

    /// A host's own connection in the port isn't built on Windows yet: the IOCP source (a zero-byte
    /// overlapped receive as readiness) is native plan step 8's next change, and no Windows host has a
    /// source to add until then (its guest's connection isn't evented). Refused by name.
    fn watch_source(&self, _: super::Handle<'_>, _: u64, _: bool) -> Result<(), Refusal> {
        Err(Refusal::Platform {
            what: "a host's own connection in the node's event set",
            platform: std::env::consts::OS,
            why: "the completion port's source for it is native plan step 8's next change"
                .to_owned(),
        })
    }

    /// Nothing is watched as a source on Windows yet ([`Os::watch_source`]).
    fn unwatch_source(&self, _: super::Handle<'_>, _: u64) {}
}
