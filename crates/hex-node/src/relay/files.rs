//! The relay's staging port (docs/design/shards-guest.md §4a): it writes a box's bundle and root, as the
//! host sends them ([`hex_core::stream::files`]), under `<staged>/<box id>/`, and removes that copy when
//! the host deletes the box. Every name is resolved inside the box's staged root (`RESOLVE_IN_ROOT`,
//! openat2(2)): a layer's entry through the layer's own links, inside its root, as containerd's unpack
//! resolves it; any other through no link, so a stream that names its way out through one is refused.
//! Every count is bounded by the core's [`Limits`].

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{self, Write};
use std::net::Shutdown;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::Path;

use hex_api::{BoxId, Refusal};
use hex_client::guest::Duplex;
use hex_core::stream::{
    self,
    files::{self, Entry, Limits, Meta, Mtime, Op, Step, Unpack, What},
};
use hex_core::wire::{self, Answer};
use rustix::fs::{
    AtFlags, Dir, FileType, Gid, Mode, OFlags, ResolveFlags, Timespec, Timestamps, Uid,
};
use rustix::io::Errno;

/// How a name under a staged root resolves: inside it, through no symbolic link or magic link and no
/// mount point (openat2(2)).
const RESOLVE: ResolveFlags = ResolveFlags::IN_ROOT
    .union(ResolveFlags::NO_SYMLINKS)
    .union(ResolveFlags::NO_MAGICLINKS)
    .union(ResolveFlags::NO_XDEV);
/// How a layer's entry resolves within its root: through its symbolic links, inside the root, as
/// containerd's `fs.RootPath` does (§4a), through no magic link or mount point.
const WITHIN: ResolveFlags = ResolveFlags::IN_ROOT
    .union(ResolveFlags::NO_MAGICLINKS)
    .union(ResolveFlags::NO_XDEV);
/// How many times an openat2 that the kernel answers `EAGAIN` is tried: it does so when a rename or mount
/// anywhere on the host crossed a `..` during the lookup (openat2(2); fs/namei.c v6.19 :2195-2206), and
/// leaves the retry to the caller. The oci class's resolver's budget (`IN_ROOT_TRIES`,
/// crates/hex-runtime/src/linux/oci/inroot.rs on cp/t7-inroot-eagain): libpathrs's (src/syscalls.rs:864-873),
/// as runc v1.5.2 cites it (`internal/pathrs/retry.go`); measured on fedora under 16 mount and 16 rename
/// loops, no resolve needed more than 5 (unit hex-inroot-storm-20261105T130241Z,
/// docs/research/runtime/inroot-eagain.md). The last `EAGAIN` is returned typed.
const RESOLVE_TRIES: usize = 128;
/// The most links a layer's path follows, as Linux's own lookup (`MAXSYMLINKS`, include/linux/namei.h).
const MAX_LINKS: usize = 40;
/// Linux's bounds on an inode's xattr names together and on one value (`XATTR_LIST_MAX`,
/// `XATTR_SIZE_MAX`, include/uapi/linux/limits.h): what a copied directory's xattrs are read within.
const XATTR_LIST_MAX: usize = files::XATTR_LIST_MAX;
const XATTR_SIZE_MAX: usize = files::XATTR_SIZE_MAX;
/// A staged root's mode once whole: the relay writes it, and the node and a container's mapped users read
/// and search it, which every path through it needs (path_resolution(7), "Permission"): `S_IRWXU` and
/// read and search for group and others (inode(7)).
const ROOT_MODE: u16 = 0o755;
/// A directory or file the relay alone writes while a stage is under way (`S_IRWXU`, `S_IRUSR | S_IWUSR`,
/// inode(7)): its staged mode is set once its contents are whole, and a removal makes a read-only one its
/// own again to empty it.
const OWNER_DIR: u32 = 0o700;
const OWNER_FILE: u32 = 0o600;
/// A directory mkparent makes: `mkdir` 0755 then an explicit chmod past the umask (containerd v2.3.6,
/// `pkg/archive/tar_unix.go:90-97`).
const MKPARENT_MODE: u32 = 0o755;

fn os(op: &'static str) -> impl Fn(Errno) -> Refusal {
    move |e| Refusal::os(op, e)
}

/// Serves one staging connection: reads its op, does it, and answers `Done` or `Refused`.
pub fn serve(mut conn: Box<dyn Duplex>, staged: &Path, limits: &Limits) {
    let answer = match take(conn.as_mut(), staged, limits) {
        Ok(()) => Answer::Done,
        Err(why) => Answer::Refused(why),
    };
    if let Some(bytes) = wire::answer(wire::FIRST, 0, &answer, wire::MAX_MESSAGE)
        .ok()
        .and_then(|bytes| stream::frame(&bytes, wire::MAX_MESSAGE).ok())
    {
        let _ = conn.write_all(&bytes);
    }
    let _ = conn.shutdown(Shutdown::Both);
}

fn refused(why: files::Refused) -> Refusal {
    Refusal::Invalid {
        what: "staged files",
        why: why.to_string(),
    }
}

/// Reads the connection to its end and does what it asks.
fn take(conn: &mut dyn Duplex, staged: &Path, limits: &Limits) -> Result<(), Refusal> {
    let mut unpack = Unpack::new(*limits);
    let mut buf = vec![0u8; 64 << 10];
    let mut stage: Option<Stage> = None;
    loop {
        let n = match conn.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return fail(stage, Refusal::os("reading a staging connection", e)),
        };
        let mut input = buf.get(..n).unwrap_or_default();
        while !input.is_empty() {
            let (took, step) = match unpack.take(input) {
                Ok(taken) => taken,
                Err(why) => return fail(stage, refused(why)),
            };
            let data = input.get(..took).unwrap_or_default();
            input = input.get(took..).unwrap_or_default();
            let Some(step) = step else { continue };
            let done = match (step, stage.as_mut()) {
                (Step::Open { op, id }, None) => {
                    let id = BoxId::new(&id)?;
                    match op {
                        Op::Remove => remove(staged, &id),
                        Op::Stage => {
                            let begun = Stage::begin(staged, id)?;
                            let learned = begun.free;
                            stage = Some(begun);
                            // From version 2 the packer learns the stage's budget first, and both sides
                            // spend against it.
                            unpack.learn(learned.entries, learned.bytes);
                            if unpack.version() >= 2 {
                                send_budget(conn, learned)
                            } else {
                                Ok(())
                            }
                        }
                    }
                }
                (Step::Entry(entry), Some(s)) => s.entry(&entry),
                (Step::Data { done }, Some(s)) => s.data(data, done),
                (Step::End, Some(s)) => s.end(),
                _ => Err(Refusal::Invalid {
                    what: "staged files",
                    why: "a step came out of order".to_owned(),
                }),
            };
            if let Err(why) = done {
                return fail(stage, why);
            }
        }
    }
    if let Err(why) = unpack.finish() {
        return fail(stage, refused(why));
    }
    Ok(())
}

/// What a file system has free and holds, as statvfs(3) reads it: what a stage may spend, learned when
/// it begins (CLAUDE.md §2: a bound the system says, not a number Hex picks).
#[derive(Debug, Clone, Copy)]
struct Free {
    /// Free inodes (`f_ffree`): the entries a stage may make.
    entries: u64,
    /// Bytes free to an unprivileged writer (`f_bavail` × `f_frsize`; past `u64`, no bound short of it).
    bytes: u64,
    /// Inodes in use (`f_files` − `f_ffree`): the most entries a removal can visit.
    used: u64,
}

fn free(dir: &OwnedFd) -> Result<Free, Refusal> {
    let st = rustix::fs::fstatvfs(dir).map_err(os("reading the staged folder's free space"))?;
    Ok(Free {
        entries: st.f_ffree,
        bytes: st.f_bavail.saturating_mul(st.f_frsize),
        used: st.f_files.saturating_sub(st.f_ffree),
    })
}

/// Sends a stage's budget, framed as the relay's answers are.
fn send_budget(conn: &mut dyn Duplex, free: Free) -> Result<(), Refusal> {
    let bytes = stream::frame(&files::budget(free.entries, free.bytes), wire::MAX_MESSAGE)
        .map_err(|e| Refusal::Broken(format!("framing a stage's budget: {e}")))?;
    conn.write_all(&bytes)
        .map_err(|e| Refusal::os("sending a stage's budget", e))
}

/// Removes a stage's partial copy, then returns why it failed.
fn fail(stage: Option<Stage>, why: Refusal) -> Result<(), Refusal> {
    if let Some(stage) = stage {
        let _ = stage.abandon();
    }
    Err(why)
}

/// A stage under way: its root, the file being written, and the directories whose modes are set last.
struct Stage {
    staged: OwnedFd,
    id: BoxId,
    root: OwnedFd,
    /// The root's own path in the guest, which every resolved path begins with.
    root_path: String,
    /// The file being written, with what's applied once its last byte is (§4a's order).
    file: Option<(std::fs::File, String, u16, Option<Meta>)>,
    /// Each directory's mode and times, by its resolved path under the root, the latest header's: set
    /// when its layer ends (or the stage, outside a layer), deepest first, after everything in it, so
    /// neither a read-only mode nor writing its children undoes them (`tar.go:330-337`).
    dirs: BTreeMap<String, (u16, Option<Timestamps>)>,
    /// The layer being applied, if any: its root, and what it has made by resolved path.
    layer: Option<Layer>,
    /// Entries made so far: what an abandoned stage's removal visits.
    entries: usize,
    /// What removals during the stage (a replaced entry, a whiteout, an opaque directory) may visit.
    removals: Budget,
    /// What the staged folder's file system had free when the stage began.
    free: Free,
}

/// A layer under way (§4a): its root, which its entries' names resolve within as containerd's
/// `fs.RootPath` does (absolute links and `..` stay inside it), and what it has made, which an opaque
/// marker keeps (containerd's `unpackedPaths`).
struct Layer {
    /// Its root's path in the stream, which its entries' paths begin with.
    path: String,
    fd: OwnedFd,
    made: BTreeSet<String>,
}

/// A count of entries a removal may still visit, refused by type past its bound (CLAUDE.md §2).
struct Budget {
    left: usize,
    bound: usize,
}

impl Budget {
    fn new(bound: usize) -> Self {
        Self { left: bound, bound }
    }

    fn spend(&mut self) -> Result<(), Refusal> {
        self.left = self.left.checked_sub(1).ok_or(Refusal::Exhausted {
            what: "entries one removal of a staged copy visits",
            bound: self.bound,
        })?;
        Ok(())
    }
}

/// A staged entry's times, as utimensat(2) takes them: its atime and mtime (§4a).
fn times(meta: &Meta) -> Timestamps {
    let at = |t: Mtime| Timespec {
        tv_sec: t.secs,
        tv_nsec: i64::from(t.nanos),
    };
    Timestamps {
        last_access: at(meta.atime),
        last_modification: at(meta.mtime),
    }
}

fn owner(meta: &Meta) -> (Option<Uid>, Option<Gid>) {
    (Some(Uid::from_raw(meta.uid)), Some(Gid::from_raw(meta.gid)))
}

/// An extended attribute the guest refused to set, by name: the stage is refused rather than staged
/// without it (CLAUDE.md §3).
fn refused_xattr(path: &str, name: &str, e: Errno) -> Refusal {
    Refusal::Invalid {
        what: "staged files",
        why: format!("{path:?}: the guest refused extended attribute {name}: {e}"),
    }
}

fn invalid(why: String) -> Refusal {
    Refusal::Invalid {
        what: "staged files",
        why,
    }
}

/// Applies a file's or directory's owner and xattrs, then (for a file) its mode and times, on its open
/// descriptor, in §4a's order: owner, mode, xattrs, times. A directory's mode and times wait for its
/// layer's end (`mode_and_time` false).
fn apply(
    fd: &OwnedFd,
    path: &str,
    mode: u16,
    meta: Option<&Meta>,
    mode_and_time: bool,
) -> Result<(), Refusal> {
    if let Some(meta) = meta {
        let (uid, gid) = owner(meta);
        rustix::fs::fchown(fd, uid, gid).map_err(os("setting a staged entry's owner"))?;
    }
    if mode_and_time {
        rustix::fs::fchmod(fd, Mode::from_raw_mode(mode.into()))
            .map_err(os("setting a staged entry's mode"))?;
    }
    if let Some(meta) = meta {
        for x in &meta.xattrs {
            rustix::fs::fsetxattr(
                fd,
                x.name.as_str(),
                &x.value,
                rustix::fs::XattrFlags::empty(),
            )
            .map_err(|e| refused_xattr(path, &x.name, e))?;
        }
        if mode_and_time {
            rustix::fs::futimens(fd, &times(meta)).map_err(os("setting a staged entry's times"))?;
        }
    }
    Ok(())
}

/// `rel`'s parent and last component: `("", rel)` for a name at the top.
fn split(rel: &str) -> (&str, &str) {
    rel.rsplit_once('/').unwrap_or(("", rel))
}

/// `leaf` under the resolved directory `dir`, `""` being the root.
fn join(dir: &str, leaf: &str) -> String {
    if dir.is_empty() {
        leaf.to_owned()
    } else {
        format!("{dir}/{leaf}")
    }
}

impl Stage {
    /// Makes `<staged>/<id>`, which must not exist: one there is another box's, or a copy the host
    /// hasn't removed, and staging over it would change what that box runs.
    fn begin(staged: &Path, id: BoxId) -> Result<Self, Refusal> {
        let staged = rustix::fs::open(
            staged,
            OFlags::DIRECTORY | OFlags::RDONLY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(os("opening the staged folder"))?;
        match rustix::fs::mkdirat(&staged, id.as_str(), Mode::from_raw_mode(OWNER_DIR)) {
            Ok(()) => {}
            Err(Errno::EXIST) => return Err(Refusal::Exists(id)),
            Err(e) => return Err(Refusal::os("making a box's staged root", e)),
        }
        let root = rustix::fs::openat(
            &staged,
            id.as_str(),
            OFlags::DIRECTORY | OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(os("opening a box's staged root"))?;
        let root_path = fd_path(&root)?;
        let free = free(&staged)?;
        Ok(Self {
            staged,
            id,
            root,
            root_path,
            file: None,
            dirs: BTreeMap::new(),
            layer: None,
            entries: 0,
            // A removal visits no more entries than the file system holds.
            removals: Budget::new(usize::try_from(free.used).unwrap_or(usize::MAX)),
            free,
        })
    }

    /// Where the directory `fd` is under the root, by the kernel's own resolution of it: what an
    /// entry's resolved path is made from, so one reached through a link is known by where it is.
    fn resolved(&self, fd: &OwnedFd) -> Result<String, Refusal> {
        let path = fd_path(fd)?;
        let rest = path.strip_prefix(self.root_path.as_str()).ok_or_else(|| {
            Refusal::Broken(format!(
                "a staged directory, {path:?}, resolved outside its root"
            ))
        })?;
        Ok(rest.trim_start_matches('/').to_owned())
    }

    /// The directory that holds `path` and `path`'s last component: as the layer's overlay unpack finds it
    /// when `path` is a layer's entry ([`Stage::walk`]; its parents made when `make`, as an entry's are and
    /// a hard link's target's aren't), else inside the stage's root through no link.
    fn parent<'p>(&mut self, path: &'p str, make: bool) -> Result<(OwnedFd, &'p str), Refusal> {
        if let Some(rel) = self.in_layer(path).map(|(_, rel)| rel) {
            let (parent, leaf) = split(rel);
            return Ok((self.walk(parent, make)?, leaf));
        }
        let (parent, leaf) = path.rsplit_once('/').unwrap_or((".", path));
        Ok((open_in(&self.root, parent)?, leaf))
    }

    /// The directory `rel` within the layer's root, as containerd's overlay applier finds it: it unpacks a
    /// layer into the snapshot's upper directory alone (`core/diff/apply/apply_linux.go:36-55` at v2.3.6),
    /// so `fs.RootPath` follows only links this layer made, inside the root; an earlier layer's link or
    /// file in the way isn't in that directory, and mkparent makes a directory there
    /// (`pkg/archive/tar.go:448-503`), copying the owner, mode, times and xattrs but `trusted.*` of what
    /// the name resolves to below when that is a directory (`WithParents`, `:482-500`; `copyDirInfo`,
    /// `copyUpXAttrs`, `tar_unix.go:143-203`), else 0755; in the merged view it replaces the link or file.
    /// ctr's own snapshot holds exactly this (bench/native/guest-tree, lab:tree and lab:file). A name
    /// missing, or (when not `make`) not in this layer's view, is refused. Flat: one step per component,
    /// at most [`MAX_LINKS`] links followed, each component opened `O_NOFOLLOW`.
    fn walk(&mut self, rel: &str, make: bool) -> Result<OwnedFd, Refusal> {
        let top = self
            .layer
            .as_ref()
            .ok_or_else(|| invalid(format!("{rel:?} is in no layer")))?
            .fd
            .try_clone()
            .map_err(|e| Refusal::os("opening a layer's root", e))?;
        let base = self.resolved(&top)?;
        let mut fd = top
            .try_clone()
            .map_err(|e| Refusal::os("opening a layer's root", e))?;
        // The components below the layer's root reached so far, and those still to walk.
        let mut at: Vec<String> = Vec::new();
        let mut todo: VecDeque<String> = rel.split('/').map(str::to_owned).collect();
        let mut links: usize = 0;
        while let Some(part) = todo.pop_front() {
            if part.is_empty() || part == "." {
                continue;
            }
            if part == ".." {
                // Inside the root, as RESOLVE_IN_ROOT holds it: `..` at the root stays there.
                at.pop();
                fd = if at.is_empty() {
                    top.try_clone()
                        .map_err(|e| Refusal::os("opening a layer's root", e))?
                } else {
                    open_in(&self.root, &format!("{base}/{}", at.join("/")))?
                };
                continue;
            }
            let below = if at.is_empty() {
                part.clone()
            } else {
                format!("{}/{part}", at.join("/"))
            };
            let path = join(&base, &below);
            let made = self
                .layer
                .as_ref()
                .is_some_and(|layer| layer.made.contains(&path));
            match rustix::fs::statat(&fd, part.as_str(), AtFlags::SYMLINK_NOFOLLOW)
                .map(|st| FileType::from_raw_mode(st.st_mode))
            {
                Ok(FileType::Directory) => {}
                Ok(FileType::Symlink) if made => {
                    links = links.saturating_add(1);
                    if links > MAX_LINKS {
                        return Err(invalid(format!(
                            "{rel:?} follows more than {MAX_LINKS} links"
                        )));
                    }
                    let target = rustix::fs::readlinkat(&fd, part.as_str(), Vec::new())
                        .map_err(os("reading a staged link"))?
                        .into_string()
                        .map_err(|_| invalid(format!("{path:?}'s target isn't UTF-8")))?;
                    if target.starts_with('/') {
                        at.clear();
                        fd = top
                            .try_clone()
                            .map_err(|e| Refusal::os("opening a layer's root", e))?;
                    }
                    for (i, step) in target.split('/').enumerate() {
                        todo.insert(i, step.to_owned());
                    }
                    continue;
                }
                Ok(_) if made => {
                    return Err(invalid(format!(
                        "{rel:?} leads through {path:?}, which this layer made a file"
                    )));
                }
                Ok(_) if !make => {
                    return Err(invalid(format!(
                        "{rel:?} leads through {path:?}, which an earlier layer made and this layer's \
                         unpack doesn't see"
                    )));
                }
                Err(Errno::NOENT) if !make => {
                    return Err(invalid(format!(
                        "{rel:?} isn't staged: {path:?} is missing"
                    )));
                }
                Ok(_) | Err(Errno::NOENT) => {
                    // What the name resolves to below, whose attributes mkparent copies when it's a
                    // directory; then the earlier layer's link or file goes.
                    let below_dir = within(&top, &below).ok();
                    self.remove(&fd, &part, &path)?;
                    rustix::fs::mkdirat(&fd, part.as_str(), Mode::from_raw_mode(MKPARENT_MODE))
                        .map_err(|e| invalid(format!("making {path:?}, a parent: {e}")))?;
                    let dir = rustix::fs::openat(
                        &fd,
                        part.as_str(),
                        OFlags::DIRECTORY | OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    )
                    .map_err(os("opening a parent directory staging made"))?;
                    match below_dir {
                        Some(from) => copy_dir_info(&from, &dir)?,
                        None => rustix::fs::fchmod(&dir, Mode::from_raw_mode(MKPARENT_MODE))
                            .map_err(os("setting a made parent's mode"))?,
                    }
                }
                Err(e) => {
                    return Err(Refusal::Os {
                        op: "walking a staged path",
                        why: format!("{path:?}: {e}"),
                    });
                }
            }
            fd = rustix::fs::openat(
                &fd,
                part.as_str(),
                OFlags::DIRECTORY | OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(os("opening a staged directory"))?;
            at.push(part);
        }
        Ok(fd)
    }

    /// The layer under way and `path` within its root, when `path` is under it.
    fn in_layer<'p>(&self, path: &'p str) -> Option<(&Layer, &'p str)> {
        let layer = self.layer.as_ref()?;
        let rest = path.strip_prefix(layer.path.as_str())?;
        if rest.is_empty() {
            return Some((layer, ""));
        }
        Some((layer, rest.strip_prefix('/')?))
    }

    fn entry(&mut self, entry: &Entry) -> Result<(), Refusal> {
        if self.file.is_some() {
            return Err(invalid("an entry came in the middle of a file".to_owned()));
        }
        self.entries = self.entries.saturating_add(1);
        match &entry.what {
            What::Layer => return self.layer(&entry.path),
            What::Opaque => return self.opaque(&entry.path),
            _ => {}
        }
        let layered = self.in_layer(&entry.path).is_some();
        let (parent, leaf) = self.parent(&entry.path, true)?;
        let at = join(&self.resolved(&parent)?, leaf);
        if matches!(entry.what, What::Whiteout) {
            if !layered {
                return Err(invalid(format!("whiteout {:?} is in no layer", entry.path)));
            }
            return self.remove(&parent, leaf, &at);
        }
        if layered {
            // A layer's entry replaces whatever is at its path, but a directory onto a directory merges
            // and takes this header's owner, mode, xattrs and times (`tar.go:304-311`).
            match rustix::fs::statat(&parent, leaf, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(st)
                    if matches!(entry.what, What::Dir)
                        && FileType::from_raw_mode(st.st_mode) == FileType::Directory => {}
                Ok(_) => self.remove(&parent, leaf, &at)?,
                Err(Errno::NOENT) => {}
                Err(e) => {
                    return Err(Refusal::Os {
                        op: "staging an entry",
                        why: format!("{:?}: {e}", entry.path),
                    });
                }
            }
        }
        let exists = |e: Errno| match e {
            Errno::EXIST => invalid(format!("{:?} came twice", entry.path)),
            e => Refusal::Os {
                op: "staging an entry",
                why: format!("{:?}: {e}", entry.path),
            },
        };
        let meta = entry.meta.as_ref();
        match &entry.what {
            What::Dir => {
                // Writable by the relay until its layer ends, whatever its mode: its entries come after it.
                match rustix::fs::mkdirat(&parent, leaf, Mode::from_raw_mode(OWNER_DIR)) {
                    Err(Errno::EXIST) if layered => {}
                    done => done.map_err(exists)?,
                }
                if meta.is_some() {
                    let dir = rustix::fs::openat(
                        &parent,
                        leaf,
                        OFlags::DIRECTORY | OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    )
                    .map_err(os("opening a staged directory"))?;
                    apply(&dir, &entry.path, entry.mode, meta, false)?;
                }
                self.dirs.insert(at.clone(), (entry.mode, meta.map(times)));
            }
            What::File { size } => {
                let fd = rustix::fs::openat(
                    &parent,
                    leaf,
                    OFlags::CREATE
                        | OFlags::EXCL
                        | OFlags::WRONLY
                        | OFlags::NOFOLLOW
                        | OFlags::CLOEXEC,
                    Mode::from_raw_mode(OWNER_FILE),
                )
                .map_err(exists)?;
                if *size > 0 {
                    self.file = Some((
                        std::fs::File::from(fd),
                        entry.path.clone(),
                        entry.mode,
                        entry.meta.clone(),
                    ));
                } else {
                    apply(&fd, &entry.path, entry.mode, meta, true)?;
                }
            }
            What::Symlink { target } => {
                rustix::fs::symlinkat(target.as_str(), &parent, leaf).map_err(exists)?;
                if let Some(meta) = meta {
                    let (uid, gid) = owner(meta);
                    rustix::fs::chownat(&parent, leaf, uid, gid, AtFlags::SYMLINK_NOFOLLOW)
                        .map_err(os("setting a staged link's owner"))?;
                    // A link has no descriptor to set an xattr through: its path through the parent's
                    // descriptor, which the relay alone writes under.
                    let at = format!("/proc/self/fd/{}/{leaf}", parent.as_raw_fd());
                    for x in &meta.xattrs {
                        rustix::fs::lsetxattr(
                            at.as_str(),
                            x.name.as_str(),
                            &x.value,
                            rustix::fs::XattrFlags::empty(),
                        )
                        .map_err(|e| refused_xattr(&entry.path, &x.name, e))?;
                    }
                    rustix::fs::utimensat(&parent, leaf, &times(meta), AtFlags::SYMLINK_NOFOLLOW)
                        .map_err(os("setting a staged link's times"))?;
                }
            }
            What::Hardlink { target } => {
                // The target is resolved as an entry's path is (within this layer's view, as containerd's
                // `hardlinkRootPath` does in the upper directory, `tar.go:795-807`); linkat(2) follows no link at
                // its last component, as containerd's `link` (os.Link) doesn't, so a link names the link
                // itself. A directory, which link(2) refuses, is refused by name.
                let (from, from_leaf) = self.parent(target, false)?;
                let st = rustix::fs::statat(&from, from_leaf, AtFlags::SYMLINK_NOFOLLOW).map_err(
                    |e| {
                        invalid(format!(
                            "hard link {:?} names {target:?}, which isn't staged: {e}",
                            entry.path
                        ))
                    },
                )?;
                // In a layer, only what this layer made: overlay's unpack links within the layer's own
                // directory, and fails a link to an earlier layer's file (ENOENT; ctr, lab:hardlink).
                if layered {
                    let source = join(&self.resolved(&from)?, from_leaf);
                    if !self
                        .layer
                        .as_ref()
                        .is_some_and(|layer| layer.made.contains(&source))
                    {
                        return Err(invalid(format!(
                            "hard link {:?} names {target:?}, a file an earlier layer made, which \
                             containerd's overlay unpack can't link (ENOENT in this layer)",
                            entry.path
                        )));
                    }
                }
                if !matches!(
                    FileType::from_raw_mode(st.st_mode),
                    FileType::RegularFile | FileType::Symlink
                ) {
                    return Err(invalid(format!(
                        "hard link {:?} names {target:?}, which is neither a file nor a link",
                        entry.path
                    )));
                }
                rustix::fs::linkat(&from, from_leaf, &parent, leaf, AtFlags::empty())
                    .map_err(exists)?;
            }
            What::Layer | What::Whiteout | What::Opaque => {}
        }
        if let Some(layer) = self.layer.as_mut() {
            layer.made.insert(at);
        }
        Ok(())
    }

    /// A layer begins at `path`, a directory already staged: the last one's directories are finished,
    /// and what follows applies over what came before.
    fn layer(&mut self, path: &str) -> Result<(), Refusal> {
        self.finish_dirs()?;
        let fd = open_in(&self.root, path)?;
        self.layer = Some(Layer {
            path: path.to_owned(),
            fd,
            made: BTreeSet::new(),
        });
        Ok(())
    }

    /// Removes `leaf` under `parent`, resolved as `at`, with everything under it: nothing when it isn't
    /// there, as containerd's `os.RemoveAll` (`tar.go:218-222,304-311`).
    fn remove(&mut self, parent: &OwnedFd, leaf: &str, at: &str) -> Result<(), Refusal> {
        let kind = match rustix::fs::statat(parent, leaf, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(st) => FileType::from_raw_mode(st.st_mode),
            Err(Errno::NOENT) => return Ok(()),
            Err(e) => return Err(Refusal::os("reading a staged entry's kind", e)),
        };
        self.removals.spend()?;
        if kind == FileType::Directory {
            remove_tree(parent, leaf, &mut self.removals)?;
        } else {
            rustix::fs::unlinkat(parent, leaf, AtFlags::empty())
                .map_err(os("removing a staged file"))?;
        }
        // Its directories' modes and times, and theirs under it, are no longer set.
        let under = format!("{at}/");
        self.dirs.remove(at);
        let gone: Vec<String> = self
            .dirs
            .range(under.clone()..)
            .map(|(k, _)| k)
            .take_while(|k| k.starts_with(&under))
            .cloned()
            .collect();
        for k in gone {
            self.dirs.remove(&k);
        }
        Ok(())
    }

    /// An opaque marker: removes what the directory `path` holds that this layer hasn't made, with
    /// everything under it, as containerd's walk does (`tar.go:184-216`): flat, a list of the directories
    /// this layer made to look into, every visit counted.
    fn opaque(&mut self, path: &str) -> Result<(), Refusal> {
        let Some(rel) = self.in_layer(path).map(|(_, rel)| rel) else {
            return Err(invalid(format!("opaque marker {path:?} is in no layer")));
        };
        let top = self.walk(rel, true)?;
        let mut look = vec![self.resolved(&top)?];
        drop(top);
        while let Some(dir) = look.pop() {
            let fd = open_in(&self.root, if dir.is_empty() { "." } else { &dir })?;
            let mut names: Vec<(String, FileType)> = Vec::new();
            for entry in Dir::read_from(&fd).map_err(os("listing a staged directory"))? {
                let entry = entry.map_err(os("listing a staged directory"))?;
                let name = entry.file_name();
                if name == c"." || name == c".." {
                    continue;
                }
                self.removals.spend()?;
                let name = name
                    .to_str()
                    .map_err(|_| invalid(format!("a name under {dir:?} isn't UTF-8")))?;
                let kind = match entry.file_type() {
                    FileType::Unknown => rustix::fs::statat(&fd, name, AtFlags::SYMLINK_NOFOLLOW)
                        .map(|st| FileType::from_raw_mode(st.st_mode))
                        .map_err(os("reading a staged entry's kind"))?,
                    kind => kind,
                };
                names.push((name.to_owned(), kind));
            }
            for (name, kind) in names {
                let at = join(&dir, &name);
                let made = self
                    .layer
                    .as_ref()
                    .is_some_and(|layer| layer.made.contains(&at));
                if !made {
                    self.remove(&fd, &name, &at)?;
                } else if kind == FileType::Directory {
                    look.push(at);
                }
            }
        }
        Ok(())
    }

    fn data(&mut self, bytes: &[u8], done: bool) -> Result<(), Refusal> {
        let (file, ..) = self
            .file
            .as_mut()
            .ok_or_else(|| invalid("file bytes came with no file under way".to_owned()))?;
        // A full tmpfs (`ENOSPC`) is the guest's memory bound on staged files, returned as the error it is.
        file.write_all(bytes)
            .map_err(|e| Refusal::os("writing a staged file", e))?;
        if done && let Some((file, path, mode, meta)) = self.file.take() {
            let fd = OwnedFd::from(file);
            apply(&fd, &path, mode, meta.as_ref(), true)?;
        }
        Ok(())
    }

    /// Sets each directory's mode and times, deepest first (a name sorts before those under it, so in
    /// reverse they come first) so a directory made read-only doesn't stop its children's and writing
    /// them doesn't move its time.
    fn finish_dirs(&mut self) -> Result<(), Refusal> {
        for (path, (mode, times)) in self.dirs.iter().rev() {
            let dir = open_in(&self.root, path)?;
            rustix::fs::fchmod(&dir, Mode::from_raw_mode((*mode).into()))
                .map_err(os("setting a staged directory's mode"))?;
            if let Some(times) = times {
                rustix::fs::futimens(&dir, times)
                    .map_err(os("setting a staged directory's times"))?;
            }
        }
        self.dirs = BTreeMap::new();
        Ok(())
    }

    /// Finishes the last layer's directories, then sets the root's mode.
    fn end(&mut self) -> Result<(), Refusal> {
        self.finish_dirs()?;
        self.layer = None;
        rustix::fs::fchmod(&self.root, Mode::from_raw_mode(ROOT_MODE.into()))
            .map_err(os("setting a staged root's mode"))
    }

    fn abandon(self) -> Result<(), Refusal> {
        drop(self.file);
        drop(self.layer);
        let mut budget = Budget::new(self.entries);
        remove_tree(&self.staged, self.id.as_str(), &mut budget)
    }
}

/// What mkparent copies to a directory it makes from the one below (`copyDirInfo`, `copyUpXAttrs`,
/// containerd `pkg/archive/tar_unix.go:143-203`): owner, mode, atime and mtime, and every xattr but
/// `trusted.*`.
fn copy_dir_info(from: &OwnedFd, to: &OwnedFd) -> Result<(), Refusal> {
    let st = rustix::fs::fstat(from).map_err(os("reading a lower directory's attributes"))?;
    rustix::fs::fchown(
        to,
        Some(Uid::from_raw(st.st_uid)),
        Some(Gid::from_raw(st.st_gid)),
    )
    .map_err(os("copying a lower directory's owner"))?;
    rustix::fs::fchmod(
        to,
        Mode::from_raw_mode(st.st_mode & u32::from(files::MODE_BITS)),
    )
    .map_err(os("copying a lower directory's mode"))?;
    let mut names = vec![0u8; XATTR_LIST_MAX];
    let n = match rustix::fs::flistxattr(from, &mut names) {
        Ok(n) => n,
        Err(Errno::NOTSUP) => 0,
        Err(e) => return Err(Refusal::os("listing a lower directory's xattrs", e)),
    };
    let mut value = vec![0u8; XATTR_SIZE_MAX];
    for name in names.get(..n).unwrap_or_default().split(|b| *b == 0) {
        if name.is_empty() || name.starts_with(b"trusted.") {
            continue;
        }
        let len = match rustix::fs::fgetxattr(from, name, &mut value) {
            Ok(len) => len,
            Err(Errno::NODATA | Errno::NOTSUP) => continue,
            Err(e) => return Err(Refusal::os("reading a lower directory's xattr", e)),
        };
        rustix::fs::fsetxattr(
            to,
            name,
            value.get(..len).unwrap_or_default(),
            rustix::fs::XattrFlags::empty(),
        )
        .map_err(os("copying a lower directory's xattr"))?;
    }
    rustix::fs::futimens(
        to,
        &Timestamps {
            last_access: Timespec {
                tv_sec: st.st_atime,
                tv_nsec: st.st_atime_nsec.try_into().unwrap_or(0),
            },
            last_modification: Timespec {
                tv_sec: st.st_mtime,
                tv_nsec: st.st_mtime_nsec.try_into().unwrap_or(0),
            },
        },
    )
    .map_err(os("copying a lower directory's times"))
}

/// The path the kernel holds for the directory `fd` (proc(5), `/proc/self/fd/<n>`).
fn fd_path(fd: &OwnedFd) -> Result<String, Refusal> {
    let link = format!("/proc/self/fd/{}", fd.as_raw_fd());
    let path = rustix::fs::readlink(link.as_str(), Vec::new())
        .map_err(os("reading a staged directory's path"))?;
    path.into_string()
        .map_err(|_| invalid("a staged directory's path isn't UTF-8".to_owned()))
}

/// `path` within `root`, a layer's root, opened through every link in it, an absolute one or a `..`
/// resolved inside `root` (`RESOLVE_IN_ROOT`, openat2(2)), through no magic link or mount: what an earlier
/// layer's name resolves to in the layers below, whose attributes mkparent copies (`WithParents`,
/// `pkg/archive/tar.go:482-500`). `""` is `root` itself.
fn within(root: &OwnedFd, path: &str) -> Result<OwnedFd, Errno> {
    let path = if path.is_empty() { "." } else { path };
    let mut tries = 0;
    loop {
        match rustix::fs::openat2(
            root,
            path,
            OFlags::DIRECTORY | OFlags::RDONLY | OFlags::CLOEXEC,
            Mode::empty(),
            WITHIN,
        ) {
            Err(Errno::AGAIN) if tries < RESOLVE_TRIES => tries = tries.saturating_add(1),
            done => return done,
        }
    }
}

/// `path` under `root`, a directory, opened inside it through no symbolic link.
fn open_in(root: &OwnedFd, path: &str) -> Result<OwnedFd, Refusal> {
    let mut tries = 0;
    loop {
        match rustix::fs::openat2(
            root,
            path,
            OFlags::DIRECTORY | OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
            RESOLVE,
        ) {
            Ok(fd) => return Ok(fd),
            Err(Errno::AGAIN) if tries < RESOLVE_TRIES => tries = tries.saturating_add(1),
            Err(Errno::XDEV) => {
                return Err(Refusal::Invalid {
                    what: "staged files",
                    why: format!("{path:?} leads through a mount"),
                });
            }
            // A symbolic link in the way is `ELOOP` from openat2's RESOLVE_NO_SYMLINKS, but the
            // last component's, opened O_DIRECTORY|O_NOFOLLOW, is `ENOTDIR` on Linux 6.19 (as a
            // file in the way is): which is read from the component itself.
            Err(e @ (Errno::LOOP | Errno::NOTDIR)) => return Err(in_the_way(root, path, e)),
            Err(e) => {
                return Err(Refusal::Os {
                    op: "opening a staged directory",
                    why: format!("{path:?}: {e}"),
                });
            }
        }
    }
}

/// Why `path` under `root` isn't a directory to open, `err` being the open's: its first component that
/// is a symbolic link or not a directory, read without following it, each prefix before it a directory
/// already (so each read passes through directories alone). At most the path's components are read.
fn in_the_way(root: &OwnedFd, path: &str, err: Errno) -> Refusal {
    let mut end: usize = 0;
    for part in path.split('/') {
        end = end.saturating_add(part.len());
        let prefix = path.get(..end).unwrap_or(path);
        end = end.saturating_add(1);
        let kind = rustix::fs::statat(root, prefix, AtFlags::SYMLINK_NOFOLLOW)
            .map(|st| FileType::from_raw_mode(st.st_mode));
        let why = match kind {
            Ok(FileType::Directory) => continue,
            Ok(FileType::Symlink) => format!("{path:?} leads through a symbolic link, {prefix:?}"),
            Ok(_) => format!("{path:?} leads through {prefix:?}, which isn't a directory"),
            Err(_) => break,
        };
        return Refusal::Invalid {
            what: "staged files",
            why,
        };
    }
    Refusal::Os {
        op: "opening a staged directory",
        why: format!("{path:?}: {err}"),
    }
}

/// Removes `<staged>/<id>` and everything under it.
pub fn remove(staged: &Path, id: &BoxId) -> Result<(), Refusal> {
    let staged = rustix::fs::open(
        staged,
        OFlags::DIRECTORY | OFlags::RDONLY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(os("opening the staged folder"))?;
    // A removal visits no more entries than the file system holds.
    let mut budget = Budget::new(usize::try_from(free(&staged)?.used).unwrap_or(usize::MAX));
    match rustix::fs::statat(&staged, id.as_str(), AtFlags::SYMLINK_NOFOLLOW) {
        Err(Errno::NOENT) => return Err(Refusal::NoSuchBox(id.clone())),
        Err(e) => return Err(Refusal::os("opening a box's staged root", e)),
        Ok(_) => {}
    }
    remove_tree(&staged, id.as_str(), &mut budget)
}

/// Removes the directory `leaf` under `parent` and everything under it, each entry visited spending
/// `budget`: flat, a list of the directories found, emptied deepest first (CLAUDE.md §11).
fn remove_tree(parent: &OwnedFd, leaf: &str, budget: &mut Budget) -> Result<(), Refusal> {
    let root = rustix::fs::openat(
        parent,
        leaf,
        OFlags::DIRECTORY | OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(os("opening a staged directory to removal"))?;
    // Every directory under the root, parents before children; the root's own is ".".
    let mut dirs: Vec<String> = vec![".".to_owned()];
    let mut next = 0;
    while let Some(dir) = dirs.get(next).cloned() {
        next = next.saturating_add(1);
        let fd = open_in(&root, &dir)?;
        // A directory may be read-only (its staged mode, or the container's): the relay empties it.
        rustix::fs::fchmod(&fd, Mode::from_raw_mode(OWNER_DIR))
            .map_err(os("opening a staged directory to removal"))?;
        let reader = Dir::read_from(&fd).map_err(os("listing a staged directory"))?;
        for entry in reader {
            let entry = entry.map_err(os("listing a staged directory"))?;
            let name = entry.file_name();
            if name == c"." || name == c".." {
                continue;
            }
            budget.spend()?;
            let kind = match entry.file_type() {
                FileType::Unknown => rustix::fs::statat(&fd, name, AtFlags::SYMLINK_NOFOLLOW)
                    .map(|st| FileType::from_raw_mode(st.st_mode))
                    .map_err(os("reading a staged entry's kind"))?,
                kind => kind,
            };
            if kind == FileType::Directory {
                let name = name
                    .to_str()
                    .map_err(|_| invalid(format!("a name under {dir:?} isn't UTF-8")))?;
                dirs.push(if dir == "." {
                    name.to_owned()
                } else {
                    format!("{dir}/{name}")
                });
            } else {
                rustix::fs::unlinkat(&fd, name, AtFlags::empty())
                    .map_err(os("removing a staged file"))?;
            }
        }
    }
    for dir in dirs.iter().skip(1).rev() {
        let (up, name) = dir.rsplit_once('/').unwrap_or((".", dir.as_str()));
        let up = open_in(&root, up)?;
        rustix::fs::unlinkat(&up, name, AtFlags::REMOVEDIR)
            .map_err(os("removing a staged directory"))?;
    }
    drop(root);
    rustix::fs::unlinkat(parent, leaf, AtFlags::REMOVEDIR)
        .map_err(os("removing a staged directory"))?;
    Ok(())
}
