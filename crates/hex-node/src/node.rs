//! The Linux node agent: [`hex_api::Runtime`] over the `process` and `oci` classes, with its boxes'
//! records kept durably and recovered after a crash. An `oci` box is an OCI container the node runs
//! in a cgroup under its own box root with the box's bounds in force, as the `process` class does, so
//! one code path makes, bounds, deletes and recovers every box's cgroup; the container's init and
//! `exec` programs run through the `hex` binary's `__oci-init` and `__oci-exec` helpers (decision 3
//! of docs/design/runtime-architecture.md).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hex_api::{
    BoxId, BoxInfo, BoxSpec, BoxState, Class, ClassSpec, Proc, Program, Refusal, Runtime, Signal,
    Stdio, Usage,
};
use hex_core::cgroup::Dev;
use hex_core::record::{BoxRecord, Phase, ProcId};
use hex_core::recovery::{self, Decision, Seen, Why};
use hex_runtime::linux::cgroup::BoxRoot;
use hex_runtime::linux::oci::run::Runtime as OciRuntime;
use hex_runtime::linux::oci::state::{Status, start_time};
use hex_runtime::linux::proc::PidfdProc;
use hex_runtime::linux::{disks, process};

use crate::store::Store;

/// How long [`Runtime::delete`] waits for a box to empty after killing it before it reports the
/// kernel is wedged. A `cgroup.kill` empties a box in milliseconds; a minute is far past any real
/// delay, so missing it is a fault to report, not a limit to raise (CLAUDE.md §4).
pub const EMPTY_PATIENCE: Duration = Duration::from_secs(60);

/// A node agent over the `process` and `oci` classes.
pub struct Node {
    root: BoxRoot,
    store: Store,
    /// The state root the `oci` class keeps each container's `state.json` and exec FIFO under.
    oci_root: PathBuf,
    /// The `hex` binary the `oci` class re-execs for a container's init and `exec` helper; `None`
    /// when this process's own binary is `hex` (it handles `__oci-init`/`__oci-exec`). A node linked
    /// into another program, or the bench, names it ([`Node::with_oci_helper`]).
    oci_helper: Option<PathBuf>,
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
        Ok(Self {
            root,
            store,
            oci_root: state_dir.join("oci"),
            oci_helper: None,
        })
    }

    /// Opens a node agent in a cgroup subtree beside this process's own cgroup (the usual case: the
    /// node agent runs in a delegated leaf and its boxes in a sibling), records under `state_dir`.
    pub fn beside_self(state_dir: &Path, disk_path: &Path) -> Result<Self, Refusal> {
        let disks = disks::under(disk_path)?;
        let root = BoxRoot::beside_self(disks)?;
        let store = Store::open(state_dir)?;
        Ok(Self {
            root,
            store,
            oci_root: state_dir.join("oci"),
            oci_helper: None,
        })
    }

    /// Names the `hex` binary the `oci` class re-execs for a container's init and `exec` helper, for
    /// a node whose own binary isn't `hex` (the bench, a node linked into another program).
    pub fn with_oci_helper(mut self, hex_binary: PathBuf) -> Self {
        self.oci_helper = Some(hex_binary);
        self
    }

    /// The `oci` runtime, over the node's state root and helper binary.
    fn oci(&self) -> OciRuntime {
        match &self.oci_helper {
            Some(exe) => OciRuntime::with_helper(self.oci_root.clone(), exe.clone()),
            None => OciRuntime::new(self.oci_root.clone()),
        }
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
        let id = &record.spec.id;
        let state = match record.spec.class {
            // A process box runs when its cgroup holds a process; it has no `created` state.
            ClassSpec::Process => match self.root.get(id) {
                Some(cg) if cg.populated()? => BoxState::Running,
                _ => BoxState::Empty,
            },
            // An oci box follows the container's lifecycle: `created` until `start`, then `running`,
            // then `empty` once its init no longer runs (the oci state's `stopped`).
            ClassSpec::Oci(_) | ClassSpec::Bundle(_) => {
                match self.oci().state(id.as_str()).map(|s| s.status) {
                    Ok(Status::Created) => BoxState::Created,
                    Ok(Status::Running) => BoxState::Running,
                    Ok(Status::Creating | Status::Stopped) => BoxState::Empty,
                    // No container state yet (mid-create) or it's gone: nothing runs.
                    Err(_) => BoxState::Empty,
                }
            }
        };
        Ok(BoxInfo {
            id: id.clone(),
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
            // The `process` class has no init; an `oci` box's init is alive when the very process its
            // record named still runs (its pid with the start time create recorded), as crun tells a
            // live container from a recycled pid by `process-start-time`.
            if let Some(Ok(record)) = &s.record
                && record.spec.class.class() == Class::Oci
            {
                s.init_alive = Some(record.init.is_some_and(init_alive));
            }
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

    /// A `process` box: its cgroup with the bounds in force, empty until [`Runtime::exec`].
    fn create_process(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
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

    /// An `oci` box: its cgroup under the box root with the bounds in force (as a `process` box), and
    /// the container's init started in it from `bundle`, blocked at the exec FIFO until
    /// [`Runtime::start`]. The container's state dir is the box's, removed with it by [`Node::scrub`].
    fn create_oci(&self, spec: BoxSpec, bundle: &Path) -> Result<BoxInfo, Refusal> {
        let id = spec.id.clone();
        // The container's own state directory, which the oci class makes; recorded as the box's, so
        // delete and recovery remove it with the box.
        let oci_dir = self.oci_root.join(id.as_str());
        let mut record = BoxRecord::new(spec.clone(), Phase::Creating, vec![oci_dir]);
        self.store.put(&record)?;
        let cgroup = match self.root.create(&id, &spec.bounds) {
            Ok(cgroup) => cgroup,
            Err(e) => {
                let _ = self.store.remove(&id);
                return Err(e);
            }
        };
        if let Err(e) = self
            .oci()
            .create_in_cgroup(id.as_str(), bundle, None, cgroup)
        {
            // The container, if it started, is in the cgroup; scrub ends it and removes the cgroup,
            // the state dir and the record.
            let _ = self.scrub(&id);
            return Err(e.into());
        }
        // The init is blocked at the exec FIFO, alive; record which process it is so a later restart
        // can tell it from a recycled pid (recovery's init_alive).
        let state = self.oci().state(id.as_str())?;
        record.init = init_proc_id(state.pid, state.start_time);
        record.phase = Phase::Ready;
        self.store.put(&record)?;
        self.info(&record, false)
    }

    /// `exec` in an `oci` box: the `hex __oci-exec` helper, spawned (not exec'd into) and watched by
    /// its pidfd, in seam mode so it ends the way its program did. No process is spawned but the
    /// helper that becomes the container's; a multi-threaded agent stays safe, since the helper
    /// re-execs to the single-threaded context joining namespaces needs.
    fn exec_oci(
        &self,
        id: &BoxId,
        program: Program,
        stdio: Stdio,
    ) -> Result<Box<dyn Proc>, Refusal> {
        program.check()?;
        let env = program
            .env
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        let user = program.user.as_ref().map(|u| (u.uid, Some(u.gid)));
        let mut cmd =
            self.oci()
                .exec_command(id.as_str(), program.args, program.cwd, env, user, true)?;
        set_stdio(&mut cmd, stdio);
        let child = cmd
            .spawn()
            .map_err(|e| Refusal::os("starting a program in a container", e))?;
        // The child stays unwaited (dropping a std Child neither waits nor kills), so its pid is ours
        // until the pidfd reaps it.
        Ok(Box::new(PidfdProc::of_child(child.id())?))
    }
}

impl Runtime for Node {
    fn classes(&self) -> Vec<Class> {
        vec![Class::Process, Class::Oci]
    }

    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        spec.bounds.check()?;
        if self.record(&spec.id)?.is_some() || self.root.get(&spec.id).is_some() {
            return Err(Refusal::Exists(spec.id));
        }
        match &spec.class {
            ClassSpec::Process => self.create_process(spec),
            ClassSpec::Bundle(bundle) => {
                let bundle = bundle.clone();
                self.create_oci(spec, &bundle)
            }
            // The structured `OciSpec` (kiln-seams' shape) isn't rendered to a bundle yet; callers
            // through the seam use `Bundle`. Refused by name rather than run half-configured.
            ClassSpec::Oci(_) => Err(Refusal::Invalid {
                what: "oci spec",
                why: "the node runs an oci box from a Bundle; the structured OciSpec isn't rendered yet"
                    .to_owned(),
            }),
        }
    }

    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        match self.record(id)?.map(|r| r.spec.class) {
            None => Err(Refusal::NoSuchBox(id.clone())),
            // A process box has no init to start: exec runs its programs. Refused as the simulator
            // refuses it (HEX-RT-10).
            Some(ClassSpec::Process) => Err(Refusal::Invalid {
                what: "start",
                why: format!("box {id} is a process box, which has no init to start"),
            }),
            Some(ClassSpec::Oci(_) | ClassSpec::Bundle(_)) => Ok(self.oci().start(id.as_str())?),
        }
    }

    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        match self.record(id)?.map(|r| r.spec.class) {
            None => Err(Refusal::NoSuchBox(id.clone())),
            Some(ClassSpec::Process) => {
                let cg = self
                    .root
                    .get(id)
                    .ok_or_else(|| Refusal::NoSuchBox(id.clone()))?;
                Ok(Box::new(process::exec(&cg, &program, stdio)?))
            }
            Some(ClassSpec::Oci(_) | ClassSpec::Bundle(_)) => self.exec_oci(id, program, stdio),
        }
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

/// The identity of a container's init, from its oci state: its pid and the start time create read, so
/// a later restart tells it from a process given its pid since (recovery's [`init_alive`]). `None`
/// when the state names no live pid.
fn init_proc_id(pid: i32, start_time: Option<u64>) -> Option<ProcId> {
    let pid = u32::try_from(pid).ok().filter(|p| *p > 0)?;
    Some(ProcId {
        pid,
        start_ticks: start_time.unwrap_or(0),
    })
}

/// Whether the very process a record named as its container's init still runs: its pid is live with
/// the start time recorded, as crun's `process-start-time` tells a container from a recycled pid.
fn init_alive(init: ProcId) -> bool {
    i32::try_from(init.pid)
        .ok()
        .and_then(start_time)
        .is_some_and(|now| now == init.start_ticks)
}

/// Sets a command's standard streams from a box's [`Stdio`]: each stream the caller gave, else the
/// null device, as the `oci` exec helper runs on the fds it's handed.
fn set_stdio(cmd: &mut Command, stdio: Stdio) {
    use std::os::fd::OwnedFd;
    use std::process::Stdio as Std;
    cmd.stdin(
        stdio
            .stdin
            .map_or_else(Std::null, |p| Std::from(OwnedFd::from(p))),
    );
    cmd.stdout(
        stdio
            .stdout
            .map_or_else(Std::null, |p| Std::from(OwnedFd::from(p))),
    );
    cmd.stderr(
        stdio
            .stderr
            .map_or_else(Std::null, |p| Std::from(OwnedFd::from(p))),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decision 2: an oci box's init is told from a process given its pid since by its start time, as
    /// crun's `process-start-time` does. A record naming the running test process is alive; one
    /// naming no live pid, or the right pid with the wrong start time, is not.
    #[test]
    fn an_inits_liveness_is_its_pid_and_start_time() {
        let me = i32::try_from(std::process::id()).unwrap();
        let ticks = start_time(me).expect("this process has a start time");
        let here = init_proc_id(me, Some(ticks)).expect("a live pid is an init");
        assert!(init_alive(here), "this process is its own live init");
        let moved = ProcId {
            pid: here.pid,
            start_ticks: ticks.wrapping_add(1),
        };
        assert!(
            !init_alive(moved),
            "a different start time isn't the same init"
        );
        assert_eq!(init_proc_id(0, Some(1)), None, "pid 0 names no init");
        assert_eq!(
            init_proc_id(-1, Some(1)),
            None,
            "a negative pid names no init"
        );
        let gone = ProcId {
            pid: 0x7fff_fffe,
            start_ticks: 1,
        };
        assert!(!init_alive(gone), "a pid with no live process isn't alive");
    }
}
