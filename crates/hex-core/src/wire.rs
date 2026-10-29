//! The node agent's local socket as bytes (decision 3 of docs/design/runtime-architecture.md): the
//! `Runtime` seam's requests and answers as messages of a Unix `SOCK_SEQPACKET` socket, and back.
//! Pure: the shells, `hex-node`'s server and `hex-client`, move the messages and the descriptors that
//! ride with them (exec's stdio, by `SCM_RIGHTS`); this decides what each message says, and refuses by
//! type every one that says nothing valid.
//!
//! A message is one `SOCK_SEQPACKET` record, so the kernel keeps its bounds (unix(7)): a 16-byte
//! header, then the body its kind defines.
//!
//! | bytes | field |
//! |---|---|
//! | 0..4 | `HEXW` |
//! | 4..6 | version |
//! | 6..8 | kind: a request below `0x8000`, an answer from it |
//! | 8..16 | id: a request's, chosen by its client; its answer, and an exec'd program's end, carry it |
//!
//! Integers are little-endian. A string or a path is a `u32` length and its bytes, an option a byte
//! (0 none, 1 some) before the value, a list a `u32` count before its items, and an enum a byte naming
//! its variant before its fields.
//!
//! The format changes append-only (CLAUDE.md): version 1's kinds and their layouts never change. A
//! later version adds kinds and appends fields to a kind's body, and reads every earlier version's
//! messages as they were; an answer is written in its request's version. A reader refuses a message
//! with bytes past what its version defines rather than skip them, so a field an older reader can't
//! act on is never dropped silently (§3), and a kind newer than a message's version is refused as one
//! that version doesn't define. A client says hello in [`FIRST`], which every node agent reads, learns
//! from the answer the highest version the agent speaks, and writes the rest in the highest both speak.
//!
//! Version 2 (goal 4 plan step 2) adds one request and its answer: the node agent's network namespace
//! counts ([`Call::Netns`], [`Answer::Netns`]). Every version 1 kind reads the same in it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use hex_api::{
    Bounds, BoxId, BoxInfo, BoxSpec, BoxState, Class, ClassSpec, Exit, IoRates, Mount, Network,
    OciSpec, Program, Refusal, Rootfs, Signal, Usage, User,
};

use crate::netns;

/// Every message's first bytes.
pub const MAGIC: [u8; 4] = *b"HEXW";
/// The highest version this side speaks; it reads and writes every one from [`FIRST`].
pub const VERSION: u16 = 2;
/// The first version, which every side reads: a client's hello goes in it, and so does a refusal of a
/// message whose version this side can't tell or doesn't speak.
pub const FIRST: u16 = 1;
/// The header's length: magic, version, kind and id.
pub const HEADER_BYTES: usize = 16;
/// Largest message either side sends or reads. It sits under the kernel's own bound on a
/// `SOCK_SEQPACKET` record, the sender's send buffer less 32 bytes (net/unix/af_unix.c,
/// `unix_dgram_sendmsg`), which is 212,960 bytes under the default 212,992-byte buffer
/// (`net.core.wmem_default`): measured on adacachy, Linux 7.2.9, 2026-10-27. 128 KiB holds any box spec
/// or program a caller passes in practice (an id is at most 128 bytes, a path 4 KiB); a larger one is
/// refused by type before it is sent. The in-process seam has no such bound: a program whose arguments
/// and environment pass it runs only there.
pub const MAX_MESSAGE: usize = 128 << 10;
/// Most descriptors one message carries: exec's stdin, stdout and stderr.
pub const MAX_FDS: usize = 3;

/// The kinds, each from the version that added it. Requests sit below `0x8000`, answers from it.
mod kind {
    // Version 1's.
    pub const HELLO: u16 = 0x0001;
    pub const CREATE: u16 = 0x0002;
    pub const START: u16 = 0x0003;
    pub const EXEC: u16 = 0x0004;
    pub const SIGNAL: u16 = 0x0005;
    pub const INSPECT: u16 = 0x0006;
    pub const USAGE: u16 = 0x0007;
    pub const LIST: u16 = 0x0008;
    pub const DELETE: u16 = 0x0009;
    pub const RELEASE: u16 = 0x000a;
    pub const HELLO_ANSWER: u16 = 0x8001;
    pub const INFO: u16 = 0x8002;
    pub const DONE: u16 = 0x8003;
    pub const STARTED: u16 = 0x8004;
    pub const USAGE_ANSWER: u16 = 0x8005;
    pub const BOXES: u16 = 0x8006;
    pub const REFUSED: u16 = 0x8007;
    pub const EXITED: u16 = 0x8008;
    // Version 2's.
    pub const NETNS: u16 = 0x000b;
    pub const NETNS_ANSWER: u16 = 0x8009;

    /// The version that added `kind`: a message of an earlier version is never of it. A kind no
    /// version defines is refused by the reader's match, whatever this says.
    pub fn since(kind: u16) -> u16 {
        match kind {
            NETNS | NETNS_ANSWER => 2,
            _ => 1,
        }
    }
}

/// A message's header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub version: u16,
    pub kind: u16,
    pub id: u64,
}

/// What a client sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// A connection's first message: answered with what the node agent runs and its bounds.
    Hello,
    /// A call of the seam, answered once.
    Call(Call),
    /// The client no longer waits for the end of the program its header's id started (its `Proc` was
    /// dropped): the node agent stops watching it and lets it run on, as dropping an in-process `Proc`
    /// does. Not answered.
    Release,
}

/// A call of the node agent: the `Runtime` seam's, and from version 2 its network namespace counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Create(BoxSpec),
    Start(BoxId),
    /// `program` in box `id`, with the streams `streams` names as the message's descriptors, in order.
    Exec {
        id: BoxId,
        program: Program,
        streams: Streams,
    },
    Signal {
        id: BoxId,
        signal: Signal,
    },
    Inspect(BoxId),
    Usage(BoxId),
    List,
    Delete(BoxId),
    /// The node agent's network namespace counts ([`netns::Counts`]), version 2's.
    Netns,
}

/// Which of a program's standard streams travel with its exec, in this order; the rest are the null
/// device.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Streams {
    pub stdin: bool,
    pub stdout: bool,
    pub stderr: bool,
}

impl Streams {
    /// How many descriptors travel with the exec.
    pub fn count(self) -> usize {
        [self.stdin, self.stdout, self.stderr]
            .into_iter()
            .filter(|s| *s)
            .count()
    }

    fn bits(self) -> u8 {
        [(self.stdin, 0b1), (self.stdout, 0b10), (self.stderr, 0b100)]
            .into_iter()
            .filter(|(on, _)| *on)
            .fold(0, |bits, (_, bit)| bits | bit)
    }

    fn of(bits: u8) -> Result<Self, Fault> {
        if bits & !0b111 != 0 {
            return Err(Fault::value(
                "streams",
                format!("{bits:#04x} names more than stdin, stdout and stderr"),
            ));
        }
        Ok(Self {
            stdin: bits & 1 != 0,
            stdout: bits & 0b10 != 0,
            stderr: bits & 0b100 != 0,
        })
    }
}

/// What the node agent sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Hello(Hello),
    /// create's and inspect's.
    Info(BoxInfo),
    /// start's, signal's and delete's.
    Done,
    /// exec's: the program runs, with this process id; its end comes later as [`Answer::Exited`].
    Started {
        pid: u32,
    },
    Usage(Usage),
    /// list's: its boxes, in as many messages as they need ([`boxes`]), all with the list's id, each
    /// but the last with `more`.
    Boxes {
        infos: Vec<BoxInfo>,
        more: bool,
    },
    Refused(Refusal),
    /// The program the header's id started has ended. Sent unasked, after that exec's answer.
    Exited(Exit),
    /// netns's, version 2's.
    Netns(netns::Counts),
}

/// The answer to a hello.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    /// The highest version the node agent speaks; it speaks every one from 1.
    pub version: u16,
    pub classes: Vec<Class>,
    pub limits: Limits,
}

/// The node agent's bounds on its socket, each refused past by type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Connections open at once.
    pub connections: usize,
    /// Bytes in one message, at most [`MAX_MESSAGE`].
    pub message_bytes: usize,
    /// Requests one connection has asked and not yet had answered.
    pub in_flight: usize,
    /// Programs one connection's execs started that it still watches.
    pub programs: usize,
}

/// Why bytes aren't a message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Fault {
    #[error("it ends at byte {have}, short of the {need} it needs")]
    Truncated { need: usize, have: usize },
    #[error("{bytes} bytes is over the {bound}-byte bound on one message")]
    Oversized { bytes: usize, bound: usize },
    #[error("it doesn't start with HEXW")]
    Magic,
    #[error("version {got}: this side speaks 1 to {speaks}")]
    Version { got: u16, speaks: u16 },
    #[error("kind {kind:#06x} isn't {expected} version {version} defines")]
    Kind {
        kind: u16,
        version: u16,
        expected: &'static str,
    },
    #[error("{bytes} bytes follow what version {version} defines for kind {kind:#06x}")]
    Trailing {
        bytes: usize,
        kind: u16,
        version: u16,
    },
    #[error("{what} isn't valid: {why}")]
    Value { what: &'static str, why: String },
    #[error("it carries {got} descriptors where {what} takes {want}")]
    Descriptors {
        what: &'static str,
        want: usize,
        got: usize,
    },
    #[error("it carries more descriptors than the {MAX_FDS} a message takes")]
    DescriptorsCut,
}

impl Fault {
    pub(crate) fn value(what: &'static str, why: impl Into<String>) -> Self {
        Self::Value {
            what,
            why: why.into(),
        }
    }
}

impl From<Fault> for Refusal {
    fn from(fault: Fault) -> Self {
        Refusal::Invalid {
            what: "message",
            why: fault.to_string(),
        }
    }
}

/// A message as a socket received it: the bytes its buffer held, its whole size (the kernel's, which
/// is larger when the message was cut to the buffer), how many descriptors came with it, and whether
/// more came than the receiver had room for (`MSG_CTRUNC`; the kernel closed the rest, unix(7)).
#[derive(Debug, Clone, Copy)]
pub struct Received<'a> {
    pub bytes: &'a [u8],
    pub size: usize,
    pub descriptors: usize,
    pub descriptors_cut: bool,
}

/// A request refused: the fault, and where to send the refusal (the request's version and id; version
/// [`FIRST`] when the request's isn't one this side speaks, and id 0 when its header couldn't be read).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bad {
    pub version: u16,
    pub id: u64,
    pub fault: Fault,
}

/// A request as a message of at most `bound` bytes, in `version`: the highest its node agent speaks,
/// as far as this side does, or [`FIRST`] for a hello. A request of a kind `version` doesn't define is
/// refused before it is written.
pub fn request(version: u16, id: u64, request: &Request, bound: usize) -> Result<Vec<u8>, Fault> {
    speaks(version)?;
    let mut w = Writer::new(bound);
    match request {
        Request::Hello => w.header(version, kind::HELLO, id),
        Request::Release => w.header(version, kind::RELEASE, id),
        Request::Call(call) => put_call(&mut w, version, id, call)?,
    }
    w.done()
}

/// An answer as a message of at most `bound` bytes, in `version` (its request's). An answer of a kind
/// `version` doesn't define is refused before it is written.
pub fn answer(version: u16, id: u64, answer: &Answer, bound: usize) -> Result<Vec<u8>, Fault> {
    speaks(version)?;
    let mut w = Writer::new(bound);
    match answer {
        Answer::Hello(h) => {
            w.header(version, kind::HELLO_ANSWER, id);
            w.u16(h.version);
            w.list(&h.classes, |w, c| {
                w.u8(class_tag(*c));
                Ok(())
            })?;
            for n in [
                h.limits.connections,
                h.limits.message_bytes,
                h.limits.in_flight,
                h.limits.programs,
            ] {
                w.usize(n)?;
            }
        }
        Answer::Info(info) => {
            w.header(version, kind::INFO, id);
            put_info(&mut w, info)?;
        }
        Answer::Done => w.header(version, kind::DONE, id),
        Answer::Started { pid } => {
            w.header(version, kind::STARTED, id);
            w.u32(*pid);
        }
        Answer::Usage(u) => {
            w.header(version, kind::USAGE_ANSWER, id);
            for n in [
                u.memory_bytes,
                u.memory_peak_bytes,
                u.oom_kills,
                u.pids,
                u.pids_refused,
                u.cpu_usec,
                u.io_read_bytes,
                u.io_write_bytes,
            ] {
                w.u64(n);
            }
        }
        Answer::Boxes { infos, more } => {
            w.header(version, kind::BOXES, id);
            w.bool(*more);
            w.list(infos, put_info)?;
        }
        Answer::Refused(r) => {
            w.header(version, kind::REFUSED, id);
            put_refusal(&mut w, r)?;
        }
        Answer::Exited(exit) => {
            w.header(version, kind::EXITED, id);
            put_exit(&mut w, *exit);
        }
        Answer::Netns(counts) => {
            defines(version, kind::NETNS_ANSWER, "an answer")?;
            w.header(version, kind::NETNS_ANSWER, id);
            for n in [
                counts.made_ahead,
                counts.taken,
                counts.made_on_path,
                counts.discarded,
                counts.failed,
            ] {
                w.u64(n);
            }
        }
    }
    w.done()
}

/// A list's answer as messages of at most `bound` bytes each: as many boxes in each as fit, every
/// message but the last marked `more`, so a node holding more boxes than one message carries still
/// answers a list whole. An empty list is one message.
pub fn boxes(
    version: u16,
    id: u64,
    infos: &[BoxInfo],
    bound: usize,
) -> Result<Vec<Vec<u8>>, Fault> {
    // A message's bytes before its boxes: the header, `more` and the list's count.
    const OVERHEAD: usize = HEADER_BYTES + 1 + 4;
    let room = bound.checked_sub(OVERHEAD).ok_or(Fault::Oversized {
        bytes: OVERHEAD,
        bound,
    })?;
    let mut parts: Vec<Vec<BoxInfo>> = vec![Vec::new()];
    let mut used = 0usize;
    for info in infos {
        let mut w = Writer::new(room);
        put_info(&mut w, info)?;
        let size = w.done()?.len();
        let fits = used.checked_add(size).filter(|u| *u <= room);
        match (fits, parts.last_mut()) {
            (Some(u), Some(part)) => {
                used = u;
                part.push(info.clone());
            }
            _ => {
                used = size;
                parts.push(vec![info.clone()]);
            }
        }
    }
    let last = parts.len().saturating_sub(1);
    parts
        .into_iter()
        .enumerate()
        .map(|(n, infos)| {
            answer(
                version,
                id,
                &Answer::Boxes {
                    infos,
                    more: n < last,
                },
                bound,
            )
        })
        .collect()
}

/// The request a received message holds, at most `bound` bytes, or why it holds none.
pub fn read_request(msg: &Received<'_>, bound: usize) -> Result<(Header, Request), Bad> {
    let mut r = Reader::new(msg.bytes);
    let (version, kind, id) = r.header().map_err(|fault| Bad {
        version: FIRST,
        id: 0,
        fault,
    })?;
    let bad = |fault| Bad {
        version: if speaks(version).is_ok() {
            version
        } else {
            FIRST
        },
        id,
        fault,
    };
    check(msg, bound, version).map_err(bad)?;
    defines(version, kind, "a request").map_err(bad)?;
    let request = match kind {
        kind::HELLO => Request::Hello,
        kind::RELEASE => Request::Release,
        kind::CREATE => Request::Call(Call::Create(box_spec(&mut r).map_err(bad)?)),
        kind::START => Request::Call(Call::Start(box_id(&mut r).map_err(bad)?)),
        kind::EXEC => {
            let id = box_id(&mut r).map_err(bad)?;
            let program = program(&mut r).map_err(bad)?;
            let streams = r.u8().and_then(Streams::of).map_err(bad)?;
            Request::Call(Call::Exec {
                id,
                program,
                streams,
            })
        }
        kind::SIGNAL => {
            let id = box_id(&mut r).map_err(bad)?;
            let signal = r.u8().map_err(bad)?;
            let signal = Signal::new(i64::from(signal))
                .map_err(|e| bad(Fault::value("signal", e.to_string())))?;
            Request::Call(Call::Signal { id, signal })
        }
        kind::INSPECT => Request::Call(Call::Inspect(box_id(&mut r).map_err(bad)?)),
        kind::USAGE => Request::Call(Call::Usage(box_id(&mut r).map_err(bad)?)),
        kind::LIST => Request::Call(Call::List),
        kind::DELETE => Request::Call(Call::Delete(box_id(&mut r).map_err(bad)?)),
        kind::NETNS => Request::Call(Call::Netns),
        _ => {
            return Err(bad(Fault::Kind {
                kind,
                version,
                expected: "a request",
            }));
        }
    };
    let want = match &request {
        Request::Call(Call::Exec { streams, .. }) => streams.count(),
        _ => 0,
    };
    if msg.descriptors != want {
        return Err(bad(Fault::Descriptors {
            what: request_name(kind),
            want,
            got: msg.descriptors,
        }));
    }
    r.end(kind, version).map_err(bad)?;
    Ok((Header { version, kind, id }, request))
}

/// The answer a received message holds, at most `bound` bytes, or why it holds none. An answer
/// carries no descriptors.
pub fn read_answer(msg: &Received<'_>, bound: usize) -> Result<(Header, Answer), Fault> {
    let mut r = Reader::new(msg.bytes);
    let (version, kind, id) = r.header()?;
    check(msg, bound, version)?;
    defines(version, kind, "an answer")?;
    if msg.descriptors != 0 {
        return Err(Fault::Descriptors {
            what: "an answer",
            want: 0,
            got: msg.descriptors,
        });
    }
    let answer = match kind {
        kind::HELLO_ANSWER => {
            let version = r.u16()?;
            let classes = r.list(class)?;
            let mut limit = || r.usize();
            Answer::Hello(Hello {
                version,
                classes,
                limits: Limits {
                    connections: limit()?,
                    message_bytes: limit()?,
                    in_flight: limit()?,
                    programs: limit()?,
                },
            })
        }
        kind::INFO => Answer::Info(info(&mut r)?),
        kind::DONE => Answer::Done,
        kind::STARTED => Answer::Started { pid: r.u32()? },
        kind::USAGE_ANSWER => Answer::Usage(Usage {
            memory_bytes: r.u64()?,
            memory_peak_bytes: r.u64()?,
            oom_kills: r.u64()?,
            pids: r.u64()?,
            pids_refused: r.u64()?,
            cpu_usec: r.u64()?,
            io_read_bytes: r.u64()?,
            io_write_bytes: r.u64()?,
        }),
        kind::BOXES => Answer::Boxes {
            more: r.bool("more")?,
            infos: r.list(info)?,
        },
        kind::REFUSED => Answer::Refused(refusal(&mut r)?),
        kind::EXITED => Answer::Exited(exit(&mut r)?),
        kind::NETNS_ANSWER => Answer::Netns(netns::Counts {
            made_ahead: r.u64()?,
            taken: r.u64()?,
            made_on_path: r.u64()?,
            discarded: r.u64()?,
            failed: r.u64()?,
        }),
        _ => {
            return Err(Fault::Kind {
                kind,
                version,
                expected: "an answer",
            });
        }
    };
    r.end(kind, version)?;
    Ok((Header { version, kind, id }, answer))
}

/// What every message is checked for once its header is read: its size, its descriptors' room and
/// its version.
fn check(msg: &Received<'_>, bound: usize, version: u16) -> Result<(), Fault> {
    if msg.size > bound {
        return Err(Fault::Oversized {
            bytes: msg.size,
            bound,
        });
    }
    if msg.descriptors_cut || msg.descriptors > MAX_FDS {
        return Err(Fault::DescriptorsCut);
    }
    speaks(version)
}

/// That this side speaks `version`.
fn speaks(version: u16) -> Result<(), Fault> {
    if (FIRST..=VERSION).contains(&version) {
        Ok(())
    } else {
        Err(Fault::Version {
            got: version,
            speaks: VERSION,
        })
    }
}

/// That `version` defines `kind` (`expected`, a request or an answer): a kind a later version added
/// is refused in an earlier one's message, as one that version doesn't define.
fn defines(version: u16, kind: u16, expected: &'static str) -> Result<(), Fault> {
    if version >= kind::since(kind) {
        Ok(())
    } else {
        Err(Fault::Kind {
            kind,
            version,
            expected,
        })
    }
}

fn request_name(kind: u16) -> &'static str {
    match kind {
        kind::HELLO => "a hello",
        kind::CREATE => "a create",
        kind::START => "a start",
        kind::EXEC => "an exec",
        kind::SIGNAL => "a signal",
        kind::INSPECT => "an inspect",
        kind::USAGE => "a usage",
        kind::LIST => "a list",
        kind::DELETE => "a delete",
        kind::RELEASE => "a release",
        kind::NETNS => "a netns",
        _ => "a request",
    }
}

fn put_call(w: &mut Writer, version: u16, id: u64, call: &Call) -> Result<(), Fault> {
    match call {
        Call::Create(spec) => {
            w.header(version, kind::CREATE, id);
            put_box_spec(w, spec)
        }
        Call::Start(b) => {
            w.header(version, kind::START, id);
            w.string(b.as_str())
        }
        Call::Exec {
            id: b,
            program,
            streams,
        } => {
            w.header(version, kind::EXEC, id);
            w.string(b.as_str())?;
            put_program(w, program)?;
            w.u8(streams.bits());
            Ok(())
        }
        Call::Signal { id: b, signal } => {
            w.header(version, kind::SIGNAL, id);
            w.string(b.as_str())?;
            let n = u8::try_from(signal.number())
                .map_err(|_| Fault::value("signal", format!("{} isn't a byte", signal.number())))?;
            w.u8(n);
            Ok(())
        }
        Call::Inspect(b) => {
            w.header(version, kind::INSPECT, id);
            w.string(b.as_str())
        }
        Call::Usage(b) => {
            w.header(version, kind::USAGE, id);
            w.string(b.as_str())
        }
        Call::List => {
            w.header(version, kind::LIST, id);
            Ok(())
        }
        Call::Delete(b) => {
            w.header(version, kind::DELETE, id);
            w.string(b.as_str())
        }
        Call::Netns => {
            defines(version, kind::NETNS, "a request")?;
            w.header(version, kind::NETNS, id);
            Ok(())
        }
    }
}

fn put_box_spec(w: &mut Writer, spec: &BoxSpec) -> Result<(), Fault> {
    w.string(spec.id.as_str())?;
    put_bounds(w, &spec.bounds);
    match &spec.class {
        ClassSpec::Process => w.u8(0),
        ClassSpec::Oci(oci) => {
            w.u8(1);
            put_oci(w, oci)?;
        }
        ClassSpec::Bundle(path) => {
            w.u8(2);
            w.path(path)?;
        }
    }
    Ok(())
}

fn box_spec(r: &mut Reader<'_>) -> Result<BoxSpec, Fault> {
    let id = box_id(r)?;
    let bounds = bounds(r)?;
    let class = match r.u8()? {
        0 => ClassSpec::Process,
        1 => ClassSpec::Oci(Box::new(oci(r)?)),
        2 => ClassSpec::Bundle(r.path()?),
        t => return Err(Fault::value("class spec", format!("variant {t}"))),
    };
    Ok(BoxSpec { id, bounds, class })
}

fn put_bounds(w: &mut Writer, b: &Bounds) {
    w.option(b.cpu_weight, Writer::u16);
    w.option(b.cpu_max_milli, Writer::u32);
    w.option(b.memory_max, Writer::u64);
    w.option(b.memory_high, Writer::u64);
    w.option(b.io_weight, Writer::u16);
    w.option(b.io_latency_us, Writer::u32);
    w.option(b.io_max, |w, rates: IoRates| {
        for rate in [
            rates.read_bps,
            rates.write_bps,
            rates.read_iops,
            rates.write_iops,
        ] {
            w.option(rate, Writer::u64);
        }
    });
    w.option(b.pids_max, Writer::u32);
}

fn bounds(r: &mut Reader<'_>) -> Result<Bounds, Fault> {
    Ok(Bounds {
        cpu_weight: r.option(Reader::u16)?,
        cpu_max_milli: r.option(Reader::u32)?,
        memory_max: r.option(Reader::u64)?,
        memory_high: r.option(Reader::u64)?,
        io_weight: r.option(Reader::u16)?,
        io_latency_us: r.option(Reader::u32)?,
        io_max: r.option(|r| {
            Ok(IoRates {
                read_bps: r.option(Reader::u64)?,
                write_bps: r.option(Reader::u64)?,
                read_iops: r.option(Reader::u64)?,
                write_iops: r.option(Reader::u64)?,
            })
        })?,
        pids_max: r.option(Reader::u32)?,
    })
}

fn put_oci(w: &mut Writer, spec: &OciSpec) -> Result<(), Fault> {
    match &spec.rootfs {
        Rootfs::Dir(dir) => {
            w.u8(0);
            w.path(dir)?;
        }
        Rootfs::Layers(layers) => {
            w.u8(1);
            w.list(layers, |w, p| w.path(p))?;
        }
    }
    put_program(w, &spec.init)?;
    w.list(&spec.mounts, |w, m| {
        w.path(&m.source)?;
        w.string(&m.destination)?;
        w.bool(m.read_only);
        Ok(())
    })?;
    w.option_with(spec.hostname.as_deref(), Writer::string)?;
    w.u8(match spec.network {
        Network::Host => 0,
        Network::Private => 1,
    });
    w.list(&spec.hosts, |w, (name, addr)| {
        w.string(name)?;
        w.string(addr)
    })
}

fn oci(r: &mut Reader<'_>) -> Result<OciSpec, Fault> {
    let rootfs = match r.u8()? {
        0 => Rootfs::Dir(r.path()?),
        1 => Rootfs::Layers(r.list(|r| r.path())?),
        t => return Err(Fault::value("rootfs", format!("variant {t}"))),
    };
    let init = program(r)?;
    let mounts = r.list(|r| {
        Ok(Mount {
            source: r.path()?,
            destination: r.string("mount destination")?,
            read_only: r.bool("read_only")?,
        })
    })?;
    let hostname = r.option(|r| r.string("hostname"))?;
    let network = match r.u8()? {
        0 => Network::Host,
        1 => Network::Private,
        t => return Err(Fault::value("network", format!("variant {t}"))),
    };
    let hosts = r.list(|r| Ok((r.string("host name")?, r.string("host address")?)))?;
    Ok(OciSpec {
        rootfs,
        init,
        mounts,
        hostname,
        network,
        hosts,
    })
}

fn put_program(w: &mut Writer, p: &Program) -> Result<(), Fault> {
    w.list(&p.args, |w, a| w.string(a))?;
    w.list(&p.env, |w, (k, v)| {
        w.string(k)?;
        w.string(v)
    })?;
    w.option_with(p.cwd.as_deref(), Writer::string)?;
    w.option_with(p.user.as_ref(), |w, u| {
        w.u32(u.uid);
        w.u32(u.gid);
        w.list(&u.groups, |w, g| {
            w.u32(*g);
            Ok(())
        })
    })
}

fn program(r: &mut Reader<'_>) -> Result<Program, Fault> {
    Ok(Program {
        args: r.list(|r| r.string("argument"))?,
        env: r.list(|r| Ok((r.string("variable")?, r.string("value")?)))?,
        cwd: r.option(|r| r.string("cwd"))?,
        user: r.option(|r| {
            Ok(User {
                uid: r.u32()?,
                gid: r.u32()?,
                groups: r.list(Reader::u32)?,
            })
        })?,
    })
}

fn box_id(r: &mut Reader<'_>) -> Result<BoxId, Fault> {
    let s = r.string("box id")?;
    BoxId::new(&s).map_err(|e| Fault::value("box id", e.to_string()))
}

fn class_tag(c: Class) -> u8 {
    match c {
        Class::Process => 0,
        Class::Oci => 1,
    }
}

fn class(r: &mut Reader<'_>) -> Result<Class, Fault> {
    match r.u8()? {
        0 => Ok(Class::Process),
        1 => Ok(Class::Oci),
        t => Err(Fault::value("class", format!("variant {t}"))),
    }
}

fn put_info(w: &mut Writer, info: &BoxInfo) -> Result<(), Fault> {
    w.string(info.id.as_str())?;
    w.u8(class_tag(info.class));
    w.u8(match info.state {
        BoxState::Empty => 0,
        BoxState::Running => 1,
        BoxState::Created => 2,
    });
    put_bounds(w, &info.bounds);
    w.bool(info.adopted);
    w.option(info.exit, put_exit);
    w.u64(info.abandoned);
    Ok(())
}

fn info(r: &mut Reader<'_>) -> Result<BoxInfo, Fault> {
    Ok(BoxInfo {
        id: box_id(r)?,
        class: class(r)?,
        state: match r.u8()? {
            0 => BoxState::Empty,
            1 => BoxState::Running,
            2 => BoxState::Created,
            t => return Err(Fault::value("box state", format!("variant {t}"))),
        },
        bounds: bounds(r)?,
        adopted: r.bool("adopted")?,
        exit: r.option(exit)?,
        abandoned: r.u64()?,
    })
}

fn put_exit(w: &mut Writer, exit: Exit) {
    match exit {
        Exit::Code(c) => {
            w.u8(0);
            w.i32(c);
        }
        Exit::Signal(s) => {
            w.u8(1);
            w.i32(s);
        }
        Exit::Unknown => w.u8(2),
    }
}

fn exit(r: &mut Reader<'_>) -> Result<Exit, Fault> {
    match r.u8()? {
        0 => Ok(Exit::Code(r.i32()?)),
        1 => Ok(Exit::Signal(r.i32()?)),
        2 => Ok(Exit::Unknown),
        t => Err(Fault::value("exit", format!("variant {t}"))),
    }
}

fn put_refusal(w: &mut Writer, refusal: &Refusal) -> Result<(), Fault> {
    match refusal {
        Refusal::Invalid { what, why } => {
            w.u8(0);
            w.string(what)?;
            w.string(why)
        }
        Refusal::NoSuchBox(b) => {
            w.u8(1);
            w.string(b.as_str())
        }
        Refusal::Exists(b) => {
            w.u8(2);
            w.string(b.as_str())
        }
        Refusal::NotRunning(b, op) => {
            w.u8(3);
            w.string(b.as_str())?;
            w.string(op)
        }
        Refusal::Platform {
            what,
            platform,
            why,
        } => {
            w.u8(4);
            w.string(what)?;
            w.string(platform)?;
            w.string(why)
        }
        Refusal::Full(n) => {
            w.u8(5);
            w.usize(*n)
        }
        Refusal::Exhausted { what, bound } => {
            w.u8(6);
            w.string(what)?;
            w.usize(*bound)
        }
        Refusal::Os { op, why } => {
            w.u8(7);
            w.string(op)?;
            w.string(why)
        }
        Refusal::Broken(why) => {
            w.u8(8);
            w.string(why)
        }
    }
}

fn refusal(r: &mut Reader<'_>) -> Result<Refusal, Fault> {
    Ok(match r.u8()? {
        0 => Refusal::Invalid {
            what: name(&r.string("refusal name")?)?,
            why: r.string("refusal reason")?,
        },
        1 => Refusal::NoSuchBox(box_id(r)?),
        2 => Refusal::Exists(box_id(r)?),
        3 => Refusal::NotRunning(box_id(r)?, name(&r.string("refusal name")?)?),
        4 => Refusal::Platform {
            what: name(&r.string("refusal name")?)?,
            platform: name(&r.string("refusal name")?)?,
            why: r.string("refusal reason")?,
        },
        5 => Refusal::Full(r.usize()?),
        6 => Refusal::Exhausted {
            what: name(&r.string("refusal name")?)?,
            bound: r.usize()?,
        },
        7 => Refusal::Os {
            op: name(&r.string("refusal name")?)?,
            why: r.string("refusal reason")?,
        },
        8 => Refusal::Broken(r.string("refusal reason")?),
        t => return Err(Fault::value("refusal", format!("variant {t}"))),
    })
}

/// Most distinct refusal names a process keeps, and the longest: a node agent's refusals name what
/// and why from a fixed set in its code (a few hundred at most), so these bound only a hostile one.
const NAMES: usize = 1024;
const NAME_BYTES: usize = 256;

/// The names decoded refusals carry. hex-api's refusals name what they refuse with `&'static str`,
/// so a name read from the wire is kept for the process's life, once however often it comes (at
/// most [`NAMES`] of at most [`NAME_BYTES`] each: 256 KiB). A name past either bound is refused.
static KEPT: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());

fn name(s: &str) -> Result<&'static str, Fault> {
    if s.len() > NAME_BYTES {
        return Err(Fault::value(
            "refusal name",
            format!("{} bytes, over the {NAME_BYTES} kept", s.len()),
        ));
    }
    let mut kept = KEPT
        .lock()
        .map_err(|_| Fault::value("refusal name", "the kept names' lock was poisoned"))?;
    if let Some(k) = kept.get(s) {
        return Ok(k);
    }
    if kept.len() >= NAMES {
        return Err(Fault::value(
            "refusal name",
            format!("{s:?}: the {NAMES} names this process keeps are spent"),
        ));
    }
    let leaked: &'static str = s.to_owned().leak();
    kept.insert(leaked);
    Ok(leaked)
}

/// A message being written, refused once it would pass its bound.
pub(crate) struct Writer {
    out: Vec<u8>,
    bound: usize,
}

impl Writer {
    pub(crate) fn new(bound: usize) -> Self {
        Self {
            out: Vec::with_capacity(HEADER_BYTES.min(bound)),
            bound,
        }
    }

    pub(crate) fn header(&mut self, version: u16, kind: u16, id: u64) {
        self.out.extend_from_slice(&MAGIC);
        self.u16(version);
        self.u16(kind);
        self.u64(id);
    }

    /// Bytes as they are, with no length before them: another message's header.
    pub(crate) fn raw(&mut self, b: &[u8]) {
        self.out.extend_from_slice(b);
    }

    pub(crate) fn done(self) -> Result<Vec<u8>, Fault> {
        if self.out.len() > self.bound {
            return Err(Fault::Oversized {
                bytes: self.out.len(),
                bound: self.bound,
            });
        }
        Ok(self.out)
    }

    pub(crate) fn u8(&mut self, v: u8) {
        self.out.push(v);
    }

    pub(crate) fn u16(&mut self, v: u16) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn u32(&mut self, v: u32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn u64(&mut self, v: u64) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn i32(&mut self, v: i32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn usize(&mut self, v: usize) -> Result<(), Fault> {
        let v =
            u64::try_from(v).map_err(|_| Fault::value("count", format!("{v} is over 64 bits")))?;
        self.u64(v);
        Ok(())
    }

    pub(crate) fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }

    /// A length, refused when what follows it would pass the bound: a caller's huge argument list is
    /// refused before it is copied.
    pub(crate) fn len(&mut self, n: usize) -> Result<(), Fault> {
        let after = self.out.len().checked_add(4).and_then(|l| l.checked_add(n));
        if after.is_none_or(|after| after > self.bound) {
            return Err(Fault::Oversized {
                bytes: after.unwrap_or(usize::MAX),
                bound: self.bound,
            });
        }
        let n = u32::try_from(n).map_err(|_| Fault::Oversized {
            bytes: n,
            bound: self.bound,
        })?;
        self.u32(n);
        Ok(())
    }

    pub(crate) fn bytes(&mut self, b: &[u8]) -> Result<(), Fault> {
        self.len(b.len())?;
        self.out.extend_from_slice(b);
        Ok(())
    }

    pub(crate) fn string(&mut self, s: &str) -> Result<(), Fault> {
        self.bytes(s.as_bytes())
    }

    pub(crate) fn path(&mut self, p: &Path) -> Result<(), Fault> {
        self.bytes(path_bytes(p)?)
    }

    pub(crate) fn option<T>(&mut self, v: Option<T>, put: impl FnOnce(&mut Self, T)) {
        match v {
            None => self.u8(0),
            Some(v) => {
                self.u8(1);
                put(self, v);
            }
        }
    }

    pub(crate) fn option_with<T>(
        &mut self,
        v: Option<T>,
        put: impl FnOnce(&mut Self, T) -> Result<(), Fault>,
    ) -> Result<(), Fault> {
        match v {
            None => {
                self.u8(0);
                Ok(())
            }
            Some(v) => {
                self.u8(1);
                put(self, v)
            }
        }
    }

    pub(crate) fn list<T>(
        &mut self,
        items: &[T],
        mut put: impl FnMut(&mut Self, &T) -> Result<(), Fault>,
    ) -> Result<(), Fault> {
        let n = u32::try_from(items.len()).map_err(|_| Fault::Oversized {
            bytes: items.len(),
            bound: self.bound,
        })?;
        self.u32(n);
        for item in items {
            put(self, item)?;
        }
        Ok(())
    }
}

/// A message being read, from the front.
pub(crate) struct Reader<'a> {
    rest: &'a [u8],
    len: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self {
            rest: bytes,
            len: bytes.len(),
        }
    }

    pub(crate) fn at(&self) -> usize {
        self.len.saturating_sub(self.rest.len())
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], Fault> {
        let Some((head, rest)) = self.rest.split_at_checked(n) else {
            return Err(Fault::Truncated {
                need: self.at().saturating_add(n),
                have: self.len,
            });
        };
        self.rest = rest;
        Ok(head)
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], Fault> {
        let at = self.at();
        <[u8; N]>::try_from(self.take(N)?).map_err(|_| Fault::Truncated {
            need: at.saturating_add(N),
            have: self.len,
        })
    }

    /// The header: magic first, so a message of something else is refused as that.
    pub(crate) fn header(&mut self) -> Result<(u16, u16, u64), Fault> {
        if self.array::<4>()? != MAGIC {
            return Err(Fault::Magic);
        }
        Ok((self.u16()?, self.u16()?, self.u64()?))
    }

    /// That nothing follows what the message's version defines.
    pub(crate) fn end(&self, kind: u16, version: u16) -> Result<(), Fault> {
        if self.rest.is_empty() {
            Ok(())
        } else {
            Err(Fault::Trailing {
                bytes: self.rest.len(),
                kind,
                version,
            })
        }
    }

    pub(crate) fn u8(&mut self) -> Result<u8, Fault> {
        Ok(u8::from_le_bytes(self.array()?))
    }

    pub(crate) fn u16(&mut self) -> Result<u16, Fault> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, Fault> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, Fault> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    pub(crate) fn i32(&mut self) -> Result<i32, Fault> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    pub(crate) fn usize(&mut self) -> Result<usize, Fault> {
        let v = self.u64()?;
        usize::try_from(v)
            .map_err(|_| Fault::value("count", format!("{v} is over this platform's")))
    }

    pub(crate) fn bool(&mut self, what: &'static str) -> Result<bool, Fault> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            b => Err(Fault::value(what, format!("byte {b} is neither 0 nor 1"))),
        }
    }

    pub(crate) fn bytes(&mut self) -> Result<&'a [u8], Fault> {
        let n = self.u32()?;
        let n = usize::try_from(n)
            .map_err(|_| Fault::value("length", format!("{n} is over this platform's")))?;
        self.take(n)
    }

    pub(crate) fn string(&mut self, what: &'static str) -> Result<String, Fault> {
        let b = self.bytes()?;
        String::from_utf8(b.to_vec()).map_err(|e| Fault::value(what, format!("isn't UTF-8: {e}")))
    }

    pub(crate) fn path(&mut self) -> Result<PathBuf, Fault> {
        path_of(self.bytes()?)
    }

    pub(crate) fn option<T>(
        &mut self,
        get: impl FnOnce(&mut Self) -> Result<T, Fault>,
    ) -> Result<Option<T>, Fault> {
        match self.u8()? {
            0 => Ok(None),
            1 => get(self).map(Some),
            b => Err(Fault::value(
                "option",
                format!("byte {b} is neither 0 nor 1"),
            )),
        }
    }

    /// A list: its count first, refused when more items are claimed than bytes are left, since each
    /// takes at least one; so a short message never makes room for a long list.
    pub(crate) fn list<T>(
        &mut self,
        mut get: impl FnMut(&mut Self) -> Result<T, Fault>,
    ) -> Result<Vec<T>, Fault> {
        let n = self.u32()?;
        let n = usize::try_from(n)
            .map_err(|_| Fault::value("count", format!("{n} is over this platform's")))?;
        if n > self.rest.len() {
            return Err(Fault::Truncated {
                need: self.at().saturating_add(n),
                have: self.len,
            });
        }
        // Room for a few: each item's room past that comes as it is read, so a claimed count costs
        // nothing until its items arrive.
        let mut out = Vec::with_capacity(n.min(16));
        for _ in 0..n {
            out.push(get(self)?);
        }
        Ok(out)
    }
}

/// A path's bytes: a Unix path is any bytes, so it crosses as they are.
#[cfg(unix)]
fn path_bytes(p: &Path) -> Result<&[u8], Fault> {
    use std::os::unix::ffi::OsStrExt;
    Ok(p.as_os_str().as_bytes())
}

/// A path's bytes where paths aren't bytes: its UTF-8, or a refusal for a path that has none.
#[cfg(not(unix))]
fn path_bytes(p: &Path) -> Result<&[u8], Fault> {
    p.to_str()
        .map(str::as_bytes)
        .ok_or_else(|| Fault::value("path", format!("{} isn't Unicode", p.display())))
}

#[cfg(unix)]
fn path_of(b: &[u8]) -> Result<PathBuf, Fault> {
    use std::os::unix::ffi::OsStrExt;
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(b)))
}

#[cfg(not(unix))]
fn path_of(b: &[u8]) -> Result<PathBuf, Fault> {
    std::str::from_utf8(b)
        .map(PathBuf::from)
        .map_err(|e| Fault::value("path", format!("isn't UTF-8: {e}")))
}

#[cfg(test)]
mod tests;
