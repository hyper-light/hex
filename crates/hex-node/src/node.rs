//! The Linux node agent: [`hex_api::Runtime`] over the `process` and `oci` classes, with its boxes'
//! records kept durably and recovered after a crash. An `oci` box is an OCI container the node runs
//! in a cgroup under its own box root with the box's bounds in force, as the `process` class does, so
//! one code path makes, bounds, deletes and recovers every box's cgroup; the container's init and
//! `exec` programs run through the `hex` binary's `__oci-init` and `__oci-exec` helpers (decision 3
//! of docs/design/runtime-architecture.md). A box made from the structured [`hex_api::OciSpec`] runs
//! from a bundle the node renders into a directory of the box's own ([`hex_core::bundle`]), holding
//! its writable layer, which goes when the box does. The node agent is its containers' parent,
//! holding each init's pidfd and recording how it ended ([`crate::supervise`], decision 2).

use std::collections::BTreeMap;
use std::fs;
use std::os::fd::AsFd;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use hex_api::{
    BoxId, BoxInfo, BoxSpec, BoxState, Class, ClassSpec, OciSpec, Proc, Program, Refusal, Rootfs,
    Runtime, Signal, Stdio, Usage,
};
use hex_core::bundle::{self, Bundle, Owner, Place};
use hex_core::cgroup::Dev;
use hex_core::record::{BoxRecord, Phase, ProcId};
use hex_core::recovery::{self, Decision, Seen, Why};
use hex_runtime::linux::cgroup::BoxRoot;
use hex_runtime::linux::oci::OciError;
use hex_runtime::linux::oci::run::Runtime as OciRuntime;
use hex_runtime::linux::oci::seed::{Direct, Specs, exec_direct, open_cgroup};
use hex_runtime::linux::oci::state::{Status, start_time};
use hex_runtime::linux::proc::PidfdProc;
use hex_runtime::linux::{disks, process};

use crate::remove::remove_tree;
use crate::seed::SeedPool;
use crate::store::Store;
use crate::supervise::{self, Live, Supervisor};

/// overlayfs's magic number (include/uapi/linux/magic.h): a node whose state is on overlayfs can't
/// keep a layered box's writable layer there, since overlayfs can't be an overlay's upper layer.
const OVERLAYFS_SUPER_MAGIC: i128 = 0x794c_7630;

/// How long [`Runtime::delete`] waits for a box to empty after killing it before it reports the
/// kernel is wedged. A `cgroup.kill` empties a box in milliseconds; a minute is far past any real
/// delay, so missing it is a fault to report, not a limit to raise (CLAUDE.md §4).
pub const EMPTY_PATIENCE: Duration = Duration::from_secs(60);

/// How long [`Runtime::delete`] spends taking a box's directories apart before it refuses, leaving
/// the rest for a later delete or recovery to finish where it stopped: a bound on a caller's wait, not
/// a guess at how long a layer takes, so a layer of any size goes, over as many calls as it needs.
pub const REMOVE_PATIENCE: Duration = Duration::from_secs(60);

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
    /// Where each box made from an [`OciSpec`] has its bundle: `<bundles>/<id>`, its writable layer
    /// in it.
    bundles: PathBuf,
    /// Who this node runs as, which decides the user namespace its containers get.
    owner: Owner,
    /// The host's paths every container has masked beyond Docker's list ([`Place::masked`]).
    masked: Vec<String>,
    /// The null device, open for reading and writing: what an exec's unset stdio streams get.
    null: std::os::fd::OwnedFd,
    /// The bundles' parsed configs an exec straight from the node's threads reads ([`exec_direct`]).
    specs: Mutex<Specs>,
    /// The pool of `hex __oci-seed` processes that run execs into `oci` boxes, started on the first
    /// such exec from the same `hex` binary the class re-execs for its init ([`Node::seed_pool`]).
    seeds: OnceLock<SeedPool>,
}

/// Where an `oci` box's bundle comes from.
enum Source {
    /// The caller's ([`ClassSpec::Bundle`]), run as it is.
    Given(PathBuf),
    /// Rendered from the box's [`OciSpec`], to be written in this directory of the box's own.
    Rendered(PathBuf, Bundle),
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
        // Readable by the node alone: a bundle's config.json holds its init's environment.
        let bundles = state_dir.join("bundles");
        fs::create_dir_all(&bundles)
            .and_then(|()| fs::set_permissions(&bundles, fs::Permissions::from_mode(0o700)))
            .map_err(|e| Refusal::os("making the bundles directory", e))?;
        let owner = match (
            rustix::process::geteuid().as_raw(),
            rustix::process::getegid().as_raw(),
        ) {
            (0, _) => Owner::Root,
            (uid, gid) => Owner::Rootless { uid, gid },
        };
        let supervisor = Supervisor::start(root.dir(), store.clone())?;
        Ok(Self {
            root,
            store,
            oci_root: state_dir.join("oci"),
            oci_helper: None,
            supervisor,
            abandoned: Mutex::new(BTreeMap::new()),
            bundles,
            owner,
            masked: thermal_throttle()?,
            seeds: OnceLock::new(),
            specs: Mutex::new(Specs::default()),
            null: rustix::fs::open(
                "/dev/null",
                rustix::fs::OFlags::RDWR | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )
            .map_err(|e| Refusal::os("opening /dev/null", e))?,
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
            // The kill reaches every process in the box; now reap the exec'd programs the agent watches
            // for it, so an `oci` box's init — which the kernel holds in its pid-namespace teardown until
            // the programs in that namespace are reaped — can finish exiting and the box empty.
            self.supervisor.reap_programs(id, EMPTY_PATIENCE)?;
            if !cg.wait_empty(EMPTY_PATIENCE)? {
                return Err(Refusal::Os {
                    op: "emptying a box",
                    why: format!(
                        "box {id} still holds processes {EMPTY_PATIENCE:?} after cgroup.kill:{}",
                        survivors(&cg)
                    ),
                });
            }
            cg.remove()?;
        }
        if let Some(Ok(r)) = self.store.get(id)? {
            for dir in &r.dirs {
                remove_tree(dir, REMOVE_PATIENCE)?;
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
    /// the container's init started in it from its bundle, blocked at the exec FIFO until
    /// [`Runtime::start`]. The container's state dir is the box's, removed with it by [`Node::scrub`],
    /// and so is a rendered bundle's directory, its writable layer with it.
    fn create_oci(&self, spec: BoxSpec, source: Source) -> Result<BoxInfo, Refusal> {
        // A place for the init's pidfd first: past the budget nothing is made, so no init is ever
        // left unwatched.
        let slot = self.supervisor.reserve()?;
        // From the shim's spawn until the init is watched, no sweep runs: the shim is reaped by
        // create and the init by the supervisor, never by a sweep.
        let _spawning = self.supervisor.spawning()?;
        let id = spec.id.clone();
        // The container's own state directory, which the oci class makes; recorded as the box's, so
        // delete and recovery remove it with the box, as they remove a rendered bundle.
        let mut dirs = vec![self.oci_root.join(id.as_str())];
        if let Source::Rendered(dir, _) = &source {
            dirs.push(dir.clone());
        }
        let mut record = BoxRecord::new(spec.clone(), Phase::Creating, dirs);
        self.store.put(&record)?;
        let bundle = match source {
            Source::Given(bundle) => bundle,
            Source::Rendered(dir, rendered) => match write_bundle(&dir, &rendered) {
                Ok(()) => dir,
                Err(e) => {
                    let _ = self.scrub(&id);
                    return Err(e);
                }
            },
        };
        let cgroup = match self.root.create(&id, &spec.bounds) {
            Ok(cgroup) => cgroup,
            Err(e) => {
                // No cgroup, so scrub removes the directories and the record alone.
                let _ = self.scrub(&id);
                return Err(e);
            }
        };
        if let Err(e) = self
            .oci()
            .create_in_cgroup(id.as_str(), &bundle, None, cgroup)
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
                let container = supervise::Container {
                    bundle: state.bundle,
                    cgroup: state.cgroup,
                };
                self.supervisor
                    .watch(slot, &id, state.pid, start_ticks, container)
            });
        if let Err(e) = watched {
            let _ = self.scrub(&id);
            return Err(e);
        }
        self.info(&record, false)
    }

    /// A box's [`OciSpec`] as a bundle in a directory of its own, rendered ([`bundle::render`]) and
    /// checked against this host before anything is made: its root and each mount's source are there,
    /// and a layered root's writable layer can go where the node keeps bundles. What can't be is
    /// refused by name (CLAUDE.md §3).
    fn render(&self, id: &BoxId, spec: &OciSpec) -> Result<Source, Refusal> {
        let dir = self.bundles.join(id.as_str());
        let rendered = bundle::render(
            spec,
            &Place {
                dir: &dir,
                owner: self.owner,
                masked: &self.masked,
            },
        )?;
        match &spec.rootfs {
            Rootfs::Dir(root) => a_dir("rootfs.dir", root)?,
            Rootfs::Layers(layers) => {
                for (i, layer) in layers.iter().enumerate() {
                    a_dir(&format!("rootfs.layers[{i}]"), layer)?;
                }
                let fs = rustix::fs::statfs(&self.bundles)
                    .map_err(|e| Refusal::os("reading the bundles directory's filesystem", e))?;
                if i128::from(fs.f_type) == OVERLAYFS_SUPER_MAGIC {
                    return Err(Refusal::Platform {
                        what: "an oci box from layers",
                        platform: "a node whose state is on overlayfs",
                        why: format!(
                            "{} is on overlayfs, which can't hold an overlay's writable layer",
                            self.bundles.display()
                        ),
                    });
                }
            }
        }
        for (i, m) in spec.mounts.iter().enumerate() {
            // Followed, as mount(2) follows a bind's source.
            if let Err(e) = fs::metadata(&m.source) {
                return Err(Refusal::Invalid {
                    what: "oci spec",
                    why: format!("mounts[{i}].source {}: {e}", m.source.display()),
                });
            }
        }
        Ok(Source::Rendered(dir, rendered))
    }

    /// `exec` in an `oci` box whose init runs (`live`, from the supervisor that watches it), straight
    /// from this thread or through a seed (decision 3, goal 3 plan step 2): the program is cloned into
    /// the container's cgroup and namespaces as the agent's own child and returned with its pidfd; no
    /// other process is spawned and no `hex` binary is re-exec'd per call. The supervisor owns the
    /// program. The spawning lock is held across the start and the own, so a sweep doesn't reap the
    /// program (which may exit at once, as `true` does) before it is owned.
    fn exec_oci(
        &self,
        id: &BoxId,
        live: Live,
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
        let request = hex_core::seed::Request {
            init_pid: live.pid,
            bundle: live.container.bundle.clone(),
            cgroup: live.container.cgroup.clone(),
            args: program.args,
            cwd: program.cwd,
            env,
            user,
        };
        // Each stream the caller gave, else the null device the node holds open, so an exec opens and
        // duplicates nothing for its stdio. The caller's pipes are closed here once the program has its
        // own copies, so its streams end when it does.
        let fds = [
            stdio.stdin.as_ref().map_or(self.null.as_fd(), AsFd::as_fd),
            stdio.stdout.as_ref().map_or(self.null.as_fd(), AsFd::as_fd),
            stdio.stderr.as_ref().map_or(self.null.as_fd(), AsFd::as_fd),
        ];
        let cgroup = open_cgroup(live.container.cgroup.as_deref())?;
        let _spawning = self.supervisor.spawning()?;
        // Straight from this thread where it may choose the container's pid namespace (a node that runs
        // as root), as posix_spawn starts a program; through a seed otherwise (a rootless node), whose
        // intermediate joins the container's user namespace first.
        let direct = {
            let mut specs = self.specs.lock().map_err(|_| {
                Refusal::Broken("the node's parsed configs' lock was poisoned".to_owned())
            })?;
            exec_direct(
                &request,
                fds,
                live.pidfd,
                cgroup.as_ref().map(AsFd::as_fd),
                &mut specs,
            )?
        };
        let (pid, pidfd) = match direct {
            Direct::Started(pid, pidfd) => (pid, pidfd),
            Direct::Unable(init) => self.seed_pool()?.exec(&request, fds, init)?,
        };
        drop((stdio, cgroup));
        // The agent keeps a duplicate pidfd so it can reap the program on this box's delete: the
        // container's init blocks in the kernel until the programs in its pid namespace are reaped, so a
        // delete that didn't reap them would never empty the box. The caller's Proc keeps the other end
        // and reaps the program itself otherwise.
        let agent_pidfd = pidfd
            .try_clone()
            .map_err(|e| Refusal::os("duplicating the program's pidfd", e))?;
        self.supervisor
            .own_program(id, Box::new(PidfdProc::of_clone(pid, pidfd)), agent_pidfd)
    }

    /// The exec seed pool, started on first use from the `hex` binary the class re-execs (so the bench
    /// and a linked-in node use the binary [`Node::with_oci_helper`] names). Built once; a race to
    /// build it drops the losing empty pool, which has started no seed.
    fn seed_pool(&self) -> Result<&SeedPool, Refusal> {
        if let Some(pool) = self.seeds.get() {
            return Ok(pool);
        }
        let exe = match &self.oci_helper {
            Some(exe) => exe.clone(),
            None => {
                std::env::current_exe().map_err(|e| Refusal::os("finding the hex binary", e))?
            }
        };
        let _ = self.seeds.set(SeedPool::new(exe));
        self.seeds
            .get()
            .ok_or_else(|| Refusal::Broken("the exec seed pool wasn't built".to_owned()))
    }
}

impl Runtime for Node {
    fn classes(&self) -> Vec<Class> {
        vec![Class::Process, Class::Oci]
    }

    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        spec.bounds.check()?;
        // A structured spec is checked before anything else, as the simulator checks it: what can't
        // be rendered is refused by name whether or not the id is taken (HEX-RT-13).
        if let ClassSpec::Oci(oci) = &spec.class {
            oci.check()?;
        }
        if self.record(&spec.id)?.is_some() || self.root.get(&spec.id).is_some() {
            return Err(Refusal::Exists(spec.id));
        }
        let source = match &spec.class {
            ClassSpec::Process => return self.create_process(spec),
            ClassSpec::Bundle(bundle) => Source::Given(bundle.clone()),
            ClassSpec::Oci(oci) => self.render(&spec.id, oci)?,
        };
        self.create_oci(spec, source)
    }

    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        // An oci box's start is tried first, so the hot path reads only the container's state: a box with
        // no container under the oci root is a process box or no box, which the record then says (goal 3
        // plan step 3: start costs one FIFO operation and the state read beside it). The init's liveness is
        // judged by the pidfd the supervisor holds for it, a poll.
        let started = self.oci().start_with(id.as_str(), &|_| {
            matches!(self.supervisor.runs(id), Ok(true))
        });
        match started {
            Ok(()) => Ok(()),
            // `running` has started already (Invalid); `stopped` or mid-create has no live init to start
            // (NotRunning). Mapped here so the node and the simulator agree (HEX-RT-10), rather than
            // passing WrongState on (a generic Os refusal).
            Err(OciError::WrongState(_, "running", _)) => Err(Refusal::Invalid {
                what: "start",
                why: format!("container {id} has started already"),
            }),
            Err(OciError::WrongState(..)) => Err(Refusal::NotRunning(id.clone(), "start")),
            Err(OciError::NoSuchContainer(_)) => match self.record(id)?.map(|r| r.spec.class) {
                None => Err(Refusal::NoSuchBox(id.clone())),
                // A process box has no init to start: exec runs its programs. Refused as the
                // simulator refuses it (HEX-RT-10).
                Some(ClassSpec::Process) => Err(Refusal::Invalid {
                    what: "start",
                    why: format!("box {id} is a process box, which has no init to start"),
                }),
                // An oci box whose container isn't there: mid-create, or its create failed.
                Some(ClassSpec::Oci(_) | ClassSpec::Bundle(_)) => {
                    Err(Refusal::NotRunning(id.clone(), "start"))
                }
            },
            Err(e) => Err(e.into()),
        }
    }

    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        // An oci box whose init runs is known to the supervisor, with what its exec needs, so the hot
        // path reads nothing from disk; any other box's record says what it is.
        match self.supervisor.live(id)? {
            Some(live) => self.exec_oci(id, live, program, stdio),
            None => match self.record(id)?.map(|r| r.spec.class) {
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
                // An oci box with no running init: mid-create, stopped, or its create failed.
                Some(ClassSpec::Oci(_) | ClassSpec::Bundle(_)) => {
                    Err(Refusal::NotRunning(id.clone(), "exec"))
                }
            },
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

/// What a box still holds when a delete can't empty it: each surviving process with its command,
/// state and parent, so the refusal names what is stuck, not only that something is (§3). Bounded to a
/// few: enough to see what it is. An empty `cgroup.procs` while the box reads populated means a
/// descendant cgroup holds them.
fn survivors(cg: &hex_runtime::linux::cgroup::BoxCgroup) -> String {
    let root = cg.dir().to_path_buf();
    let mut out = String::new();
    let mut queue = vec![root.clone()];
    let mut dirs_seen = 0usize;
    // A flat walk of the box's cgroup subtree (CLAUDE.md §11), bounded in cgroups and pids (§2).
    while let Some(dir) = queue.pop() {
        if dirs_seen >= 256 {
            break;
        }
        dirs_seen = dirs_seen.saturating_add(1);
        let rel = dir
            .strip_prefix(&root)
            .ok()
            .map(|p| p.display().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| ".".to_owned());
        if let Ok(text) = fs::read_to_string(dir.join("cgroup.procs")) {
            for pid in text
                .lines()
                .filter_map(|l| l.trim().parse::<u32>().ok())
                .take(16)
            {
                let comm = fs::read_to_string(format!("/proc/{pid}/comm"))
                    .map(|s| s.trim().to_owned())
                    .unwrap_or_default();
                let (state, ppid) = fs::read_to_string(format!("/proc/{pid}/stat"))
                    .ok()
                    .and_then(|stat| {
                        let (_, rest) = stat.rsplit_once(')')?;
                        let mut f = rest.split_whitespace();
                        Some((f.next()?.to_owned(), f.next()?.to_owned()))
                    })
                    .unwrap_or_default();
                out.push_str(&format!(
                    " [{rel}/{pid} {comm:?} state {state} ppid {ppid}]"
                ));
            }
        }
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten().take(256) {
                let path = entry.path();
                if path.is_dir() {
                    queue.push(path);
                }
            }
        }
    }
    if out.is_empty() {
        // Populated with nothing in any cgroup.procs: a process the kernel still holds, typically a
        // container init stuck in its pid-namespace teardown until a process in that namespace is reaped.
        " (populated but no process listed: a pid namespace is tearing down)".to_owned()
    } else {
        out
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

/// Refuses, by the spec's field, a path that isn't a directory on this host.
fn a_dir(field: &str, path: &Path) -> Result<(), Refusal> {
    let why = match fs::metadata(path) {
        Ok(m) if m.is_dir() => return Ok(()),
        Ok(_) => "isn't a directory".to_owned(),
        Err(e) => e.to_string(),
    };
    Err(Refusal::Invalid {
        what: "oci spec",
        why: format!("{field} {}: {why}", path.display()),
    })
}

/// Writes a rendered bundle into its own directory, made here and readable by the node alone, since
/// its config.json holds the init's environment. The container's `/etc/hosts` is readable by all, as a
/// hosts file is. A layered root's directories are mode 0755: the writable layer's mode is the one the
/// container sees on its `/`. Nothing is fsynced: after a host crash recovery removes the box with its
/// bundle (decision 1 of docs/design/runtime-architecture.md).
fn write_bundle(dir: &Path, rendered: &Bundle) -> Result<(), Refusal> {
    let mode = |path: &Path, mode: u32| fs::set_permissions(path, fs::Permissions::from_mode(mode));
    fs::create_dir(dir)
        .and_then(|()| mode(dir, 0o700))
        .map_err(|e| Refusal::os("making a box's bundle directory", e))?;
    let config = dir.join(bundle::CONFIG);
    let hosts = dir.join(bundle::HOSTS);
    fs::write(&config, &rendered.config)
        .and_then(|()| mode(&config, 0o600))
        .and_then(|()| fs::write(&hosts, &rendered.hosts))
        .and_then(|()| mode(&hosts, 0o644))
        .map_err(|e| Refusal::os("writing a box's bundle", e))?;
    for d in &rendered.dirs {
        fs::create_dir(d)
            .and_then(|()| mode(d, 0o755))
            .map_err(|e| Refusal::os("making a box's writable layer", e))?;
    }
    Ok(())
}

/// The host's CPUs' `thermal_throttle` directories, which every container has masked, as Docker's and
/// containerd's do (GHSA-6fw5-f8r9-fgfm): `/sys/devices/system/cpu/cpu<N>/thermal_throttle` for each
/// CPU that has one, in CPU order. A host without them has none to mask.
fn thermal_throttle() -> Result<Vec<String>, Refusal> {
    let cpus = Path::new("/sys/devices/system/cpu");
    let entries = match fs::read_dir(cpus) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Refusal::os("listing the host's CPUs", e)),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| Refusal::os("listing the host's CPUs", e))?;
        let Some(cpu) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.strip_prefix("cpu"))
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let path = format!("/sys/devices/system/cpu/cpu{cpu}/thermal_throttle");
        if Path::new(&path).is_dir() {
            found.push((cpu, path));
        }
    }
    found.sort_unstable();
    Ok(found.into_iter().map(|(_, path)| path).collect())
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

#[cfg(test)]
mod rendered {
    use super::*;
    use hex_api::{Mount, Network, User};
    use hex_runtime::linux::oci::spec::{MAX_CONFIG_BYTES, Spec};

    /// Doc-truth (CLAUDE.md §6): the renderer's bound on config.json is the runtime's.
    #[test]
    fn the_renderers_bound_is_the_runtimes() {
        assert_eq!(bundle::MAX_CONFIG_BYTES, MAX_CONFIG_BYTES);
    }

    /// Doc-truth (AC-T7.1.1): the `oci` class reads every shape the renderer writes, through its own
    /// model and check, with each field where the renderer put it. The runtime ignores a field it
    /// doesn't know, so a field the renderer misnamed would be dropped without a word; this is
    /// where that shows.
    #[test]
    fn the_runtime_reads_every_rendered_shape_as_rendered() {
        let dir = Path::new("/var/lib/hex/bundles/b");
        let rootless = Owner::Rootless {
            uid: 1000,
            gid: 100,
        };
        for (owner, network, layered) in [
            (Owner::Root, Network::Private, true),
            (Owner::Root, Network::Host, false),
            (rootless, Network::Private, true),
            (rootless, Network::Host, false),
        ] {
            let mut init = Program::of(&["/bin/sh", "-c", "exec sleep infinity"]);
            init.cwd = Some("/work".to_owned());
            if owner == Owner::Root {
                init.user = Some(User {
                    uid: 7,
                    gid: 8,
                    groups: vec![9],
                });
            }
            let spec = OciSpec {
                rootfs: if layered {
                    Rootfs::Layers(vec!["/images/base".into(), "/images/app".into()])
                } else {
                    Rootfs::Dir("/srv/rootfs".into())
                },
                init: init.clone(),
                mounts: vec![Mount {
                    source: "/srv/data".into(),
                    destination: "/data".to_owned(),
                    read_only: true,
                }],
                hostname: Some("box".to_owned()),
                network,
                hosts: vec![("db".to_owned(), "10.0.0.2".to_owned())],
            };
            let masked = ["/sys/devices/system/cpu/cpu0/thermal_throttle".to_owned()];
            let place = Place {
                dir,
                owner,
                masked: &masked,
            };
            let rendered = bundle::render(&spec, &place).unwrap();
            let read = Spec::parse(&rendered.config).unwrap();
            let shape = format!("{owner:?} {network:?} layered {layered}");

            assert_eq!(read.hostname.as_deref(), Some("box"), "{shape}");
            assert!(!read.root.readonly, "{shape}");
            let root = if layered {
                "/var/lib/hex/bundles/b/rootfs"
            } else {
                "/srv/rootfs"
            };
            assert_eq!(read.root.path, root, "{shape}");

            let process = read.process.clone().unwrap();
            assert_eq!(process.args, init.args, "{shape}");
            assert_eq!(
                process.env,
                ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_owned()],
                "{shape}"
            );
            assert_eq!(process.cwd, "/work", "{shape}");
            let user = init.user.unwrap_or_default();
            assert_eq!(
                (
                    process.user.uid,
                    process.user.gid,
                    process.user.additional_gids
                ),
                (user.uid, user.gid, user.groups),
                "{shape}"
            );
            let caps = process.capabilities.unwrap();
            for set in [&caps.bounding, &caps.effective, &caps.permitted] {
                assert_eq!(set, &bundle::CAPABILITIES, "{shape}");
            }
            assert!(
                caps.inheritable.is_empty() && caps.ambient.is_empty(),
                "{shape}"
            );
            assert!(!process.no_new_privileges && !process.terminal, "{shape}");

            for kind in ["pid", "ipc", "uts", "mount", "cgroup"] {
                assert!(read.makes_namespace(kind), "{shape}: {kind}");
            }
            assert_eq!(
                read.makes_namespace("network"),
                network == Network::Private,
                "{shape}"
            );
            let maps = |m: &[hex_runtime::linux::oci::spec::IdMapping]| {
                m.iter()
                    .map(|m| (m.container_id, m.host_id, m.size))
                    .collect::<Vec<_>>()
            };
            match owner {
                Owner::Root => {
                    assert!(!read.makes_namespace("user"), "{shape}");
                    assert!(read.linux.uid_mappings.is_empty(), "{shape}");
                }
                Owner::Rootless { uid, gid } => {
                    assert!(read.makes_namespace("user"), "{shape}");
                    assert_eq!(maps(&read.linux.uid_mappings), [(0, uid, 1)], "{shape}");
                    assert_eq!(maps(&read.linux.gid_mappings), [(0, gid, 1)], "{shape}");
                }
            }
            let devices = &read.linux.resources.as_ref().unwrap().devices;
            assert!(
                devices.len() == 1 && !devices[0].allow && devices[0].kind.is_none(),
                "{shape}: deny all"
            );
            assert_eq!(devices[0].access.as_deref(), Some("rwm"), "{shape}");
            let mut want_masked: Vec<String> = bundle::MASKED_PATHS.map(str::to_owned).to_vec();
            want_masked.extend(masked.iter().cloned());
            assert_eq!(read.linux.masked_paths, want_masked, "{shape}");
            assert_eq!(read.linux.readonly_paths, bundle::READONLY_PATHS, "{shape}");

            // The mounts, in order: the overlay first for a layered root, every default, the hosts
            // file, then the spec's.
            let mounts: Vec<(String, String, String, Vec<String>)> = read
                .mounts
                .iter()
                .map(|m| {
                    (
                        m.destination.clone(),
                        m.kind.clone().unwrap_or_default(),
                        m.source.clone().unwrap_or_default(),
                        m.options.clone(),
                    )
                })
                .collect();
            let first = mounts.first().unwrap();
            if layered {
                assert_eq!((first.0.as_str(), first.1.as_str()), ("/", "overlay"));
                assert_eq!(
                    first.3[0], "lowerdir=/images/app:/images/base",
                    "{shape}: top layer first"
                );
                assert_eq!(
                    first.3.contains(&"userxattr".to_owned()),
                    owner != Owner::Root,
                    "{shape}"
                );
            } else {
                assert_eq!(first.0, "/proc", "{shape}");
            }
            let hosts = mounts.iter().position(|m| m.0 == "/etc/hosts").unwrap();
            assert_eq!(mounts[hosts].2, "/var/lib/hex/bundles/b/hosts", "{shape}");
            assert_eq!(
                mounts.last().unwrap(),
                &(
                    "/data".to_owned(),
                    "bind".to_owned(),
                    "/srv/data".to_owned(),
                    vec!["bind".to_owned(), "ro".to_owned()]
                ),
                "{shape}"
            );
            let sys = mounts.iter().find(|m| m.0 == "/sys").unwrap();
            let host_sys = owner != Owner::Root && network == Network::Host;
            assert_eq!(sys.1, if host_sys { "bind" } else { "sysfs" }, "{shape}");
            let cgroup = mounts
                .iter()
                .rposition(|m| m.0 == "/sys/fs/cgroup")
                .unwrap();
            assert_eq!(
                mounts[cgroup].1, "cgroup2",
                "{shape}: the box's own, on top"
            );
        }
    }
}
