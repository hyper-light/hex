//! The client on Windows (native plan step 8): one AF_UNIX `SOCK_STREAM` connection to the node agent's
//! socket (afunix.sys has no other kind), each message framed ([`hex_core::stream`]), as on macOS; a thread
//! that reads every message the agent sends and hands it to [`crate::calls`]; and calls that send their
//! request whole (one at a time, as a stream's writes could interleave) and wait, at most
//! [`crate::ANSWER_PATIENCE`], for the answer. The Win32 calls are `hex_afunix`'s.
//! Windows' AF_UNIX passes no handles, and how an exec's stdio reaches the agent is step 8's measured
//! decision, so an exec that gives a stream is refused by name until then.

use std::path::Path;
use std::sync::{Arc, Mutex};

use hex_afunix::{self as socket, Socket as Raw};
use hex_api::Refusal;
use hex_core::stream::{self, Deframer};
use hex_core::wire::{self, Received};

use crate::ANSWER_PATIENCE;
use crate::calls::{Core, Lent, Sent, Shared, Transport};

/// A node agent, reached through its socket.
pub type Client = Core<Socket>;

/// The node agent's socket, as a client's transport.
pub struct Socket {
    raw: Raw,
    /// Held for a whole message's send, so two threads' messages never interleave on the stream.
    sending: Mutex<()>,
}

impl Transport for Socket {
    /// Sends `bytes` framed. Windows' AF_UNIX carries no descriptors, so a message that lends any is
    /// refused by name: an exec's stdio through the node agent's socket waits on its transport, step 8's
    /// measured decision. (A guest makes its own stdio connections, [`Transport::MAKES_ASKED`], not this.)
    fn send(&self, bytes: &[u8], lent: &[Lent<'_>]) -> Result<(), Sent> {
        if !lent.is_empty() {
            return Err(Sent::Refused(Refusal::Platform {
                what: "an exec's stdio through the node agent's socket",
                platform: std::env::consts::OS,
                why:
                    "Windows' AF_UNIX passes no handles, and the transport that carries an exec's \
                      streams to its caller waits on its measured decision (native plan step 8)"
                        .to_owned(),
            }));
        }
        let _whole = self.sending.lock().map_err(|_| {
            Sent::Refused(Refusal::Broken(
                "the client's send lock was poisoned".to_owned(),
            ))
        })?;
        send(&self.raw, bytes)
    }

    fn shutdown(&self) {
        self.raw.shutdown();
    }
}

/// The reading thread: every message the agent sends, until the connection ends.
fn read(shared: &Shared<Socket>) {
    let raw = &shared.transport.raw;
    let mut buf = vec![0u8; stream::MAX_FRAME];
    let mut deframer = Deframer::new(wire::MAX_MESSAGE);
    let mut reset = None;
    let why = 'reading: loop {
        let n = match raw.recv(&mut buf) {
            Ok(n) => n,
            Err(e) if socket::is_reset(&e) && reset.is_none() => {
                reset = Some(Refusal::Broken(
                    "the node agent closed the connection with requests unread".to_owned(),
                ));
                continue;
            }
            Err(e) => break Refusal::Broken(format!("reading the node agent's socket: {e}")),
        };
        if n == 0 {
            break reset.unwrap_or_else(|| {
                Refusal::Broken("the node agent closed the connection".to_owned())
            });
        }
        let mut input = buf.get(..n).unwrap_or_default();
        while !input.is_empty() {
            let message = match deframer.take(input) {
                Ok((took, message)) => {
                    input = input.get(took..).unwrap_or_default();
                    message
                }
                Err(fault) => {
                    break 'reading Refusal::Broken(format!(
                        "the node agent's stream can't be read: {fault}"
                    ));
                }
            };
            let Some(message) = message else { continue };
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
                break 'reading why;
            }
        }
    };
    shared.close(why);
}

impl Client {
    /// Connects to the node agent at `path` and learns what it runs and its bounds.
    pub fn connect(path: &Path) -> Result<Self, Refusal> {
        let raw = Raw::connect(path).map_err(|e| Refusal::Os {
            op: "connecting to the node agent's socket",
            why: format!("{}: {e}", path.display()),
        })?;
        // Bounds a send by the agent's room to take it.
        raw.set_send_patience(ANSWER_PATIENCE)
            .map_err(|e| Refusal::os("bounding a socket's sends", e))?;
        let shared = Arc::new(Shared::new(Socket {
            raw,
            sending: Mutex::new(()),
        }));
        let reader = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("hex-client".to_owned())
                .spawn(move || read(&shared))
                .map_err(|e| Refusal::os("starting the client's reading thread", e))?
        };
        Core::hello(shared, reader)
    }
}

/// Sends one framed message whole.
fn send(raw: &Raw, bytes: &[u8]) -> Result<(), Sent> {
    let framed = stream::frame(bytes, wire::MAX_MESSAGE).map_err(|fault| {
        Sent::Refused(Refusal::Invalid {
            what: "message",
            why: fault.to_string(),
        })
    })?;
    let mut sent = 0usize;
    while sent < framed.len() {
        let rest = framed.get(sent..).unwrap_or_default();
        match raw.send(rest) {
            Ok(n) => sent = sent.saturating_add(n),
            Err(e) if socket::is_reset(&e) => return Err(Sent::Gone),
            // Cut short mid-frame, the stream can't carry another message: the connection goes.
            Err(_) if sent > 0 => {
                raw.shutdown();
                return Err(Sent::Gone);
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                return Err(Sent::Refused(Refusal::Os {
                    op: "sending to the node agent",
                    why: format!("no room in its socket for {ANSWER_PATIENCE:?}: it isn't reading"),
                }));
            }
            Err(e) => return Err(Sent::Refused(Refusal::os("sending to the node agent", e))),
        }
    }
    Ok(())
}
