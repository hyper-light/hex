//! The client on macOS (native plan step 8b): one AF_UNIX `SOCK_STREAM` connection to the node agent's
//! socket, as macOS has no `SOCK_SEQPACKET` local socket, each message framed ([`hex_core::stream`]); a
//! thread that reads every message the agent sends and hands it to [`crate::calls`]; and calls that send
//! their request whole (one at a time, as a stream's writes could interleave), an exec's stdio as
//! `SCM_RIGHTS` descriptors at its message's first byte, and wait, at most [`crate::ANSWER_PATIENCE`],
//! for the answer.

use std::io::{IoSlice, IoSliceMut};
use std::mem::MaybeUninit;
use std::os::fd::{BorrowedFd, OwnedFd};
use std::path::Path;
use std::sync::{Arc, Mutex};

use hex_api::Refusal;
use hex_core::stream::{self, Deframer};
use hex_core::wire::{self, Received};
use rustix::io::{Errno, FdFlags};
use rustix::net::sockopt::Timeout;
use rustix::net::{
    AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer,
    SendAncillaryMessage, SendFlags, Shutdown, SocketAddrUnix, SocketType,
};

use crate::ANSWER_PATIENCE;
use crate::calls::{Core, Lent, Sent, Shared, Transport};

/// A node agent, reached through its socket.
pub type Client = Core<Socket>;

/// The node agent's socket, as a client's transport.
pub struct Socket {
    fd: OwnedFd,
    /// Held for a whole message's send, so two threads' messages never interleave on the stream.
    sending: Mutex<()>,
}

impl Transport for Socket {
    /// Sends `bytes` framed with the descriptors lent to it, which stay the caller's.
    fn send(&self, bytes: &[u8], lent: &[Lent<'_>]) -> Result<(), Sent> {
        let _whole = self.sending.lock().map_err(|_| {
            Sent::Refused(Refusal::Broken(
                "the client's send lock was poisoned".to_owned(),
            ))
        })?;
        send(&self.fd, bytes, lent)
    }

    fn shutdown(&self) {
        let _ = rustix::net::shutdown(&self.fd, Shutdown::Both);
    }
}

/// The reading thread: every message the agent sends, until the connection ends.
fn read(shared: &Shared<Socket>) {
    let fd = &shared.transport.fd;
    let mut buf = vec![0u8; stream::MAX_FRAME];
    let mut deframer = Deframer::new(wire::MAX_MESSAGE);
    // Descriptors that came, at the first byte of the message they ride with (XNU stops a stream read before a
    // record that carries them): a given answer's, kept for its exec; any other answer is refused for them.
    let mut fds: Vec<std::os::fd::OwnedFd> = Vec::new();
    let mut reset = None;
    let why = 'reading: loop {
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(wire::MAX_FDS))];
        let mut control = RecvAncillaryBuffer::new(&mut space);
        let msg = match rustix::net::recvmsg(
            fd,
            &mut [IoSliceMut::new(&mut buf)],
            &mut control,
            RecvFlags::empty(),
        ) {
            Ok(msg) => msg,
            Err(Errno::INTR) => continue,
            Err(Errno::CONNRESET) if reset.is_none() => {
                reset = Some(Refusal::Broken(
                    "the node agent closed the connection with requests unread".to_owned(),
                ));
                continue;
            }
            Err(e) => break Refusal::Broken(format!("reading the node agent's socket: {e}")),
        };
        let before = fds.len();
        for m in control.drain() {
            if let RecvAncillaryMessage::ScmRights(came) = m {
                fds.extend(came);
            }
        }
        let came = fds.len().saturating_sub(before);
        if msg.bytes == 0 && came == 0 {
            break reset.unwrap_or_else(|| {
                Refusal::Broken("the node agent closed the connection".to_owned())
            });
        }
        let mut input = buf.get(..msg.bytes).unwrap_or_default();
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
            let these = std::mem::take(&mut fds);
            let received = Received {
                bytes: &message,
                size: message.len(),
                descriptors: these.len(),
                descriptors_cut: false,
            };
            let delivered = wire::read_answer(&received, wire::MAX_MESSAGE)
                .map_err(|fault| {
                    Refusal::Broken(format!(
                        "the node agent sent a message that isn't an answer: {fault}"
                    ))
                })
                .and_then(|(header, answer)| shared.deliver_with(header, answer, these));
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
        let addr = SocketAddrUnix::new(path).map_err(|e| Refusal::Invalid {
            what: "socket path",
            why: format!("{}: {e}", path.display()),
        })?;
        let fd = rustix::net::socket(AddressFamily::UNIX, SocketType::STREAM, None)
            .map_err(|e| Refusal::os("making a socket", e))?;
        // macOS's `socket` takes no flags: close-on-exec set here, and no SIGPIPE when the agent goes.
        rustix::io::fcntl_setfd(&fd, FdFlags::CLOEXEC)
            .map_err(|e| Refusal::os("making a socket close-on-exec", e))?;
        rustix::net::sockopt::set_socket_nosigpipe(&fd, true)
            .map_err(|e| Refusal::os("making a socket's sends signal-free", e))?;
        // Bounds a send, and the connect itself, by the agent's room to take them.
        rustix::net::sockopt::set_socket_timeout(&fd, Timeout::Send, Some(ANSWER_PATIENCE))
            .map_err(|e| Refusal::os("bounding a socket's sends", e))?;
        rustix::net::connect(&fd, &addr).map_err(|e| Refusal::Os {
            op: "connecting to the node agent's socket",
            why: format!("{}: {e}", path.display()),
        })?;
        let shared = Arc::new(Shared::new(Socket {
            fd,
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

/// Sends one framed message whole, `fds` at its first byte.
fn send(fd: &OwnedFd, bytes: &[u8], fds: &[BorrowedFd<'_>]) -> Result<(), Sent> {
    let framed = stream::frame(bytes, wire::MAX_MESSAGE).map_err(|fault| {
        Sent::Refused(Refusal::Invalid {
            what: "message",
            why: fault.to_string(),
        })
    })?;
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(wire::MAX_FDS))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    if !fds.is_empty() && !control.push(SendAncillaryMessage::ScmRights(fds)) {
        return Err(Sent::Refused(Refusal::Invalid {
            what: "stdio",
            why: format!(
                "{} descriptors, past the {} a message carries",
                fds.len(),
                wire::MAX_FDS
            ),
        }));
    }
    let mut sent = 0usize;
    while sent < framed.len() {
        let rest = framed.get(sent..).unwrap_or_default();
        let n = if sent == 0 {
            rustix::net::sendmsg(fd, &[IoSlice::new(rest)], &mut control, SendFlags::empty())
        } else {
            rustix::net::send(fd, rest, SendFlags::empty())
        };
        match n {
            Ok(n) => sent = sent.saturating_add(n),
            Err(Errno::INTR) => {}
            Err(Errno::PIPE | Errno::CONNRESET | Errno::NOTCONN) => return Err(Sent::Gone),
            // Cut short mid-frame, the stream can't carry another message: the connection goes.
            Err(_) if sent > 0 => {
                let _ = rustix::net::shutdown(fd, Shutdown::Both);
                return Err(Sent::Gone);
            }
            Err(Errno::AGAIN) => {
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
