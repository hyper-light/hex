//! Linux's event sources and local socket for the node's socket core: a Unix `SOCK_SEQPACKET` socket
//! (unix(7)), which keeps each message whole; an exec's stdio passed as descriptors (`SCM_RIGHTS`); and one
//! epoll set over the listener, the connections, the programs' pidfds and an eventfd that wakes it.

use std::io::{IoSliceMut, PipeReader, PipeWriter};
use std::mem::MaybeUninit;
use std::os::fd::OwnedFd;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::Path;
use std::time::Duration;

use hex_api::{Refusal, Stdio};
use hex_core::wire::{self, Streams};
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

use super::{Accepted, EVENTS, Event, Interest, Os, Peer, Recv, Sent, Unwatched};

/// Connections waiting to be accepted: the I/O thread accepts them as they come.
const BACKLOG: i32 = 64;

/// The epoll tokens of the listener and the wake eventfd; the core's tokens start above them.
const LISTENER: u64 = 0;
const WAKE: u64 = 1;

/// The listener, the epoll set and the eventfd that wakes it.
pub struct Linux {
    epoll: OwnedFd,
    listener: OwnedFd,
    wake: OwnedFd,
}

impl Linux {
    /// A socket at `path`, readable and writable by this user alone, and its epoll set. A socket left
    /// at `path` by an agent that has ended is replaced; one an agent still serves, or a file that
    /// isn't a socket, is refused.
    pub fn bind(path: &Path) -> Result<Self, Refusal> {
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
        epoll::add(&epoll, &wake, EventData::new_u64(WAKE), EventFlags::IN)
            .map_err(|e| Refusal::os("watching the node agent's socket", e))?;
        epoll::add(
            &epoll,
            &listener,
            EventData::new_u64(LISTENER),
            EventFlags::empty(),
        )
        .map_err(|e| Refusal::os("watching the node agent's socket", e))?;
        Ok(Self {
            epoll,
            listener,
            wake,
        })
    }
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

fn flags(interest: Interest) -> EventFlags {
    match interest {
        Interest::Read => EventFlags::IN | EventFlags::RDHUP,
        Interest::Write => EventFlags::OUT | EventFlags::RDHUP,
    }
}

impl Os for Linux {
    type Conn = OwnedFd;
    type Watch = OwnedFd;
    type Passed = OwnedFd;

    fn wait(&self, events: &mut Vec<Event>, timeout: Option<Duration>) -> Result<(), Refusal> {
        let ts = timeout.map(|left| Timespec {
            tv_sec: i64::try_from(left.as_secs()).unwrap_or(i64::MAX),
            tv_nsec: i64::from(left.subsec_nanos()),
        });
        let mut got = Vec::with_capacity(EVENTS);
        match epoll::wait(&self.epoll, spare_capacity(&mut got), ts.as_ref()) {
            Ok(_) | Err(Errno::INTR) => {}
            Err(e) => return Err(Refusal::os("waiting on the socket's epoll set", e)),
        }
        events.extend(got.iter().map(|e| {
            // Copied out: epoll's event is packed on x86_64, so its fields can't be borrowed.
            let (flags, data) = (e.flags, e.data);
            match data.u64() {
                LISTENER => Event::Listener,
                WAKE => Event::Wake,
                token => Event::Ready {
                    token,
                    // RDHUP: its client half-closed, and what it sent before is read first.
                    readable: flags.intersects(EventFlags::IN | EventFlags::RDHUP),
                    writable: flags.contains(EventFlags::OUT),
                    hung_up: flags.intersects(EventFlags::HUP | EventFlags::ERR),
                },
            }
        }));
        Ok(())
    }

    fn wake(&self) -> Result<(), Refusal> {
        rustix::io::write(&self.wake, &1u64.to_ne_bytes())
            .map(drop)
            .map_err(|e| Refusal::os("waking the socket's thread", e))
    }

    fn accept(&self) -> Accepted<OwnedFd> {
        loop {
            return match rustix::net::accept_with(
                &self.listener,
                SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
            ) {
                Ok(fd) => Accepted::Conn(fd),
                Err(Errno::AGAIN) => Accepted::None,
                Err(Errno::INTR | Errno::CONNABORTED) => continue,
                Err(Errno::MFILE | Errno::NFILE | Errno::NOBUFS | Errno::NOMEM) => Accepted::Spent,
                Err(e) => Accepted::Failed(Refusal::os("accepting on the node agent's socket", e)),
            };
        }
    }

    fn accepting(&self, on: bool) -> Result<(), Refusal> {
        epoll::modify(
            &self.epoll,
            &self.listener,
            EventData::new_u64(LISTENER),
            if on {
                EventFlags::IN
            } else {
                EventFlags::empty()
            },
        )
        .map_err(|e| Refusal::os("pausing or resuming the node agent's socket", e))
    }

    fn stop_listening(&self) {
        let _ = epoll::delete(&self.epoll, &self.listener);
        let _ = epoll::delete(&self.epoll, &self.wake);
    }

    fn peer(&self, conn: &OwnedFd) -> Peer {
        let me = rustix::process::geteuid();
        match rustix::net::sockopt::socket_peercred(conn) {
            Ok(peer) if peer.uid == me || peer.uid.is_root() => Peer::Allowed,
            Ok(peer) => Peer::Refused(format!(
                "uid {} isn't the node agent's user ({})",
                peer.uid.as_raw(),
                me.as_raw()
            )),
            Err(_) => Peer::Gone,
        }
    }

    fn watch(&self, conn: &OwnedFd, token: u64, interest: Interest) -> Result<(), Refusal> {
        epoll::add(
            &self.epoll,
            conn,
            EventData::new_u64(token),
            flags(interest),
        )
        .map_err(|e| Refusal::os("watching a connection", e))
    }

    fn rewatch(&self, conn: &OwnedFd, token: u64, interest: Interest) -> Result<(), Refusal> {
        epoll::modify(
            &self.epoll,
            conn,
            EventData::new_u64(token),
            flags(interest),
        )
        .map_err(|e| Refusal::os("changing a connection's watch", e))
    }

    fn unwatch(&self, conn: &OwnedFd) {
        let _ = epoll::delete(&self.epoll, conn);
    }

    fn recv(&self, conn: &OwnedFd, buf: &mut [u8]) -> Recv<OwnedFd> {
        loop {
            let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(wire::MAX_FDS))];
            let mut control = RecvAncillaryBuffer::new(&mut space);
            let got = rustix::net::recvmsg(
                conn,
                &mut [IoSliceMut::new(buf)],
                &mut control,
                RecvFlags::DONTWAIT | RecvFlags::CMSG_CLOEXEC | RecvFlags::TRUNC,
            );
            let msg = match got {
                Ok(msg) => msg,
                Err(Errno::AGAIN) => return Recv::WouldBlock,
                Err(Errno::INTR) => continue,
                // A client that closed with answers unread leaves this error, reported once and before
                // what it sent (net/unix/af_unix.c, `unix_release_sock`), which is still there to read.
                Err(Errno::CONNRESET) => return Recv::Reset,
                Err(_) => return Recv::Gone,
            };
            let passed: Vec<OwnedFd> = control
                .drain()
                .filter_map(|m| match m {
                    RecvAncillaryMessage::ScmRights(fds) => Some(fds),
                    _ => None,
                })
                .flatten()
                .collect();
            return Recv::Message {
                size: msg.bytes,
                passed,
                cut: msg.flags.contains(ReturnFlags::CTRUNC),
            };
        }
    }

    fn send(&self, conn: &OwnedFd, bytes: &[u8]) -> Sent {
        loop {
            return match rustix::net::send(conn, bytes, SendFlags::DONTWAIT | SendFlags::NOSIGNAL) {
                Ok(_) => Sent::Sent,
                Err(Errno::INTR) => continue,
                Err(Errno::AGAIN) => Sent::WouldBlock,
                Err(_) => Sent::Gone,
            };
        }
    }

    fn shutdown(&self, conn: &OwnedFd) {
        let _ = rustix::net::shutdown(conn, Shutdown::Both);
    }

    fn hung_up(&self, conn: &OwnedFd) -> bool {
        let mut fds = [PollFd::new(conn, PollFlags::RDHUP | PollFlags::HUP)];
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

    fn watch_program(&self, pid: u32, token: u64) -> Result<OwnedFd, Unwatched> {
        let pidfd = i32::try_from(pid)
            .ok()
            .and_then(Pid::from_raw)
            .ok_or_else(|| Refusal::Broken(format!("a program's pid {pid} isn't one")))
            .and_then(|pid| {
                rustix::process::pidfd_open(pid, PidfdFlags::empty()).map_err(|e| Refusal::Os {
                    op: "watching a started program",
                    why: format!(
                        "{e}: it runs on unwatched by the socket, and the node's sweep reaps it"
                    ),
                })
            })
            .map_err(|why| Unwatched { why, killed: false })?;
        if let Err(e) = epoll::add(
            &self.epoll,
            &pidfd,
            EventData::new_u64(token),
            EventFlags::IN,
        ) {
            // It can't be watched: ended now, and the exec refused saying so.
            let _ = rustix::process::pidfd_send_signal(&pidfd, Signal::KILL);
            return Err(Unwatched {
                why: Refusal::Os {
                    op: "watching a started program",
                    why: format!("{e}: the node agent ended it"),
                },
                killed: true,
            });
        }
        Ok(pidfd)
    }

    fn kill(&self, watch: &OwnedFd) {
        // Fails only for one that has ended already, which is reaped and counted just the same.
        let _ = rustix::process::pidfd_send_signal(watch, Signal::KILL);
    }

    fn unwatch_program(&self, watch: OwnedFd) {
        // Closing the pidfd takes it out of the epoll set.
        drop(watch);
    }

    fn stdio(&self, streams: Streams, passed: Vec<OwnedFd>) -> Result<Stdio, Refusal> {
        stdio_of(streams, passed)
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
