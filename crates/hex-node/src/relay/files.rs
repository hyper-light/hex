//! The relay's staging port (docs/design/shards-guest.md §4a): it writes a box's bundle and root, as the
//! host sends them ([`hex_core::stream::files`]), under `<staged>/<box id>/`, and removes that copy when
//! the host deletes the box. Every name is resolved inside the box's staged root (`RESOLVE_IN_ROOT`,
//! through no symbolic link, openat2(2)), so a stream that names its way out, or through a link it
//! made, is refused, never followed; and every count is bounded by the core's [`Limits`].

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
                            stage = Some(Stage::begin(staged, id)?);
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
    /// The file being written, with what's applied once its last byte is (§4a's order).
    file: Option<(std::fs::File, String, u16, Option<Meta>)>,
    /// Each directory's path, mode and mtime, set when the stage ends: deepest first, after everything
    /// in it, so neither a read-only mode nor writing its children undoes them.
    dirs: Vec<(String, u16, Option<Mtime>)>,
    /// Entries made so far: what an abandoned stage's removal visits.
    entries: usize,
}

/// A staged entry's time, as utimensat(2) takes it: atime set to mtime, as containerd's unpack does.
fn times(mtime: Mtime) -> Timestamps {
    let at = Timespec {
        tv_sec: mtime.secs,
        tv_nsec: i64::from(mtime.nanos),
    };
    Timestamps {
        last_access: at,
        last_modification: at,
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

/// Applies a file's or directory's owner and xattrs, then (for a file) its mode and times, on its open
/// descriptor, in §4a's order: owner, mode, xattrs, times. A directory's mode and times wait for the
/// stage's end (`mode_and_time` false).
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
            rustix::fs::futimens(fd, &times(meta.mtime))
                .map_err(os("setting a staged entry's times"))?;
        }
    }
    Ok(())
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
        Ok(Self {
            staged,
            id,
            root,
            file: None,
            dirs: Vec::new(),
            entries: 0,
        })
    }

    /// The directory that holds `path`, opened inside the root, and `path`'s last component.
    fn parent<'p>(&self, path: &'p str) -> Result<(OwnedFd, &'p str), Refusal> {
        let (parent, leaf) = path.rsplit_once('/').unwrap_or((".", path));
        Ok((open_in(&self.root, parent)?, leaf))
    }

    fn entry(&mut self, entry: &Entry) -> Result<(), Refusal> {
        if self.file.is_some() {
            return Err(Refusal::Invalid {
                what: "staged files",
                why: "an entry came in the middle of a file".to_owned(),
            });
        }
        self.entries = self.entries.saturating_add(1);
        let (parent, leaf) = self.parent(&entry.path)?;
        let exists = |e: Errno| match e {
            Errno::EXIST => Refusal::Invalid {
                what: "staged files",
                why: format!("{:?} came twice", entry.path),
            },
            e => Refusal::Os {
                op: "staging an entry",
                why: format!("{:?}: {e}", entry.path),
            },
        };
        let meta = entry.meta.as_ref();
        match &entry.what {
            What::Dir => {
                // Writable by the relay until the stage ends, whatever its mode: its entries come after it.
                rustix::fs::mkdirat(&parent, leaf, Mode::from_raw_mode(0o700)).map_err(exists)?;
                if meta.is_some() {
                    let dir = open_in(&self.root, &entry.path)?;
                    apply(&dir, &entry.path, entry.mode, meta, false)?;
                }
                self.dirs
                    .push((entry.path.clone(), entry.mode, meta.map(|m| m.mtime)));
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
                    rustix::fs::utimensat(
                        &parent,
                        leaf,
                        &times(meta.mtime),
                        AtFlags::SYMLINK_NOFOLLOW,
                    )
                    .map_err(os("setting a staged link's times"))?;
                }
            }
            What::Hardlink { target } => {
                // The target, an earlier entry, is opened inside the root through no symbolic link and
                // must be a regular file; linkat(2) follows no link at its last component.
                let (from, from_leaf) = self.parent(target)?;
                let st = rustix::fs::statat(&from, from_leaf, AtFlags::SYMLINK_NOFOLLOW).map_err(
                    |e| Refusal::Invalid {
                        what: "staged files",
                        why: format!(
                            "hard link {:?} names {target:?}, which isn't staged: {e}",
                            entry.path
                        ),
                    },
                )?;
                if FileType::from_raw_mode(st.st_mode) != FileType::RegularFile {
                    return Err(Refusal::Invalid {
                        what: "staged files",
                        why: format!(
                            "hard link {:?} names {target:?}, which isn't a regular file",
                            entry.path
                        ),
                    });
                }
                rustix::fs::linkat(&from, from_leaf, &parent, leaf, AtFlags::empty())
                    .map_err(exists)?;
            }
        }
        Ok(())
    }

    fn data(&mut self, bytes: &[u8], done: bool) -> Result<(), Refusal> {
        let (file, ..) = self.file.as_mut().ok_or_else(|| Refusal::Invalid {
            what: "staged files",
            why: "file bytes came with no file under way".to_owned(),
        })?;
        // A full tmpfs (`ENOSPC`) is the guest's memory bound on staged files, returned as the error it is.
        file.write_all(bytes)
            .map_err(|e| Refusal::os("writing a staged file", e))?;
        if done && let Some((file, path, mode, meta)) = self.file.take() {
            let fd = OwnedFd::from(file);
            apply(&fd, &path, mode, meta.as_ref(), true)?;
        }
        Ok(())
    }

    /// Sets each directory's mode and mtime, deepest first so a directory made read-only doesn't stop its
    /// children's and writing them doesn't move its time, then the root's mode.
    fn end(&mut self) -> Result<(), Refusal> {
        for (path, mode, mtime) in self.dirs.iter().rev() {
            let dir = open_in(&self.root, path)?;
            rustix::fs::fchmod(&dir, Mode::from_raw_mode((*mode).into()))
                .map_err(os("setting a staged directory's mode"))?;
            if let Some(mtime) = mtime {
                rustix::fs::futimens(&dir, &times(*mtime))
                    .map_err(os("setting a staged directory's times"))?;
            }
        }
        self.dirs = Vec::new();
        rustix::fs::fchmod(&self.root, Mode::from_raw_mode(ROOT_MODE.into()))
            .map_err(os("setting a staged root's mode"))
    }

    fn abandon(self) -> Result<(), Refusal> {
        drop(self.file);
        remove_at(&self.staged, &self.id, self.entries)
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
pub fn remove(staged: &Path, id: &BoxId, limits: &Limits) -> Result<(), Refusal> {
    let staged = rustix::fs::open(
        staged,
        OFlags::DIRECTORY | OFlags::RDONLY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(os("opening the staged folder"))?;
    let budget =
        usize::try_from(limits.entries.saturating_mul(REMOVE_FACTOR)).unwrap_or(usize::MAX);
    remove_at(&staged, id, budget)
}

/// Removes `id` under `staged`, visiting at most `budget` entries: flat, a list of the directories
/// found, emptied deepest first (CLAUDE.md §11).
fn remove_at(staged: &OwnedFd, id: &BoxId, budget: usize) -> Result<(), Refusal> {
    let root = match rustix::fs::openat(
        staged,
        id.as_str(),
        OFlags::DIRECTORY | OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(Errno::NOENT) => return Err(Refusal::NoSuchBox(id.clone())),
        Err(e) => return Err(Refusal::os("opening a box's staged root", e)),
    };
    // Every directory under the root, parents before children; the root's own is ".".
    let mut dirs: Vec<String> = vec![".".to_owned()];
    let mut seen: usize = 0;
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
            seen = seen.saturating_add(1);
            if seen > budget {
                return Err(Refusal::Exhausted {
                    what: "entries one removal of a staged copy visits",
                    bound: budget,
                });
            }
            let kind = match entry.file_type() {
                FileType::Unknown => rustix::fs::statat(&fd, name, AtFlags::SYMLINK_NOFOLLOW)
                    .map(|st| FileType::from_raw_mode(st.st_mode))
                    .map_err(os("reading a staged entry's kind"))?,
                kind => kind,
            };
            if kind == FileType::Directory {
                let name = name.to_str().map_err(|_| Refusal::Invalid {
                    what: "staged files",
                    why: format!("a name under {dir:?} isn't UTF-8"),
                })?;
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
        let (parent, leaf) = dir.rsplit_once('/').unwrap_or((".", dir.as_str()));
        let parent = open_in(&root, parent)?;
        rustix::fs::unlinkat(&parent, leaf, AtFlags::REMOVEDIR)
            .map_err(os("removing a staged directory"))?;
    }
    drop(root);
    rustix::fs::unlinkat(staged, id.as_str(), AtFlags::REMOVEDIR)
        .map_err(os("removing a box's staged root"))?;
    Ok(())
}
