//! A node over any OS's runtime (native plan step 8): each box's record kept durably in the node's state
//! directory beside the runtime that makes the box ([`crate::store`], the records Linux's node keeps), and the
//! programs the socket's disconnect rule ends counted into it ([`crate::abandoned`]), so a node served on
//! any OS's socket ([`crate::serve`]) answers as Linux's does. Linux's node ([`crate::Node`]) keeps the same
//! records and counts through the same code, with its cgroups and containers beside them; macOS's node is
//! this over its runtime (the `process` class, and Linux images in its Shards guest).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Mutex;

use hex_api::{
    BoxId, BoxInfo, BoxSpec, Class, Proc, Program, Refusal, Runtime, Signal, Stdio, Usage,
};
use hex_core::netns;
use hex_core::record::{BoxRecord, Phase, ProcId};
use hex_core::recovery::{self, Decision, Seen, Why};

use crate::abandoned::Abandoned;
use crate::serve::Host;
use crate::store::Store;

/// What a recovery did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoverySummary {
    /// Boxes taken back as they were.
    pub adopted: Vec<BoxId>,
    /// Boxes removed, each with why.
    pub removed: Vec<(BoxId, String)>,
}

/// Why a box was removed, for its summary.
pub(crate) fn why_text(why: &Why) -> String {
    match why {
        Why::MidCreate => "the node agent stopped while it was being made".to_owned(),
        Why::MidDelete => "the node agent stopped while it was being removed".to_owned(),
        Why::InitGone => "its container's init had ended".to_owned(),
        Why::NoCgroup => "its record had no cgroup".to_owned(),
        Why::NoRecord => "its cgroup had no record".to_owned(),
        Why::Unreadable(w) => format!("its record couldn't be read: {w}"),
    }
}

/// What a node needs of its runtime to take its boxes back after a crash (T-T7.1.2).
pub trait Recover: Runtime {
    /// What names box `id`'s processes after the node is gone, recorded with the box once it has any (its
    /// first member started): a process and its start time, which tells it from one given its pid since.
    fn trace(&self, id: &BoxId) -> Result<Option<ProcId>, Refusal>;
    /// Takes back box `spec`, its processes the ones `trace` names where they still run.
    fn adopt(&self, spec: &BoxSpec, trace: Option<ProcId>) -> Result<(), Refusal>;
    /// Ends what still runs of what `trace` names, for a box recovery removes.
    fn end(&self, trace: Option<ProcId>) -> Result<(), Refusal>;
}

/// A runtime whose boxes' records the node keeps.
pub struct Recorded<R> {
    runtime: R,
    store: Store,
    abandoned: Abandoned,
    /// Boxes a recovery took back, which inspect reports as adopted.
    adopted: Mutex<BTreeSet<BoxId>>,
}

impl<R: Recover> Recorded<R> {
    /// `runtime`'s boxes, their records in `state_dir`. Does not recover; call [`Recorded::recover`].
    pub fn open(runtime: R, state_dir: &Path) -> Result<Self, Refusal> {
        Ok(Self {
            runtime,
            store: Store::open(state_dir)?,
            abandoned: Abandoned::default(),
            adopted: Mutex::new(BTreeSet::new()),
        })
    }

    /// After a restart: takes each box back or cleans it up as its record says (T-T7.1.2), by
    /// `hex_core::recovery`'s decisions, Linux's node's. A box is a record and the runtime's processes it
    /// names: a record made is the box, whatever runs in it now, so it is taken back; one being made or
    /// removed when the node stopped is removed, with what its record names ended. Safe to call again after
    /// a crash during it: ending a box tolerates its processes being gone already.
    pub fn recover(&self) -> Result<RecoverySummary, Refusal> {
        // What a crash at a record write left: a temp file never renamed into place.
        self.store.remove_strays()?;
        let mut summary = RecoverySummary::default();
        let records = self.store.all()?;
        let seen: BTreeMap<BoxId, Seen> = records
            .iter()
            .map(|(id, record)| {
                let alive = match record {
                    Ok(r) if r.spec.class.class() == Class::Oci => Some(false),
                    _ => None,
                };
                (
                    id.clone(),
                    Seen {
                        record: Some(record.clone()),
                        cgroup: true,
                        init_alive: alive,
                    },
                )
            })
            .collect();
        for (id, decision) in recovery::plan(&seen) {
            let record = records.get(&id).and_then(|r| r.as_ref().ok());
            let trace = record.and_then(|r| r.init);
            match decision {
                Decision::Adopt => {
                    if let Some(r) = record {
                        self.runtime.adopt(&r.spec, trace)?;
                    }
                    self.adopted
                        .lock()
                        .map_err(|_| poisoned())?
                        .insert(id.clone());
                    summary.adopted.push(id);
                }
                Decision::Remove(why) => {
                    self.runtime.end(trace)?;
                    self.store.remove(&id)?;
                    summary.removed.push((id, why_text(&why)));
                }
            }
        }
        Ok(summary)
    }

    /// `info` with the abandoned programs counted for its box, recorded and not yet, and whether a recovery
    /// took it back.
    fn counted(&self, mut info: BoxInfo) -> Result<BoxInfo, Refusal> {
        let recorded = match self.store.get(&info.id)? {
            Some(Ok(record)) => record.abandoned,
            _ => 0,
        };
        info.abandoned = recorded.saturating_add(self.abandoned.unrecorded(&info.id)?);
        info.adopted |= self
            .adopted
            .lock()
            .map_err(|_| poisoned())?
            .contains(&info.id);
        Ok(info)
    }
}

fn poisoned() -> Refusal {
    Refusal::Broken("the node's adopted boxes' lock was poisoned".to_owned())
}

impl<R: Recover> Runtime for Recorded<R> {
    fn classes(&self) -> Vec<Class> {
        self.runtime.classes()
    }

    /// Records the box as being made before anything is made, so a crash leaves a half-made box known as
    /// one (T-T7.1.2), then as made once the runtime has made it; a refused create leaves no record.
    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        let mut record = BoxRecord::new(spec.clone(), Phase::Creating, Vec::new());
        self.store.put(&record)?;
        let info = match self.runtime.create(spec) {
            Ok(info) => info,
            Err(why) => {
                let _ = self.store.remove(&record.spec.id);
                return Err(why);
            }
        };
        record.phase = Phase::Ready;
        self.store.put(&record)?;
        self.counted(info)
    }

    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        self.runtime.start(id)
    }

    /// Runs `program`, and once the box has processes, records what names them after a crash.
    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        let proc = self.runtime.exec(id, program, stdio)?;
        if let Some(Ok(record)) = self.store.get(id)?
            && record.init.is_none()
            && let Some(trace) = self.runtime.trace(id)?
        {
            self.store.update(id, |r| {
                r.init = Some(trace);
                true
            })?;
        }
        Ok(proc)
    }

    fn signal(&self, id: &BoxId, signal: Signal) -> Result<(), Refusal> {
        self.runtime.signal(id, signal)
    }

    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
        self.counted(self.runtime.inspect(id)?)
    }

    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        self.runtime.usage(id)
    }

    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        self.runtime
            .list()?
            .into_iter()
            .map(|info| self.counted(info))
            .collect()
    }

    /// Records the box as being deleted, deletes it, then removes its record: a crash between leaves a
    /// record recovery finishes.
    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        if let Some(Ok(mut record)) = self.store.get(id)? {
            record.phase = Phase::Deleting;
            self.store.put(&record)?;
        }
        self.runtime.delete(id)?;
        self.store.remove(id)?;
        self.abandoned.forget(id);
        if let Ok(mut adopted) = self.adopted.lock() {
            adopted.remove(id);
        }
        Ok(())
    }
}

impl<R: Recover> Host for Recorded<R> {
    fn abandoned(&self, id: &BoxId) -> Result<(), Refusal> {
        self.abandoned.count(&self.store, id)
    }

    /// Network namespaces are Linux's: a node elsewhere has none to count.
    fn netns(&self) -> Result<netns::Counts, Refusal> {
        Err(Refusal::Platform {
            what: "the node's network namespace counts",
            platform: std::env::consts::OS,
            why: "network namespaces are Linux's; this node makes none".to_owned(),
        })
    }
}

/// The macOS `process` class: a box's processes are its process group, named by its leader's pid and start
/// time (`proc_pid_rusage`'s Mach absolute time, in [`ProcId::start_ticks`]).
#[cfg(target_os = "macos")]
impl Recover for hex_runtime::macos::MacRuntime {
    fn trace(&self, id: &BoxId) -> Result<Option<ProcId>, Refusal> {
        Ok(self.group(id)?.and_then(|(pgid, started)| {
            Some(ProcId {
                pid: u32::try_from(pgid).ok()?,
                start_ticks: started,
            })
        }))
    }

    fn adopt(&self, spec: &BoxSpec, trace: Option<ProcId>) -> Result<(), Refusal> {
        hex_runtime::macos::MacRuntime::adopt(self, spec, group(trace))
    }

    fn end(&self, trace: Option<ProcId>) -> Result<(), Refusal> {
        hex_runtime::macos::MacRuntime::end(group(trace))
    }
}

#[cfg(target_os = "macos")]
fn group(trace: Option<ProcId>) -> Option<(i32, u64)> {
    trace.and_then(|t| Some((i32::try_from(t.pid).ok()?, t.start_ticks)))
}

/// A node whose Linux images are its Shards guest's: its `process` boxes are taken back through the host's
/// runtime. Its `oci` boxes lived in the guest the node booted, which went with the node (the node boots a
/// new one when it starts, HEX-GUEST-39), so their records are removed (`hex_core::recovery`: their init has
/// ended) and they have no host processes to end.
impl<P: Recover> Recover for hex_runtime::guest::WithGuest<P> {
    fn trace(&self, id: &BoxId) -> Result<Option<ProcId>, Refusal> {
        match self.host().inspect(id) {
            Ok(_) => self.host().trace(id),
            Err(Refusal::NoSuchBox(_)) => Ok(None),
            Err(why) => Err(why),
        }
    }

    fn adopt(&self, spec: &BoxSpec, trace: Option<ProcId>) -> Result<(), Refusal> {
        self.host().adopt(spec, trace)
    }

    fn end(&self, trace: Option<ProcId>) -> Result<(), Refusal> {
        self.host().end(trace)
    }
}
