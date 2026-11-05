//! The node's client to the macOS box helper (native plan step 2). The node agent runs unprivileged; to
//! run a box's programs as a pool user, arm their physical-footprint ledger limits and catch their Mach
//! exceptions it reaches the privileged helper (a launchd daemon) over a Unix `SOCK_STREAM` socket, with
//! the typed, bounded protocol of [`hex_core::helper`]. macOS local sockets have no `SOCK_SEQPACKET`, so
//! each message is framed with a `u32` length; a `Start`'s three stdio descriptors ride with it by
//! `SCM_RIGHTS`. Synchronous: one request, one answer, as the node drives the seam in process.
//!
//! No `unsafe`: rustix wraps the socket calls (`socket`, `connect`, `sendmsg`, `recvmsg`), so this isn't a
//! file CLAUDE.md §12 names. The helper daemon's own privileged calls (setuid-spawn, the ledger limit, the
//! exception server) are the §12 file's.

use std::io::{IoSlice, IoSliceMut};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use hex_api::{BoxId, Program, Refusal};
use hex_core::helper::{self, Answer, MAX_MESSAGE, Refused, Request, START_FDS, State, VERSION};
use rustix::io::Errno;
use rustix::net::{
    AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer,
    SendAncillaryMessage, SendFlags, SocketAddrUnix, SocketType,
};

/// The four bytes of a message's length prefix on the stream.
const LEN_BYTES: usize = 4;

/// A connection to the box helper. Dropping it closes the socket; the helper ends the boxes it was
/// managing for this node on its own (its own disconnect rule), as the Linux node's socket does.
pub struct Helper {
    fd: OwnedFd,
    /// The version both sides settled on at [`Helper::connect`].
    version: u16,
    /// The id of the next request, so an answer is matched to its request.
    next_id: AtomicU64,
    /// Where the helper listens, for messages and refusals that name it.
    path: PathBuf,
}

impl Helper {
    /// Connects to the helper at `path`. `Ok(None)` when no helper is there (the socket is absent or
    /// refuses): the node then runs the rootless `process`-group class (step 1). `Ok(Some)` once the
    /// version handshake answers.
    pub fn connect(path: &Path) -> Result<Option<Self>, Refusal> {
        let fd = match rustix::net::socket(AddressFamily::UNIX, SocketType::STREAM, None) {
            Ok(fd) => fd,
            Err(e) => return Err(Refusal::os("making the helper socket", e)),
        };
        // macOS has no MSG_NOSIGNAL on send; SO_NOSIGPIPE makes a write to a closed helper an EPIPE error,
        // not a SIGPIPE that would kill the node (best-effort: a failure here only risks the signal).
        let _ = rustix::net::sockopt::set_socket_nosigpipe(&fd, true);
        let addr =
            SocketAddrUnix::new(path).map_err(|e| Refusal::os("naming the helper socket", e))?;
        match rustix::net::connect(&fd, &addr) {
            Ok(()) => {}
            // No helper installed, or it isn't listening: run rootless.
            Err(Errno::NOENT | Errno::CONNREFUSED | Errno::CONNRESET | Errno::AGAIN) => {
                return Ok(None);
            }
            Err(e) => return Err(Refusal::os("reaching the helper", e)),
        }
        let helper = Self {
            fd,
            version: VERSION,
            next_id: AtomicU64::new(1),
            path: path.to_path_buf(),
        };
        // Settle the version both speak; the helper answers the highest it and we share.
        match helper.rpc(&Request::Hello { version: VERSION }, &[])? {
            Answer::Hello { version } if version >= 1 && version <= VERSION => {
                Ok(Some(Self { version, ..helper }))
            }
            Answer::Hello { version } => Err(Refusal::Broken(format!(
                "the helper speaks version {version}, past this node's {VERSION}"
            ))),
            other => Err(unexpected("hello", &other)),
        }
    }

    /// A client over an already-connected socket at `version`, without the hello handshake: for a caller
    /// that connected the socket itself (a test's socketpair to a daemon in the same process).
    #[cfg(test)]
    pub(crate) fn over(fd: OwnedFd, version: u16) -> Self {
        Self {
            fd,
            version,
            next_id: AtomicU64::new(1),
            path: PathBuf::from("<connected>"),
        }
    }

    /// Takes a pool user for `box_id` with memory bound and reserve in bytes; returns the user's id.
    pub fn acquire(&self, box_id: &BoxId, bound: u64, reserve: u64) -> Result<u32, Refusal> {
        match self.rpc(
            &Request::Acquire {
                box_id: box_id.clone(),
                bound,
                reserve,
            },
            &[],
        )? {
            Answer::Acquired { uid } => Ok(uid),
            Answer::Refused(r) => Err(refusal(r)),
            other => Err(unexpected("acquire", &other)),
        }
    }

    /// Starts `program` as the box's pool user, armed, its three stdio descriptors riding with the
    /// request; returns its pid.
    pub fn start(
        &self,
        box_id: &BoxId,
        program: &Program,
        stdio: [BorrowedFd<'_>; START_FDS],
        leader: bool,
    ) -> Result<i32, Refusal> {
        let fds: Vec<BorrowedFd<'_>> = stdio.to_vec();
        match self.rpc(
            &Request::Start {
                box_id: box_id.clone(),
                program: program.clone(),
                leader,
            },
            &fds,
        )? {
            Answer::Started { pid } => Ok(pid),
            Answer::Refused(r) => Err(refusal(r)),
            other => Err(unexpected("start", &other)),
        }
    }

    /// Ends the box: the helper kills every process of its user and frees the user back to the pool.
    pub fn end(&self, box_id: &BoxId) -> Result<(), Refusal> {
        match self.rpc(
            &Request::End {
                box_id: box_id.clone(),
            },
            &[],
        )? {
            Answer::Ended => Ok(()),
            Answer::Refused(r) => Err(refusal(r)),
            other => Err(unexpected("end", &other)),
        }
    }

    /// Sends `signal` (a Linux signal number) to every member of the box; the helper maps it to macOS's.
    pub fn signal(&self, box_id: &BoxId, signal: i32) -> Result<(), Refusal> {
        match self.rpc(
            &Request::Signal {
                box_id: box_id.clone(),
                signal,
            },
            &[],
        )? {
            Answer::Signalled => Ok(()),
            Answer::Refused(r) => Err(refusal(r)),
            other => Err(unexpected("signal", &other)),
        }
    }

    /// What the box holds now.
    pub fn inspect(&self, box_id: &BoxId) -> Result<State, Refusal> {
        match self.rpc(
            &Request::Inspect {
                box_id: box_id.clone(),
            },
            &[],
        )? {
            Answer::State(s) => Ok(s),
            Answer::Refused(r) => Err(refusal(r)),
            other => Err(unexpected("inspect", &other)),
        }
    }

    /// One request and its answer. Frames the request (its `u32` length, then its bytes), sends it with
    /// `fds` riding by `SCM_RIGHTS`, reads the framed answer, and checks it carries the request's id.
    fn rpc(&self, request: &Request, fds: &[BorrowedFd<'_>]) -> Result<Answer, Refusal> {
        if fds.len() != request.descriptors() {
            return Err(Refusal::Invalid {
                what: "helper request",
                why: format!(
                    "{} carries {} descriptors, not the {} it takes",
                    self.path.display(),
                    fds.len(),
                    request.descriptors()
                ),
            });
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let msg =
            helper::encode_request(self.version, id, request).map_err(|e| Refusal::Invalid {
                what: "helper request",
                why: e.to_string(),
            })?;
        send_framed(self.fd.as_fd(), &msg, fds)?;
        let (bytes, extra) = recv_framed(self.fd.as_fd())?;
        // An answer carries no descriptors; close any the helper wrongly sent rather than leak them.
        drop(extra);
        let (_, answer_id, answer) = helper::read_answer(&bytes)
            .map_err(|e| Refusal::Broken(format!("the helper's answer is malformed: {e}")))?;
        if answer_id != id {
            return Err(Refusal::Broken(format!(
                "the helper answered request {answer_id}, not {id}"
            )));
        }
        Ok(answer)
    }
}

/// Maps a helper's typed refusal to the node's [`Refusal`], each by name (CLAUDE.md §3).
fn refusal(r: Refused) -> Refusal {
    match r {
        Refused::UnknownBox(b) => Refusal::NoSuchBox(b),
        Refused::NotRunning(b) => Refusal::NotRunning(b, "signal"),
        Refused::AlreadyAcquired(b) => Refusal::Exists(b),
        Refused::NoFreeUser => Refusal::Os {
            op: "acquire a box user from the helper",
            why: "the pool has no free user".to_owned(),
        },
        Refused::StartFailed(why) => Refusal::Os {
            op: "start a box member through the helper",
            why,
        },
        Refused::NotPermitted(why) => Refusal::Invalid {
            what: "helper request",
            why,
        },
        Refused::Malformed(why) => {
            Refusal::Broken(format!("the helper rejected a message as malformed: {why}"))
        }
    }
}

fn unexpected(call: &str, answer: &Answer) -> Refusal {
    let kind = match answer {
        Answer::Hello { .. } => "a hello",
        Answer::Acquired { .. } => "an acquired user",
        Answer::Started { .. } => "a started program",
        Answer::Ended => "an ended box",
        Answer::State(_) => "a box's state",
        Answer::Signalled => "a signalled box",
        Answer::Refused(_) => "a refusal",
    };
    Refusal::Broken(format!("the helper answered a {call} with {kind}"))
}

/// Sends `msg` framed with its `u32` length, `fds` riding with it by `SCM_RIGHTS` on the first write.
/// Loops until every byte is sent (a `SOCK_STREAM` write may be partial), carrying the descriptors only
/// on the first send, as `SCM_RIGHTS` attaches them to the bytes of one `sendmsg`.
pub(crate) fn send_framed(
    fd: BorrowedFd<'_>,
    msg: &[u8],
    fds: &[BorrowedFd<'_>],
) -> Result<(), Refusal> {
    let len = u32::try_from(msg.len()).map_err(|_| Refusal::Invalid {
        what: "helper request",
        why: format!("{} bytes is past a u32 length", msg.len()),
    })?;
    let mut framed = Vec::with_capacity(LEN_BYTES.saturating_add(msg.len()));
    framed.extend_from_slice(&len.to_le_bytes());
    framed.extend_from_slice(msg);
    let mut sent = 0usize;
    let mut first = true;
    while sent < framed.len() {
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(START_FDS))];
        let mut control = SendAncillaryBuffer::new(&mut space);
        if first && !fds.is_empty() && !control.push(SendAncillaryMessage::ScmRights(fds)) {
            return Err(Refusal::Invalid {
                what: "helper request",
                why: format!("{} descriptors is past what one message carries", fds.len()),
            });
        }
        let chunk = framed.get(sent..).unwrap_or_default();
        // macOS has no MSG_NOSIGNAL; the socket carries SO_NOSIGPIPE instead ([`Helper::connect`]).
        match rustix::net::sendmsg(fd, &[IoSlice::new(chunk)], &mut control, SendFlags::empty()) {
            Ok(n) if n > 0 => {
                sent = sent.saturating_add(n);
                first = false;
            }
            Ok(_) => return Err(Refusal::Broken("the helper took none of a send".to_owned())),
            Err(Errno::INTR) => {}
            Err(e) => return Err(Refusal::os("sending to the helper", e)),
        }
    }
    Ok(())
}

/// Reads one framed message: its `u32` length, then that many bytes, returning the bytes and any
/// descriptors that rode with them. Refuses a length past [`MAX_MESSAGE`] before reading the body.
pub(crate) fn recv_framed(fd: BorrowedFd<'_>) -> Result<(Vec<u8>, Vec<OwnedFd>), Refusal> {
    let mut len_buf = [0u8; LEN_BYTES];
    let mut fds = recv_exact(fd, &mut len_buf)?;
    let len = usize::try_from(u32::from_le_bytes(len_buf)).unwrap_or(usize::MAX);
    if len > MAX_MESSAGE {
        return Err(Refusal::Broken(format!(
            "the helper framed a {len}-byte message, past the {MAX_MESSAGE}-byte bound"
        )));
    }
    let mut body = vec![0u8; len];
    fds.extend(recv_exact(fd, &mut body)?);
    Ok((body, fds))
}

/// Fills `buf` from the socket, returning any descriptors that arrived with those bytes. A read of
/// nothing before `buf` is full is the helper gone.
fn recv_exact(fd: BorrowedFd<'_>, buf: &mut [u8]) -> Result<Vec<OwnedFd>, Refusal> {
    let mut got = 0usize;
    let mut fds: Vec<OwnedFd> = Vec::new();
    while got < buf.len() {
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(START_FDS))];
        let mut control = RecvAncillaryBuffer::new(&mut space);
        let rest = buf.get_mut(got..).unwrap_or_default();
        // macOS has no MSG_CMSG_CLOEXEC; a received descriptor is set close-on-exec by the daemon that
        // consumes it (the node side here receives none, on answers).
        let msg = match rustix::net::recvmsg(
            fd,
            &mut [IoSliceMut::new(rest)],
            &mut control,
            RecvFlags::empty(),
        ) {
            Ok(msg) => msg,
            Err(Errno::INTR) => continue,
            Err(e) => return Err(Refusal::os("reading from the helper", e)),
        };
        for m in control.drain() {
            if let RecvAncillaryMessage::ScmRights(rights) = m {
                fds.extend(rights);
            }
        }
        if msg.bytes == 0 {
            return Err(Refusal::Broken(
                "the helper closed before a whole message".to_owned(),
            ));
        }
        got = got.saturating_add(msg.bytes);
    }
    Ok(fds)
}

#[cfg(test)]
mod tests;
