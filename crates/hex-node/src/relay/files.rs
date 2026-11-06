//! The relay's staging port (docs/design/shards-guest.md §4a): it writes a box's bundle and root, as the
//! host sends them ([`hex_core::stream::files`]), under `<staged>/<box id>/`, and removes that copy when
//! the host deletes the box. Every name is resolved inside the box's staged root (`RESOLVE_IN_ROOT`,
//! through no symbolic link, openat2(2)), so a stream that names its way out, or through a link it
//! made, is refused, never followed; and every count is bounded by the core's [`Limits`].

use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::fd::OwnedFd;
use std::path::Path;

use hex_api::{BoxId, Refusal};
use hex_client::guest::Duplex;
use hex_core::stream::{
    self,
    files::{self, Entry, Limits, Op, Step, Unpack, What},
};
use hex_core::wire::{self, Answer};
use rustix::fs::{AtFlags, Dir, FileType, Mode, OFlags, ResolveFlags};
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
    file: Option<std::fs::File>,
    dirs: Vec<(String, u16)>,
    /// Entries made so far: what an abandoned stage's removal visits.
    entries: usize,
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
        match &entry.what {
            What::Dir => {
                // Writable by the relay until the stage ends, whatever its mode: its entries come after it.
                rustix::fs::mkdirat(&parent, leaf, Mode::from_raw_mode(0o700)).map_err(exists)?;
                self.dirs.push((entry.path.clone(), entry.mode));
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
                rustix::fs::fchmod(&fd, Mode::from_raw_mode(entry.mode.into()))
                    .map_err(os("setting a staged file's mode"))?;
                if *size > 0 {
                    self.file = Some(std::fs::File::from(fd));
                }
            }
            What::Symlink { target } => {
                rustix::fs::symlinkat(target.as_str(), &parent, leaf).map_err(exists)?;
            }
        }
        Ok(())
    }

    fn data(&mut self, bytes: &[u8], done: bool) -> Result<(), Refusal> {
        let file = self.file.as_mut().ok_or_else(|| Refusal::Invalid {
            what: "staged files",
            why: "file bytes came with no file under way".to_owned(),
        })?;
        // A full tmpfs (`ENOSPC`) is the guest's memory bound on staged files, returned as the error it is.
        file.write_all(bytes)
            .map_err(|e| Refusal::os("writing a staged file", e))?;
        if done {
            self.file = None;
        }
        Ok(())
    }

    /// Sets each directory's mode, deepest first so a directory made read-only doesn't stop its
    /// children's, then the root's.
    fn end(&mut self) -> Result<(), Refusal> {
        for (path, mode) in self.dirs.iter().rev() {
            let dir = open_in(&self.root, path)?;
            rustix::fs::fchmod(&dir, Mode::from_raw_mode((*mode).into()))
                .map_err(os("setting a staged directory's mode"))?;
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
            Err(Errno::LOOP | Errno::XDEV) => {
                return Err(Refusal::Invalid {
                    what: "staged files",
                    why: format!("{path:?} leads through a symbolic link or a mount"),
                });
            }
            Err(e) => {
                return Err(Refusal::Os {
                    op: "opening a staged directory",
                    why: format!("{path:?}: {e}"),
                });
            }
        }
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
        let mut reader = Dir::read_from(&fd).map_err(os("listing a staged directory"))?;
        while let Some(entry) = reader.next() {
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
