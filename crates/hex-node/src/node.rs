//! The Linux node agent: [`hex_api::Runtime`] over the `process` and `oci` classes, with its boxes'
//! records kept durably and recovered after a crash. An `oci` box is an OCI container the node runs
//! in a cgroup under its own box root with the box's bounds in force, as the `process` class does, so
//! one code path makes, bounds, deletes and recovers every box's cgroup; the container's init and
//! `exec` programs run through the `hex` binary's `__oci-init` and `__oci-exec` helpers (decision 3
//! of docs/design/runtime-architecture.md). A box made from the structured [`hex_api::OciSpec`] runs
//! from a bundle the node renders into a directory of the box's own ([`hex_core::bundle`]), holding
//! its writable layer, which goes when the box does. The node agent is its containers' parent,
//! holding each init's pidfd and recording how it ended ([`crate::supervise`], decision 2). Each
//! container keeps its state on tmpfs, in the runtime directory, as runc's and crun's does: a host
//! crash takes it with the container, so a box's record that names nothing else is kept beside it and
//! never fsynced (decision 1; [`crate::store`]). A box's record is written once, when the box is
//! whole; until then each part of it lies at a path its id fixes, where recovery finds what a crash
//! left (goal 5 extended, step 5).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::fd::AsFd;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use hex_api::{
    BoxId, BoxInfo, BoxSpec, BoxState, Class, ClassSpec, OciSpec, Proc, Program, Refusal, Rootfs,
    Runtime, Signal, Stdio, Usage,
};
use hex_core::bundle::{self, Bundle, Owner, Place};
use hex_core::cgroup::Dev;
use hex_core::netns;
use hex_core::record::{BoxRecord, Phase, ProcId};
use hex_core::recovery::{self, Decision, Parts, Seen, Why};
use hex_runtime::linux::cgroup::{BoxCgroup, BoxRoot};
use hex_runtime::linux::oci::OciError;
use hex_runtime::linux::oci::devshare::MAX_PROGRAMS;
use hex_runtime::linux::oci::programs::{self, Programs};
use hex_runtime::linux::oci::run::{Held, Runtime as OciRuntime};
use hex_runtime::linux::oci::seed::{Direct, Specs, exec_direct, open_cgroup};
use hex_runtime::linux::oci::state::{State, Status, start_time};
use hex_runtime::linux::proc::PidfdProc;
use hex_runtime::linux::{disks, process};

use crate::createpool::CreatePool;
use crate::netns::Netns;
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

/// How long [`Node::recover`] spends listing a directory for the parts of boxes before it refuses: the
/// runtime directory is shared with every node of this user on the host, so its entries are theirs as
/// well as this node's. A bound on a restart's wait, as [`REMOVE_PATIENCE`] is on a delete's, not a
/// guess at a directory's size (CLAUDE.md §2, §4).
pub const LIST_PATIENCE: Duration = Duration::from_secs(60);

/// A point of an `oci` box's create where a debug build's node, once armed ([`Node::with_crash_point`]),
/// ends its own process as a kill there would: T-T7.1.2's crash test kills the real agent at the points
/// of a create that no file it opens can hold it at (CLAUDE.md §6; goal 5 extended, step 5). Release
/// builds have none, and an unarmed point does nothing.
#[cfg(debug_assertions)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashPoint {
    /// The box's cgroup made and bounded, nothing else of it yet.
    CgroupMade,
    /// Its bundle rendered after its cgroup, its container's state directory not yet made.
    BundleRendered,
    /// In the record write that marks the box made, between opening a non-durable record's file and
    /// writing it: the box made, its record torn ([`crate::store::Store::crash_before_first_write`]).
    RecordFirstWrite,
}

#[cfg(debug_assertions)]
impl CrashPoint {
    /// The point `name` names, as `hex node --crash-at` takes it.
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "cgroup-made" => Some(Self::CgroupMade),
            "bundle-rendered" => Some(Self::BundleRendered),
            "record-first-write" => Some(Self::RecordFirstWrite),
            _ => None,
        }
    }
}

/// The cgroup under the box root that the `oci` boxes alone live under, so a root node's default device
/// program can sit on it without reaching a `process` box (goal 5 extended, step 1; research f44). No
/// box id starts with `.`, so it can't be a box's.
pub const OCI_BOXES: &str = ".oci";

/// A node agent over the `process` and `oci` classes.
pub struct Node {
    /// The `process` boxes' cgroups, and the `oci` boxes' root among them ([`OCI_BOXES`]).
    root: BoxRoot,
    /// The `oci` boxes' cgroups: under the box root, but apart from the `process` boxes. An `oci` box
    /// made before its node kept them apart is still found under the box root ([`Node::cgroup`]).
    oci_boxes: BoxRoot,
    /// The `oci` boxes' device programs: one held open per policy, and on a root node the default
    /// policy's on [`Node::oci_boxes`]'s root.
    devices: Programs,
    store: Store,
    /// The state root the `oci` class keeps each container's `state.json` and exec FIFO under
    /// ([`oci_root`]), shared with the other nodes on the host that run as this one's user.
    oci_root: PathBuf,
    /// The node's own key among them, which names its containers ([`Node::container`]).
    key: String,
    /// The `hex` binary the `oci` class re-execs for a container's init and `exec` helper; `None`
    /// when this process's own binary is `hex` (it handles `__oci-init`/`__oci-exec`). A node linked
    /// into another program, or the bench, names it ([`Node::with_oci_helper`]).
    oci_helper: Option<PathBuf>,
    /// The `oci` boxes' inits, watched and reaped; orphaned box processes, reaped.
    supervisor: Arc<Supervisor>,
    /// Abandoned programs ([`Node::abandoned`]) counted but not yet in their box's record, by box:
    /// kept and reported until a record write takes them, never dropped (§3). Each box's count has a
    /// lock of its own, held across the write that takes it, so one box's durable write holds up no
    /// other box's inspect or create (goal 5 plan step 2). An entry goes with its box's delete.
    abandoned: Mutex<Pending>,
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
    /// The pool of `hex __oci-seed` processes that make `oci` boxes: a create hands one the container's
    /// spec and it clones the init in place of the shim's re-exec (decision 5; [`Node::create_pool`]).
    create_seeds: OnceLock<CreatePool>,
    /// The network namespaces the node makes ahead for its containers, and what it counts of every
    /// container's (decision 4; [`Node::netns`]).
    netns: Netns,
    /// The debug-only crash point the caller armed ([`Node::with_crash_point`]), `None` unless one did.
    #[cfg(debug_assertions)]
    crash_at: Option<CrashPoint>,
}

/// Each box's abandoned programs not yet in its record ([`Node::abandoned`]).
type Pending = BTreeMap<BoxId, Arc<Mutex<u64>>>;

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

    /// The node over `root`, records under `state_dir` and its runtime directory ([`crate::store`]);
    /// this process becomes a child subreaper, so each `oci` box's init is its child once the shim
    /// exits.
    fn with_root(root: BoxRoot, state_dir: &Path) -> Result<Self, Refusal> {
        let owner = match (
            rustix::process::geteuid().as_raw(),
            rustix::process::getegid().as_raw(),
        ) {
            (0, _) => Owner::Root,
            (uid, gid) => Owner::Rootless { uid, gid },
        };
        let oci_root = oci_root(owner, state_dir)?;
        let key = node_key(state_dir)?;
        let store = Store::open(state_dir, &records_dir(&oci_root, state_dir)?)?;
        // Readable by the node alone: a bundle's config.json holds its init's environment.
        let bundles = state_dir.join("bundles");
        fs::create_dir_all(&bundles)
            .and_then(|()| fs::set_permissions(&bundles, fs::Permissions::from_mode(0o700)))
            .map_err(|e| Refusal::os("making the bundles directory", e))?;
        let supervisor = Supervisor::start(root.dir(), store.clone())?;
        // A namespace made ahead belongs to the agent's user namespace, so only a node whose
        // containers are in it, a root node's, makes them; a rootless node's containers each have a
        // user namespace of their own (decision 4).
        let netns = Netns::start(match owner {
            Owner::Root => netns::BOUND,
            Owner::Rootless { .. } => 0,
        })?;
        let oci_boxes = BoxRoot::open(&root.dir().join(OCI_BOXES), root.disks().to_vec())?;
        let devices = device_programs(owner, &oci_boxes, MAX_PROGRAMS)?;
        Ok(Self {
            root,
            oci_boxes,
            devices,
            store,
            oci_root,
            key,
            oci_helper: None,
            supervisor,
            abandoned: Mutex::new(BTreeMap::new()),
            bundles,
            owner,
            masked: thermal_throttle()?,
            seeds: OnceLock::new(),
            create_seeds: OnceLock::new(),
            specs: Mutex::new(Specs::default()),
            netns,
            null: rustix::fs::open(
                "/dev/null",
                rustix::fs::OFlags::RDWR | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )
            .map_err(|e| Refusal::os("opening /dev/null", e))?,
            #[cfg(debug_assertions)]
            crash_at: None,
        })
    }

    /// Arms a debug-only crash point (T-T7.1.2): at `point` of each `oci` box's create from now on, the
    /// node sends its own process SIGKILL, so a test kills the real agent where nothing it opens can hold
    /// it. Only the node's caller arms it (`hex node --crash-at`, from the agent's own arguments): no
    /// variable or file a container can set is read. Debug builds only.
    #[cfg(debug_assertions)]
    pub fn with_crash_point(mut self, point: CrashPoint) -> Self {
        self.crash_at = Some(point);
        // The record write's crash point lies inside the store, so it is armed there; the rest the
        // node reaches itself ([`Node::crash_point`]).
        #[cfg(target_os = "linux")]
        if point == CrashPoint::RecordFirstWrite {
            self.store.crash_before_first_write();
        }
        self
    }

    /// Ends this process here as a kill would, when the caller armed `point`.
    #[cfg(debug_assertions)]
    fn crash_point(&self, point: CrashPoint) {
        if self.crash_at == Some(point) {
            let _ = rustix::process::kill_process(
                rustix::process::getpid(),
                rustix::process::Signal::KILL,
            );
        }
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
        // The create seeds are built now, from this binary, so their first spare is up before the first
        // create (decision 5): no create waits on a seed's exec. A pool that can't be built now is built
        // at the first create, which reports why.
        if let Ok(pool) = CreatePool::new(hex_binary.clone()) {
            let _ = self.create_seeds.set(pool);
        }
        self.oci_helper = Some(hex_binary);
        self
    }

    /// Sets the most network namespaces a root node makes ahead, [`netns::BOUND`] unless set, in place
    /// of the pool it opened with, which closes what it held. With 0 it makes none, and every container
    /// that makes a network namespace makes it on its create's path, counted. A rootless node makes
    /// none (its containers' namespaces belong to user namespaces of their own), so a bound above 0
    /// there is refused.
    pub fn with_netns_bound(mut self, bound: usize) -> Result<Self, Refusal> {
        if bound > 0 && self.owner != Owner::Root {
            return Err(Refusal::Invalid {
                what: "network-namespace bound",
                why: format!(
                    "a rootless node makes no network namespaces ahead, so it can't hold {bound}"
                ),
            });
        }
        self.netns = Netns::start(bound)?;
        Ok(self)
    }

    /// Sets the most device programs the node holds open, one per policy, [`MAX_PROGRAMS`] unless set,
    /// in place of those it opened with, which it closes (each lives on in the cgroups it's attached to).
    /// A root node's oci boxes' root keeps its program.
    pub fn with_device_program_bound(mut self, bound: usize) -> Result<Self, Refusal> {
        self.devices = device_programs(self.owner, &self.oci_boxes, bound)?;
        Ok(self)
    }

    /// What the node's device programs have done since it opened them: the loads, attaches and closes
    /// tests and the bench count, and whether its start attached the default policy's program to the oci
    /// boxes' root.
    pub fn device_programs(&self) -> Result<programs::Counts, Refusal> {
        self.devices.counts().map_err(Refusal::from)
    }

    /// The root the node's `oci` boxes alone live under ([`OCI_BOXES`]).
    pub fn oci_boxes(&self) -> &Path {
        self.oci_boxes.dir()
    }

    /// Box `id`'s cgroup: an `oci` box's under the oci boxes' root, else (a `process` box, or an `oci`
    /// box made before its node kept them apart) under the box root.
    fn cgroup(&self, id: &BoxId) -> Option<BoxCgroup> {
        self.oci_boxes.get(id).or_else(|| self.root.get(id))
    }

    /// The `oci` runtime, over the node's state root and helper binary.
    fn oci(&self) -> OciRuntime {
        self.oci_in(self.oci_root.clone())
    }

    /// The `oci` runtime over the state root `root`, with the node's helper binary.
    fn oci_in(&self, root: PathBuf) -> OciRuntime {
        match &self.oci_helper {
            Some(exe) => OciRuntime::with_helper(root, exe.clone()),
            None => OciRuntime::new(root),
        }
    }

    /// Where `record`'s box keeps its container's state, as the record names it: the runtime over the
    /// directory's parent, and the container's name there. Create names the container's directory
    /// first, so a box made before its node kept container state under the runtime directory (defda14),
    /// whose record names `<state>/oci/<id>`, is found there and managed as any other (T-T7.1.2 across
    /// the upgrade). `None` for a record that names no directory: a `process` box's.
    fn container_of(&self, record: &BoxRecord) -> Option<(OciRuntime, String)> {
        let dir = record.dirs.first()?;
        let name = dir.file_name()?.to_str()?.to_owned();
        Some((self.oci_in(dir.parent()?.to_path_buf()), name))
    }

    /// Box `id`'s container's name under the node's oci root ([`container_name`]).
    fn container(&self, id: &BoxId) -> String {
        container_name(&self.key, id)
    }

    /// Where box `id`'s container keeps its state, its `state.json` and exec FIFO: on tmpfs, under the
    /// runtime directory ([`oci_root`]).
    pub fn container_dir(&self, id: &BoxId) -> PathBuf {
        self.oci_root.join(self.container(id))
    }

    /// What the node's record store has fsynced since it opened ([`crate::store::Syncs`]).
    pub fn record_syncs(&self) -> crate::store::Syncs {
        self.store.syncs()
    }

    /// The disks the node holds boxes' files on, as `io.latency` and `io.max` name them.
    pub fn disks(&self) -> &[Dev] {
        self.root.disks()
    }

    /// What the node has counted of its containers' network namespaces since its pool opened
    /// (decision 4): made ahead, taken by a create, made on a create's path (the pool dry, or the
    /// container in a user namespace of its own, as every rootless node's is), discarded at shutdown,
    /// and makes that failed. Counted at the take, so a create that fails after it is counted all the
    /// same. The bench reads these around each phase, in process here and over the socket
    /// ([`crate::serve::Host::netns`]).
    pub fn netns(&self) -> Result<netns::Counts, Refusal> {
        self.netns.counts()
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
                // Read where the record says the container's state is, whichever node made it.
                let status = self
                    .container_of(record)
                    .map(|(oci, name)| oci.state(&name).map(|s| s.status));
                match status {
                    Some(Ok(Status::Created)) => BoxState::Created,
                    Some(Ok(Status::Running)) => BoxState::Running,
                    Some(Ok(Status::Creating | Status::Stopped)) => BoxState::Empty,
                    // No container state yet (mid-create) or it's gone: nothing runs.
                    Some(Err(_)) | None => BoxState::Empty,
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
        let count = Arc::clone(self.pending()?.entry(id.clone()).or_default());
        let mut count = count.lock().map_err(|_| counts_poisoned())?;
        // Saturating: 2^64 programs is past any box's life, so the cap is never met.
        *count = count.saturating_add(1);
        let add = *count;
        self.store.update(id, |r| {
            r.abandoned = r.abandoned.saturating_add(add);
            true
        })?;
        *count = 0;
        Ok(())
    }

    fn pending(&self) -> Result<MutexGuard<'_, Pending>, Refusal> {
        self.abandoned.lock().map_err(|_| counts_poisoned())
    }

    /// Box `id`'s abandoned programs not yet in its record; waits only for a write of its own count.
    fn unrecorded(&self, id: &BoxId) -> Result<u64, Refusal> {
        let Some(count) = self.pending()?.get(id).map(Arc::clone) else {
            return Ok(0);
        };
        let count = count.lock().map_err(|_| counts_poisoned())?;
        Ok(*count)
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
    /// crash during it, because removing a box tolerates each part being gone already. A box caught
    /// mid-create has no record yet, so its parts are found where its id puts them: its cgroup under
    /// either root, its container's state directory under the runtime directory and its rendered
    /// bundle under the bundles directory (goal 5 extended, step 5).
    pub fn recover(&self) -> Result<RecoverySummary, Refusal> {
        // What a crash at a record write left: a temp file never renamed into place.
        self.store.remove_strays()?;
        // A node before goal 5 extended step 5 kept every record in its state directory.
        self.store.settle()?;
        let mut seen: BTreeMap<BoxId, Seen> = BTreeMap::new();
        for (id, record) in self.store.all()? {
            seen.entry(id).or_default().record = Some(record);
        }
        // Both roots' cgroups; the oci boxes' root itself is no box (its name is no id).
        for name in self
            .root
            .boxes()?
            .into_iter()
            .chain(self.oci_boxes.boxes()?)
        {
            if let Ok(id) = BoxId::new(&name) {
                seen.entry(id).or_default();
            }
        }
        // This node's containers, `<key>-<id>` among every node's under the runtime directory, and its
        // rendered bundles, `<id>` under its bundles directory.
        let state_dirs = ids_in(&self.oci_root, &format!("{}-", self.key))?;
        let bundles = ids_in(&self.bundles, "")?;
        for id in state_dirs.iter().chain(&bundles) {
            seen.entry(id.clone()).or_default();
        }
        for (id, s) in &mut seen {
            s.parts = Parts {
                cgroup: self.cgroup(id).is_some(),
                state_dir: state_dirs.contains(id),
                bundle: bundles.contains(id),
            };
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
        if let Some(cg) = self.cgroup(id) {
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
        // The directories the box owns: those its record names (a box made before its node kept
        // container state under the runtime directory names `<state>/oci/<id>`), or, with no record to
        // read, those at the paths its id fixes, which a box caught mid-create left.
        let dirs = match self.store.get(id)? {
            Some(Ok(r)) => r.dirs,
            None | Some(Err(_)) => vec![self.container_dir(id), self.bundles.join(id.as_str())],
        };
        for dir in &dirs {
            remove_tree(dir, REMOVE_PATIENCE)?;
        }
        self.store.remove(id)
    }

    /// A `process` box: its cgroup with the bounds in force, empty until [`Runtime::exec`]. Its record
    /// is written once the cgroup is: a crash before leaves a cgroup no record names, which recovery
    /// removes (T-T7.1.2).
    fn create_process(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        self.root.create(&spec.id, &spec.bounds)?;
        let record = BoxRecord::new(spec, Phase::Ready, Vec::new());
        if let Err(e) = self.store.put(&record) {
            let _ = self.scrub(&record.spec.id);
            return Err(e);
        }
        self.info(&record, false)
    }

    /// An `oci` box: its cgroup under the box root with the bounds in force (as a `process` box), and
    /// the container's init started in it from its bundle, blocked at the exec FIFO until
    /// [`Runtime::start`]. The container's state dir is the box's, removed with it by [`Node::scrub`],
    /// and so is a rendered bundle's directory, its writable layer with it. The box's record is
    /// written once, when the box is whole, with no lock held that another box's create takes, so a
    /// durable record's fsyncs hold up no other box (goal 5 plan step 2). Until then each part a
    /// crash leaves (the cgroup, the bundle, the container's state directory and the init in the
    /// cgroup) lies at a path the box's id fixes, where recovery finds it (goal 5 extended, step 5).
    /// The cgroup is made first and claims the id: a create of an id taken since the check fails at its
    /// mkdir, as [`Refusal::Exists`], having made nothing, and so removes nothing.
    fn create_oci(&self, spec: BoxSpec, source: Source) -> Result<BoxInfo, Refusal> {
        // A place for the init's pidfd first: past the budget nothing is made, so no init is ever
        // left unwatched.
        let slot = self.supervisor.reserve()?;
        // A create hands an idle seed the container's spec and the seed clones the init as the agent's
        // own child (decision 5), in place of the shim's re-exec of the hex binary. A pool that can't
        // be built refuses the create before anything is made.
        let seeds = self.create_pool()?;
        let id = spec.id.clone();
        // The container's own state directory, which the oci class makes; recorded as the box's, so
        // delete and recovery remove it with the box, as they remove a rendered bundle.
        let mut dirs = vec![self.container_dir(&id)];
        if let Source::Rendered(dir, _) = &source {
            dirs.push(dir.clone());
        }
        let cgroup = self.oci_boxes.create(&id, &spec.bounds)?;
        #[cfg(debug_assertions)]
        self.crash_point(CrashPoint::CgroupMade);
        let bundle = match source {
            Source::Given(bundle) => bundle,
            Source::Rendered(dir, rendered) => match write_bundle(&dir, &rendered) {
                Ok(()) => {
                    #[cfg(debug_assertions)]
                    self.crash_point(CrashPoint::BundleRendered);
                    dir
                }
                Err(e) => {
                    let _ = self.scrub(&id);
                    return Err(e);
                }
            },
        };
        let made = self.supervisor.spawning().and_then(|_spawning| {
            // A container that makes a network namespace of its own in the agent's user namespace
            // joins one the node made ahead, and the pool counts every container's (decision 4). Its
            // device program is one the node holds, or the oci boxes' root's (goal 5 extended, step 1).
            self.oci()
                .create_in_cgroup_seeded(
                    &self.container(&id),
                    &bundle,
                    None,
                    cgroup,
                    Held {
                        networks: Some(&self.netns),
                        devices: Some(&self.devices),
                    },
                    seeds,
                )
                .map_err(Refusal::from)
        });
        let state = match made {
            Ok(state) => state,
            Err(e) => {
                // The container, if it started, is in the cgroup; scrub ends it and removes the cgroup,
                // the state dir and the bundle.
                let _ = self.scrub(&id);
                return Err(e);
            }
        };
        // The init is blocked at the exec FIFO, alive, and the agent's child; the state is the one the
        // runtime wrote, so nothing is read back. Record which process the init is so a later restart
        // can tell it from a recycled pid (recovery's init_alive), and watch it through its pidfd from
        // now on. Recorded before it's watched, so the exit the supervisor records goes into a record
        // that names it.
        let mut record = BoxRecord::new(spec, Phase::Ready, dirs);
        record.init = init_proc_id(state.pid, state.start_time);
        let watched = self.store.put(&record).and_then(|()| {
            let start_ticks = state.start_time.ok_or_else(|| {
                Refusal::Broken(format!("container {id}'s state has no start time"))
            })?;
            let container = supervise::Container {
                bundle: state.bundle,
                cgroup: state.cgroup,
            };
            // Watched under the spawning lock, so no sweep reaps the init before it's owned; until
            // then a sweep passes it by, alive at the exec FIFO.
            let _spawning = self.supervisor.spawning()?;
            self.supervisor
                .watch(slot, &id, state.pid, start_ticks, container)
        });
        if let Err(e) = watched {
            let _ = self.scrub(&id);
            return Err(e);
        }
        // The box as create made it: its container created, its init at the exec FIFO, nothing exec'd
        // in it yet, as the state create wrote says without reading it back.
        Ok(BoxInfo {
            id,
            class: Class::Oci,
            state: BoxState::Created,
            bounds: record.spec.bounds,
            adopted: false,
            exit: None,
            abandoned: 0,
        })
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

    /// The create seed pool: built when [`Node::with_oci_helper`] names the `hex` binary, else here on
    /// first use from this process's own (as [`Node::seed_pool`] finds it). A race to build it drops the
    /// losing pool, whose drop stops its starting thread and ends the spare it started.
    fn create_pool(&self) -> Result<&CreatePool, Refusal> {
        if let Some(pool) = self.create_seeds.get() {
            return Ok(pool);
        }
        let exe = match &self.oci_helper {
            Some(exe) => exe.clone(),
            None => {
                std::env::current_exe().map_err(|e| Refusal::os("finding the hex binary", e))?
            }
        };
        let _ = self.create_seeds.set(CreatePool::new(exe)?);
        self.create_seeds
            .get()
            .ok_or_else(|| Refusal::Broken("the create seed pool wasn't built".to_owned()))
    }

    /// What the node's create seeds have done since the pool opened (decision 5): the counts the bench
    /// and the node's metrics read, or the pool's default when no create has built it. Read around a
    /// phase like the network-namespace counts ([`Node::netns`]).
    pub fn create_seeds(&self) -> Result<hex_core::seeds::Counts, Refusal> {
        match self.create_seeds.get() {
            Some(pool) => pool.counts(),
            None => Ok(hex_core::seeds::Counts::default()),
        }
    }

    /// How many of the node's creates were made through the shim since its seed pool opened: those whose
    /// container joins a pid or user namespace by path, which no seed's one clone can make (decision 5).
    pub fn creates_through_shim(&self) -> u64 {
        self.create_seeds
            .get()
            .map_or(0, CreatePool::creates_through_shim)
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
        // A cgroup under either root is a box's, recorded or not: a failed create's scrub would end it.
        if self.record(&spec.id)?.is_some() || self.cgroup(&spec.id).is_some() {
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
        // no container under the oci root is a process box, no box, or one made before its node kept
        // container state there, which the record then says (goal 3 plan step 3: start costs one FIFO
        // operation and the state read beside it). The init's liveness is judged by the pidfd the
        // supervisor holds for it, a poll.
        let alive = |_: &State| matches!(self.supervisor.runs(id), Ok(true));
        let mut started = self.oci().start_with(&self.container(id), &alive);
        let mut record = None;
        if matches!(started, Err(OciError::NoSuchContainer(_))) {
            record = self.record(id)?;
            // A box made before defda14: its container's state is where its record names it.
            let elsewhere = record
                .as_ref()
                .filter(|r| r.dirs.first() != Some(&self.container_dir(id)))
                .and_then(|r| self.container_of(r));
            if let Some((oci, name)) = elsewhere {
                started = oci.start_with(&name, &alive);
            }
        }
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
            Err(OciError::NoSuchContainer(_)) => match record.map(|r| r.spec.class) {
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
            .cgroup(id)
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
            .cgroup(id)
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
        if record.is_none() && self.cgroup(id).is_none() {
            return Err(Refusal::NoSuchBox(id.clone()));
        }
        // Unwatched first, so the supervisor stops collecting the init; an exit it's recording as it's
        // unwatched goes into this box's record alone, which the scrub removes.
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

    fn netns(&self) -> Result<netns::Counts, Refusal> {
        Node::netns(self)
    }
}

/// What a box still holds when a delete can't empty it: each surviving process with its command,
/// state and parent, so the refusal names what is stuck, not only that something is (§3). Bounded to a
/// few: enough to see what it is. A descendant cgroup's are named under its path. `cgroup.procs`
/// leaves out a process whose leader has ended while another of its threads hasn't (Linux v6.19,
/// `kernel/cgroup/cgroup.c:5112-5121`), so with no process listed the box's threads are named
/// instead, each with its process, state and where it waits.
fn survivors(cg: &hex_runtime::linux::cgroup::BoxCgroup) -> String {
    let root = cg.dir().to_path_buf();
    let (mut out, mut threads) = (String::new(), String::new());
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
        if let Ok(text) = fs::read_to_string(dir.join("cgroup.threads")) {
            for tid in text
                .lines()
                .filter_map(|l| l.trim().parse::<u32>().ok())
                .take(16)
            {
                let status = fs::read_to_string(format!("/proc/{tid}/status")).unwrap_or_default();
                let field = |name: &str| {
                    status
                        .lines()
                        .find_map(|l| l.strip_prefix(name))
                        .map(|v| v.trim().to_owned())
                        .unwrap_or_default()
                };
                let wchan = fs::read_to_string(format!("/proc/{tid}/wchan"))
                    .map(|s| s.trim().to_owned())
                    .unwrap_or_default();
                threads.push_str(&format!(
                    " [{rel}/{tid} of {} {:?} state {} wchan {wchan}]",
                    field("Tgid:"),
                    field("Name:"),
                    field("State:")
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
    match (out.is_empty(), threads.is_empty()) {
        (false, _) => out,
        (true, false) => format!(" (no process listed, only threads:{threads})"),
        // Populated when the wait last read it, nothing listed now: its last task ended in between.
        (true, true) => {
            " (populated, but no process or thread listed: the last ended as they were read)"
                .to_owned()
        }
    }
}

fn why_text(why: &Why) -> String {
    match why {
        Why::MidCreate => "the node agent stopped while it was being made".to_owned(),
        Why::MidDelete => "the node agent stopped while it was being removed".to_owned(),
        Why::InitGone => "its container's init had ended".to_owned(),
        Why::NoCgroup => "its record had no cgroup".to_owned(),
        Why::NoRecord(parts) => {
            let left: Vec<&str> = [
                (parts.cgroup, "its cgroup"),
                (parts.state_dir, "its container's state"),
                (parts.bundle, "its bundle"),
            ]
            .into_iter()
            .filter(|(found, _)| *found)
            .map(|(_, what)| what)
            .collect();
            format!(
                "no record named it, made only in part: {} left",
                left.join(", ")
            )
        }
        Why::Unreadable(w) => format!("its record couldn't be read: {w}"),
    }
}

/// The box ids named by the directories in `dir` whose names start with `prefix`, the rest of each name
/// the id, or none when `dir` isn't there: a box's parts there are directories, so any other entry is no
/// box's. The listing ends at [`LIST_PATIENCE`], refused past it by type.
fn ids_in(dir: &Path, prefix: &str) -> Result<BTreeSet<BoxId>, Refusal> {
    let listing = |e| Refusal::os("listing a directory for boxes' parts", e);
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(e) => return Err(listing(e)),
    };
    let deadline = Instant::now().checked_add(LIST_PATIENCE);
    let mut ids = BTreeSet::new();
    for entry in entries {
        if deadline.is_none_or(|d| Instant::now() >= d) {
            return Err(Refusal::Os {
                op: "listing a directory for boxes' parts",
                why: format!(
                    "{} was still being listed after {LIST_PATIENCE:?}",
                    dir.display()
                ),
            });
        }
        let entry = entry.map_err(listing)?;
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        if let Some(id) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.strip_prefix(prefix))
            .and_then(|n| BoxId::new(n).ok())
        {
            ids.insert(id);
        }
    }
    Ok(ids)
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

fn counts_poisoned() -> Refusal {
    Refusal::Broken("the node's abandoned-program counts' lock was poisoned".to_owned())
}

/// The device programs of a node that runs as `owner`, at most `bound` held open: a root node's start
/// with the default policy's program on its oci boxes' root; a rootless node loads none, its containers
/// held to their device lists by construction (goal 5 extended, step 1).
fn device_programs(owner: Owner, oci_boxes: &BoxRoot, bound: usize) -> Result<Programs, Refusal> {
    match owner {
        Owner::Root => Programs::at_root(oci_boxes.dir(), bound).map_err(Refusal::from),
        Owner::Rootless { .. } => Ok(Programs::rootless(bound)),
    }
}

/// Where a node keeps its containers' state: on tmpfs, where a host crash takes it with the
/// containers it describes, as runc and crun keep theirs (decision 1; runc's `--root` "should be
/// located in tmpfs", crun(1) STATE): `/run/hex-node` for a node that runs as root,
/// `$XDG_RUNTIME_DIR/hex-node` for one that doesn't, shared by the nodes there, each container named
/// for its node ([`Node::container_dir`]). A rootless node with no `$XDG_RUNTIME_DIR` keeps it in its
/// state directory instead, where it outlives the host and its boxes' records are fsynced for it (the
/// store judges by filesystem). Readable by its owner alone.
fn oci_root(owner: Owner, state_dir: &Path) -> Result<PathBuf, Refusal> {
    let root = match owner {
        Owner::Root => PathBuf::from("/run/hex-node"),
        Owner::Rootless { .. } => std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .map_or_else(|| state_dir.join("oci"), |dir| dir.join("hex-node")),
    };
    fs::create_dir_all(&root)
        .and_then(|()| fs::set_permissions(&root, fs::Permissions::from_mode(0o700)))
        .map_err(|e| Refusal::os("making the containers' state directory", e))?;
    Ok(root)
}

/// Where the node whose state directory is `state_dir`, and whose runtime directory is `oci_root`, keeps
/// the records of its boxes that needn't outlive the host ([`crate::store`]): a directory of its own
/// among its containers' state, `<key>.records`, a name no container's takes (theirs are `<key>-<id>`).
pub fn records_dir(oci_root: &Path, state_dir: &Path) -> Result<PathBuf, Refusal> {
    Ok(oci_root.join(format!("{}.records", node_key(state_dir)?)))
}

/// A box's container's name under an oci root other nodes share: the node's key, then the box's id,
/// so two nodes' boxes of one id never share a container.
fn container_name(key: &str, id: &BoxId) -> String {
    format!("{key}-{id}")
}

/// A node's key among the nodes that share its oci root: FNV-1a (64-bit) of its state directory's
/// canonical path, the node's identity on its host, so a node restarted on the same state directory
/// finds its containers again and two nodes never name one alike. FNV-1a because the key must come out
/// the same in every run of every build, which std's hashers don't promise (`DefaultHasher`'s
/// algorithm may change between releases, `RandomState` is seeded per process), and it takes five
/// lines rather than a new dependency. The offset basis and prime are FNV-1a's own (IETF
/// draft-eastlake-fnv, "FNV Basics" and "FNV Constants").
fn node_key(state_dir: &Path) -> Result<String, Refusal> {
    use std::os::unix::ffi::OsStrExt;
    let path = fs::canonicalize(state_dir)
        .map_err(|e| Refusal::os("finding the state directory's path", e))?;
    Ok(format!("{:016x}", fnv1a(path.as_os_str().as_bytes())))
}

/// FNV-1a, 64-bit: its arithmetic is modulo 2^64 by definition, so the multiply wraps.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash: u64, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
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

    /// Goal 5 plan step 2: a node's key, which names its containers in the oci root it shares with
    /// other nodes, is its state directory's: the same directory however it's named gives the same
    /// key, and two directories give two. FNV-1a's 64-bit test vectors for "" and "a" hold.
    #[test]
    fn a_nodes_key_is_its_state_directorys() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let key = node_key(a.path()).unwrap();
        assert_eq!(key.len(), 16, "{key}");
        assert_eq!(
            node_key(&a.path().join(".")).unwrap(),
            key,
            "the same directory, named otherwise"
        );
        assert_ne!(node_key(b.path()).unwrap(), key);
    }

    /// Goal 5 plan step 2: two nodes with state directories of their own name a box of one id two
    /// containers, and a node restarted on the same directory names it as before.
    #[test]
    fn two_nodes_name_a_box_of_one_id_two_containers() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let id = BoxId::new("same").unwrap();
        let in_a = container_name(&node_key(a.path()).unwrap(), &id);
        let in_b = container_name(&node_key(b.path()).unwrap(), &id);
        assert_ne!(in_a, in_b);
        assert!(
            in_a.ends_with("-same") && in_b.ends_with("-same"),
            "{in_a}, {in_b}"
        );
        assert_eq!(container_name(&node_key(a.path()).unwrap(), &id), in_a);
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
