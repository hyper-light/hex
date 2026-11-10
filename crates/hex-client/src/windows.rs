//! Windows' local client transport (native plan step 8, HEX-RT-17): an AF_UNIX stream, framed messages,
//! and handles the node duplicated into this process, named in each answer's envelope. The one client core
//! owns reads, waits, whole-message sends and the Taken acknowledgement; this transport starts no thread.

use std::io::Write as _;
use std::os::windows::io::AsSocket;
use std::path::Path;
use std::sync::Arc;

use hex_afunix::{self as socket, Socket as Raw};
use hex_api::Refusal;
use hex_core::patience::Patience;
use hex_core::stream::{self, Deframer};
use hex_core::wire::{self, Received};

use crate::calls::{Core, Handle, KINDS, Lent, Sent, Shared, Take, Transport};

/// The largest wire message plus its envelope's three stream handles and their count.
const FRAME_BOUND: usize = wire::MAX_MESSAGE + socket::ENVELOPE_MAX;

/// Each call kind learns its wait on this Windows node, as on the other native transports (decision 9).
static ANSWERS: [Patience; KINDS] = [
    Patience::measuring("client.answer.hello"),
    Patience::measuring("client.answer.create"),
    Patience::measuring("client.answer.start"),
    Patience::measuring("client.answer.exec"),
    Patience::measuring("client.answer.signal"),
    Patience::measuring("client.answer.inspect"),
    Patience::measuring("client.answer.usage"),
    Patience::measuring("client.answer.list"),
    Patience::measuring("client.answer.delete"),
    Patience::measuring("client.answer.netns"),
    Patience::measuring("client.answer.kill"),
    Patience::measuring("client.answer.exec_streams"),
    Patience::measuring("client.answer.run"),
    Patience::measuring("client.answer.schedule.describe"),
    Patience::measuring("client.answer.schedule.advertise"),
    Patience::measuring("client.answer.schedule.submit"),
    Patience::measuring("client.answer.schedule.hear_create"),
    Patience::measuring("client.answer.schedule.hear_delete"),
    Patience::measuring("client.answer.schedule.hear_control"),
    Patience::measuring("client.answer.schedule.link_up"),
    Patience::measuring("client.answer.schedule.retry"),
    Patience::measuring("client.answer.schedule.delete"),
    Patience::measuring("client.answer.schedule.status"),
    Patience::measuring("client.answer.capacity"),
    Patience::measuring("client.answer.physical_report"),
];

pub type Client = Core<Socket>;

/// A non-blocking AF_UNIX connection to the local node agent.
pub struct Socket {
    raw: Raw,
}

pub struct Reading {
    buf: Vec<u8>,
    deframer: Deframer,
    reset: Option<Refusal>,
}

impl Transport for Socket {
    type Reading = Reading;

    fn answers() -> &'static [Patience; KINDS] {
        &ANSWERS
    }

    fn reading(&self) -> Reading {
        Reading {
            buf: vec![0; stream::MAX_FRAME],
            deframer: Deframer::new(FRAME_BOUND),
            reset: None,
        }
    }

    fn handle(&self) -> Handle<'_> {
        self.raw.as_socket()
    }

    fn open_offer(&self, offer: &wire::StreamOffer) -> Result<hex_api::Given, Refusal> {
        let identity = shards_api::Identity {
            pid: offer.pid,
            started: offer.started,
            vsock: offer.vsock.clone(),
        };
        let dial = |on: bool, stream| -> Result<Option<hex_api::End>, Refusal> {
            if !on {
                return Ok(None);
            }
            // The dedicated control connection has already said hello, so this is one attempt at an
            // already-listening guest, bounded by Shards' existing CONNECT handshake and the VM's life.
            let mut conn = shards_api::connect_borrowed(
                &identity,
                stream::STDIO_PORT,
                std::time::Duration::ZERO,
            )
            .map_err(|why| {
                crate::guest::shards::refusal("opening an offered guest stream", &why)
            })?;
            let named = stream::Named {
                exec: stream::Exec {
                    conn: offer.token,
                    id: offer.exec,
                },
                stream,
            };
            conn.write_all(&stream::name(named))
                .map_err(|why| Refusal::os("naming an offered guest stream", why))?;
            let end = conn
                .handle()
                .try_clone_to_owned()
                .map_err(|why| Refusal::os("owning the caller's guest stream socket", why))?;
            socket::set_nonblocking(end.as_socket(), false)
                .map_err(|why| Refusal::os("making the caller's guest stream socket wait", why))?;
            Ok(Some(end.into()))
        };
        Ok(hex_api::Given {
            stdin: dial(offer.streams.stdin, stream::Stdio::In)?,
            stdout: dial(offer.streams.stdout, stream::Stdio::Out)?,
            stderr: dial(offer.streams.stderr, stream::Stdio::Err)?,
        })
    }

    fn recv(&self, reading: &mut Reading, take: &mut Take<'_>) -> Result<bool, Refusal> {
        let n = match self.raw.recv(&mut reading.buf) {
            Ok(n) => n,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
                ) =>
            {
                return Ok(false);
            }
            Err(e) if socket::is_reset(&e) && reading.reset.is_none() => {
                reading.reset = Some(Refusal::Broken(
                    "the node agent closed the connection with requests unread".to_owned(),
                ));
                return Ok(true);
            }
            Err(e) => return Err(Refusal::os("reading the node agent's socket", e)),
        };
        if n == 0 {
            return Err(reading.reset.take().unwrap_or_else(|| {
                Refusal::Broken("the node agent closed the connection".to_owned())
            }));
        }
        let mut input = reading.buf.get(..n).unwrap_or_default();
        while !input.is_empty() {
            let (took, whole) = reading.deframer.take(input).map_err(|fault| {
                Refusal::Broken(format!("the node agent's stream can't be read: {fault}"))
            })?;
            input = input.get(took..).unwrap_or_default();
            let Some(payload) = whole else { continue };
            // The server keeps its copies until the common core sends Taken. The duplicated ends named by
            // this envelope become ours here, and drop with an answer that doesn't take them.
            let (ends, message) = socket::take_given(&payload)
                .map_err(|e| Refusal::os("taking the node agent's given stream handles", e))?;
            let received = Received {
                bytes: message,
                size: message.len(),
                descriptors: ends.len(),
                descriptors_cut: false,
            };
            take(
                &received,
                ends.into_iter().map(hex_api::End::from).collect(),
            )?;
        }
        Ok(true)
    }

    fn frame(message: Vec<u8>) -> Result<Vec<u8>, Refusal> {
        stream::frame(&message, wire::MAX_MESSAGE).map_err(|fault| Refusal::Invalid {
            what: "message",
            why: fault.to_string(),
        })
    }

    fn try_send(&self, bytes: &[u8], at: &mut usize, lent: &[Lent<'_>]) -> Result<bool, Sent> {
        if !lent.is_empty() {
            return Err(Sent::Refused(Refusal::Platform {
                what: "an exec's stdio through the node agent's socket",
                platform: std::env::consts::OS,
                why: "Windows' AF_UNIX passes no handles; ask the node for the exec's streams instead of lending a pipe".to_owned(),
            }));
        }
        while *at < bytes.len() {
            match self.raw.send(bytes.get(*at..).unwrap_or_default()) {
                Ok(0) => return Err(Sent::Gone),
                Ok(n) => *at = at.saturating_add(n),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
                Err(e)
                    if socket::is_reset(&e)
                        || matches!(
                            e.kind(),
                            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::NotConnected
                        ) =>
                {
                    return Err(Sent::Gone);
                }
                Err(e) => return Err(Sent::Refused(Refusal::os("sending to the node agent", e))),
            }
        }
        Ok(true)
    }

    fn shutdown(&self) {
        self.raw.shutdown();
    }
}

impl Client {
    /// Connects to the node agent, then gives the non-blocking socket to the common reader's turn.
    pub fn connect(path: &Path) -> Result<Self, Refusal> {
        let raw = Raw::connect(path).map_err(|e| Refusal::Os {
            op: "connecting to the node agent's socket",
            why: format!("{}: {e}", path.display()),
        })?;
        raw.set_nonblocking(true)
            .map_err(|e| Refusal::os("making the node agent's socket non-blocking", e))?;
        Core::hello(Arc::new(Shared::new(Socket { raw })))
    }
}

#[cfg(test)]
mod tests;
