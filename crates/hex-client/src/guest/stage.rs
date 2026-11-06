//! The host's side of staging (docs/design/shards-guest.md §4a): a box's bundle and root, host paths
//! the guest can't see, packed onto the guest's staging port ([`files::FILES_PORT`]) before its create,
//! and their copy removed after its delete. The bytes are the core's ([`hex_core::stream::files`]); this
//! walks the host's files, flat and counted (CLAUDE.md §2, §11), and refuses by name what is never
//! staged: a device node, a FIFO, a socket, a name that isn't UTF-8.

use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::net::Shutdown;
use std::path::{Path, PathBuf};

use hex_api::{BoxId, Refusal};
use hex_core::stream::Deframer;
use hex_core::stream::files::{self, Entry, Limits, Op, What};
use hex_core::wire::{self, Answer, Received};

use super::{Duplex, Link};
use crate::ANSWER_PATIENCE;

/// Where the guest's relay stages each box: `<STAGED>/<box id>/` (`hex_node::relay::STAGED`).
pub const STAGED: &str = "/run/hex/staged";

/// One thing to stage, at the top of the box's staged root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Its name under the staged root: a plain name or a relative path.
    pub name: String,
    pub source: Source,
}

/// What an item is made from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A host path, a directory with everything under it, a file or a symbolic link.
    Path(PathBuf),
    /// A file of these bytes (a bundle's rewritten `config.json`), mode 0644.
    Bytes(Vec<u8>),
    /// A tarball (`.tar`, or gzip's `.tar.gz` or `.tgz`), staged from its own entries: their Linux modes
    /// and symlink texts exact, which a root unpacked on a host that keeps neither would lose (§4a).
    Tar(PathBuf),
}

/// Whether `path` names a tarball root, by its name: `.tar`, `.tar.gz` or `.tgz`.
pub fn is_tarball(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(".tar") || n.ends_with(".tar.gz") || n.ends_with(".tgz"))
}

/// The guest's path of `name` under `id`'s staged root, the relay staging under `staged`.
pub fn staged_path(staged: &str, id: &BoxId, name: &str) -> String {
    format!("{staged}/{id}/{name}")
}

/// Stages `items` for `id` through `link`, waiting for the relay's answer.
pub fn stage(link: &dyn Link, id: &BoxId, items: &[Item], limits: &Limits) -> Result<(), Refusal> {
    let mut conn = dial(link)?;
    let sent = {
        let mut out = BufWriter::with_capacity(64 << 10, Writer(conn.as_mut()));
        pack(&mut out, id, items, limits).and_then(|()| {
            out.flush()
                .map_err(|e| Refusal::os("sending staged files", e))
        })
    };
    // The relay answers even when it refused part way, and its answer says why better than a broken
    // pipe: read it first.
    let _ = conn.shutdown(Shutdown::Write);
    match (answer(conn.as_mut()), sent) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(why), _) | (Ok(()), Err(why)) => Err(why),
    }
}

/// Removes `id`'s staged copy through `link`. A copy that isn't there is [`Refusal::NoSuchBox`].
pub fn unstage(link: &dyn Link, id: &BoxId) -> Result<(), Refusal> {
    let mut conn = dial(link)?;
    let open = files::open(Op::Remove, id.as_str()).map_err(refused)?;
    conn.write_all(&open)
        .map_err(|e| Refusal::os("asking for a staged copy's removal", e))?;
    let _ = conn.shutdown(Shutdown::Write);
    answer(conn.as_mut())
}

fn dial(link: &dyn Link) -> Result<Box<dyn Duplex>, Refusal> {
    let conn = link.dial(files::FILES_PORT)?;
    conn.set_write_timeout(Some(ANSWER_PATIENCE))
        .and_then(|()| conn.set_read_timeout(Some(ANSWER_PATIENCE)))
        .map_err(|e| Refusal::os("bounding a staging connection", e))?;
    Ok(conn)
}

fn refused(why: files::Refused) -> Refusal {
    Refusal::Invalid {
        what: "staged files",
        why: why.to_string(),
    }
}

/// A `&mut dyn Duplex` as a writer a `BufWriter` can own.
struct Writer<'a>(&'a mut dyn Duplex);

impl Write for Writer<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

/// The relay's one answer: `Done`, or the refusal it carries.
fn answer(conn: &mut dyn Duplex) -> Result<(), Refusal> {
    let mut deframer = Deframer::new(wire::MAX_MESSAGE);
    let mut buf = [0u8; 4096];
    loop {
        let n = match conn.read(&mut buf) {
            Ok(0) => {
                return Err(Refusal::Broken(
                    "the guest's relay closed a staging connection without an answer".to_owned(),
                ));
            }
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Refusal::os("reading the relay's staging answer", e)),
        };
        let mut input = buf.get(..n).unwrap_or_default();
        while !input.is_empty() {
            let (took, whole) = deframer.take(input).map_err(|f| {
                Refusal::Broken(format!("the relay's staging answer isn't a message: {f}"))
            })?;
            input = input.get(took..).unwrap_or_default();
            let Some(message) = whole else { continue };
            let received = Received {
                bytes: &message,
                size: message.len(),
                descriptors: 0,
                descriptors_cut: false,
            };
            return match wire::read_answer(&received, wire::MAX_MESSAGE) {
                Ok((_, Answer::Done)) => Ok(()),
                Ok((_, Answer::Refused(why))) => Err(why),
                Ok((_, other)) => Err(Refusal::Broken(format!(
                    "the relay answered a stage with {other:?}"
                ))),
                Err(f) => Err(Refusal::Broken(format!(
                    "the relay's staging answer isn't one: {f}"
                ))),
            };
        }
    }
}

/// What the walk refuses: a path of a kind never staged, or one whose name can't be carried.
fn never(path: &Path, why: &str) -> Refusal {
    Refusal::Invalid {
        what: "staged files",
        why: format!("{}: {why}", path.display()),
    }
}

/// A file's mode, its permission bits. NTFS holds none, so on Windows every entry is staged 0755, as
/// WSL's DrvFs shows a file without metadata (its default `umask` 022 over 0777).
#[cfg(unix)]
fn mode(meta: &fs::Metadata) -> u16 {
    use std::os::unix::fs::PermissionsExt;
    u16::try_from(meta.permissions().mode() & u32::from(files::MODE_BITS)).unwrap_or(0o755)
}

#[cfg(not(unix))]
fn mode(_meta: &fs::Metadata) -> u16 {
    0o755
}

/// What kind of file a host path is, or why it's never staged.
enum Kind {
    Dir,
    File(u64),
    Symlink,
}

fn kind(path: &Path, meta: &fs::Metadata) -> Result<Kind, Refusal> {
    let t = meta.file_type();
    if t.is_symlink() {
        return Ok(Kind::Symlink);
    }
    if t.is_dir() {
        return Ok(Kind::Dir);
    }
    if t.is_file() {
        return Ok(Kind::File(meta.len()));
    }
    #[cfg(unix)]
    let what = {
        use std::os::unix::fs::FileTypeExt;
        if t.is_block_device() || t.is_char_device() {
            "a device node is never staged"
        } else if t.is_fifo() {
            "a FIFO is never staged"
        } else if t.is_socket() {
            "a socket is never staged"
        } else {
            "a file of no kind staging carries"
        }
    };
    #[cfg(not(unix))]
    let what = "a file of no kind staging carries";
    Err(never(path, what))
}

/// Writes `id`'s stage of `items` to `out`: its opening, every entry, and its end, within `limits`.
/// Each directory is walked flat, a queue of the directories found (CLAUDE.md §11), and every entry
/// counted against the stage's bound before it is sent.
pub fn pack(
    out: &mut dyn Write,
    id: &BoxId,
    items: &[Item],
    limits: &Limits,
) -> Result<(), Refusal> {
    let send = |out: &mut dyn Write, bytes: &[u8]| {
        out.write_all(bytes)
            .map_err(|e| Refusal::os("sending staged files", e))
    };
    send(out, &files::open(Op::Stage, id.as_str()).map_err(refused)?)?;
    let mut counts = Counts::default();
    for item in items {
        match &item.source {
            Source::Bytes(bytes) => {
                let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
                let entry = Entry {
                    path: item.name.clone(),
                    mode: 0o644,
                    what: What::File { size },
                };
                counts.add(size, limits)?;
                send(out, &files::head(&entry, limits).map_err(refused)?)?;
                send(out, bytes)?;
            }
            Source::Path(path) => walk(out, &item.name, path, limits, &mut counts)?,
            Source::Tar(path) => tarball(out, &item.name, path, limits, &mut counts)?,
        }
    }
    send(out, &files::END)
}

/// Entries and bytes sent so far, refused past the stage's bounds before they are sent.
#[derive(Default)]
struct Counts {
    entries: u64,
    bytes: u64,
}

impl Counts {
    fn add(&mut self, size: u64, limits: &Limits) -> Result<(), Refusal> {
        let over = |what: &'static str, bound: u64| Refusal::Exhausted {
            what,
            bound: usize::try_from(bound).unwrap_or(usize::MAX),
        };
        self.entries = self
            .entries
            .checked_add(1)
            .filter(|n| *n <= limits.entries)
            .ok_or_else(|| over("entries in one stage", limits.entries))?;
        self.bytes = self
            .bytes
            .checked_add(size)
            .filter(|n| *n <= limits.bytes)
            .ok_or_else(|| over("file bytes in one stage", limits.bytes))?;
        Ok(())
    }
}

/// Sends `path` as `name`, and everything under it.
fn walk(
    out: &mut dyn Write,
    name: &str,
    path: &Path,
    limits: &Limits,
    counts: &mut Counts,
) -> Result<(), Refusal> {
    // Directories found and not yet listed: their host path and staged name. A directory's entries
    // are sent after it, so the relay makes each parent first.
    let mut dirs: Vec<(PathBuf, String)> = Vec::new();
    one(out, name, path, limits, counts, &mut dirs)?;
    while let Some((dir, dir_name)) = dirs.pop() {
        let listing = fs::read_dir(&dir).map_err(|e| {
            Refusal::os(
                "listing a folder to stage",
                format!("{}: {e}", dir.display()),
            )
        })?;
        for entry in listing {
            let entry = entry.map_err(|e| {
                Refusal::os(
                    "listing a folder to stage",
                    format!("{}: {e}", dir.display()),
                )
            })?;
            let file_name = entry.file_name();
            let child = entry.path();
            let file_name = file_name
                .to_str()
                .ok_or_else(|| never(&child, "its name isn't UTF-8"))?;
            let child_name = format!("{dir_name}/{file_name}");
            one(out, &child_name, &child, limits, counts, &mut dirs)?;
        }
    }
    Ok(())
}

/// Sends one entry: a directory's listing is left to the walk.
fn one(
    out: &mut dyn Write,
    name: &str,
    path: &Path,
    limits: &Limits,
    counts: &mut Counts,
    dirs: &mut Vec<(PathBuf, String)>,
) -> Result<(), Refusal> {
    let meta = fs::symlink_metadata(path).map_err(|e| {
        Refusal::os(
            "reading a path to stage",
            format!("{}: {e}", path.display()),
        )
    })?;
    let mode = mode(&meta);
    let (what, size) = match kind(path, &meta)? {
        Kind::Dir => (What::Dir, 0),
        Kind::File(size) => (What::File { size }, size),
        Kind::Symlink => {
            let target = fs::read_link(path).map_err(|e| {
                Refusal::os(
                    "reading a link to stage",
                    format!("{}: {e}", path.display()),
                )
            })?;
            let target = target
                .to_str()
                .ok_or_else(|| never(path, "its target isn't UTF-8"))?
                .to_owned();
            (What::Symlink { target }, 0)
        }
    };
    counts.add(size, limits)?;
    let entry = Entry {
        path: name.to_owned(),
        mode,
        what,
    };
    let head = files::head(&entry, limits).map_err(refused)?;
    out.write_all(&head)
        .map_err(|e| Refusal::os("sending staged files", e))?;
    match entry.what {
        What::Dir => dirs.push((path.to_owned(), name.to_owned())),
        What::File { size } if size > 0 => copy(out, path, size)?,
        What::File { .. } | What::Symlink { .. } => {}
    }
    Ok(())
}

/// Sends exactly `size` bytes of the file at `path`: one that changed size while it was sent is refused,
/// as its stream would no longer say what it holds.
fn copy(out: &mut dyn Write, path: &Path, size: u64) -> Result<(), Refusal> {
    let file = File::open(path).map_err(|e| {
        Refusal::os(
            "opening a file to stage",
            format!("{}: {e}", path.display()),
        )
    })?;
    let sent = io::copy(&mut file.take(size), out)
        .map_err(|e| Refusal::os("sending a staged file", format!("{}: {e}", path.display())))?;
    if sent != size {
        return Err(never(path, "it shrank while it was staged"));
    }
    Ok(())
}

/// The most bytes read past a tarball's end-of-archive block, to reach a gzip stream's trailer and check
/// it: tar pads an archive to its record, 10,240 bytes by default (GNU tar's `--blocking-factor` 20), and
/// one this much longer is refused rather than read without end (CLAUDE.md §2).
const TRAILING: u64 = 1 << 20;

/// A tarball's bytes, counted as they're read, so its end is checked against its last entry's.
struct Counted<R> {
    inner: R,
    read: u64,
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read = self
            .read
            .saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
        Ok(n)
    }
}

/// Sends the tarball at `path` as `name`, a directory holding its entries in the archive's order: each one
/// counted against the stage's bounds before it is sent, each directory before what it holds, and every
/// fault refused naming the archive and the entry.
fn tarball(
    out: &mut dyn Write,
    name: &str,
    path: &Path,
    limits: &Limits,
    counts: &mut Counts,
) -> Result<(), Refusal> {
    let bad = |what: &str, why: &dyn std::fmt::Display| never(path, &format!("{what}: {why}"));
    let file = File::open(path).map_err(|e| bad("opening it", &e))?;
    let gzip = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(".gz") || n.ends_with(".tgz"));
    let reader: Box<dyn Read> = if gzip {
        Box::new(flate2::read::GzDecoder::new(io::BufReader::new(file)))
    } else {
        Box::new(io::BufReader::new(file))
    };
    let mut archive = tar::Archive::new(Counted {
        inner: reader,
        read: 0,
    });
    // The root itself: the archive's own `./` entry, if it has one, names nothing more.
    counts.add(0, limits)?;
    let root = Entry {
        path: name.to_owned(),
        mode: 0o755,
        what: What::Dir,
    };
    out.write_all(&files::head(&root, limits).map_err(refused)?)
        .map_err(|e| Refusal::os("sending staged files", e))?;
    let mut dirs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    // Where the last entry's data ends: the archive must go on past it to its end-of-archive block.
    let mut end: u64 = 0;
    for entry in archive.entries().map_err(|e| bad("reading it", &e))? {
        let mut entry = entry.map_err(|e| bad("reading an entry", &e))?;
        let raw = entry.path_bytes().into_owned();
        let shown = String::from_utf8_lossy(&raw).into_owned();
        let text = std::str::from_utf8(&raw).map_err(|_| bad(&shown, &"its name isn't UTF-8"))?;
        if text.starts_with('/') {
            return Err(bad(&shown, &"its name is absolute"));
        }
        let mut rel = text.trim_end_matches('/');
        while let Some(rest) = rel.strip_prefix("./") {
            rel = rest;
        }
        if rel.split('/').any(|part| part == "..") {
            return Err(bad(&shown, &"its name has a '..' component"));
        }
        let size = entry.size();
        end = entry
            .raw_file_position()
            .checked_add(size.div_ceil(512).saturating_mul(512))
            .ok_or_else(|| bad(&shown, &"its size is past any archive's"))?;
        if rel.is_empty() || rel == "." {
            continue;
        }
        let parent = rel.rsplit_once('/').map_or("", |(p, _)| p);
        if !parent.is_empty() && !dirs.contains(parent) {
            return Err(bad(&shown, &"it comes before its directory"));
        }
        let header = entry.header();
        let mode = u16::try_from(
            header.mode().map_err(|e| bad(&shown, &e))? & u32::from(files::MODE_BITS),
        )
        .unwrap_or(0o755);
        let what = match header.entry_type() {
            tar::EntryType::Directory => What::Dir,
            tar::EntryType::Regular | tar::EntryType::Continuous => What::File { size },
            tar::EntryType::Symlink => {
                let target = entry
                    .link_name_bytes()
                    .ok_or_else(|| bad(&shown, &"a symbolic link with no target"))?
                    .into_owned();
                let target = String::from_utf8(target)
                    .map_err(|_| bad(&shown, &"its target isn't UTF-8"))?;
                What::Symlink { target }
            }
            tar::EntryType::Char | tar::EntryType::Block => {
                return Err(bad(&shown, &"a device node is never staged"));
            }
            tar::EntryType::Fifo => return Err(bad(&shown, &"a FIFO is never staged")),
            tar::EntryType::Link => {
                return Err(bad(
                    &shown,
                    &"a hard link is never staged (none of the roots it serves holds one, §4a)",
                ));
            }
            _ => return Err(bad(&shown, &"an entry of a kind staging carries not")),
        };
        let size = match what {
            What::File { size } => size,
            What::Dir | What::Symlink { .. } => 0,
        };
        counts.add(size, limits)?;
        let staged = Entry {
            path: format!("{name}/{rel}"),
            mode,
            what,
        };
        out.write_all(&files::head(&staged, limits).map_err(refused)?)
            .map_err(|e| Refusal::os("sending staged files", e))?;
        match staged.what {
            What::Dir => {
                dirs.insert(rel.to_owned());
            }
            What::File { size } if size > 0 => {
                let sent =
                    io::copy(&mut (&mut entry).take(size), out).map_err(|e| bad(&shown, &e))?;
                if sent != size {
                    return Err(bad(&shown, &"the archive ends inside it"));
                }
            }
            What::File { .. } | What::Symlink { .. } => {}
        }
    }
    let mut counted = archive.into_inner();
    if counted.read < end.saturating_add(512) {
        return Err(bad("its end", &"it ends without its end-of-archive block"));
    }
    // Read to the stream's end, so a gzip trailer that lies about the archive is caught.
    let trailing = io::copy(
        &mut (&mut counted).take(TRAILING.saturating_add(1)),
        &mut io::sink(),
    )
    .map_err(|e| bad("its end", &e))?;
    if trailing > TRAILING {
        return Err(bad(
            "its end",
            &format!("more than {TRAILING} bytes follow its end-of-archive block"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tar_tests;
