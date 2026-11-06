//! The client of a node agent in a Shards guest (docs/design/shards-guest.md §3): the same calls as the
//! local socket's, over a byte stream that carries no descriptors. Its control connection carries the
//! wire's messages framed by length ([`hex_core::stream`]); each exec's stdin, stdout and stderr ride a
//! connection of their own, named by its first bytes, and this side copies between the caller's pipes and
//! them, one thread and one buffer per stream. A [`Link`] dials the guest's ports: vsock through Shards
//! in production, a loopback socket in tests.

use std::io::{self, PipeReader, PipeWriter, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hex_api::{Refusal, Stdio};
use hex_core::stream::{self, Deframer, Exec, Named};
use hex_core::wire::{self, Received};

use crate::ANSWER_PATIENCE;
use crate::calls::{Core, Sent, Shared, Transport, poisoned};

/// A connection to one of the guest's ports.
pub trait Duplex: Read + Write + Send + Sync + 'static {
    /// Another handle on the same connection.
    fn try_clone(&self) -> io::Result<Box<dyn Duplex>>;
    fn shutdown(&self, how: Shutdown) -> io::Result<()>;
    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
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
}

/// Each stdio copier's stack: it holds one copy buffer (`std::io::copy`'s 8 KiB) and little else.
const COPIER_STACK: usize = 64 << 10;

fn os(op: &'static str) -> impl Fn(io::Error) -> Refusal {
    move |e| Refusal::os(op, e)
}

impl Stream {
    /// Dials a stdio connection for `named`, says what it is, and starts the thread that copies it.
    fn stream(&self, named: Named, copy: Copier) -> Result<(), Refusal> {
        let mut conn = self.link.dial(stream::STDIO_PORT)?;
        conn.set_write_timeout(Some(ANSWER_PATIENCE))
            .map_err(os("bounding a stdio connection's sends"))?;
        conn.write_all(&stream::name(named))
            .map_err(os("naming a stdio connection"))?;
        std::thread::Builder::new()
            .name("hex-client-stdio".to_owned())
            .stack_size(COPIER_STACK)
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

/// The reading thread: every message the agent sends, until the connection ends.
fn read(shared: &Shared<Stream>, mut conn: Box<dyn Duplex>) {
    let mut deframer = Deframer::new(wire::MAX_MESSAGE);
    let mut buf = vec![0u8; 64 << 10];
    let why = 'conn: loop {
        let n = match conn.read(&mut buf) {
            Ok(0) if deframer.at_boundary() => {
                break Refusal::Broken("the node agent closed the connection".to_owned());
            }
            Ok(0) => {
                break Refusal::Broken(
                    "the node agent closed the connection in the middle of a message".to_owned(),
                );
            }
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => break Refusal::Broken(format!("reading the node agent's connection: {e}")),
        };
        let mut input = buf.get(..n).unwrap_or_default();
        while !input.is_empty() {
            let (took, whole) = match deframer.take(input) {
                Ok(taken) => taken,
                Err(fault) => {
                    break 'conn Refusal::Broken(format!(
                        "the node agent sent bytes that aren't a message: {fault}"
                    ));
                }
            };
            input = input.get(took..).unwrap_or_default();
            let Some(message) = whole else { continue };
            let received = Received {
                bytes: &message,
                size: message.len(),
                descriptors: 0,
                descriptors_cut: false,
            };
            let delivered = wire::read_answer(&received, wire::MAX_MESSAGE)
                .map_err(|fault| {
                    Refusal::Broken(format!(
                        "the node agent sent a message that isn't an answer: {fault}"
                    ))
                })
                .and_then(|(header, answer)| shared.deliver(header, answer));
            if let Err(why) = delivered {
                break 'conn why;
            }
        }
    };
    shared.close(why);
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
}

pub mod shards;
pub mod stage;

#[cfg(test)]
mod tests;
