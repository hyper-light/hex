//! The node agent's store of box records. Each box is one file, written whole to a temp file and
//! renamed into place, so a crash of the node agent leaves either the old record or the new one,
//! never a torn one (T-T7.1.2): the page cache outlives the process, and rename(2) replaces
//! atomically. Only a record that must outlive the host is fsynced too, file and directory: one naming
//! a directory on a filesystem a host crash leaves behind. After a host crash every box's cgroup is
//! gone, and so is everything on tmpfs, so recovery scrubs each record it finds, torn or whole, and
//! only a record naming a directory on disk points at anything left to remove
//! (docs/design/runtime-architecture.md, decision 1).
//!
//! Changes of one box's record are made one at a time; changes of different boxes never wait on each
//! other, so one box's fsync never holds up another box's create (goal 5 plan step 2).

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use hex_api::{BoxId, Refusal};
use hex_core::record::BoxRecord;

/// How long a change to a box's record waits for the change of the same box before it to finish:
/// that one's write and its fsyncs, which take milliseconds. A minute is far past any real delay, so
/// missing it is a fault to report (a disk that has stopped answering), not a limit to raise
/// (CLAUDE.md §4).
pub const WRITE_PATIENCE: Duration = Duration::from_secs(60);

/// Box records on disk, one file each, named `<id>.json`.
#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
    /// The boxes whose records are being changed now, one change each: [`Store::put`],
    /// [`Store::update`] and [`Store::remove`] of a box wait for the change of it under way, so an
    /// update's read and write of a record are one change and two users changing one record at once
    /// (the supervisor recording an init's exit, the socket counting an abandoned program) never lose
    /// either's change. A change of another box never waits. Bounded by the threads that change
    /// records, each changing one at a time.
    busy: Arc<Busy>,
    /// The fsyncs the store has made since it opened.
    synced: Arc<Synced>,
}

#[derive(Debug, Default)]
struct Busy {
    boxes: Mutex<BTreeSet<BoxId>>,
    /// Told whenever a box is let go.
    free: Condvar,
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

/// Box `id`'s turn to change its record, given up when dropped.
struct Claim<'a> {
    busy: &'a Busy,
    id: BoxId,
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        // The set stays whole whoever held its lock (nothing panics holding it: CLAUDE.md §1), so a
        // poisoned lock still lets the box go.
        let mut boxes = self
            .busy
            .boxes
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        boxes.remove(&self.id);
        drop(boxes);
        self.busy.free.notify_all();
    }
}

fn poisoned<T>(_: PoisonError<T>) -> Refusal {
    Refusal::Broken("the record store's lock was poisoned".to_owned())
}

impl Store {
    /// Opens the store at `dir`, making it when it's missing.
    pub fn open(dir: &Path) -> Result<Self, Refusal> {
        fs::create_dir_all(dir).map_err(|e| Refusal::os("making the record directory", e))?;
        Ok(Self {
            dir: dir.to_owned(),
            busy: Arc::default(),
            synced: Arc::default(),
        })
    }

    /// Box `id`'s turn to change its record: waits for a change of the same box under way, never for
    /// another box's, and refuses past [`WRITE_PATIENCE`].
    fn claim(&self, id: &BoxId) -> Result<Claim<'_>, Refusal> {
        let deadline = Instant::now().checked_add(WRITE_PATIENCE);
        let mut boxes = self.busy.boxes.lock().map_err(poisoned)?;
        while boxes.contains(id) {
            let left = deadline.map_or(Duration::ZERO, |d| {
                d.saturating_duration_since(Instant::now())
            });
            if left.is_zero() {
                return Err(Refusal::Os {
                    op: "changing a box's record",
                    why: format!(
                        "box {id}'s record was still being written after {WRITE_PATIENCE:?}"
                    ),
                });
            }
            boxes = self
                .busy
                .free
                .wait_timeout(boxes, left)
                .map_err(poisoned)?
                .0;
        }
        boxes.insert(id.clone());
        Ok(Claim {
            busy: &self.busy,
            id: id.clone(),
        })
    }

    fn path(&self, id: &BoxId) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    fn temp(&self, id: &BoxId) -> PathBuf {
        self.dir.join(format!(".{id}.tmp"))
    }

    /// The fsyncs the store has made since it opened.
    pub fn syncs(&self) -> Syncs {
        Syncs {
            files: self.synced.files.load(Ordering::Relaxed),
            dirs: self.synced.dirs.load(Ordering::Relaxed),
        }
    }

    /// Writes `record` whole: either the old file or the new one survives a crash of the node
    /// agent, and a record that must outlive the host ([`must_outlive_host`]) is fsynced too.
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
        let final_path = self.path(&record.spec.id);
        let tmp = self.temp(&record.spec.id);
        let durable = must_outlive_host(record);
        {
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
        fs::rename(&tmp, &final_path)
            .map_err(|e| Refusal::os("renaming a record into place", e))?;
        if durable {
            self.sync_dir()?;
        }
        Ok(())
    }

    /// The record for `id`, if the store holds one. An unreadable file is returned as its reason,
    /// so recovery can clean the box up rather than crash (T-T7.1.2).
    pub fn get(&self, id: &BoxId) -> Result<Option<Result<BoxRecord, String>>, Refusal> {
        match fs::read(self.path(id)) {
            Ok(bytes) => Ok(Some(BoxRecord::decode(&bytes))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Refusal::os("reading a record", e)),
        }
    }

    /// Removes `id`'s record; absent already is removed. Not fsynced: the box's directories are gone
    /// before its record is, so a removal a host crash undoes leaves a record recovery scrubs again.
    pub fn remove(&self, id: &BoxId) -> Result<(), Refusal> {
        let _turn = self.claim(id)?;
        match fs::remove_file(self.path(id)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Refusal::os("removing a record", e)),
        }
    }

    /// Every record the store holds, by box id; an id whose file can't be read is included with its
    /// reason. A stray temp file (a crash mid-write) is ignored: its rename never completed.
    pub fn all(&self) -> Result<BTreeMap<BoxId, Result<BoxRecord, String>>, Refusal> {
        let mut out = BTreeMap::new();
        let entries = fs::read_dir(&self.dir).map_err(|e| Refusal::os("listing records", e))?;
        for entry in entries {
            let entry = entry.map_err(|e| Refusal::os("listing records", e))?;
            let name = entry.file_name();
            let Some(stem) = Path::new(&name)
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".json"))
            else {
                continue;
            };
            let id = match BoxId::new(stem) {
                Ok(id) => id,
                Err(_) => continue,
            };
            match fs::read(entry.path()) {
                Ok(bytes) => {
                    out.insert(id, BoxRecord::decode(&bytes));
                }
                Err(e) => {
                    out.insert(id, Err(format!("reading the record: {e}")));
                }
            }
        }
        Ok(out)
    }

    /// Removes the temp files a crash mid-write left, each never renamed into place, under its box's
    /// turn, so a write under way keeps its own; how many it removed. Recovery's, so a crash at a
    /// record write leaves nothing behind (T-T7.1.2).
    pub fn remove_strays(&self) -> Result<usize, Refusal> {
        let entries = fs::read_dir(&self.dir).map_err(|e| Refusal::os("listing records", e))?;
        let mut removed = 0usize;
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
            match fs::remove_file(self.temp(&id)) {
                Ok(()) => removed = removed.saturating_add(1),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Refusal::os("removing a record's stray temp file", e)),
            }
        }
        Ok(removed)
    }

    /// fsync the directory, so a rename is durable.
    fn sync_dir(&self) -> Result<(), Refusal> {
        File::open(&self.dir)
            .and_then(|d| d.sync_all())
            .map_err(|e| Refusal::os("flushing the record directory", e))?;
        self.synced.dirs.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

/// Whether a record must outlive the host, and so is fsynced: one naming a directory a host crash
/// leaves behind. A directory on tmpfs goes with the host, as the box's cgroup does, so a record whose
/// directories all lie on tmpfs (an `oci` box's container state, under the node's runtime directory)
/// describes nothing left to remove after a host crash; nor does one naming none (a `process` box's).
/// A rendered bundle's writable layer on disk makes its record durable.
fn must_outlive_host(record: &BoxRecord) -> bool {
    record.dirs.iter().any(|dir| !on_tmpfs(dir))
}

/// Whether `dir` lies on tmpfs, judged by its filesystem, or, when it isn't made yet, by that of the
/// nearest directory above it that is, where it will be made. A filesystem that can't be read is
/// taken to outlive the host, so a record that has to be durable is never written without its fsync.
#[cfg(target_os = "linux")]
fn on_tmpfs(dir: &Path) -> bool {
    // tmpfs's magic number (include/uapi/linux/magic.h, `TMPFS_MAGIC`).
    const TMPFS_MAGIC: i128 = 0x0102_1994;
    // As many steps as the path has components.
    for at in dir.ancestors() {
        match rustix::fs::statfs(at) {
            Ok(fs) => return i128::from(fs.f_type) == TMPFS_MAGIC,
            Err(rustix::io::Errno::NOENT) => {}
            Err(_) => return false,
        }
    }
    false
}

/// tmpfs is Linux's: elsewhere every directory is taken to outlive the host.
#[cfg(not(target_os = "linux"))]
fn on_tmpfs(_dir: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_api::{Bounds, BoxSpec, ClassSpec};
    use hex_core::record::Phase;

    fn record(id: &str) -> BoxRecord {
        let spec = BoxSpec {
            id: BoxId::new(id).unwrap(),
            bounds: Bounds::default(),
            class: ClassSpec::Process,
        };
        BoxRecord::new(spec, Phase::Ready, vec![])
    }

    #[test]
    fn a_record_is_put_got_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
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

    /// T-T7.1.2 and decision 1 (goal 5 plan step 2): a record whose directories all lie on tmpfs (an
    /// `oci` box's container state under the node's runtime directory), or that names none, is written
    /// without a sync; one with a directory on disk (a rendered bundle's writable layer) is written
    /// with a sync of the file and of the store's directory, so recovery after a host crash still
    /// finds what to remove. Directories not made yet are judged where they will be made.
    #[cfg(target_os = "linux")]
    #[test]
    fn only_a_record_with_a_directory_on_disk_is_synced() {
        let (tmpfs, disk) = match tmpfs_and_disk() {
            Ok(pair) => pair,
            Err(why) => {
                eprintln!("SKIP: {why}");
                return;
            }
        };
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
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
    }

    /// Goal 5 plan step 2: no box's record write waits on another box's. While one box's turn is held,
    /// as its write holds it across the fsyncs, 31 other boxes' records are written from 31 threads at
    /// once; the held box's own write goes ahead once its turn is let go.
    #[test]
    fn a_write_of_one_box_never_waits_on_another_boxs() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
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
        drop(turn);
        store.put(&held).unwrap();
        assert_eq!(store.get(&held.spec.id).unwrap(), Some(Ok(held)));
    }

    /// HEX-SOCK-6: an update reads and writes a record as one change, so changes made from many
    /// threads at once all land; a record that isn't there, or a change that changes nothing, is no
    /// update.
    #[test]
    fn updates_from_many_threads_all_land() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
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

    /// T-T7.1.2: the temp files a crash mid-write leaves are removed, and only they; a box's record
    /// and anything else in the directory stay.
    #[test]
    fn strays_are_removed_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store.put(&record("a")).unwrap();
        fs::write(dir.path().join(".a.tmp"), b"{ half").unwrap();
        fs::write(dir.path().join(".b.tmp"), b"").unwrap();
        fs::write(dir.path().join(".notes"), b"hi").unwrap();
        assert_eq!(store.remove_strays().unwrap(), 2);
        let mut left: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, [".notes", "a.json"]);
        assert_eq!(store.remove_strays().unwrap(), 0);
    }

    #[test]
    fn all_reads_every_record_and_ignores_strays() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store.put(&record("a")).unwrap();
        store.put(&record("b")).unwrap();
        // A stray temp file (a crash mid-write) and an unrelated file are not records.
        fs::write(dir.path().join(".c.tmp"), b"{}").unwrap();
        fs::write(dir.path().join("notes.txt"), b"hi").unwrap();
        // A damaged record is reported, not skipped.
        fs::write(dir.path().join("d.json"), b"{ not json").unwrap();
        let all = store.all().unwrap();
        assert_eq!(all.len(), 3);
        assert!(all[&BoxId::new("a").unwrap()].is_ok());
        assert!(all[&BoxId::new("d").unwrap()].is_err());
    }
}
