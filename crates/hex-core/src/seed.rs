//! The node agent's seeds as bytes (decisions 3 and 5 of docs/design/runtime-architecture.md, goal 3
//! plan step 2 and goal 5 plan step 4): what the agent asks a seed (`hex __oci-seed`) to run in a
//! container or to make one, and what the seed answers, as messages of the `SOCK_SEQPACKET` socketpair
//! between them. Pure: the agent's seed pools (`hex-node`) and the seed (`hex-runtime`) move the
//! messages and the descriptors that ride with them; this decides what each says, and refuses by type
//! every one that says nothing valid.
//!
//! A message is one `SOCK_SEQPACKET` record: an 8-byte header, then the body its kind defines,
//! encoded as the socket's messages are ([`crate::wire`]: little-endian integers, a string as a `u32`
//! length and its bytes, an option as a byte before the value, a list as a `u32` count before its
//! items).
//!
//! | bytes | field |
//! |---|---|
//! | 0..4 | `HEXS` |
//! | 4..6 | version |
//! | 6..8 | kind: a request below `0x8000`, an answer from it |
//!
//! An exec [`Request`] carries [`REQUEST_FDS`] descriptors: the program's stdin, stdout and stderr, then
//! the container init's pidfd, by which the program joins the init's namespaces (`setns(2)` with a pidfd
//! joins several at once, and names that process, never a later one given its pid). The container's
//! cgroup isn't a descriptor: the request names its path, which the seed opens. A [`Create`] carries
//! [`CREATE_FDS`]: the write end of the pipe on which the container's init tells the agent it is
//! ready, and the container's cgroup directory, which its init is born in; then, when it says so, the
//! network namespace made ahead that the init joins. [`Answer::Started`] carries the pidfd of the
//! process the seed started, the program or the container's init, which the agent's supervisor
//! watches; [`Answer::Refused`] carries none. A node spawns its seeds from the `hex` binary it is
//! given (the bench names one), so the two sides can be of different builds: a reader refuses another
//! version's message, or a kind it doesn't know, by type, and the format changes append-only
//! (CLAUDE.md), as the socket's does: the create is a kind added to version 1, whose exec bytes are as
//! they were.

use crate::wire::{Fault, Reader, Writer};

/// Every message's first bytes.
pub const MAGIC: [u8; 4] = *b"HEXS";
/// The version this side writes and reads.
pub const VERSION: u16 = 1;
/// The header's length: magic, version and kind.
pub const HEADER_BYTES: usize = 8;
/// Largest request. It sits under the kernel's bound on one `SOCK_SEQPACKET` record (212,960 bytes
/// under the default send buffer; [`crate::wire::MAX_MESSAGE`] says where that was measured) and above
/// the largest request a socket client's exec makes, its program and environment under
/// [`crate::wire::MAX_MESSAGE`]. A larger one is refused by type before it is sent.
pub const MAX_REQUEST: usize = 160 << 10;
/// Largest answer. A refusal's reason past [`MAX_WHY`] bytes is cut, so every answer fits.
pub const MAX_ANSWER: usize = 4 << 10;
/// Longest refusal reason an answer carries; the rest is cut, at a character's boundary.
pub const MAX_WHY: usize = 2 << 10;
/// Descriptors an exec request carries: stdin, stdout, stderr, the container init's pidfd.
pub const REQUEST_FDS: usize = 4;
/// Descriptors a create carries before the network namespace made ahead, when there is one: the write
/// end of the init's sync pipe and the container's cgroup directory.
pub const CREATE_FDS: usize = 2;
/// Most descriptors any request carries: what a seed's read makes room for, so one carrying more is
/// cut and refused.
pub const MAX_REQUEST_FDS: usize = REQUEST_FDS;
/// Descriptors a started answer carries: the pidfd of the process the seed started.
pub const STARTED_FDS: usize = 1;

/// The kinds of version 1.
mod kind {
    pub const EXEC: u16 = 0x0001;
    pub const CREATE: u16 = 0x0002;
    pub const STARTED: u16 = 0x8001;
    pub const REFUSED: u16 = 0x8002;
}

/// What to run in a container and where: the init whose namespaces the program joins, the cgroup it is
/// born in, and the program with the exec's overrides of the container's process (its arguments, and
/// optionally its working directory, environment entries and user). The intermediate reads the rest
/// (the capabilities, rlimits, seccomp filter and the rest of the environment) from the bundle's
/// `config.json`, as `hex __oci-exec` does, so the wire carries only what the exec names.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Request {
    /// The container init's pid, for what the seed reports; its namespaces are joined by its pidfd.
    pub init_pid: i32,
    /// The container's bundle, whose `config.json` gives the process the program becomes.
    pub bundle: String,
    /// The container's cgroup directory, which the intermediate opens; the program is born in it.
    /// `None` for a container with no cgroup of its own.
    pub cgroup: Option<String>,
    /// The program and its arguments; the first is the program.
    pub args: Vec<String>,
    /// Overrides the process's working directory.
    pub cwd: Option<String>,
    /// `K=V` entries, each overriding the process's entry for `K` or adding one.
    pub env: Vec<String>,
    /// Overrides the process's user: a uid, and a gid (the process's own when absent).
    pub user: Option<(u32, Option<u32>)>,
}

/// A container for the seed to make (decision 5): its init cloned into its cgroup and the namespaces its
/// spec asks for, set up to the exec FIFO. The seed reads the spec from the bundle's `config.json`, as
/// the shim did, and opens the cgroup the create names, as `hex __oci-exec` opens its container's; the
/// descriptors that ride with it ([`CREATE_FDS`]) are the init's sync pipe (the write end) and the map
/// pipe (the read end), and, when `network` says so, the network namespace made ahead the init joins.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Create {
    /// The container's bundle, whose `config.json` gives its namespaces and the init's set-up.
    pub bundle: String,
    /// The container's state directory, which holds the exec FIFO its init waits on.
    pub dir: String,
    /// The container's cgroup directory, which the seed opens and the init is born in; `None` for a
    /// container with no cgroup of its own (the seam always gives one).
    pub cgroup: Option<String>,
    /// Whether a network namespace made ahead rides with it, the last descriptor, for the init to join
    /// in place of making its own (decision 4).
    pub network: bool,
}

impl Create {
    /// How many descriptors this create carries: the sync pipe's write end and the map pipe's read end,
    /// then a network namespace made ahead when there is one.
    pub fn fds(&self) -> usize {
        CREATE_FDS.saturating_add(usize::from(self.network))
    }
}

/// A request a seed reads: an exec into a container, or a container to make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Exec(Request),
    Create(Create),
}

/// What failed when a seed refuses, so the agent reports a typed refusal and knows whether to replace
/// the seed. The detail is in the answer's reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failed {
    /// The request wasn't one the seed could read: its bytes or its descriptors.
    Request,
    /// Joining the container (its namespaces or its cgroup), or cloning the program into it.
    Start,
    /// The seed's own pipes, sockets or fork: the seed is broken and is replaced.
    Seed,
    /// The seed can't make this container in one clone here: this kernel or sandbox has no `clone3`
    /// into a cgroup. Nothing was made, and the seed is whole; the agent makes it as the command line
    /// does.
    Unable,
}

impl Failed {
    fn code(self) -> u16 {
        match self {
            Self::Request => 1,
            Self::Start => 2,
            Self::Seed => 3,
            Self::Unable => 4,
        }
    }

    fn of(code: u16) -> Result<Self, Fault> {
        Ok(match code {
            1 => Self::Request,
            2 => Self::Start,
            3 => Self::Seed,
            4 => Self::Unable,
            n => {
                return Err(Fault::value(
                    "refusal",
                    format!("code {n} isn't one version 1 defines"),
                ));
            }
        })
    }
}

/// A seed's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The program runs: its pid (the descriptor is its pidfd).
    Started { pid: u32 },
    /// Nothing was started.
    Refused { failed: Failed, why: String },
}

impl Answer {
    /// How many descriptors this answer carries.
    pub fn fds(&self) -> usize {
        match self {
            Self::Started { .. } => STARTED_FDS,
            Self::Refused { .. } => 0,
        }
    }
}

/// An exec request as a message.
pub fn request(req: &Request) -> Result<Vec<u8>, Fault> {
    let mut w = Writer::new(MAX_REQUEST);
    header(&mut w, kind::EXEC);
    w.i32(req.init_pid);
    w.string(&req.bundle)?;
    w.option_with(req.cgroup.as_deref(), |w, c| w.string(c))?;
    w.list(&req.args, |w, a| w.string(a))?;
    w.option_with(req.cwd.as_deref(), |w, c| w.string(c))?;
    w.list(&req.env, |w, e| w.string(e))?;
    w.option(req.user, |w, (uid, gid)| {
        w.u32(uid);
        w.option(gid, Writer::u32);
    });
    w.done()
}

/// A message's exec request. `size` is its whole size as the socket gave it, larger than `bytes`
/// when the message was cut to the reader's buffer.
pub fn read_request(bytes: &[u8], size: usize) -> Result<Request, Fault> {
    if size > MAX_REQUEST {
        return Err(Fault::Oversized {
            bytes: size,
            bound: MAX_REQUEST,
        });
    }
    let mut r = Reader::new(bytes);
    let (version, kind) = read_header(&mut r)?;
    if kind != kind::EXEC {
        return Err(Fault::Kind {
            kind,
            version,
            expected: "an exec request",
        });
    }
    let req = Request {
        init_pid: r.i32()?,
        bundle: r.string("bundle")?,
        cgroup: r.option(|r| r.string("cgroup"))?,
        args: r.list(|r| r.string("args"))?,
        cwd: r.option(|r| r.string("cwd"))?,
        env: r.list(|r| r.string("env"))?,
        user: r.option(|r| Ok((r.u32()?, r.option(Reader::u32)?)))?,
    };
    r.end(kind::EXEC, version)?;
    if req.args.is_empty() {
        return Err(Fault::value("args", "an exec needs a program"));
    }
    Ok(req)
}

/// A create as a message.
pub fn create(create: &Create) -> Result<Vec<u8>, Fault> {
    let mut w = Writer::new(MAX_REQUEST);
    header(&mut w, kind::CREATE);
    w.string(&create.bundle)?;
    w.string(&create.dir)?;
    w.option_with(create.cgroup.as_deref(), |w, c| w.string(c))?;
    w.bool(create.network);
    w.done()
}

/// A message's create. `size` is its whole size as the socket gave it, as [`read_request`] takes it.
pub fn read_create(bytes: &[u8], size: usize) -> Result<Create, Fault> {
    if size > MAX_REQUEST {
        return Err(Fault::Oversized {
            bytes: size,
            bound: MAX_REQUEST,
        });
    }
    let mut r = Reader::new(bytes);
    let (version, kind) = read_header(&mut r)?;
    if kind != kind::CREATE {
        return Err(Fault::Kind {
            kind,
            version,
            expected: "a create",
        });
    }
    let create = Create {
        bundle: r.string("bundle")?,
        dir: r.string("dir")?,
        cgroup: r.option(|r| r.string("cgroup"))?,
        network: r.bool("network")?,
    };
    r.end(kind::CREATE, version)?;
    if create.bundle.is_empty() || create.dir.is_empty() {
        return Err(Fault::value(
            "create",
            "a create names its bundle and its state directory",
        ));
    }
    Ok(create)
}

/// A request a seed reads, of either kind, which came with `fds` descriptors: as many as its kind and
/// body say it carries, or it's refused.
pub fn read_message(bytes: &[u8], size: usize, fds: usize) -> Result<Message, Fault> {
    let mut r = Reader::new(bytes);
    let (version, kind) = read_header(&mut r)?;
    let (message, want) = match kind {
        kind::EXEC => (Message::Exec(read_request(bytes, size)?), REQUEST_FDS),
        kind::CREATE => {
            let create = read_create(bytes, size)?;
            let want = create.fds();
            (Message::Create(create), want)
        }
        _ => {
            return Err(Fault::Kind {
                kind,
                version,
                expected: "an exec or a create",
            });
        }
    };
    if fds != want {
        return Err(Fault::Descriptors {
            what: "this request",
            want,
            got: fds,
        });
    }
    Ok(message)
}

/// An answer as a message. A refusal's reason past [`MAX_WHY`] bytes is cut, so it always fits.
pub fn answer(answer: &Answer) -> Result<Vec<u8>, Fault> {
    let mut w = Writer::new(MAX_ANSWER);
    match answer {
        Answer::Started { pid } => {
            header(&mut w, kind::STARTED);
            w.u32(*pid);
        }
        Answer::Refused { failed, why } => {
            header(&mut w, kind::REFUSED);
            w.u16(failed.code());
            w.string(cut(why, MAX_WHY))?;
        }
    }
    w.done()
}

/// A message's answer, which came with `fds` descriptors: as many as it says it carries, or it's
/// refused.
pub fn read_answer(bytes: &[u8], size: usize, fds: usize) -> Result<Answer, Fault> {
    if size > MAX_ANSWER {
        return Err(Fault::Oversized {
            bytes: size,
            bound: MAX_ANSWER,
        });
    }
    let mut r = Reader::new(bytes);
    let (version, kind) = read_header(&mut r)?;
    let answer = match kind {
        kind::STARTED => Answer::Started { pid: r.u32()? },
        kind::REFUSED => Answer::Refused {
            failed: Failed::of(r.u16()?)?,
            why: r.string("why")?,
        },
        _ => {
            return Err(Fault::Kind {
                kind,
                version,
                expected: "an answer",
            });
        }
    };
    r.end(kind, version)?;
    if answer.fds() != fds {
        return Err(Fault::Descriptors {
            what: "this answer",
            want: answer.fds(),
            got: fds,
        });
    }
    Ok(answer)
}

fn header(w: &mut Writer, kind: u16) {
    w.raw(&MAGIC);
    w.u16(VERSION);
    w.u16(kind);
}

/// A message's version and kind: magic first, so a message of something else is refused as that.
fn read_header(r: &mut Reader<'_>) -> Result<(u16, u16), Fault> {
    if r.array::<4>()? != MAGIC {
        return Err(Fault::value("message", "it doesn't start with HEXS"));
    }
    let version = r.u16()?;
    if version != VERSION {
        return Err(Fault::Version {
            got: version,
            speaks: VERSION,
        });
    }
    Ok((version, r.u16()?))
}

/// `s` cut to at most `max` bytes, at a character's boundary.
fn cut(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    s.get(..end).unwrap_or_default()
}

#[cfg(test)]
mod tests;
