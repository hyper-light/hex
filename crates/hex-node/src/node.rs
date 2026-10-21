//! The Linux node agent: [`hex_api::Runtime`] over the `process` class, with its boxes' records kept
//! durably and recovered after a crash.

use std::path::{Path, PathBuf};
use std::time::Duration;

use hex_api::{
    BoxId, BoxInfo, BoxSpec, BoxState, Class, ClassSpec, Proc, Program, Refusal, Runtime, Signal,
    Stdio, Usage,
};
use hex_core::cgroup::Dev;
use hex_core::record::{BoxRecord, Phase};
use hex_core::recovery::{self, Decision, Seen, Why};
use hex_runtime::linux::cgroup::BoxRoot;
use hex_runtime::linux::{disks, process};

use crate::store::Store;

/// How long [`Runtime::delete`] waits for a box to empty after killing it before it reports the
/// kernel is wedged. A `cgroup.kill` empties a box in milliseconds; a minute is far past any real
/// delay, so missing it is a fault to report, not a limit to raise (CLAUDE.md §4).
pub const EMPTY_PATIENCE: Duration = Duration::from_secs(60);

/// A node agent over the `process` class.
pub struct Node {
    root: BoxRoot,
    store: Store,
}

/// What a recovery did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoverySummary {
    /// Boxes taken back as they were.
    pub adopted: Vec<BoxId>,
    /// Boxes removed, each with why.
    pub removed: Vec<(BoxId, String)>,
}

impl Node {
    /// Opens a node agent whose boxes live under cgroup `box_root` with their records in `state_dir`,
    /// the `io` bounds applying to the disks under `disk_path`. Does not recover; call [`Node::recover`].
    pub fn open(box_root: &Path, state_dir: &Path, disk_path: &Path) -> Result<Self, Refusal> {
        let disks = disks::under(disk_path)?;
        let root = BoxRoot::open(box_root, disks)?;
        let store = Store::open(state_dir)?;
        Ok(Self { root, store })
    }

    /// Opens a node agent in a cgroup subtree beside this process's own cgroup (the usual case: the
    /// node agent runs in a delegated leaf and its boxes in a sibling), records under `state_dir`.
    pub fn beside_self(state_dir: &Path, disk_path: &Path) -> Result<Self, Refusal> {
        let disks = disks::under(disk_path)?;
        let root = BoxRoot::beside_self(disks)?;
        let store = Store::open(state_dir)?;
        Ok(Self { root, store })
    }

    /// The disks the node holds boxes' files on, as `io.latency` and `io.max` name them.
    pub fn disks(&self) -> &[Dev] {
        self.root.disks()
    }

    fn record(&self, id: &BoxId) -> Result<Option<BoxRecord>, Refusal> {
        match self.store.get(id)? {
            None => Ok(None),
            Some(Ok(r)) => Ok(Some(r)),
            Some(Err(why)) => Err(Refusal::Broken(format!("box {id}'s record: {why}"))),
        }
    }

    fn info(&self, record: &BoxRecord, adopted: bool) -> Result<BoxInfo, Refusal> {
        let state = match self.root.get(&record.spec.id) {
            Some(cg) if cg.populated()? => BoxState::Running,
            _ => BoxState::Empty,
        };
        Ok(BoxInfo {
            id: record.spec.id.clone(),
            class: record.spec.class.class(),
            state,
            bounds: record.spec.bounds.clone(),
            adopted,
        })
    }

    /// After a restart: take each box back or clean it up (T-T7.1.2). Safe to call again after a
    /// crash during it, because removing a box tolerates each part being gone already.
    pub fn recover(&self) -> Result<RecoverySummary, Refusal> {
        let mut seen: std::collections::BTreeMap<BoxId, Seen> = std::collections::BTreeMap::new();
        for (id, record) in self.store.all()? {
            seen.insert(
                id,
                Seen {
                    record: Some(record),
                    cgroup: false,
                    init_alive: None,
                },
            );
        }
        for name in self.root.boxes()? {
            if let Ok(id) = BoxId::new(&name) {
                seen.entry(id).or_insert_with(|| Seen {
                    record: None,
                    cgroup: false,
                    init_alive: None,
                });
            }
        }
        for (id, s) in &mut seen {
            s.cgroup = self.root.get(id).is_some();
            // The `process` class has no init; `oci` sets `init_alive` from its init's liveness.
        }
        let mut summary = RecoverySummary::default();
        for (id, decision) in recovery::plan(&seen) {
            match decision {
                Decision::Adopt => summary.adopted.push(id),
                Decision::Remove(why) => {
                    self.scrub(&id)?;
                    summary.removed.push((id, why_text(&why)));
                }
            }
        }
        Ok(summary)
    }

    /// Ends what runs in a box and removes every trace of it, each part tolerant of being gone.
    fn scrub(&self, id: &BoxId) -> Result<(), Refusal> {
        if let Some(cg) = self.root.get(id) {
            cg.kill()?;
            if !cg.wait_empty(EMPTY_PATIENCE)? {
                return Err(Refusal::Os {
                    op: "emptying a box",
                    why: format!(
                        "box {id} still holds processes {EMPTY_PATIENCE:?} after cgroup.kill"
                    ),
                });
            }
            cg.remove()?;
        }
        if let Some(Ok(r)) = self.store.get(id)? {
            for dir in &r.dirs {
                remove_tree(dir)?;
            }
        }
        self.store.remove(id)
    }
}

impl Runtime for Node {
    fn classes(&self) -> Vec<Class> {
        // The `oci` class joins this list when it lands (STATUS.md); until then the node refuses it.
        vec![Class::Process]
    }

    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        spec.bounds.check()?;
        if let ClassSpec::Oci(_) = spec.class {
            return Err(Refusal::Unsupported(Class::Oci));
        }
        if self.record(&spec.id)?.is_some() || self.root.get(&spec.id).is_some() {
            return Err(Refusal::Exists(spec.id));
        }
        // Record the box as being made before the machine changes, so recovery knows a half-made
        // box is half-made (T-T7.1.2).
        let mut record = BoxRecord::new(spec.clone(), Phase::Creating, Vec::new());
        self.store.put(&record)?;
        if let Err(e) = self.root.create(&spec.id, &spec.bounds) {
            // The create failed; leave nothing behind.
            let _ = self.store.remove(&spec.id);
            return Err(e);
        }
        record.phase = Phase::Ready;
        self.store.put(&record)?;
        self.info(&record, false)
    }

    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        let cg = self
            .root
            .get(id)
            .ok_or_else(|| Refusal::NoSuchBox(id.clone()))?;
        let proc = process::exec(&cg, &program, stdio)?;
        Ok(Box::new(proc))
    }

    fn signal(&self, id: &BoxId, signal: Signal) -> Result<(), Refusal> {
        let cg = self
            .root
            .get(id)
            .ok_or_else(|| Refusal::NoSuchBox(id.clone()))?;
        cg.signal(id, signal)
    }

    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
        let record = self
            .record(id)?
            .ok_or_else(|| Refusal::NoSuchBox(id.clone()))?;
        // Whether a box was adopted is [`Node::recover`]'s to report; a later inspect doesn't carry it.
        self.info(&record, false)
    }

    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        let cg = self
            .root
            .get(id)
            .ok_or_else(|| Refusal::NoSuchBox(id.clone()))?;
        Ok(cg.usage())
    }

    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        let mut out = Vec::new();
        for (id, record) in self.store.all()? {
            if let Ok(record) = record {
                out.push(self.info(&record, false)?);
            } else {
                // A box whose record can't be read is reported as its id with an empty state rather
                // than hidden; recovery is what cleans it up.
                out.push(BoxInfo {
                    id,
                    class: Class::Process,
                    state: BoxState::Empty,
                    bounds: hex_api::Bounds::default(),
                    adopted: false,
                });
            }
        }
        Ok(out)
    }

    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        let record = self.record(id)?;
        if record.is_none() && self.root.get(id).is_none() {
            return Err(Refusal::NoSuchBox(id.clone()));
        }
        if let Some(mut record) = record {
            record.phase = Phase::Deleting;
            self.store.put(&record)?;
        }
        self.scrub(id)
    }
}

fn why_text(why: &Why) -> String {
    match why {
        Why::MidCreate => "the node agent stopped while it was being made".to_owned(),
        Why::MidDelete => "the node agent stopped while it was being removed".to_owned(),
        Why::InitGone => "its container's init had ended".to_owned(),
        Why::NoCgroup => "its record had no cgroup".to_owned(),
        Why::NoRecord => "its cgroup had no record".to_owned(),
        Why::Unreadable(w) => format!("its record couldn't be read: {w}"),
    }
}

/// Removes a directory tree the box owns; absent already is removed.
fn remove_tree(dir: &PathBuf) -> Result<(), Refusal> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Refusal::os("removing a box's directory", e)),
    }
}
