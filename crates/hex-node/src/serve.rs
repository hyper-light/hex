//! The node agent's local socket (decision 3 of docs/design/runtime-architecture.md): the `Runtime`
//! seam served over a local socket to callers that don't link Hex, in [`hex_core::wire`]'s format, and
//! from its version 2 the node's network namespace counts ([`Host::netns`]). An exec's stdio comes in
//! with its request, and a program's end goes out as a message after its exec's answer.
//!
//! This is the one core for every OS (native plan step 8): the protocol, the bounds, the disconnect rule
//! and the workers, written once over an [`Os`], each OS's event sources and local socket. Linux's is
//! [`linux::Linux`]: a `SOCK_SEQPACKET` socket (unix(7)), descriptors passed by `SCM_RIGHTS`, and one epoll
//! set over the listener, the connections and the programs' pidfds. macOS and Windows have no
//! `SOCK_SEQPACKET` local socket (macOS 26.2 refuses it, EPROTONOSUPPORT, measured on studio-3,
//! 2026-11-07; Windows' AF_UNIX is `SOCK_STREAM` alone), so theirs deliver whole messages framed over a
//! stream ([`hex_core::stream`]).
//!
//! One thread does every connection's I/O and watches every exec'd program's end through the OS's event
//! set; it never blocks but in [`Os::wait`]. The runtime's calls, which may block (a delete waits for its
//! box to empty), run on a pool of workers, which answer their calls themselves.
//!
//! Each resource has a bound (CLAUDE.md §2), refused past by type, naming it (§3):
//! - connections ([`Limits::connections`]): one more is answered `Exhausted` and closed;
//! - a message's size ([`Limits::message_bytes`]): a larger one is answered `Invalid`, naming it;
//! - requests in flight on a connection ([`Limits::in_flight`]): one more is answered `Exhausted`;
//! - programs a connection watches ([`Limits::programs`]): an exec past them is answered `Exhausted`;
//! - answers waiting for room in a connection's socket: while any wait, its requests aren't read, so a
//!   client that doesn't read its answers stops being read rather than growing the agent's memory.
//!   By construction they number at most `in_flight + programs + 1`: an answer for each call in
//!   flight (a list's parts go one at a time), an end for each program, and the one refusal read
//!   before reading stopped;
//! - workers ([`WORKERS`]): a call past them waits in the queue its connections' in-flight bounds
//!   hold.
//!
//! The disconnect rule (decision 3): when a connection closes, every program its execs started that it
//! still watches is sent KILL through the OS's watch of it, reaped, and counted in its box ([`Host::abandoned`]),
//! which inspect reports; none is left running unwatched. A program the client released (it dropped
//! the `Proc`) runs on, as a dropped in-process `Proc` does. A request the agent has read is carried
//! out whether or not its client stays to take the answer; one it hadn't read is let go with the
//! connection.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hex_api::{BoxId, Exit, Proc, Program, Refusal, Runtime, Stdio};
use hex_core::netns;
use hex_core::wire::{self, Answer, Call, Header, Hello, Received, Request, Streams};

pub use hex_core::wire::Limits;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(unix)]
mod unix;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod unsupported;

/// This OS's event sources and local socket.
#[cfg(target_os = "linux")]
pub type Native = linux::Linux;
#[cfg(target_os = "macos")]
pub type Native = macos::MacOs;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub type Native = unsupported::Unsupported;

/// The node agent's bounds on its socket unless set ([`Server::bind`]'s `limits`):
/// - 64 connections: a node's local callers are few (kiln's runner and executor, the command line),
///   so this is far past them while holding each connection's cost to a known size;
/// - messages of [`wire::MAX_MESSAGE`] bytes, the largest the format sends;
/// - 64 requests in flight on a connection: twice the bench's most threads sharing one client (32,
///   seam_oci's throughput phase), so a client's threads don't meet it in use;
/// - 1,024 programs watched by a connection, each holding a watch here (a pidfd on Linux): 64
///   connections' worth is 65,536 descriptors, the supervisor's `INIT_BUDGET`.
pub const LIMITS: Limits = Limits {
    connections: 64,
    message_bytes: wire::MAX_MESSAGE,
    in_flight: 64,
    programs: 1_024,
};

/// Most workers running calls at once, started as calls need them and kept: past them, calls wait in
/// the queue. Twice [`LIMITS`]' in-flight bound, so one connection's calls never wait on another's.
pub const WORKERS: usize = 128;

/// What [`Refusal::Exhausted`] names for each bound.
pub const CONNECTIONS: &str = "connections to the node agent's socket";
pub const IN_FLIGHT: &str = "requests in flight on one connection";
pub const PROGRAMS: &str = "programs watched by one connection";

/// Messages read from one connection before the others get a turn: a fairness bound, so one busy
/// client can't hold the I/O thread.
const READS_PER_WAKE: usize = 64;
/// Most events one wake takes; more come on the next.
pub const EVENTS: usize = 256;
/// Longest a list's worker waits for its client to read one part before it sends the next; past it
/// the client has stopped reading, and its connection is closed. Five times the node's longest wait
/// of its own (a delete's `EMPTY_PATIENCE`, 60 s), so a client busy with another call isn't taken for
/// one that stopped.
const READ_PATIENCE: Duration = Duration::from_secs(300);
/// Longest a stopping server waits for the programs it ended to be reaped and recorded, as the node's
/// `EMPTY_PATIENCE`: a killed process ends in milliseconds.
const STOP_PATIENCE: Duration = Duration::from_secs(60);

/// The first token a connection or program takes: below it are the listener's and the wake's.
const FIRST_TOKEN: u64 = 2;

/// What the socket needs of the runtime it serves, beyond the seam.
pub trait Host: Runtime {
    /// Counts in box `id` a program exec'd in it that the node ended because the connection watching
    /// it closed (the disconnect rule): reported as [`hex_api::BoxInfo::abandoned`].
    fn abandoned(&self, id: &BoxId) -> Result<(), Refusal>;

    /// The node's network namespace counts (decision 4), which a client of wire version 2 asks for.
    fn netns(&self) -> Result<netns::Counts, Refusal>;
}

/// What a wake of the OS's event set says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// The listener has connections waiting.
    Listener,
    /// [`Os::wake`] was called.
    Wake,
    /// A connection or watched program, by its token: a connection readable (or its peer half-closed),
    /// writable, or hung up; a program's watch ready, which is its end.
    Ready {
        token: u64,
        readable: bool,
        writable: bool,
        hung_up: bool,
    },
}

/// What a connection is watched for: its requests, or room to send its waiting answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interest {
    Read,
    Write,
}

/// What [`Os::accept`] found.
pub enum Accepted<C> {
    Conn(C),
    /// None waits.
    None,
    /// The agent is out of descriptors or handles: the caller waits in the backlog until a connection
    /// closes or a program ends, which resumes accepting.
    Spent,
    Failed(Refusal),
}

/// Who a connection's peer is.
pub enum Peer {
    /// This user, or root.
    Allowed,
    /// Anyone else, and why, for the refusal.
    Refused(String),
    /// Gone before it could be asked.
    Gone,
}

/// What [`Os::recv`] read: one whole message, as the client sent it.
pub enum Recv<P> {
    /// A message of `size` bytes, its first `got` in the buffer (fewer when it is past the buffer), and
    /// what came with it (`cut`: more came than a message takes, or came where none belongs).
    Message {
        size: usize,
        got: usize,
        passed: Vec<P>,
        cut: bool,
    },
    /// Nothing waits.
    WouldBlock,
    /// The client closed with answers unread: reported once, before what it sent, which is still there
    /// to read (Linux, net/unix/af_unix.c `unix_release_sock`).
    Reset,
    /// The connection failed.
    Gone,
}

/// What [`Os::send`] did with one whole message.
pub enum Sent {
    Sent,
    /// No room: try again once the connection is writable.
    WouldBlock,
    /// Its client is gone.
    Gone,
}

/// Why a started program couldn't be watched, and whether the OS ended it (it could open a handle to the
/// program but not watch it); one it couldn't open runs on, unwatched, for the node's sweep.
pub struct Unwatched {
    pub why: Refusal,
    pub killed: bool,
}

/// One OS's event sources and local socket: everything the core does that isn't the same on every OS.
/// The event set is level-triggered: a connection watched for [`Interest::Read`] is reported while a
/// message waits or its client has gone, one watched for [`Interest::Write`] while it has room.
pub trait Os: Send + Sync + 'static {
    /// A connection's socket.
    type Conn: Send + Sync + 'static;
    /// A watched program: what its end is seen through and it is killed through, so a kill can't reach a
    /// process given its pid since.
    type Watch: Send + Sync + 'static;
    /// A descriptor or handle that came with a request: an exec's stdio.
    type Passed: Send + 'static;

    /// Waits for events, at most `timeout` (`None`: until one comes), appending them to `events`, at
    /// most [`EVENTS`]; an interrupted wait returns none.
    fn wait(&self, events: &mut Vec<Event>, timeout: Option<Duration>) -> Result<(), Refusal>;
    /// Makes the next (or current) [`Os::wait`] return [`Event::Wake`].
    fn wake(&self) -> Result<(), Refusal>;
    /// Takes one waiting connection.
    fn accept(&self) -> Accepted<Self::Conn>;
    /// Watches the listener (`on`) or stops: accepting is paused while descriptors are spent.
    fn accepting(&self, on: bool) -> Result<(), Refusal>;
    /// Stops watching the listener and the wake, for a stopping server.
    fn stop_listening(&self);
    /// Who `conn`'s peer is.
    fn peer(&self, conn: &Self::Conn) -> Peer;
    /// Starts watching `conn` under `token`.
    fn watch(&self, conn: &Self::Conn, token: u64, interest: Interest) -> Result<(), Refusal>;
    /// Changes what `conn` is watched for.
    fn rewatch(&self, conn: &Self::Conn, token: u64, interest: Interest) -> Result<(), Refusal>;
    /// Stops watching `conn`.
    fn unwatch(&self, conn: &Self::Conn);
    /// Reads one whole message into `buf` without blocking; interrupted reads are retried.
    fn recv(&self, conn: &Self::Conn, buf: &mut [u8]) -> Recv<Self::Passed>;
    /// Sends one whole message without blocking, or none of it.
    fn send(&self, conn: &Self::Conn, bytes: &[u8]) -> Sent;
    /// Shuts `conn` both ways: its client sees the end, and the event set reports it hung up.
    fn shutdown(&self, conn: &Self::Conn);
    /// Whether `conn`'s peer has gone: a read of nothing then is its end, not an empty message.
    fn hung_up(&self, conn: &Self::Conn) -> bool;
    /// Watches the program `pid` under `token` until it ends.
    fn watch_program(&self, pid: u32, token: u64) -> Result<Self::Watch, Unwatched>;
    /// Sends the watched program KILL; one that has ended already is reaped and counted just the same.
    fn kill(&self, watch: &Self::Watch);
    /// Stops watching a program.
    fn unwatch_program(&self, watch: Self::Watch);
    /// A program's stdio from what its exec carried, in the order `streams` names them; anything that
    /// isn't a stream's right end is refused by name.
    fn stdio(&self, streams: Streams, passed: Vec<Self::Passed>) -> Result<Stdio, Refusal>;
}

impl<O: Os> Os for Arc<O> {
    type Conn = O::Conn;
    type Watch = O::Watch;
    type Passed = O::Passed;

    fn wait(&self, events: &mut Vec<Event>, timeout: Option<Duration>) -> Result<(), Refusal> {
        (**self).wait(events, timeout)
    }
    fn wake(&self) -> Result<(), Refusal> {
        (**self).wake()
    }
    fn accept(&self) -> Accepted<Self::Conn> {
        (**self).accept()
    }
    fn accepting(&self, on: bool) -> Result<(), Refusal> {
        (**self).accepting(on)
    }
    fn stop_listening(&self) {
        (**self).stop_listening();
    }
    fn peer(&self, conn: &Self::Conn) -> Peer {
        (**self).peer(conn)
    }
    fn watch(&self, conn: &Self::Conn, token: u64, interest: Interest) -> Result<(), Refusal> {
        (**self).watch(conn, token, interest)
    }
    fn rewatch(&self, conn: &Self::Conn, token: u64, interest: Interest) -> Result<(), Refusal> {
        (**self).rewatch(conn, token, interest)
    }
    fn unwatch(&self, conn: &Self::Conn) {
        (**self).unwatch(conn);
    }
    fn recv(&self, conn: &Self::Conn, buf: &mut [u8]) -> Recv<Self::Passed> {
        (**self).recv(conn, buf)
    }
    fn send(&self, conn: &Self::Conn, bytes: &[u8]) -> Sent {
        (**self).send(conn, bytes)
    }
    fn shutdown(&self, conn: &Self::Conn) {
        (**self).shutdown(conn);
    }
    fn hung_up(&self, conn: &Self::Conn) -> bool {
        (**self).hung_up(conn)
    }
    fn watch_program(&self, pid: u32, token: u64) -> Result<Self::Watch, Unwatched> {
        (**self).watch_program(pid, token)
    }
    fn kill(&self, watch: &Self::Watch) {
        (**self).kill(watch);
    }
    fn unwatch_program(&self, watch: Self::Watch) {
        (**self).unwatch_program(watch);
    }
    fn stdio(&self, streams: Streams, passed: Vec<Self::Passed>) -> Result<Stdio, Refusal> {
        (**self).stdio(streams, passed)
    }
}

/// The socket, served until dropped.
pub struct Server<O: Os = Native> {
    shared: Arc<Shared<O>>,
    io: Option<JoinHandle<()>>,
}

struct Shared<O: Os> {
    os: O,
    host: Arc<dyn Host>,
    /// The host's classes, asked once: a hello is answered on the I/O thread, which never calls the
    /// host.
    classes: Vec<hex_api::Class>,
    limits: Limits,
    workers: usize,
    path: PathBuf,
    stopping: AtomicBool,
    table: Mutex<Table<O>>,
    jobs: Mutex<Jobs<O>>,
    work: Condvar,
    /// Why the I/O thread stopped, if it failed.
    fault: Mutex<Option<String>>,
}

/// The connections and the programs they watch, by token. Locked before a connection's own state when
/// both are held.
struct Table<O: Os> {
    conns: BTreeMap<u64, Arc<Conn<O>>>,
    watched: BTreeMap<u64, Watched<O>>,
    next: u64,
    /// Accepting paused: the agent ran out of descriptors, so a closed connection or a reaped program
    /// resumes it.
    accept_paused: bool,
}

impl<O: Os> Default for Table<O> {
    fn default() -> Self {
        Self {
            conns: BTreeMap::new(),
            watched: BTreeMap::new(),
            next: FIRST_TOKEN,
            accept_paused: false,
        }
    }
}

impl<O: Os> Table<O> {
    fn token(&mut self) -> Result<u64, Refusal> {
        let token = self.next.max(FIRST_TOKEN);
        self.next = token
            .checked_add(1)
            .ok_or_else(|| Refusal::Broken("the socket's tokens ran out".to_owned()))?;
        Ok(token)
    }
}

struct Conn<O: Os> {
    token: u64,
    sock: O::Conn,
    out: Mutex<Out>,
    /// Signalled when its waiting answers are sent, or it closes.
    drained: Condvar,
}

struct Out {
    open: bool,
    in_flight: usize,
    /// The programs it watches, by their exec's id, with their watch's token.
    programs: BTreeMap<u64, u64>,
    /// Places taken by execs under way.
    reserved: usize,
    /// Answers waiting for room in the socket, oldest first.
    backlog: VecDeque<Vec<u8>>,
    /// Whether its requests are read ([`Interest::Read`]) or only its backlog written.
    reading: bool,
}

/// A program a connection's exec started, watched by the OS.
struct Watched<O: Os> {
    conn: Arc<Conn<O>>,
    /// Its exec's id and version, which its end is sent with.
    exec: u64,
    version: u16,
    box_id: BoxId,
    proc: Box<dyn Proc>,
    watch: Option<O::Watch>,
    /// Sent KILL because its connection closed: its end is counted in its box, not sent.
    killed: bool,
}

struct Jobs<O: Os> {
    queue: VecDeque<Job<O>>,
    workers: usize,
    idle: usize,
}

impl<O: Os> Default for Jobs<O> {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            workers: 0,
            idle: 0,
        }
    }
}

enum Job<O: Os> {
    Call {
        conn: Arc<Conn<O>>,
        header: Header,
        call: Box<Call>,
        passed: Vec<O::Passed>,
    },
    Abandoned(BoxId),
}

fn poisoned<T>(_: std::sync::PoisonError<T>) -> Refusal {
    Refusal::Broken("a socket lock was poisoned".to_owned())
}

impl Server<Native> {
    /// Serves `host` on this OS's socket at `path`, readable and writable by this user alone, holding
    /// `limits` and running calls on at most `workers` threads. A socket left at `path` by an agent that
    /// has ended is replaced; one an agent still serves, or a file that isn't a socket, is refused.
    pub fn bind(
        path: &Path,
        host: Arc<dyn Host>,
        limits: Limits,
        workers: usize,
    ) -> Result<Self, Refusal> {
        check_limits(&limits, workers)?;
        let os = Native::bind(path)?;
        Self::with_os(os, host, limits, workers, path)
    }
}

impl<O: Os> Server<O> {
    /// Serves `host` over `os`, listening at `path`.
    pub fn with_os(
        os: O,
        host: Arc<dyn Host>,
        limits: Limits,
        workers: usize,
        path: &Path,
    ) -> Result<Self, Refusal> {
        check_limits(&limits, workers)?;
        os.accepting(true)?;
        let shared = Arc::new(Shared {
            os,
            classes: host.classes(),
            host,
            limits,
            workers,
            path: path.to_owned(),
            stopping: AtomicBool::new(false),
            table: Mutex::new(Table::default()),
            jobs: Mutex::new(Jobs::default()),
            work: Condvar::new(),
            fault: Mutex::new(None),
        });
        let io = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("hex-socket".to_owned())
                .spawn(move || shared.run())
                .map_err(|e| Refusal::os("starting the socket's thread", e))?
        };
        Ok(Self {
            shared,
            io: Some(io),
        })
    }

    /// Where it listens.
    pub fn path(&self) -> &Path {
        &self.shared.path
    }

    /// Why its I/O thread stopped, if it failed: it then serves nothing more.
    pub fn fault(&self) -> Option<String> {
        self.shared.fault.lock().ok().and_then(|f| f.clone())
    }

    /// Serves until the I/O thread stops, which it does only when it fails; returns why.
    pub fn wait(&mut self) -> Option<String> {
        if let Some(io) = self.io.take() {
            let _ = io.join();
        }
        self.fault()
    }
}

impl<O: Os> Drop for Server<O> {
    /// Stops serving: the listener and every connection close, their programs are ended by the
    /// disconnect rule, and the socket's file is removed.
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        let _ = self.shared.os.wake();
        if let Some(io) = self.io.take() {
            let _ = io.join();
        }
        let _ = std::fs::remove_file(&self.shared.path);
    }
}

fn check_limits(limits: &Limits, workers: usize) -> Result<(), Refusal> {
    let invalid = |why: String| {
        Err(Refusal::Invalid {
            what: "socket limits",
            why,
        })
    };
    if limits.message_bytes < wire::HEADER_BYTES || limits.message_bytes > wire::MAX_MESSAGE {
        return invalid(format!(
            "a message bound of {} bytes is outside {}..={}",
            limits.message_bytes,
            wire::HEADER_BYTES,
            wire::MAX_MESSAGE
        ));
    }
    if [
        limits.connections,
        limits.in_flight,
        limits.programs,
        workers,
    ]
    .contains(&0)
    {
        return invalid(
            "a bound of 0 connections, requests, programs or workers serves nothing".to_owned(),
        );
    }
    Ok(())
}

impl<O: Os> Shared<O> {
    fn table(&self) -> Result<MutexGuard<'_, Table<O>>, Refusal> {
        self.table.lock().map_err(poisoned)
    }

    /// The I/O thread: until the server stops, or fails (every connection then closes, so no client
    /// waits on a server that is gone).
    fn run(self: &Arc<Self>) {
        let mut buf = vec![0u8; self.limits.message_bytes];
        let mut events = Vec::with_capacity(EVENTS);
        let outcome = loop {
            events.clear();
            if let Err(e) = self.os.wait(&mut events, None) {
                break Err(e);
            }
            if self.stopping.load(Ordering::SeqCst) {
                break Ok(());
            }
            let mut failed = None;
            for event in &events {
                if let Err(e) = self.event(*event, &mut buf) {
                    failed.get_or_insert(e);
                }
            }
            if let Some(e) = failed {
                break Err(e);
            }
        };
        if let Err(e) = &outcome
            && let Ok(mut fault) = self.fault.lock()
        {
            *fault = Some(e.to_string());
        }
        self.stop();
    }

    fn event(self: &Arc<Self>, event: Event, buf: &mut [u8]) -> Result<(), Refusal> {
        match event {
            Event::Listener => self.accept(),
            Event::Wake => Ok(()),
            Event::Ready {
                token,
                readable,
                writable,
                hung_up,
            } => {
                let (conn, watched) = {
                    let table = self.table()?;
                    (
                        table.conns.get(&token).cloned(),
                        table.watched.contains_key(&token),
                    )
                };
                if let Some(conn) = conn {
                    self.connection(&conn, readable, writable, hung_up, buf)
                } else if watched {
                    self.ended(token)
                } else {
                    // A connection or program removed since the event was queued.
                    Ok(())
                }
            }
        }
    }

    /// Takes every connection waiting, refusing past the connection bound and any caller that isn't
    /// this user (or root).
    fn accept(self: &Arc<Self>) -> Result<(), Refusal> {
        loop {
            match self.os.accept() {
                Accepted::Conn(sock) => self.admit(sock)?,
                Accepted::None => return Ok(()),
                // Out of descriptors: the caller waits in the backlog until a connection closes or a
                // program ends, which resumes accepting.
                Accepted::Spent => {
                    let mut table = self.table()?;
                    table.accept_paused = true;
                    return self.os.accepting(false);
                }
                Accepted::Failed(e) => return Err(e),
            }
        }
    }

    fn admit(self: &Arc<Self>, sock: O::Conn) -> Result<(), Refusal> {
        match self.os.peer(&sock) {
            Peer::Allowed => {}
            Peer::Gone => return Ok(()),
            Peer::Refused(why) => {
                self.refuse(
                    &sock,
                    Refusal::Invalid {
                        what: "caller",
                        why,
                    },
                );
                return Ok(());
            }
        }
        let mut table = self.table()?;
        if table.conns.len() >= self.limits.connections {
            drop(table);
            self.refuse(
                &sock,
                Refusal::Exhausted {
                    what: CONNECTIONS,
                    bound: self.limits.connections,
                },
            );
            return Ok(());
        }
        let token = table.token()?;
        if let Err(e) = self.os.watch(&sock, token, Interest::Read) {
            // This connection can't be watched (the OS's watches are spent): refused, and the ones
            // held go on being served.
            drop(table);
            self.refuse(&sock, e);
            return Ok(());
        }
        table.conns.insert(
            token,
            Arc::new(Conn {
                token,
                sock,
                out: Mutex::new(Out {
                    open: true,
                    in_flight: 0,
                    programs: BTreeMap::new(),
                    reserved: 0,
                    backlog: VecDeque::new(),
                    reading: true,
                }),
                drained: Condvar::new(),
            }),
        );
        Ok(())
    }

    /// Answers a connection that won't be served with why, as id 0 (its client hasn't asked anything
    /// yet), in the first version, which a client of any version reads, then lets it go.
    fn refuse(&self, sock: &O::Conn, refusal: Refusal) {
        if let Ok(bytes) =
            wire::answer(wire::FIRST, 0, &Answer::Refused(refusal), wire::MAX_MESSAGE)
        {
            let _ = self.os.send(sock, &bytes);
        }
        self.os.shutdown(sock);
    }

    fn connection(
        self: &Arc<Self>,
        conn: &Arc<Conn<O>>,
        readable: bool,
        writable: bool,
        hung_up: bool,
        buf: &mut [u8],
    ) -> Result<(), Refusal> {
        // Closed by its client: what it sent before it went is read and carried out (a release among
        // it lets its program go), then the connection closes. Its answers can't be taken.
        if writable && !hung_up {
            self.flush(conn)?;
        }
        let ended = if hung_up || readable {
            self.read(conn, buf, hung_up)?
        } else {
            false
        };
        if ended { self.close(conn) } else { Ok(()) }
    }

    /// Reads a connection's requests until none wait, its answers back up, or it has had its turn;
    /// or, once its client has gone (`gone`), every request it left. Whether its client has gone.
    fn read(
        self: &Arc<Self>,
        conn: &Arc<Conn<O>>,
        buf: &mut [u8],
        mut gone: bool,
    ) -> Result<bool, Refusal> {
        if gone {
            conn.out.lock().map_err(poisoned)?.backlog.clear();
        }
        let mut turn = 0usize;
        let mut reset = false;
        loop {
            if !gone {
                if turn >= READS_PER_WAKE || !conn.out.lock().map_err(poisoned)?.reading {
                    return Ok(false);
                }
                turn = turn.saturating_add(1);
            }
            let (size, got, passed, cut) = match self.os.recv(&conn.sock, buf) {
                Recv::Message {
                    size,
                    got,
                    passed,
                    cut,
                } => (size, got, passed, cut),
                Recv::WouldBlock => return Ok(gone),
                Recv::Reset if !reset => {
                    reset = true;
                    gone = true;
                    conn.out.lock().map_err(poisoned)?.backlog.clear();
                    continue;
                }
                Recv::Reset | Recv::Gone => return Ok(true),
            };
            // A read of nothing is the client's end, unless it sent an empty message on purpose,
            // which is refused as one.
            if size == 0 && passed.is_empty() && (gone || self.os.hung_up(&conn.sock)) {
                return Ok(true);
            }
            let bytes = buf.get(..got.min(size).min(buf.len())).unwrap_or_default();
            let received = Received {
                bytes,
                size,
                descriptors: passed.len(),
                descriptors_cut: cut,
            };
            self.request(conn, &received, passed)?;
        }
    }

    fn request(
        self: &Arc<Self>,
        conn: &Arc<Conn<O>>,
        msg: &Received<'_>,
        passed: Vec<O::Passed>,
    ) -> Result<(), Refusal> {
        match wire::read_request(msg, self.limits.message_bytes) {
            Err(bad) => {
                drop(passed);
                self.answer(
                    conn,
                    bad.version,
                    bad.id,
                    &Answer::Refused(bad.fault.into()),
                )
            }
            Ok((h, Request::Hello)) => self.answer(
                conn,
                h.version,
                h.id,
                &Answer::Hello(Hello {
                    version: wire::VERSION,
                    classes: self.classes.clone(),
                    limits: self.limits,
                }),
            ),
            Ok((h, Request::Release)) => self.release(conn, h.id),
            Ok((header, Request::Call(call))) => {
                let admitted = {
                    let mut out = conn.out.lock().map_err(poisoned)?;
                    match out.in_flight.checked_add(1) {
                        Some(n) if n <= self.limits.in_flight => {
                            out.in_flight = n;
                            true
                        }
                        _ => false,
                    }
                };
                if !admitted {
                    return self.answer(
                        conn,
                        header.version,
                        header.id,
                        &Answer::Refused(Refusal::Exhausted {
                            what: IN_FLIGHT,
                            bound: self.limits.in_flight,
                        }),
                    );
                }
                self.enqueue(Job::Call {
                    conn: Arc::clone(conn),
                    header,
                    call: Box::new(call),
                    passed,
                })
            }
        }
    }

    /// Sends one answer on a connection, or keeps it until there is room; an answer that can't be
    /// written is refused to its request in its place.
    fn answer(
        &self,
        conn: &Conn<O>,
        version: u16,
        id: u64,
        answer: &Answer,
    ) -> Result<(), Refusal> {
        let bytes = wire::answer(version, id, answer, self.limits.message_bytes)
            .or_else(|fault| {
                let refused = Answer::Refused(Refusal::Broken(format!(
                    "the node agent's answer couldn't be written: {fault}"
                )));
                wire::answer(version, id, &refused, self.limits.message_bytes)
            })
            .map_err(|e| Refusal::Broken(format!("a refusal couldn't be written: {e}")))?;
        self.send(conn, bytes, None)
    }

    /// Queues `bytes` after a connection's waiting answers, or sends them now when none wait. With a
    /// `deadline`, first waits for the waiting ones to go (a list's next part), closing the connection
    /// once it passes. Sending on a closed connection does nothing: its client is gone.
    fn send(
        &self,
        conn: &Conn<O>,
        bytes: Vec<u8>,
        deadline: Option<Instant>,
    ) -> Result<(), Refusal> {
        let mut out = conn.out.lock().map_err(poisoned)?;
        if let Some(deadline) = deadline {
            while out.open && !out.backlog.is_empty() {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    drop(out);
                    // The I/O thread sees the hang-up and closes it, by the disconnect rule.
                    self.os.shutdown(&conn.sock);
                    return Ok(());
                }
                out = conn.drained.wait_timeout(out, left).map_err(poisoned)?.0;
            }
        }
        if !out.open {
            return Ok(());
        }
        if out.backlog.is_empty() {
            match self.os.send(&conn.sock, &bytes) {
                // Gone: its client is gone, and the I/O thread sees the hang-up and closes it.
                Sent::Sent | Sent::Gone => return Ok(()),
                Sent::WouldBlock => {}
            }
        }
        out.backlog.push_back(bytes);
        if out.reading {
            out.reading = false;
            self.os
                .rewatch(&conn.sock, conn.token, Interest::Write)
                .map_err(|e| Refusal::os("waiting for room in a connection", e))?;
        }
        Ok(())
    }

    /// Writes a connection's waiting answers while there is room; once all are out its requests are
    /// read again.
    fn flush(&self, conn: &Conn<O>) -> Result<(), Refusal> {
        let mut out = conn.out.lock().map_err(poisoned)?;
        while let Some(front) = out.backlog.front() {
            match self.os.send(&conn.sock, front) {
                Sent::Sent => {
                    out.backlog.pop_front();
                }
                Sent::WouldBlock => return Ok(()),
                Sent::Gone => {
                    // Its client is gone: the hang-up closes it.
                    out.backlog.clear();
                    break;
                }
            }
        }
        if out.open && !out.reading {
            out.reading = true;
            self.os
                .rewatch(&conn.sock, conn.token, Interest::Read)
                .map_err(|e| Refusal::os("reading a connection again", e))?;
        }
        conn.drained.notify_all();
        Ok(())
    }

    /// Closes a connection and applies the disconnect rule to the programs it still watches.
    fn close(&self, conn: &Conn<O>) -> Result<(), Refusal> {
        let mut table = self.table()?;
        let programs: Vec<u64> = {
            let mut out = conn.out.lock().map_err(poisoned)?;
            if !out.open {
                return Ok(());
            }
            out.open = false;
            out.backlog.clear();
            out.programs.values().copied().collect()
        };
        conn.drained.notify_all();
        self.os.unwatch(&conn.sock);
        self.os.shutdown(&conn.sock);
        table.conns.remove(&conn.token);
        for token in programs {
            if let Some(w) = table.watched.get_mut(&token) {
                self.end(w);
            }
        }
        self.resume_accepting(&mut table)
    }

    /// The client no longer waits for exec `exec`'s program: it runs on, unwatched by the socket,
    /// and the node's sweep reaps it once it ends, as with a dropped in-process `Proc`.
    fn release(&self, conn: &Conn<O>, exec: u64) -> Result<(), Refusal> {
        let mut table = self.table()?;
        let token = conn.out.lock().map_err(poisoned)?.programs.remove(&exec);
        if let Some(mut w) = token.and_then(|t| table.watched.remove(&t))
            && let Some(watch) = w.watch.take()
        {
            self.os.unwatch_program(watch);
        }
        self.resume_accepting(&mut table)
    }

    /// KILL to the program exec `exec` of `conn` started, through the OS's watch as the disconnect rule's
    /// is, so it can't reach a process given its pid since. Its end is then sent as any end is; one that has
    /// ended (or was released) is no longer the connection's, and there is nothing to kill.
    fn kill(&self, conn: &Conn<O>, exec: u64) -> Result<(), Refusal> {
        let table = self.table()?;
        let token = conn
            .out
            .lock()
            .map_err(poisoned)?
            .programs
            .get(&exec)
            .copied();
        if let Some(watch) = token
            .and_then(|t| table.watched.get(&t))
            .and_then(|w| w.watch.as_ref())
        {
            self.os.kill(watch);
        }
        Ok(())
    }

    /// A watched program's watch is ready: it has ended. Reaped, and its end sent to its connection,
    /// or counted in its box when the connection closed (the disconnect rule).
    fn ended(self: &Arc<Self>, token: u64) -> Result<(), Refusal> {
        let mut w = {
            let mut table = self.table()?;
            let Some(w) = table.watched.remove(&token) else {
                return Ok(());
            };
            self.resume_accepting(&mut table)?;
            w
        };
        let exit = match w.proc.try_wait() {
            Ok(Some(exit)) => exit,
            // Ready but not yet waitable can't happen on an OS whose watch reports the end (pidfd_open(2));
            // watched again rather than lost.
            Ok(None) => {
                let mut table = self.table()?;
                table.watched.insert(token, w);
                return Ok(());
            }
            // Reaped elsewhere in this process: how it ended is no longer known.
            Err(_) => Exit::Unknown,
        };
        if let Some(watch) = w.watch.take() {
            self.os.unwatch_program(watch);
        }
        let open = {
            let mut out = w.conn.out.lock().map_err(poisoned)?;
            out.programs.remove(&w.exec);
            out.open
        };
        if open && !w.killed {
            self.answer(&w.conn, w.version, w.exec, &Answer::Exited(exit))
        } else {
            self.enqueue(Job::Abandoned(w.box_id))
        }
    }

    fn resume_accepting(&self, table: &mut Table<O>) -> Result<(), Refusal> {
        if table.accept_paused {
            table.accept_paused = false;
            self.os
                .accepting(true)
                .map_err(|e| Refusal::os("resuming the node agent's socket", e))?;
        }
        Ok(())
    }

    /// Ends a program whose connection closed: KILL through the OS's watch, so it can't reach a process
    /// given its pid since; its end, reaped when the watch is ready, is counted in its box.
    fn end(&self, w: &mut Watched<O>) {
        if let Some(watch) = &w.watch {
            self.os.kill(watch);
        }
        w.killed = true;
    }

    /// Hands a job to a worker, starting one when none is idle and fewer than the bound run.
    fn enqueue(self: &Arc<Self>, job: Job<O>) -> Result<(), Refusal> {
        let mut jobs = self.jobs.lock().map_err(poisoned)?;
        jobs.queue.push_back(job);
        if jobs.idle == 0 && jobs.workers < self.workers {
            let shared = Arc::clone(self);
            match std::thread::Builder::new()
                .name("hex-socket-worker".to_owned())
                .spawn(move || shared.work())
            {
                Ok(_) => jobs.workers = jobs.workers.saturating_add(1),
                // With no worker at all the job would never run: a call is refused to its request
                // instead, and a count made here.
                Err(e) if jobs.workers == 0 => {
                    let job = jobs.queue.pop_back();
                    drop(jobs);
                    return match job {
                        Some(Job::Call { conn, header, .. }) => {
                            self.done(&conn)?;
                            self.answer(
                                &conn,
                                header.version,
                                header.id,
                                &Answer::Refused(Refusal::os("starting a worker for a call", e)),
                            )
                        }
                        Some(Job::Abandoned(id)) => {
                            let _ = self.host.abandoned(&id);
                            Ok(())
                        }
                        None => Ok(()),
                    };
                }
                // Another worker takes it once free.
                Err(_) => {}
            }
        }
        self.work.notify_one();
        Ok(())
    }

    /// A worker: runs jobs until the server stops and the queue is empty.
    fn work(self: Arc<Self>) {
        loop {
            let job = {
                let Ok(mut jobs) = self.jobs.lock() else {
                    return;
                };
                loop {
                    if let Some(job) = jobs.queue.pop_front() {
                        break job;
                    }
                    if self.stopping.load(Ordering::SeqCst) {
                        jobs.workers = jobs.workers.saturating_sub(1);
                        return;
                    }
                    jobs.idle = jobs.idle.saturating_add(1);
                    let Ok(woken) = self.work.wait(jobs) else {
                        return;
                    };
                    jobs = woken;
                    jobs.idle = jobs.idle.saturating_sub(1);
                }
            };
            match job {
                Job::Call {
                    conn,
                    header,
                    call,
                    passed,
                } => {
                    let _ = self.call(&conn, header, *call, passed);
                }
                // Its box gone since (deleted by another caller) is nothing to count it in; any other
                // failure the node keeps and reports itself (`Node::abandoned`).
                Job::Abandoned(id) => {
                    let _ = self.host.abandoned(&id);
                }
            }
        }
    }

    /// Runs one call and answers it; its place in flight is given back before the answer goes, so a
    /// client that sends its next call on reading the answer never meets the bound.
    fn call(
        &self,
        conn: &Arc<Conn<O>>,
        header: Header,
        call: Call,
        passed: Vec<O::Passed>,
    ) -> Result<(), Refusal> {
        let host = &self.host;
        let answer = match call {
            Call::Exec {
                id,
                program,
                streams,
            } => return self.exec(conn, header, id, program, streams, passed),
            Call::List => {
                let listed = host.list();
                self.done(conn)?;
                return match listed {
                    Ok(infos) => self.list(conn, header, &infos),
                    Err(r) => self.answer(conn, header.version, header.id, &Answer::Refused(r)),
                };
            }
            Call::Create(spec) => host.create(spec).map(Answer::Info),
            Call::Start(id) => host.start(&id).map(|()| Answer::Done),
            Call::Signal { id, signal } => host.signal(&id, signal).map(|()| Answer::Done),
            Call::Inspect(id) => host.inspect(&id).map(Answer::Info),
            Call::Usage(id) => host.usage(&id).map(Answer::Usage),
            Call::Delete(id) => host.delete(&id).map(|()| Answer::Done),
            Call::Netns => host.netns().map(Answer::Netns),
            Call::Kill { exec } => self.kill(conn, exec).map(|()| Answer::Done),
        };
        self.done(conn)?;
        let answer = answer.unwrap_or_else(Answer::Refused);
        self.answer(conn, header.version, header.id, &answer)
    }

    fn done(&self, conn: &Conn<O>) -> Result<(), Refusal> {
        let mut out = conn.out.lock().map_err(poisoned)?;
        out.in_flight = out.in_flight.saturating_sub(1);
        Ok(())
    }

    /// A list's answer, in as many parts as its boxes need, each sent once the one before has gone.
    fn list(
        &self,
        conn: &Conn<O>,
        header: Header,
        infos: &[hex_api::BoxInfo],
    ) -> Result<(), Refusal> {
        let parts = match wire::boxes(header.version, header.id, infos, self.limits.message_bytes) {
            Ok(parts) => parts,
            Err(fault) => {
                return self.answer(
                    conn,
                    header.version,
                    header.id,
                    &Answer::Refused(Refusal::Broken(format!(
                        "the node agent's list couldn't be written: {fault}"
                    ))),
                );
            }
        };
        let deadline = Instant::now().checked_add(READ_PATIENCE);
        for (n, part) in parts.into_iter().enumerate() {
            self.send(conn, part, if n == 0 { None } else { deadline })?;
        }
        Ok(())
    }

    /// An exec: the program started with its stdio from what the request carried, then watched by the
    /// OS, announced, and its end sent after.
    fn exec(
        &self,
        conn: &Arc<Conn<O>>,
        header: Header,
        id: BoxId,
        program: Program,
        streams: Streams,
        passed: Vec<O::Passed>,
    ) -> Result<(), Refusal> {
        let reply = |answer: Answer| -> Result<(), Refusal> {
            self.done(conn)?;
            self.answer(conn, header.version, header.id, &answer)
        };
        // A place among the programs the connection watches, taken before the program starts, so
        // none starts that can't be watched.
        {
            let mut out = conn.out.lock().map_err(poisoned)?;
            let held = out.programs.len().checked_add(out.reserved);
            if held.is_none_or(|h| h >= self.limits.programs) {
                drop(out);
                return reply(Answer::Refused(Refusal::Exhausted {
                    what: PROGRAMS,
                    bound: self.limits.programs,
                }));
            }
            out.reserved = out.reserved.saturating_add(1);
        }
        let unreserve = || -> Result<(), Refusal> {
            let mut out = conn.out.lock().map_err(poisoned)?;
            out.reserved = out.reserved.saturating_sub(1);
            Ok(())
        };
        let proc = match self
            .os
            .stdio(streams, passed)
            .and_then(|stdio| self.host.exec(&id, program, stdio))
        {
            Ok(proc) => proc,
            Err(r) => {
                unreserve()?;
                return reply(Answer::Refused(r));
            }
        };
        let pid = proc.pid();
        // Announced and watched under the table, so the I/O thread can't send its end before its
        // start: it looks the program up under the same lock.
        let mut table = self.table()?;
        let token = table.token()?;
        let watch = match self.os.watch_program(pid, token) {
            Ok(watch) => watch,
            Err(Unwatched { why, killed }) => {
                drop(table);
                if killed {
                    let mut proc = proc;
                    let _ = proc.wait_for(STOP_PATIENCE);
                }
                unreserve()?;
                return reply(Answer::Refused(why));
            }
        };
        let open = {
            let mut out = conn.out.lock().map_err(poisoned)?;
            out.reserved = out.reserved.saturating_sub(1);
            out.in_flight = out.in_flight.saturating_sub(1);
            if out.open {
                out.programs.insert(header.id, token);
            }
            out.open
        };
        let mut watched = Watched {
            conn: Arc::clone(conn),
            exec: header.id,
            version: header.version,
            box_id: id,
            proc,
            watch: Some(watch),
            killed: false,
        };
        if !open {
            // Its connection closed while it started: ended by the disconnect rule.
            self.end(&mut watched);
        }
        table.watched.insert(token, watched);
        if open {
            self.answer(conn, header.version, header.id, &Answer::Started { pid })?;
        }
        Ok(())
    }

    /// The server stops: every connection closes, ending its programs, and the thread waits a bounded
    /// time for them to be reaped and counted before the workers finish the queue and go.
    fn stop(self: &Arc<Self>) {
        // Nothing new comes in, and the stop's own wake no longer wakes the wait below.
        self.os.stop_listening();
        let conns: Vec<Arc<Conn<O>>> = match self.table() {
            Ok(table) => table.conns.values().cloned().collect(),
            Err(_) => Vec::new(),
        };
        for conn in &conns {
            let _ = self.close(conn);
        }
        let deadline = Instant::now().checked_add(STOP_PATIENCE);
        let mut events = Vec::with_capacity(EVENTS);
        loop {
            let tokens: Vec<u64> = match self.table() {
                Ok(table) => table.watched.keys().copied().collect(),
                Err(_) => break,
            };
            if tokens.is_empty() {
                break;
            }
            let left = deadline.map_or(Duration::ZERO, |d| {
                d.saturating_duration_since(Instant::now())
            });
            if left.is_zero() {
                break;
            }
            events.clear();
            if self.os.wait(&mut events, Some(left)).is_err() {
                break;
            }
            for event in &events {
                if let Event::Ready { token, .. } = event
                    && tokens.contains(token)
                {
                    let _ = self.ended(*token);
                }
            }
        }
        self.stopping.store(true, Ordering::SeqCst);
        if let Ok(_jobs) = self.jobs.lock() {
            self.work.notify_all();
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests;
#[cfg(all(test, target_os = "macos"))]
mod macos_tests;
#[cfg(test)]
mod sim_tests;
