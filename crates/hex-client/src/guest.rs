//! The client of a node agent in a Shards guest (docs/design/shards-guest.md §3): the same calls as the
//! local socket's, over a byte stream that carries no descriptors. Its control connection carries the
//! wire's messages framed by length ([`hex_core::stream`]); each exec's stdin, stdout and stderr ride a
//! connection of their own, named by its first bytes, and this side copies between the caller's pipes and
//! them, one thread and one buffer per stream. A [`Link`] dials the guest's ports: vsock through Shards
//! in production, a loopback socket in tests.

use std::collections::VecDeque;
use std::io::{self, PipeReader, PipeWriter, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hex_api::{Refusal, Stdio};
use hex_core::stream::{self, Deframer, Exec, Named};
use hex_core::wire::{self, Received};

use crate::ANSWER_PATIENCE;
use crate::calls::{Core, Sent, Shared, Transport, poisoned};

/// The OS handle under a guest connection, which an event loop waits on: a file descriptor on Unix, a
/// Winsock socket on Windows.
pub use shards_api::Handle;

/// A connection to one of the guest's ports.
pub trait Duplex: Read + Write + Send + Sync + 'static {
    /// Another handle on the same connection.
    fn try_clone(&self) -> io::Result<Box<dyn Duplex>>;
    fn shutdown(&self, how: Shutdown) -> io::Result<()>;
    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
    /// The OS handle under it, borrowed, for an event loop to wait on; it stays the connection's.
    fn handle(&self) -> Handle<'_>;
    /// With `on`, a read or a write that would wait fails `WouldBlock` at once; shared by its clones.
    fn set_nonblocking(&self, on: bool) -> io::Result<()>;
}

impl Duplex for TcpStream {
    fn try_clone(&self) -> io::Result<Box<dyn Duplex>> {
        Ok(Box::new(TcpStream::try_clone(self)?))
    }

    fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        TcpStream::shutdown(self, how)
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        TcpStream::set_write_timeout(self, timeout)
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        TcpStream::set_read_timeout(self, timeout)
    }

    fn handle(&self) -> Handle<'_> {
        #[cfg(unix)]
        return std::os::fd::AsFd::as_fd(self);
        #[cfg(windows)]
        std::os::windows::io::AsSocket::as_socket(self)
    }

    fn set_nonblocking(&self, on: bool) -> io::Result<()> {
        TcpStream::set_nonblocking(self, on)
    }
}

#[cfg(unix)]
impl Duplex for std::os::unix::net::UnixStream {
    fn try_clone(&self) -> io::Result<Box<dyn Duplex>> {
        Ok(Box::new(std::os::unix::net::UnixStream::try_clone(self)?))
    }

    fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        std::os::unix::net::UnixStream::shutdown(self, how)
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        std::os::unix::net::UnixStream::set_write_timeout(self, timeout)
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        std::os::unix::net::UnixStream::set_read_timeout(self, timeout)
    }

    fn handle(&self) -> Handle<'_> {
        std::os::fd::AsFd::as_fd(self)
    }

    fn set_nonblocking(&self, on: bool) -> io::Result<()> {
        std::os::unix::net::UnixStream::set_nonblocking(self, on)
    }
}

/// What dials the guest's ports.
pub trait Link: Send + Sync + 'static {
    /// A new connection to the guest's `port`.
    fn dial(&self, port: u32) -> Result<Box<dyn Duplex>, Refusal>;
}

/// A node agent in a guest, reached through a [`Link`].
pub type Client = Core<Stream>;

/// The control connection, as a client's transport.
pub struct Stream {
    link: Arc<dyn Link>,
    /// The token the relay sent first on the control connection, which its stdio connections name.
    token: u64,
    /// The writing handle: one message at a time.
    write: Mutex<Box<dyn Duplex>>,
    /// A handle to end the connection with while a send holds the writing one.
    end: Box<dyn Duplex>,
    /// In the event form, what waits to be written ([`Backlog`]); in the reading thread's, nothing.
    backlog: Mutex<Option<Backlog>>,
}

/// The event form's bytes waiting to be written, oldest first, flushed by its [`Pump`] as the
/// connection takes them: at most `bound` bytes, the agent's in-flight bound in whole messages, so a
/// send past it is refused, naming the bound (CLAUDE.md §2), and a whole message is queued or none.
struct Backlog {
    bytes: VecDeque<u8>,
    bound: usize,
    /// Told when the backlog stops being empty, so its loop watches for room.
    wants_room: Arc<dyn Fn() + Send + Sync>,
}

/// What [`Backlog::bound`] names in its refusal.
pub const BACKLOG: &str = "bytes waiting to be written to the guest's node agent";

fn os(op: &'static str) -> impl Fn(io::Error) -> Refusal {
    move |e| Refusal::os(op, e)
}

impl Stream {
    /// Dials a stdio connection for `named`, says what it is, and starts the thread that copies it.
    fn stream(&self, named: Named, copy: Copier) -> Result<(), Refusal> {
        // Unbounded in time, as a local exec's stdin is the caller's own pipe: a program that doesn't read
        // it holds its copier, which ends with the connection, when the exec does (CLAUDE.md §8).
        let mut conn = self.link.dial(stream::STDIO_PORT)?;
        conn.write_all(&stream::name(named))
            .map_err(os("naming a stdio connection"))?;
        std::thread::Builder::new()
            .name("hex-client-stdio".to_owned())
            .spawn(move || copy.run(conn))
            .map(drop)
            .map_err(os("starting a stdio copier"))
    }
}

/// What one stdio copier copies.
enum Copier {
    /// From the caller's pipe into the connection, which then ends its sending half.
    In(PipeReader),
    /// From the connection into the caller's pipe.
    Out(PipeWriter),
}

impl Copier {
    /// Copies until either side ends. Its end is the stream's: the program sees stdin end, the caller
    /// sees stdout end, as with a pipe; there is no one else to tell.
    fn run(self, mut conn: Box<dyn Duplex>) {
        match self {
            Self::In(mut from) => {
                let _ = io::copy(&mut from, &mut conn);
                let _ = conn.shutdown(Shutdown::Write);
            }
            Self::Out(mut to) => {
                let _ = io::copy(&mut conn, &mut to);
            }
        }
    }
}

impl Transport for Stream {
    type Stdio = ();

    fn stdio(&self, exec: u64, stdio: Stdio) -> Result<(), Refusal> {
        let exec = Exec {
            conn: self.token,
            id: exec,
        };
        let named = |stream| Named { exec, stream };
        if let Some(r) = stdio.stdin {
            self.stream(named(stream::Stdio::In), Copier::In(r))?;
        }
        if let Some(w) = stdio.stdout {
            self.stream(named(stream::Stdio::Out), Copier::Out(w))?;
        }
        if let Some(w) = stdio.stderr {
            self.stream(named(stream::Stdio::Err), Copier::Out(w))?;
        }
        Ok(())
    }

    fn send(&self, bytes: &[u8], _with: Option<()>) -> Result<(), Sent> {
        let framed =
            stream::frame(bytes, wire::MAX_MESSAGE).map_err(|f| Sent::Refused(f.into()))?;
        let mut write = self.write.lock().map_err(|e| Sent::Refused(poisoned(e)))?;
        let mut backlog = self
            .backlog
            .lock()
            .map_err(|e| Sent::Refused(poisoned(e)))?;
        if let Some(backlog) = backlog.as_mut() {
            return backlog.send(&mut **write, &framed);
        }
        drop(backlog);
        match write.write_all(&framed) {
            Ok(()) => Ok(()),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::BrokenPipe
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::NotConnected
                ) =>
            {
                Err(Sent::Gone)
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                Err(Sent::Refused(Refusal::Os {
                    op: "sending to the node agent",
                    why: format!(
                        "no room in its connection for {ANSWER_PATIENCE:?}: it isn't reading"
                    ),
                }))
            }
            Err(e) => Err(Sent::Refused(Refusal::os("sending to the node agent", e))),
        }
    }

    fn shutdown(&self) {
        let _ = self.end.shutdown(Shutdown::Both);
    }
}

impl Backlog {
    /// Writes `framed` now as far as the connection takes it, behind anything waiting, and queues the
    /// rest; refused whole when it wouldn't fit.
    fn send(&mut self, conn: &mut dyn Duplex, framed: &[u8]) -> Result<(), Sent> {
        if self.bytes.len().saturating_add(framed.len()) > self.bound {
            return Err(Sent::Refused(Refusal::Exhausted {
                what: BACKLOG,
                bound: self.bound,
            }));
        }
        let mut wrote = 0;
        if self.bytes.is_empty() {
            while let Some(rest) = framed.get(wrote..).filter(|r| !r.is_empty()) {
                match conn.write(rest) {
                    Ok(0) => return Err(Sent::Gone),
                    Ok(n) => wrote = wrote.saturating_add(n),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) if gone(&e) => return Err(Sent::Gone),
                    Err(e) => {
                        return Err(Sent::Refused(Refusal::os("sending to the node agent", e)));
                    }
                }
            }
        }
        let rest = framed.get(wrote..).unwrap_or_default();
        if !rest.is_empty() {
            let was_empty = self.bytes.is_empty();
            self.bytes.extend(rest);
            if was_empty {
                (self.wants_room)();
            }
        }
        Ok(())
    }

    /// Writes what waits as far as the connection takes it: whether any is left.
    fn flush(&mut self, conn: &mut dyn Duplex) -> Result<bool, Refusal> {
        while !self.bytes.is_empty() {
            let (front, _) = self.bytes.as_slices();
            match conn.write(front) {
                Ok(0) => {
                    return Err(Refusal::Broken(
                        "the node agent's connection took no bytes".to_owned(),
                    ));
                }
                Ok(n) => {
                    self.bytes.drain(..n.min(front.len()));
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(true),
                Err(e) => return Err(Refusal::os("sending to the node agent", e)),
            }
        }
        Ok(false)
    }
}

fn gone(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset | io::ErrorKind::NotConnected
    )
}

/// The reading thread: every message the agent sends, until the connection ends.
fn read(shared: &Shared<Stream>, mut conn: Box<dyn Duplex>) {
    let mut deframer = Deframer::new(wire::MAX_MESSAGE);
    // A read takes at most one whole frame, the most the deframer holds.
    let mut buf = vec![0u8; stream::MAX_FRAME];
    let why = loop {
        let n = match conn.read(&mut buf) {
            Ok(0) => break ended(&deframer),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => break Refusal::Broken(format!("reading the node agent's connection: {e}")),
        };
        if let Err(why) = take(shared, &mut deframer, buf.get(..n).unwrap_or_default()) {
            break why;
        }
    };
    shared.close(why);
}

/// Why a connection that read its end ended: between messages, or in the middle of one.
fn ended(deframer: &Deframer) -> Refusal {
    if deframer.at_boundary() {
        Refusal::Broken("the node agent closed the connection".to_owned())
    } else {
        Refusal::Broken(
            "the node agent closed the connection in the middle of a message".to_owned(),
        )
    }
}

/// Hands every whole message in `input`, with what came before it, to whoever waits for it.
fn take(shared: &Shared<Stream>, deframer: &mut Deframer, mut input: &[u8]) -> Result<(), Refusal> {
    while !input.is_empty() {
        let (took, whole) = deframer.take(input).map_err(|fault| {
            Refusal::Broken(format!(
                "the node agent sent bytes that aren't a message: {fault}"
            ))
        })?;
        input = input.get(took..).unwrap_or_default();
        let Some(message) = whole else { continue };
        let received = Received {
            bytes: &message,
            size: message.len(),
            descriptors: 0,
            descriptors_cut: false,
        };
        let (header, answer) =
            wire::read_answer(&received, wire::MAX_MESSAGE).map_err(|fault| {
                Refusal::Broken(format!(
                    "the node agent sent a message that isn't an answer: {fault}"
                ))
            })?;
        shared.deliver(header, answer)?;
    }
    Ok(())
}

/// Reads at most this many times on one [`Pump::ready`] before the loop's other sources get a turn: a
/// fairness bound, as the node's socket's `READS_PER_WAKE`.
const READS_PER_READY: usize = 64;

/// The event form's reading side ([`Client::connect_evented`]): no thread, but the loop that waits on
/// [`Pump::handle`] calls [`Pump::ready`] when it is ready, which reads what has come and hands each
/// message to its call's completion, and writes what waits.
pub struct Pump {
    shared: Arc<Shared<Stream>>,
    conn: Box<dyn Duplex>,
    deframer: Deframer,
    buf: Vec<u8>,
    ended: bool,
}

/// What a [`Pump`] wants watched next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pumped {
    /// Readable.
    Read,
    /// Readable, and writable: bytes wait to be written.
    ReadWrite,
    /// Nothing: the connection has ended, and every call and program waiting has heard why.
    Ended,
}

impl Pump {
    /// The connection's OS handle, borrowed, for the loop to wait on; out of the loop's set before the
    /// pump drops.
    pub fn handle(&self) -> Handle<'_> {
        self.conn.handle()
    }

    /// Reads what has come, without waiting, and writes what waits as far as the connection takes it.
    pub fn ready(&mut self) -> Pumped {
        if self.ended {
            return Pumped::Ended;
        }
        let mut reads = 0;
        let outcome: Result<(), Refusal> = loop {
            if reads >= READS_PER_READY {
                break Ok(());
            }
            reads = reads.saturating_add(1);
            match self.conn.read(&mut self.buf) {
                Ok(0) => break Err(ended(&self.deframer)),
                Ok(n) => {
                    let input = self.buf.get(..n).unwrap_or_default();
                    if let Err(why) = take(&self.shared, &mut self.deframer, input) {
                        break Err(why);
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break Ok(()),
                Err(e) => {
                    break Err(Refusal::Broken(format!(
                        "reading the node agent's connection: {e}"
                    )));
                }
            }
        };
        let waiting = outcome.and_then(|()| self.flush());
        match waiting {
            Ok(true) => Pumped::ReadWrite,
            Ok(false) => Pumped::Read,
            Err(why) => {
                self.ended = true;
                self.shared.transport.shutdown();
                self.shared.close(why);
                Pumped::Ended
            }
        }
    }

    /// Writes what waits: whether any is left.
    fn flush(&mut self) -> Result<bool, Refusal> {
        let transport = &self.shared.transport;
        let mut write = transport.write.lock().map_err(poisoned)?;
        let mut backlog = transport.backlog.lock().map_err(poisoned)?;
        match backlog.as_mut() {
            Some(b) => b.flush(&mut **write),
            None => Ok(false),
        }
    }
}

impl Client {
    /// Dials the guest's node agent through `link` and learns what it runs and its bounds.
    pub fn connect(link: Arc<dyn Link>) -> Result<Self, Refusal> {
        let mut conn = link.dial(stream::CONTROL_PORT)?;
        conn.set_write_timeout(Some(ANSWER_PATIENCE))
            .map_err(os("bounding the node agent connection's sends"))?;
        let mut token = [0u8; stream::TOKEN_BYTES];
        conn.read_exact(&mut token)
            .map_err(os("reading the relay's token for the connection"))?;
        let token = u64::from_le_bytes(token);
        let reading = conn
            .try_clone()
            .map_err(os("holding the node agent's connection"))?;
        let end = conn
            .try_clone()
            .map_err(os("holding the node agent's connection"))?;
        let shared = Arc::new(Shared::new(Stream {
            link,
            token,
            write: Mutex::new(conn),
            end,
            backlog: Mutex::new(None),
        }));
        let reader = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("hex-client-guest".to_owned())
                .spawn(move || read(&shared, reading))
                .map_err(os("starting the client's reading thread"))?
        };
        Core::hello(shared, reader)
    }

    /// The event form ([`Pump`]): dials the guest's node agent through `link`, says hello and waits,
    /// bounded, for its answer, then hands back the client and its reading side, which no thread
    /// reads: the caller's loop waits on [`Pump::handle`] and calls [`Pump::ready`]. Calls made with
    /// [`Core::begin`] are answered from there. `wants_room` is told when bytes wait to be written, so
    /// the loop watches for room too. Every connection's waits are the loop's from here: its reads and
    /// writes never wait.
    pub fn connect_evented(
        link: Arc<dyn Link>,
        wants_room: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<(Self, Pump), Refusal> {
        let mut conn = link.dial(stream::CONTROL_PORT)?;
        conn.set_write_timeout(Some(ANSWER_PATIENCE))
            .map_err(os("bounding the node agent connection's sends"))?;
        conn.set_read_timeout(Some(ANSWER_PATIENCE))
            .map_err(os("bounding the wait for the node agent's hello"))?;
        let mut token = [0u8; stream::TOKEN_BYTES];
        conn.read_exact(&mut token)
            .map_err(os("reading the relay's token for the connection"))?;
        let token = u64::from_le_bytes(token);
        let mut reading = conn
            .try_clone()
            .map_err(os("holding the node agent's connection"))?;
        let end = conn
            .try_clone()
            .map_err(os("holding the node agent's connection"))?;
        let shared = Arc::new(Shared::new(Stream {
            link,
            token,
            write: Mutex::new(conn),
            end,
            backlog: Mutex::new(None),
        }));
        let mut deframer = Deframer::new(wire::MAX_MESSAGE);
        let mut buf = vec![0u8; stream::MAX_FRAME];
        let client = Core::hello_inline(Arc::clone(&shared), |shared| {
            let n = reading
                .read(&mut buf)
                .map_err(os("reading the node agent's hello"))?;
            if n == 0 {
                return Err(ended(&deframer));
            }
            take(shared, &mut deframer, buf.get(..n).unwrap_or_default())
        })?;
        reading
            .set_nonblocking(true)
            .map_err(os("handing the node agent's connection to its loop"))?;
        let limits = client.limits();
        *shared.transport.backlog.lock().map_err(poisoned)? = Some(Backlog {
            bytes: VecDeque::new(),
            bound: limits.in_flight.saturating_mul(limits.message_bytes),
            wants_room,
        });
        let pump = Pump {
            shared,
            conn: reading,
            deframer,
            buf,
            ended: false,
        };
        Ok((client, pump))
    }
}

pub mod shards;
pub mod stage;

#[cfg(test)]
mod tests;
