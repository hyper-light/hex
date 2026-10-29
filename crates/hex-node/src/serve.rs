//! The node agent's local socket (decision 3 of docs/design/runtime-architecture.md): the `Runtime`
//! seam served over a Unix `SOCK_SEQPACKET` socket to callers that don't link Hex, in
//! [`hex_core::wire`]'s format, and from its version 2 the node's network namespace counts
//! ([`Host::netns`]). An exec's stdio comes in as descriptors with its request
//! (`SCM_RIGHTS`, unix(7)), and a program's end goes out as a message after its exec's answer.
//!
//! One thread does every connection's I/O and watches every exec'd program's end, through one epoll
//! set over the listener, the connections and the programs' pidfds; it never blocks but in
//! `epoll_wait`. The runtime's calls, which may block (a delete waits for its box to empty), run on a
//! pool of workers, which answer their calls themselves.
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
//! still watches is sent KILL through its pidfd, reaped, and counted in its box ([`Host::abandoned`]),
//! which inspect reports; none is left running unwatched. A program the client released (it dropped
//! the `Proc`) runs on, as a dropped in-process `Proc` does. A request the agent has read is carried
//! out whether or not its client stays to take the answer; one it hadn't read is let go with the
//! connection.

use std::collections::{BTreeMap, VecDeque};
use std::io::{IoSliceMut, PipeReader, PipeWriter};
use std::mem::MaybeUninit;
use std::os::fd::OwnedFd;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hex_api::{BoxId, Exit, Proc, Program, Refusal, Runtime, Stdio};
use hex_core::netns;
use hex_core::wire::{self, Answer, Call, Header, Hello, Received, Request, Streams};
use rustix::buffer::spare_capacity;
use rustix::event::epoll::{self, CreateFlags, EventData, EventFlags};
use rustix::event::{EventfdFlags, PollFd, PollFlags, Timespec};
use rustix::fs::{FileType, OFlags};
use rustix::io::Errno;
use rustix::net::{
    AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags, SendFlags,
    Shutdown, SocketAddrUnix, SocketFlags, SocketType,
};
use rustix::process::{Pid, PidfdFlags, Signal};

pub use hex_core::wire::Limits;

/// The node agent's bounds on its socket unless set ([`Server::bind`]'s `limits`):
/// - 64 connections: a node's local callers are few (kiln's runner and executor, the command line),
///   so this is far past them while holding each connection's cost to a known size;
/// - messages of [`wire::MAX_MESSAGE`] bytes, the largest the format sends;
/// - 64 requests in flight on a connection: twice the bench's most threads sharing one client (32,
///   seam_oci's throughput phase), so a client's threads don't meet it in use;
/// - 1,024 programs watched by a connection, each holding a pidfd here: 64 connections' worth is
///   65,536 descriptors, the supervisor's [`crate::supervise::INIT_BUDGET`].
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
/// Most epoll events one wake takes; more come on the next.
const EVENTS: usize = 256;
/// Connections waiting to be accepted: the I/O thread accepts them as they come.
const BACKLOG: i32 = 64;
/// Longest a list's worker waits for its client to read one part before it sends the next; past it
/// the client has stopped reading, and its connection is closed. Five times the node's longest wait
/// of its own (a delete's [`crate::node::EMPTY_PATIENCE`]), so a client busy with another call isn't
/// taken for one that stopped.
const READ_PATIENCE: Duration = Duration::from_secs(300);
/// Longest a stopping server waits for the programs it ended to be reaped and recorded, as
/// [`crate::node::EMPTY_PATIENCE`]: a killed process ends in milliseconds.
const STOP_PATIENCE: Duration = Duration::from_secs(60);

/// The epoll tokens of the listener and the stop eventfd; connections and programs take the rest.
const LISTENER: u64 = 0;
const WAKE: u64 = 1;

/// What the socket needs of the runtime it serves, beyond the seam.
pub trait Host: Runtime {
    /// Counts in box `id` a program exec'd in it that the node ended because the connection watching
    /// it closed (the disconnect rule): reported as [`hex_api::BoxInfo::abandoned`].
    fn abandoned(&self, id: &BoxId) -> Result<(), Refusal>;

    /// The node's network namespace counts (decision 4), which a client of wire version 2 asks for.
    fn netns(&self) -> Result<netns::Counts, Refusal>;
}

/// The socket, served until dropped.
pub struct Server {
    shared: Arc<Shared>,
    io: Option<JoinHandle<()>>,
}

struct Shared {
    host: Arc<dyn Host>,
    /// The host's classes, asked once: a hello is answered on the I/O thread, which never calls the
    /// host.
    classes: Vec<hex_api::Class>,
    limits: Limits,
    workers: usize,
    path: PathBuf,
    epoll: OwnedFd,
    listener: OwnedFd,
    wake: OwnedFd,
    stopping: AtomicBool,
    table: Mutex<Table>,
    jobs: Mutex<Jobs>,
    work: Condvar,
    /// Why the I/O thread stopped, if it failed.
    fault: Mutex<Option<String>>,
}

/// The connections and the programs they watch, by epoll token. Locked before a connection's own
/// state when both are held.
#[derive(Default)]
struct Table {
    conns: BTreeMap<u64, Arc<Conn>>,
    watched: BTreeMap<u64, Watched>,
    next: u64,
    /// Accepting paused: the agent ran out of descriptors, so a closed connection or a reaped program
    /// resumes it.
    accept_paused: bool,
}

impl Table {
    fn token(&mut self) -> Result<u64, Refusal> {
        let token = self.next.max(WAKE.saturating_add(1));
        self.next = token
            .checked_add(1)
            .ok_or_else(|| Refusal::Broken("the socket's epoll tokens ran out".to_owned()))?;
        Ok(token)
    }
}

struct Conn {
    token: u64,
    fd: OwnedFd,
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
    /// Whether its requests are read (epoll's IN) or only its backlog written (OUT).
    reading: bool,
}

/// A program a connection's exec started, watched by its pidfd.
struct Watched {
    conn: Arc<Conn>,
    /// Its exec's id and version, which its end is sent with.
    exec: u64,
    version: u16,
    box_id: BoxId,
    proc: Box<dyn Proc>,
    pidfd: OwnedFd,
    /// Sent KILL because its connection closed: its end is counted in its box, not sent.
    killed: bool,
}

#[derive(Default)]
struct Jobs {
    queue: VecDeque<Job>,
    workers: usize,
    idle: usize,
}

enum Job {
    Call {
        conn: Arc<Conn>,
        header: Header,
        call: Call,
        fds: Vec<OwnedFd>,
    },
    Abandoned(BoxId),
}

fn poisoned<T>(_: std::sync::PoisonError<T>) -> Refusal {
    Refusal::Broken("a socket lock was poisoned".to_owned())
}

impl Server {
    /// Serves `host` on a socket at `path`, readable and writable by this user alone, holding
    /// `limits` and running calls on at most `workers` threads. A socket left at `path` by an agent
    /// that has ended is replaced; one an agent still serves, or a file that isn't a socket, is
    /// refused.
    pub fn bind(
        path: &Path,
        host: Arc<dyn Host>,
        limits: Limits,
        workers: usize,
    ) -> Result<Self, Refusal> {
        check_limits(&limits, workers)?;
        clear(path)?;
        let addr = SocketAddrUnix::new(path).map_err(|e| Refusal::Invalid {
            what: "socket path",
            why: format!("{}: {e}", path.display()),
        })?;
        let listener = rustix::net::socket_with(
            AddressFamily::UNIX,
            SocketType::SEQPACKET,
            SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
            None,
        )
        .map_err(|e| Refusal::os("making the node agent's socket", e))?;
        rustix::net::bind(&listener, &addr).map_err(|e| Refusal::Os {
            op: "binding the node agent's socket",
            why: format!("{}: {e}", path.display()),
        })?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| Refusal::os("restricting the node agent's socket to its user", e))?;
        rustix::net::listen(&listener, BACKLOG)
            .map_err(|e| Refusal::os("listening on the node agent's socket", e))?;
        let epoll = epoll::create(CreateFlags::CLOEXEC)
            .map_err(|e| Refusal::os("making the socket's epoll set", e))?;
        let wake = rustix::event::eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)
            .map_err(|e| Refusal::os("making the socket's stop eventfd", e))?;
        for (fd, token) in [(&listener, LISTENER), (&wake, WAKE)] {
            epoll::add(&epoll, fd, EventData::new_u64(token), EventFlags::IN)
                .map_err(|e| Refusal::os("watching the node agent's socket", e))?;
        }
        let shared = Arc::new(Shared {
            classes: host.classes(),
            host,
            limits,
            workers,
            path: path.to_owned(),
            epoll,
            listener,
            wake,
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

impl Drop for Server {
    /// Stops serving: the listener and every connection close, their programs are ended by the
    /// disconnect rule, and the socket's file is removed.
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        let _ = rustix::io::write(&self.shared.wake, &1u64.to_ne_bytes());
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

/// Makes room for a socket at `path`: a socket no agent serves (one that ended) is removed; one an
/// agent serves, or anything that isn't a socket, is refused.
fn clear(path: &Path) -> Result<(), Refusal> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(Refusal::os("looking at the node agent's socket path", e)),
    };
    if !meta.file_type().is_socket() {
        return Err(Refusal::Invalid {
            what: "socket path",
            why: format!("{} exists and isn't a socket", path.display()),
        });
    }
    let probe = rustix::net::socket_with(
        AddressFamily::UNIX,
        SocketType::SEQPACKET,
        SocketFlags::CLOEXEC,
        None,
    )
    .map_err(|e| Refusal::os("probing the node agent's socket", e))?;
    let addr = SocketAddrUnix::new(path).map_err(|e| Refusal::Invalid {
        what: "socket path",
        why: format!("{}: {e}", path.display()),
    })?;
    match rustix::net::connect(&probe, &addr) {
        Ok(()) => Err(Refusal::Invalid {
            what: "socket path",
            why: format!("{} is served by a node agent already", path.display()),
        }),
        Err(Errno::CONNREFUSED) => std::fs::remove_file(path)
            .map_err(|e| Refusal::os("removing an ended node agent's socket", e)),
        Err(e) => Err(Refusal::Os {
            op: "probing the node agent's socket",
            why: format!("{}: {e}", path.display()),
        }),
    }
}

impl Shared {
    fn table(&self) -> Result<MutexGuard<'_, Table>, Refusal> {
        self.table.lock().map_err(poisoned)
    }

    /// The I/O thread: until the server stops, or fails (every connection then closes, so no client
    /// waits on a server that is gone).
    fn run(self: &Arc<Self>) {
        let mut buf = vec![0u8; self.limits.message_bytes];
        let mut events = Vec::with_capacity(EVENTS);
        let outcome = loop {
            events.clear();
            match epoll::wait(&self.epoll, spare_capacity(&mut events), None) {
                Ok(_) | Err(Errno::INTR) => {}
                Err(e) => break Err(Refusal::os("waiting on the socket's epoll set", e)),
            }
            if self.stopping.load(Ordering::SeqCst) {
                break Ok(());
            }
            let mut failed = None;
            for event in &events {
                if let Err(e) = self.event(event.data.u64(), event.flags, &mut buf) {
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

    fn event(
        self: &Arc<Self>,
        token: u64,
        flags: EventFlags,
        buf: &mut [u8],
    ) -> Result<(), Refusal> {
        match token {
            LISTENER => self.accept(),
            WAKE => Ok(()),
            _ => {
                let (conn, watched) = {
                    let table = self.table()?;
                    (
                        table.conns.get(&token).cloned(),
                        table.watched.contains_key(&token),
                    )
                };
                if let Some(conn) = conn {
                    self.connection(&conn, flags, buf)
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
            let fd = match rustix::net::accept_with(
                &self.listener,
                SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
            ) {
                Ok(fd) => fd,
                Err(Errno::AGAIN) => return Ok(()),
                Err(Errno::INTR | Errno::CONNABORTED) => continue,
                // Out of descriptors: the caller waits in the backlog until a connection closes or a
                // program ends, which resumes accepting (the listener stays readable meanwhile).
                Err(Errno::MFILE | Errno::NFILE | Errno::NOBUFS | Errno::NOMEM) => {
                    let mut table = self.table()?;
                    table.accept_paused = true;
                    epoll::modify(
                        &self.epoll,
                        &self.listener,
                        EventData::new_u64(LISTENER),
                        EventFlags::empty(),
                    )
                    .map_err(|e| Refusal::os("pausing the node agent's socket", e))?;
                    return Ok(());
                }
                Err(e) => return Err(Refusal::os("accepting on the node agent's socket", e)),
            };
            self.admit(fd)?;
        }
    }

    fn admit(self: &Arc<Self>, fd: OwnedFd) -> Result<(), Refusal> {
        let me = rustix::process::geteuid();
        let peer = match rustix::net::sockopt::socket_peercred(&fd) {
            Ok(peer) => peer,
            // Gone before it could be asked who it is.
            Err(_) => return Ok(()),
        };
        if peer.uid != me && !peer.uid.is_root() {
            refuse(
                &fd,
                Refusal::Invalid {
                    what: "caller",
                    why: format!(
                        "uid {} isn't the node agent's user ({})",
                        peer.uid.as_raw(),
                        me.as_raw()
                    ),
                },
            );
            return Ok(());
        }
        let mut table = self.table()?;
        if table.conns.len() >= self.limits.connections {
            drop(table);
            refuse(
                &fd,
                Refusal::Exhausted {
                    what: CONNECTIONS,
                    bound: self.limits.connections,
                },
            );
            return Ok(());
        }
        let token = table.token()?;
        if let Err(e) = epoll::add(
            &self.epoll,
            &fd,
            EventData::new_u64(token),
            EventFlags::IN | EventFlags::RDHUP,
        ) {
            // This connection can't be watched (the kernel's epoll watches are spent): refused, and
            // the ones held go on being served.
            drop(table);
            refuse(&fd, Refusal::os("watching a connection", e));
            return Ok(());
        }
        table.conns.insert(
            token,
            Arc::new(Conn {
                token,
                fd,
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

    fn connection(
        self: &Arc<Self>,
        conn: &Arc<Conn>,
        flags: EventFlags,
        buf: &mut [u8],
    ) -> Result<(), Refusal> {
        // Closed by its client: what it sent before it went is read and carried out (a release among
        // it lets its program go), then the connection closes. Its answers can't be taken.
        let hung_up = flags.intersects(EventFlags::HUP | EventFlags::ERR);
        if flags.contains(EventFlags::OUT) && !hung_up {
            self.flush(conn)?;
        }
        let ended = if hung_up || flags.intersects(EventFlags::IN | EventFlags::RDHUP) {
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
        conn: &Arc<Conn>,
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
            let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(wire::MAX_FDS))];
            let mut control = RecvAncillaryBuffer::new(&mut space);
            let got = rustix::net::recvmsg(
                &conn.fd,
                &mut [IoSliceMut::new(buf)],
                &mut control,
                RecvFlags::DONTWAIT | RecvFlags::CMSG_CLOEXEC | RecvFlags::TRUNC,
            );
            let msg = match got {
                Ok(msg) => msg,
                Err(Errno::AGAIN) => return Ok(gone),
                Err(Errno::INTR) => continue,
                // A client that closed with answers unread leaves this error, reported once and
                // before what it sent (net/unix/af_unix.c, `unix_release_sock`), which is still
                // there to read.
                Err(Errno::CONNRESET) if !reset => {
                    reset = true;
                    gone = true;
                    conn.out.lock().map_err(poisoned)?.backlog.clear();
                    continue;
                }
                Err(_) => return Ok(true),
            };
            let fds: Vec<OwnedFd> = control
                .drain()
                .filter_map(|m| match m {
                    RecvAncillaryMessage::ScmRights(fds) => Some(fds),
                    _ => None,
                })
                .flatten()
                .collect();
            // A read of nothing is the client's end, unless it sent an empty message on purpose,
            // which is refused as one.
            if msg.bytes == 0 && fds.is_empty() && (gone || hung_up(&conn.fd)) {
                return Ok(true);
            }
            let bytes = buf.get(..msg.bytes.min(buf.len())).unwrap_or_default();
            let received = Received {
                bytes,
                size: msg.bytes,
                descriptors: fds.len(),
                descriptors_cut: msg.flags.contains(ReturnFlags::CTRUNC),
            };
            self.request(conn, &received, fds)?;
        }
    }

    fn request(
        self: &Arc<Self>,
        conn: &Arc<Conn>,
        msg: &Received<'_>,
        fds: Vec<OwnedFd>,
    ) -> Result<(), Refusal> {
        match wire::read_request(msg, self.limits.message_bytes) {
            Err(bad) => {
                drop(fds);
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
                    call,
                    fds,
                })
            }
        }
    }

    /// Sends one answer on a connection, or keeps it until there is room; an answer that can't be
    /// written is refused to its request in its place.
    fn answer(&self, conn: &Conn, version: u16, id: u64, answer: &Answer) -> Result<(), Refusal> {
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
    fn send(&self, conn: &Conn, bytes: Vec<u8>, deadline: Option<Instant>) -> Result<(), Refusal> {
        let mut out = conn.out.lock().map_err(poisoned)?;
        if let Some(deadline) = deadline {
            while out.open && !out.backlog.is_empty() {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    drop(out);
                    // The I/O thread sees the hang-up and closes it, by the disconnect rule.
                    let _ = rustix::net::shutdown(&conn.fd, Shutdown::Both);
                    return Ok(());
                }
                out = conn.drained.wait_timeout(out, left).map_err(poisoned)?.0;
            }
        }
        if !out.open {
            return Ok(());
        }
        if out.backlog.is_empty() {
            loop {
                match rustix::net::send(&conn.fd, &bytes, SendFlags::DONTWAIT | SendFlags::NOSIGNAL)
                {
                    Ok(_) => return Ok(()),
                    Err(Errno::INTR) => continue,
                    Err(Errno::AGAIN) => break,
                    // Its client is gone: the I/O thread sees the hang-up and closes it.
                    Err(_) => return Ok(()),
                }
            }
        }
        out.backlog.push_back(bytes);
        if out.reading {
            out.reading = false;
            epoll::modify(
                &self.epoll,
                &conn.fd,
                EventData::new_u64(conn.token),
                EventFlags::OUT | EventFlags::RDHUP,
            )
            .map_err(|e| Refusal::os("waiting for room in a connection", e))?;
        }
        Ok(())
    }

    /// Writes a connection's waiting answers while there is room; once all are out its requests are
    /// read again.
    fn flush(&self, conn: &Conn) -> Result<(), Refusal> {
        let mut out = conn.out.lock().map_err(poisoned)?;
        while let Some(front) = out.backlog.front() {
            match rustix::net::send(&conn.fd, front, SendFlags::DONTWAIT | SendFlags::NOSIGNAL) {
                Ok(_) => {
                    out.backlog.pop_front();
                }
                Err(Errno::INTR) => {}
                Err(Errno::AGAIN) => return Ok(()),
                Err(_) => {
                    // Its client is gone: the hang-up closes it.
                    out.backlog.clear();
                    break;
                }
            }
        }
        if out.open && !out.reading {
            out.reading = true;
            epoll::modify(
                &self.epoll,
                &conn.fd,
                EventData::new_u64(conn.token),
                EventFlags::IN | EventFlags::RDHUP,
            )
            .map_err(|e| Refusal::os("reading a connection again", e))?;
        }
        conn.drained.notify_all();
        Ok(())
    }

    /// Closes a connection and applies the disconnect rule to the programs it still watches.
    fn close(&self, conn: &Conn) -> Result<(), Refusal> {
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
        let _ = epoll::delete(&self.epoll, &conn.fd);
        let _ = rustix::net::shutdown(&conn.fd, Shutdown::Both);
        table.conns.remove(&conn.token);
        for token in programs {
            if let Some(w) = table.watched.get_mut(&token) {
                end(w);
            }
        }
        self.resume_accepting(&mut table)
    }

    /// The client no longer waits for exec `exec`'s program: it runs on, unwatched by the socket,
    /// and the node's sweep reaps it once it ends, as with a dropped in-process `Proc`.
    fn release(&self, conn: &Conn, exec: u64) -> Result<(), Refusal> {
        let mut table = self.table()?;
        let token = conn.out.lock().map_err(poisoned)?.programs.remove(&exec);
        if let Some(token) = token {
            // Dropping the watch closes its pidfd, which leaves the epoll set, and lets the program go.
            table.watched.remove(&token);
        }
        self.resume_accepting(&mut table)
    }

    /// A watched program's pidfd turned readable: it has ended. Reaped, and its end sent to its
    /// connection, or counted in its box when the connection closed (the disconnect rule).
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
            // Readable but not yet waitable can't happen (pidfd_open(2)); watched again rather than
            // lost.
            Ok(None) => {
                let mut table = self.table()?;
                table.watched.insert(token, w);
                return Ok(());
            }
            // Reaped elsewhere in this process: how it ended is no longer known.
            Err(_) => Exit::Unknown,
        };
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

    fn resume_accepting(&self, table: &mut Table) -> Result<(), Refusal> {
        if table.accept_paused {
            table.accept_paused = false;
            epoll::modify(
                &self.epoll,
                &self.listener,
                EventData::new_u64(LISTENER),
                EventFlags::IN,
            )
            .map_err(|e| Refusal::os("resuming the node agent's socket", e))?;
        }
        Ok(())
    }

    /// Hands a job to a worker, starting one when none is idle and fewer than the bound run.
    fn enqueue(self: &Arc<Self>, job: Job) -> Result<(), Refusal> {
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
                    fds,
                } => {
                    let _ = self.call(&conn, header, call, fds);
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
        conn: &Arc<Conn>,
        header: Header,
        call: Call,
        fds: Vec<OwnedFd>,
    ) -> Result<(), Refusal> {
        let host = &self.host;
        let answer = match call {
            Call::Exec {
                id,
                program,
                streams,
            } => return self.exec(conn, header, id, program, streams, fds),
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
        };
        self.done(conn)?;
        let answer = answer.unwrap_or_else(Answer::Refused);
        self.answer(conn, header.version, header.id, &answer)
    }

    fn done(&self, conn: &Conn) -> Result<(), Refusal> {
        let mut out = conn.out.lock().map_err(poisoned)?;
        out.in_flight = out.in_flight.saturating_sub(1);
        Ok(())
    }

    /// A list's answer, in as many parts as its boxes need, each sent once the one before has gone.
    fn list(&self, conn: &Conn, header: Header, infos: &[hex_api::BoxInfo]) -> Result<(), Refusal> {
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

    /// An exec: the program started with its stdio from the request's descriptors, then watched by
    /// its pidfd, announced, and its end sent after.
    fn exec(
        &self,
        conn: &Arc<Conn>,
        header: Header,
        id: BoxId,
        program: Program,
        streams: Streams,
        fds: Vec<OwnedFd>,
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
        let started = stdio_of(streams, fds)
            .and_then(|stdio| self.host.exec(&id, program, stdio))
            .and_then(|proc| {
                let pidfd = i32::try_from(proc.pid())
                    .ok()
                    .and_then(Pid::from_raw)
                    .ok_or_else(|| Refusal::Broken(format!("a program's pid {} isn't one", proc.pid())))
                    .and_then(|pid| {
                        rustix::process::pidfd_open(pid, PidfdFlags::empty()).map_err(|e| Refusal::Os {
                            op: "watching a started program",
                            why: format!(
                                "{e}: it runs on unwatched by the socket, and the node's sweep reaps it"
                            ),
                        })
                    })?;
                Ok((proc, pidfd))
            });
        let (proc, pidfd) = match started {
            Ok(p) => p,
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
        if let Err(e) = epoll::add(
            &self.epoll,
            &pidfd,
            EventData::new_u64(token),
            EventFlags::IN,
        ) {
            drop(table);
            // It can't be watched: ended now, and the exec refused saying so.
            let _ = rustix::process::pidfd_send_signal(&pidfd, Signal::KILL);
            let mut proc = proc;
            let _ = proc.wait_for(STOP_PATIENCE);
            unreserve()?;
            return reply(Answer::Refused(Refusal::Os {
                op: "watching a started program",
                why: format!("{e}: the node agent ended it"),
            }));
        }
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
            pidfd,
            killed: false,
        };
        if !open {
            // Its connection closed while it started: ended by the disconnect rule.
            end(&mut watched);
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
        // Nothing new comes in, and the stop's own wake (never read) no longer wakes the wait below.
        let _ = epoll::delete(&self.epoll, &self.listener);
        let _ = epoll::delete(&self.epoll, &self.wake);
        let conns: Vec<Arc<Conn>> = match self.table() {
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
            let ts = Timespec {
                tv_sec: i64::try_from(left.as_secs()).unwrap_or(i64::MAX),
                tv_nsec: i64::from(left.subsec_nanos()),
            };
            events.clear();
            if epoll::wait(&self.epoll, spare_capacity(&mut events), Some(&ts)).is_err() {
                break;
            }
            for event in &events {
                let token = event.data.u64();
                if tokens.contains(&token) {
                    let _ = self.ended(token);
                }
            }
        }
        self.stopping.store(true, Ordering::SeqCst);
        if let Ok(_jobs) = self.jobs.lock() {
            self.work.notify_all();
        }
    }
}

/// Ends a program whose connection closed: KILL through its pidfd, so it can't reach a process given
/// its pid since; its end, reaped when its pidfd turns readable, is counted in its box.
fn end(w: &mut Watched) {
    // Fails only for one that has ended already, which is reaped and counted just the same.
    let _ = rustix::process::pidfd_send_signal(&w.pidfd, Signal::KILL);
    w.killed = true;
}

/// Whether a socket's peer has gone: a read of nothing then is its end, not an empty message.
fn hung_up(fd: &OwnedFd) -> bool {
    let mut fds = [PollFd::new(fd, PollFlags::RDHUP | PollFlags::HUP)];
    let zero = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    match rustix::event::poll(&mut fds, Some(&zero)) {
        Ok(_) => fds
            .first()
            .is_some_and(|f| f.revents().intersects(PollFlags::RDHUP | PollFlags::HUP)),
        Err(_) => true,
    }
}

/// Answers a connection that won't be served with why, as id 0 (its client hasn't asked anything
/// yet), in the first version, which a client of any version reads, then lets it go.
fn refuse(fd: &OwnedFd, refusal: Refusal) {
    if let Ok(bytes) = wire::answer(wire::FIRST, 0, &Answer::Refused(refusal), wire::MAX_MESSAGE) {
        let _ = rustix::net::send(fd, &bytes, SendFlags::DONTWAIT | SendFlags::NOSIGNAL);
    }
}

/// A program's stdio from the descriptors its exec carried, in the order `streams` names them: each
/// the right end of a pipe, as the seam's `Stdio` holds; anything else is refused by name.
fn stdio_of(streams: Streams, fds: Vec<OwnedFd>) -> Result<Stdio, Refusal> {
    let mut fds = fds.into_iter();
    let mut take = |on: bool, name: &str, reads: bool| -> Result<Option<OwnedFd>, Refusal> {
        if !on {
            return Ok(None);
        }
        let fd = fds.next().ok_or_else(|| Refusal::Invalid {
            what: "stdio",
            why: format!("{name}'s descriptor didn't come"),
        })?;
        pipe_end(&fd, name, reads)?;
        Ok(Some(fd))
    };
    let stdin = take(streams.stdin, "stdin", true)?.map(PipeReader::from);
    let stdout = take(streams.stdout, "stdout", false)?.map(PipeWriter::from);
    let stderr = take(streams.stderr, "stderr", false)?.map(PipeWriter::from);
    Ok(Stdio {
        stdin,
        stdout,
        stderr,
    })
}

/// That `fd` is a pipe's end the program can read (`reads`) or write.
fn pipe_end(fd: &OwnedFd, name: &str, reads: bool) -> Result<(), Refusal> {
    let invalid = |why: String| Refusal::Invalid {
        what: "stdio",
        why: format!("{name} {why}"),
    };
    let stat = rustix::fs::fstat(fd).map_err(|e| invalid(format!("can't be looked at: {e}")))?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Fifo {
        return Err(invalid("isn't a pipe".to_owned()));
    }
    let mode = rustix::fs::fcntl_getfl(fd)
        .map_err(|e| invalid(format!("can't be looked at: {e}")))?
        & OFlags::RWMODE;
    let fits = mode == OFlags::RDWR
        || mode
            == if reads {
                OFlags::RDONLY
            } else {
                OFlags::WRONLY
            };
    if fits {
        Ok(())
    } else if reads {
        Err(invalid(
            "is a pipe's write end, which a program can't read".to_owned(),
        ))
    } else {
        Err(invalid(
            "is a pipe's read end, which a program can't write".to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests;
