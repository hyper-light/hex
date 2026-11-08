//! The client on Linux: one `SOCK_SEQPACKET` connection to the node agent's socket, a thread that reads
//! every message the agent sends and hands it to [`crate::calls`], and calls that send their request
//! (with an exec's stdio as `SCM_RIGHTS` descriptors) and wait, at most [`crate::ANSWER_PATIENCE`], for
//! the answer.

use std::io::{IoSlice, IoSliceMut};
use std::mem::MaybeUninit;
use std::os::fd::{BorrowedFd, OwnedFd};
use std::path::Path;
use std::sync::Arc;

use hex_api::Refusal;
use hex_core::wire::{self, Received};
use rustix::io::Errno;
use rustix::net::sockopt::Timeout;
use rustix::net::{
    AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags,
    SendAncillaryBuffer, SendAncillaryMessage, SendFlags, Shutdown, SocketAddrUnix, SocketFlags,
    SocketType,
};

use crate::ANSWER_PATIENCE;
use crate::calls::{Core, Lent, Sent, Shared, Transport};

/// A node agent, reached through its socket.
pub type Client = Core<Socket>;

/// The node agent's socket, as a client's transport.
pub struct Socket {
    fd: OwnedFd,
}

impl Transport for Socket {
    /// Sends `bytes` with the descriptors lent to it, which stay the caller's.
    fn send(&self, bytes: &[u8], lent: &[Lent<'_>]) -> Result<(), Sent> {
        send(&self.fd, bytes, lent)
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

fn send(fd: &OwnedFd, bytes: &[u8], fds: &[BorrowedFd<'_>]) -> Result<(), Sent> {
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
    use hex_api::{BoxId, Class, Exit, Program, Runtime, Stdio};
    use hex_core::wire::{Answer, Call, Header, Limits, Request, Streams};
    use rustix::event::{PollFd, PollFlags, Timespec};
    use std::io::{Read, Write};
    use std::os::fd::AsFd;
    use std::time::Duration;

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

    /// The next request on `conn` and the descriptors that came with it, waiting at most 30 s for it.
    fn next_request_with(conn: &OwnedFd) -> (Header, Request, Vec<OwnedFd>) {
        let mut ready = [PollFd::new(conn, PollFlags::IN)];
        let ts = Timespec {
            tv_sec: 30,
            tv_nsec: 0,
        };
        assert_eq!(rustix::event::poll(&mut ready, Some(&ts)).unwrap(), 1);
        let mut buf = vec![0u8; wire::MAX_MESSAGE];
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(wire::MAX_FDS))];
        let mut control = RecvAncillaryBuffer::new(&mut space);
        let msg = rustix::net::recvmsg(
            conn,
            &mut [IoSliceMut::new(&mut buf)],
            &mut control,
            RecvFlags::CMSG_CLOEXEC,
        )
        .unwrap();
        let mut fds = Vec::new();
        for m in control.drain() {
            if let RecvAncillaryMessage::ScmRights(came) = m {
                fds.extend(came);
            }
        }
        let received = Received {
            bytes: &buf[..msg.bytes],
            size: msg.bytes,
            descriptors: fds.len(),
            descriptors_cut: false,
        };
        let (h, request) = wire::read_request(&received, wire::MAX_MESSAGE).unwrap();
        (h, request, fds)
    }

    /// Sends `answer` to request `h`, in its version, with `fds` riding as its descriptors.
    fn answer_with(conn: &OwnedFd, h: Header, answer: &Answer, fds: &[BorrowedFd<'_>]) {
        let bytes = wire::answer(h.version, h.id, answer, wire::MAX_MESSAGE).unwrap();
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(wire::MAX_FDS))];
        let mut control = SendAncillaryBuffer::new(&mut space);
        if !fds.is_empty() {
            assert!(control.push(SendAncillaryMessage::ScmRights(fds)));
        }
        rustix::net::sendmsg(
            conn,
            &[IoSlice::new(&bytes)],
            &mut control,
            SendFlags::NOSIGNAL,
        )
        .unwrap();
    }

    /// Native plan step 8, guest stdio (HEX-GUEST-41): an agent whose program can't take the caller's own pipes
    /// (a guest's, whose streams are connections of their own) lets them go and answers the exec with its streams'
    /// ends; expect the client to copy between those ends and the caller's pipes, so what the caller writes to its
    /// stdin pipe comes back on its stdout pipe, to its end, and the program's end comes as the agent sent it. The
    /// one copy left runs in the caller's client, never in the node.
    #[test]
    fn the_client_copies_between_its_callers_pipes_and_the_ends_its_agent_gives() {
        let (_dir, path, listener) = agent_socket();
        let agent = std::thread::spawn(move || {
            let conn = rustix::net::accept_with(&listener, SocketFlags::CLOEXEC).unwrap();
            let (h, request, _) = next_request_with(&conn);
            assert_eq!(request, Request::Hello);
            let hello = Answer::Hello(wire::Hello {
                version: wire::VERSION,
                classes: vec![Class::Oci],
                limits: Limits {
                    connections: 1,
                    message_bytes: wire::MAX_MESSAGE,
                    in_flight: 1,
                    programs: 1,
                },
            });
            answer_with(&conn, h, &hello, &[]);
            let (h, request, carried) = next_request_with(&conn);
            let Request::Call(Call::Exec { streams, .. }) = request else {
                panic!("expected an exec, got {request:?}");
            };
            assert!(streams.stdin && streams.stdout && !streams.stderr);
            assert_eq!(carried.len(), 2);
            // A guest's program can't take them: let go here, as a guest's node lets them go.
            drop(carried);
            let pair = || {
                rustix::net::socketpair(
                    AddressFamily::UNIX,
                    SocketType::STREAM,
                    SocketFlags::CLOEXEC,
                    None,
                )
                .unwrap()
            };
            let ((stdin, stdin_end), (stdout, stdout_end)) = (pair(), pair());
            let given = Answer::Given {
                pid: 42,
                given: Streams {
                    stdin: true,
                    stdout: true,
                    stderr: false,
                },
            };
            answer_with(&conn, h, &given, &[stdin_end.as_fd(), stdout_end.as_fd()]);
            drop((stdin_end, stdout_end));
            // The program: its stdin to its stdout, to its end, then its own end.
            std::io::copy(
                &mut std::fs::File::from(stdin),
                &mut std::fs::File::from(stdout),
            )
            .unwrap();
            answer_with(&conn, h, &Answer::Exited(Exit::Code(0)), &[]);
            // Held open until the client has gone, as an agent's connection is.
            let mut rest = [0u8; 64];
            let _ = rustix::net::recv(&conn, &mut rest, RecvFlags::empty());
        });
        let client = Client::connect(&path).unwrap();
        let (in_r, mut in_w) = std::io::pipe().unwrap();
        let (mut out_r, out_w) = std::io::pipe().unwrap();
        let mut proc = client
            .exec(
                &BoxId::new("g").unwrap(),
                Program::of(&["cat"]),
                Stdio {
                    stdin: Some(in_r),
                    stdout: Some(out_w),
                    ..Stdio::null()
                },
            )
            .unwrap();
        let wrote = in_w.write_all(b"round trip\n");
        drop(in_w);
        let mut out = String::new();
        let read = out_r.read_to_string(&mut out);
        assert!(
            wrote.is_ok() && read.is_ok() && out == "round trip\n",
            "the caller's pipes carried nothing: wrote {wrote:?}, read {read:?}, {out:?}"
        );
        assert_eq!(
            proc.wait_for(Duration::from_secs(30)).unwrap(),
            Some(Exit::Code(0))
        );
        drop(proc);
        drop(client);
        agent.join().unwrap();
    }

    /// Native plan step 8, guest stdio (HEX-GUEST-41): an agent that answers an exec with the end of a stream the
    /// exec neither carried nor asked for: expect the exec refused as broken, naming the stream, and its program
    /// killed, never an end handed to a caller that didn't ask for it.
    #[test]
    fn an_end_of_a_stream_its_exec_neither_carried_nor_asked_for_is_refused() {
        let (_dir, path, listener) = agent_socket();
        let agent = std::thread::spawn(move || {
            let conn = rustix::net::accept_with(&listener, SocketFlags::CLOEXEC).unwrap();
            let (h, _, _) = next_request_with(&conn);
            let hello = Answer::Hello(wire::Hello {
                version: wire::VERSION,
                classes: vec![Class::Oci],
                limits: Limits {
                    connections: 1,
                    message_bytes: wire::MAX_MESSAGE,
                    in_flight: 1,
                    programs: 1,
                },
            });
            answer_with(&conn, h, &hello, &[]);
            let (exec, request, _) = next_request_with(&conn);
            assert!(
                matches!(&request, Request::Call(Call::Exec { streams, .. }) if *streams == Streams::default()),
                "{request:?}"
            );
            let (_here, end) = rustix::net::socketpair(
                AddressFamily::UNIX,
                SocketType::STREAM,
                SocketFlags::CLOEXEC,
                None,
            )
            .unwrap();
            let given = Answer::Given {
                pid: 42,
                given: Streams {
                    stdout: true,
                    ..Streams::default()
                },
            };
            answer_with(&conn, exec, &given, &[end.as_fd()]);
            let (k, kill, _) = next_request_with(&conn);
            answer_with(&conn, k, &Answer::Done, &[]);
            (exec.id, kill)
        });
        let client = Client::connect(&path).unwrap();
        let got = client.exec(
            &BoxId::new("g").unwrap(),
            Program::of(&["true"]),
            Stdio::null(),
        );
        assert!(
            matches!(&got, Err(Refusal::Broken(why)) if why.contains("stdout") && why.contains("neither carried nor asked")),
            "{:?}",
            got.as_ref().err()
        );
        let (exec, kill) = agent.join().unwrap();
        assert_eq!(kill, Request::Call(Call::Kill { exec }));
    }
}
