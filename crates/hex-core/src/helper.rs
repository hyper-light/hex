//! The macOS box helper's protocol (native plan step 2). On macOS a box runs as a user of its own, from
//! a pool an installer makes; starting a program as that user, arming a running member's physical-footprint
//! ledger limit, and routing its Mach exceptions are all privileged, so a privileged launchd daemon — the
//! helper — does them, and only them, for the node agent, which runs unprivileged. The node asks over this
//! typed, bounded protocol on a Unix `SOCK_STREAM` socket; the helper does one of a fixed set of things and
//! **refuses everything else by name** (CLAUDE.md §3).
//!
//! Pure (CLAUDE.md §9): the shell (`hex_runtime::macos`) moves the messages and the stdio descriptors a
//! `Start` rides with (`SCM_RIGHTS`); this says what each message is, and refuses by type every one that
//! says nothing valid — a hostile-input test drives the parser (CLAUDE.md §6).
//!
//! A message is a 16-byte header (`HEXM`, a `u16` version, a `u16` kind, a `u64` id the answer carries
//! back), then the body its kind defines. Integers are little-endian; a string is a `u32` length and its
//! bytes; an enum a byte naming its variant. The format changes append-only, as [`crate::wire`]'s does.
//! Requests sit below `0x8000`, answers from it. macOS local sockets have no `SOCK_SEQPACKET`
//! (`hex_client` refuses the node's own socket on macOS for the same reason), so the shell frames each
//! message on the stream with a `u32` length before it ([`read_request`]/[`read_answer`] take one whole
//! message's bytes); a descriptor rides out of band with its message (`SCM_RIGHTS`), never in the bytes.

use hex_api::{BoxId, Program};

use crate::wire::{self, Fault, Reader, Writer};

/// Every helper message's first bytes: "HEX macOS helper".
pub const MAGIC: [u8; 4] = *b"HEXM";
/// The version this side speaks; it reads every one from [`FIRST`].
pub const VERSION: u16 = 1;
/// The first version, which every side reads: a hello and a refusal of an unreadable message go in it.
pub const FIRST: u16 = 1;
/// The header: magic, version, kind, id.
pub const HEADER_BYTES: usize = 16;
/// Largest helper message. A request is a box id (≤128 bytes) and at most a program (its argv, environment,
/// a working folder); 64 KiB holds any a caller passes in practice, and a larger one is refused by type
/// before it is sent. Under the kernel's `SOCK_SEQPACKET` record bound, as [`crate::wire::MAX_MESSAGE`] is.
pub const MAX_MESSAGE: usize = 64 << 10;
/// The descriptors a [`Request::Start`] carries: the program's stdin, stdout and stderr.
pub const START_FDS: usize = 3;

mod kind {
    pub const HELLO: u16 = 0x0001;
    pub const ACQUIRE: u16 = 0x0002;
    pub const START: u16 = 0x0003;
    pub const END: u16 = 0x0004;
    pub const INSPECT: u16 = 0x0005;
    pub const SIGNAL: u16 = 0x0006;

    pub const ANSWER: u16 = 0x8000;
    pub const A_HELLO: u16 = ANSWER | HELLO;
    pub const A_ACQUIRE: u16 = ANSWER | ACQUIRE;
    pub const A_START: u16 = ANSWER | START;
    pub const A_END: u16 = ANSWER | END;
    pub const A_INSPECT: u16 = ANSWER | INSPECT;
    pub const A_SIGNAL: u16 = ANSWER | SIGNAL;
    pub const A_REFUSED: u16 = ANSWER | 0x00ff;
}

/// What the node asks the helper to do. The helper does exactly these; a message of any other kind is
/// refused at [`read_request`] as a kind this version doesn't define.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Settle the version both sides speak, before anything else.
    Hello { version: u16 },
    /// Take a free pool user for a box with this memory bound and reserve (both in bytes); the helper binds
    /// the user to the box and arms its exception handling. Refused when the box already has one, or the
    /// pool is empty.
    Acquire {
        box_id: BoxId,
        bound: u64,
        reserve: u64,
    },
    /// Start `program` as the box's pool user, armed at its apportioned ledger limit, its three stdio
    /// descriptors riding with the message. `leader` is the box's first program (a new session); a later
    /// one (the node's `exec`) joins the user's processes. Refused when the box has no user.
    Start {
        box_id: BoxId,
        program: Program,
        leader: bool,
    },
    /// End the box: the helper kills every process of its user and frees the user back to the pool only once
    /// none is left.
    End { box_id: BoxId },
    /// What the box holds now, by its user's processes.
    Inspect { box_id: BoxId },
    /// Send a signal to every member of the box (the node can't signal another user's processes itself).
    /// `signal` is the Linux signal number, as the rest of Hex carries it; the helper maps it to macOS's
    /// (`hex_core::darwin`), as the process class does. Refused as not running when no member is live.
    Signal { box_id: BoxId, signal: i32 },
}

/// What the box holds now, as the helper reports it (`proc_listpids(PROC_UID_ONLY)` of the box's user, each
/// member's footprint summed).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct State {
    /// Its members running now.
    pub members: u32,
    /// Its members' footprint summed, in bytes.
    pub footprint: u64,
    /// The highest footprint the helper has read.
    pub peak: u64,
    /// Its processes, as its user's `RLIMIT_NPROC` counts them.
    pub processes: u32,
    /// Times a member crossed its ledger share.
    pub crossings: u64,
    /// Whether the box has been ended for its memory bound.
    pub ended_over: bool,
}

/// What the helper answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Hello { version: u16 },
    Acquired { uid: u32 },
    Started { pid: i32 },
    Ended,
    State(State),
    Signalled,
    Refused(Refused),
}

/// Why the helper refused a request — each by name (CLAUDE.md §3), so the node learns what and why.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    #[error("the box {0} has no pool user; acquire one first")]
    UnknownBox(BoxId),
    #[error("the box {0} already holds a pool user")]
    AlreadyAcquired(BoxId),
    #[error("the pool has no free user")]
    NoFreeUser,
    #[error("starting the program failed: {0}")]
    StartFailed(String),
    #[error("the helper does not do that: {0}")]
    NotPermitted(String),
    #[error("the request was malformed: {0}")]
    Malformed(String),
    #[error("the box {0} has no live member")]
    NotRunning(BoxId),
}

impl Request {
    fn kind(&self) -> u16 {
        match self {
            Request::Hello { .. } => kind::HELLO,
            Request::Acquire { .. } => kind::ACQUIRE,
            Request::Start { .. } => kind::START,
            Request::End { .. } => kind::END,
            Request::Inspect { .. } => kind::INSPECT,
            Request::Signal { .. } => kind::SIGNAL,
        }
    }

    /// The descriptors this request carries with it: three for a [`Request::Start`], none else.
    pub fn descriptors(&self) -> usize {
        match self {
            Request::Start { .. } => START_FDS,
            _ => 0,
        }
    }
}

/// Writes a helper message's header: [`MAGIC`] (not [`crate::wire::MAGIC`]), the version, the kind, the id.
fn put_header(w: &mut Writer, version: u16, kind: u16, id: u64) {
    w.raw(&MAGIC);
    w.u16(version);
    w.u16(kind);
    w.u64(id);
}

/// A refused request as a [`Refused`] reason carries it.
fn refused_tag(r: &Refused) -> u8 {
    match r {
        Refused::UnknownBox(_) => 0,
        Refused::AlreadyAcquired(_) => 1,
        Refused::NoFreeUser => 2,
        Refused::StartFailed(_) => 3,
        Refused::NotPermitted(_) => 4,
        Refused::Malformed(_) => 5,
        Refused::NotRunning(_) => 6,
    }
}

/// Encodes `request` in `version` with `id`, as a message of at most [`MAX_MESSAGE`] bytes. The stdio
/// descriptors of a [`Request::Start`] ride with the message itself, not in its bytes.
pub fn encode_request(version: u16, id: u64, request: &Request) -> Result<Vec<u8>, Fault> {
    let mut w = Writer::new(MAX_MESSAGE);
    put_header(&mut w, version, request.kind(), id);
    match request {
        Request::Hello { version } => w.u16(*version),
        Request::Acquire {
            box_id,
            bound,
            reserve,
        } => {
            w.string(box_id.as_str())?;
            w.u64(*bound);
            w.u64(*reserve);
        }
        Request::Start {
            box_id,
            program,
            leader,
        } => {
            w.string(box_id.as_str())?;
            wire::put_program(&mut w, program)?;
            w.bool(*leader);
        }
        Request::End { box_id } | Request::Inspect { box_id } => w.string(box_id.as_str())?,
        Request::Signal { box_id, signal } => {
            w.string(box_id.as_str())?;
            w.i32(*signal);
        }
    }
    w.done()
}

/// Encodes `answer` in `version` with the request's `id`.
pub fn encode_answer(version: u16, id: u64, answer: &Answer) -> Result<Vec<u8>, Fault> {
    let mut w = Writer::new(MAX_MESSAGE);
    let kind = match answer {
        Answer::Hello { .. } => kind::A_HELLO,
        Answer::Acquired { .. } => kind::A_ACQUIRE,
        Answer::Started { .. } => kind::A_START,
        Answer::Ended => kind::A_END,
        Answer::State(_) => kind::A_INSPECT,
        Answer::Signalled => kind::A_SIGNAL,
        Answer::Refused(_) => kind::A_REFUSED,
    };
    put_header(&mut w, version, kind, id);
    match answer {
        Answer::Hello { version } => w.u16(*version),
        Answer::Acquired { uid } => w.u32(*uid),
        Answer::Started { pid } => w.i32(*pid),
        Answer::Ended | Answer::Signalled => {}
        Answer::State(s) => {
            w.u32(s.members);
            w.u64(s.footprint);
            w.u64(s.peak);
            w.u32(s.processes);
            w.u64(s.crossings);
            w.bool(s.ended_over);
        }
        Answer::Refused(r) => {
            w.u8(refused_tag(r));
            match r {
                Refused::UnknownBox(b) | Refused::AlreadyAcquired(b) | Refused::NotRunning(b) => {
                    w.string(b.as_str())?
                }
                Refused::StartFailed(m) | Refused::NotPermitted(m) | Refused::Malformed(m) => {
                    w.string(m)?
                }
                Refused::NoFreeUser => {}
            }
        }
    }
    w.done()
}

/// Reads the header of a `version`-this-side message: `(version, kind, id)`, or the fault that it isn't a
/// helper message this side can read.
fn header(bytes: &[u8]) -> Result<(Reader<'_>, u16, u16, u64), Fault> {
    let mut r = Reader::new(bytes);
    let (version, kind, id) = read_header(&mut r)?;
    if version == 0 || version > VERSION {
        return Err(Fault::Version {
            got: version,
            speaks: VERSION,
        });
    }
    Ok((r, version, kind, id))
}

/// The header with [`MAGIC`] checked (the shared [`Reader::header`] checks [`crate::wire::MAGIC`]).
fn read_header(r: &mut Reader<'_>) -> Result<(u16, u16, u64), Fault> {
    if r.array::<4>()? != MAGIC {
        return Err(Fault::Magic);
    }
    Ok((r.u16()?, r.u16()?, r.u64()?))
}

/// Reads a request from one message's `bytes`, carrying `descriptors` descriptors. Refuses, by a typed
/// [`Fault`], a message that isn't a helper request this version defines, that a `Start` without exactly
/// its three descriptors, and one with bytes past what its kind defines (CLAUDE.md §3: nothing is skipped).
pub fn read_request(bytes: &[u8], descriptors: usize) -> Result<(u16, u64, Request), Fault> {
    if bytes.len() > MAX_MESSAGE {
        return Err(Fault::Oversized {
            bytes: bytes.len(),
            bound: MAX_MESSAGE,
        });
    }
    let (mut r, version, kind, id) = header(bytes)?;
    let request = match kind {
        kind::HELLO => Request::Hello { version: r.u16()? },
        kind::ACQUIRE => Request::Acquire {
            box_id: wire::box_id(&mut r)?,
            bound: r.u64()?,
            reserve: r.u64()?,
        },
        kind::START => Request::Start {
            box_id: wire::box_id(&mut r)?,
            program: wire::program(&mut r)?,
            leader: r.bool("leader")?,
        },
        kind::END => Request::End {
            box_id: wire::box_id(&mut r)?,
        },
        kind::INSPECT => Request::Inspect {
            box_id: wire::box_id(&mut r)?,
        },
        kind::SIGNAL => Request::Signal {
            box_id: wire::box_id(&mut r)?,
            signal: r.i32()?,
        },
        other => {
            return Err(Fault::Kind {
                kind: other,
                version,
                expected: "a helper request",
            });
        }
    };
    r.end(kind, version)?;
    let want = request.descriptors();
    if want != descriptors {
        return Err(Fault::Descriptors {
            what: "a start",
            want,
            got: descriptors,
        });
    }
    Ok((version, id, request))
}

/// Reads an answer from one message's `bytes`.
pub fn read_answer(bytes: &[u8]) -> Result<(u16, u64, Answer), Fault> {
    if bytes.len() > MAX_MESSAGE {
        return Err(Fault::Oversized {
            bytes: bytes.len(),
            bound: MAX_MESSAGE,
        });
    }
    let (mut r, version, kind, id) = header(bytes)?;
    let answer = match kind {
        kind::A_HELLO => Answer::Hello { version: r.u16()? },
        kind::A_ACQUIRE => Answer::Acquired { uid: r.u32()? },
        kind::A_START => Answer::Started { pid: r.i32()? },
        kind::A_END => Answer::Ended,
        kind::A_INSPECT => Answer::State(State {
            members: r.u32()?,
            footprint: r.u64()?,
            peak: r.u64()?,
            processes: r.u32()?,
            crossings: r.u64()?,
            ended_over: r.bool("ended_over")?,
        }),
        kind::A_SIGNAL => Answer::Signalled,
        kind::A_REFUSED => Answer::Refused(read_refused(&mut r)?),
        other => {
            return Err(Fault::Kind {
                kind: other,
                version,
                expected: "a helper answer",
            });
        }
    };
    r.end(kind, version)?;
    Ok((version, id, answer))
}

fn read_refused(r: &mut Reader<'_>) -> Result<Refused, Fault> {
    Ok(match r.u8()? {
        0 => Refused::UnknownBox(wire::box_id(r)?),
        1 => Refused::AlreadyAcquired(wire::box_id(r)?),
        2 => Refused::NoFreeUser,
        3 => Refused::StartFailed(r.string("reason")?),
        4 => Refused::NotPermitted(r.string("reason")?),
        5 => Refused::Malformed(r.string("reason")?),
        6 => Refused::NotRunning(wire::box_id(r)?),
        t => return Err(Fault::value("refusal", format!("variant {t}"))),
    })
}

#[cfg(test)]
mod tests;
