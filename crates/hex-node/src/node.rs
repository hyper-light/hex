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
use hex_core::patience::Patience;
use hex_core::record::{BoxRecord, Phase, ProcId, Slot};
use hex_core::recovery::{self, Decision, Parts, Seen, Why};
use hex_core::reuse;
use hex_core::wire;
use hex_runtime::linux::cgroup::{BoxCgroup, BoxRoot, MAX_BOXES};
use hex_runtime::linux::oci::OciError;
use hex_runtime::linux::oci::devshare::MAX_PROGRAMS;
use hex_runtime::linux::oci::programs::{self, Programs};
use hex_runtime::linux::oci::run::{ClassicReason, Held, Runtime as OciRuntime};
use hex_runtime::linux::oci::seed::{Direct, Specs, exec_direct, open_cgroup};
use hex_runtime::linux::oci::state::{self, Status, start_time};
use hex_runtime::linux::proc::PidfdProc;
use hex_runtime::linux::{disks, process};

use crate::cgroups::{self, Cgroups, Choice, Made};
use crate::createpool::CreatePool;
use crate::devfs::Devfs;
use crate::ipcuts::IpcUtsPool;
use crate::netns::{Netns, Pools};
use crate::remove::remove_tree;
use crate::seed::SeedPool;
use crate::sizes::{Keeper, Sizes};
use crate::store::Store;
use crate::supervise::{self, Forgotten, Live, Supervisor};
use crate::turns::{Turn, Turns};

/// overlayfs's magic number (include/uapi/linux/magic.h): a node whose state is on overlayfs can't
/// keep a layered box's writable layer there, since overlayfs can't be an overlay's upper layer.
const OVERLAYFS_SUPER_MAGIC: i128 = 0x794c_7630;

/// How long a killed box takes to empty, from its kill (decision 9; Ada's 2026-11-06 rule, the lead's
/// failure-bound rule): twice the slowest emptying recorded by this process's nodes, from every delete, scrub
/// and removal, since all wait on one kernel. A wait that runs out is a fault to report (the kernel is
/// wedged), not a limit to raise (CLAUDE.md §4). Its programs' reaping and its init's end, which a box's
/// emptying waits on, share the wait. MEASURING: no record times a box's emptying from its kill (the delete
/// records, `bench/competitive/probes/oci_breakdown.md`'s among them, time the whole delete); the record to come is
/// `bench/competitive/probes/waits_census.md`'s `node.emptying` row, which then seeds this. Until then a node's
/// first emptying is waited out, and arms the bound.
static EMPTYING: Patience = Patience::measuring("node.emptying");

/// What is left at the time of asking of the wait for a box killed at `killed` to empty ([`EMPTYING`]):
/// `Duration::MAX`, unbounded, while it is measuring with no emptying seen.
pub(crate) fn empty_patience(killed: Instant) -> Duration {
    EMPTYING
        .deadline()
        .map_or(Duration::MAX, |wait| wait.saturating_sub(killed.elapsed()))
}

/// Waits for `cg`, killed at `killed`, to empty within the learned wait ([`EMPTYING`]), which it teaches
/// when it empties: the time from its kill. Says whether it emptied, and the wait it had from the kill.
pub(crate) fn wait_empty(cg: &BoxCgroup, killed: Instant) -> Result<(bool, Duration), Refusal> {
    let wait = EMPTYING.deadline();
    let emptied = cg.wait_empty(empty_patience(killed))?;
    if emptied {
        EMPTYING.answered(killed.elapsed());
    }
    Ok((emptied, wait.unwrap_or(Duration::MAX)))
}

/// How long [`Runtime::delete`] spends taking a box's directories apart before it refuses, leaving
/// the rest for a later delete or recovery to finish where it stopped: a bound on a caller's wait, not
/// a guess at how long a layer takes, so a layer of any size goes, over as many calls as it needs.
pub const REMOVE_PATIENCE: Duration = Duration::from_secs(60);

/// How long [`Node::recover`] spends on each entry of a directory it lists for the parts of boxes before it
/// refuses: the runtime directory is shared with every node of this user on the host, so its entries are
/// theirs as well as this node's, and a listing's time grows with them. So the sample is a listing's time per
/// entry, and the wait for a listing's first `k` entries is `k` times twice the slowest recorded (decision 9;
/// Ada's 2026-11-06 rule, the lead's failure-bound rule), refused by type past it (CLAUDE.md §2, §4). MEASURING:
/// no record times a listing per entry; the record to come is `bench/competitive/probes/waits_census.md`'s
/// `node.listing` row. Until then recovery's first listing, the state
/// directory's, is bounded by its entries, which are finite, not by time, and its time per entry arms the bound.
static LISTING: Patience = Patience::measuring("node.listing");

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
    /// The `oci` boxes' cgroups that may be handed from box to box of one owner, and how the node's
    /// deletes ended their boxes (goal 5 extended, step 6).
    cgroups: Cgroups,
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
    /// A debug-only crash point a test arms on this node's create seeds ([`Node::with_inject`]), for the
    /// crash-safety test of the pruned init's root build; `None` unless a test arms it, and absent from
    /// release builds (CLAUDE.md §6).
    #[cfg(debug_assertions)]
    inject: Option<String>,
    /// The way a pruned create's mount namespace is made, as its owner asked (`copy`, `empty` or `tree`,
    /// [`Node::with_mount_namespace`]); `None` takes the node's default (decision 6's update).
    mount_namespace: Option<String>,
    /// The network namespaces the node makes ahead for its containers, and what it counts of every
    /// container's (decision 4; [`Node::netns`]).
    netns: Netns,
    /// Each box's turn to be made or removed. A box has no record until it's whole (goal 5 extended,
    /// step 5), so this is what tells a box being made: a second create of its id, or a delete, waits
    /// for the create under way and then finds the box made or not, rather than make or remove parts at
    /// the paths the id fixes; so neither scrubs what the other is making. At most [`MAX_BOXES`] turns
    /// at once, the node's box budget, a wait learned from how long creates and deletes held their turns
    /// (decision 9: twice the longest held, measuring until the first is let go, a wait before it lasting
    /// until then); past either the call is refused by type, a create past the budget as [`Refusal::Full`]
    /// ([`create_turn`]).
    turns: Turns,
    /// The debug-only crash point the caller armed ([`Node::with_crash_point`]), `None` unless one did.
    #[cfg(debug_assertions)]
    crash_at: Option<CrashPoint>,
    /// The ipc and uts namespaces it makes ahead (goal 5 extended, step 4; [`Node::ipc_uts`]).
    ipc_uts: IpcUtsPool,
    /// Both pools' bounds, which the kept namespaces' files are bounded past ([`crate::netns::FilesBound`]).
    pools: Pools,
    devfs: Devfs,
    /// Keeps what the pools learned in the node's state, for its next start ([`crate::sizes::Keeper`]).
    keeper: Keeper,
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
    /// Slots removed that no box's record named: idle when the agent stopped, or a create's caught
    /// before its record (goal 5 extended, step 6).
    pub slots_removed: Vec<u64>,
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
        // user namespace of their own (decision 4). The kept ones hold their files within the descriptor
        // limit the node was given, never raised, past what both pools hold.
        // Every pool's bound is derived from this host and its last start's demand, and its size learned from the
        // demand it sees from where the last start peaked (the host's parallelism on a first start), so a restart
        // takes no cold miss the last start had learned past (Ada's rule and the lead's calls, 2026-11-06;
        // crate::sizes).
        let keeper = Keeper::open(state_dir)?;
        let sizes = Sizes::learned(&keeper.base())?;
        let pools = match owner {
            Owner::Root => Pools::root(sizes),
            Owner::Rootless { .. } => Pools::rootless(sizes),
        };
        let netns = Netns::learning(pools.netns, crate::netns::FilesBound::now(pools)?, sizes)?;
        let ipc_uts = IpcUtsPool::learning(sizes.starts.ipc_uts, pools.ipc_uts)?;
        let oci_boxes = BoxRoot::open(&root.dir().join(OCI_BOXES), root.disks().to_vec())?;
        let devices = device_programs(owner, &oci_boxes, MAX_PROGRAMS)?;
        // Fresh slots are numbered past every slot already under the root, so none is made twice.
        let first = slots_in(&oci_boxes)?
            .last()
            .map_or(Some(0), |n| n.checked_add(1))
            .ok_or_else(|| {
                Refusal::Broken("the oci boxes' root holds the last slot a u64 numbers".to_owned())
            })?;
        let cgroups = Cgroups::start(first, sizes)?;
        cgroups.learn_ahead(sizes.starts.ahead, sizes.ahead);
        // Parked procs are a root node's, always (decision 8, parked procs; the lead, 2026-11-06; their off path went
        // by Ada's 2026-11-06 rule once bench/competitive/probes/parked_procs_ab.md decided them on): a rootless node's
        // devfs makes none, so it parks none.
        let node = Self {
            root,
            oci_boxes,
            devices,
            cgroups,
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
            #[cfg(debug_assertions)]
            inject: None,
            mount_namespace: None,
            specs: Mutex::new(Specs::default()),
            netns,
            ipc_uts,
            pools,
            devfs: Devfs::start(
                pools.devfs,
                pools.devfs,
                pools.devfs,
                hex_core::devfs::Shape {
                    keys: sizes.starts.devfs_keys,
                    per_key: sizes.starts.devfs_per_key,
                    cap: sizes.seeds.get(),
                },
            )?,
            keeper,
            null: rustix::fs::open(
                "/dev/null",
                rustix::fs::OFlags::RDWR | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )
            .map_err(|e| Refusal::os("opening /dev/null", e))?,
            turns: Turns::new(
                "making or removing a box",
                "creates and deletes under way",
                MAX_BOXES,
                // MEASURING: the record to come is `bench/competitive/probes/waits_census.md`'s `node.turns`
                // row, which then seeds it.
                Patience::measuring("node.turns"),
            ),
            #[cfg(debug_assertions)]
            crash_at: None,
        };
        let bound = node.pools.sizes.procs_alive();
        node.park_procs(bound)
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

    /// Whether the node ends abruptly once a box's own cgroup is made (a debug build's crash point).
    #[cfg(debug_assertions)]
    fn crashes_at_cgroup_made(&self) -> bool {
        self.crash_at == Some(CrashPoint::CgroupMade)
    }

    /// A release build has no crash points.
    #[cfg(not(debug_assertions))]
    fn crashes_at_cgroup_made(&self) -> bool {
        false
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

    /// A test control, with no production or bench caller: `false` stops the procs made ahead (decision 6's update,
    /// part 2), so each create mounts its own proc. Its one production consumer is a kernel up to 7.0, where the
    /// pool stops itself at its first make (`MakeFailure::Namespace`); the bench's A/B knob (HEXBENCH_PROCFS) went by
    /// Ada's 2026-11-06 rule. The parked-proc and masked-directory tests (oci_box.rs, AC-T7.1.4) use it to read a
    /// create's own proc, which on 7.1 and later the pool would otherwise serve; a stalled maker would only make the
    /// pool dry after it had made some, never from the first create. Root nodes only.
    pub fn with_prepared_procs(mut self, enabled: bool) -> Result<Self, Refusal> {
        let (dev, _, root) = self.devfs.enabled();
        self.devfs = self.devfs.restart(dev, enabled && root)?;
        Ok(self)
    }

    /// Fresh `/dev` mounts made, taken, discarded or missed. Counts include failed creates and
    /// every preparation failure, so a speed measurement cannot hide a dry or disabled pool.
    pub fn dev_mounts(&self) -> Result<hex_core::devfs::Counts, Refusal> {
        self.devfs.counts()
    }

    /// What the node's prepared proc pool has done (decision 6's update, part 2): the bench and the lab read
    /// its takes and misses around a phase, as [`Node::dev_mounts`]'s.
    pub fn proc_mounts(&self) -> Result<hex_core::devfs::Counts, Refusal> {
        self.devfs.proc_counts()
    }

    /// Sets how many `oci` boxes' inits the node watches at once, [`supervise::INIT_BUDGET`] unless
    /// set: past it `create` of an `oci` box is refused with [`Refusal::Exhausted`].
    pub fn with_init_budget(self, budget: usize) -> Result<Self, Refusal> {
        self.supervisor.set_budget(budget)?;
        Ok(self)
    }

    /// Arms a debug-only crash point on this node's create seeds (goal 5 plan step 2): the pruned init
    /// ends at the named point, as a kill would, so a test proves the agent cleans up a container whose
    /// init dies mid-build (CLAUDE.md §6). Must come before [`Node::with_oci_helper`], which builds the
    /// seed pool. Debug builds only; the point names are the runtime's (`seed::INJECT_DIE_BEFORE_SWITCH`).
    #[cfg(debug_assertions)]
    pub fn with_inject(mut self, point: &str) -> Self {
        self.inject = Some(point.to_owned());
        self
    }

    /// Asks for a way of making a pruned create's mount namespace (decision 6's update): `copy`, `empty` or
    /// `tree`, for the bench's A/B and the lab; a node agent asks for none, taking the way its kernel's probe chooses
    /// ([`hex_runtime::linux::oci::mntns::default_way`]).
    /// Must come before [`Node::with_oci_helper`], which builds the seed pool. A way this node can't take is
    /// refused by type at its first create.
    pub fn with_mount_namespace(mut self, way: &str) -> Self {
        self.mount_namespace = Some(way.to_owned());
        self
    }

    /// Names the `hex` binary the `oci` class re-execs for a container's init and `exec` helper, for
    /// a node whose own binary isn't `hex` (the bench, a node linked into another program).
    pub fn with_oci_helper(mut self, hex_binary: PathBuf) -> Self {
        // The create seeds are built now, from this binary, so their first spare is up before the first
        // create (decision 5): no create waits on a seed's exec. A pool that can't be built now is built
        // at the first create, which reports why.
        if let Ok(pool) = CreatePool::new(
            hex_binary.clone(),
            self.pools.sizes,
            self.seed_prune(),
            self.mount_namespace.as_deref(),
            #[cfg(debug_assertions)]
            self.inject.clone(),
        ) {
            let _ = self.create_seeds.set(pool);
        }
        self.oci_helper = Some(hex_binary);
        self
    }

    /// The mountpoint a root node's create seeds prune their mount namespace at (goal 5 plan step 2), or
    /// `None` on a rootless node, whose seeds keep the agent's mount namespace and build the root the
    /// classic way. A dedicated directory under the oci root, which the seed mounts a fresh tmpfs over
    /// and pivots away from in its own namespace: the agent's view of it stays an empty directory, and
    /// nothing scans the oci root (recovery reaches containers by the paths their records name).
    fn seed_prune(&self) -> Option<PathBuf> {
        match self.owner {
            Owner::Root => Some(self.oci_root.join(".seed-root")),
            Owner::Rootless { .. } => None,
        }
    }

    /// A test control, with no production or bench caller: the network namespaces a root node makes ahead fixed at
    /// `bound` in place of the pool it opened with, which learns how many from its draws up to its [`Sizes::netns`]
    /// (Ada's rule, 2026-11-06), and which this closes. With 0 it makes none, and every container that makes a
    /// network namespace makes it on its create's path, counted. HEX-NETNS-9 (oci_box.rs) needs that from the first
    /// create: a stalled maker would leave dry only a pool that had already made some, so it can't stand in. A
    /// rootless node makes none (its containers' namespaces belong to user namespaces of their own), so a bound
    /// above 0 there is refused.
    pub fn with_netns_bound(mut self, bound: usize) -> Result<Self, Refusal> {
        if bound > 0 && self.owner != Owner::Root {
            return Err(Refusal::Invalid {
                what: "network-namespace bound",
                why: format!(
                    "a rootless node makes no network namespaces ahead, so it can't hold {bound}"
                ),
            });
        }
        self.pools.netns = bound;
        self.netns = Netns::start(
            bound,
            crate::netns::FilesBound::now(self.pools)?,
            self.pools.sizes,
        )?;
        Ok(self)
    }

    /// A test control, with no production or bench caller (the bench's drained arm went by Ada's 2026-11-06 rule):
    /// the ipc and uts namespace pairs a root node makes ahead fixed at `bound`, in place of the pool it opened
    /// with, whose size it learns from its draws, which this closes. With 0 it makes none, and every container's
    /// clone makes its own, counted. HEX-IPCUTS-3 (oci_ipcuts.rs) needs a create that finds none from the first
    /// and pairs it can count exactly; a stalled maker would leave dry only a pool that had already made some. A
    /// rootless node makes none, so a bound above 0 there is refused.
    pub fn with_ipc_uts_bound(mut self, bound: usize) -> Result<Self, Refusal> {
        if bound > 0 && self.owner != Owner::Root {
            return Err(Refusal::Invalid {
                what: "ipc and uts namespace bound",
                why: format!(
                    "a rootless node makes no ipc or uts namespaces ahead, so it can't hold {bound}"
                ),
            });
        }
        self.ipc_uts = IpcUtsPool::start(bound)?;
        self.pools.ipc_uts = bound;
        // The kept namespaces' files are bounded past this pool too, so the network-namespace pool starts again
        // with the bound they leave.
        self.netns = Netns::start(
            self.pools.netns,
            crate::netns::FilesBound::now(self.pools)?,
            self.pools.sizes,
        )?;
        Ok(self)
    }

    /// A test control, with no production or bench caller (the bench's HEXBENCH_AHEAD pin went by Ada's 2026-11-06
    /// rule): the node keeps `bound` cgroups made ahead for boxes of no owner, each with an init parked in it
    /// (decision 8, "The p99.9 that stayed"), in place of the number it learns from its draws, from the host's
    /// parallelism up ([`crate::sizes::Sizes`]). 0 keeps none: each such box makes its own cgroup, named by its id.
    /// The tests that read one parked init's claim (oci_reuse.rs, oci_box.rs) need exactly one ready, and those of a
    /// box of no owner's own cgroup none ever; a learned pool makes as many as the host's parallelism, and a stalled
    /// refill would leave none only after it had made some.
    pub fn with_inits_ahead(self, bound: usize) -> Self {
        self.cgroups.set_ahead(bound);
        self
    }

    /// Each init the node parks ahead (decision 8, "The p99.9 that stayed") has the proc of its own pid namespace
    /// made with it, with the masks and read-only paths of the spec it is parked for: a claim whose spec has the same
    /// key attaches it, mounting no proc of its own (decision 8, parked procs). At most `bound` are alive at once: the
    /// dropper's queue holds as many, so a tree handed to it never finds it full. Where the node has a proc maker (a
    /// root node's); a rootless node parks none. Refused when the dropper can't be started.
    fn park_procs(self, bound: usize) -> Result<Self, Refusal> {
        if let Some(maker) = self.devfs.proc_maker() {
            self.cgroups.set_parked_procs(maker, bound)?;
        }
        Ok(self)
    }

    /// Whether the node parks procs with its inits (decision 8, parked procs): a root node's, never a rootless one's.
    pub fn parked_procs(&self) -> bool {
        self.cgroups.makes_parked_procs()
    }

    /// A test control, with no production or bench caller: at most `bound` parked procs alive at once in place of the
    /// node's [`crate::sizes::Sizes::procs_alive`], which is derived from the host and so too high for a test to reach.
    /// The bound test (oci_reuse.rs, AC-T7.1.3) needs the cap itself reached, with a handful of boxes; a stalled maker
    /// would leave procs unmade, never capped, so it can't stand in. Refused past the dropper's queue, started at the
    /// node's own bound.
    pub fn with_parked_procs_bound(self, bound: usize) -> Result<Self, Refusal> {
        self.park_procs(bound)
    }

    /// Sets the most device programs the node holds open, one per policy, [`MAX_PROGRAMS`] unless set,
    /// in place of those it opened with, which it closes (each lives on in the cgroups it's attached to).
    /// A root node's oci boxes' root keeps its program.
    pub fn with_device_program_bound(mut self, bound: usize) -> Result<Self, Refusal> {
        self.devices = device_programs(self.owner, &self.oci_boxes, bound)?;
        Ok(self)
    }

    /// The node's pools' bounds on its host, and where their learned sizes start (Ada's rule, 2026-11-06).
    pub fn sizes(&self) -> Sizes {
        self.pools.sizes
    }

    /// Why the node's last peaks couldn't be read, where they couldn't: its start was a first start (the lead,
    /// 2026-11-06). And the peaks its keeper has written since, with the last write's failure if one failed.
    pub fn peaks_kept(&self) -> (Option<String>, u64, Option<String>) {
        let (written, failure) = self.keeper.counts();
        (self.keeper.unread().map(str::to_owned), written, failure)
    }

    /// What the pools have learned now, and each memory pool's takes in this start, as [`hex_core::demand::Peaks`]
    /// keeps them: idle cgroups, cgroups made ahead, idle network namespaces, network namespaces made ahead, seeds,
    /// and ipc and uts pairs. A pool whose lock was poisoned reads as having learned nothing more.
    fn peaks_now(&self) -> hex_core::demand::Peaks {
        let (owner_cgroups, ahead) = self.cgroups.learned().unwrap_or((0, 0));
        let (netns, owner_netns) = self.netns.learned().unwrap_or((0, 0));
        let (devfs_keys, devfs_per_key) = self.devfs.learned().unwrap_or((0, 0));
        let (seeds, spare) = self
            .create_seeds
            .get()
            .and_then(|p| p.learned().ok())
            .unwrap_or((0, 0));
        let cg = self.cgroups.counts().ok();
        let kept = self.netns.reuse_counts().ok().flatten();
        let made = self.netns.counts().ok();
        let ipc = self.ipc_uts.counts().ok();
        let served = self
            .create_seeds
            .get()
            .and_then(|p| p.counts().ok())
            .map_or(0, |c| c.handed);
        hex_core::demand::Peaks {
            owner_cgroups,
            owner_netns,
            netns,
            ipc_uts: self.ipc_uts.bound().unwrap_or(0),
            ahead,
            devfs_keys,
            devfs_per_key,
            seeds,
            spare,
            takes: [
                cg.as_ref()
                    .map_or(0, |c| c.pool.handed_on.saturating_add(c.pool.fresh)),
                cg.as_ref()
                    .map_or(0, |c| c.ahead.taken.saturating_add(c.ahead.dry)),
                kept.as_ref()
                    .map_or(0, |k| k.pool.handed_on.saturating_add(k.pool.fresh)),
                made.map_or(0, |c| c.taken.saturating_add(c.made_on_path)),
                served,
                ipc.map_or(0, |c| c.taken.saturating_add(c.made_on_path)),
            ],
        }
    }

    /// Hands what the pools learned to the keeper when a size rose since it last did: off the create's own work,
    /// the snapshot taken only after a rise, which no draw already seen makes again.
    fn keep_peaks(&self) {
        if self.keeper.behind() {
            let rises = crate::sizes::rises();
            self.keeper.offer(&self.peaks_now(), rises);
        }
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

    /// Box `id`'s cgroup directory, while it has one: a slot's for a box given one, else its own.
    pub fn cgroup_dir(&self, id: &BoxId) -> Option<PathBuf> {
        self.cgroup(id).map(|cg| cg.dir().to_path_buf())
    }

    /// What the node's pool of idle cgroups has done since it opened (goal 5 extended, step 6).
    pub fn cgroup_reuse(&self) -> Result<cgroups::Counts, Refusal> {
        self.cgroups.counts()
    }

    /// How the node's deletes have ended their boxes since it opened: through an init's pidfd, that
    /// falling back to `cgroup.kill`, or `cgroup.kill` (goal 5 extended, step 6).
    pub fn kills(&self) -> cgroups::Kills {
        self.cgroups.kills()
    }

    /// Removes every idle cgroup now, before it returns, and closes every idle kept network namespace: for
    /// a caller that wants none left waiting (a test's end, a shutdown). Recovery removes the cgroups a
    /// crash leaves; a crash closes the namespaces with the node's descriptors.
    pub fn release_idle(&self) -> Result<(), Refusal> {
        self.netns.release_idle()?;
        self.cgroups.drain(&self.oci_boxes)
    }

    /// What the network namespaces kept for owners have done (goal 5 extended, step 6); `None` on a node
    /// that keeps none (a rootless node, whose containers make theirs in user namespaces of their own).
    pub fn netns_reuse(&self) -> Result<Option<crate::netns::ReuseCounts>, Refusal> {
        self.netns.reuse_counts()
    }

    /// Box `id`'s cgroup: a slot's for a live box given one, else an `oci` box's under the oci boxes'
    /// root, else (a `process` box, or an `oci` box made before its node kept them apart) under the box
    /// root.
    fn cgroup(&self, id: &BoxId) -> Option<BoxCgroup> {
        if let Ok(Some(n)) = self.cgroups.slot_of(id) {
            return self.oci_boxes.slot(n);
        }
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
    /// first. `None` for a record that names no directory: a `process` box's.
    fn container_of(&self, record: &BoxRecord) -> Option<(OciRuntime, String)> {
        let (root, name) = state::root_of(record.dirs.first()?)?;
        Some((self.oci_in(root), name))
    }

    /// Box `id`'s container's name under the node's oci root ([`container_name`]).
    fn container(&self, id: &BoxId) -> String {
        container_name(&self.key, id)
    }

    /// Where box `id`'s container keeps its state, its `state.json` and exec FIFO: on tmpfs, in its shard of
    /// the runtime directory ([`oci_root`], [`state::shard`]; goal 5 extended, step 6).
    pub fn container_dir(&self, id: &BoxId) -> PathBuf {
        state::Dir::new(&self.oci_root, &self.container(id))
            .path()
            .to_path_buf()
    }

    /// Where a node before the shards kept box `id`'s container state: in the runtime directory itself.
    fn unsharded_container_dir(&self, id: &BoxId) -> PathBuf {
        state::Dir::unsharded(&self.oci_root, &self.container(id))
            .path()
            .to_path_buf()
    }

    /// The runtime directory the node keeps its containers' state in ([`oci_root`]).
    pub fn oci_root(&self) -> &Path {
        &self.oci_root
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

    /// What the node has counted of its containers' ipc and uts namespaces since its pool opened (goal 5
    /// extended, step 4): pairs made ahead, taken by a create, made on a create's path (the pool dry, or a
    /// container the pool can't serve), discarded at shutdown, and makes that failed. Read in process, as the
    /// seeds' counts are.
    pub fn ipc_uts(&self) -> Result<netns::Counts, Refusal> {
        self.ipc_uts.counts()
    }

    /// How many creates and deletes have waited for one of the same box under way ([`Node::turns`]).
    pub fn waited_for_a_box(&self) -> u64 {
        self.turns.waited()
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
        let slots: BTreeSet<u64> = slots_in(&self.oci_boxes)?.into_iter().collect();
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
        // This node's containers, `<key>-<id>` among every node's in the runtime directory's shards and, where
        // a node before the shards kept them, in the runtime directory itself (goal 5 extended, step 6); and its
        // rendered bundles, `<id>` under its bundles directory.
        let prefix = format!("{}-", self.key);
        let mut state_dirs = ids_in(&self.oci_root, &prefix)?;
        for n in 0..state::SHARDS {
            state_dirs.extend(ids_in(&self.oci_root.join(format!("{n:02x}")), &prefix)?);
        }
        let bundles = ids_in(&self.bundles, "")?;
        for id in state_dirs.iter().chain(&bundles) {
            seen.entry(id.clone()).or_default();
        }
        for (id, s) in &mut seen {
            // A box given a slot has its cgroup where its record names it, not at its id (goal 5
            // extended, step 6).
            let slot = match &s.record {
                Some(Ok(r)) => r.slot.map(|slot| slot.n),
                _ => None,
            };
            s.parts = Parts {
                cgroup: match slot {
                    Some(n) => slots.contains(&n),
                    None => self.cgroup(id).is_some(),
                },
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
                s.start_interrupted =
                    hex_runtime::linux::oci::state::start_interrupted(&self.container_dir(id));
            }
        }
        let mut summary = RecoverySummary::default();
        // The slots no readable record names, idle or a create's caught before its record: ended and
        // removed first, so nothing of them is left (T-T7.1.2).
        for n in recovery::strays(&seen, &slots) {
            if let Some(cg) = self.oci_boxes.slot(n) {
                let killed = Instant::now();
                cg.kill()?;
                let (emptied, patience) = wait_empty(&cg, killed)?;
                if !emptied {
                    let op = "removing an idle cgroup";
                    let mut why =
                        format!("slot {n} still holds processes {patience:?} after cgroup.kill:");
                    why.push_str(&survivors(&cg, why_room(op, &why)));
                    return Err(Refusal::Os { op, why });
                }
                cg.remove()?;
            }
            summary.slots_removed.push(n);
        }
        for (id, decision) in recovery::plan(&seen) {
            match decision {
                Decision::Adopt => {
                    // A box taken back keeps its slot, reported from the baseline its record holds.
                    let slot = seen
                        .get(&id)
                        .and_then(|s| s.record.as_ref())
                        .and_then(|r| r.as_ref().ok())
                        .and_then(|r| r.slot);
                    if let Some(slot) = slot
                        && let Some(cg) = self.oci_boxes.slot(slot.n)
                    {
                        self.cgroups.adopt(&id, slot.n, slot.baseline, &cg)?;
                    }
                    summary.adopted.push(id);
                }
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
        self.scrub_ending(id, &Ending::Kill, None)
    }

    /// Ends what runs in box `id` as `ending` says ([`Node::end`]) and removes every trace of it, each
    /// part tolerant of being gone. A box given a slot has it named by its record or
    /// `held`, its delete's; the slot goes back to its owner's idle cgroups last, once its box's record is
    /// gone, so a crash before leaves it to recovery and never to two boxes (goal 5 extended, step 6).
    fn scrub_ending(
        &self,
        id: &BoxId,
        ending: &Ending<'_>,
        mut held: Option<cgroups::Held>,
    ) -> Result<(), Refusal> {
        let record = match self.store.get(id)? {
            Some(Ok(r)) => Some(r),
            None | Some(Err(_)) => None,
        };
        let slot = record
            .as_ref()
            .and_then(|r| r.slot)
            .map(|slot| slot.n)
            .or_else(|| held.as_ref().map(|h| h.slot));
        let cg = match slot {
            Some(n) => self.oci_boxes.slot(n),
            None => self.cgroup(id),
        };
        if let Some(cg) = &cg {
            if self.end(id, cg, ending)?
                && let Some(held) = held.as_mut()
            {
                held.mark_killed();
            }
            if slot.is_none() {
                cg.remove()?;
                // Gone from either root, so a later create of this id needs no lookup to find it absent.
                self.oci_boxes.forget(id.as_str());
                self.root.forget(id.as_str());
            }
        }
        // The directories the box owns: those its record names (a box made before its node kept
        // container state under the runtime directory names `<state>/oci/<id>`), or, with no record to
        // read, those at the paths its id fixes, which a box caught mid-create left: its container's in its
        // shard or, made by a node before the shards, in the runtime directory itself.
        let dirs = match record {
            Some(r) => r.dirs,
            None => vec![
                self.container_dir(id),
                self.unsharded_container_dir(id),
                self.bundles.join(id.as_str()),
            ],
        };
        for dir in &dirs {
            remove_tree(dir, REMOVE_PATIENCE)?;
        }
        self.store.remove(id)?;
        if let (Some(cg), Some(_)) = (cg, slot) {
            match held {
                Some(held) => self.cgroups.give_back(cg, held, &self.oci_boxes)?,
                None => cg.remove()?,
            }
        }
        // Its network namespace, if the node kept it, goes to be checked for its owner's next box, once
        // its record is gone, as its cgroup does (goal 5 extended, step 6).
        self.netns.give_back(id)?;
        Ok(())
    }

    /// Ends every process in box `id`'s cgroup `cg` and waits until none is left, bounded. An `oci` box
    /// whose init made a pid namespace of its own is ended through the init's pidfd, which takes no
    /// `cgroup_mutex` and kills the namespace (research f45, f46, f50). Its exec'd programs are reaped,
    /// since the init's end waits for them (f48), and once the init has ended anything still in the cgroup
    /// with a process of its own is outside the namespace (f47): then, and for every other box, ended
    /// by `cgroup.kill` as before (f49). An `oci` box whose init ended and was reaped before its delete
    /// needs no kill when its cgroup holds nothing: a pid namespace of its own ended with it. Each way is
    /// counted ([`Node::kills`]). Says whether it wrote `cgroup.kill`, after which the cgroup can't be
    /// handed on ([`cgroups::Held`]'s `killed`).
    fn end(&self, id: &BoxId, cg: &BoxCgroup, ending: &Ending<'_>) -> Result<bool, Refusal> {
        if let Ending::Ended = ending
            && !cg.populated()?
        {
            self.cgroups.ended_through_init();
            return Ok(false);
        }
        if let Ending::Init(init) = ending
            && init.pid_namespace
        {
            let killed = Instant::now();
            init.kill()?;
            self.supervisor.reap_programs(id, empty_patience(killed))?;
            if !init.ended_within(empty_patience(killed))? {
                return Err(Refusal::Os {
                    op: "emptying a box",
                    why: format!(
                        "box {id}'s init still runs {:?} after SIGKILL through its pidfd",
                        killed.elapsed()
                    ),
                });
            }
            // The init leaves its cgroup at its last switch, after its pidfd reads as ended (Linux v6.19,
            // kernel/sched/core.c:5156), so a cgroup that holds nothing else empties without a kill.
            if !cg.populated()? || (!outsiders(cg, init.pid()) && wait_empty(cg, killed)?.0) {
                self.cgroups.ended_through_init();
                return Ok(false);
            }
            self.cgroups.fell_back();
        } else {
            self.cgroups.killed();
        }
        let killed = Instant::now();
        cg.kill()?;
        // The kill reaches every process in the box; now reap the exec'd programs the agent watches for
        // it, so an `oci` box's init — which the kernel holds in its pid-namespace teardown until the
        // programs in that namespace are reaped — can finish exiting and the box empty.
        self.supervisor.reap_programs(id, empty_patience(killed))?;
        let (emptied, patience) = wait_empty(cg, killed)?;
        if !emptied {
            let op = "emptying a box";
            let mut why = format!("box {id} still holds processes {patience:?} after cgroup.kill:");
            why.push_str(&survivors(cg, why_room(op, &why)));
            return Err(Refusal::Os { op, why });
        }
        Ok(true)
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
    ///
    /// A box whose spec names an owner has its cgroup chosen once the runtime has read its spec
    /// ([`Choice`]): its owner's idle cgroup under the box's device policy when one reads clean, else a
    /// fresh slot, else a cgroup of its own (goal 5 extended, step 6). Its bundle comes first, and the id is
    /// held by the create's turn ([`Node::turns`]).
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
        let cgroup = match &spec.owner {
            // With cgroups made ahead, a box of no owner's is chosen once its spec is read, as an owner's is.
            None if self.cgroups.takes_ahead() => None,
            None => {
                let cg = self.oci_boxes.create(&id, &spec.bounds)?;
                self.cgroups.no_owner();
                #[cfg(debug_assertions)]
                self.crash_point(CrashPoint::CgroupMade);
                Some(cg)
            }
            Some(_) => None,
        };
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
        let choice = (spec.owner.is_some() || self.cgroups.takes_ahead()).then(|| {
            Choice::new(
                &self.cgroups,
                &self.oci_boxes,
                &id,
                spec.owner.as_ref(),
                &spec.bounds,
            )
            .crashing_on_own(self.crashes_at_cgroup_made())
        });
        // A container that makes a network namespace of its own in the agent's user namespace joins one
        // the node made ahead, and the pool counts every container's (decision 4); a box that may share
        // its owner's is given one its owner's earlier box left, or one the node keeps for the owner
        // (goal 5 extended, step 6).
        let networks = self.netns.choice(spec.owner.as_ref());
        let made = self.supervisor.spawning().and_then(|_spawning| {
            // Its device program is one the node holds, or the oci boxes' root's (goal 5 extended,
            // step 1).
            let held = Held {
                networks: Some(&networks),
                devices: Some(&self.devices),
                dev_mounts: Some(&self.devfs),
                ipc_uts: Some(&self.ipc_uts),
            };
            let name = self.container(&id);
            match (cgroup, &choice) {
                (Some(cg), _) => self
                    .oci()
                    .create_in_cgroup_seeded(&name, &bundle, None, cg, held, seeds),
                (None, Some(choice)) => self
                    .oci()
                    .create_chosen_seeded(&name, &bundle, choice, held, seeds),
                (None, None) => Err(OciError::Broken(format!(
                    "box {id}'s create has neither a cgroup nor an owner to choose one"
                ))),
            }
            .map_err(Refusal::from)
        });
        let (chose, refused) = choice.map(Choice::made).unwrap_or((None, None));
        // The namespace the node keeps for the box's owner; closed here if the create failed, never
        // handed on, since a half made box may have left anything in it.
        let network = networks.kept();
        let state = match made {
            Ok(state) => state,
            Err(e) => {
                // The container, if it started, is in the cgroup; scrub ends it and removes the cgroup,
                // the state dir and the bundle, and a slot the create took is ended and removed with it.
                self.unmade(&id, chose);
                return Err(refused.unwrap_or(e));
            }
        };
        // The init is blocked at the exec FIFO, alive, and the agent's child; the state is the one the
        // runtime wrote, so nothing is read back. Record which process the init is so a later restart
        // can tell it from a recycled pid (recovery's init_alive), and watch it through its pidfd from
        // now on. Recorded before it's watched, so the exit the supervisor records goes into a record
        // that names it.
        let mut record = BoxRecord::new(spec, Phase::Ready, dirs);
        record.init = init_proc_id(state.pid, state.start_time);
        if let Some(Made::Slot(held)) = &chose {
            record.slot = Some(Slot {
                n: held.slot,
                baseline: held.baseline,
            });
        }
        let watched = self.store.put(&record).and_then(|()| {
            let start_ticks = state.start_time.ok_or_else(|| {
                Refusal::Broken(format!("container {id}'s state has no start time"))
            })?;
            let container = supervise::Container {
                bundle: state.bundle,
                cgroup: state.cgroup,
                pid_namespace: state.pid_namespace == Some(true),
            };
            // Watched under the spawning lock, so no sweep reaps the init before it's owned; until
            // then a sweep passes it by, alive at the exec FIFO.
            let _spawning = self.supervisor.spawning()?;
            self.supervisor
                .watch(slot, &id, state.pid, start_ticks, container)
        });
        let watched = match (watched, chose) {
            (Ok(()), Some(Made::Slot(held))) => self.cgroups.hold(&id, held).map_err(|e| (e, None)),
            (Ok(()), _) => Ok(()),
            (Err(e), chose) => Err((e, chose)),
        };
        let watched = match (watched, network, &record.spec.owner) {
            (Ok(()), Some(ns), Some(owner)) => {
                self.netns.hold(&id, &owner.name, ns).map_err(|e| (e, None))
            }
            (watched, _, _) => watched,
        };
        if let Err((e, chose)) = watched {
            self.unmade(&id, chose);
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

    /// Takes apart what a failed create of box `id` made: its own cgroup, directories and record through
    /// [`Node::scrub`], and the slot it took (`chose`), ended and removed, never handed on, since a half
    /// made box may have left anything in it.
    fn unmade(&self, id: &BoxId, chose: Option<Made>) {
        let _ = self.scrub(id);
        if let Some(Made::Slot(held)) = chose
            && let Some(cg) = self.oci_boxes.slot(held.slot)
        {
            self.cgroups.remove_later(cg);
        }
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
            dir: Some(self.container_dir(id).to_string_lossy().into_owned()),
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
        let _ = self.seeds.set(SeedPool::new(exe, &self.pools.sizes));
        self.seeds
            .get()
            .ok_or_else(|| Refusal::Broken("the exec seed pool wasn't built".to_owned()))
    }

    /// The create seed pool: built when [`Node::with_oci_helper`] names the `hex` binary, else here on
    /// first use from this process's own (as [`Node::seed_pool`] finds it). A race to build it drops the
    /// losing pool, whose drop stops its starting thread and ends the spare it started.
    fn create_pool(&self) -> Result<&CreatePool, Refusal> {
        if let Some(pool) = self.create_seeds.get() {
            // Its seeds make inits ahead in idle cgroups too (decision 8, "The p99.9 that stayed").
            self.cgroups.start_parker(pool.parker(), &self.oci_boxes)?;
            return Ok(pool);
        }
        let exe = match &self.oci_helper {
            Some(exe) => exe.clone(),
            None => {
                std::env::current_exe().map_err(|e| Refusal::os("finding the hex binary", e))?
            }
        };
        let _ = self.create_seeds.set(CreatePool::new(
            exe,
            self.pools.sizes,
            self.seed_prune(),
            self.mount_namespace.as_deref(),
            #[cfg(debug_assertions)]
            self.inject.clone(),
        )?);
        let pool = self
            .create_seeds
            .get()
            .ok_or_else(|| Refusal::Broken("the create seed pool wasn't built".to_owned()))?;
        self.cgroups.start_parker(pool.parker(), &self.oci_boxes)?;
        Ok(pool)
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

    /// How many claims of a parked init failed and fell back to a seed's clone since the seed pool opened, by why:
    /// (hung, refused or broken). What a pass reads to see how often a parked init cost its create a clone.
    pub fn parked_claims_failed(&self) -> (u64, u64) {
        self.create_seeds
            .get()
            .map_or((0, 0), CreatePool::claims_failed)
    }

    /// How many of the node's creates were made through the shim since its seed pool opened: those a
    /// pruned seed couldn't serve, and those that join a pid or user namespace by path (decision 5, goal 5
    /// plan step 2). A rootless node's other creates take the classic root in a seed, not the shim
    /// ([`ClassicReason::Rootless`]).
    pub fn creates_through_shim(&self) -> u64 {
        self.create_seeds
            .get()
            .map_or(0, CreatePool::creates_through_shim)
    }

    /// How many of the node's creates took the classic root path for `reason` since its seed pool opened
    /// (goal 5 plan step 2, AC-T7.1.4). Counted in the node's memory and read here, as the tests read
    /// it; no `hex_*` metric carries it until kiln's Observe owner agrees the name (CLAUDE.md §10).
    pub fn classic_creates_for(&self, reason: ClassicReason) -> u64 {
        self.create_seeds
            .get()
            .map_or(0, |pool| pool.classic_creates_for(reason))
    }

    /// How many of the node's creates a pruned seed made on the pruned root path (goal 5 plan step 2):
    /// zero on a rootless node, positive on a root node once it has made a pruned-eligible box. The root
    /// lab's tests read it to confirm the pruned path was taken, not a silent shim fallback.
    pub fn pruned_creates(&self) -> u64 {
        self.create_seeds
            .get()
            .map_or(0, CreatePool::pruned_creates)
    }

    /// How many of the node's pruned creates had their mount namespace made `way` since its seed pool
    /// opened (decision 6's update), as the seed answered: a create asked to be made empty on a kernel
    /// without `CLONE_EMPTY_MNTNS` is counted as copied. Kept in memory, as
    /// [`Node::classic_creates_for`]'s counts are.
    pub fn mount_namespaces_made(
        &self,
        way: hex_runtime::linux::oci::mntns::MountNamespace,
    ) -> u64 {
        self.create_seeds
            .get()
            .map_or(0, |pool| pool.mount_namespaces_made(way))
    }
}

impl Runtime for Node {
    fn classes(&self) -> Vec<Class> {
        vec![Class::Process, Class::Oci]
    }

    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        // A spec is checked before anything else, as the simulator checks it: what can't be rendered,
        // and an owner not well formed, is refused by name whether or not the id is taken (HEX-RT-13,
        // HEX-RT-14).
        spec.check()?;
        // Held until the create is done, made or scrubbed: a box being made has no record yet.
        let _turn: Turn<'_> = create_turn(&self.turns, &spec.id)?;
        // A cgroup under either root is a box's, recorded or not: a failed create's scrub would end it.
        if self.record(&spec.id)?.is_some() || self.cgroup(&spec.id).is_some() {
            return Err(Refusal::Exists(spec.id));
        }
        let source = match &spec.class {
            ClassSpec::Process => return self.create_process(spec),
            ClassSpec::Bundle(bundle) => Source::Given(bundle.clone()),
            ClassSpec::Oci(oci) => self.render(&spec.id, oci)?,
        };
        let made = self.create_oci(spec, source);
        self.keep_peaks();
        made
    }

    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        // An oci box's start is tried first, so the hot path reads only the container's state: a box with
        // no container under the oci root is a process box or no box, which the record then says (goal 3 plan
        // step 3: start costs one FIFO operation and the state read beside it). The init's end is watched
        // through the pidfd the supervisor holds for it; one not watched is found from its state.
        let init = self.supervisor.live(id)?.map(|live| live.pidfd);
        let started = self.oci().start_with(&self.container(id), init);
        let record = if matches!(started, Err(OciError::NoSuchContainer(_))) {
            self.record(id)?
        } else {
            None
        };
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
        // KILL goes to the whole box through `cgroup.kill`, after which its cgroup can't be handed on.
        if signal == Signal::KILL {
            self.cgroups.killed_in(id)?;
        }
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
        // A box given a slot is reported what it used since: its counters from the baseline read when it
        // was given the slot, its peak through the descriptor written then (goal 5 extended, step 6).
        match self.cgroups.usage(id, &cg)? {
            Some(usage) => Ok(usage),
            None => Ok(cg.usage()),
        }
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
        // After a create or delete of the box under way, so it removes what that one made, or finds it
        // gone, and scrubs nothing it's doing.
        let _turn: Turn<'_> = self.turns.take(id)?;
        let record = self.record(id)?;
        if record.is_none() && self.cgroup(id).is_none() {
            return Err(Refusal::NoSuchBox(id.clone()));
        }
        // Unwatched first, so the supervisor stops collecting the init; an exit it's recording as it's
        // unwatched goes into this box's record alone, which the scrub removes.
        let init = self.supervisor.forget(id)?;
        // An `oci` box whose init the supervisor reaped already, its exit in the record, has no init to
        // forget: it ended on its own (`true` does). The supervisor writes the exit before it stops
        // watching the init, so with none watched the record is read again: one read before the forget
        // may predate the exit.
        let record = match (&init, record) {
            (None, Some(_)) => self.record(id)?,
            (_, record) => record,
        };
        let ended = record
            .as_ref()
            .is_some_and(|r| r.spec.class.class() == Class::Oci && r.exit.is_some());
        if let Some(mut record) = record {
            record.phase = Phase::Deleting;
            self.store.put(&record)?;
        }
        // The box's slot, if it holds one, goes back to its owner once the box is gone.
        let held = self.cgroups.release(id)?;
        let ending = match &init {
            Some(init) => Ending::Init(init),
            None if ended => Ending::Ended,
            None => Ending::Kill,
        };
        self.scrub_ending(id, &ending, held)?;
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

/// What a refusal named `op` may say past `said`, its reason so far, for the answer carrying it to fit
/// one message of the node's socket: [`wire::MAX_MESSAGE`] less the header ([`wire::HEADER_BYTES`]),
/// the refusal's tag byte and its two strings' `u32` lengths (`hex_core::wire`'s encoding of
/// [`Refusal::Os`]), `op` and `said`. Past it the socket would answer the call's refusal with another,
/// losing what it named (CLAUDE.md §3).
fn why_room(op: &str, said: &str) -> usize {
    // A tag byte and two u32 lengths.
    const TAG_AND_LENGTHS: usize = 9;
    wire::MAX_MESSAGE
        .saturating_sub(wire::HEADER_BYTES)
        .saturating_sub(TAG_AND_LENGTHS)
        .saturating_sub(op.len())
        .saturating_sub(said.len())
}

/// What a box still holds when a delete can't empty it: each surviving process with its command,
/// state and parent, so the refusal names what is stuck, not only that something is (§3). As many as
/// fit in `room` bytes ([`why_room`]), so the refusal reaches its caller whole. A descendant cgroup's are
/// named under its path. `cgroup.procs` leaves out a process whose leader has ended while another of its
/// threads hasn't (Linux v6.19, `kernel/cgroup/cgroup.c:5112-5121`), so with no process listed the box's
/// threads are named instead, each with its process, state and where it waits.
fn survivors(cg: &hex_runtime::linux::cgroup::BoxCgroup, room: usize) -> String {
    const NO_PROCESS: &str = " (no process listed, only threads:)";
    let root = cg.dir().to_path_buf();
    let (mut out, mut threads) = (String::new(), String::new());
    let thread_room = room.saturating_sub(NO_PROCESS.len());
    // Where the count can't be read, the box's own cgroup alone, which is always there.
    let walk = cg.subtree_budget().unwrap_or(1);
    let mut queue = vec![root.clone()];
    let mut dirs_seen = 0usize;
    // A flat walk of the box's cgroup subtree (CLAUDE.md §11), bounded in cgroups as the kill sweep is
    // (its `subtree_budget`, from its `cgroup.stat`, past which a delete is refused anyway) and in what the text holds
    // (§2).
    while let Some(dir) = queue.pop() {
        if dirs_seen >= walk || (out.len() >= room && threads.len() >= thread_room) {
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
            for pid in text.lines().filter_map(|l| l.trim().parse::<u32>().ok()) {
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
                let named = format!(" [{rel}/{pid} {comm:?} state {state} ppid {ppid}]");
                if out.len().saturating_add(named.len()) > room {
                    break;
                }
                out.push_str(&named);
            }
        }
        if let Ok(text) = fs::read_to_string(dir.join("cgroup.threads")) {
            for tid in text.lines().filter_map(|l| l.trim().parse::<u32>().ok()) {
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
                let named = format!(
                    " [{rel}/{tid} of {} {:?} state {} wchan {wchan}]",
                    field("Tgid:"),
                    field("Name:"),
                    field("State:")
                );
                if threads.len().saturating_add(named.len()) > thread_room {
                    break;
                }
                threads.push_str(&named);
            }
        }
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if dirs_seen.saturating_add(queue.len()) >= walk {
                    break;
                }
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

/// How a delete or a scrub ends a box's processes ([`Node::end`]).
enum Ending<'a> {
    /// Through the box's watched init, its pidfd's SIGKILL when the init made a pid namespace of its own;
    /// `cgroup.kill` otherwise.
    Init(&'a Forgotten),
    /// The box's init ended and was reaped before the delete, its exit in the box's record: nothing to
    /// kill when the cgroup holds nothing more, else `cgroup.kill`.
    Ended,
    /// `cgroup.kill`.
    Kill,
}

/// A create's turn for box `id` ([`Node::turns`]): past the turns' bound, [`MAX_BOXES`] creates and deletes at once,
/// the node's box budget, the create is refused as [`Refusal::Full`], as the node refuses a box past its budget
/// (CLAUDE.md §2; the bound step 6's claim held creates to). A wait for a create or delete of the same box is refused
/// past its learned patience as [`Turns::take`] refuses it.
fn create_turn<'a>(turns: &'a Turns, id: &BoxId) -> Result<Turn<'a>, Refusal> {
    turns.take(id).map_err(|e| match e {
        Refusal::Exhausted { bound, .. } => Refusal::Full(bound),
        e => e,
    })
}

/// The slots under the oci boxes' root, by number, lowest first ([`reuse::slot_of`]).
fn slots_in(root: &BoxRoot) -> Result<Vec<u64>, Refusal> {
    let mut slots: Vec<u64> = root
        .boxes()?
        .iter()
        .filter_map(|name| reuse::slot_of(name))
        .collect();
    slots.sort_unstable();
    Ok(slots)
}

/// The threads there may be on the host: the kernel's `pid_max` (Documentation/admin-guide/sysctl/kernel.rst,
/// "pid_max"), read at runtime, since every thread holds a pid. [`outsiders`] reads at most this many, so its
/// walk ends by what the box holds, never by a picked count; `None` where it can't be read, and then the walk
/// takes the box for holding outsiders, ending it by `cgroup.kill` as before.
fn pid_max() -> Option<usize> {
    static PID_MAX: OnceLock<Option<usize>> = OnceLock::new();
    *PID_MAX.get_or_init(|| {
        fs::read_to_string("/proc/sys/kernel/pid_max")
            .ok()
            .and_then(|s| s.trim().parse().ok())
    })
}

/// Whether `cg`'s subtree holds a thread of a live process other than `init`, whose own threads may
/// stay listed until each is gone: once a pid namespace's init has ended, the namespace's processes
/// have all been released (Linux v6.19, kernel/pid_namespace.c:263-265), so such a thread is outside it
/// (research f47). A listed thread whose process is gone from /proc is on its way out. A flat walk,
/// bounded in cgroups and threads (CLAUDE.md §11, §2): in cgroups as the kill sweep is (hex-runtime's
/// `subtree_budget`, from its `cgroup.stat`, past which a delete is refused anyway), in threads by the pids the host has
/// ([`pid_max`]). Past either it says there are, so the box is ended by `cgroup.kill` as before.
fn outsiders(cg: &BoxCgroup, init: i32) -> bool {
    let Some(most_threads) = pid_max() else {
        return true;
    };
    let Ok(walk) = cg.subtree_budget() else {
        return true;
    };
    let mut queue = vec![cg.dir().to_path_buf()];
    let (mut dirs, mut threads) = (0usize, 0usize);
    while let Some(dir) = queue.pop() {
        dirs = dirs.saturating_add(1);
        if dirs > walk {
            return true;
        }
        let Ok(text) = fs::read_to_string(dir.join("cgroup.threads")) else {
            continue;
        };
        for tid in text.lines().filter_map(|l| l.trim().parse::<u32>().ok()) {
            threads = threads.saturating_add(1);
            if threads > most_threads {
                return true;
            }
            let tgid = fs::read_to_string(format!("/proc/{tid}/status"))
                .ok()
                .and_then(|status| {
                    status
                        .lines()
                        .find_map(|l| l.strip_prefix("Tgid:"))
                        .and_then(|v| v.trim().parse::<i32>().ok())
                });
            if tgid.is_some_and(|tgid| tgid != init) {
                return true;
            }
        }
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    queue.push(entry.path());
                }
            }
        }
    }
    false
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
        Why::StartInterrupted => {
            "the node agent stopped while it started the box, in its startContainer hooks"
                .to_owned()
        }
    }
}

/// The box ids named by the directories in `dir` whose names start with `prefix`, the rest of each name
/// the id, or none when `dir` isn't there: a box's parts there are directories, so any other entry is no
/// box's. The listing ends at the wait learned from the listings before it, for as many entries as it has
/// listed ([`LISTING`]), refused past it by type.
fn ids_in(dir: &Path, prefix: &str) -> Result<BTreeSet<BoxId>, Refusal> {
    let listing = |e| Refusal::os("listing a directory for boxes' parts", e);
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(e) => return Err(listing(e)),
    };
    let per_entry = LISTING.deadline();
    let began = Instant::now();
    let mut listed = 0u32;
    let mut ids = BTreeSet::new();
    for entry in entries {
        listed = listed.saturating_add(1);
        if let Some(per_entry) = per_entry
            && began.elapsed() > per_entry.saturating_mul(listed)
        {
            return Err(Refusal::Os {
                op: "listing a directory for boxes' parts",
                why: format!(
                    "{} took {:?} for {listed} entries, past twice the slowest a listing has taken an entry",
                    dir.display(),
                    began.elapsed()
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
    LISTING.answered(
        began
            .elapsed()
            .checked_div(listed.max(1))
            .unwrap_or_default(),
    );
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
/// among its containers' state, `<key>.records`, a name no container's takes (theirs are `<key>-<id>`),
/// each record in one of its shards ([`crate::store::runtime_record`]).
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
    Ok(format!(
        "{:016x}",
        crate::store::fnv1a(path.as_os_str().as_bytes())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CLAUDE.md §3 (Ada's 2026-11-06 rule): what a refusal names of a box's survivors is bounded by the wire,
    /// derived, not picked. Do: fill a delete's refusal to its room past what it says first, then one byte past
    /// it. Expect: the first answer is one message of the node's socket; the second is refused as oversized.
    #[test]
    fn a_refusal_filled_to_its_room_fits_one_message() {
        let op = "emptying a box";
        let said = "box b still holds processes 1s after cgroup.kill:";
        let room = why_room(op, said);
        let fits = |n: usize| {
            let why = format!("{said}{}", "x".repeat(n));
            let refused = wire::Answer::Refused(Refusal::Os { op, why });
            wire::answer(wire::FIRST, u64::MAX, &refused, wire::MAX_MESSAGE).is_ok()
        };
        assert!(fits(room), "a refusal filled to its room of {room} fits");
        assert!(!fits(room + 1), "one byte past its room doesn't");
    }

    /// Decision 9 (Ada's 2026-11-06 rule, the lead's failure-bound rule; T-T7.1.2): a recovery's listing teaches
    /// the next its wait. Do: list a directory holding two boxes' parts and a file. Expect: the two ids, and the
    /// listings' slowest time per entry recorded, so the next listing's bound is armed (this one, or one before,
    /// armed it).
    #[test]
    fn a_listing_teaches_the_next_its_wait() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("hex-a")).unwrap();
        fs::create_dir(dir.path().join("hex-b")).unwrap();
        fs::write(dir.path().join("hex-c"), b"").unwrap();
        let ids = ids_in(dir.path(), "hex-").unwrap();
        assert_eq!(ids.len(), 2, "{ids:?}");
        assert!(LISTING.slowest().is_some());
    }

    /// CLAUDE.md §2 (T-T7.1.2): creates under way are bounded, and past the bound a create is refused as `Full`,
    /// never queued. Do: turns bounded at two, two boxes' turns held, a third box's create turn asked. Expect:
    /// `Full(2)`; with one let go, the third box's turn given.
    #[test]
    fn a_create_past_the_turns_bound_is_refused_full() {
        let turns = Turns::new(
            "making or removing a box",
            "creates and deletes under way",
            2,
            Patience::measuring("node.turns"),
        );
        let id = |n: &str| BoxId::new(n).unwrap();
        let a = create_turn(&turns, &id("a")).unwrap();
        let _b = create_turn(&turns, &id("b")).unwrap();
        assert_eq!(create_turn(&turns, &id("c")).err(), Some(Refusal::Full(2)));
        drop(a);
        assert!(
            create_turn(&turns, &id("c")).is_ok(),
            "a turn let go is free again"
        );
    }

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
        assert_eq!(crate::store::fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(crate::store::fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
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

impl Drop for Node {
    /// What the pools learned is kept for the node's next start, with this start's takes (the lead, 2026-11-06): a
    /// node that can't write it starts the next time from the last it kept.
    fn drop(&mut self) {
        let now = self.peaks_now();
        let _ = self.keeper.finish(&now);
    }
}
