//! The node agent's durable store of box records (CLAUDE.md §4: durable before acknowledged). Each
//! box is one file, written whole by a temp file, fsync, rename, then an fsync of the directory, so
//! a crash leaves either the old record or the new one, never a torn one (T-T7.1.2).

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use hex_api::{BoxId, Refusal};
use hex_core::record::BoxRecord;

/// Box records on disk, one file each, named `<id>.json`.
#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// Opens the store at `dir`, making it when it's missing.
    pub fn open(dir: &Path) -> Result<Self, Refusal> {
        fs::create_dir_all(dir).map_err(|e| Refusal::os("making the record directory", e))?;
        Ok(Self {
            dir: dir.to_owned(),
        })
    }

    fn path(&self, id: &BoxId) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// Writes `record` durably: either the old file or the whole new one survives a crash.
    pub fn put(&self, record: &BoxRecord) -> Result<(), Refusal> {
        let bytes = record.encode()?;
        let final_path = self.path(&record.spec.id);
        let tmp = self.dir.join(format!(".{}.tmp", record.spec.id));
        {
            let mut f =
                File::create(&tmp).map_err(|e| Refusal::os("making a record temp file", e))?;
            f.write_all(&bytes)
                .map_err(|e| Refusal::os("writing a record", e))?;
            f.sync_all()
                .map_err(|e| Refusal::os("flushing a record", e))?;
        }
        fs::rename(&tmp, &final_path)
            .map_err(|e| Refusal::os("renaming a record into place", e))?;
        self.sync_dir()
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

    /// Removes `id`'s record; absent already is removed.
    pub fn remove(&self, id: &BoxId) -> Result<(), Refusal> {
        match fs::remove_file(self.path(id)) {
            Ok(()) => self.sync_dir(),
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

    /// fsync the directory, so a rename or a remove is durable.
    fn sync_dir(&self) -> Result<(), Refusal> {
        File::open(&self.dir)
            .and_then(|d| d.sync_all())
            .map_err(|e| Refusal::os("flushing the record directory", e))
    }
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
