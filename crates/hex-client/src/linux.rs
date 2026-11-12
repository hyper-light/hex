//! The client on Linux: one `SOCK_SEQPACKET` connection to the node agent's socket, which keeps each message whole
//! (unix(7)), an exec's stdio passed as `SCM_RIGHTS` descriptors, on the core every transport shares
//! (`crate::calls`): no thread of its own reads it, and a call waits for its answer until its caller's deadline,
//! or with none at most twice the slowest answer this process has had to a call of its kind ([`ANSWERS`]).

use std::io::{IoSlice, IoSliceMut};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;
use std::time::{Duration, Instant};

use hex_api::Refusal;
use hex_core::patience::Patience;
use hex_core::wire::{self, Received};
use rustix::io::Errno;
use rustix::net::{
    AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags,
    SendAncillaryBuffer, SendAncillaryMessage, SendFlags, Shutdown, SocketAddrUnix, SocketFlags,
    SocketType,
};

use crate::calls::{Core, Handle, KINDS, Lent, ReplyDeadline, Sent, Shared, Take, Transport};

/// How long a call waits for the node agent's answer, and its send for room in the agent's socket, one wait a kind
/// of call (`crate::calls::kind_of`'s order), since a delete waits on its box's emptying and a list on its parts,
/// so a fast inspect's answer says nothing of theirs: twice the slowest answer this process has had to a call of that
/// kind (decision 9; the lead's failure-bound rule of 2026-11-06), past which a call with no caller's deadline is
/// refused by type, the only way left to it (under one, a run-out recorded). Each kind is seeded by build profile
/// from its `client.answer.<kind>` rows of
/// `bench/competitive/probes/waits_census.md`, in nanoseconds as the census keeps them: a release build from run 4's
/// largest max of the release arms with a row's 1,000 samples (adacachy2), a debug build from run 5's largest answer
/// of the debug-build arms, the debug arm and the suites' (adacachy2), for the kind and the tests' waits seeded from
/// it, never under a bound one ran out at (decision 9). A kill, an exec asking for streams and a run have no rows
/// yet, nor do the scheduler operations: MEASURING until a census with them in records them.
static ANSWERS: [Patience; KINDS] = [
    Patience::by_profile(
        "client.answer.hello",
        Duration::from_nanos(1_177_037),
        Some(Duration::from_nanos(17_232_102)),
    ),
    Patience::by_profile(
        "client.answer.create",
        Duration::from_nanos(43_655_221),
        Some(Duration::from_nanos(49_798_121)),
    ),
    Patience::by_profile(
        "client.answer.start",
        Duration::from_nanos(11_263_989),
        Some(Duration::from_nanos(16_120_605)),
    ),
    Patience::by_profile(
        "client.answer.exec",
        Duration::from_nanos(17_330_544),
        Some(Duration::from_nanos(100_780_659)),
    ),
    Patience::by_profile(
        "client.answer.signal",
        Duration::from_nanos(583_198),
        Some(Duration::from_nanos(3_952_426)),
    ),
    Patience::by_profile(
        "client.answer.inspect",
        Duration::from_nanos(10_914_126),
        Some(Duration::from_nanos(12_007_514)),
    ),
    Patience::by_profile(
        "client.answer.usage",
        Duration::from_nanos(757_183),
        Some(Duration::from_nanos(386_833)),
    ),
    Patience::by_profile(
        "client.answer.list",
        Duration::from_nanos(386_711_080),
        Some(Duration::from_nanos(868_709_473)),
    ),
    Patience::by_profile(
        "client.answer.delete",
        Duration::from_nanos(35_715_197),
        Some(Duration::from_nanos(706_090_511)),
    ),
    Patience::by_profile(
        "client.answer.netns",
        Duration::from_nanos(715_105),
        Some(Duration::from_nanos(2_733_541)),
    ),
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
    Patience::measuring("client.answer.initialize_backing"),
    Patience::measuring("client.answer.release_backing"),
    Patience::measuring("client.answer.backing_usage"),
];

/// A node agent, reached through its socket.
pub type Client = Core<Socket>;

/// The node agent's socket, as a client's transport.
pub struct Socket {
    fd: OwnedFd,
}

/// What the turn's holder reads into, and the reset a close with requests unread left.
pub struct Reading {
    buf: Vec<u8>,
    /// An agent that closed the connection with a request of ours unread leaves a reset, reported once and before what
    /// it sent (net/unix/af_unix.c, `unix_release_sock`): its last answers, a refusal of the connection among them, are
    /// still there to read, and the reset is why the connection ended once they are.
    reset: Option<Refusal>,
}

impl Transport for Socket {
    type Reading = Reading;

    fn answers() -> &'static [Patience; KINDS] {
        &ANSWERS
    }

    fn reading(&self) -> Reading {
        Reading {
            buf: vec![0u8; wire::MAX_MESSAGE],
            reset: None,
        }
    }

    fn handle(&self) -> Handle<'_> {
        self.fd.as_fd()
    }

    /// One message, whole: `SOCK_SEQPACKET` keeps each as it was sent, and says its true size past the buffer.
    fn recv(&self, reading: &mut Reading, take: &mut Take<'_>) -> Result<bool, Refusal> {
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(wire::MAX_FDS))];
        let mut control = RecvAncillaryBuffer::new(&mut space);
        let msg = match rustix::net::recvmsg(
            &self.fd,
            &mut [IoSliceMut::new(&mut reading.buf)],
            &mut control,
            RecvFlags::CMSG_CLOEXEC | RecvFlags::TRUNC | RecvFlags::DONTWAIT,
        ) {
            Ok(msg) => msg,
            Err(Errno::INTR | Errno::AGAIN) => return Ok(false),
            Err(Errno::CONNRESET) if reading.reset.is_none() => {
                reading.reset = Some(Refusal::Broken(
                    "the node agent closed the connection with requests unread".to_owned(),
                ));
                return Ok(true);
            }
            Err(e) => {
                return Err(Refusal::Broken(format!(
                    "reading the node agent's socket: {e}"
                )));
            }
        };
        // Only a given answer carries descriptors: any that came with another are counted, closed, and refused.
        let mut fds: Vec<OwnedFd> = Vec::new();
        for m in control.drain() {
            if let RecvAncillaryMessage::ScmRights(came) = m {
                fds.extend(came);
            }
        }
        if msg.bytes == 0 && fds.is_empty() {
            return Err(reading.reset.take().unwrap_or_else(|| {
                Refusal::Broken("the node agent closed the connection".to_owned())
            }));
        }
        let received = Received {
            bytes: reading
                .buf
                .get(..msg.bytes.min(reading.buf.len()))
                .unwrap_or_default(),
            size: msg.bytes,
            descriptors: fds.len(),
            descriptors_cut: msg.flags.contains(ReturnFlags::CTRUNC),
        };
        take(&received, fds)?;
        Ok(true)
    }

    /// The whole message or none of it: a `SOCK_SEQPACKET` send never goes in part.
    fn try_send(&self, bytes: &[u8], at: &mut usize, lent: &[Lent<'_>]) -> Result<bool, Sent> {
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(wire::MAX_FDS))];
        let mut control = SendAncillaryBuffer::new(&mut space);
        if !lent.is_empty() && !control.push(SendAncillaryMessage::ScmRights(lent)) {
            return Err(Sent::Refused(Refusal::Invalid {
                what: "stdio",
                why: format!(
                    "{} descriptors, past the {} a message carries",
                    lent.len(),
                    wire::MAX_FDS
                ),
            }));
        }
        loop {
            match rustix::net::sendmsg(
                &self.fd,
                &[IoSlice::new(bytes)],
                &mut control,
                SendFlags::NOSIGNAL | SendFlags::DONTWAIT,
            ) {
                Ok(_) => {
                    *at = bytes.len();
                    return Ok(true);
                }
                Err(Errno::INTR) => {}
                Err(Errno::AGAIN) => return Ok(false),
                Err(Errno::PIPE | Errno::CONNRESET | Errno::NOTCONN) => return Err(Sent::Gone),
                Err(e) => return Err(Sent::Refused(Refusal::os("sending to the node agent", e))),
            }
        }
    }

    fn shutdown(&self) {
        let _ = rustix::net::shutdown(&self.fd, Shutdown::Both);
    }
}

impl Client {
    /// Connects to the node agent at `path` and learns what it runs and its bounds.
    pub fn connect(path: &Path) -> Result<Self, Refusal> {
        Core::hello(Shared::try_new(connected(path)?)?)
    }

    /// Connects as [`Client::connect`] does, its hello within the caller's original absolute `deadline`, which its
    /// kind's learned wait never ends sooner (AC-T7.4.1: a guest's relay asking its node, busy as the guest boots).
    pub fn connect_before(path: &Path, deadline: Instant) -> Result<Self, Refusal> {
        let deadline = ReplyDeadline::caller_before(deadline)?;
        Core::hello_before(Shared::try_new(connected(path)?)?, Some(deadline))
    }
}

/// A connection to the node agent's socket at `path`.
fn connected(path: &Path) -> Result<Socket, Refusal> {
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
    // A connect to a socket the agent listens on doesn't wait for it to accept (unix(7)); each call's send
    // is bounded by its call's deadline.
    rustix::net::connect(&fd, &addr).map_err(|e| Refusal::Os {
        op: "connecting to the node agent's socket",
        why: format!("{}: {e}", path.display()),
    })?;
    Ok(Socket { fd })
}

#[cfg(test)]
mod tests;
