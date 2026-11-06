//! The relay's staging port (docs/design/shards-guest.md §4a): it writes a box's bundle and root, as the
//! host sends them ([`hex_core::stream::files`]), under `<staged>/<box id>/`, and removes that copy when
//! the host deletes the box. Every name is resolved inside the box's staged root (`RESOLVE_IN_ROOT`,
//! openat2(2)): a layer's entry through the layer's own links, inside its root, as containerd's unpack
//! resolves it; any other through no link, so a stream that names its way out through one is refused.
//! Every count is bounded by the core's [`Limits`].

use std::collections::{BTreeMap, BTreeSet};
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
/// How many times an openat2 that the kernel answers `EAGAIN` is tried: it does so when a rename or
/// mount ran during the lookup (openat2(2), "EAGAIN ... RESOLVE_IN_ROOT"), which nothing but the relay
/// does under a staged root, so a few tries are past any race and the last `EAGAIN` is returned typed.
const RESOLVE_TRIES: usize = 8;
/// A staged root's mode once whole: the node and a container's mapped users traverse it.
const ROOT_MODE: u16 = 0o755;
/// Each removal visits at most this many entries per entry a stage may hold: a container writes into
/// its root, so its copy may hold more than was staged, but not without bound (CLAUDE.md §2); past it the
/// removal is refused by type, the copy left for the next try.
const REMOVE_FACTOR: u64 = 4;

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
                        Op::Remove => remove(staged, &id, limits),
                        Op::Stage => {
                            stage = Some(Stage::begin(staged, id, limits)?);
                            Ok(())
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
    fn begin(staged: &Path, id: BoxId, limits: &Limits) -> Result<Self, Refusal> {
        let staged = rustix::fs::open(
            staged,
            OFlags::DIRECTORY | OFlags::RDONLY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(os("opening the staged folder"))?;
        match rustix::fs::mkdirat(&staged, id.as_str(), Mode::from_raw_mode(0o700)) {
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
        Ok(Self {
            staged,
            id,
            root,
            root_path,
            file: None,
            dirs: BTreeMap::new(),
            layer: None,
            entries: 0,
            removals: Budget::new(
                usize::try_from(limits.entries.saturating_mul(REMOVE_FACTOR)).unwrap_or(usize::MAX),
            ),
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

    /// The directory that holds `path` and `path`'s last component: inside the layer's root, through its
    /// links, when `path` is a layer's entry (its missing parents made when `make`, as an entry's are and
    /// a hard link's target's aren't), else inside the stage's root through none.
    fn parent<'p>(&self, path: &'p str, make: bool) -> Result<(OwnedFd, &'p str), Refusal> {
        if let Some((layer, rel)) = self.in_layer(path) {
            let (parent, leaf) = split(rel);
            let dir = if make {
                make_within(&layer.fd, parent)?
            } else {
                open_within(&layer.fd, parent)?
            };
            return Ok((dir, leaf));
        }
        let (parent, leaf) = path.rsplit_once('/').unwrap_or((".", path));
        Ok((open_in(&self.root, parent)?, leaf))
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
                match rustix::fs::mkdirat(&parent, leaf, Mode::from_raw_mode(0o700)) {
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
                    Mode::from_raw_mode(0o600),
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
                // The target is resolved as an entry's path is (within a layer's root through its links,
                // as containerd's `hardlinkRootPath` does, `tar.go:795-807`); linkat(2) follows no link at
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
        let Some((layer, rel)) = self.in_layer(path) else {
            return Err(invalid(format!("opaque marker {path:?} is in no layer")));
        };
        let top = open_within(&layer.fd, rel)?;
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

/// The path the kernel holds for the directory `fd` (proc(5), `/proc/self/fd/<n>`).
fn fd_path(fd: &OwnedFd) -> Result<String, Refusal> {
    let link = format!("/proc/self/fd/{}", fd.as_raw_fd());
    let path = rustix::fs::readlink(link.as_str(), Vec::new())
        .map_err(os("reading a staged directory's path"))?;
    path.into_string()
        .map_err(|_| invalid("a staged directory's path isn't UTF-8".to_owned()))
}

/// `path` within `root`, a layer's root, opened as containerd's `fs.RootPath` resolves it: through its
/// symbolic links, an absolute one or a `..` resolved inside `root` (`RESOLVE_IN_ROOT`, openat2(2)),
/// through no magic link or mount. `""` is `root` itself.
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

/// Why `path` within a layer's root couldn't be opened, by the open's `err`.
fn within_refused(path: &str, err: Errno) -> Refusal {
    match err {
        Errno::XDEV => invalid(format!("{path:?} leads through a mount")),
        Errno::NOENT => invalid(format!("{path:?} isn't staged")),
        Errno::NOTDIR => invalid(format!("{path:?} leads through a non-directory")),
        e => Refusal::Os {
            op: "opening a staged directory",
            why: format!("{path:?}: {e}"),
        },
    }
}

fn open_within(root: &OwnedFd, path: &str) -> Result<OwnedFd, Refusal> {
    within(root, path).map_err(|e| within_refused(path, e))
}

/// The directory `path` within a layer's root, each of its directories no layer made made as
/// containerd's `mkparent` makes it (`pkg/archive/tar.go:448-503`): `mkdir` 0755 then an explicit chmod
/// past the umask (`tar_unix.go:90-97`), owned by the relay and timed when it's made, since no layer
/// says otherwise. Flat: one step per component of `path`, which the path bound bounds.
fn make_within(root: &OwnedFd, path: &str) -> Result<OwnedFd, Refusal> {
    match within(root, path) {
        Err(Errno::NOENT) => {}
        done => return done.map_err(|e| within_refused(path, e)),
    }
    let mut dir = open_within(root, "")?;
    let mut end: usize = 0;
    for part in path.split('/') {
        end = end.saturating_add(part.len());
        let prefix = path.get(..end).unwrap_or(path);
        end = end.saturating_add(1);
        dir = match within(root, prefix) {
            Ok(fd) => fd,
            Err(Errno::NOENT) => {
                // A name in the way that resolves nowhere (a link to nothing) isn't made over.
                rustix::fs::mkdirat(&dir, part, Mode::from_raw_mode(0o755)).map_err(|e| {
                    invalid(format!("making {prefix:?}, a parent no layer made: {e}"))
                })?;
                let made = rustix::fs::openat(
                    &dir,
                    part,
                    OFlags::DIRECTORY | OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(os("opening a parent directory staging made"))?;
                rustix::fs::fchmod(&made, Mode::from_raw_mode(0o755))
                    .map_err(os("setting a made parent's mode"))?;
                made
            }
            Err(e) => return Err(within_refused(prefix, e)),
        };
    }
    Ok(dir)
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
pub fn remove(staged: &Path, id: &BoxId, limits: &Limits) -> Result<(), Refusal> {
    let staged = rustix::fs::open(
        staged,
        OFlags::DIRECTORY | OFlags::RDONLY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(os("opening the staged folder"))?;
    let mut budget = Budget::new(
        usize::try_from(limits.entries.saturating_mul(REMOVE_FACTOR)).unwrap_or(usize::MAX),
    );
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
        rustix::fs::fchmod(&fd, Mode::from_raw_mode(0o700))
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
