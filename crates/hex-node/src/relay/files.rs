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
    files::{self, Entry, Limits, Meta, Mtime, Op, STORE, Step, Unpack, What},
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

/// Serves one staging connection: reads its op, does it, and answers `Done` or `Refused`. A refusal can come
/// with the host's stream still unread; closing on it then would reset the connection, which over vsock
/// discards the answer in flight, and the host would see a broken pipe, never why (CLAUDE.md §3). So a
/// refused connection is half-closed after its answer and read to its end, within what a stage may still
/// send ([`unread_bound`]), before it closes.
pub fn serve(mut conn: Box<dyn Duplex>, staged: &Path, limits: &Limits) {
    let answer = match take(conn.as_mut(), staged, limits) {
        Ok(()) => Answer::Done,
        Err(why) => Answer::Refused(why),
    };
    let refused = matches!(answer, Answer::Refused(_));
    if let Some(bytes) = wire::answer(wire::FIRST, 0, &answer, wire::MAX_MESSAGE)
        .ok()
        .and_then(|bytes| stream::frame(&bytes, wire::MAX_MESSAGE).ok())
    {
        let _ = conn.write_all(&bytes);
    }
    if refused {
        let _ = conn.shutdown(Shutdown::Write);
        drain(conn.as_mut(), unread_bound(staged, limits));
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
                (Step::Open { op, id, layers }, None) => {
                    let id = BoxId::new(&id)?;
                    match op {
                        Op::Remove => remove(staged, &id),
                        Op::Stage => {
                            let (begun, budget) = Stage::begin(staged, id, &layers)?;
                            stage = Some(begun);
                            // The packer learns the stage's budget, and which layers the guest holds,
                            // first; both sides spend against it.
                            unpack.learn(budget.entries, budget.bytes);
                            send_budget(conn, &budget)
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
fn send_budget(conn: &mut dyn Duplex, budget: &files::Budget) -> Result<(), Refusal> {
    let bytes = stream::frame(&files::budget(budget), wire::MAX_MESSAGE)
        .map_err(|e| Refusal::Broken(format!("framing a stage's budget: {e}")))?;
    conn.write_all(&bytes)
        .map_err(|e| Refusal::os("sending a stage's budget", e))
}

/// The most a stage's stream can still hold, learned as its budget is: the bytes the staged folder's file
/// system has free (file and xattr bytes), and for each entry it has inodes free for, a head, a path and a
/// link target at the path bound, and an owner and times. Past it the stream is no stage's, and the relay
/// closes on it.
fn unread_bound(staged: &Path, limits: &Limits) -> u64 {
    let Ok(dir) = rustix::fs::open(
        staged,
        OFlags::DIRECTORY | OFlags::RDONLY | OFlags::CLOEXEC,
        Mode::empty(),
    ) else {
        return 0;
    };
    let Ok(free) = free(&dir) else {
        return 0;
    };
    let entry = files::HEAD_BYTES
        .saturating_add(limits.path.saturating_mul(2))
        .saturating_add(files::META_BYTES);
    free.bytes
        .saturating_add(
            free.entries
                .saturating_mul(u64::try_from(entry).unwrap_or(u64::MAX)),
        )
        .saturating_add(u64::try_from(files::HEAD_BYTES).unwrap_or(u64::MAX))
}

/// Reads `conn` to its end, at most `bound` bytes, discarding them: what the host sent past a refusal.
fn drain(conn: &mut dyn Duplex, bound: u64) {
    let mut buf = vec![0u8; wire::MAX_MESSAGE];
    let mut read: u64 = 0;
    while read <= bound {
        match conn.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => read = read.saturating_add(u64::try_from(n).unwrap_or(u64::MAX)),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}

/// Removes a stage's partial copy, then returns why it failed.
fn fail(stage: Option<Stage>, why: Refusal) -> Result<(), Refusal> {
    if let Some(stage) = stage {
        let _ = stage.abandon();
    }
    Err(why)
}

/// A box's record of its root's layers, lowest first, a digest a line, in its staged folder: what makes a
/// layer referenced. Written when its stage opens, so a layer it names is never evicted under it.
const RECORD: &str = ".layers";
/// A layer being staged: `.partial-<relay pid>-<n>-<digest>`, renamed to `<digest>` only once whole, so a
/// stage cut or crashed mid-layer leaves no layer that counts as present.
const PARTIAL: &str = ".partial-";
/// Beside a layer `<digest>`, its `<digest>.size`: the entries and bytes it holds, what its eviction frees.
const SIZE: &str = ".size";

/// A stage under way: its root, the layer being applied, the file being written, and the directories whose
/// modes are set last.
struct Stage {
    staged: OwnedFd,
    id: BoxId,
    root: OwnedFd,
    /// The root's own path in the guest.
    root_path: String,
    /// The layer store, and the layers this stage names, by digest, lowest first, and which it is to stage.
    store: OwnedFd,
    wanted: Vec<String>,
    absent: VecDeque<usize>,
    /// The file being written, with what's applied once its last byte is (§4a's order).
    file: Option<(std::fs::File, String, u16, Option<Meta>)>,
    /// Each directory's mode and times, by where it is (in the layer under way, or under the box's root)
    /// and its resolved path there, the latest header's: set when its layer ends (or the stage, outside a
    /// layer), deepest first, after everything in it (`tar.go:330-337`).
    dirs: BTreeMap<(Base, String), (u16, Option<Timestamps>)>,
    /// The layer being applied, if any.
    layer: Option<Layer>,
    /// Entries made so far: what an abandoned stage's removal visits.
    entries: usize,
    /// What removals during the stage (a replaced entry, a whiteout) may visit.
    removals: Budget,
    /// The xattr that makes a layer's directory opaque to the overlay the guest's node mounts:
    /// `trusted.overlay.opaque` where the relay runs as root and the node mounts as root, else
    /// `user.overlay.opaque`, as a rootless node mounts with `userxattr` (Documentation/filesystems/
    /// overlayfs.rst, "Whiteouts and opaque directories"; `hex_core::bundle`'s `overlay`).
    opaque_xattr: &'static str,
    /// Layers this stage committed, counted for none to commit twice.
    seq: u64,
}

/// Where a directory whose mode and times are set last is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Base {
    Root,
    Layer,
}

/// A layer under way (§4a): its folder, which its entries' names resolve within as containerd's overlay
/// applier resolves them in a layer's own upper directory, the layers below it (nearest first), whose
/// directories a parent this layer makes copies its attributes from, and, in the store, where it's
/// committed once whole.
struct Layer {
    /// Its root's path in the stream, which its entries' paths begin with.
    path: String,
    fd: OwnedFd,
    /// Its folder's own path in the guest.
    base: String,
    lowers: Vec<OwnedFd>,
    /// In the store: its digest, its partial name, and the entries and bytes it holds so far.
    store: Option<Pending>,
}

struct Pending {
    digest: String,
    partial: String,
    entries: u64,
    bytes: u64,
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

/// What a removal under `dir` may visit: no more entries than its file system holds (statvfs(3)).
fn fs_budget(dir: &OwnedFd) -> Budget {
    Budget::new(free(dir).map_or(0, |f| usize::try_from(f.used).unwrap_or(usize::MAX)))
}

/// Whether `name` is a layer's name in the store: 64 lowercase hex digits.
fn is_layer_name(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Now, as utimensat(2) takes it: a layer's last use, by which the least recently used is evicted.
fn now() -> Timestamps {
    let now = Timespec {
        tv_sec: 0,
        tv_nsec: rustix::fs::UTIME_NOW,
    };
    Timestamps {
        last_access: now,
        last_modification: now,
    }
}

/// The names in the directory `dir`, read whole: a store or a staged folder, bounded by the file system's
/// own entries.
fn names(dir: &OwnedFd) -> Result<Vec<String>, Refusal> {
    let mut out = Vec::new();
    for entry in Dir::read_from(dir).map_err(os("listing a staged folder"))? {
        let entry = entry.map_err(os("listing a staged folder"))?;
        let name = entry.file_name();
        if name == c"." || name == c".." {
            continue;
        }
        if let Ok(name) = name.to_str() {
            out.push(name.to_owned());
        }
    }
    Ok(out)
}

/// Every layer some box's record names: the layers no eviction may take.
fn referenced(staged: &OwnedFd) -> Result<BTreeSet<String>, Refusal> {
    let mut held = BTreeSet::new();
    for name in names(staged)? {
        if name.starts_with('.') {
            continue;
        }
        let Ok(fd) = rustix::fs::openat(
            staged,
            format!("{name}/{RECORD}").as_str(),
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        ) else {
            continue;
        };
        let mut text = String::new();
        io::Read::read_to_string(&mut std::fs::File::from(fd), &mut text)
            .map_err(|e| Refusal::os("reading a box's layer record", e))?;
        held.extend(text.lines().filter(|l| is_layer_name(l)).map(str::to_owned));
    }
    Ok(held)
}

/// The entries and bytes a layer's size file says it holds, or none for a layer without one.
fn size_of(store: &OwnedFd, digest: &str) -> (u64, u64) {
    let Ok(fd) = rustix::fs::openat(
        store,
        format!("{digest}{SIZE}").as_str(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    ) else {
        return (0, 0);
    };
    let mut text = String::new();
    if io::Read::read_to_string(&mut std::fs::File::from(fd), &mut text).is_err() {
        return (0, 0);
    }
    let mut parts = text
        .split_whitespace()
        .map(|p| p.parse::<u64>().unwrap_or(0));
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0))
}

/// Writes `text` as `name` under `dir` whole: to a temporary name, then renamed over it, so a reader never
/// sees part of it.
fn write_whole(dir: &OwnedFd, name: &str, text: &str) -> Result<(), Refusal> {
    let tmp = format!(".tmp-{}-{name}", std::process::id());
    let fd = rustix::fs::openat(
        dir,
        tmp.as_str(),
        OFlags::CREATE | OFlags::TRUNC | OFlags::WRONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(OWNER_FILE),
    )
    .map_err(os("writing a staged record"))?;
    std::fs::File::from(fd)
        .write_all(text.as_bytes())
        .map_err(|e| Refusal::os("writing a staged record", e))?;
    rustix::fs::renameat(dir, tmp.as_str(), dir, name).map_err(os("writing a staged record"))
}

impl Stage {
    /// Makes `<staged>/<id>`, which must not exist: one there is another box's, or a copy the host
    /// hasn't removed, and staging over it would change what that box runs. Records the layers `layers`
    /// names in it, says which the store holds, and what the stage may spend.
    fn begin(
        staged: &Path,
        id: BoxId,
        layers: &[files::Digest],
    ) -> Result<(Self, files::Budget), Refusal> {
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
        match rustix::fs::mkdirat(&staged, STORE, Mode::from_raw_mode(OWNER_DIR)) {
            Ok(()) | Err(Errno::EXIST) => {}
            Err(e) => return Err(Refusal::os("making the layer store", e)),
        }
        let store = rustix::fs::openat(
            &staged,
            STORE,
            OFlags::DIRECTORY | OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(os("opening the layer store"))?;
        let wanted: Vec<String> = layers.iter().map(files::digest_hex).collect();
        // The record first, so no eviction takes a layer this stage names while it stages.
        if !wanted.is_empty() {
            write_whole(&root, RECORD, &(wanted.join("\n") + "\n"))?;
        }
        // A partial another relay process left (one that ended mid-layer) is no layer: removed.
        let me = format!("{PARTIAL}{}-", std::process::id());
        for name in names(&store)? {
            if name.starts_with(PARTIAL) && !name.starts_with(&me) {
                let mut budget = fs_budget(&store);
                let _ = remove_tree(&store, &name, &mut budget);
            }
        }
        let mut present = Vec::with_capacity(wanted.len());
        let mut absent = VecDeque::new();
        for (i, digest) in wanted.iter().enumerate() {
            let held = rustix::fs::statat(&store, digest.as_str(), AtFlags::SYMLINK_NOFOLLOW)
                .is_ok_and(|st| FileType::from_raw_mode(st.st_mode) == FileType::Directory);
            if held {
                // Its last use, for the least recently used to go first.
                let _ = rustix::fs::utimensat(
                    &store,
                    digest.as_str(),
                    &now(),
                    AtFlags::SYMLINK_NOFOLLOW,
                );
            } else {
                absent.push_back(i);
            }
            present.push(held);
        }
        let free = free(&staged)?;
        // What unreferenced layers hold counts as free: the store evicts them to make room.
        let held = referenced(&staged)?;
        let (mut entries, mut bytes) = (free.entries, free.bytes);
        for name in names(&store)? {
            if is_layer_name(&name) && !held.contains(&name) {
                let (e, b) = size_of(&store, &name);
                entries = entries.saturating_add(e);
                bytes = bytes.saturating_add(b);
            }
        }
        let opaque_xattr = if rustix::process::geteuid().is_root() {
            "trusted.overlay.opaque"
        } else {
            "user.overlay.opaque"
        };
        Ok((
            Self {
                staged,
                id,
                root,
                root_path,
                store,
                wanted,
                absent,
                file: None,
                dirs: BTreeMap::new(),
                layer: None,
                entries: 0,
                // A removal visits no more entries than the file system holds.
                removals: Budget::new(usize::try_from(free.used).unwrap_or(usize::MAX)),
                opaque_xattr,
                seq: 0,
            },
            files::Budget {
                entries,
                bytes,
                present,
            },
        ))
    }

    /// Where the directory `fd` is under the layer under way, or else under the box's root, by the
    /// kernel's own resolution of it.
    fn resolved(&self, fd: &OwnedFd) -> Result<String, Refusal> {
        let path = fd_path(fd)?;
        let base = self
            .layer
            .as_ref()
            .map_or(self.root_path.as_str(), |l| l.base.as_str());
        let rest = path.strip_prefix(base).ok_or_else(|| {
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

    /// The directory `rel` within the layer's folder, as containerd's overlay applier finds it: it unpacks
    /// a layer into its own upper directory (`core/diff/apply/apply_linux.go:36-55` at v2.3.6), so
    /// `fs.RootPath` follows only links this layer made, inside the layer; a name this layer didn't make
    /// isn't there, and mkparent makes a directory there (`pkg/archive/tar.go:448-503`), copying the owner,
    /// mode, times and xattrs but `trusted.*` of the nearest layer below whose view of the name is a
    /// directory (`WithParents`, `:482-500`; `copyDirInfo`, `copyUpXAttrs`, `tar_unix.go:143-203`), else
    /// 0755. A name missing (when not `make`) is refused. Flat: one step per component, at most
    /// [`MAX_LINKS`] links followed, each component opened `O_NOFOLLOW`.
    fn walk(&mut self, rel: &str, make: bool) -> Result<OwnedFd, Refusal> {
        let layer = self
            .layer
            .as_ref()
            .ok_or_else(|| invalid(format!("{rel:?} is in no layer")))?;
        let top = layer
            .fd
            .try_clone()
            .map_err(|e| Refusal::os("opening a layer's root", e))?;
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
                    open_in(&top, &at.join("/"))?
                };
                continue;
            }
            let below = if at.is_empty() {
                part.clone()
            } else {
                format!("{}/{part}", at.join("/"))
            };
            match rustix::fs::statat(&fd, part.as_str(), AtFlags::SYMLINK_NOFOLLOW)
                .map(|st| FileType::from_raw_mode(st.st_mode))
            {
                Ok(FileType::Directory) => {}
                Ok(FileType::Symlink) => {
                    links = links.saturating_add(1);
                    if links > MAX_LINKS {
                        return Err(invalid(format!(
                            "{rel:?} follows more than {MAX_LINKS} links"
                        )));
                    }
                    let target = rustix::fs::readlinkat(&fd, part.as_str(), Vec::new())
                        .map_err(os("reading a staged link"))?
                        .into_string()
                        .map_err(|_| invalid(format!("{below:?}'s target isn't UTF-8")))?;
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
                Ok(_) => {
                    return Err(invalid(format!(
                        "{rel:?} leads through {below:?}, which this layer made a file"
                    )));
                }
                Err(Errno::NOENT) if !make => {
                    return Err(invalid(format!(
                        "{rel:?} isn't staged: {below:?} is missing"
                    )));
                }
                Err(Errno::NOENT) => {
                    self.room(|| {
                        rustix::fs::mkdirat(&fd, part.as_str(), Mode::from_raw_mode(MKPARENT_MODE))
                    })
                    .map_err(|e| invalid(format!("making {below:?}, a parent: {e}")))?;
                    let dir = rustix::fs::openat(
                        &fd,
                        part.as_str(),
                        OFlags::DIRECTORY | OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    )
                    .map_err(os("opening a parent directory staging made"))?;
                    match self.below(&below) {
                        Some(from) => copy_dir_info(&from, &dir)?,
                        None => rustix::fs::fchmod(&dir, Mode::from_raw_mode(MKPARENT_MODE))
                            .map_err(os("setting a made parent's mode"))?,
                    }
                }
                Err(e) => {
                    return Err(Refusal::Os {
                        op: "walking a staged path",
                        why: format!("{below:?}: {e}"),
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

    /// The nearest layer below's view of `rel` when it's a directory, whose attributes a parent mkparent
    /// makes copies; none when no layer below holds it, or the nearest holds no directory there
    /// ("Replaced, do not copy attributes", `tar.go:490-492`).
    fn below(&self, rel: &str) -> Option<OwnedFd> {
        let layer = self.layer.as_ref()?;
        for lower in &layer.lowers {
            match within(lower, rel) {
                Ok(dir) => return Some(dir),
                Err(Errno::NOENT) => {}
                Err(_) => return None,
            }
        }
        None
    }

    /// Whether `rel` is in a layer below this one: a hard link to it is an earlier layer's.
    fn in_lower(&self, rel: &str) -> bool {
        let (parent, leaf) = split(rel);
        self.layer.as_ref().is_some_and(|layer| {
            layer.lowers.iter().any(|lower| {
                within(lower, parent).is_ok_and(|dir| {
                    rustix::fs::statat(&dir, leaf, AtFlags::SYMLINK_NOFOLLOW).is_ok()
                })
            })
        })
    }

    /// Runs `op`, evicting the least recently used layer no box names while the file system says it has
    /// no room (`ENOSPC`), until `op` fits or none is left to evict.
    fn room<T>(&self, mut op: impl FnMut() -> Result<T, Errno>) -> Result<T, Errno> {
        loop {
            match op() {
                Err(Errno::NOSPC) => {
                    if !self.evict_one().unwrap_or(false) {
                        return Err(Errno::NOSPC);
                    }
                }
                done => return done,
            }
        }
    }

    /// Evicts the least recently used layer no box's record names (its last use its folder's mtime);
    /// `false` when there is none.
    fn evict_one(&self) -> Result<bool, Refusal> {
        let held = referenced(&self.staged)?;
        let mut oldest: Option<(i64, i64, String)> = None;
        for name in names(&self.store)? {
            if !is_layer_name(&name) || held.contains(&name) {
                continue;
            }
            let Ok(st) = rustix::fs::statat(&self.store, name.as_str(), AtFlags::SYMLINK_NOFOLLOW)
            else {
                continue;
            };
            let when = (st.st_mtime, i64::try_from(st.st_mtime_nsec).unwrap_or(0));
            if oldest.as_ref().is_none_or(|(s, n, _)| when < (*s, *n)) {
                oldest = Some((when.0, when.1, name));
            }
        }
        let Some((_, _, name)) = oldest else {
            return Ok(false);
        };
        let mut budget = fs_budget(&self.store);
        remove_tree(&self.store, &name, &mut budget)?;
        let _ = rustix::fs::unlinkat(
            &self.store,
            format!("{name}{SIZE}").as_str(),
            AtFlags::empty(),
        );
        Ok(true)
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

    /// Counts an entry's bytes into the layer under way's size, what its eviction frees.
    fn count(&mut self, bytes: u64) {
        if let Some(pending) = self.layer.as_mut().and_then(|l| l.store.as_mut()) {
            pending.entries = pending.entries.saturating_add(1);
            pending.bytes = pending.bytes.saturating_add(bytes);
        }
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
        let base = if layered { Base::Layer } else { Base::Root };
        let (parent, leaf) = self.parent(&entry.path, true)?;
        let at = join(&self.resolved(&parent)?, leaf);
        let in_store = self.layer.as_ref().is_some_and(|l| l.store.is_some());
        self.count(
            match entry.what {
                What::File { size } => size,
                _ => 0,
            }
            .saturating_add(entry.meta.as_ref().map_or(0, files::xattr_bytes)),
        );
        if matches!(entry.what, What::Whiteout) {
            if !layered {
                return Err(invalid(format!("whiteout {:?} is in no layer", entry.path)));
            }
            if !in_store {
                // A root applied in place, with nothing below it: the whiteout removes what this layer
                // made, as containerd's naive applier's `os.RemoveAll` (`tar.go:218-222`).
                return self.remove(&parent, leaf, &at);
            }
            // In the store, a whiteout is overlayfs's own: a character device 0/0 hiding the name below
            // (containerd's `OverlayConvertWhiteout`, pkg/archive/tar_opts_linux.go; Documentation/
            // filesystems/overlayfs.rst, "Whiteouts and opaque directories"); one over a name this layer
            // made is refused, as its mknod fails there (EEXIST).
            return self
                .room(|| {
                    rustix::fs::mknodat(
                        &parent,
                        leaf,
                        FileType::CharacterDevice,
                        Mode::empty(),
                        rustix::fs::makedev(0, 0),
                    )
                })
                .map_err(|e| invalid(format!("whiteout {:?}: {e}", entry.path)));
        }
        if layered {
            // A layer's entry replaces what this layer put at its path, but a directory onto a directory
            // merges and takes this header's owner, mode, xattrs and times (`tar.go:304-311`).
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
                match self
                    .room(|| rustix::fs::mkdirat(&parent, leaf, Mode::from_raw_mode(OWNER_DIR)))
                {
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
                self.dirs
                    .insert((base, at.clone()), (entry.mode, meta.map(times)));
            }
            What::File { size } => {
                let fd = self
                    .room(|| {
                        rustix::fs::openat(
                            &parent,
                            leaf,
                            OFlags::CREATE
                                | OFlags::EXCL
                                | OFlags::WRONLY
                                | OFlags::NOFOLLOW
                                | OFlags::CLOEXEC,
                            Mode::from_raw_mode(OWNER_FILE),
                        )
                    })
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
                self.room(|| rustix::fs::symlinkat(target.as_str(), &parent, leaf))
                    .map_err(exists)?;
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
                // The target is resolved as an entry's path is (within this layer's folder, as
                // containerd's `hardlinkRootPath` does in the upper directory, `tar.go:795-807`); linkat(2)
                // follows no link at its last component, as containerd's `link` (os.Link) doesn't, so a
                // link names the link itself. A target in an earlier layer, or none, is refused (ENOENT
                // in this layer; ctr, lab:hardlink), and a directory, which link(2) refuses, by name.
                let rel = self
                    .in_layer(target)
                    .map(|(_, rel)| rel.to_owned())
                    .unwrap_or_default();
                let earlier = || {
                    invalid(format!(
                        "hard link {:?} names {target:?}, a file an earlier layer made, which \
                         containerd's overlay unpack can't link (ENOENT in this layer)",
                        entry.path
                    ))
                };
                let (from, from_leaf) = match self.parent(target, false) {
                    Ok(found) => found,
                    Err(_) if layered && self.in_lower(&rel) => return Err(earlier()),
                    Err(why) => return Err(why),
                };
                let st = match rustix::fs::statat(&from, from_leaf, AtFlags::SYMLINK_NOFOLLOW) {
                    Ok(st) => st,
                    Err(_) if layered && self.in_lower(&rel) => return Err(earlier()),
                    Err(e) => {
                        return Err(invalid(format!(
                            "hard link {:?} names {target:?}, which isn't staged: {e}",
                            entry.path
                        )));
                    }
                };
                if !matches!(
                    FileType::from_raw_mode(st.st_mode),
                    FileType::RegularFile | FileType::Symlink
                ) {
                    return Err(invalid(format!(
                        "hard link {:?} names {target:?}, which is neither a file nor a link",
                        entry.path
                    )));
                }
                self.room(|| rustix::fs::linkat(&from, from_leaf, &parent, leaf, AtFlags::empty()))
                    .map_err(exists)?;
            }
            What::Layer | What::Whiteout | What::Opaque => {}
        }
        Ok(())
    }

    /// A layer begins at `path`. In a stage naming its layers, it is the next the store lacks, staged
    /// into a partial folder of its own over the layers below it; else (a root of one copy) it applies in
    /// place, into `path`, a directory already staged, with nothing below.
    fn layer(&mut self, path: &str) -> Result<(), Refusal> {
        self.end_layer()?;
        if self.wanted.is_empty() {
            // A root of one copy (a bundle's, a `Rootfs::Dir`'s) is one tarball: one layer.
            if self.seq > 0 {
                return Err(invalid(
                    "a root staged as one copy is one layer; a stage of more names them".to_owned(),
                ));
            }
            self.seq = 1;
            let fd = open_in(&self.root, path)?;
            let base = fd_path(&fd)?;
            self.layer = Some(Layer {
                path: path.to_owned(),
                fd,
                base,
                lowers: Vec::new(),
                store: None,
            });
            return Ok(());
        }
        let i = self
            .absent
            .pop_front()
            .ok_or_else(|| invalid("the stage sent more layers than the guest lacks".to_owned()))?;
        let digest = self
            .wanted
            .get(i)
            .cloned()
            .ok_or_else(|| invalid("a layer out of the stage's list".to_owned()))?;
        self.seq = self.seq.saturating_add(1);
        let partial = format!("{PARTIAL}{}-{}-{digest}", std::process::id(), self.seq);
        self.room(|| {
            rustix::fs::mkdirat(
                &self.store,
                partial.as_str(),
                Mode::from_raw_mode(OWNER_DIR),
            )
        })
        .map_err(os("making a layer's folder"))?;
        let fd = rustix::fs::openat(
            &self.store,
            partial.as_str(),
            OFlags::DIRECTORY | OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(os("opening a layer's folder"))?;
        let base = fd_path(&fd)?;
        // The layers below, nearest first: each in the store already (held, or committed by this stage).
        let mut lowers = Vec::with_capacity(i);
        for below in self.wanted.iter().take(i).rev() {
            lowers.push(
                rustix::fs::openat(
                    &self.store,
                    below.as_str(),
                    OFlags::DIRECTORY | OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(os("opening a layer below"))?,
            );
        }
        self.layer = Some(Layer {
            path: path.to_owned(),
            fd,
            base,
            lowers,
            store: Some(Pending {
                digest,
                partial,
                entries: 0,
                bytes: 0,
            }),
        });
        Ok(())
    }

    /// Finishes the layer under way: its directories' modes and times, and, in the store, its commit: its
    /// size beside it, then its partial folder renamed to its digest, whole. Another stage's commit of the
    /// same layer first leaves this one's partial to go.
    fn end_layer(&mut self) -> Result<(), Refusal> {
        self.finish_dirs()?;
        let Some(layer) = self.layer.take() else {
            return Ok(());
        };
        let Some(pending) = layer.store else {
            return Ok(());
        };
        rustix::fs::fchmod(&layer.fd, Mode::from_raw_mode(ROOT_MODE.into()))
            .map_err(os("setting a layer's root mode"))?;
        write_whole(
            &self.store,
            &format!("{}{SIZE}", pending.digest),
            &format!("{} {}\n", pending.entries, pending.bytes),
        )?;
        match rustix::fs::renameat(
            &self.store,
            pending.partial.as_str(),
            &self.store,
            pending.digest.as_str(),
        ) {
            Ok(()) => {}
            Err(Errno::EXIST | Errno::NOTEMPTY) => {
                let mut budget = fs_budget(&self.store);
                remove_tree(&self.store, &pending.partial, &mut budget)?;
            }
            Err(e) => return Err(Refusal::os("committing a staged layer", e)),
        }
        let _ = rustix::fs::utimensat(
            &self.store,
            pending.digest.as_str(),
            &now(),
            AtFlags::SYMLINK_NOFOLLOW,
        );
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
        let base = if self.layer.is_some() {
            Base::Layer
        } else {
            Base::Root
        };
        let under = format!("{at}/");
        self.dirs.remove(&(base, at.to_owned()));
        let gone: Vec<(Base, String)> = self
            .dirs
            .range((base, under.clone())..)
            .map(|(k, _)| k)
            .take_while(|(b, k)| *b == base && k.starts_with(&under))
            .cloned()
            .collect();
        for k in gone {
            self.dirs.remove(&k);
        }
        Ok(())
    }

    /// An opaque marker. In the store, overlayfs's own: the directory gets the opaque xattr, hiding what
    /// the layers below hold there (containerd's `OverlayConvertWhiteout`; overlayfs.rst). In place, with
    /// nothing below, it hides nothing.
    fn opaque(&mut self, path: &str) -> Result<(), Refusal> {
        let Some(rel) = self.in_layer(path).map(|(_, rel)| rel.to_owned()) else {
            return Err(invalid(format!("opaque marker {path:?} is in no layer")));
        };
        if self.layer.as_ref().is_none_or(|l| l.store.is_none()) {
            return Ok(());
        }
        let dir = self.walk(&rel, true)?;
        let name = self.opaque_xattr;
        rustix::fs::fsetxattr(&dir, name, b"y", rustix::fs::XattrFlags::empty())
            .map_err(|e| refused_xattr(path, name, e))
    }

    fn data(&mut self, bytes: &[u8], done: bool) -> Result<(), Refusal> {
        let mut rest = bytes;
        while !rest.is_empty() {
            let (file, ..) = self
                .file
                .as_mut()
                .ok_or_else(|| invalid("file bytes came with no file under way".to_owned()))?;
            match io::Write::write(file, rest) {
                Ok(0) => return Err(invalid("a staged file took no bytes".to_owned())),
                Ok(n) => rest = rest.get(n..).unwrap_or_default(),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                // A full file system evicts the least recently used unreferenced layer and tries again;
                // with none left, `ENOSPC` is returned as the error it is.
                Err(e) if e.raw_os_error() == Some(Errno::NOSPC.raw_os_error()) => {
                    if !self.evict_one()? {
                        return Err(Refusal::os("writing a staged file", e));
                    }
                }
                Err(e) => return Err(Refusal::os("writing a staged file", e)),
            }
        }
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
        let dirs = std::mem::take(&mut self.dirs);
        for ((base, path), (mode, times)) in dirs.iter().rev() {
            let top = match base {
                Base::Root => &self.root,
                Base::Layer => match self.layer.as_ref() {
                    Some(layer) => &layer.fd,
                    None => continue,
                },
            };
            let dir = open_in(top, if path.is_empty() { "." } else { path })?;
            rustix::fs::fchmod(&dir, Mode::from_raw_mode((*mode).into()))
                .map_err(os("setting a staged directory's mode"))?;
            if let Some(times) = times {
                rustix::fs::futimens(&dir, times)
                    .map_err(os("setting a staged directory's times"))?;
            }
        }
        Ok(())
    }

    /// Finishes the last layer, and the stage: every layer the store lacked sent, and the root's mode.
    fn end(&mut self) -> Result<(), Refusal> {
        self.end_layer()?;
        if !self.absent.is_empty() {
            return Err(invalid(format!(
                "the stage ended {} layers short of what the guest lacks",
                self.absent.len()
            )));
        }
        rustix::fs::fchmod(&self.root, Mode::from_raw_mode(ROOT_MODE.into()))
            .map_err(os("setting a staged root's mode"))
    }

    fn abandon(mut self) -> Result<(), Refusal> {
        drop(self.file.take());
        if let Some(pending) = self.layer.take().and_then(|l| l.store) {
            let mut budget = fs_budget(&self.store);
            let _ = remove_tree(&self.store, &pending.partial, &mut budget);
        }
        let mut budget = Budget::new(self.entries.saturating_add(1));
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

#[cfg(test)]
mod store_tests;
