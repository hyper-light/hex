//! The client on Linux: one `SOCK_SEQPACKET` connection to the node agent's socket, a thread that reads
//! every message the agent sends and hands it to [`crate::calls`], and calls that send their request
//! (with an exec's stdio as `SCM_RIGHTS` descriptors) and wait, at most [`crate::ANSWER_PATIENCE`], for
//! the answer.

use std::io::{IoSlice, IoSliceMut};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::Path;
use std::sync::Arc;

use hex_api::{Refusal, Stdio};
use hex_core::wire::{self, Received};
use rustix::io::Errno;
use rustix::net::sockopt::Timeout;
use rustix::net::{
    AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags,
    SendAncillaryBuffer, SendAncillaryMessage, SendFlags, Shutdown, SocketAddrUnix, SocketFlags,
    SocketType,
};

use crate::ANSWER_PATIENCE;
use crate::calls::{Core, Sent, Shared, Transport};

/// A node agent, reached through its socket.
pub type Client = Core<Socket>;

/// The node agent's socket, as a client's transport.
pub struct Socket {
    fd: OwnedFd,
}

impl Transport for Socket {
    type Stdio = Vec<OwnedFd>;

    fn stdio(&self, _exec: u64, stdio: Stdio) -> Result<Self::Stdio, Refusal> {
        Ok([
            stdio.stdin.map(OwnedFd::from),
            stdio.stdout.map(OwnedFd::from),
            stdio.stderr.map(OwnedFd::from),
        ]
        .into_iter()
        .flatten()
        .collect())
    }

    /// Sends `bytes` with `fds`, closing this side's copies of them once sent.
    fn send(&self, bytes: &[u8], fds: Option<Vec<OwnedFd>>) -> Result<(), Sent> {
        send(&self.fd, bytes, &fds.unwrap_or_default())
    }

    fn shutdown(&self) {
        let _ = rustix::net::shutdown(&self.fd, Shutdown::Both);
    }
}

/// The reading thread: every message the agent sends, until the connection ends.
fn read(shared: &Shared<Socket>) {
    let fd = &shared.transport.fd;
    let mut buf = vec![0u8; wire::MAX_MESSAGE];
    let mut reset = None;
    let why = loop {
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(wire::MAX_FDS))];
        let mut control = RecvAncillaryBuffer::new(&mut space);
        let msg = match rustix::net::recvmsg(
            fd,
            &mut [IoSliceMut::new(&mut buf)],
            &mut control,
            RecvFlags::CMSG_CLOEXEC | RecvFlags::TRUNC,
        ) {
            Ok(msg) => msg,
            Err(Errno::INTR) => continue,
            // An agent that closed the connection with a request of ours unread leaves this error,
            // reported once and before what it sent (net/unix/af_unix.c, `unix_release_sock`):
            // its last answers, a refusal of the connection among them, are still there to read.
            Err(Errno::CONNRESET) if reset.is_none() => {
                reset = Some(Refusal::Broken(
                    "the node agent closed the connection with requests unread".to_owned(),
                ));
                continue;
            }
            Err(e) => break Refusal::Broken(format!("reading the node agent's socket: {e}")),
        };
        // Only a given answer carries descriptors: any that came with another are counted, closed, and
        // refused below.
        let mut fds = Vec::new();
        for m in control.drain() {
            if let RecvAncillaryMessage::ScmRights(came) = m {
                fds.extend(came);
            }
        }
        let descriptors = fds.len();
        if msg.bytes == 0 && descriptors == 0 {
            break reset.unwrap_or_else(|| {
                Refusal::Broken("the node agent closed the connection".to_owned())
            });
        }
        let received = Received {
            bytes: buf.get(..msg.bytes.min(buf.len())).unwrap_or_default(),
            size: msg.bytes,
            descriptors,
            descriptors_cut: msg.flags.contains(ReturnFlags::CTRUNC),
        };
        let delivered = wire::read_answer(&received, wire::MAX_MESSAGE)
            .map_err(|fault| {
                Refusal::Broken(format!(
                    "the node agent sent a message that isn't an answer: {fault}"
                ))
            })
            .and_then(|(header, answer)| shared.deliver_with(header, answer, fds));
        if let Err(why) = delivered {
            break why;
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
        let fd = rustix::net::socket_with(
            AddressFamily::UNIX,
            SocketType::SEQPACKET,
            SocketFlags::CLOEXEC,
            None,
        )
        .map_err(|e| Refusal::os("making a socket", e))?;
        // Bounds a send, and the connect itself, by the agent's room to take them.
        rustix::net::sockopt::set_socket_timeout(&fd, Timeout::Send, Some(ANSWER_PATIENCE))
            .map_err(|e| Refusal::os("bounding a socket's sends", e))?;
        rustix::net::connect(&fd, &addr).map_err(|e| Refusal::Os {
            op: "connecting to the node agent's socket",
            why: format!("{}: {e}", path.display()),
        })?;
        let shared = Arc::new(Shared::new(Socket { fd }));
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

fn send(fd: &OwnedFd, bytes: &[u8], fds: &[OwnedFd]) -> Result<(), Sent> {
    let borrowed: Vec<BorrowedFd<'_>> = fds.iter().map(AsFd::as_fd).collect();
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(wire::MAX_FDS))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    if !borrowed.is_empty() && !control.push(SendAncillaryMessage::ScmRights(&borrowed)) {
        return Err(Sent::Refused(Refusal::Invalid {
            what: "stdio",
            why: format!(
                "{} descriptors, past the {} a message carries",
                borrowed.len(),
                wire::MAX_FDS
            ),
        }));
    }
    loop {
        match rustix::net::sendmsg(
            fd,
            &[IoSlice::new(bytes)],
            &mut control,
            SendFlags::NOSIGNAL,
        ) {
            Ok(_) => return Ok(()),
            Err(Errno::INTR) => continue,
            Err(Errno::PIPE | Errno::CONNRESET | Errno::NOTCONN) => return Err(Sent::Gone),
            Err(Errno::AGAIN) => {
                return Err(Sent::Refused(Refusal::Os {
                    op: "sending to the node agent",
                    why: format!("no room in its socket for {ANSWER_PATIENCE:?}: it isn't reading"),
                }));
            }
            Err(e) => {
                return Err(Sent::Refused(Refusal::os("sending to the node agent", e)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_api::{BoxId, Class, Runtime};
    use hex_core::wire::{Answer, Call, Header, Limits, Request};
    use rustix::event::{PollFd, PollFlags, Timespec};

    /// A stand-in agent's listening socket, in a directory of its own.
    fn agent_socket() -> (tempfile::TempDir, std::path::PathBuf, OwnedFd) {
        let cache = std::path::PathBuf::from(std::env::var_os("HOME").unwrap())
            .join(".cache/hex-t7/client");
        std::fs::create_dir_all(&cache).unwrap();
        let dir = tempfile::Builder::new()
            .prefix("agent-")
            .tempdir_in(&cache)
            .unwrap();
        let path = dir.path().join("node.sock");
        let listener = rustix::net::socket_with(
            AddressFamily::UNIX,
            SocketType::SEQPACKET,
            SocketFlags::CLOEXEC,
            None,
        )
        .unwrap();
        rustix::net::bind(&listener, &SocketAddrUnix::new(&path).unwrap()).unwrap();
        rustix::net::listen(&listener, 1).unwrap();
        (dir, path, listener)
    }

    /// The next request on `conn`, waiting at most 30 s for it.
    fn next_request(conn: &OwnedFd) -> (Header, Request) {
        let mut ready = [PollFd::new(conn, PollFlags::IN)];
        let ts = Timespec {
            tv_sec: 30,
            tv_nsec: 0,
        };
        assert_eq!(rustix::event::poll(&mut ready, Some(&ts)).unwrap(), 1);
        let mut buf = vec![0u8; wire::MAX_MESSAGE];
        let (n, _) = rustix::net::recv(conn, &mut buf, RecvFlags::empty()).unwrap();
        let msg = Received {
            bytes: &buf[..n],
            size: n,
            descriptors: 0,
            descriptors_cut: false,
        };
        wire::read_request(&msg, wire::MAX_MESSAGE).unwrap()
    }

    /// HEX-SOCK-4: an agent that refuses a connection and closes it with the client's hello unread
    /// leaves the client a reset, reported once and before the refusal it sent; the client still reads
    /// the refusal, so connect fails with it, typed, rather than with the reset.
    #[test]
    fn a_refusal_before_a_reset_is_the_one_connect_returns() {
        let (_dir, path, listener) = agent_socket();
        let refusal = Refusal::Exhausted {
            what: "connections to the node agent's socket",
            bound: 2,
        };
        let agent = {
            let refusal = refusal.clone();
            std::thread::spawn(move || {
                let conn = rustix::net::accept_with(&listener, SocketFlags::CLOEXEC).unwrap();
                // The client's hello is in this side's queue, unread, before the refusal goes.
                let mut hello = [PollFd::new(&conn, PollFlags::IN)];
                let ts = Timespec {
                    tv_sec: 30,
                    tv_nsec: 0,
                };
                assert_eq!(rustix::event::poll(&mut hello, Some(&ts)).unwrap(), 1);
                // In the first version, as the node agent refuses a connection.
                let bytes =
                    wire::answer(wire::FIRST, 0, &Answer::Refused(refusal), wire::MAX_MESSAGE)
                        .unwrap();
                rustix::net::send(&conn, &bytes, SendFlags::NOSIGNAL).unwrap();
            })
        };
        assert_eq!(Client::connect(&path).err(), Some(refusal));
        agent.join().unwrap();
    }

    /// HEX-SOCK-2, append-only: a client of version 2 serves the seam to an agent of version 1. Its
    /// hello goes in version 1; told the agent speaks version 1 alone, it writes every call in
    /// version 1, and the network namespace counts, which version 1 doesn't define, are refused by
    /// type without a message sent: the agent's next request is the call after them.
    #[test]
    fn a_version_1_agent_is_served_in_version_1() {
        let (_dir, path, listener) = agent_socket();
        let agent = std::thread::spawn(move || {
            let conn = rustix::net::accept_with(&listener, SocketFlags::CLOEXEC).unwrap();
            let mut seen = Vec::new();
            for _ in 0..3 {
                let (h, request) = next_request(&conn);
                let answer = match request {
                    Request::Hello => Answer::Hello(wire::Hello {
                        version: 1,
                        classes: vec![Class::Process],
                        limits: Limits {
                            connections: 1,
                            message_bytes: wire::MAX_MESSAGE,
                            in_flight: 1,
                            programs: 1,
                        },
                    }),
                    Request::Call(Call::Inspect(id)) => Answer::Refused(Refusal::NoSuchBox(id)),
                    other => panic!("a version 1 agent was sent {other:?}"),
                };
                let bytes = wire::answer(1, h.id, &answer, wire::MAX_MESSAGE).unwrap();
                rustix::net::send(&conn, &bytes, SendFlags::NOSIGNAL).unwrap();
                seen.push((h.version, h.kind));
            }
            seen
        });
        let client = Client::connect(&path).unwrap();
        let b = BoxId::new("b").unwrap();
        assert_eq!(client.inspect(&b), Err(Refusal::NoSuchBox(b.clone())));
        assert_eq!(
            client.netns(),
            Err(Refusal::Invalid {
                what: "message",
                why: "kind 0x000b isn't a request version 1 defines".to_owned(),
            })
        );
        assert_eq!(client.inspect(&b), Err(Refusal::NoSuchBox(b.clone())));
        assert_eq!(
            agent.join().unwrap(),
            [(1, 0x0001), (1, 0x0006), (1, 0x0006)],
            "the hello and both inspects, each in version 1, and nothing between them"
        );
    }
}
