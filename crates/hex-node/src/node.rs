//! The Linux node agent: [`hex_api::Runtime`] over the `process` and `oci` classes, with its boxes'
//! records kept durably and recovered after a crash. An `oci` box is an OCI container the node runs
//! in a cgroup under its own box root with the box's bounds in force, as the `process` class does, so
//! one code path makes, bounds, deletes and recovers every box's cgroup; the container's init and
//! `exec` programs run through the `hex` binary's `__oci-init` and `__oci-exec` helpers (decision 3
//! of docs/design/runtime-architecture.md). The node agent is its containers' parent, holding each
//! init's pidfd and recording how it ended ([`crate::supervise`], decision 2).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
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
use crate::supervise::{self, Supervisor};

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
    /// The `oci` boxes' inits, watched and reaped; orphaned box processes, reaped.
    supervisor: Arc<Supervisor>,
    /// Abandoned programs ([`Node::abandoned`]) counted but not yet in their box's record, by box:
    /// kept and reported until a record write takes them, never dropped (§3).
    abandoned: Mutex<BTreeMap<BoxId, u64>>,
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
        Self::with_root(BoxRoot::open(box_root, disks)?, state_dir)
    }

    /// Opens a node agent in a cgroup subtree beside this process's own cgroup (the usual case: the
    /// node agent runs in a delegated leaf and its boxes in a sibling), records under `state_dir`.
    pub fn beside_self(state_dir: &Path, disk_path: &Path) -> Result<Self, Refusal> {
        let disks = disks::under(disk_path)?;
        Self::with_root(BoxRoot::beside_self(disks)?, state_dir)
    }

    /// The node over `root`, records under `state_dir`; this process becomes a child subreaper, so
    /// each `oci` box's init is its child once the shim exits.
    fn with_root(root: BoxRoot, state_dir: &Path) -> Result<Self, Refusal> {
        let store = Store::open(state_dir)?;
        let supervisor = Supervisor::start(root.dir(), store.clone())?;
        Ok(Self {
            root,
            store,
            oci_root: state_dir.join("oci"),
            oci_helper: None,
            supervisor,
            abandoned: Mutex::new(BTreeMap::new()),
        })
    }

    /// Sets how many `oci` boxes' inits the node watches at once, [`supervise::INIT_BUDGET`] unless
    /// set: past it `create` of an `oci` box is refused with [`Refusal::Exhausted`].
    pub fn with_init_budget(self, budget: usize) -> Result<Self, Refusal> {
        self.supervisor.set_budget(budget)?;
        Ok(self)
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
        let exit = match record.spec.class {
            ClassSpec::Process => None,
            // Read after the state, so an init that ended since is reaped here and reported.
            ClassSpec::Oci(_) | ClassSpec::Bundle(_) => self.init_exit(record, state)?,
        };
        Ok(BoxInfo {
            id: id.clone(),
            class: record.spec.class.class(),
            // An oci box is its init: once that has ended, the box is empty.
            state: if exit.is_some() {
                BoxState::Empty
            } else {
                state
            },
            bounds: record.spec.bounds.clone(),
            adopted,
            exit,
            abandoned: record.abandoned.saturating_add(self.unrecorded(id)?),
        })
    }

    /// Counts in box `id` a program exec'd in it that the node ended because the caller watching it
    /// went away: its connection to the node's socket closed while it ran (decision 3's disconnect
    /// rule, [`crate::serve`]). Written to the box's record, which inspect reports. A count a write
    /// can't take now is kept, reported all the same, and written with the next; a box with no
    /// readable record (deleted since) has none to take it.
    pub fn abandoned(&self, id: &BoxId) -> Result<(), Refusal> {
        let mut pending = self.abandoned.lock().map_err(|_| {
            Refusal::Broken("the node's abandoned-program counts' lock was poisoned".to_owned())
        })?;
        let count = pending.entry(id.clone()).or_insert(0);
        // Saturating: 2^64 programs is past any box's life, so the cap is never met.
        *count = count.saturating_add(1);
        let add = *count;
        self.store
            .update(id, |r| r.abandoned = r.abandoned.saturating_add(add))?;
        pending.remove(id);
        Ok(())
    }

    /// Box `id`'s abandoned programs not yet in its record.
    fn unrecorded(&self, id: &BoxId) -> Result<u64, Refusal> {
        let pending = self.abandoned.lock().map_err(|_| {
            Refusal::Broken("the node's abandoned-program counts' lock was poisoned".to_owned())
        })?;
        Ok(pending.get(id).copied().unwrap_or(0))
    }

    /// How an `oci` box's init ended: as the supervisor reaped it, or as the box's record holds it
    /// (reaped since `record` was read, or before a restart). An init that ended where the agent
    /// couldn't reap it, not its child after a restart, ended in a way no one recorded:
    /// [`Exit::Unknown`].
    fn init_exit(
        &self,
        record: &BoxRecord,
        state: BoxState,
    ) -> Result<Option<hex_api::Exit>, Refusal> {
        let id = &record.spec.id;
        if let Some(exit) = self.supervisor.exit(id)? {
            return Ok(Some(exit));
        }
        let recorded = match self.record(id)? {
            Some(now) => now.exit,
            None => record.exit,
        };
        let ended_unseen = state == BoxState::Empty && record.init.is_some();
        Ok(match recorded {
            Some(exit) => Some(exit),
            None if ended_unseen => Some(hex_api::Exit::Unknown),
            None => None,
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
        // A place for the init's pidfd first: past the budget nothing is made, so no init is ever
        // left unwatched.
        let slot = self.supervisor.reserve()?;
        // From the shim's spawn until the init is watched, no sweep runs: the shim is reaped by
        // create and the init by the supervisor, never by a sweep.
        let _spawning = self.supervisor.spawning()?;
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
        // The init is blocked at the exec FIFO, alive, and the agent's child since the shim exited;
        // record which process it is so a later restart can tell it from a recycled pid (recovery's
        // init_alive), and watch it through its pidfd from now on.
        let watched = self
            .oci()
            .state(id.as_str())
            .map_err(Refusal::from)
            .and_then(|state| {
                record.init = init_proc_id(state.pid, state.start_time);
                record.phase = Phase::Ready;
                self.store.put(&record)?;
                let start_ticks = state.start_time.ok_or_else(|| {
                    Refusal::Broken(format!("container {id}'s state has no start time"))
                })?;
                self.supervisor.watch(slot, &id, state.pid, start_ticks)
            });
        if let Err(e) = watched {
            let _ = self.scrub(&id);
            return Err(e);
        }
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
        let _spawning = self.supervisor.spawning()?;
        let child = cmd
            .spawn()
            .map_err(|e| Refusal::os("starting a program in a container", e))?;
        // The child stays unwaited (dropping a std Child neither waits nor kills), so its pid is ours
        // until the pidfd reaps it.
        self.supervisor
            .own(Box::new(PidfdProc::of_child(child.id())?))
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
            // An oci box follows the container's lifecycle, refused as the simulator refuses it
            // (HEX-RT-10): `created` starts; `running` has started already (Invalid); `stopped` or
            // mid-create has no live init to start (NotRunning). Mapped here so the node and the
            // simulator agree, rather than passing the oci runtime's WrongState (a generic Os refusal).
            Some(ClassSpec::Oci(_) | ClassSpec::Bundle(_)) => {
                match self.oci().state(id.as_str())?.status {
                    Status::Created => Ok(self.oci().start(id.as_str())?),
                    Status::Running => Err(Refusal::Invalid {
                        what: "start",
                        why: format!("container {id} has started already"),
                    }),
                    Status::Creating | Status::Stopped => {
                        Err(Refusal::NotRunning(id.clone(), "start"))
                    }
                }
            }
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
                let _spawning = self.supervisor.spawning()?;
                self.supervisor
                    .own(Box::new(process::exec(&cg, &program, stdio)?))
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
                    exit: None,
                    abandoned: 0,
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
        // Unwatched first, so nothing writes the record on the init's behalf once delete has.
        let init = self.supervisor.forget(id)?;
        if let Some(mut record) = record {
            record.phase = Phase::Deleting;
            self.store.put(&record)?;
        }
        self.scrub(id)?;
        if let Ok(mut pending) = self.abandoned.lock() {
            pending.remove(id);
        }
        // The box is empty, so its init has ended: reaped here, leaving no zombie of it.
        init.as_ref().map_or(Ok(()), supervise::reap)
    }
}

impl crate::serve::Host for Node {
    fn abandoned(&self, id: &BoxId) -> Result<(), Refusal> {
        Node::abandoned(self, id)
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

/// Whether the very process a record named as its container's init still runs: a live process at its
/// pid with the start time recorded, as crun's `process-start-time` tells a container from a recycled
/// pid. A recorded start time of 0 means it wasn't known at record time (state without the field), so
/// the pid alone decides, as [`hex_runtime::linux::oci::state::State::init_alive`] does for that case.
fn init_alive(init: ProcId) -> bool {
    match i32::try_from(init.pid).ok().and_then(start_time) {
        None => false,
        Some(now) => init.start_ticks == 0 || now == init.start_ticks,
    }
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
        // A 0 start time means it wasn't recorded (state without the field): the live pid decides,
        // as the oci runtime's own init_alive does, so recovery doesn't scrub a running container.
        let unknown = ProcId {
            pid: here.pid,
            start_ticks: 0,
        };
        assert!(
            init_alive(unknown),
            "a live pid with an unrecorded start time is alive"
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
