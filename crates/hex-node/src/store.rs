//! The node agent's store of box records, one file each. A record is kept in one of two places, by
//! what it names, and how it is written follows. One naming a directory on a filesystem a host crash
//! leaves behind must outlive the host, so it is kept in the node's state directory, fsynced file and
//! directory, and written whole to a temp file and renamed into place: a crash of the node agent
//! leaves either the old record or the new one, never a torn one (T-T7.1.2), because the page cache
//! outlives the process and rename(2) replaces atomically. Every other record names only directories
//! on tmpfs (an `oci` box's container state, under the node's runtime directory) or none (a `process`
//! box's), so a host crash leaves nothing it describes; it is kept beside that state, in a directory of
//! the node's own under the runtime directory, and never synced, where a record-sized write measured
//! about a seventh of one on the overlay or btrfs (bench/competitive/probes/levers.md; goal 5 extended
//! step 5, research f92 and f93).
//!
//! A box's record is written once over its create (goal 5 extended step 5), so that one write to a
//! non-durable record goes straight to its file with `O_EXCL` — no temp, no rename (criterion 3) — and
//! only a later write of the same record, an update, takes the temp-and-rename path every durable
//! record takes. Writing in place opens a window a durable record never has: a node crash between the
//! file's open and the write leaves a torn record, short of the trailing newline its encoding ends on,
//! that never decodes. Recovery covers it. A torn record names only tmpfs directories, so a host crash
//! takes it with everything it described; a node-only crash leaves it on tmpfs, where recovery reads
//! it, fails to decode it, and removes its box as unreadable (recovery's `Why::Unreadable`). So
//! recovery scrubs each record it finds, torn or whole, and only a record naming a directory on disk —
//! always written whole — points at anything a host crash leaves behind to remove
//! (docs/design/runtime-architecture.md, decision 1).
//!
//! Changes of one box's record are made one at a time; changes of different boxes never wait on each
//! other, so one box's fsync never holds up another box's create (goal 5 plan step 2). Nor do they wait on
//! one directory's lock: the runtime directory's records are spread over [`SHARDS`] subdirectories by
//! their box's id (goal 5 extended, step 6).

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(all(debug_assertions, target_os = "linux"))]
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use hex_api::{BoxId, Refusal};
use hex_core::patience::RFC_6298;
use hex_core::record::BoxRecord;

use crate::turns::{Turn, Turns};

/// How many subdirectories the runtime directory's records are spread over, by their box's id: 64, the shard's
/// name two hex digits of [`SHARD_MASK`]. Each create makes its box's record file and each delete removes it, and
/// a directory makes a name or removes one only under its own inode's lock, held exclusively (Linux v6.19
/// fs/namei.c:4545-4546 for an open that may create, :2873 for an unlink, from :5429), so with every record in
/// one directory the boxes made and deleted at once queued on it: 0.548 ms a lifecycle waiting to write it and
/// 0.189 to read it at concurrency 32 on fedora's E-cores (bench/competitive/probes/create_serial.md, run 19).
/// A stated bound, not a tuning: 64 directories, made as the store opens.
pub const SHARDS: usize = 64;
/// The bits of a box id's hash that name its shard, [`SHARDS`] of them.
const SHARD_MASK: u64 = 0x3f;

/// Most boxes whose records are being changed at once: one for each box the node can hold, its box
/// budget ([`hex_runtime::MAX_BOXES`]), so the changes of the boxes a node holds never reach it. Past it
/// a change is refused by type ([`Refusal::Exhausted`]), never kept waiting (CLAUDE.md §2).
pub const TURNS: usize = hex_runtime::MAX_BOXES;

/// Box records, one file each, named `<id>.json`, in the place each belongs ([`Store`]).
#[derive(Debug, Clone)]
pub struct Store {
    /// Where the records that must outlive the host are kept, each fsynced: the node's state
    /// directory.
    disk: PathBuf,
    /// Where every other record is kept, none synced: a directory of the node's own under its runtime
    /// directory.
    run: PathBuf,
    /// Two descriptors per store, shared by clones. Relative names keep the runtime directory's
    /// prefix out of each record's open and rename (decision 1, goal 5 extended step 5).
    #[cfg(target_os = "linux")]
    disk_fd: Arc<File>,
    #[cfg(target_os = "linux")]
    run_fd: Arc<File>,
    /// The boxes whose records are being changed now, one change each: [`Store::put`],
    /// [`Store::update`] and [`Store::remove`] of a box wait for the change of it under way, so an
    /// update's read and write of a record are one change and two users changing one record at once
    /// (the supervisor recording an init's exit, the socket counting an abandoned program) never lose
    /// either's change. A change of another box never waits. At most [`TURNS`] of them.
    turns: Arc<Turns>,
    /// The fsyncs the store has made since it opened.
    synced: Arc<Synced>,
    /// A debug build's one crash point in the store: once armed, the node ends its own process after
    /// it opens a non-durable record's file and before it writes, so T-T7.1.2's crash test leaves a
    /// torn record ([`crate::Node::with_crash_point`]). Shared by clones, as every field is.
    #[cfg(all(debug_assertions, target_os = "linux"))]
    crash_before_write: Arc<AtomicBool>,
}

#[derive(Debug, Default)]
struct Synced {
    files: AtomicU64,
    dirs: AtomicU64,
}

/// The fsyncs a store has made since it opened: of record files and of its directory, one of each per
/// write of a record that must outlive the host. Non-vacuity counters (CLAUDE.md §6) for the tests of
/// which records are durable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Syncs {
    pub files: u64,
    pub dirs: u64,
}

impl Store {
    /// Opens the store over the node's state directory `disk`, for the records that must outlive the
    /// host, and `run`, a directory of the node's own under its runtime directory for the rest, making
    /// each when it's missing. `run` is readable by the node alone: a record holds its box's spec, an
    /// init's environment among it.
    pub fn open(disk: &Path, run: &Path) -> Result<Self, Refusal> {
        fs::create_dir_all(disk).map_err(|e| Refusal::os("making the record directory", e))?;
        fs::create_dir_all(run)
            .and_then(|()| owner_only(run))
            .map_err(|e| Refusal::os("making the runtime record directory", e))?;
        for n in 0..=SHARD_MASK {
            let shard = run.join(format!("{n:02x}"));
            fs::create_dir_all(&shard)
                .and_then(|()| owner_only(&shard))
                .map_err(|e| Refusal::os("making a shard of the runtime record directory", e))?;
        }
        Ok(Self {
            disk: disk.to_owned(),
            run: run.to_owned(),
            #[cfg(target_os = "linux")]
            disk_fd: Arc::new(open_directory(disk)?),
            #[cfg(target_os = "linux")]
            run_fd: Arc::new(open_directory(run)?),
            // A change waits for the change of the same box before it: that one's write and its fsyncs,
            // which take milliseconds. How long is learned from the changes made (decision 9), so a disk that
            // has stopped answering is reported as a fault, never waited on for ever (CLAUDE.md §4).
            turns: Arc::new(Turns::new(
                "changing a box's record",
                "record changes under way",
                RFC_6298,
                TURNS,
            )),
            synced: Arc::default(),
            #[cfg(all(debug_assertions, target_os = "linux"))]
            crash_before_write: Arc::default(),
        })
    }

    /// Arms this store's one crash point ([`Store::crash_before_write`]): a debug build's node ends its
    /// own process the next time it opens a non-durable record's file, before it writes, so T-T7.1.2's
    /// crash test leaves a torn record for recovery. Debug builds only.
    #[cfg(all(debug_assertions, target_os = "linux"))]
    pub fn crash_before_first_write(&self) {
        self.crash_before_write.store(true, Ordering::Relaxed);
    }

    /// Box `id`'s turn to change its record: waits for a change of the same box under way, never for
    /// another box's, and refuses past the wait the store's changes taught it, or past [`TURNS`] boxes
    /// changed at once.
    fn claim(&self, id: &BoxId) -> Result<Turn<'_>, Refusal> {
        self.turns.take(id)
    }

    /// How many changes of a box's record have waited for a change of the same box to finish.
    pub fn waited(&self) -> u64 {
        self.turns.waited()
    }

    /// Where `record` is kept, and whether it's fsynced there: the state directory for a record that
    /// must outlive the host ([`must_outlive_host`]), else the runtime directory's.
    fn place(&self, record: &BoxRecord) -> (&Path, bool) {
        if must_outlive_host(record) {
            (&self.disk, true)
        } else {
            (&self.run, false)
        }
    }

    /// Box `id`'s record files in the two places it may be kept: the runtime directory's, in its shard, then the
    /// state directory's. A record a node before step 6 left in the runtime directory itself is in neither:
    /// recovery moves it to its shard first ([`Store::settle`]), so no create or delete reads that directory.
    fn files(&self, id: &BoxId) -> [PathBuf; 2] {
        [
            record_file(&self.run.join(shard(id)), id),
            record_file(&self.disk, id),
        ]
    }

    /// The directories records are found in, each listed whole by recovery: the state directory, the runtime
    /// directory (a node's before step 6), then each of its shards.
    fn listed(&self) -> Vec<PathBuf> {
        let mut dirs = vec![self.disk.clone(), self.run.clone()];
        dirs.extend((0..=SHARD_MASK).map(|n| self.run.join(format!("{n:02x}"))));
        dirs
    }

    /// The fsyncs the store has made since it opened.
    pub fn syncs(&self) -> Syncs {
        Syncs {
            files: self.synced.files.load(Ordering::Relaxed),
            dirs: self.synced.dirs.load(Ordering::Relaxed),
        }
    }

    /// Writes `record` whole, in its place: either the old file or the new one survives a crash of
    /// the node agent, and a record that must outlive the host ([`must_outlive_host`]) is fsynced too.
    pub fn put(&self, record: &BoxRecord) -> Result<(), Refusal> {
        let _turn = self.claim(&record.spec.id)?;
        self.write(record)
    }

    /// Changes `id`'s record by `change`, which says whether it changed anything, and writes it if it
    /// did, as one change among the box's; `false` when there's no readable record or nothing changed.
    pub fn update(
        &self,
        id: &BoxId,
        change: impl FnOnce(&mut BoxRecord) -> bool,
    ) -> Result<bool, Refusal> {
        let _turn = self.claim(id)?;
        let Some(Ok(mut record)) = self.get(id)? else {
            return Ok(false);
        };
        if !change(&mut record) {
            return Ok(false);
        }
        self.write(&record)?;
        Ok(true)
    }

    fn write(&self, record: &BoxRecord) -> Result<(), Refusal> {
        let bytes = record.encode()?;
        let (dir, durable) = self.place(record);
        #[cfg(target_os = "linux")]
        let dir_fd = if durable { &self.disk_fd } else { &self.run_fd };
        // A non-durable record goes to its shard of the runtime directory: a name under it on linux, where
        // the runtime directory's descriptor is held.
        let under = if durable {
            String::new()
        } else {
            format!("{}/", shard(&record.spec.id))
        };
        #[cfg(target_os = "linux")]
        let (tmp, final_path) = (
            format!("{under}.{}.tmp", record.spec.id),
            format!("{under}{}.json", record.spec.id),
        );
        #[cfg(not(target_os = "linux"))]
        let final_path = record_file(&dir.join(&under), &record.spec.id);
        #[cfg(not(target_os = "linux"))]
        let tmp = temp_file(&dir.join(&under), &record.spec.id);
        // A record that need not outlive the host (its box's directories all on tmpfs) is written once
        // per create (goal 5 extended step 5), so its first write goes straight to its file with O_EXCL:
        // no temp, no rename (criterion 3). A crash mid-write leaves a torn file that never decodes, which
        // recovery discards (T-T7.1.2) — the record is whole only once encode's trailing newline lands. A
        // later write of the same record (an update) finds it present (EEXIST) and falls through to the
        // atomic temp-and-rename path, as every durable record takes.
        #[cfg(target_os = "linux")]
        if !durable {
            match rustix::fs::openat(
                dir_fd.as_ref(),
                final_path.as_str(),
                rustix::fs::OFlags::WRONLY
                    | rustix::fs::OFlags::CREATE
                    | rustix::fs::OFlags::EXCL
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::from_raw_mode(0o666),
            ) {
                Ok(fd) => {
                    let mut file = File::from(fd);
                    // T-T7.1.2's crash test kills the node here, after the record's file is open and
                    // before it is written, to leave a torn (empty) record recovery must discard (goal
                    // 5 extended step 5, criterion 3). Debug builds only, and only once the node's
                    // caller armed it ([`crate::Node::with_crash_point`]); release builds never test it.
                    #[cfg(debug_assertions)]
                    if self.crash_before_write.load(Ordering::Relaxed) {
                        let _ = rustix::process::kill_process(
                            rustix::process::getpid(),
                            rustix::process::Signal::KILL,
                        );
                    }
                    return file
                        .write_all(&bytes)
                        .map_err(|e| Refusal::os("writing a record", e));
                }
                Err(rustix::io::Errno::EXIST) => {}
                Err(e) => return Err(Refusal::os("making a record file", e)),
            }
        }
        {
            #[cfg(target_os = "linux")]
            let mut f = File::from(
                rustix::fs::openat(
                    dir_fd.as_ref(),
                    tmp.as_str(),
                    rustix::fs::OFlags::WRONLY
                        | rustix::fs::OFlags::CREATE
                        | rustix::fs::OFlags::TRUNC
                        | rustix::fs::OFlags::CLOEXEC,
                    rustix::fs::Mode::from_raw_mode(0o666),
                )
                .map_err(|e| Refusal::os("making a record temp file", e))?,
            );
            #[cfg(not(target_os = "linux"))]
            let mut f =
                File::create(&tmp).map_err(|e| Refusal::os("making a record temp file", e))?;
            f.write_all(&bytes)
                .map_err(|e| Refusal::os("writing a record", e))?;
            if durable {
                f.sync_all()
                    .map_err(|e| Refusal::os("flushing a record", e))?;
                self.synced.files.fetch_add(1, Ordering::Relaxed);
            }
        }
        #[cfg(target_os = "linux")]
        rustix::fs::renameat(
            dir_fd.as_ref(),
            tmp.as_str(),
            dir_fd.as_ref(),
            final_path.as_str(),
        )
        .map_err(|e| Refusal::os("renaming a record into place", e))?;
        #[cfg(not(target_os = "linux"))]
        fs::rename(&tmp, &final_path)
            .map_err(|e| Refusal::os("renaming a record into place", e))?;
        if durable {
            self.sync_dir(dir)?;
        }
        Ok(())
    }

    /// The record for `id`, if the store holds one: the runtime directory's, else the state
    /// directory's. An unreadable file is returned as its reason, so recovery can clean the box up
    /// rather than crash (T-T7.1.2).
    pub fn get(&self, id: &BoxId) -> Result<Option<Result<BoxRecord, String>>, Refusal> {
        for file in self.files(id) {
            match fs::read(file) {
                Ok(bytes) => return Ok(Some(BoxRecord::decode(&bytes))),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Refusal::os("reading a record", e)),
            }
        }
        Ok(None)
    }

    /// Removes `id`'s record from both places; absent already is removed. Not fsynced: the box's
    /// directories are gone before its record is, so a removal a host crash undoes leaves a record
    /// recovery scrubs again.
    pub fn remove(&self, id: &BoxId) -> Result<(), Refusal> {
        let _turn = self.claim(id)?;
        for file in self.files(id) {
            match fs::remove_file(file) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Refusal::os("removing a record", e)),
            }
        }
        Ok(())
    }

    /// Every record the store holds, by box id; an id whose file can't be read is included with its
    /// reason. A stray temp file (a crash mid-write) is ignored: its rename never completed. A record
    /// in both places is the runtime directory's: a node before goal 5 extended step 5 kept every
    /// record in the state directory, and a record whose place is the runtime directory has been
    /// written there since.
    pub fn all(&self) -> Result<BTreeMap<BoxId, Result<BoxRecord, String>>, Refusal> {
        let mut out = BTreeMap::new();
        for dir in self.listed() {
            for (id, path) in records_in(&dir)? {
                let record = match fs::read(path) {
                    Ok(bytes) => BoxRecord::decode(&bytes),
                    Err(e) => Err(format!("reading the record: {e}")),
                };
                out.insert(id, record);
            }
        }
        Ok(out)
    }

    /// Moves each record kept outside its place to it, written there in full before the copy left
    /// is removed, so a crash between leaves the record in both places and the next recovery moves it
    /// again; how many it moved. Recovery's: a node before goal 5 extended step 5 kept every record in
    /// the state directory, and an unreadable record is left where it is, for recovery to remove.
    pub fn settle(&self) -> Result<usize, Refusal> {
        let mut moved = 0usize;
        // A node before step 6 kept the runtime directory's records in it, not in its shards: each is moved to
        // its shard whole, readable or not (recovery removes an unreadable one's box from there), and one its
        // shard holds already is the shard's, written since.
        for (id, path) in records_in(&self.run)? {
            let _turn = self.claim(&id)?;
            let [to, _] = self.files(&id);
            let gone = if to.exists() {
                fs::remove_file(&path)
            } else {
                fs::rename(&path, &to)
            };
            match gone {
                Ok(()) => moved = moved.saturating_add(1),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Refusal::os("moving a record to its shard", e)),
            }
        }
        for (id, path) in records_in(&self.disk)? {
            let _turn = self.claim(&id)?;
            let Ok(Ok(record)) = fs::read(&path).map(|bytes| BoxRecord::decode(&bytes)) else {
                continue;
            };
            if record.spec.id != id || must_outlive_host(&record) {
                continue;
            }
            self.write(&record)?;
            match fs::remove_file(&path) {
                Ok(()) => moved = moved.saturating_add(1),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Refusal::os("removing a record moved to its place", e)),
            }
        }
        Ok(moved)
    }

    /// Removes the temp files a crash mid-write left in either place, each never renamed into place,
    /// under its box's turn, so a write under way keeps its own; how many it removed. Recovery's, so a
    /// crash at a record write leaves nothing behind (T-T7.1.2).
    pub fn remove_strays(&self) -> Result<usize, Refusal> {
        let mut removed = 0usize;
        for dir in self.listed() {
            let entries = fs::read_dir(&dir).map_err(|e| Refusal::os("listing records", e))?;
            for entry in entries {
                let entry = entry.map_err(|e| Refusal::os("listing records", e))?;
                let name = entry.file_name();
                let Some(id) = name
                    .to_str()
                    .and_then(|n| n.strip_prefix('.'))
                    .and_then(|n| n.strip_suffix(".tmp"))
                    .and_then(|n| BoxId::new(n).ok())
                else {
                    continue;
                };
                let _turn = self.claim(&id)?;
                match fs::remove_file(temp_file(&dir, &id)) {
                    Ok(()) => removed = removed.saturating_add(1),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(Refusal::os("removing a record's stray temp file", e)),
                }
            }
        }
        Ok(removed)
    }

    /// fsync directory `dir`, so a rename in it is durable.
    fn sync_dir(&self, dir: &Path) -> Result<(), Refusal> {
        #[cfg(target_os = "linux")]
        {
            // Only durable records reach this call, and their directory is always disk.
            let _ = dir;
            self.disk_fd
                .sync_all()
                .map_err(|e| Refusal::os("flushing the record directory", e))?;
        }
        #[cfg(not(target_os = "linux"))]
        File::open(dir)
            .and_then(|d| d.sync_all())
            .map_err(|e| Refusal::os("flushing the record directory", e))?;
        self.synced.dirs.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

/// Retain a close-on-exec directory descriptor for relative record writes and durable renames.
#[cfg(target_os = "linux")]
fn open_directory(dir: &Path) -> Result<File, Refusal> {
    rustix::fs::open(
        dir,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map(File::from)
    .map_err(|e| Refusal::os("opening a record directory", e))
}

/// Makes `dir` readable by its owner alone, by its mode where the platform has one (unix); elsewhere
/// it keeps the access its parent's gives it.
#[cfg(unix)]
fn owner_only(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn owner_only(_dir: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Box `id`'s shard of the runtime directory: two hex digits of FNV-1a over its id, the same in every run of
/// every build.
fn shard(id: &BoxId) -> String {
    format!("{:02x}", fnv1a(id.as_str().as_bytes()) & SHARD_MASK)
}

/// FNV-1a, 64-bit: its arithmetic is modulo 2^64 by definition, so the multiply wraps. A record's shard's hash,
/// and the node's key's on linux. FNV-1a because the hash must come out the same in every run of every build,
/// which std's hashers don't promise; the offset basis and prime are FNV-1a's own (IETF
/// draft-eastlake-fnv, "FNV Basics" and "FNV Constants").
pub(crate) fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash: u64, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Where box `id`'s record is kept in the runtime record directory `run` ([`crate::records_dir`]): in its
/// shard. For tests and tools that look for a box's record from outside the store.
pub fn runtime_record(run: &Path, id: &BoxId) -> PathBuf {
    record_file(&run.join(shard(id)), id)
}

/// The temp file a write of box `id`'s record goes through in the runtime record directory `run`.
pub fn runtime_temp(run: &Path, id: &BoxId) -> PathBuf {
    temp_file(&run.join(shard(id)), id)
}

/// Box `id`'s record file in `dir`.
fn record_file(dir: &Path, id: &BoxId) -> PathBuf {
    dir.join(format!("{id}.json"))
}

/// The temp file a write of box `id`'s record in `dir` goes through.
fn temp_file(dir: &Path, id: &BoxId) -> PathBuf {
    dir.join(format!(".{id}.tmp"))
}

/// The records in `dir`, each box's id with its file: every `<id>.json`, nothing else.
fn records_in(dir: &Path) -> Result<Vec<(BoxId, PathBuf)>, Refusal> {
    let mut out = Vec::new();
    let entries = fs::read_dir(dir).map_err(|e| Refusal::os("listing records", e))?;
    for entry in entries {
        let entry = entry.map_err(|e| Refusal::os("listing records", e))?;
        let name = entry.file_name();
        let Some(id) = name
            .to_str()
            .and_then(|n| n.strip_suffix(".json"))
            .and_then(|n| BoxId::new(n).ok())
        else {
            continue;
        };
        out.push((id, entry.path()));
    }
    Ok(out)
}

/// Whether a record must outlive the host, and so is fsynced: one naming a directory a host crash
/// leaves behind. A directory on tmpfs goes with the host, as the box's cgroup does, so a record whose
/// directories all lie on tmpfs (an `oci` box's container state, under the node's runtime directory)
/// describes nothing left to remove after a host crash; nor does one naming none (a `process` box's).
/// A rendered bundle's writable layer on disk makes its record durable.
fn must_outlive_host(record: &BoxRecord) -> bool {
    record.dirs.iter().any(|dir| !on_tmpfs(dir))
}

/// tmpfs's magic number (include/uapi/linux/magic.h, `TMPFS_MAGIC`).
#[cfg(target_os = "linux")]
const TMPFS_MAGIC: i128 = 0x0102_1994;

/// Whether `dir` lies on tmpfs, judged by its filesystem ([`filesystem`]).
#[cfg(target_os = "linux")]
fn on_tmpfs(dir: &Path) -> bool {
    gone_with_the_host(filesystem(dir))
}

/// Whether a filesystem, by its magic number, goes with the host: tmpfs, which lives in memory, alone.
/// Every other (overlayfs, btrfs, ext4, xfs) outlives it, and so does one whose type can't be read, so
/// a record that has to be durable is never written without its fsync.
#[cfg(target_os = "linux")]
fn gone_with_the_host(filesystem: Option<i128>) -> bool {
    filesystem == Some(TMPFS_MAGIC)
}

/// The magic number of the filesystem `dir` lies on (statfs(2)'s `f_type`), or, when it isn't made yet,
/// of the nearest directory above it that is, where it will be made; `None` when that can't be read.
#[cfg(target_os = "linux")]
fn filesystem(dir: &Path) -> Option<i128> {
    // As many steps as the path has components.
    for at in dir.ancestors() {
        match rustix::fs::statfs(at) {
            Ok(fs) => return Some(i128::from(fs.f_type)),
            Err(rustix::io::Errno::NOENT) => {}
            Err(_) => return None,
        }
    }
    None
}

/// tmpfs is Linux's: elsewhere every directory is taken to outlive the host.
#[cfg(not(target_os = "linux"))]
fn on_tmpfs(_dir: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::time::Instant;

    use super::*;
    use hex_api::{Bounds, BoxSpec, ClassSpec};
    use hex_core::record::Phase;

    fn record(id: &str) -> BoxRecord {
        let spec = BoxSpec {
            id: BoxId::new(id).unwrap(),
            bounds: Bounds::default(),
            class: ClassSpec::Process,
            owner: None,
        };
        BoxRecord::new(spec, Phase::Ready, vec![])
    }

    /// A store over `<dir>/state` and `<dir>/run`, its two places.
    fn store_in(dir: &Path) -> Store {
        Store::open(&dir.join("state"), &dir.join("run")).unwrap()
    }

    /// The names of the records in the runtime record directory `run`'s shards, sorted.
    fn sharded(run: &Path) -> Vec<String> {
        let mut out: Vec<String> = (0..=SHARD_MASK)
            .flat_map(|n| names(&run.join(format!("{n:02x}"))))
            .collect();
        out.sort();
        out
    }

    /// The names of the files in `dir`, sorted.
    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_record_is_put_got_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        assert_eq!(store.get(&BoxId::new("a").unwrap()).unwrap(), None);
        let r = record("a");
        store.put(&r).unwrap();
        assert_eq!(store.get(&r.spec.id).unwrap(), Some(Ok(r.clone())));
        store.remove(&r.spec.id).unwrap();
        assert_eq!(store.get(&r.spec.id).unwrap(), None);
    }

    /// A directory on tmpfs and one on disk this test can write in, judged as the store judges them,
    /// or why the host has no such pair.
    #[cfg(target_os = "linux")]
    fn tmpfs_and_disk() -> Result<(tempfile::TempDir, tempfile::TempDir), &'static str> {
        let tmpfs = ["/dev/shm", "/tmp"]
            .into_iter()
            .map(PathBuf::from)
            .chain(std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from))
            .filter(|d| on_tmpfs(d))
            .find_map(|d| tempfile::tempdir_in(d).ok())
            .ok_or("no tmpfs this test can write in")?;
        // Beside this test's binary, else its crate's sources.
        let disk = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_owned))
            .into_iter()
            .chain([PathBuf::from(env!("CARGO_MANIFEST_DIR"))])
            .filter(|d| !on_tmpfs(d))
            .find_map(|d| tempfile::tempdir_in(d).ok())
            .ok_or("no disk this test can write in")?;
        Ok((tmpfs, disk))
    }

    /// T-T7.1.2 and decision 1 (goal 5 plan step 2; goal 5 extended step 5): a record whose
    /// directories all lie on tmpfs (an `oci` box's container state under the node's runtime
    /// directory), or that names none, is written without a sync, in the runtime directory's place;
    /// one with a directory on disk (a rendered bundle's writable layer) is written in the state
    /// directory, with a sync of the file and of that directory, so recovery after a host crash still
    /// finds what to remove. Directories not made yet are judged where they will be made.
    #[cfg(target_os = "linux")]
    #[test]
    fn only_a_record_with_a_directory_on_disk_is_synced_and_kept_on_disk() {
        let (tmpfs, disk) = match tmpfs_and_disk() {
            Ok(pair) => pair,
            Err(why) => {
                eprintln!("SKIP: {why}");
                return;
            }
        };
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        let none = Syncs::default();

        store.put(&record("p")).unwrap();
        assert_eq!(store.syncs(), none, "a record naming no directory");

        let mut on_tmpfs_alone = record("t");
        on_tmpfs_alone.dirs = vec![tmpfs.path().join("t"), tmpfs.path().to_owned()];
        assert!(!must_outlive_host(&on_tmpfs_alone));
        store.put(&on_tmpfs_alone).unwrap();
        assert_eq!(store.syncs(), none, "a record on tmpfs alone is not synced");
        assert_eq!(
            store.get(&on_tmpfs_alone.spec.id).unwrap(),
            Some(Ok(on_tmpfs_alone.clone())),
            "it's written all the same"
        );

        let mut with_disk = record("d");
        with_disk.dirs = vec![tmpfs.path().join("d"), disk.path().join("d")];
        assert!(must_outlive_host(&with_disk));
        store.put(&with_disk).unwrap();
        assert_eq!(
            store.syncs(),
            Syncs { files: 1, dirs: 1 },
            "a record with a directory on disk is synced, file and directory"
        );
        let change = |r: &mut BoxRecord| {
            r.abandoned = r.abandoned.saturating_add(1);
            true
        };
        assert_eq!(store.update(&with_disk.spec.id, change), Ok(true));
        assert_eq!(
            store.syncs(),
            Syncs { files: 2, dirs: 2 },
            "and so is each change of it"
        );
        assert_eq!(store.update(&on_tmpfs_alone.spec.id, change), Ok(true));
        assert_eq!(store.syncs(), Syncs { files: 2, dirs: 2 });
        assert_eq!(
            (
                names(&dir.path().join("state")),
                sharded(&dir.path().join("run"))
            ),
            (
                vec!["d.json".to_owned()],
                vec!["p.json".to_owned(), "t.json".to_owned()]
            ),
            "each record in its place: the durable one in the state directory, the rest beside the runtime state"
        );
    }

    /// Goal 5 extended step 5: a record found in both places, as a node before the step left one in
    /// the state directory and wrote it again since in its place, is the runtime directory's, to
    /// [`Store::get`] and [`Store::all`]; [`Store::remove`] takes both copies.
    #[test]
    fn a_record_in_both_places_is_the_runtime_directorys_and_goes_from_both() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        let old = record("a");
        fs::write(dir.path().join("state/a.json"), old.encode().unwrap()).unwrap();
        let mut new = old.clone();
        new.abandoned = 7;
        store.put(&new).unwrap();
        assert_eq!(store.get(&new.spec.id).unwrap(), Some(Ok(new.clone())));
        assert_eq!(
            store.all().unwrap().into_iter().collect::<Vec<_>>(),
            [(new.spec.id.clone(), Ok(new.clone()))]
        );
        store.remove(&new.spec.id).unwrap();
        assert_eq!(
            store.get(&new.spec.id).unwrap(),
            None,
            "neither copy is left"
        );
        assert!(names(&dir.path().join("state")).is_empty());
    }

    /// Goal 5 extended step 5, T-T7.1.2 across the step's upgrade: a node before it kept every record
    /// in its state directory. Recovery's settle moves each whose place is the runtime directory
    /// there, and leaves a record that must outlive the host, a file that isn't a record and an
    /// unreadable one where they are; a second settle moves nothing.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_record_an_older_node_left_in_the_state_directory_is_moved_to_its_place() {
        let (_tmpfs, disk) = match tmpfs_and_disk() {
            Ok(pair) => pair,
            Err(why) => {
                eprintln!("SKIP: {why}");
                return;
            }
        };
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        let state = dir.path().join("state");
        let moving = record("moving");
        let mut staying = record("staying");
        staying.dirs = vec![disk.path().join("staying")];
        for r in [&moving, &staying] {
            fs::write(
                state.join(format!("{}.json", r.spec.id)),
                r.encode().unwrap(),
            )
            .unwrap();
        }
        fs::write(state.join("broken.json"), b"{ not json").unwrap();
        fs::create_dir(state.join("bundles")).unwrap();

        assert_eq!(store.settle().unwrap(), 1);
        assert_eq!(
            names(&state),
            ["broken.json", "bundles", "staying.json"],
            "the durable record, the unreadable one and the bundles stay"
        );
        assert_eq!(sharded(&dir.path().join("run")), ["moving.json"]);
        assert_eq!(store.get(&moving.spec.id).unwrap(), Some(Ok(moving)));
        assert_eq!(store.get(&staying.spec.id).unwrap(), Some(Ok(staying)));
        assert_eq!(
            store.syncs(),
            Syncs::default(),
            "the moved record isn't synced"
        );
        assert_eq!(store.settle().unwrap(), 0);
    }

    /// Goal 5 plan step 2: writes of two boxes never wait on each other; two of one box do. While one
    /// box's turn is held, as its write holds it across the fsyncs, 31 other boxes' records are written
    /// from 31 threads at once, none of them waiting; a second write of the held box waits, its record
    /// not written, until the turn is let go, and then is.
    #[test]
    fn writes_of_two_boxes_never_wait_on_each_other_and_of_one_box_do() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        let held = record("held");
        let turn = store.claim(&held.spec.id).unwrap();
        let (done, written) = std::sync::mpsc::channel();
        std::thread::scope(|s| {
            for n in 0..31 {
                let (store, done) = (&store, done.clone());
                s.spawn(move || done.send(store.put(&record(&format!("b{n}")))).unwrap());
            }
            for _ in 0..31 {
                let put = written
                    .recv_timeout(hex_api::conformance::PATIENCE)
                    .expect("a write waited on the held box's turn");
                put.unwrap();
            }
        });
        assert_eq!(store.all().unwrap().len(), 31);
        assert_eq!(store.waited(), 0, "no write of another box waited");

        std::thread::scope(|s| {
            let second = s.spawn(|| store.put(&held));
            let deadline = Instant::now() + hex_api::conformance::PATIENCE;
            while store.waited() == 0 && Instant::now() < deadline {
                std::thread::yield_now();
            }
            assert_eq!(store.waited(), 1, "the second write of the held box waits");
            assert_eq!(
                store.get(&held.spec.id).unwrap(),
                None,
                "and writes nothing while the turn is held"
            );
            drop(turn);
            second.join().unwrap().unwrap();
        });
        assert_eq!(store.get(&held.spec.id).unwrap(), Some(Ok(held)));
    }

    /// CLAUDE.md §2 and §3: with [`TURNS`] boxes' records being changed at once, a change of one more
    /// is refused by type, writing nothing, and goes ahead once they're done.
    #[test]
    fn a_change_past_the_turns_bound_is_refused_by_type() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        let ids: Vec<BoxId> = (0..TURNS)
            .map(|n| BoxId::new(&format!("b{n}")).unwrap())
            .collect();
        let turns: Vec<Turn<'_>> = ids.iter().map(|id| store.claim(id).unwrap()).collect();
        let past = record("past");
        assert_eq!(
            store.put(&past),
            Err(Refusal::Exhausted {
                what: "record changes under way",
                bound: TURNS
            })
        );
        assert_eq!(store.get(&past.spec.id).unwrap(), None, "nothing written");
        drop(turns);
        store.put(&past).unwrap();
    }

    /// Doc-truth (CLAUDE.md §6): the bound on record changes under way is the node's box budget, the one
    /// the cgroup root refuses past.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_turns_bound_is_the_box_budget() {
        assert_eq!(TURNS, hex_runtime::linux::cgroup::MAX_BOXES);
    }

    /// Decision 1's rule by filesystem (goal 5 plan step 2), on the magic numbers statfs(2) gives
    /// (include/uapi/linux/magic.h): tmpfs goes with the host; overlayfs (a container's root) and btrfs
    /// (fedora's /home), as every filesystem but tmpfs, outlive it, and so does one that can't be read.
    #[cfg(target_os = "linux")]
    #[test]
    fn tmpfs_alone_goes_with_the_host() {
        const OVERLAYFS_SUPER_MAGIC: i128 = 0x794c_7630;
        const BTRFS_SUPER_MAGIC: i128 = 0x9123_683e;
        assert!(gone_with_the_host(Some(TMPFS_MAGIC)));
        for outlives in [Some(OVERLAYFS_SUPER_MAGIC), Some(BTRFS_SUPER_MAGIC), None] {
            assert!(!gone_with_the_host(outlives), "{outlives:x?}");
        }
    }

    /// Goal 5 plan step 2: a directory not made yet is judged by the nearest one above it that is,
    /// where it will be made: three levels below a directory on tmpfs it lies on tmpfs, and below one on
    /// disk on that disk's filesystem, which the store judges to outlive the host.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_missing_directory_is_judged_by_its_nearest_ancestor() {
        let (tmpfs, disk) = match tmpfs_and_disk() {
            Ok(pair) => pair,
            Err(why) => {
                eprintln!("SKIP: {why}");
                return;
            }
        };
        let deep = tmpfs.path().join("a/b/c");
        assert!(!deep.exists());
        assert_eq!(filesystem(&deep), Some(TMPFS_MAGIC));
        assert!(on_tmpfs(&deep));
        let on_disk = disk.path().join("x/y");
        assert!(!on_disk.exists());
        let disk_fs = filesystem(disk.path()).unwrap();
        assert_eq!(filesystem(&on_disk), Some(disk_fs));
        assert!(!on_tmpfs(&on_disk));
        // `/` is overlayfs in a container (the root lab's, seam_root.sh), btrfs or another disk's on a
        // host: a path under it that isn't made is judged by it.
        let root_fs = filesystem(Path::new("/")).unwrap();
        let under_root = Path::new("/hex-no-such-directory/x");
        assert!(!under_root.exists());
        assert_eq!(filesystem(under_root), Some(root_fs));
        assert_eq!(on_tmpfs(under_root), root_fs == TMPFS_MAGIC);
        eprintln!("the disk here is filesystem {disk_fs:#x}; the root, {root_fs:#x}");
    }

    /// HEX-SOCK-6: an update reads and writes a record as one change, so changes made from many
    /// threads at once all land; a record that isn't there, or a change that changes nothing, is no
    /// update.
    #[test]
    fn updates_from_many_threads_all_land() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        let r = record("a");
        store.put(&r).unwrap();
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| {
                    for _ in 0..25 {
                        let counted = store.update(&r.spec.id, |r| {
                            r.abandoned += 1;
                            true
                        });
                        assert!(counted.unwrap());
                    }
                });
            }
        });
        let back = store.get(&r.spec.id).unwrap().unwrap().unwrap();
        assert_eq!(back.abandoned, 200);
        assert!(
            !store
                .update(&BoxId::new("none").unwrap(), |_| true)
                .unwrap()
        );
        assert!(!store.update(&r.spec.id, |_| false).unwrap());
    }

    /// T-T7.1.2: the temp files a crash mid-write leaves in either place are removed, and only they;
    /// a box's record and anything else in each directory stay.
    #[test]
    fn strays_are_removed_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        store.put(&record("a")).unwrap();
        let (state, run) = (dir.path().join("state"), dir.path().join("run"));
        // One in a shard, one where a node before the shards wrote, one in the state directory.
        fs::write(runtime_temp(&run, &BoxId::new("a").unwrap()), b"{ half").unwrap();
        fs::write(run.join(".c.tmp"), b"{ half").unwrap();
        fs::write(state.join(".b.tmp"), b"").unwrap();
        fs::write(run.join(".notes"), b"hi").unwrap();
        fs::write(state.join(".notes"), b"hi").unwrap();
        assert_eq!(store.remove_strays().unwrap(), 3);
        assert_eq!(sharded(&run), ["a.json"]);
        assert!(
            names(&run).iter().all(|n| n == ".notes" || n.len() == 2),
            "the runtime directory holds its shards and the notes: {:?}",
            names(&run)
        );
        assert_eq!(names(&state), [".notes"]);
        assert_eq!(store.remove_strays().unwrap(), 0);
    }

    #[test]
    fn all_reads_every_record_in_both_places_and_ignores_strays() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        store.put(&record("a")).unwrap();
        store.put(&record("b")).unwrap();
        let state = dir.path().join("state");
        // A stray temp file (a crash mid-write) and an unrelated file are not records.
        fs::write(state.join(".c.tmp"), b"{}").unwrap();
        fs::write(state.join("notes.txt"), b"hi").unwrap();
        // A damaged record is reported, not skipped.
        fs::write(state.join("d.json"), b"{ not json").unwrap();
        let all = store.all().unwrap();
        assert_eq!(all.len(), 3);
        assert!(all[&BoxId::new("a").unwrap()].is_ok());
        assert!(all[&BoxId::new("d").unwrap()].is_err());
    }

    /// Goal 5 extended step 6: the runtime directory's records are spread over its shards by their box's id, each
    /// where [`runtime_record`] says, none in the directory itself, so the boxes made and deleted at once rarely
    /// take one directory's lock; got, listed and removed from there. Non-vacuity: 256 ids reach most shards,
    /// at least [`shards_256_ids_reach`] of them.
    #[test]
    fn records_are_spread_over_the_shards_by_their_ids() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        let run = dir.path().join("run");
        assert_eq!(
            names(&run).len(),
            SHARDS,
            "the shards are made as the store opens"
        );
        let ids: Vec<BoxId> = (0..256)
            .map(|n| BoxId::new(&format!("s{n:06}")).unwrap())
            .collect();
        for id in &ids {
            store.put(&record(id.as_str())).unwrap();
            assert!(runtime_record(&run, id).exists(), "{id} in its shard");
        }
        assert_eq!(names(&run).len(), SHARDS, "nothing in the directory itself");
        let used: BTreeSet<PathBuf> = ids
            .iter()
            .filter_map(|id| runtime_record(&run, id).parent().map(Path::to_owned))
            .collect();
        let least = shards_256_ids_reach();
        assert!(
            used.len() >= least,
            "256 ids took {} of the shards, fewer than {least}",
            used.len()
        );
        assert_eq!(store.all().unwrap().len(), 256);
        for id in &ids {
            assert!(store.get(id).unwrap().is_some_and(|r| r.is_ok()));
            store.remove(id).unwrap();
            assert!(!runtime_record(&run, id).exists());
        }
        assert!(sharded(&run).is_empty());
    }

    /// The fewest of the [`SHARDS`] shards 256 ids reach but once in 2^32 sets of ids, were the id hash
    /// uniform. Each shard is missed by all 256 with chance (63/64)^256, so the missed shards number μ =
    /// 64·(63/64)^256 ≈ 1.14 on average and the ids reach 64·(1 − (63/64)^256) ≈ 62.9. The misses are
    /// negatively associated (balls in bins: Dubhashi and Ranjan, "Balls and Bins: A Study in Negative
    /// Dependence", Random Structures & Algorithms 13(2), 1998, §2), so Chernoff's bound holds for their count:
    /// P(misses ≥ t) ≤ (e^δ / (1+δ)^(1+δ))^μ with t = (1+δ)μ. This is the least t that bound puts under 2^-32,
    /// taken from [`SHARDS`]. Derived, not picked: a hash that spreads ids worse than chance fails it.
    fn shards_256_ids_reach() -> usize {
        let shards = SHARDS as f64;
        let mu = shards * ((shards - 1.0) / shards).powi(256);
        let tail = |t: f64| {
            let delta = t / mu - 1.0;
            mu * (delta - (1.0 + delta) * (1.0 + delta).ln())
        };
        let misses = (1..SHARDS)
            .find(|&t| t as f64 > mu && tail(t as f64) <= -32.0 * std::f64::consts::LN_2)
            .unwrap();
        SHARDS - misses
    }

    /// Doc-truth for the bound above (CLAUDE.md §6): do derive it; expect the ids' average reach, about 62.9 of
    /// 64, and a least reach of 50: 14 shards missed is the first count Chernoff's bound puts under 2^-32.
    #[test]
    fn the_shards_256_ids_reach_follow_from_the_hash_being_uniform() {
        let shards = SHARDS as f64;
        let reach = shards * (1.0 - ((shards - 1.0) / shards).powi(256));
        assert!((reach - 62.86).abs() < 0.01, "{reach}");
        assert_eq!(shards_256_ids_reach(), 50);
    }

    /// Goal 5 extended step 6, T-T7.1.2 across the shards' upgrade: a node before them kept the runtime
    /// directory's records in it. Recovery's settle moves each to its shard, readable or not (recovery then
    /// removes an unreadable one's box from there), and of a record its shard holds already keeps the shard's,
    /// written since; a second settle moves nothing, and no create or delete reads the directory itself.
    #[test]
    fn a_record_an_older_node_left_in_the_runtime_directory_is_moved_to_its_shard() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        let run = dir.path().join("run");
        let moving = record("moving");
        fs::write(run.join("moving.json"), moving.encode().unwrap()).unwrap();
        fs::write(run.join("broken.json"), b"{ not json").unwrap();
        let mut newer = record("both");
        newer.abandoned = 3;
        store.put(&newer).unwrap();
        fs::write(run.join("both.json"), record("both").encode().unwrap()).unwrap();
        assert_eq!(store.settle().unwrap(), 3);
        assert_eq!(
            sharded(&run),
            ["both.json", "broken.json", "moving.json"],
            "each in its shard"
        );
        assert!(
            names(&run).iter().all(|n| n.len() == 2),
            "{:?}",
            names(&run)
        );
        assert_eq!(store.get(&moving.spec.id).unwrap(), Some(Ok(moving)));
        assert_eq!(
            store.get(&newer.spec.id).unwrap(),
            Some(Ok(newer)),
            "the shard's copy, written since"
        );
        let all = store.all().unwrap();
        assert!(
            all[&BoxId::new("broken").unwrap()].is_err(),
            "still found, as unreadable"
        );
        assert_eq!(store.settle().unwrap(), 0);
    }
}
