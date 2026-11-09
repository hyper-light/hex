//! The client of a node agent in a Shards guest (docs/design/shards-guest.md §3): the same calls as the
//! local socket's, over a byte stream that carries no descriptors. Its control connection carries the
//! wire's messages framed by length ([`hex_core::stream`]); each exec's stdin, stdout and stderr ride a
//! connection of their own, named by its first bytes. One asked for is the caller's to hold, its socket the
//! stream's end; one the caller gives as a pipe of its own is copied between that pipe and the connection in
//! the caller's process, one thread and one buffer per stream: here, or, through a node's socket, by the
//! caller's socket client, which the node gives the connection's end (native plan step 8). The event form, a
//! node's, copies none. A [`Link`] dials the guest's ports: vsock through Shards in production, a loopback
//! socket in tests. The control connection is read as every client's is (`crate::calls`), by whoever waits on it, or
//! in the event form by the node's loop ([`Pump`]), never by a thread of its own.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hex_api::{Refusal, Stdio};
use hex_core::patience::Patience;
use hex_core::stream::{self, Deframer, Exec, Named};
use hex_core::wire::{self, Received};

use crate::calls::{Core, KINDS, Lent, Sent, Shared, Take, Transport, poisoned};
use crate::copy::{copy, copy_in};

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

/// How long a call waits for the guest's node agent's answer, one wait a kind of call (`crate::calls::kind_of`'s
/// order), as the socket's client waits (decision 9): twice the slowest answer this process has had to a call of
/// that kind, through the guest. MEASURING: no record of a guest's answers yet, whose creates stage a root through
/// the guest's relay; each kind's `client.guest.answer.<kind>` rows of `bench/competitive/probes/waits_census.md` will
/// seed it, and until then a call waits for its answer alone.
static ANSWERS: [Patience; KINDS] = [
    Patience::measuring("client.guest.answer.hello"),
    Patience::measuring("client.guest.answer.create"),
    Patience::measuring("client.guest.answer.start"),
    Patience::measuring("client.guest.answer.exec"),
    Patience::measuring("client.guest.answer.signal"),
    Patience::measuring("client.guest.answer.inspect"),
    Patience::measuring("client.guest.answer.usage"),
    Patience::measuring("client.guest.answer.list"),
    Patience::measuring("client.guest.answer.delete"),
    Patience::measuring("client.guest.answer.netns"),
    Patience::measuring("client.guest.answer.kill"),
    Patience::measuring("client.guest.answer.exec_streams"),
    Patience::measuring("client.guest.answer.run"),
];

/// A node agent in a guest, reached through a [`Link`].
pub type Client = Core<Stream>;

/// The control connection, as a client's transport: read and written without waiting, waited on by the core.
pub struct Stream {
    link: Arc<dyn Link>,
    /// The token the relay sent first on the control connection, which its stdio connections name.
    token: u64,
    /// The writing handle, its messages one at a time under the core's send lock.
    write: Mutex<Box<dyn Duplex>>,
    /// A handle to wait on and to end the connection with while a send holds the writing one.
    end: Box<dyn Duplex>,
    /// The reading handle, until the core's read state takes it.
    read: Mutex<Option<Box<dyn Duplex>>>,
    /// In the event form, what waits to be written ([`Backlog`]); else nothing.
    backlog: Mutex<Option<Backlog>>,
}

/// What the turn's holder reads with and into, and what of a frame has come.
pub struct Reading {
    conn: Option<Box<dyn Duplex>>,
    /// A read takes at most one whole frame, the most the deframer holds.
    buf: Vec<u8>,
    deframer: Deframer,
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
    /// A stdio connection of its own to the guest, named for `named`, so the guest's relay hands it to that exec's
    /// program as its stream.
    fn dial_named(&self, named: Named) -> Result<Box<dyn Duplex>, Refusal> {
        let mut conn = self.link.dial(stream::STDIO_PORT)?;
        conn.write_all(&stream::name(named))
            .map_err(os("naming a stdio connection"))?;
        Ok(conn)
    }
}

impl Transport for Stream {
    type Reading = Reading;
    const MAKES_ASKED: bool = true;

    fn answers() -> &'static [Patience; KINDS] {
        &ANSWERS
    }

    fn reading(&self) -> Reading {
        Reading {
            conn: self.read.lock().ok().and_then(|mut r| r.take()),
            buf: vec![0u8; stream::MAX_FRAME],
            deframer: Deframer::new(wire::MAX_MESSAGE),
        }
    }

    fn handle(&self) -> crate::calls::Handle<'_> {
        self.end.handle()
    }

    /// Each stream asked for is a stdio connection of its own to the guest, named for the exec, and the
    /// caller holds that connection's socket: the guest's relay splices it to the program, so no byte is
    /// copied on the host (native plan step 8).
    #[cfg(unix)]
    fn asks(&self, exec: u64, ask: hex_api::Ask) -> Result<hex_api::Given, Refusal> {
        let exec = Exec {
            conn: self.token,
            id: exec,
        };
        let dial = |on: bool, stream| -> Result<Option<std::os::fd::OwnedFd>, Refusal> {
            if !on {
                return Ok(None);
            }
            let conn = self.dial_named(Named { exec, stream })?;
            // The caller's own descriptor of the connection; this side's closes as `conn` drops.
            let end = conn
                .handle()
                .try_clone_to_owned()
                .map_err(os("handing a stdio connection to its caller"))?;
            // HEX-RT-17: Shards waits around a non-blocking socket in its Stream wrapper. Only the raw descriptor
            // outlives this handoff, so its caller's File must block itself; the control connection stays evented.
            let flags = rustix::fs::fcntl_getfl(&end)
                .map_err(|e| Refusal::os("reading a stdio connection's mode", e))?;
            rustix::fs::fcntl_setfl(&end, flags & !rustix::fs::OFlags::NONBLOCK)
                .map_err(|e| Refusal::os("making a caller's stdio connection wait", e))?;
            Ok(Some(end))
        };
        Ok(hex_api::Given {
            stdin: dial(ask.stdin, stream::Stdio::In)?,
            stdout: dial(ask.stdout, stream::Stdio::Out)?,
            stderr: dial(ask.stderr, stream::Stdio::Err)?,
        })
    }

    #[cfg(not(unix))]
    fn asks(&self, _exec: u64, _ask: hex_api::Ask) -> Result<hex_api::Given, Refusal> {
        Err(Refusal::Platform {
            what: "stdio streams made by the guest's transport",
            platform: std::env::consts::OS,
            why: "a guest stream's handle as the caller's is the Windows agent's to add".to_owned(),
        })
    }

    /// Each pipe of the caller's own is copied between it and a stdio connection of its own to the guest, named
    /// for the exec: here, in the caller's process, whose client this is (native plan step 8).
    fn carries(&self, exec: u64, stdio: Stdio) -> Result<(), Refusal> {
        let exec = Exec {
            conn: self.token,
            id: exec,
        };
        let named = |stream| Named { exec, stream };
        if let Some(r) = stdio.stdin {
            copy_in(r, self.dial_named(named(stream::Stdio::In))?)?;
        }
        if let Some(w) = stdio.stdout {
            copy(self.dial_named(named(stream::Stdio::Out))?, w, drop)?;
        }
        if let Some(w) = stdio.stderr {
            copy(self.dial_named(named(stream::Stdio::Err))?, w, drop)?;
        }
        Ok(())
    }

    /// What one read takes, each whole message in it handed on; a frame cut across reads stays in the deframer.
    fn recv(&self, reading: &mut Reading, take: &mut Take<'_>) -> Result<bool, Refusal> {
        let Reading {
            conn,
            buf,
            deframer,
        } = reading;
        let Some(conn) = conn.as_mut() else {
            return Err(Refusal::Broken(
                "the guest's connection has no reading handle".to_owned(),
            ));
        };
        let n = match conn.read(buf) {
            Ok(0) => return Err(ended(deframer)),
            Ok(n) => n,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                return Ok(false);
            }
            Err(e) => {
                return Err(Refusal::Broken(format!(
                    "reading the node agent's connection: {e}"
                )));
            }
        };
        let mut input = buf.get(..n).unwrap_or_default();
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
            take(&received, Vec::new())?;
        }
        Ok(true)
    }

    fn frame(message: Vec<u8>) -> Result<Vec<u8>, Refusal> {
        stream::frame(&message, wire::MAX_MESSAGE).map_err(Refusal::from)
    }

    /// What the connection takes of the frame now. It carries no descriptor: its streams are connections of their
    /// own. In the event form the frame is queued whole behind what waits ([`Backlog`]), for the loop to write.
    fn try_send(&self, bytes: &[u8], at: &mut usize, lent: &[Lent<'_>]) -> Result<bool, Sent> {
        if !lent.is_empty() {
            return Err(Sent::Refused(Refusal::Broken(
                "a guest's connection carries no descriptors".to_owned(),
            )));
        }
        let mut write = self.write.lock().map_err(|e| Sent::Refused(poisoned(e)))?;
        let mut backlog = self
            .backlog
            .lock()
            .map_err(|e| Sent::Refused(poisoned(e)))?;
        if let Some(backlog) = backlog.as_mut() {
            backlog.send(&mut **write, bytes)?;
            *at = bytes.len();
            return Ok(true);
        }
        drop(backlog);
        while let Some(rest) = bytes.get(*at..).filter(|r| !r.is_empty()) {
            match write.write(rest) {
                Ok(0) => return Err(Sent::Gone),
                Ok(n) => *at = at.saturating_add(n),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                Err(e) if gone(&e) => return Err(Sent::Gone),
                Err(e) => return Err(Sent::Refused(Refusal::os("sending to the node agent", e))),
            }
        }
        Ok(true)
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

/// Reads at most this many times on one [`Pump::ready`] before the loop's other sources get a turn: a
/// fairness bound, as the node's socket's `READS_PER_WAKE`.
const READS_PER_READY: usize = 64;

/// The event form's reading side ([`Client::connect_evented`]): no thread, but the loop that waits on
/// [`Pump::handle`] calls [`Pump::ready`] when it is ready, which reads what has come and hands each
/// message to its call's completion, and writes what waits. It holds the connection's reader's turn for good.
pub struct Pump {
    shared: Arc<Shared<Stream>>,
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
        self.shared.transport.end.handle()
    }

    /// Reads what has come, without waiting, and writes what waits as far as the connection takes it.
    pub fn ready(&mut self) -> Pumped {
        if self.ended {
            return Pumped::Ended;
        }
        for _ in 0..READS_PER_READY {
            match self.shared.recv() {
                Ok(true) => {}
                Ok(false) => break,
                // The connection ended: every call and program waiting was told why.
                Err(_) => {
                    self.ended = true;
                    return Pumped::Ended;
                }
            }
        }
        // What the reads owe the agent goes behind what waits, and with it.
        self.shared.send_owed_now();
        match self.flush() {
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
        Core::hello(Arc::new(Shared::new(Stream::dial(link)?)))
    }

    /// The event form ([`Pump`]): dials the guest's node agent through `link` and says hello, its answer read by
    /// the hello itself, then hands back the client and its reading side, which no thread reads: the caller's loop
    /// waits on [`Pump::handle`] and calls [`Pump::ready`]. Calls made with [`Core::begin`] are answered from there.
    /// `wants_room` is told when bytes wait to be written, so the loop watches for room too. Every connection's
    /// waits are the loop's from here: its reads and writes never wait.
    pub fn connect_evented(
        link: Arc<dyn Link>,
        wants_room: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<(Self, Pump), Refusal> {
        let shared = Arc::new(Shared::new(Stream::dial(link)?));
        let client = Core::hello(Arc::clone(&shared))?;
        let limits = client.limits();
        *shared.transport.backlog.lock().map_err(poisoned)? = Some(Backlog {
            bytes: VecDeque::new(),
            bound: limits.in_flight.saturating_mul(limits.message_bytes),
            wants_room,
        });
        shared.read_by_loop()?;
        Ok((
            client,
            Pump {
                shared,
                ended: false,
            },
        ))
    }
}

impl Stream {
    /// The guest's control port dialed through `link`, its relay's token read, and the connection made to read and
    /// write without waiting, as the core waits on it.
    fn dial(link: Arc<dyn Link>) -> Result<Self, Refusal> {
        let mut conn = link.dial(stream::CONTROL_PORT)?;
        let mut token = [0u8; stream::TOKEN_BYTES];
        conn.read_exact(&mut token)
            .map_err(os("reading the relay's token for the connection"))?;
        let token = u64::from_le_bytes(token);
        let read = conn
            .try_clone()
            .map_err(os("holding the node agent's connection"))?;
        let end = conn
            .try_clone()
            .map_err(os("holding the node agent's connection"))?;
        conn.set_nonblocking(true)
            .map_err(os("reading the node agent's connection without waiting"))?;
        Ok(Self {
            link,
            token,
            write: Mutex::new(conn),
            end,
            read: Mutex::new(Some(read)),
            backlog: Mutex::new(None),
        })
    }
}

pub mod shards;
pub mod stage;

#[cfg(test)]
mod tests;
