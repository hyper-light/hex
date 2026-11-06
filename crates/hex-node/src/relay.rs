//! The guest's relay (docs/design/shards-guest.md §3): in a Shards guest, it takes the host's
//! connections and speaks the node agent's own socket, so the node, its wire and its disconnect rule stay
//! as they are on any Linux host (CLAUDE.md §8).
//!
//! - **Control:** each host connection is sent its token ([`stream::TOKEN_BYTES`]) and given a node
//!   connection of its own. Its framed requests go to the node one message each; the node's answers go
//!   back framed. When either side ends, so does the other: the node's disconnect rule then ends the
//!   programs the host left.
//! - **Stdio:** an exec's streams come on connections of their own, named by their first bytes. The relay
//!   gathers them ([`Gather`]), makes a pipe for each, passes the pipe's end to the node with the exec,
//!   and copies between the pipe and the connection, one thread and one buffer per stream.
//!
//! Each resource has a bound, refused past by type (CLAUDE.md §2, §3):
//! - control connections ([`Limits::connections`], the node's own): one more is sent its token and a
//!   refusal (`Exhausted`, as the node's socket refuses one) and closed;
//! - execs and early streams waiting for each other ([`Limits::sets`]): one more is refused, the exec
//!   answered `Invalid` naming the bound, the stream closed;
//! - a set's patience ([`Limits::patience`]): an exec whose streams don't all come in it is answered
//!   `Invalid`, naming them, and the streams that came are closed;
//! - a stdio connection's name ([`NAME_PATIENCE`]): one that doesn't name itself in it is closed;
//! - staging connections at once (the control connections' bound again): one more is answered
//!   `Exhausted` and closed; a stage's entries, bytes and path lengths ([`Limits::files`]).
//!
//! - **Files:** a box's bundle and root come on the staging port before its create
//!   (docs/design/shards-guest.md §4a, [`files`]), written under the staged folder, and go with it.

use std::collections::BTreeMap;
use std::io::{self, IoSlice, IoSliceMut, PipeReader, PipeWriter, Read, Write};
use std::mem::MaybeUninit;
use std::net::Shutdown;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use hex_api::Refusal;
use hex_client::guest::Duplex;
use hex_core::stream::{self, Complete, Deframer, Exec, Gather, Stray};
use hex_core::wire::{self, Answer, Call, Request};
use rustix::io::Errno;
use rustix::net::{
    AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer,
    SendAncillaryMessage, SendFlags, SocketAddrUnix, SocketFlags, SocketType,
};

/// The relay's bounds.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Host control connections at once.
    pub connections: usize,
    /// Execs and early streams waiting for each other at once.
    pub sets: usize,
    /// Longest an exec waits for its streams.
    pub patience: Duration,
    /// A stage's entries, bytes and path lengths.
    pub files: hex_core::stream::files::Limits,
}

/// The relay's bounds unless set:
/// - the node socket's own connections (`crate::serve::LIMITS`), as each holds one of the node's;
/// - a connection's in-flight bound for every connection, so no host that holds the node's bounds meets
///   the sets' bound;
/// - a minute's patience: the host's client dials an exec's streams before it sends the exec, so they
///   are there unless a dial failed, and a minute is past any vsock connect (Linux gives one 2 s,
///   `VSOCK_DEFAULT_CONNECT_TIMEOUT`, net/vmw_vsock/af_vsock.c) many times over.
pub const LIMITS: Limits = Limits {
    connections: crate::serve::LIMITS.connections,
    sets: crate::serve::LIMITS.connections * crate::serve::LIMITS.in_flight,
    patience: Duration::from_secs(60),
    files: hex_core::stream::files::LIMITS,
};
/// Where the guest's relay writes staged bundles and roots: the guest's tmpfs `/run`, charged to its
/// memory (docs/design/shards-guest.md §4).
pub const STAGED: &str = "/run/hex/staged";
/// Longest a stdio connection takes to name itself: the host writes its name as soon as it connects.
pub const NAME_PATIENCE: Duration = Duration::from_secs(5);
/// Each stdio copier's stack: one copy buffer and little else.
const COPIER_STACK: usize = 64 << 10;

/// What accepts the host's connections: AF_VSOCK in the guest, a Unix socket in tests.
pub trait Listen: Send + Sync + 'static {
    fn accept(&self) -> io::Result<Box<dyn Duplex>>;
}

impl Listen for std::os::unix::net::UnixListener {
    fn accept(&self) -> io::Result<Box<dyn Duplex>> {
        Ok(Box::new(std::os::unix::net::UnixListener::accept(self)?.0))
    }
}

/// The relay's state, shared by its threads.
pub struct Relay {
    node: PathBuf,
    staged: PathBuf,
    limits: Limits,
    sets: Mutex<Sets>,
    came: Condvar,
    tokens: AtomicU64,
    connections: AtomicUsize,
    stagings: AtomicUsize,
}

/// Sets under way, and those made whole by a stream, waiting for their exec's thread to take them.
struct Sets {
    gather: Gather<Box<dyn Duplex>>,
    whole: BTreeMap<Exec, Vec<Box<dyn Duplex>>>,
}

fn poisoned<T>(_: std::sync::PoisonError<T>) -> Refusal {
    Refusal::Broken("a lock of the guest's relay was poisoned".to_owned())
}

impl Relay {
    /// A relay to the node agent's socket at `node`, staging under `staged`, holding `limits`.
    pub fn new(node: &Path, staged: &Path, limits: Limits) -> Arc<Self> {
        Arc::new(Self {
            node: node.to_owned(),
            staged: staged.to_owned(),
            limits,
            sets: Mutex::new(Sets {
                gather: Gather::new(limits.sets),
                whole: BTreeMap::new(),
            }),
            came: Condvar::new(),
            tokens: AtomicU64::new(0),
            connections: AtomicUsize::new(0),
            stagings: AtomicUsize::new(0),
        })
    }

    fn sets(&self) -> Result<MutexGuard<'_, Sets>, Refusal> {
        self.sets.lock().map_err(poisoned)
    }

    /// Serves `control`, `stdio` and `files` until a listener fails, which is returned.
    pub fn serve(
        self: &Arc<Self>,
        control: Box<dyn Listen>,
        stdio: Box<dyn Listen>,
        files: Box<dyn Listen>,
    ) -> Result<(), Refusal> {
        let streams = {
            let relay = Arc::clone(self);
            std::thread::Builder::new()
                .name("hex-relay-stdio".to_owned())
                .spawn(move || relay.take_streams(stdio.as_ref()))
                .map_err(|e| Refusal::os("starting the relay's stdio thread", e))?
        };
        let stagings = {
            let relay = Arc::clone(self);
            std::thread::Builder::new()
                .name("hex-relay-files".to_owned())
                .spawn(move || relay.take_stagings(files.as_ref()))
                .map_err(|e| Refusal::os("starting the relay's staging thread", e))?
        };
        let why = loop {
            let conn = match control.accept() {
                Ok(conn) => conn,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => break Refusal::os("accepting a host's control connection", e),
            };
            let relay = Arc::clone(self);
            if let Err(e) = std::thread::Builder::new()
                .name("hex-relay-control".to_owned())
                .spawn(move || relay.control(conn))
            {
                break Refusal::os("starting a control connection's thread", e);
            }
        };
        drop((streams, stagings));
        Err(why)
    }

    /// Takes staging connections as they come, each on a thread of its own, at most the control
    /// connections' bound at once.
    fn take_stagings(self: &Arc<Self>, listener: &dyn Listen) -> Refusal {
        loop {
            let mut conn = match listener.accept() {
                Ok(conn) => conn,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Refusal::os("accepting a host's staging connection", e),
            };
            let held = self.stagings.fetch_add(1, Ordering::AcqRel);
            if held >= self.limits.connections {
                self.stagings.fetch_sub(1, Ordering::AcqRel);
                refuse(
                    conn.as_mut(),
                    Refusal::Exhausted {
                        what: "staging connections",
                        bound: self.limits.connections,
                    },
                );
                continue;
            }
            let relay = Arc::clone(self);
            let started = std::thread::Builder::new()
                .name("hex-relay-stage".to_owned())
                .spawn(move || {
                    let _release = Release(&relay.stagings);
                    files::serve(conn, &relay.staged, &relay.limits.files);
                });
            if let Err(e) = started {
                return Refusal::os("starting a staging connection's thread", e);
            }
        }
    }

    /// Takes stdio connections as they come, each to its exec's set.
    fn take_streams(&self, stdio: &dyn Listen) -> Refusal {
        loop {
            let mut conn = match stdio.accept() {
                Ok(conn) => conn,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Refusal::os("accepting a host's stdio connection", e),
            };
            let mut name = [0u8; stream::NAME_BYTES];
            let named = conn
                .set_read_timeout(Some(NAME_PATIENCE))
                .and_then(|()| conn.read_exact(&mut name))
                .and_then(|()| conn.set_read_timeout(None))
                .ok()
                .and_then(|()| stream::read_name(&name).ok());
            // One that doesn't name itself in time, or names nothing, is closed as it drops: the exec
            // it was for is answered when its patience ends.
            let Some(named) = named else { continue };
            let Ok(mut sets) = self.sets() else { continue };
            match sets.gather.arrive(named, conn, Instant::now()) {
                Ok(Some((exec, conns))) => {
                    sets.whole.insert(exec, conns);
                    self.came.notify_all();
                }
                Ok(None) => {}
                // Closed as it drops; its exec, if it comes, waits out its patience and is refused.
                Err(Stray { .. }) => {}
            }
        }
    }

    /// Serves one host control connection until either side ends.
    fn control(self: Arc<Self>, mut host: Box<dyn Duplex>) {
        let token = self.tokens.fetch_add(1, Ordering::Relaxed);
        if host.write_all(&token.to_le_bytes()).is_err() {
            return;
        }
        let held = self.connections.fetch_add(1, Ordering::AcqRel);
        let _release = Release(&self.connections);
        if held >= self.limits.connections {
            refuse(
                host.as_mut(),
                Refusal::Exhausted {
                    what: crate::serve::CONNECTIONS,
                    bound: self.limits.connections,
                },
            );
            return;
        }
        let node = match connect(&self.node) {
            Ok(node) => Arc::new(node),
            Err(why) => {
                refuse(host.as_mut(), why);
                return;
            }
        };
        let Ok(writer) = host.try_clone() else { return };
        let writer = Arc::new(Mutex::new(writer));
        let answers = {
            let (node, writer) = (Arc::clone(&node), Arc::clone(&writer));
            std::thread::Builder::new()
                .name("hex-relay-answers".to_owned())
                .spawn(move || answers(&node, &writer))
        };
        let Ok(answers) = answers else { return };
        self.requests(token, host.as_mut(), &node, &writer);
        // The host has gone: the node sees its connection end and applies its disconnect rule.
        let _ = rustix::net::shutdown(&*node, rustix::net::Shutdown::Both);
        let _ = answers.join();
    }

    /// The host's requests, each to the node, until the host's connection ends.
    fn requests(
        &self,
        token: u64,
        host: &mut dyn Duplex,
        node: &OwnedFd,
        writer: &Mutex<Box<dyn Duplex>>,
    ) {
        let mut deframer = Deframer::new(wire::MAX_MESSAGE);
        let mut buf = vec![0u8; 64 << 10];
        loop {
            let n = match host.read(&mut buf) {
                Ok(0) => return,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => return,
            };
            let mut input = buf.get(..n).unwrap_or_default();
            while !input.is_empty() {
                let Ok((took, whole)) = deframer.take(input) else {
                    // Bytes that aren't a frame: the connection can't be read further, so it ends.
                    return;
                };
                input = input.get(took..).unwrap_or_default();
                let Some(message) = whole else { continue };
                if self.request(token, &message, node, writer).is_err() {
                    return;
                }
            }
        }
    }

    /// One request to the node: an exec with its streams' pipes, anything else as it came (the node
    /// refuses what isn't a request itself).
    fn request(
        &self,
        token: u64,
        message: &[u8],
        node: &OwnedFd,
        writer: &Mutex<Box<dyn Duplex>>,
    ) -> Result<(), Gone> {
        let exec = match wire::read_request_apart(message, wire::MAX_MESSAGE) {
            Ok((header, Request::Call(Call::Exec { streams, .. }))) => Some((header, streams)),
            _ => None,
        };
        let Some((header, streams)) = exec else {
            return send(node, message, &[]);
        };
        let exec = Exec {
            conn: token,
            id: header.id,
        };
        let stdio = self
            .streams_of(exec, streams)
            .and_then(|conns| pipes(streams, conns));
        match stdio {
            Ok(fds) => send(node, message, &fds),
            Err(why) => {
                let answer = wire::answer(
                    header.version,
                    header.id,
                    &Answer::Refused(why),
                    wire::MAX_MESSAGE,
                )
                .ok()
                .and_then(|bytes| stream::frame(&bytes, wire::MAX_MESSAGE).ok());
                let mut w = writer.lock().map_err(|_| Gone)?;
                w.write_all(&answer.unwrap_or_default()).map_err(|_| Gone)
            }
        }
    }

    /// The connections of `exec`'s streams, once all have come, or why not.
    fn streams_of(
        &self,
        exec: Exec,
        streams: wire::Streams,
    ) -> Result<Vec<Box<dyn Duplex>>, Refusal> {
        let start = Instant::now();
        let mut sets = self.sets()?;
        match sets.gather.expect(exec, streams, start) {
            Ok(Some((_, conns))) => return Ok(conns),
            Ok(None) => {}
            Err(refused) => {
                return Err(Refusal::Invalid {
                    what: "stdio",
                    why: format!("the exec's streams were refused: {}", refused.why),
                });
            }
        }
        loop {
            if let Some(conns) = sets.whole.remove(&exec) {
                return Ok(conns);
            }
            let patience = self.limits.patience;
            let left = patience.saturating_sub(start.elapsed());
            if left.is_zero() {
                let late: Vec<Complete<Box<dyn Duplex>>> =
                    sets.gather.expire(Instant::now(), patience);
                let came = late
                    .iter()
                    .find(|(e, _)| *e == exec)
                    .map_or(0, |(_, conns)| conns.len());
                return Err(Refusal::Invalid {
                    what: "stdio",
                    why: format!(
                        "{came} of the exec's {} streams came in {patience:?}",
                        streams.count()
                    ),
                });
            }
            sets = self.came.wait_timeout(sets, left).map_err(poisoned)?.0;
        }
    }
}

/// Gives back a connection's place in the bound when its thread ends.
struct Release<'a>(&'a AtomicUsize);

impl Drop for Release<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// The host's connection is gone, or the node's.
struct Gone;

/// Refuses a host connection whose token went: a refusal of no request (id 0), as the node's socket
/// refuses one, which the host's hello returns.
fn refuse(host: &mut dyn Duplex, why: Refusal) {
    if let Some(bytes) = wire::answer(wire::FIRST, 0, &Answer::Refused(why), wire::MAX_MESSAGE)
        .ok()
        .and_then(|bytes| stream::frame(&bytes, wire::MAX_MESSAGE).ok())
    {
        let _ = host.write_all(&bytes);
    }
    let _ = host.shutdown(Shutdown::Both);
}

/// A connection to the node agent's socket at `path`.
fn connect(path: &Path) -> Result<OwnedFd, Refusal> {
    let addr = SocketAddrUnix::new(path).map_err(|e| Refusal::Invalid {
        what: "socket path",
        why: format!("{}: {e}", path.display()),
    })?;
    let fd = rustix::net::socket_with(
        AddressFamily::UNIX,
        SocketType::SEQPACKET,
        SocketFlags::CLOEXEC,
        None,
    )
    .map_err(|e| Refusal::os("making a socket to the node agent", e))?;
    rustix::net::connect(&fd, &addr).map_err(|e| Refusal::Os {
        op: "connecting to the node agent's socket",
        why: format!("{}: {e}", path.display()),
    })?;
    Ok(fd)
}

/// For each stream, in stdin-stdout-stderr order, the pipe end the node takes, with a copier between the
/// pipe's other end and its connection.
fn pipes(streams: wire::Streams, conns: Vec<Box<dyn Duplex>>) -> Result<Vec<OwnedFd>, Refusal> {
    let order = [
        (streams.stdin, true),
        (streams.stdout, false),
        (streams.stderr, false),
    ];
    let mut conns = conns.into_iter();
    let mut fds = Vec::with_capacity(streams.count());
    for (on, input) in order {
        if !on {
            continue;
        }
        let conn = conns.next().ok_or_else(|| Refusal::Invalid {
            what: "stdio",
            why: "a stream named by the exec didn't come".to_owned(),
        })?;
        let (r, w) = std::io::pipe().map_err(|e| Refusal::os("making a stdio pipe", e))?;
        let (theirs, copy): (OwnedFd, Copier) = if input {
            (OwnedFd::from(r), Copier::In(w))
        } else {
            (OwnedFd::from(w), Copier::Out(r))
        };
        std::thread::Builder::new()
            .name("hex-relay-stdio-copy".to_owned())
            .stack_size(COPIER_STACK)
            .spawn(move || copy.run(conn))
            .map_err(|e| Refusal::os("starting a stdio copier", e))?;
        fds.push(theirs);
    }
    Ok(fds)
}

/// What one stdio copier copies.
enum Copier {
    /// From the host's connection into the program's stdin pipe.
    In(PipeWriter),
    /// From the program's stdout or stderr pipe to the host's connection, which then ends its sending
    /// half, so the host sees the stream end.
    Out(PipeReader),
}

impl Copier {
    fn run(self, mut conn: Box<dyn Duplex>) {
        match self {
            Self::In(mut to) => {
                let _ = io::copy(&mut conn, &mut to);
            }
            Self::Out(mut from) => {
                let _ = io::copy(&mut from, &mut conn);
                let _ = conn.shutdown(Shutdown::Write);
            }
        }
    }
}

/// The node's answers, each framed to the host, until the node's connection ends; then the host's ends.
fn answers(node: &OwnedFd, writer: &Mutex<Box<dyn Duplex>>) {
    let mut buf = vec![0u8; wire::MAX_MESSAGE];
    loop {
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(wire::MAX_FDS))];
        let mut control = RecvAncillaryBuffer::new(&mut space);
        let msg = match rustix::net::recvmsg(
            node,
            &mut [IoSliceMut::new(&mut buf)],
            &mut control,
            RecvFlags::CMSG_CLOEXEC | RecvFlags::TRUNC,
        ) {
            Ok(msg) => msg,
            Err(Errno::INTR) => continue,
            Err(_) => break,
        };
        // An answer carries no descriptors: a message with any isn't one, and the connection ends.
        let descriptors = control
            .drain()
            .any(|m| matches!(m, RecvAncillaryMessage::ScmRights(_)));
        if msg.bytes == 0 || descriptors || msg.bytes > buf.len() {
            break;
        }
        let framed = buf
            .get(..msg.bytes)
            .and_then(|bytes| stream::frame(bytes, wire::MAX_MESSAGE).ok());
        let Some(framed) = framed else { break };
        let Ok(mut w) = writer.lock() else { break };
        if w.write_all(&framed).is_err() {
            break;
        }
    }
    if let Ok(w) = writer.lock() {
        let _ = w.shutdown(Shutdown::Both);
    }
}

/// Sends one message to the node with `fds`.
fn send(node: &OwnedFd, bytes: &[u8], fds: &[OwnedFd]) -> Result<(), Gone> {
    let borrowed: Vec<BorrowedFd<'_>> = fds.iter().map(AsFd::as_fd).collect();
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(wire::MAX_FDS))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    if !borrowed.is_empty() && !control.push(SendAncillaryMessage::ScmRights(&borrowed)) {
        return Err(Gone);
    }
    loop {
        match rustix::net::sendmsg(
            node,
            &[IoSlice::new(bytes)],
            &mut control,
            SendFlags::NOSIGNAL,
        ) {
            Ok(_) => return Ok(()),
            Err(Errno::INTR) => continue,
            Err(_) => return Err(Gone),
        }
    }
}

pub mod files;

#[cfg(test)]
mod tests;
