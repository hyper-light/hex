//! The client on Linux: one `SOCK_SEQPACKET` connection to the node agent's socket, which keeps each message whole
//! (unix(7)), an exec's stdio passed as `SCM_RIGHTS` descriptors, on the core every transport shares
//! (`crate::calls`): no thread of its own reads it, and a call waits for its answer at most twice the slowest
//! answer this process has had to a call of its kind ([`ANSWERS`]).

use std::io::{IoSlice, IoSliceMut};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use hex_api::Refusal;
use hex_core::patience::Patience;
use hex_core::wire::{self, Received};
use rustix::io::Errno;
use rustix::net::{
    AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags,
    SendAncillaryBuffer, SendAncillaryMessage, SendFlags, Shutdown, SocketAddrUnix, SocketFlags,
    SocketType,
};

use crate::calls::{Core, Handle, KINDS, Lent, Sent, Shared, Transport};

/// How long a call waits for the node agent's answer, and its send for room in the agent's socket, one wait a kind
/// of call (`crate::calls::kind_of`'s order), since a delete waits on its box's emptying and a list on its parts,
/// so a fast inspect's answer says nothing of theirs: twice the slowest answer this process has had to a call of that
/// kind (decision 9; the lead's failure-bound rule of 2026-11-06), past which the call is refused by type, the only
/// way left to it. Each kind is seeded by build profile from its `client.answer.<kind>` rows of
/// `bench/competitive/probes/waits_census.md`, in nanoseconds as the census keeps them: a release build from run 4's
/// largest max of the release arms with a row's 1,000 samples (adacachy2), a debug build from run 3's debug arm's
/// (fedora) until run 5 records the suites' run-outs (decision 9). A kill and an exec asking for streams have no rows
/// yet: MEASURING until a census with them in records them.
static ANSWERS: [Patience; KINDS] = [
    Patience::by_profile(
        "client.answer.hello",
        Duration::from_nanos(1_177_037),
        Some(Duration::from_nanos(10_396_345)),
    ),
    Patience::by_profile(
        "client.answer.create",
        Duration::from_nanos(43_655_221),
        Some(Duration::from_nanos(220_155_458)),
    ),
    Patience::by_profile(
        "client.answer.start",
        Duration::from_nanos(11_263_989),
        Some(Duration::from_nanos(7_610_223)),
    ),
    Patience::by_profile(
        "client.answer.exec",
        Duration::from_nanos(17_330_544),
        Some(Duration::from_nanos(38_753_534)),
    ),
    Patience::by_profile(
        "client.answer.signal",
        Duration::from_nanos(583_198),
        Some(Duration::from_nanos(273_003)),
    ),
    Patience::by_profile(
        "client.answer.inspect",
        Duration::from_nanos(10_914_126),
        Some(Duration::from_nanos(22_023_765)),
    ),
    Patience::by_profile(
        "client.answer.usage",
        Duration::from_nanos(757_183),
        Some(Duration::from_nanos(162_395)),
    ),
    Patience::by_profile(
        "client.answer.list",
        Duration::from_nanos(386_711_080),
        Some(Duration::from_nanos(1_439_696_712)),
    ),
    Patience::by_profile(
        "client.answer.delete",
        Duration::from_nanos(35_715_197),
        Some(Duration::from_nanos(306_051_519)),
    ),
    Patience::by_profile(
        "client.answer.netns",
        Duration::from_nanos(715_105),
        Some(Duration::from_nanos(8_048_226)),
    ),
    Patience::measuring("client.answer.kill"),
    Patience::measuring("client.answer.exec_streams"),
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
    fn recv(
        &self,
        reading: &mut Reading,
        take: &mut dyn FnMut(&Received<'_>, Vec<hex_api::End>) -> Result<(), Refusal>,
    ) -> Result<bool, Refusal> {
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
        // is bounded by its kind's learned wait.
        rustix::net::connect(&fd, &addr).map_err(|e| Refusal::Os {
            op: "connecting to the node agent's socket",
            why: format!("{}: {e}", path.display()),
        })?;
        Core::hello(Arc::new(Shared::new(Socket { fd })))
    }
}

#[cfg(test)]
mod tests;
