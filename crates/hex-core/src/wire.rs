//! The node agent's local socket as bytes (decision 3 of docs/design/runtime-architecture.md): the
//! `Runtime` seam's requests and answers as messages of a Unix `SOCK_SEQPACKET` socket, and back.
//! Pure: the shells, `hex-node`'s server and `hex-client`, move the messages and the descriptors that
//! ride with them (exec's stdio, by `SCM_RIGHTS`); this decides what each message says, and refuses by
//! type every one that says nothing valid.
//!
//! A message is one `SOCK_SEQPACKET` record, so the kernel keeps its bounds (unix(7)): a 22-byte
//! header, then the body its kind defines.
//!
//! | bytes | field |
//! |---|---|
//! | 0..4 | `HEXW` |
//! | 4..12 | the sender's wire identity ([`IDENTITY`]) |
//! | 12..14 | kind: a request below `0x8000`, an answer from it |
//! | 14..22 | id: a request's, chosen by its client; its answer, and an exec'd program's end, carry it |
//!
//! Integers are little-endian. A string or a path is a `u32` length and its bytes, an option a byte
//! (0 none, 1 some) before the value, a list a `u32` count before its items, and an enum a byte naming
//! its variant before its fields.
//!
//! One wire: no versions. Both sides are built from the same wire definitions or they don't talk. Every message
//! carries its sender's wire identity, a digest of this module's source and [`crate::stream`]'s (the kinds and
//! their bodies, and the frames a stream carries them in) made at compile time, so a change to either is a new
//! identity with no number to bump. A reader refuses a message of another identity before it reads its body, so
//! nothing of another wire is misread (§3). The one layout every build keeps is a hello's head: the header, then
//! its sender's build as a string ([`build`]), first in a hello and first in its answer. So a peer built from
//! another wire is refused at the hello, typed, naming both identities and both builds ([`Fault::Wire`]): the agent
//! answers a message of another identity, its hello first, with its own hello and ends the connection, and the client
//! refuses an answer of another identity. A reader also refuses bytes past what a kind's body holds rather than
//! skip them, so nothing is dropped silently.
//!
//! The client says it took a given answer's ends ([`Request::Taken`], not answered, its header's id the exec's)
//! before anything it sends after the answer; the node agent keeps its own copies of them until then, or until the
//! connection closes, so no end is ever in flight with no other reference to it: XNU's collector of descriptors in
//! flight flushes a socket whose only reference is a message in flight, and it arrives reading nothing (on macOS
//! 26.4, 719 of 20,000 sent so arrived dead with the sender's copy closed at once, none with it kept until
//! received). Kept ends hold their program's place among the programs until taken ([`Limits::programs`],
//! [`Budgets::programs`]).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use hex_api::{
    Allocation, Bounds, BoxId, BoxInfo, BoxOwner, BoxSpec, BoxState, Capacity, CapacityResources,
    Class, ClassSpec, DeviceId, DeviceInfo, DeviceKind, Exit, IoRates, LifecycleCause, MAX_DEVICES,
    Mount, Network, OciSpec, Program, Refusal, Rootfs, RuntimeCost, Signal, Usage, User,
};

use crate::netns;

/// Every message's first bytes.
pub const MAGIC: [u8; 4] = *b"HEXW";
/// This build's wire identity: FNV-1a (64-bit) of the wire definitions' source, this module's and
/// [`crate::stream`]'s, made at compile time, carriage returns skipped so a checkout's line endings can't split it.
/// Two builds talk only when theirs are the same.
pub const IDENTITY: u64 = identity(&[
    include_bytes!("wire.rs"),
    include_bytes!("stream.rs"),
    include_bytes!("schedule.rs"),
    include_bytes!("../../hex-sim/src/peer.rs"),
    include_bytes!("../../hex-sim/src/model.rs"),
    include_bytes!("../../hex-api/src/spec.rs"),
    include_bytes!("../../hex-api/src/id.rs"),
    include_bytes!("../../hex-api/src/bounds.rs"),
    include_bytes!("../../hex-api/src/devices.rs"),
    include_bytes!("../../hex-api/src/runtime.rs"),
]);
/// The header's length: magic, wire identity, kind and id.
pub const HEADER_BYTES: usize = 22;
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

/// FNV-1a (64-bit) of `sources` in turn, every carriage return skipped: the offset basis and prime are FNV-1a's own
/// (IETF draft-eastlake-fnv, "FNV Basics" and "FNV Constants"). A digest, not a seal: it tells two wires apart, and
/// FNV-1a because it is a few lines of `const fn`, where a cryptographic hash isn't.
const fn identity(sources: &[&[u8]]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    let mut sources = sources;
    while let Some((source, more)) = sources.split_first() {
        let mut bytes = *source;
        while let Some((&byte, rest)) = bytes.split_first() {
            if byte != b'\r' {
                hash ^= byte as u64;
                hash = hash.wrapping_mul(PRIME);
            }
            bytes = rest;
        }
        sources = more;
    }
    hash
}

/// This build as a hello names it: its crate's version and the commit it was built from (`HEX_COMMIT` at build
/// time, which kiln CI sets), else "unknown commit".
pub fn build() -> String {
    format!(
        "hex {} ({})",
        env!("CARGO_PKG_VERSION"),
        option_env!("HEX_COMMIT").unwrap_or("unknown commit")
    )
}

/// The kinds. Requests sit below `0x8000`, answers from it.
mod kind {
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
    pub const NETNS: u16 = 0x000b;
    pub const NETNS_ANSWER: u16 = 0x8009;
    pub const KILL: u16 = 0x000c;
    pub const EXEC_STREAMS: u16 = 0x000d;
    pub const GIVEN: u16 = 0x800a;
    pub const TAKEN: u16 = 0x000e;
    pub const RUN: u16 = 0x000f;
    pub const STREAMS_READY: u16 = 0x0010;
    pub const SCHEDULE: u16 = 0x0011;
    pub const SCHEDULED: u16 = 0x800c;
    pub const OFFERED: u16 = 0x800b;
    pub const CAPACITY: u16 = 0x0012;
    pub const CAPACITY_ANSWER: u16 = 0x800d;
}

/// A message's header, once its wire identity is read as this side's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub kind: u16,
    pub id: u64,
}

/// What a client sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// A connection's first message, naming the client's build ([`build`]): answered with the node agent's build,
    /// what it runs and its bounds.
    Hello { build: String },
    /// A call of the seam, answered once.
    Call(Call),
    /// The client no longer waits for the end of the program its header's id started (its `Proc` was
    /// dropped): the node agent stops watching it and lets it run on, as dropping an in-process `Proc`
    /// does. Not answered.
    Release,
    /// The client took the ends of the streams its header's id's exec was given ([`Answer::Given`]), so the node
    /// agent lets its own copies go. Not answered.
    Taken,
    /// The caller opened every connection in an Offered answer. The original exec may now start.
    /// Not answered separately; the exec's Started/Refused and Exited keep its original header id.
    StreamsReady,
}

/// A call of the node agent: the `Runtime` seam's, its network namespace counts, and a program's kill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    /// Scheduler control, using the same bounded connection and reader as Runtime calls.
    Schedule(Box<crate::schedule::Command>),
    Create(Box<BoxSpec>),
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
    /// The node agent's network namespace counts ([`netns::Counts`]).
    Netns,
    /// KILL to the program exec `exec` of this connection started (`Proc::kill`), and only it.
    /// Answered `Done` once sent, or when that program has ended already.
    Kill {
        exec: u64,
    },
    /// An exec, as [`Call::Exec`], that also asks the node to make the streams `ask` names
    /// (`Stdio::ask`); answered [`Answer::Given`] with the caller's ends of them. A stream both carried
    /// and asked for is refused.
    ExecStreams {
        id: BoxId,
        program: Program,
        streams: Streams,
        ask: Streams,
    },
    /// `program` in a `process` box made from `spec` for its life (`Runtime::run`, decision 7 of
    /// docs/design/runtime-architecture.md): a create's body, then an exec's program, the streams it carries as the
    /// message's descriptors, in order, and the streams it asks the node to make, as [`Call::ExecStreams`]'s. Answered
    /// as an exec is, [`Answer::Started`] or [`Answer::Given`], and later [`Answer::Exited`], which comes once the
    /// box is gone.
    Run {
        spec: Box<BoxSpec>,
        program: Program,
        streams: Streams,
        ask: Streams,
    },
    Capacity(Class),
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
    Scheduled(Box<crate::schedule::Reply>),
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
    /// netns's.
    Netns(netns::Counts),
    /// An exec that asked for streams ([`Call::ExecStreams`]), or whose program can't take the
    /// pipes it carried (a guest's), runs, with this process id; the caller's ends of the streams `given` names
    /// ride as the message's descriptors, in order. Its end comes later as [`Answer::Exited`].
    Given {
        pid: u32,
        given: Streams,
    },
    /// HEX-RT-17: open these guest connections in the caller before acknowledging StreamsReady.
    /// A preparation, not a started program; no descriptor or cross-process socket reservation rides with it.
    Offered(StreamOffer),
    Capacity(Option<Capacity>),
}

/// A Shards guest's identity and the names of an exec's caller-owned streams (HEX-RT-17).
/// The node holds a dedicated control connection until the offer is committed or cancelled, so dropping an
/// uncommitted offer removes only its own early streams and cannot end another caller's program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamOffer {
    pub pid: u32,
    pub started: u64,
    pub vsock: PathBuf,
    pub token: u64,
    pub exec: u64,
    pub streams: Streams,
}

/// The answer to a hello.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    /// The node agent's build ([`build`]): first in the answer, as the client's is in its hello.
    pub build: String,
    pub classes: Vec<Class>,
    pub limits: Limits,
    /// The node's budgets on all its connections together.
    pub budgets: Budgets,
}

/// The node agent's budgets on all its connections together: past either, a call or an exec is refused
/// by type, whichever connection it comes on, but for what each connection's limits keep for it ([`Limits`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budgets {
    /// Requests asked and not yet answered, on every connection together.
    pub in_flight: usize,
    /// Programs execs started that the connections still watch, or whose given ends their clients haven't yet said
    /// they took ([`Request::Taken`]), together.
    pub programs: usize,
}

/// The node agent's bounds on its socket, each refused past by type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Connections open at once.
    pub connections: usize,
    /// Bytes in one message, at most [`MAX_MESSAGE`].
    pub message_bytes: usize,
    /// Requests one connection may have asked and not yet had answered: the node keeps this many for each
    /// connection it admits, so one within it is never refused.
    pub in_flight: usize,
    /// Programs one connection's execs started that it may still watch, or whose given ends its client hasn't yet
    /// said it took ([`Request::Taken`]): kept for each connection likewise.
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
    /// A version the other protocols this codec carries don't speak: the seeds', the macOS box helper's, a stdio
    /// connection's name (the node socket's wire has none: [`Fault::Wire`]).
    #[error("version {got}: this side speaks 1 to {speaks}")]
    Version { got: u16, speaks: u16 },
    /// A message of another wire: its sender's identity, and its build where the message is a hello or a hello's
    /// answer, whose head every build lays out alike.
    #[error("{}", other_wire(*theirs, build.as_deref()))]
    Wire { theirs: u64, build: Option<String> },
    /// A kind the node socket's wire doesn't define.
    #[error("kind {kind:#06x} isn't {expected} this wire defines")]
    Undefined { kind: u16, expected: &'static str },
    /// Bytes past what the node socket's wire defines for a message's kind.
    #[error("{bytes} bytes follow what this wire defines for kind {kind:#06x}")]
    Leftover { bytes: usize, kind: u16 },
    /// A kind a message's version doesn't define, in the other protocols this codec carries: the seeds', the macOS
    /// box helper's.
    #[error("kind {kind:#06x} isn't {expected} version {version} defines")]
    Kind {
        kind: u16,
        version: u16,
        expected: &'static str,
    },
    /// Bytes past what a message's version defines for its kind, in the other protocols this codec carries.
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
    #[error("{what} storage for {bound} bytes is unavailable")]
    Storage { what: &'static str, bound: usize },
}

impl Fault {
    pub(crate) fn value(what: &'static str, why: impl Into<String>) -> Self {
        Self::Value {
            what,
            why: why.into(),
        }
    }
}

/// What a message of another wire is refused saying: both identities and both builds.
fn other_wire(theirs: u64, build: Option<&str>) -> String {
    format!(
        "the peer's wire is {theirs:016x} ({}), this side's {IDENTITY:016x} ({}): two sides talk only when built from \
         the same wire",
        build.unwrap_or("its build unsaid"),
        self::build()
    )
}

impl From<Fault> for Refusal {
    fn from(fault: Fault) -> Self {
        match fault {
            Fault::Storage { what, bound } => Refusal::Exhausted { what, bound },
            fault => {
                let what = match &fault {
                    Fault::Wire { .. } => "wire",
                    _ => "message",
                };
                Refusal::Invalid {
                    what,
                    why: fault.to_string(),
                }
            }
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

/// A request refused: the fault, and where to send the refusal (the request's id; 0 when its header couldn't be
/// read).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bad {
    pub id: u64,
    pub fault: Fault,
}

/// A request as a message of at most `bound` bytes.
pub fn request(id: u64, request: &Request, bound: usize) -> Result<Vec<u8>, Fault> {
    let mut w = Writer::new(bound);
    match request {
        Request::Hello { build } => {
            w.header(kind::HELLO, id);
            w.string(build)?;
        }
        Request::Release => w.header(kind::RELEASE, id),
        Request::Taken => w.header(kind::TAKEN, id),
        Request::StreamsReady => w.header(kind::STREAMS_READY, id),
        Request::Call(call) => put_call(&mut w, id, call)?,
    }
    w.done()
}

/// An answer as a message of at most `bound` bytes.
pub fn answer(id: u64, answer: &Answer, bound: usize) -> Result<Vec<u8>, Fault> {
    let mut w = Writer::new(bound);
    match answer {
        Answer::Hello(h) => {
            w.header(kind::HELLO_ANSWER, id);
            w.string(&h.build)?;
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
            w.usize(h.budgets.in_flight)?;
            w.usize(h.budgets.programs)?;
        }
        Answer::Scheduled(reply) => {
            w.header(kind::SCHEDULED, id);
            w.string(
                &serde_json::to_string(reply)
                    .map_err(|e| Fault::value("scheduler reply", e.to_string()))?,
            )?;
        }
        Answer::Capacity(capacity) => {
            w.header(kind::CAPACITY_ANSWER, id);
            w.option_with(capacity.as_ref(), |w, capacity| {
                capacity
                    .check()
                    .map_err(|e| Fault::value("capacity", e.to_string()))?;
                w.string(&capacity.id)?;
                for room in [capacity.total, capacity.free] {
                    w.u64(room.cpu_milli);
                    w.u64(room.mem_bytes);
                    w.u64(room.disk_bytes);
                }
                w.device_count(capacity.devices.len(), 22)?;
                for device in &capacity.devices {
                    put_device_id(w, &device.id);
                    w.string(&device.model)?;
                    w.u64(device.size);
                    w.option_with(device.holder.as_ref(), |w, id| w.string(id.as_str()))?;
                }
                put_cost_option(w, capacity.cost)
            })?;
        }
        Answer::Info(info) => {
            w.header(kind::INFO, id);
            put_info(&mut w, info)?;
        }
        Answer::Done => w.header(kind::DONE, id),
        Answer::Started { pid } => {
            w.header(kind::STARTED, id);
            w.u32(*pid);
        }
        Answer::Offered(offer) => {
            w.header(kind::OFFERED, id);
            w.u32(offer.pid);
            w.u64(offer.started);
            w.path(&offer.vsock)?;
            w.u64(offer.token);
            w.u64(offer.exec);
            w.u8(offer.streams.bits());
        }
        Answer::Given { pid, given } => {
            w.header(kind::GIVEN, id);
            w.u32(*pid);
            w.u8(given.bits());
        }
        Answer::Usage(u) => {
            w.header(kind::USAGE_ANSWER, id);
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
            w.header(kind::BOXES, id);
            w.bool(*more);
            w.list(infos, put_info)?;
        }
        Answer::Refused(r) => {
            w.header(kind::REFUSED, id);
            put_refusal(&mut w, r)?;
        }
        Answer::Exited(exit) => {
            w.header(kind::EXITED, id);
            put_exit(&mut w, *exit);
        }
        Answer::Netns(counts) => {
            w.header(kind::NETNS_ANSWER, id);
            for n in [
                counts.made_ahead,
                counts.taken,
                counts.made_on_path,
                counts.discarded,
                counts.failed,
            ] {
                w.u64(n);
            }
            let by = counts.failed_by;
            for n in [by.home, by.unshare, by.open, by.loopback, by.back] {
                w.u64(n);
            }
            w.u8(counts.refused.map_or(0, |m| {
                u8::from(m.sys_admin()) | (u8::from(m.net_admin()) << 1)
            }));
            w.u64(counts.refused_on_path);
            let last = &counts.last_failed;
            for reason in [
                &last.home,
                &last.unshare,
                &last.open,
                &last.loopback,
                &last.back,
            ] {
                w.option_with(reason.as_ref(), |w, reason| {
                    w.option(reason.errno(), |w, errno| w.i32(errno));
                    w.string(reason.text())
                })?;
            }
        }
    }
    w.done()
}

/// A list's answer as messages of at most `bound` bytes each: as many boxes in each as fit, every
/// message but the last marked `more`, so a node holding more boxes than one message carries still
/// answers a list whole. An empty list is one message.
pub fn boxes(id: u64, infos: &[BoxInfo], bound: usize) -> Result<Vec<Vec<u8>>, Fault> {
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
    parse_request(msg, bound, true)
}

/// The request `bytes` hold, at most `bound` bytes, from a transport whose exec's stdio travels apart
/// from its message ([`crate::stream`]'s, on connections of its own): the message carries no
/// descriptors, and an exec's streams are what it names.
pub fn read_request_apart(bytes: &[u8], bound: usize) -> Result<(Header, Request), Bad> {
    let msg = Received {
        bytes,
        size: bytes.len(),
        descriptors: 0,
        descriptors_cut: false,
    };
    parse_request(&msg, bound, false)
}

/// A request, its descriptors counted against its exec's streams when they ride with it (`carried`).
fn parse_request(
    msg: &Received<'_>,
    bound: usize,
    carried: bool,
) -> Result<(Header, Request), Bad> {
    let mut r = Reader::new(msg.bytes);
    let (identity, kind, id) = r.header().map_err(|fault| Bad { id: 0, fault })?;
    let bad = |fault| Bad { id, fault };
    if identity != IDENTITY {
        return Err(bad(other(&mut r, identity, kind == kind::HELLO)));
    }
    check(msg, bound).map_err(bad)?;
    let request = match kind {
        kind::HELLO => Request::Hello {
            build: r.string("build").map_err(bad)?,
        },
        kind::RELEASE => Request::Release,
        kind::TAKEN => Request::Taken,
        kind::STREAMS_READY => Request::StreamsReady,
        kind::SCHEDULE => {
            let json = r.string("scheduler command").map_err(bad)?;
            Request::Call(Call::Schedule(Box::new(
                serde_json::from_str(&json)
                    .map_err(|e| bad(Fault::value("scheduler command", e.to_string())))?,
            )))
        }
        kind::CAPACITY => Request::Call(Call::Capacity(class(&mut r).map_err(bad)?)),
        kind::CREATE => Request::Call(Call::Create(Box::new(box_spec(&mut r).map_err(bad)?))),
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
        kind::KILL => Request::Call(Call::Kill {
            exec: r.u64().map_err(bad)?,
        }),
        kind::EXEC_STREAMS => {
            let id = box_id(&mut r).map_err(bad)?;
            let program = program(&mut r).map_err(bad)?;
            let streams = r.u8().and_then(Streams::of).map_err(bad)?;
            let ask = r.u8().and_then(Streams::of).map_err(bad)?;
            Request::Call(Call::ExecStreams {
                id,
                program,
                streams,
                ask,
            })
        }
        kind::RUN => {
            let spec = box_spec(&mut r).map_err(bad)?;
            let program = program(&mut r).map_err(bad)?;
            let streams = r.u8().and_then(Streams::of).map_err(bad)?;
            let ask = r.u8().and_then(Streams::of).map_err(bad)?;
            Request::Call(Call::Run {
                spec: Box::new(spec),
                program,
                streams,
                ask,
            })
        }
        _ => {
            return Err(bad(Fault::Undefined {
                kind,
                expected: "a request",
            }));
        }
    };
    let want = match &request {
        Request::Call(
            Call::Exec { streams, .. }
            | Call::ExecStreams { streams, .. }
            | Call::Run { streams, .. },
        ) if carried => streams.count(),
        _ => 0,
    };
    if msg.descriptors != want {
        return Err(bad(Fault::Descriptors {
            what: request_name(kind),
            want,
            got: msg.descriptors,
        }));
    }
    r.all_read(kind).map_err(bad)?;
    Ok((Header { kind, id }, request))
}

/// What a message of another wire, whose sender's identity is `theirs`, is refused as: its sender's build read
/// where the message is a hello or a hello's answer (`hello`), whose head every build lays out alike.
fn other(r: &mut Reader<'_>, theirs: u64, hello: bool) -> Fault {
    Fault::Wire {
        theirs,
        build: hello.then(|| r.string("build").ok()).flatten(),
    }
}

/// The answer a received message holds, at most `bound` bytes, or why it holds none. An answer carries
/// no descriptors but [`Answer::Given`]'s, one for each stream it names.
pub fn read_answer(msg: &Received<'_>, bound: usize) -> Result<(Header, Answer), Fault> {
    let mut r = Reader::new(msg.bytes);
    let (identity, kind, id) = r.header()?;
    if identity != IDENTITY {
        return Err(other(&mut r, identity, kind == kind::HELLO_ANSWER));
    }
    check(msg, bound)?;
    if kind != kind::GIVEN && msg.descriptors != 0 {
        return Err(Fault::Descriptors {
            what: "an answer",
            want: 0,
            got: msg.descriptors,
        });
    }
    let answer = match kind {
        kind::HELLO_ANSWER => {
            let build = r.string("build")?;
            let classes = r.list(class)?;
            let mut usize = || r.usize();
            let limits = Limits {
                connections: usize()?,
                message_bytes: usize()?,
                in_flight: usize()?,
                programs: usize()?,
            };
            let budgets = Budgets {
                in_flight: usize()?,
                programs: usize()?,
            };
            Answer::Hello(Hello {
                build,
                classes,
                limits,
                budgets,
            })
        }
        kind::SCHEDULED => {
            let json = r.string("scheduler reply")?;
            Answer::Scheduled(Box::new(
                serde_json::from_str(&json)
                    .map_err(|e| Fault::value("scheduler reply", e.to_string()))?,
            ))
        }
        kind::CAPACITY_ANSWER => Answer::Capacity(r.option(|r| {
            let id = r.string("capacity pool")?;
            let capacity = Capacity {
                id,
                total: resources(r)?,
                free: resources(r)?,
                devices: r.device_list(22, |r| {
                    let id = device_id(r)?;
                    let model = r.device_string("devices.model")?;
                    let size = r.u64()?;
                    let holder = r.option(|r| {
                        let bytes = r.bytes()?;
                        let text = std::str::from_utf8(bytes)
                            .map_err(|_| Fault::value("devices.holder", "is not UTF-8"))?;
                        BoxId::new(text)
                            .map_err(|_| Fault::value("devices.holder", "is not a valid box id"))
                    })?;
                    Ok(DeviceInfo {
                        id,
                        model,
                        size,
                        holder,
                    })
                })?,
                cost: r.option(runtime_cost)?,
            };
            capacity
                .check()
                .map_err(|e| Fault::value("capacity", e.to_string()))?;
            Ok(capacity)
        })?),
        kind::INFO => Answer::Info(info(&mut r)?),
        kind::DONE => Answer::Done,
        kind::STARTED => Answer::Started { pid: r.u32()? },
        kind::OFFERED => Answer::Offered(StreamOffer {
            pid: r.u32()?,
            started: r.u64()?,
            vsock: r.path()?,
            token: r.u64()?,
            exec: r.u64()?,
            streams: r.u8().and_then(Streams::of)?,
        }),
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
        kind::NETNS_ANSWER => {
            let mut counts = netns::Counts {
                made_ahead: r.u64()?,
                taken: r.u64()?,
                made_on_path: r.u64()?,
                discarded: r.u64()?,
                failed: r.u64()?,
                ..netns::Counts::default()
            };
            counts.failed_by = netns::FailedBy {
                home: r.u64()?,
                unshare: r.u64()?,
                open: r.u64()?,
                loopback: r.u64()?,
                back: r.u64()?,
            };
            counts.refused = missing(r.u8()?)?;
            counts.refused_on_path = r.u64()?;
            counts.last_failed = netns::LastFailed {
                home: reason(&mut r)?,
                unshare: reason(&mut r)?,
                open: reason(&mut r)?,
                loopback: reason(&mut r)?,
                back: reason(&mut r)?,
            };
            Answer::Netns(counts)
        }
        kind::GIVEN => {
            let pid = r.u32()?;
            let given = r.u8().and_then(Streams::of)?;
            if msg.descriptors != given.count() {
                return Err(Fault::Descriptors {
                    what: "a given answer",
                    want: given.count(),
                    got: msg.descriptors,
                });
            }
            Answer::Given { pid, given }
        }
        _ => {
            return Err(Fault::Undefined {
                kind,
                expected: "an answer",
            });
        }
    };
    r.all_read(kind)?;
    Ok((Header { kind, id }, answer))
}

/// What every message of this wire is checked for once its header is read: its size and its descriptors' room.
fn check(msg: &Received<'_>, bound: usize) -> Result<(), Fault> {
    if msg.size > bound {
        return Err(Fault::Oversized {
            bytes: msg.size,
            bound,
        });
    }
    if msg.descriptors_cut || msg.descriptors > MAX_FDS {
        return Err(Fault::DescriptorsCut);
    }
    Ok(())
}

/// A step's latest reason for a failed make: an option of its errno and its text, a text past
/// [`netns::MAX_REASON`] bytes refused rather than cut, so what is read is what was written.
fn reason(r: &mut Reader<'_>) -> Result<Option<netns::Reason>, Fault> {
    r.option(|r| {
        let errno = r.option(Reader::i32)?;
        let text = r.string("a failed make's reason")?;
        if text.len() > netns::MAX_REASON {
            return Err(Fault::value(
                "a failed make's reason",
                format!(
                    "{} bytes is over the {}-byte bound",
                    text.len(),
                    netns::MAX_REASON
                ),
            ));
        }
        Ok(netns::Reason::new(errno, &text))
    })
}

/// What a node lacks, from its byte in the network namespace counts: nothing, or what its bits name; a bit the wire
/// doesn't define is refused.
fn missing(bits: u8) -> Result<Option<netns::Missing>, Fault> {
    match bits {
        0 => Ok(None),
        1..=3 => Ok(netns::Missing::of(bits & 1 == 0, bits & 2 == 0)),
        _ => Err(Fault::value(
            "missing capabilities",
            format!("{bits:#04x} names a capability the wire doesn't define"),
        )),
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
        kind::KILL => "a kill",
        kind::EXEC_STREAMS => "an exec asking for streams",
        kind::TAKEN => "a taken",
        kind::RUN => "a run",
        _ => "a request",
    }
}

fn put_call(w: &mut Writer, id: u64, call: &Call) -> Result<(), Fault> {
    match call {
        Call::Capacity(class) => {
            w.header(kind::CAPACITY, id);
            w.u8(class_tag(*class));
            Ok(())
        }
        Call::Schedule(command) => {
            w.header(kind::SCHEDULE, id);
            w.string(
                &serde_json::to_string(command)
                    .map_err(|e| Fault::value("scheduler command", e.to_string()))?,
            )
        }
        Call::Create(spec) => {
            w.header(kind::CREATE, id);
            put_box_spec(w, spec)
        }
        Call::Start(b) => {
            w.header(kind::START, id);
            w.string(b.as_str())
        }
        Call::Exec {
            id: b,
            program,
            streams,
        } => {
            w.header(kind::EXEC, id);
            w.string(b.as_str())?;
            put_program(w, program)?;
            w.u8(streams.bits());
            Ok(())
        }
        Call::Signal { id: b, signal } => {
            w.header(kind::SIGNAL, id);
            w.string(b.as_str())?;
            let n = u8::try_from(signal.number())
                .map_err(|_| Fault::value("signal", format!("{} isn't a byte", signal.number())))?;
            w.u8(n);
            Ok(())
        }
        Call::Inspect(b) => {
            w.header(kind::INSPECT, id);
            w.string(b.as_str())
        }
        Call::Usage(b) => {
            w.header(kind::USAGE, id);
            w.string(b.as_str())
        }
        Call::List => {
            w.header(kind::LIST, id);
            Ok(())
        }
        Call::Delete(b) => {
            w.header(kind::DELETE, id);
            w.string(b.as_str())
        }
        Call::Netns => {
            w.header(kind::NETNS, id);
            Ok(())
        }
        Call::Kill { exec } => {
            w.header(kind::KILL, id);
            w.u64(*exec);
            Ok(())
        }
        Call::ExecStreams {
            id: b,
            program,
            streams,
            ask,
        } => {
            w.header(kind::EXEC_STREAMS, id);
            w.string(b.as_str())?;
            put_program(w, program)?;
            w.u8(streams.bits());
            w.u8(ask.bits());
            Ok(())
        }
        Call::Run {
            spec,
            program,
            streams,
            ask,
        } => {
            w.header(kind::RUN, id);
            put_box_spec(w, spec)?;
            put_program(w, program)?;
            w.u8(streams.bits());
            w.u8(ask.bits());
            Ok(())
        }
    }
}

fn put_box_spec(w: &mut Writer, spec: &BoxSpec) -> Result<(), Fault> {
    if let Some(allocation) = spec.allocation {
        allocation
            .covers(workload_caps(spec))
            .map_err(|e| Fault::value("allocation", e.to_string()))?;
    }
    spec.check_devices()
        .map_err(|e| Fault::value("devices", e.to_string()))?;
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
        ClassSpec::Firecracker(oci) => {
            w.u8(3);
            put_oci(w, oci)?;
        }
        ClassSpec::Shards(oci) => {
            w.u8(4);
            put_oci(w, oci)?;
        }
    }
    w.option_with(spec.owner.as_ref(), |w, owner| {
        w.string(&owner.name)?;
        w.bool(owner.reuse);
        Ok(())
    })?;
    w.device_count(spec.devices.len(), 9)?;
    for id in &spec.devices {
        put_device_id(w, id);
    }
    w.reserve_fixed(
        1 + if spec.allocation.is_some() {
            ALLOCATION_BYTES
        } else {
            0
        },
    )?;
    w.option_with(spec.allocation, |w, allocation| {
        put_resources(w, allocation.resources);
        put_cost(w, allocation.cost)
    })
}

fn box_spec(r: &mut Reader<'_>) -> Result<BoxSpec, Fault> {
    let id = box_id(r)?;
    let bounds = bounds(r)?;
    let class = match r.u8()? {
        0 => ClassSpec::Process,
        1 => ClassSpec::Oci(Box::new(oci(r)?)),
        2 => ClassSpec::Bundle(r.path()?),
        3 => ClassSpec::Firecracker(Box::new(oci(r)?)),
        4 => ClassSpec::Shards(Box::new(oci(r)?)),
        t => return Err(Fault::value("class spec", format!("variant {t}"))),
    };
    let owner = r.option(|r| {
        let name = r.string("owner.name")?;
        let reuse = r.bool("owner.reuse")?;
        Ok(BoxOwner { name, reuse })
    })?;
    let spec = BoxSpec {
        id,
        bounds,
        class,
        owner,
        devices: r.device_list(9, device_id)?,
        allocation: r.option(|r| {
            Ok(Allocation {
                resources: resources(r)?,
                cost: runtime_cost(r)?,
            })
        })?,
    };
    spec.check_devices()
        .map_err(|e| Fault::value("devices", e.to_string()))?;
    if let Some(allocation) = spec.allocation {
        allocation
            .covers(workload_caps(&spec))
            .map_err(|e| Fault::value("allocation", e.to_string()))?;
    }
    Ok(spec)
}

// AC-T7.4.1: fixed native integer widths; new optional fields follow the complete old device tail.
const RESOURCE_BYTES: usize = 3 * size_of::<u64>();
const COST_BYTES: usize = 32 + 5 * RESOURCE_BYTES;
const ALLOCATION_BYTES: usize = RESOURCE_BYTES + COST_BYTES;

fn workload_caps(spec: &BoxSpec) -> CapacityResources {
    CapacityResources {
        cpu_milli: spec.bounds.cpu_max_milli.map_or(0, u64::from),
        mem_bytes: spec.bounds.memory_max.unwrap_or(0),
        disk_bytes: 0,
    }
}

fn put_resources(w: &mut Writer, resources: CapacityResources) {
    w.u64(resources.cpu_milli);
    w.u64(resources.mem_bytes);
    w.u64(resources.disk_bytes);
}

fn resources(r: &mut Reader<'_>) -> Result<CapacityResources, Fault> {
    Ok(CapacityResources {
        cpu_milli: r.u64()?,
        mem_bytes: r.u64()?,
        disk_bytes: r.u64()?,
    })
}

fn put_cost_option(w: &mut Writer, cost: Option<RuntimeCost>) -> Result<(), Fault> {
    w.reserve_fixed(1 + if cost.is_some() { COST_BYTES } else { 0 })?;
    w.option_with(cost, put_cost)
}

fn put_cost(w: &mut Writer, cost: RuntimeCost) -> Result<(), Fault> {
    cost.check()
        .map_err(|e| Fault::value("runtime cost", e.to_string()))?;
    w.reserve_fixed(COST_BYTES)?;
    w.raw(&cost.id);
    for resources in [
        cost.floor,
        cost.ceiling,
        cost.quantum,
        cost.guest_overhead,
        cost.overhead,
    ] {
        put_resources(w, resources);
    }
    Ok(())
}

fn runtime_cost(r: &mut Reader<'_>) -> Result<RuntimeCost, Fault> {
    let cost = RuntimeCost {
        id: r.array::<32>()?,
        floor: resources(r)?,
        ceiling: resources(r)?,
        quantum: resources(r)?,
        guest_overhead: resources(r)?,
        overhead: resources(r)?,
    };
    cost.check()
        .map_err(|e| Fault::value("runtime cost", e.to_string()))?;
    Ok(cost)
}

// Fixed tags independent of Rust discriminants; the catalog order is retained because scheduler
// indices refer to it. DeviceId is one tag plus eight key bytes, DeviceInfo at least 22 bytes
// (identity, model length, size and holder-option tag), before any model or holder bytes.
fn put_device_id(w: &mut Writer, id: &DeviceId) {
    w.u8(match id.kind {
        DeviceKind::Gpu => 0,
        DeviceKind::Vfio => 1,
        DeviceKind::Block => 2,
        DeviceKind::Net => 3,
    });
    w.u64(id.key);
}

fn device_id(r: &mut Reader<'_>) -> Result<DeviceId, Fault> {
    let kind = match r.u8()? {
        0 => DeviceKind::Gpu,
        1 => DeviceKind::Vfio,
        2 => DeviceKind::Block,
        3 => DeviceKind::Net,
        _ => return Err(Fault::value("devices.kind", "unsupported tag")),
    };
    let id = DeviceId {
        kind,
        key: r.u64()?,
    };
    id.check()
        .map_err(|e| Fault::value("devices.id", e.to_string()))?;
    Ok(id)
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

pub(crate) fn put_program(w: &mut Writer, p: &Program) -> Result<(), Fault> {
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

pub(crate) fn program(r: &mut Reader<'_>) -> Result<Program, Fault> {
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

pub(crate) fn box_id(r: &mut Reader<'_>) -> Result<BoxId, Fault> {
    let s = r.string("box id")?;
    BoxId::new(&s).map_err(|e| Fault::value("box id", e.to_string()))
}

fn class_tag(c: Class) -> u8 {
    match c {
        Class::Process => 0,
        Class::Oci => 1,
        Class::Firecracker => 2,
        Class::Shards => 3,
    }
}

fn class(r: &mut Reader<'_>) -> Result<Class, Fault> {
    match r.u8()? {
        0 => Ok(Class::Process),
        1 => Ok(Class::Oci),
        2 => Ok(Class::Firecracker),
        3 => Ok(Class::Shards),
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
        membership: None,
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
        Refusal::Lifecycle { id, cause } => {
            w.u8(9);
            w.string(id.as_str())?;
            w.u8(match cause {
                LifecycleCause::Pending => 0,
                LifecycleCause::Uncertain => 1,
                LifecycleCause::Busy => 2,
            });
            Ok(())
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
        9 => Refusal::Lifecycle {
            id: box_id(r)?,
            cause: match r.u8()? {
                0 => LifecycleCause::Pending,
                1 => LifecycleCause::Uncertain,
                2 => LifecycleCause::Busy,
                t => return Err(Fault::value("lifecycle cause", format!("variant {t}"))),
            },
        },
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

    pub(crate) fn header(&mut self, kind: u16, id: u64) {
        self.out.extend_from_slice(&MAGIC);
        self.u64(IDENTITY);
        self.u16(kind);
        self.u64(id);
    }

    /// The new fixed-width cost/charge tail is checked and reserved before writing any of it.
    fn reserve_fixed(&mut self, bytes: usize) -> Result<(), Fault> {
        let after = self.out.len().checked_add(bytes);
        if after.is_none_or(|after| after > self.bound) {
            return Err(Fault::Oversized {
                bytes: after.unwrap_or(usize::MAX),
                bound: self.bound,
            });
        }
        self.out
            .try_reserve_exact(bytes)
            .map_err(|_| Fault::Storage {
                what: "wire allocation receipt",
                bound: bytes,
            })
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

    fn device_count(&mut self, count: usize, minimum_bytes: usize) -> Result<(), Fault> {
        if count > MAX_DEVICES {
            return Err(Fault::value(
                "devices",
                "count exceeds the u16 device index address space",
            ));
        }
        let after = count
            .checked_mul(minimum_bytes)
            .and_then(|bytes| self.out.len().checked_add(4)?.checked_add(bytes));
        if after.is_none_or(|after| after > self.bound) {
            return Err(Fault::Oversized {
                bytes: after.unwrap_or(usize::MAX),
                bound: self.bound,
            });
        }
        self.u32(u32::try_from(count).map_err(|_| Fault::value("devices", "count exceeds u32"))?);
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

    /// The header: magic first, so a message of something else is refused as that; then its sender's wire identity,
    /// its kind and its id.
    pub(crate) fn header(&mut self) -> Result<(u64, u16, u64), Fault> {
        if self.array::<4>()? != MAGIC {
            return Err(Fault::Magic);
        }
        Ok((self.u64()?, self.u16()?, self.u64()?))
    }

    /// That nothing follows what the node socket's wire defines for the message's kind.
    pub(crate) fn all_read(&self, kind: u16) -> Result<(), Fault> {
        if self.rest.is_empty() {
            Ok(())
        } else {
            Err(Fault::Leftover {
                bytes: self.rest.len(),
                kind,
            })
        }
    }

    /// That nothing follows what the message's version defines, in the other protocols this codec carries.
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

    fn device_string(&mut self, what: &'static str) -> Result<String, Fault> {
        let bytes = self.bytes()?;
        let text = std::str::from_utf8(bytes).map_err(|_| Fault::value(what, "is not UTF-8"))?;
        let mut out = String::new();
        out.try_reserve_exact(text.len())
            .map_err(|_| Fault::value(what, "cannot reserve string"))?;
        out.push_str(text);
        Ok(out)
    }

    fn device_list<T>(
        &mut self,
        minimum_bytes: usize,
        mut get: impl FnMut(&mut Self) -> Result<T, Fault>,
    ) -> Result<Vec<T>, Fault> {
        let count = usize::try_from(self.u32()?)
            .map_err(|_| Fault::value("devices", "count exceeds this platform"))?;
        if count > MAX_DEVICES {
            return Err(Fault::value(
                "devices",
                "count exceeds the u16 device index address space",
            ));
        }
        let bytes = count
            .checked_mul(minimum_bytes)
            .ok_or_else(|| Fault::value("devices", "encoded list size overflows"))?;
        if bytes > self.rest.len() {
            return Err(Fault::Truncated {
                need: self.at().saturating_add(bytes),
                have: self.len,
            });
        }
        let mut out = Vec::new();
        for _ in 0..count {
            // Invalid tags/identities are refused before making room for that item. The count
            // preflight bounds work, but does not allocate a full caller-selected list up front.
            let item = get(self)?;
            out.try_reserve(1)
                .map_err(|_| Fault::value("devices", "cannot reserve list"))?;
            out.push(item);
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
