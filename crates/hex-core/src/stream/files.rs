//! Host paths staged into a Shards guest (docs/design/shards-guest.md §4a): the bytes on the guest's
//! [`FILES_PORT`]. The guest can't see the host's folders, so before a create the host sends each path a
//! box names as a stream of entries, which the guest's relay writes under its staged root; and when the
//! box goes, it asks for the copy's removal. Pure: the host's packer and the relay's unpacker move the
//! bytes and touch the file systems; this says what each byte means and refuses, by type, every name or
//! count that a well-behaved packer never sends (CLAUDE.md §2, §3, §9).
//!
//! A connection opens with [`OPEN_BYTES`] bytes: `HEXF`, the version (`u16`, little-endian as the wire's
//! integers), the [`Op`], a zero byte and the box id's length (`u16`), then the id. A [`Op::Remove`]
//! ends there. A [`Op::Stage`] is followed by entries, each a [`HEAD_BYTES`]-byte head (its kind, its
//! flags (version 1: a zero byte), its mode, its path's length, a link's target's length, a file's size),
//! its path, a link's target, in version 2 its [`Meta`] when its flags say so ([`META_BYTES`] then its
//! xattrs), and a file's bytes; an end head (kind 0, every field zero) closes it. The relay answers once,
//! with a framed [`crate::wire`] answer: `Done`, or `Refused` saying why.
//!
//! Version 2 (docs/design/shards-guest.md §4a) adds, append-only, an entry's owner, mtime and extended
//! attributes, and hard links to an earlier entry, what Linux's own unpack of a tarball root holds.

use std::fmt;

/// The guest's vsock port the host dials to stage paths or remove them: the channel's next (`HEX` and
/// 2, beside [`super::CONTROL_PORT`] and [`super::STDIO_PORT`]).
pub const FILES_PORT: u32 = 0x4845_5802;

const MAGIC: [u8; 4] = *b"HEXF";
/// The staging stream's newest version, which this side writes; it reads every one from [`FIRST`]. It
/// changes append-only, as the wire's.
const VERSION: u16 = 2;
/// The first version: a path, a mode and a link text an entry.
const FIRST: u16 = 1;
/// A version-2 head's flag: a [`Meta`] follows the entry's names.
const HAS_META: u8 = 1;
/// A [`Meta`]'s fixed part: uid, gid (`u32`), mtime seconds (`i64`) and nanoseconds (`u32`), the xattr
/// count (`u16`), a zero `u16`, and the xattrs' bytes (`u32`), little-endian.
pub const META_BYTES: usize = 28;
/// The most extended attributes an entry carries: Linux bounds them by bytes (below), not count; 64 is
/// past any image's (one, in §9's census of six) and bounds the parse.
pub const XATTRS_MAX: usize = 64;
/// Linux's `XATTR_NAME_MAX` (include/uapi/linux/limits.h): a name's longest, in bytes.
pub const XATTR_NAME_MAX: usize = 255;
/// Linux's `XATTR_SIZE_MAX`: a value's longest, and Hex's bound on an entry's values together, as
/// Linux's `XATTR_LIST_MAX` bounds its names together (both 65,536 bytes).
pub const XATTR_SIZE_MAX: usize = 65_536;
/// The most bytes an entry's xattrs take on the stream: each a name length (`u8`), a value length
/// (`u32`), its name and its value, within the bounds above.
const XATTR_BLOCK_MAX: usize = XATTRS_MAX * (5 + XATTR_NAME_MAX) + XATTR_SIZE_MAX;
/// The extended-attribute namespaces Linux holds (xattr(7)): an attribute in any other can't exist on a
/// Linux file, so a packer leaves it out (§4a) and the stream refuses it.
pub const XATTR_NAMESPACES: [&str; 4] = ["security.", "system.", "trusted.", "user."];
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
    /// Version 2: another name for an earlier regular file of the same stage.
    Hardlink,
}

impl Kind {
    fn tag(self) -> u8 {
        match self {
            Self::Dir => 1,
            Self::File => 2,
            Self::Symlink => 3,
            Self::Hardlink => 4,
        }
    }

    fn of_tag(tag: u8, version: u16) -> Result<Option<Self>, Refused> {
        match tag {
            0 => Ok(None),
            1 => Ok(Some(Self::Dir)),
            2 => Ok(Some(Self::File)),
            3 => Ok(Some(Self::Symlink)),
            4 if version >= 2 => Ok(Some(Self::Hardlink)),
            other => Err(Refused::Kind(other)),
        }
    }
}

/// One entry to stage: its path under the box's staged root, its mode, what it is, and (version 2, from a
/// tarball root) its owner, mtime and extended attributes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub mode: u16,
    pub what: What,
    pub meta: Option<Meta>,
}

/// What Linux's unpack of a tarball holds of an entry beyond its mode (§4a).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    pub uid: u32,
    pub gid: u32,
    pub mtime: Mtime,
    pub xattrs: Vec<Xattr>,
}

/// A time, as utimensat(2) takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mtime {
    pub secs: i64,
    pub nanos: u32,
}

/// An extended attribute: its full name (`security.capability`) and its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Xattr {
    pub name: String,
    pub value: Vec<u8>,
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
    /// Another name for `target`, an earlier regular file of the same stage, named as a staged path is.
    Hardlink {
        target: String,
    },
}

impl What {
    pub fn kind(&self) -> Kind {
        match self {
            Self::Dir => Kind::Dir,
            Self::File { .. } => Kind::File,
            Self::Symlink { .. } => Kind::Symlink,
            Self::Hardlink { .. } => Kind::Hardlink,
        }
    }
}

/// Why a staging stream was refused: each a name or a count a well-behaved packer never sends.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    #[error("it doesn't start with HEXF")]
    Magic,
    #[error("version {got}: this side speaks {FIRST} to {VERSION}")]
    Version { got: u16 },
    #[error("op {0} is neither stage (1) nor remove (2)")]
    Op(u8),
    #[error(
        "kind {0} is none of directory (1), file (2), symbolic link (3) and, from version 2, hard link (4): devices, FIFOs and sockets are never staged"
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
    #[error("link {path:?}'s target can't be staged: {why}")]
    Target { path: String, why: &'static str },
    #[error("{path:?}'s owner, time or extended attributes can't be staged: {why}")]
    Meta { path: String, why: String },
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
        What::Hardlink { target } => {
            check_link(&entry.path, target, limits)?;
            (target, 0)
        }
    };
    let meta = match &entry.meta {
        Some(meta) => {
            check_meta(&entry.path, meta)?;
            Some(meta_bytes(meta))
        }
        None => None,
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
    out.push(if meta.is_some() { HAS_META } else { 0 });
    out.extend_from_slice(&entry.mode.to_le_bytes());
    out.extend_from_slice(&path_len.to_le_bytes());
    out.extend_from_slice(&target_len.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(entry.path.as_bytes());
    out.extend_from_slice(target.as_bytes());
    if let Some(meta) = meta {
        out.extend_from_slice(&meta);
    }
    Ok(out)
}

/// A checked [`Meta`] as the stream carries it: its fixed part, then each xattr's name length, value
/// length, name and value.
fn meta_bytes(meta: &Meta) -> Vec<u8> {
    let mut xattrs = Vec::new();
    for x in &meta.xattrs {
        xattrs.push(u8::try_from(x.name.len()).unwrap_or(u8::MAX));
        xattrs.extend_from_slice(
            &u32::try_from(x.value.len())
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        xattrs.extend_from_slice(x.name.as_bytes());
        xattrs.extend_from_slice(&x.value);
    }
    let mut out = Vec::with_capacity(META_BYTES.saturating_add(xattrs.len()));
    out.extend_from_slice(&meta.uid.to_le_bytes());
    out.extend_from_slice(&meta.gid.to_le_bytes());
    out.extend_from_slice(&meta.mtime.secs.to_le_bytes());
    out.extend_from_slice(&meta.mtime.nanos.to_le_bytes());
    out.extend_from_slice(
        &u16::try_from(meta.xattrs.len())
            .unwrap_or(u16::MAX)
            .to_le_bytes(),
    );
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(
        &u32::try_from(xattrs.len())
            .unwrap_or(u32::MAX)
            .to_le_bytes(),
    );
    out.extend_from_slice(&xattrs);
    out
}

/// Whether a [`Meta`] can be staged: real ids (`u32::MAX` means "unchanged" to chown(2)), a time's
/// nanoseconds under a second, and its xattrs within Linux's bounds and namespaces.
pub fn check_meta(path: &str, meta: &Meta) -> Result<(), Refused> {
    let refuse = |why: String| {
        Err(Refused::Meta {
            path: path.to_owned(),
            why,
        })
    };
    if meta.uid == u32::MAX || meta.gid == u32::MAX {
        return refuse(format!("an id of {} means no id to chown(2)", u32::MAX));
    }
    if meta.mtime.nanos >= 1_000_000_000 {
        return refuse(format!(
            "its mtime's nanoseconds, {}, are a second or more",
            meta.mtime.nanos
        ));
    }
    if meta.xattrs.len() > XATTRS_MAX {
        return refuse(format!(
            "{} extended attributes are over the {XATTRS_MAX} an entry carries",
            meta.xattrs.len()
        ));
    }
    let mut values: usize = 0;
    for x in &meta.xattrs {
        if x.name.is_empty() || x.name.len() > XATTR_NAME_MAX || x.name.contains('\0') {
            return refuse(format!(
                "extended attribute name {:?} is empty, over {XATTR_NAME_MAX} bytes or holds a NUL",
                x.name
            ));
        }
        if !XATTR_NAMESPACES.iter().any(|ns| x.name.starts_with(ns)) {
            return refuse(format!(
                "extended attribute {:?} is in no namespace Linux holds",
                x.name
            ));
        }
        if x.value.len() > XATTR_SIZE_MAX {
            return refuse(format!(
                "extended attribute {:?}'s value is over {XATTR_SIZE_MAX} bytes",
                x.name
            ));
        }
        values = values.saturating_add(x.value.len());
    }
    if values > XATTR_SIZE_MAX {
        return refuse(format!(
            "its extended attributes' values together are over {XATTR_SIZE_MAX} bytes"
        ));
    }
    Ok(())
}

/// Whether a hard link's `target` names another staged entry: a staged path, not the link itself.
fn check_link(path: &str, target: &str, limits: &Limits) -> Result<(), Refused> {
    check_path(target, limits).map_err(|_| Refused::Target {
        path: path.to_owned(),
        why: "it names no staged path",
    })?;
    if target == path {
        return Err(Refused::Target {
            path: path.to_owned(),
            why: "it names itself",
        });
    }
    Ok(())
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
    /// An entry's names read; its [`Meta`]'s fixed part comes.
    Meta,
    /// Its fixed part read; its xattrs, `len` bytes, `count` of them, come.
    Xattrs {
        count: usize,
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
    meta: bool,
}

/// Reads a staging stream as it comes. It holds at most a head and its names, never more than its
/// bounds; a file's bytes pass through.
pub struct Unpack {
    limits: Limits,
    at: At,
    op: Option<Op>,
    /// The stream's version, once its opening is read.
    version: u16,
    /// An entry whose [`Meta`] is under way: its names read, its owner and xattrs to come.
    pending: Option<Entry>,
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
            version: VERSION,
            pending: None,
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
            At::Meta | At::Xattrs { .. } => {
                Err(Refused::Truncated("in the middle of an entry's metadata"))
            }
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
                Ok((n, self.named(head)?))
            }
            At::Meta => {
                let (n, whole) = self.fill(input, META_BYTES);
                if !whole {
                    return Ok((n, None));
                }
                Ok((n, self.meta_fixed()?))
            }
            At::Xattrs { count, len } => {
                let (n, whole) = self.fill(input, len);
                if !whole {
                    return Ok((n, None));
                }
                let xattrs = self.xattrs(count)?;
                Ok((n, Some(self.whole(xattrs)?)))
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
        if !(FIRST..=VERSION).contains(&version) {
            return Err(Refused::Version { got: version });
        }
        self.version = version;
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
        let Some(kind) = Kind::of_tag(k, self.version)? else {
            if held.iter().any(|b| *b != 0) {
                return Err(Refused::Field {
                    what: "the end head",
                    field: "non-zero field",
                });
            }
            self.at = At::Closed;
            return Ok(Some(Step::End));
        };
        // Version 1's pad byte is version 2's flags, of which [`HAS_META`] alone is defined.
        let allowed = if self.version >= 2 { HAS_META } else { 0 };
        if pad & !allowed != 0 {
            return Err(Refused::Pad(pad));
        }
        let head = Head {
            kind,
            mode: u16::from_le_bytes([m0, m1]),
            path: usize::from(u16::from_le_bytes([p0, p1])),
            target: usize::from(u16::from_le_bytes([t0, t1])),
            size: u64::from_le_bytes(size),
            meta: pad & HAS_META != 0,
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
            Kind::Dir | Kind::Symlink | Kind::Hardlink if head.size != 0 => {
                return Err(Refused::Field {
                    what: "a directory or link",
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
            return self.named(head);
        }
        self.at = At::Names { head, len };
        Ok(None)
    }

    fn named(&mut self, head: Head) -> Result<Option<Step>, Refused> {
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
        let text = |path: &str| {
            std::str::from_utf8(target)
                .map(str::to_owned)
                .map_err(|_| Refused::Target {
                    path: path.to_owned(),
                    why: "it isn't UTF-8",
                })
        };
        let what = match head.kind {
            Kind::Dir => What::Dir,
            Kind::File => What::File { size: head.size },
            Kind::Symlink => {
                let target = text(&path)?;
                check_target(&path, &target, &self.limits)?;
                What::Symlink { target }
            }
            Kind::Hardlink => {
                let target = text(&path)?;
                check_link(&path, &target, &self.limits)?;
                What::Hardlink { target }
            }
        };
        let entry = Entry {
            path,
            mode: head.mode,
            what,
            meta: None,
        };
        if head.meta {
            self.pending = Some(entry);
            self.at = At::Meta;
            return Ok(None);
        }
        self.done(entry).map(Some)
    }

    /// An entry is whole: the next state is its file's bytes, or the next head.
    fn done(&mut self, entry: Entry) -> Result<Step, Refused> {
        self.at = match entry.what {
            What::File { size } if size > 0 => At::Data { left: size },
            _ => At::Head,
        };
        Ok(Step::Entry(entry))
    }

    /// A [`Meta`]'s fixed part read: its owner and time held, its xattrs' bytes to come, refused past
    /// their bounds from the counts alone.
    fn meta_fixed(&mut self) -> Result<Option<Step>, Refused> {
        let held = std::mem::take(&mut self.held);
        let fixed = held
            .get(..META_BYTES)
            .and_then(|b| <[u8; META_BYTES]>::try_from(b).ok())
            .ok_or(Refused::Truncated("in the middle of an entry's metadata"))?;
        let [
            u0,
            u1,
            u2,
            u3,
            g0,
            g1,
            g2,
            g3,
            s0,
            s1,
            s2,
            s3,
            s4,
            s5,
            s6,
            s7,
            n0,
            n1,
            n2,
            n3,
            c0,
            c1,
            z0,
            z1,
            b0,
            b1,
            b2,
            b3,
        ] = fixed;
        let entry = self
            .pending
            .as_mut()
            .ok_or(Refused::Truncated("before an entry's names"))?;
        let path = entry.path.clone();
        if [z0, z1] != [0, 0] {
            return Err(Refused::Pad(z0 | z1));
        }
        let count = usize::from(u16::from_le_bytes([c0, c1]));
        let len = usize::try_from(u32::from_le_bytes([b0, b1, b2, b3])).unwrap_or(usize::MAX);
        if count > XATTRS_MAX || len > XATTR_BLOCK_MAX {
            return Err(Refused::Meta {
                path,
                why: format!(
                    "{count} extended attributes in {len} bytes are over the {XATTRS_MAX} in {XATTR_BLOCK_MAX} an entry carries"
                ),
            });
        }
        entry.meta = Some(Meta {
            uid: u32::from_le_bytes([u0, u1, u2, u3]),
            gid: u32::from_le_bytes([g0, g1, g2, g3]),
            mtime: Mtime {
                secs: i64::from_le_bytes([s0, s1, s2, s3, s4, s5, s6, s7]),
                nanos: u32::from_le_bytes([n0, n1, n2, n3]),
            },
            xattrs: Vec::new(),
        });
        if len == 0 && count == 0 {
            return self.whole(Vec::new()).map(Some);
        }
        self.at = At::Xattrs { count, len };
        Ok(None)
    }

    /// The xattrs' bytes read: exactly `count` of them, filling them.
    fn xattrs(&mut self, count: usize) -> Result<Vec<Xattr>, Refused> {
        let held = std::mem::take(&mut self.held);
        let path = self
            .pending
            .as_ref()
            .map(|e| e.path.clone())
            .unwrap_or_default();
        let bad = |why: &str| Refused::Meta {
            path: path.clone(),
            why: why.to_owned(),
        };
        let mut rest = held.as_slice();
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let (&name_len, after) = rest
                .split_first()
                .ok_or_else(|| bad("its extended attributes end early"))?;
            let (value_len, after) = after
                .split_at_checked(4)
                .ok_or_else(|| bad("its extended attributes end early"))?;
            let value_len = usize::try_from(u32::from_le_bytes(
                <[u8; 4]>::try_from(value_len)
                    .map_err(|_| bad("its extended attributes end early"))?,
            ))
            .unwrap_or(usize::MAX);
            let (name, after) = after
                .split_at_checked(usize::from(name_len))
                .ok_or_else(|| bad("its extended attributes end early"))?;
            let (value, after) = after
                .split_at_checked(value_len)
                .ok_or_else(|| bad("its extended attributes end early"))?;
            let name = std::str::from_utf8(name)
                .map_err(|_| bad("an extended attribute's name isn't UTF-8"))?
                .to_owned();
            out.push(Xattr {
                name,
                value: value.to_vec(),
            });
            rest = after;
        }
        if !rest.is_empty() {
            return Err(bad("bytes follow its extended attributes"));
        }
        Ok(out)
    }

    /// The pending entry with its xattrs, checked whole.
    fn whole(&mut self, xattrs: Vec<Xattr>) -> Result<Step, Refused> {
        let mut entry = self
            .pending
            .take()
            .ok_or(Refused::Truncated("before an entry's names"))?;
        if let Some(meta) = entry.meta.as_mut() {
            meta.xattrs = xattrs;
            check_meta(&entry.path, meta)?;
        }
        self.done(entry)
    }
}

#[cfg(test)]
mod tests;
