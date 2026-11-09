//! A node's placement records in files: the shell `hex_sim::peer` writes through on a laptop with no
//! plane and no Amphora, and on every node of a cluster with no plane. One file per record, written
//! whole to a temp file, flushed, and renamed over the record, so a crash of the node agent or of its
//! host leaves the old record or the new one, never a torn one; a temp file a crash left is removed
//! at open.
//!
//! The rename is `std::fs::rename` on every OS: rename(2) on Linux and macOS, `MoveFileExW` with
//! `MOVEFILE_REPLACE_EXISTING` on Windows, which replaces a record another handle has open so long as
//! that handle shares delete, as every handle std opens does (an outside program holding a record
//! without it makes the write fail, typed). On Linux and macOS the directory is flushed after a rename
//! or removal, so the name change itself survives a host crash; Windows offers no directory flush
//! through std, and NTFS journals the rename itself.
//!
//! Bounds (CLAUDE.md §2): a node keeps a record for each box it runs and each it owns, each at most
//! [`hex_runtime::MAX_BOXES`], and its epoch: a directory holding more is refused, typed.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use hex_sim::peer::{Records, Stop};

/// The most records a node keeps: a record for each box it runs and each it owns, and its epoch.
pub const MOST: usize = hex_runtime::MAX_BOXES * 2 + 1;

const RECORD: &str = "rec";
const TEMP: &str = "tmp";

/// A node's placement records in `dir`.
#[derive(Debug)]
pub struct Files {
    dir: PathBuf,
}

fn io(op: &'static str) -> impl Fn(std::io::Error) -> Stop {
    move |e| Stop::Io(format!("{op}: {e}"))
}

/// A record's name is letters, digits and `-`: never a path, never a name Windows reserves (they are
/// all letters, and every record's name but the epoch's has a `-` and digits).
fn checked(name: &str) -> Result<&str, Stop> {
    let ok = !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok {
        Ok(name)
    } else {
        Err(Stop::Damaged(format!("a record named {name:?}")))
    }
}

impl Files {
    /// Opens the records in `dir`, made if missing; temp files a crash left are removed.
    pub fn open(dir: &Path) -> Result<Files, Stop> {
        fs::create_dir_all(dir).map_err(io("making the records' folder"))?;
        let files = Files {
            dir: dir.to_path_buf(),
        };
        for path in files.listed()? {
            if path.extension().is_some_and(|e| e == TEMP) {
                fs::remove_file(&path).map_err(io("removing a temp record"))?;
            }
        }
        files.sync_dir()?;
        Ok(files)
    }

    fn path(&self, name: &str, ext: &str) -> PathBuf {
        self.dir.join(format!("{name}.{ext}"))
    }

    /// Every file in the folder, at most [`MOST`] of them and the temp files beside.
    fn listed(&self) -> Result<Vec<PathBuf>, Stop> {
        let mut out = Vec::new();
        for entry in fs::read_dir(&self.dir).map_err(io("listing the records"))? {
            let entry = entry.map_err(io("listing the records"))?;
            if out.len() >= MOST.saturating_mul(2) {
                return Err(Stop::Damaged(format!(
                    "more than {MOST} records in {}",
                    self.dir.display()
                )));
            }
            out.push(entry.path());
        }
        Ok(out)
    }

    /// The folder's own entry flushed, so a rename or removal in it survives the host.
    #[cfg(unix)]
    fn sync_dir(&self) -> Result<(), Stop> {
        File::open(&self.dir)
            .and_then(|d| d.sync_all())
            .map_err(io("flushing the records' folder"))
    }

    /// NTFS journals the rename itself; Windows offers no directory flush through std.
    #[cfg(not(unix))]
    fn sync_dir(&self) -> Result<(), Stop> {
        Ok(())
    }
}

impl Records for Files {
    fn put(&mut self, name: &str, value: &str) -> Result<(), Stop> {
        let name = checked(name)?;
        let (temp, record) = (self.path(name, TEMP), self.path(name, RECORD));
        {
            let mut f = File::create(&temp).map_err(io("making a temp record"))?;
            f.write_all(value.as_bytes())
                .map_err(io("writing a temp record"))?;
            f.sync_all().map_err(io("flushing a temp record"))?;
        }
        fs::rename(&temp, &record).map_err(io("renaming a record into place"))?;
        self.sync_dir()
    }

    fn remove(&mut self, name: &str) -> Result<(), Stop> {
        let name = checked(name)?;
        match fs::remove_file(self.path(name, RECORD)) {
            Ok(()) => self.sync_dir(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io("removing a record")(e)),
        }
    }

    fn all(&mut self) -> Result<Vec<(String, String)>, Stop> {
        let mut out = Vec::new();
        for path in self.listed()? {
            if path.extension().is_none_or(|e| e != RECORD) {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                return Err(Stop::Damaged(format!("a record at {}", path.display())));
            };
            let name = checked(name)?.to_owned();
            let value = fs::read_to_string(&path).map_err(io("reading a record"))?;
            out.push((name, value));
        }
        out.sort_unstable();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_sim::model::{NodeId, NodeSpec, Resources};
    use hex_sim::peer::World;

    /// CP-T7.4, laptop-only state in files: do write, replace and remove records, with one replaced
    /// while another handle has it open for reading, and a temp file a crash left beside them; expect
    /// each read back whole, the replaced one new, the removed one gone, and the temp gone at open, on
    /// every OS (Windows' rename replaces a file a std handle has open, since std shares delete).
    #[test]
    fn records_are_written_whole_and_replaced_by_rename_on_every_os() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = Files::open(dir.path()).unwrap();
        f.put("epoch", "1").unwrap();
        f.put("own-0-1-0", "asked 0 1 1 0 0").unwrap();
        let reader = File::open(dir.path().join("own-0-1-0.rec")).unwrap();
        f.put("own-0-1-0", "at 0 1 1 0 0").unwrap();
        drop(reader);
        f.put("ran-0-1-0", "0 running 1 1 0 0").unwrap();
        f.remove("ran-0-1-0").unwrap();
        f.remove("ran-0-1-0").unwrap();
        fs::write(dir.path().join("own-0-1-1.tmp"), "torn").unwrap();
        let mut again = Files::open(dir.path()).unwrap();
        assert_eq!(
            again.all().unwrap(),
            vec![
                ("epoch".to_owned(), "1".to_owned()),
                ("own-0-1-0".to_owned(), "at 0 1 1 0 0".to_owned()),
            ]
        );
        assert!(!dir.path().join("own-0-1-1.tmp").exists());
        for bad in ["../x", "a/b", "A", "", "own_0"] {
            assert!(
                matches!(again.put(bad, "x"), Err(Stop::Damaged(_))),
                "{bad}"
            );
        }
    }

    /// CP-T7.4, laptop-only: do run one node with no plane and no Amphora, its records in files, for 6
    /// seeds of 120 steps under the crash nemesis (its agent crashing at any write, before or after it
    /// lands, and starting again from the files while its boxes run on); expect every box asked and
    /// answered created once and its end told (lost-or-double 0), and nothing left in its ledger or its
    /// records after every box ended; and (non-vacuity) crashes struck and boxes ran.
    #[test]
    fn a_laptop_keeps_every_box_once_with_its_records_in_files() {
        let (mut started, mut crashes) = (0u64, 0u64);
        for seed in 0..6 {
            let dir = tempfile::tempdir().unwrap();
            let capacity = Resources {
                cpu_milli: 4_000,
                mem_bytes: 1 << 30,
                ..Resources::default()
            };
            let mut w = World::new(
                vec![NodeSpec::new(NodeId(0), capacity)],
                vec![Files::open(dir.path()).unwrap()],
                seed,
            )
            .unwrap();
            for _ in 0..120 {
                w.step().unwrap_or_else(|e| panic!("seed {seed}: {e:?}"));
            }
            let r = w
                .finish(1 << 16)
                .unwrap_or_else(|e| panic!("seed {seed}: {e:?}"));
            assert_eq!(r.lost_or_double(), 0, "seed {seed}: {r:?}");
            assert_eq!(
                Files::open(dir.path()).unwrap().all().unwrap().len(),
                1,
                "seed {seed}: only the epoch is left"
            );
            started += r.started;
            crashes += r.crashes;
        }
        assert!(
            started > 0 && crashes > 0,
            "started {started}, crashes {crashes}"
        );
    }
}
