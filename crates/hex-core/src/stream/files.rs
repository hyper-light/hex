//! Host paths staged into a Shards guest (docs/design/shards-guest.md §4a): the bytes on the guest's
//! [`FILES_PORT`]. The guest can't see the host's folders, so before a create the host sends each path a
//! box names as a stream of entries, which the guest's relay writes under its staged root; and when the
//! box goes, it asks for the copy's removal. Pure: the host's packer and the relay's unpacker move the
//! bytes and touch the file systems; this says what each byte means and refuses, by type, every name or
//! count that a well-behaved packer never sends (CLAUDE.md §2, §3, §9).
//!
//! A connection opens with [`OPEN_BYTES`] bytes: `HEXF`, the version (`u16`, little-endian as the wire's
//! integers), the [`Op`], a zero byte and the box id's length (`u16`), then the id. A [`Op::Remove`]
//! ends there. A [`Op::Stage`] is followed by entries, each a [`HEAD_BYTES`]-byte head (its kind, a
//! zero byte, its mode, its path's length, a symlink's target's length, a file's size), its path, a
//! symlink's target and a file's bytes; an end head (kind 0, every field zero) closes it. The relay
//! answers once, with a framed [`crate::wire`] answer: `Done`, or `Refused` saying why.

use std::fmt;

/// The guest's vsock port the host dials to stage paths or remove them: the channel's next (`HEX` and
/// 2, beside [`super::CONTROL_PORT`] and [`super::STDIO_PORT`]).
pub const FILES_PORT: u32 = 0x4845_5802;

const MAGIC: [u8; 4] = *b"HEXF";
/// The staging stream's only version. It changes append-only, as the wire's.
const VERSION: u16 = 1;
/// A connection's first bytes, before the box id.
pub const OPEN_BYTES: usize = 10;
/// An entry's head, before its path.
pub const HEAD_BYTES: usize = 16;
/// The permission bits an entry's mode may hold: `S_IRWXU | S_IRWXG | S_IRWXO`, with setuid, setgid
/// and sticky (inode(7)). The file's type is its kind, never its mode.
pub const MODE_BITS: u16 = 0o7777;
/// Longest path component: Linux's `NAME_MAX` (limits.h, 255 bytes).
pub const NAME_MAX: usize = 255;

/// What a staging connection asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Stage entries under the box's staged root, which must not exist yet.
    Stage,
    /// Remove the box's staged root and everything under it.
    Remove,
}

impl Op {
    fn tag(self) -> u8 {
        match self {
            Self::Stage => 1,
            Self::Remove => 2,
        }
    }
}

/// What kind of file an entry is. Nothing else is staged: a device node, a FIFO or a socket on the
/// host is refused by name before it is sent ([`Kind::of_tag`] refuses any other on the stream).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Dir,
    File,
    Symlink,
}

impl Kind {
    fn tag(self) -> u8 {
        match self {
            Self::Dir => 1,
            Self::File => 2,
            Self::Symlink => 3,
        }
    }

    fn of_tag(tag: u8) -> Result<Option<Self>, Refused> {
        match tag {
            0 => Ok(None),
            1 => Ok(Some(Self::Dir)),
            2 => Ok(Some(Self::File)),
            3 => Ok(Some(Self::Symlink)),
            other => Err(Refused::Kind(other)),
        }
    }
}

/// One entry to stage: its path under the box's staged root, its mode, and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub mode: u16,
    pub what: What,
}

/// An entry's kind, with what that kind carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum What {
    Dir,
    /// A regular file of `size` bytes, which follow its head.
    File {
        size: u64,
    },
    /// A symbolic link to `target`, kept as its text: resolved in the container, never here.
    Symlink {
        target: String,
    },
}

impl What {
    pub fn kind(&self) -> Kind {
        match self {
            Self::Dir => Kind::Dir,
            Self::File { .. } => Kind::File,
            Self::Symlink { .. } => Kind::Symlink,
        }
    }
}

/// Why a staging stream was refused: each a name or a count a well-behaved packer never sends.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    #[error("it doesn't start with HEXF")]
    Magic,
    #[error("version {got}: this side speaks {VERSION}")]
    Version { got: u16 },
    #[error("op {0} is neither stage (1) nor remove (2)")]
    Op(u8),
    #[error(
        "kind {0} is none of directory (1), file (2) and symbolic link (3): devices, FIFOs and sockets are never staged"
    )]
    Kind(u8),
    #[error("a pad byte is {0}, not 0")]
    Pad(u8),
    #[error("{path:?}'s mode {mode:#o} holds bits other than the permissions ({MODE_BITS:#o})")]
    Mode { path: String, mode: u16 },
    #[error("{what} carries a {field} its kind doesn't have")]
    Field {
        what: &'static str,
        field: &'static str,
    },
    #[error("box id {0:?} isn't one")]
    Id(String),
    #[error("{path:?} can't be staged: {why}")]
    Name { path: String, why: &'static str },
    #[error("symbolic link {path:?}'s target can't be staged: {why}")]
    Target { path: String, why: &'static str },
    #[error("{what} is over its bound of {bound}")]
    Over { what: &'static str, bound: u64 },
    #[error("the stream ended {0}")]
    Truncated(&'static str),
    #[error("{0} bytes follow the stream's end")]
    Trailing(usize),
}

/// A staging stream's bounds (CLAUDE.md §2), each refused past by type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Entries in one stage.
    pub entries: u64,
    /// File bytes in one stage, every file's together.
    pub bytes: u64,
    /// One entry's path, and a symbolic link's target, in bytes.
    pub path: usize,
}

/// The bounds unless the relay is given others:
/// - a path and a link's target of Linux's `PATH_MAX` (limits.h, 4096 bytes, its terminating NUL
///   included, so 4095 of text), the longest any path the guest's kernel takes;
/// - 2^20 entries and 64 GiB: the relay holds one path per directory to set its mode last, so the
///   entries bound its memory; the bytes are what the guest's tmpfs holds, whose own size and inodes
///   (half the guest's memory each by default, tmpfs(5)) refuse the rest as `ENOSPC`, which the relay
///   returns typed. Either is past any image a guest whose memory the node budgets can hold.
pub const LIMITS: Limits = Limits {
    entries: 1 << 20,
    bytes: 64 << 30,
    path: 4095,
};

/// A connection's opening bytes.
pub fn open(op: Op, id: &str) -> Result<Vec<u8>, Refused> {
    let len = u16::try_from(id.len()).map_err(|_| Refused::Id(id.to_owned()))?;
    let mut out = Vec::with_capacity(OPEN_BYTES.saturating_add(id.len()));
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.push(op.tag());
    out.push(0);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(id.as_bytes());
    Ok(out)
}

/// An entry's head, its path and a symlink's target: a file's bytes follow it. Refused as the
/// unpacker would refuse it, so a packer never sends what the guest refuses.
pub fn head(entry: &Entry, limits: &Limits) -> Result<Vec<u8>, Refused> {
    check_path(&entry.path, limits)?;
    check_mode(&entry.path, entry.mode)?;
    let (target, size): (&str, u64) = match &entry.what {
        What::Dir => ("", 0),
        What::File { size } => ("", *size),
        What::Symlink { target } => {
            check_target(&entry.path, target, limits)?;
            (target, 0)
        }
    };
    let over_path = || Refused::Over {
        what: "a path",
        bound: u64::try_from(limits.path).unwrap_or(u64::MAX),
    };
    let path_len = u16::try_from(entry.path.len()).map_err(|_| over_path())?;
    let target_len = u16::try_from(target.len()).map_err(|_| over_path())?;
    let mut out = Vec::with_capacity(
        HEAD_BYTES
            .saturating_add(entry.path.len())
            .saturating_add(target.len()),
    );
    out.push(entry.what.kind().tag());
    out.push(0);
    out.extend_from_slice(&entry.mode.to_le_bytes());
    out.extend_from_slice(&path_len.to_le_bytes());
    out.extend_from_slice(&target_len.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(entry.path.as_bytes());
    out.extend_from_slice(target.as_bytes());
    Ok(out)
}

/// The head that closes a stage.
pub const END: [u8; HEAD_BYTES] = [0; HEAD_BYTES];

fn check_mode(path: &str, mode: u16) -> Result<(), Refused> {
    if mode & !MODE_BITS != 0 {
        return Err(Refused::Mode {
            path: path.to_owned(),
            mode,
        });
    }
    Ok(())
}

/// Whether `path` names a place under the staged root and nowhere else: relative, each component a
/// plain name (not empty, `.` or `..`, at most [`NAME_MAX`] bytes), no NUL, within its bound.
pub fn check_path(path: &str, limits: &Limits) -> Result<(), Refused> {
    let refuse = |why| {
        Err(Refused::Name {
            path: path.to_owned(),
            why,
        })
    };
    if path.len() > limits.path {
        return Err(Refused::Over {
            what: "a path",
            bound: u64::try_from(limits.path).unwrap_or(u64::MAX),
        });
    }
    if path.is_empty() {
        return refuse("it is empty");
    }
    if path.starts_with('/') {
        return refuse("it is absolute");
    }
    if path.contains('\0') {
        return refuse("it holds a NUL");
    }
    for part in path.split('/') {
        match part {
            "" => return refuse("it has an empty component"),
            "." => return refuse("it has a '.' component"),
            ".." => return refuse("it has a '..' component"),
            _ if part.len() > NAME_MAX => return refuse("a component is over NAME_MAX (255)"),
            _ => {}
        }
    }
    Ok(())
}

fn check_target(path: &str, target: &str, limits: &Limits) -> Result<(), Refused> {
    let refuse = |why| {
        Err(Refused::Target {
            path: path.to_owned(),
            why,
        })
    };
    if target.len() > limits.path {
        return Err(Refused::Over {
            what: "a symbolic link's target",
            bound: u64::try_from(limits.path).unwrap_or(u64::MAX),
        });
    }
    if target.is_empty() {
        return refuse("it is empty");
    }
    if target.contains('\0') {
        return refuse("it holds a NUL");
    }
    Ok(())
}

/// What the unpacker read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The connection's op and its box id (a valid one: `hex_api::BoxId`'s rule is the shell's).
    Open { op: Op, id: String },
    /// An entry. A file's bytes come next, as [`Step::Data`], unless its size is 0.
    Entry(Entry),
    /// The bytes just taken are the current file's; `done` once its last is among them.
    Data { done: bool },
    /// The stage's end head: nothing may follow it.
    End,
}

/// Where the unpacker is.
#[derive(Debug)]
enum At {
    Open,
    /// The opening's fixed part read; its id, `len` bytes, comes.
    Id {
        len: usize,
    },
    /// A remove's opening is whole: nothing may follow.
    Closed,
    Head,
    /// A head read; its path and target, `len` bytes together, come.
    Names {
        head: Head,
        len: usize,
    },
    /// A file's bytes, `left` of them to come.
    Data {
        left: u64,
    },
}

#[derive(Debug, Clone, Copy)]
struct Head {
    kind: Kind,
    mode: u16,
    path: usize,
    target: usize,
    size: u64,
}

/// Reads a staging stream as it comes. It holds at most a head and its names, never more than its
/// bounds; a file's bytes pass through.
pub struct Unpack {
    limits: Limits,
    at: At,
    op: Option<Op>,
    held: Vec<u8>,
    entries: u64,
    bytes: u64,
    /// Why the stream can't be read further, once it can't: every later take is refused the same.
    broken: Option<Refused>,
}

impl fmt::Debug for Unpack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Unpack")
            .field("at", &self.at)
            .field("entries", &self.entries)
            .field("bytes", &self.bytes)
            .finish_non_exhaustive()
    }
}

impl Unpack {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            at: At::Open,
            op: None,
            held: Vec::new(),
            entries: 0,
            bytes: 0,
            broken: None,
        }
    }

    /// Takes bytes from `input`, at most to the end of what is under way, and returns how many it took
    /// and what they made, once whole. The caller takes again with what is left. A [`Step::Data`]
    /// means the bytes just taken, `input[..taken]`, are the current file's.
    pub fn take(&mut self, input: &[u8]) -> Result<(usize, Option<Step>), Refused> {
        if let Some(why) = &self.broken {
            return Err(why.clone());
        }
        match self.step(input) {
            Ok(taken) => Ok(taken),
            Err(why) => {
                self.broken = Some(why.clone());
                Err(why)
            }
        }
    }

    /// Whether the stream may end here: a remove's opening, or a stage's end head, is whole. Else its
    /// end is refused as [`Refused::Truncated`], saying where.
    pub fn finish(&self) -> Result<(), Refused> {
        if let Some(why) = &self.broken {
            return Err(why.clone());
        }
        match self.at {
            At::Closed => Ok(()),
            At::Open | At::Id { .. } => Err(Refused::Truncated("before its opening was whole")),
            At::Head | At::Names { .. } => Err(Refused::Truncated("before its end head")),
            At::Data { .. } => Err(Refused::Truncated("in the middle of a file")),
        }
    }

    /// Fills `held` to `want` bytes from `input`: how many it took, and whether `held` is whole.
    fn fill(&mut self, input: &[u8], want: usize) -> (usize, bool) {
        let n = want.saturating_sub(self.held.len()).min(input.len());
        self.held
            .extend_from_slice(input.get(..n).unwrap_or_default());
        (n, self.held.len() >= want)
    }

    fn step(&mut self, input: &[u8]) -> Result<(usize, Option<Step>), Refused> {
        if input.is_empty() {
            return Ok((0, None));
        }
        match self.at {
            At::Closed => Err(Refused::Trailing(input.len())),
            At::Open => {
                let (n, whole) = self.fill(input, OPEN_BYTES);
                if whole {
                    self.opened()?;
                }
                Ok((n, None))
            }
            At::Id { len } => {
                let (n, whole) = self.fill(input, len);
                if !whole {
                    return Ok((n, None));
                }
                let id = String::from_utf8(std::mem::take(&mut self.held))
                    .map_err(|e| Refused::Id(String::from_utf8_lossy(e.as_bytes()).into_owned()))?;
                let op = self.op.ok_or(Refused::Truncated("before its op"))?;
                self.at = match op {
                    Op::Stage => At::Head,
                    Op::Remove => At::Closed,
                };
                Ok((n, Some(Step::Open { op, id })))
            }
            At::Head => {
                let (n, whole) = self.fill(input, HEAD_BYTES);
                if !whole {
                    return Ok((n, None));
                }
                Ok((n, self.headed()?))
            }
            At::Names { head, len } => {
                let (n, whole) = self.fill(input, len);
                if !whole {
                    return Ok((n, None));
                }
                Ok((n, Some(self.named(head)?)))
            }
            At::Data { left } => {
                let n = usize::try_from(left).unwrap_or(usize::MAX).min(input.len());
                let left = left
                    .checked_sub(u64::try_from(n).unwrap_or(u64::MAX))
                    .ok_or(Refused::Truncated("in the middle of a file"))?;
                let done = left == 0;
                self.at = if done { At::Head } else { At::Data { left } };
                Ok((n, Some(Step::Data { done })))
            }
        }
    }

    fn opened(&mut self) -> Result<(), Refused> {
        let held = std::mem::take(&mut self.held);
        let [m0, m1, m2, m3, v0, v1, op, pad, l0, l1] = held
            .get(..OPEN_BYTES)
            .and_then(|b| <[u8; OPEN_BYTES]>::try_from(b).ok())
            .ok_or(Refused::Truncated("before its opening was whole"))?;
        if [m0, m1, m2, m3] != MAGIC {
            return Err(Refused::Magic);
        }
        let version = u16::from_le_bytes([v0, v1]);
        if version != VERSION {
            return Err(Refused::Version { got: version });
        }
        self.op = Some(match op {
            1 => Op::Stage,
            2 => Op::Remove,
            other => return Err(Refused::Op(other)),
        });
        if pad != 0 {
            return Err(Refused::Pad(pad));
        }
        let len = usize::from(u16::from_le_bytes([l0, l1]));
        // A box id is at most `hex_api::BoxId::MAX_LEN` (128) bytes, and never empty.
        if len == 0 || len > 128 {
            return Err(Refused::Id(format!("of {len} bytes")));
        }
        self.at = At::Id { len };
        Ok(())
    }

    fn headed(&mut self) -> Result<Option<Step>, Refused> {
        let held = std::mem::take(&mut self.held);
        let [k, pad, m0, m1, p0, p1, t0, t1, size @ ..] = held
            .get(..HEAD_BYTES)
            .and_then(|b| <[u8; HEAD_BYTES]>::try_from(b).ok())
            .ok_or(Refused::Truncated("in the middle of a head"))?;
        let Some(kind) = Kind::of_tag(k)? else {
            if held.iter().any(|b| *b != 0) {
                return Err(Refused::Field {
                    what: "the end head",
                    field: "non-zero field",
                });
            }
            self.at = At::Closed;
            return Ok(Some(Step::End));
        };
        if pad != 0 {
            return Err(Refused::Pad(pad));
        }
        let head = Head {
            kind,
            mode: u16::from_le_bytes([m0, m1]),
            path: usize::from(u16::from_le_bytes([p0, p1])),
            target: usize::from(u16::from_le_bytes([t0, t1])),
            size: u64::from_le_bytes(size),
        };
        let path_bound = u64::try_from(self.limits.path).unwrap_or(u64::MAX);
        if head.path > self.limits.path {
            return Err(Refused::Over {
                what: "a path",
                bound: path_bound,
            });
        }
        if head.target > self.limits.path {
            return Err(Refused::Over {
                what: "a symbolic link's target",
                bound: path_bound,
            });
        }
        match kind {
            Kind::Dir | Kind::File if head.target != 0 => {
                return Err(Refused::Field {
                    what: "a directory or file",
                    field: "target",
                });
            }
            Kind::Dir | Kind::Symlink if head.size != 0 => {
                return Err(Refused::Field {
                    what: "a directory or symbolic link",
                    field: "size",
                });
            }
            _ => {}
        }
        self.entries = self
            .entries
            .checked_add(1)
            .filter(|n| *n <= self.limits.entries)
            .ok_or(Refused::Over {
                what: "the stage's entries",
                bound: self.limits.entries,
            })?;
        self.bytes = self
            .bytes
            .checked_add(head.size)
            .filter(|n| *n <= self.limits.bytes)
            .ok_or(Refused::Over {
                what: "the stage's file bytes",
                bound: self.limits.bytes,
            })?;
        let len = head.path.saturating_add(head.target);
        if len == 0 {
            // No names to wait for: the entry is whole now, and refused or taken as it is.
            return self.named(head).map(Some);
        }
        self.at = At::Names { head, len };
        Ok(None)
    }

    fn named(&mut self, head: Head) -> Result<Step, Refused> {
        let held = std::mem::take(&mut self.held);
        let (path, target) = held
            .split_at_checked(head.path)
            .ok_or(Refused::Truncated("in the middle of an entry's path"))?;
        let path = std::str::from_utf8(path)
            .map_err(|_| Refused::Name {
                path: String::from_utf8_lossy(path).into_owned(),
                why: "it isn't UTF-8",
            })?
            .to_owned();
        check_path(&path, &self.limits)?;
        check_mode(&path, head.mode)?;
        let what = match head.kind {
            Kind::Dir => What::Dir,
            Kind::File => What::File { size: head.size },
            Kind::Symlink => {
                let target = std::str::from_utf8(target)
                    .map_err(|_| Refused::Target {
                        path: path.clone(),
                        why: "it isn't UTF-8",
                    })?
                    .to_owned();
                check_target(&path, &target, &self.limits)?;
                What::Symlink { target }
            }
        };
        self.at = match what {
            What::File { size } if size > 0 => At::Data { left: size },
            _ => At::Head,
        };
        Ok(Step::Entry(Entry {
            path,
            mode: head.mode,
            what,
        }))
    }
}

#[cfg(test)]
mod tests;
