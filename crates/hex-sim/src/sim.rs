//! The simulated cluster: one event loop over virtual time, every participant a state machine.
//!
//! - **Clients** submit each workload when it arrives and resubmit it, under the same id, until the
//!   control plane acknowledges it or refuses it; a resubmit of a stored workload is a duplicate.
//! - **The control plane** is `shards` shards (H10), each holding a slice of the nodes (contiguous
//!   ids) and the workloads routed to it ([`Layout`]); one shard holds the whole cluster. A slice's
//!   plane admits workloads (refusing one no empty node could hold), decides
//!   placements on `workers` parallel deciders against its current view, and commits them through one
//!   committer in queue order. A decision is late-bound (S2): at commit it is checked against every
//!   node change since it was made and, if one could change the answer, made again against the current
//!   view (a conflict, counted). So the committed log is one the serial specification could make
//!   ([`crate::spec::check_log`]), while deciding runs in parallel.
//! - **Node agents** start a workload when the store's feed shows it placed on them, and report it
//!   done when it ends, resending until the control plane records it.
//! - **The nemesis** crashes the control plane at store writes, before one is applied or after it
//!   is applied and before anyone hears of it (DS[1][5]): at one chosen write ([`Nemesis::At`]), or at
//!   writes its own seed picks ([`Nemesis::Random`], `AC-T7.3.3`'s crash seeds). Everything the
//!   control plane holds in memory is lost; it restarts after `restart_ns` and rebuilds its view from
//!   the store. Node agents and the work they run go on through it (`AC-T7.3.4`), as Borg's tasks do
//!   when the Borgmaster is down (Verma et al., EuroSys 2015, p. 6). A crash takes down the shard
//!   that made the write, with every slice it holds.
//! - **A lost shard** ([`Nemesis::Lose`], H11) never comes back. Its lease on each slice runs out
//!   `lease_ns` after its last renewal; then the next live shard takes the slice over and rebuilds its
//!   plane from the store, at `rebuild_ns_per_record` a record read (`T-T7.11.1`). The slice's
//!   running work goes on through it, as through a crash.
//!
//! Every queue and map is bounded by the run's workloads or by a [`Config`] field, and the run by
//! `max_steps` (CLAUDE.md §2). Nothing in it calls itself (§11): each event is handled by a function
//! that schedules further events rather than calling their handlers.

use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};
use std::ops::Bound;

use crate::error::SimError;
use crate::index::{FitIndex, Key, Refusals, Sample, Sampled};
use crate::model::{Cluster, Demand, NodeId, NodeSpec, Time, WorkloadId, WorkloadSpec};
use crate::rng::Rng;
use crate::store::{Outcome, State, Store, Write};
use crate::trace::{Latencies, TraceHash};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    /// The write is never applied.
    Before,
    /// The write is applied; its writer never learns it was.
    After,
}

/// Crash the control plane at its `write`th store write (counted from 0, over every write the
/// control plane attempts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crash {
    pub write: u64,
    pub when: When,
}

/// Where the nemesis crashes the control plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nemesis {
    None,
    At(Crash),
    /// A crash at write `first`, then one at each later write with probability `per_mille / 1000`,
    /// at most `max_crashes` in all, each before or after its write by a fair draw; every draw from
    /// `seed`'s own generator, so the faults are a function of the seed (DS[1][4]).
    Random {
        seed: u64,
        first: u64,
        per_mille: u64,
        max_crashes: u32,
    },
    /// Shard `shard` is lost for good at virtual time `at`.
    Lose {
        shard: u32,
        at: Time,
    },
}

/// A takeover's rebuild, per record read. Provisional, not yet measured: `tests/shards.rs`'s
/// `rebuild_cost` measures it on fedora.
pub const REBUILD_NS_PER_RECORD: Time = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Parallel deciders.
    pub workers: u32,
    /// Virtual time one decision takes.
    pub decide_ns: Time,
    /// Virtual time from a commit being sent to the store applying it.
    pub commit_ns: Time,
    /// From the store applying a placement to its node agent seeing it.
    pub feed_ns: Time,
    pub restart_ns: Time,
    pub client_retry_ns: Time,
    pub client_attempts: u32,
    pub report_retry_ns: Time,
    pub report_attempts: u32,
    /// Node changes kept for late binding; a decision older than the window is made again.
    pub change_window: usize,
    pub max_events: usize,
    pub max_steps: u64,
    pub nemesis: Nemesis,
    /// How deciders sample nodes; the serial specification samples the same way.
    pub sample: Sample,
    /// Shards of the control plane, each first holding one slice of the nodes (H10).
    pub shards: u32,
    /// How long a shard's lease on its slices outlives its last renewal, and how often it renews.
    pub lease_ns: Time,
    pub renew_ns: Time,
    /// Virtual time a takeover's rebuild takes for each record it reads.
    pub rebuild_ns_per_record: Time,
}

impl Default for Config {
    /// Costs in virtual nanoseconds, chosen for test runs, not measured: implementation goal 2 sets
    /// `decide_ns` and `commit_ns` from the measured fast path.
    fn default() -> Self {
        Self {
            workers: 4,
            decide_ns: 50_000,
            commit_ns: 20_000,
            feed_ns: 10_000,
            restart_ns: 5_000_000,
            client_retry_ns: 10_000_000,
            client_attempts: 16,
            report_retry_ns: 10_000_000,
            report_attempts: 16,
            change_window: 4_096,
            max_events: 1 << 22,
            max_steps: 1 << 32,
            nemesis: Nemesis::None,
            sample: Sample::default(),
            shards: 1,
            // Cited: etcd's defaults, an election timeout of 1,000 ms and a heartbeat of 100 ms
            // (etcd tuning.md:14,16,20, docs/v3.5).
            lease_ns: 1_000_000_000,
            renew_ns: 100_000_000,
            rebuild_ns_per_record: REBUILD_NS_PER_RECORD,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Store writes the control plane attempted, including one the nemesis cut off.
    pub writes: u64,
    pub crashes: u64,
    /// Of those, crashes before their write was applied, and after.
    pub crashes_before: u64,
    pub crashes_after: u64,
    /// Decisions made again at commit because a node change since could have changed them.
    pub conflicts: u64,
    /// Of those, decisions older than the change window.
    pub stale: u64,
    pub placements: u64,
    /// Sampled decisions made, by deciders and again at commit.
    pub decisions: u64,
    /// Over those decisions' scans, nodes with room that a filter refused, by filter.
    pub refused: Refusals,
    /// Workloads found holding on no node, each marking its equivalence class so until capacity
    /// comes back (at most one per class per such stretch).
    pub class_checks: u64,
    /// Workloads passed over because their class was marked so, undecided.
    pub class_skips: u64,
    /// Summed over crashes, the workloads running on nodes when the control plane went down.
    pub running_through_crashes: u64,
    pub steps: u64,
    pub end: Time,
    /// Submit to placement, per placement.
    pub latencies: Latencies,
    pub trace_hash: u64,
    /// Each slice taken over from a lost shard (`T-T7.11.1`).
    pub takeovers: Vec<Takeover>,
}

/// A lost shard's slice, taken over by a live one (H11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Takeover {
    pub slice: usize,
    /// The shard lost, and the shard that took the slice over.
    pub lost: u32,
    pub by: u32,
    pub lost_at: Time,
    /// From the loss to its lease running out: at most `lease_ns` after its last renewal.
    pub detection_ns: Time,
    /// Records the rebuild read (the slice's workloads not yet done), and the time it took.
    pub records: u64,
    pub rebuild_ns: Time,
    /// Workloads running on the slice's nodes when its shard was lost.
    pub running: u64,
    /// From the loss to the slice's first commit under its new shard; `None` if it never made one.
    pub takeover_ns: Option<Time>,
    /// Store writes the slice applied under its new shard.
    pub committed_after: u64,
}

/// A finished run: its report, the store as it ended, and what the clients and node agents saw.
#[derive(Debug, Clone)]
pub struct Run {
    pub report: Report,
    pub store: Store,
    /// Starts per workload, by id.
    pub starts: Vec<u32>,
    /// Whether each workload's client was told its submit was stored.
    pub acked: Vec<bool>,
    /// Whether each workload's client was told it could never be placed.
    pub refused: Vec<bool>,
    /// Where and when each workload's first start ran, by id.
    pub ran: Vec<Option<Ran>>,
    /// The sampling the run's deciders used, for its serial specification.
    pub sample: Sample,
    /// What the control plane held in memory as the run ended, one per slice.
    pub planes: Vec<ControlPlane>,
}

/// A slice's control plane as the run left it: what it holds in memory, kept so its cost can be
/// measured (`AC-T7.11.4`).
#[derive(Debug, Clone)]
pub struct ControlPlane(Plane);

/// One run of a workload on a node, as its node agent saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ran {
    pub node: NodeId,
    pub started: Time,
    /// When the node agent saw it end; `None` while it runs.
    pub ended: Option<Time>,
}

/// A decision: the best-fit key, or none, against the view after change `snap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Decision {
    sampled: Sampled,
    snap: u64,
}

impl Decision {
    fn node(&self) -> Option<Key> {
        self.sampled.best
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Event {
    Arrive {
        at: usize,
        attempt: u32,
    },
    ClientCheck {
        at: usize,
        attempt: u32,
    },
    Proposal {
        slice: usize,
        epoch: u64,
        seq: u64,
        decision: Decision,
    },
    CommitDue {
        slice: usize,
        epoch: u64,
        seq: u64,
        decision: Decision,
        resets: u64,
    },
    NodeStart {
        at: usize,
        node: NodeId,
    },
    NodeDone {
        at: usize,
        node: NodeId,
        attempt: u32,
    },
    Restart {
        shard: u32,
    },
    Lose {
        shard: u32,
    },
    Takeover {
        slice: usize,
    },
    Rebuilt {
        slice: usize,
        epoch: u64,
    },
}

impl Event {
    /// The words the trace hash takes for this event.
    fn words(&self) -> [u64; 3] {
        let at = |a: usize| u64::try_from(a).unwrap_or(u64::MAX);
        match *self {
            Event::Arrive { at: a, attempt } => [1, at(a), u64::from(attempt)],
            Event::ClientCheck { at: a, attempt } => [2, at(a), u64::from(attempt)],
            Event::Proposal { seq, decision, .. } => [
                3,
                seq,
                decision.node().map_or(u64::MAX, |k| u64::from(k.node.0)),
            ],
            Event::CommitDue { seq, decision, .. } => [
                4,
                seq,
                decision.node().map_or(u64::MAX, |k| u64::from(k.node.0)),
            ],
            Event::NodeStart { at: a, node } => [5, at(a), u64::from(node.0)],
            Event::NodeDone { at: a, node, .. } => [6, at(a), u64::from(node.0)],
            Event::Restart { shard } => [7, u64::from(shard), 0],
            Event::Lose { shard } => [8, u64::from(shard), 0],
            Event::Takeover { slice } => [9, at(slice), 0],
            Event::Rebuilt { slice, epoch } => [10, at(slice), epoch],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Scheduled {
    at: Time,
    order: u64,
    event: Event,
}

impl Ord for Scheduled {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.at, self.order).cmp(&(other.at, other.order))
    }
}

impl PartialOrd for Scheduled {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// How the cluster's nodes are split among the control plane's slices (H10): contiguous runs of node
/// ids, one a shard, and the rule that routes a workload to one. Nothing in it changes during a run,
/// so every shard, and the specification, computes the same slice for a workload.
#[derive(Debug, Clone)]
pub struct Layout {
    parts: Vec<Part>,
}

#[derive(Debug, Clone)]
struct Part {
    /// The slice's first node id in the cluster: a node's id in the slice plus `lo` is its id there.
    lo: u32,
    /// The slice's nodes, their ids dense from 0 within it.
    nodes: Vec<NodeSpec>,
    /// Its nodes empty, for admission.
    empty: FitIndex,
}

impl Layout {
    /// `shards` slices of `nodes`, slice `s` the ids from `s * n / shards` up to `(s + 1) * n / shards`.
    pub fn new(nodes: &[NodeSpec], shards: u32) -> Result<Self, SimError> {
        if shards == 0 {
            return Err(SimError::Config("the control plane has at least one shard"));
        }
        let n = u64::try_from(nodes.len()).map_err(|_| SimError::Config("too many nodes"))?;
        if shards > 1 && u64::from(shards) > n {
            return Err(SimError::Config(
                "every shard's slice has at least one node",
            ));
        }
        let bound = |s: u32| -> Result<usize, SimError> {
            n.checked_mul(u64::from(s))
                .and_then(|x| x.checked_div(u64::from(shards)))
                .and_then(|x| usize::try_from(x).ok())
                .ok_or(SimError::Config("too many nodes"))
        };
        let mut parts = Vec::with_capacity(usize::try_from(shards).unwrap_or(0));
        for s in 0..shards {
            let (from, to) = (
                bound(s)?,
                bound(s.checked_add(1).ok_or(SimError::Config("shards"))?)?,
            );
            let lo = u32::try_from(from).map_err(|_| SimError::Config("too many nodes"))?;
            let mut local = nodes
                .get(from..to)
                .ok_or(SimError::Config("node index"))?
                .to_vec();
            for (at, node) in local.iter_mut().enumerate() {
                node.id =
                    NodeId(u32::try_from(at).map_err(|_| SimError::Config("too many nodes"))?);
            }
            parts.push(Part {
                lo,
                empty: FitIndex::new(&local)?,
                nodes: local,
            });
        }
        Ok(Self { parts })
    }

    pub fn len(&self) -> usize {
        self.parts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// Slice `s`'s nodes, by their ids within it.
    pub fn nodes(&self, s: usize) -> Option<&[NodeSpec]> {
        self.parts.get(s).map(|p| p.nodes.as_slice())
    }

    /// The slice a workload goes to: the first, from its id modulo the slices on, whose empty nodes
    /// admit it; `None` when no slice's would, and it is refused.
    pub fn route(&self, spec: &WorkloadSpec) -> Option<usize> {
        let n = u64::try_from(self.parts.len()).ok()?;
        let first = spec.id.0.checked_rem(n)?;
        (0..n)
            .filter_map(|k| first.checked_add(k)?.checked_rem(n))
            .filter_map(|s| usize::try_from(s).ok())
            .find(|&s| {
                self.parts
                    .get(s)
                    .is_some_and(|p| p.empty.admitted_anywhere(&spec.demand()))
            })
    }

    /// The slice a workload goes to when it is refused: the one its client reaches first.
    fn first(&self, spec: &WorkloadSpec) -> Option<usize> {
        let n = u64::try_from(self.parts.len()).ok()?;
        usize::try_from(spec.id.0.checked_rem(n)?).ok()
    }

    /// The slice holding a cluster node.
    pub fn slice_of(&self, node: NodeId) -> Option<usize> {
        self.parts
            .partition_point(|p| p.lo <= node.0)
            .checked_sub(1)
            .filter(|&s| self.local(s, node).is_some())
    }

    /// A cluster node's id within slice `s`, if it is one of the slice's.
    pub fn local(&self, s: usize, node: NodeId) -> Option<NodeId> {
        let p = self.parts.get(s)?;
        let at = node.0.checked_sub(p.lo)?;
        if usize::try_from(at).ok()? < p.nodes.len() {
            Some(NodeId(at))
        } else {
            None
        }
    }

    /// Slice `s`'s node `local`, by its id in the cluster.
    fn global(&self, s: usize, local: NodeId) -> Result<NodeId, SimError> {
        self.parts
            .get(s)
            .and_then(|p| p.lo.checked_add(local.0))
            .map(NodeId)
            .ok_or(SimError::UnknownNode(local))
    }

    fn part(&self, s: usize) -> Result<&Part, SimError> {
        self.parts.get(s).ok_or(SimError::Config("slice index"))
    }
}

/// What a slice's control plane holds in memory, lost when its shard crashes or is lost. Its node
/// ids are the slice's own ([`Layout`]).
#[derive(Debug, Clone)]
struct Plane {
    up: bool,
    epoch: u64,
    view: FitIndex,
    /// Pending workloads by queue position.
    pending: BTreeMap<u64, WorkloadId>,
    /// The record version the plane last wrote or read, for each workload not yet done.
    versions: BTreeMap<WorkloadId, u64>,
    undecided: BTreeSet<u64>,
    deciding: BTreeSet<u64>,
    proposals: BTreeMap<u64, Decision>,
    idle_workers: u32,
    /// Every pending workload at or before this queue position holds on no node now.
    cursor: Option<u64>,
    /// Capacity given back, which voids `cursor`'s claim; a commit sent before one is sent again.
    resets: u64,
    /// Node changes `(number, new key)`, the last `change_window` of them.
    changes: VecDeque<(u64, Key)>,
    change_no: u64,
    /// Changes up to this number have left the window.
    forgotten: u64,
    committing: bool,
    /// Equivalence classes (needs) no node holds, since capacity last came back.
    infeasible: BTreeSet<Demand>,
}

impl Plane {
    /// Slice `s`'s plane from the store, and the records it read: the slice's workloads not yet done
    /// (pending ones routed to it, placed ones on its nodes).
    fn rebuild(
        store: &Store,
        layout: &Layout,
        s: usize,
        epoch: u64,
        workers: u32,
    ) -> Result<(Self, u64), SimError> {
        let mut view = FitIndex::new(&layout.part(s)?.nodes)?;
        let mut pending = BTreeMap::new();
        let mut versions = BTreeMap::new();
        let mut records = 0u64;
        for record in store.records() {
            match record.state {
                State::Pending => {
                    if layout.route(&record.spec) != Some(s) {
                        continue;
                    }
                    pending.insert(record.seq, record.spec.id);
                    versions.insert(record.spec.id, record.version);
                }
                State::Placed(node) => {
                    let Some(local) = layout.local(s, node) else {
                        continue;
                    };
                    view.take(local, &record.spec.demand())?;
                    versions.insert(record.spec.id, record.version);
                }
                State::Complete(_) => continue,
            }
            records = records
                .checked_add(1)
                .ok_or(SimError::Config("too many records"))?;
        }
        let plane = Self {
            up: true,
            epoch,
            view,
            undecided: pending.keys().copied().collect(),
            pending,
            versions,
            deciding: BTreeSet::new(),
            proposals: BTreeMap::new(),
            idle_workers: workers,
            cursor: None,
            resets: 0,
            changes: VecDeque::new(),
            change_no: 0,
            forgotten: 0,
            committing: false,
            infeasible: BTreeSet::new(),
        };
        Ok((plane, records))
    }
}

impl ControlPlane {
    /// Slice `s`'s plane rebuilt from `store`, as a takeover rebuilds it, and the records it read:
    /// what `Config::rebuild_ns_per_record` is measured on.
    pub fn rebuild(
        store: &Store,
        layout: &Layout,
        s: usize,
        workers: u32,
    ) -> Result<(Self, u64), SimError> {
        let (plane, records) = Plane::rebuild(store, layout, s, 0, workers)?;
        Ok((Self(plane), records))
    }

    /// Workloads its queue holds, pending.
    pub fn pending(&self) -> usize {
        self.0.pending.len()
    }

    /// Workloads it holds a record version for: its slice's workloads not yet done.
    pub fn workloads(&self) -> usize {
        self.0.versions.len()
    }
}

/// A slice: its shard, its plane, and what runs on its nodes.
#[derive(Debug, Clone)]
struct Slice {
    owner: u32,
    plane: Plane,
    /// Workloads running on its nodes now.
    running: u64,
    /// Its entry in [`Report::takeovers`] once it has been taken over.
    takeover: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
struct Shard {
    up: bool,
    /// Lost for good: it never restarts, and its slices go to another shard.
    lost: bool,
}

fn slot(slices: &mut [Slice], s: usize) -> Result<&mut Slice, SimError> {
    slices.get_mut(s).ok_or(SimError::Config("slice index"))
}

struct Sim<'a> {
    cluster: &'a Cluster,
    config: Config,
    now: Time,
    queue: BinaryHeap<Reverse<Scheduled>>,
    order: u64,
    store: Store,
    layout: Layout,
    slices: Vec<Slice>,
    shards: Vec<Shard>,
    starts: Vec<u32>,
    acked: Vec<bool>,
    refused: Vec<bool>,
    ran: Vec<Option<Ran>>,
    nemesis_rng: Rng,
    latencies: Vec<Time>,
    hash: TraceHash,
    report: Report,
}

pub fn run(cluster: &Cluster, config: &Config) -> Result<Run, SimError> {
    if config.workers == 0 {
        return Err(SimError::Config(
            "the control plane has at least one decider",
        ));
    }
    if config.change_window == 0 {
        return Err(SimError::Config(
            "the change window holds at least one change",
        ));
    }
    if config.renew_ns == 0 || config.renew_ns > config.lease_ns {
        return Err(SimError::Config(
            "a shard renews its lease, within the lease's length",
        ));
    }
    if let Nemesis::Lose { shard, .. } = config.nemesis
        && (config.shards < 2 || shard >= config.shards)
    {
        return Err(SimError::Config(
            "a shard lost is one of two or more, so another can take its slice over",
        ));
    }
    for (at, w) in cluster.workloads.iter().enumerate() {
        if usize::try_from(w.id.0).ok() != Some(at) {
            return Err(SimError::Config(
                "workload ids must be dense from 0, in order",
            ));
        }
    }
    if cluster
        .workloads
        .windows(2)
        .any(|p| p.first().map(|a| a.arrives) > p.get(1).map(|b| b.arrives))
    {
        return Err(SimError::Config("workloads must be in arrival order"));
    }
    let n = cluster.workloads.len();
    // The log holds a submit, a placement and a completion per workload.
    let max_log = n
        .checked_mul(3)
        .ok_or(SimError::Config("too many workloads"))?;
    let store = Store::new(&cluster.nodes, n, max_log)?;
    let layout = Layout::new(&cluster.nodes, config.shards)?;
    let mut slices = Vec::with_capacity(layout.len());
    for s in 0..layout.len() {
        let (plane, _) = Plane::rebuild(&store, &layout, s, 0, config.workers)?;
        slices.push(Slice {
            owner: u32::try_from(s).map_err(|_| SimError::Config("shards"))?,
            plane,
            running: 0,
            takeover: None,
        });
    }
    let mut sim = Sim {
        cluster,
        config: *config,
        now: 0,
        queue: BinaryHeap::new(),
        order: 0,
        store,
        shards: vec![
            Shard {
                up: true,
                lost: false
            };
            layout.len()
        ],
        layout,
        slices,
        starts: vec![0; n],
        acked: vec![false; n],
        refused: vec![false; n],
        ran: vec![None; n],
        nemesis_rng: Rng::new(match config.nemesis {
            Nemesis::Random { seed, .. } => seed,
            Nemesis::None | Nemesis::At(_) | Nemesis::Lose { .. } => 0,
        }),
        latencies: Vec::with_capacity(n),
        hash: TraceHash::default(),
        report: Report::default(),
    };
    if let Some(first) = cluster.workloads.first() {
        sim.schedule(first.arrives, Event::Arrive { at: 0, attempt: 0 })?;
    }
    if let Nemesis::Lose { shard, at } = config.nemesis {
        sim.schedule(at, Event::Lose { shard })?;
    }
    while let Some(Reverse(next)) = sim.queue.pop() {
        sim.report.steps = sim
            .report
            .steps
            .checked_add(1)
            .ok_or(SimError::StepBudget(sim.config.max_steps))?;
        if sim.report.steps > sim.config.max_steps {
            return Err(SimError::StepBudget(sim.config.max_steps));
        }
        sim.now = next.at;
        sim.hash.word(next.at);
        for word in next.event.words() {
            sim.hash.word(word);
        }
        sim.handle(next.event)?;
    }
    for entry in sim.store.log() {
        sim.hash.word(entry.at);
    }
    sim.report.end = sim.now;
    sim.report.trace_hash = sim.hash.finish();
    sim.report.latencies = Latencies::new(sim.latencies);
    Ok(Run {
        report: sim.report,
        store: sim.store,
        starts: sim.starts,
        acked: sim.acked,
        refused: sim.refused,
        ran: sim.ran,
        sample: sim.config.sample,
        planes: sim
            .slices
            .into_iter()
            .map(|s| ControlPlane(s.plane))
            .collect(),
    })
}

/// A count past `u64::MAX`, which a run's budget keeps far off.
fn tally(n: &mut u64, by: u64, max_steps: u64) -> Result<(), SimError> {
    *n = n.checked_add(by).ok_or(SimError::StepBudget(max_steps))?;
    Ok(())
}

impl Sim<'_> {
    fn schedule(&mut self, at: Time, event: Event) -> Result<(), SimError> {
        if self.queue.len() >= self.config.max_events {
            return Err(SimError::Full {
                what: "the event queue",
                bound: self.config.max_events,
            });
        }
        self.order = self
            .order
            .checked_add(1)
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
        self.queue.push(Reverse(Scheduled {
            at,
            order: self.order,
            event,
        }));
        Ok(())
    }

    fn after(&self, delay: Time) -> Result<Time, SimError> {
        self.now.checked_add(delay).ok_or(SimError::TimeOverflow)
    }

    fn workload(&self, at: usize) -> Result<&WorkloadSpec, SimError> {
        self.cluster
            .workloads
            .get(at)
            .ok_or(SimError::Config("workload index"))
    }

    fn demand(&self, id: WorkloadId) -> Result<Demand, SimError> {
        self.store
            .record(id)
            .map(|r| r.spec.demand())
            .ok_or(SimError::UnknownWorkload(id))
    }

    fn shard(&mut self, shard: u32) -> Result<&mut Shard, SimError> {
        usize::try_from(shard)
            .ok()
            .and_then(|at| self.shards.get_mut(at))
            .ok_or(SimError::Config("shard index"))
    }

    /// The slices `shard` holds now, in order.
    fn owned(&self, shard: u32) -> Vec<usize> {
        (0..self.slices.len())
            .filter(|&s| self.slices.get(s).is_some_and(|x| x.owner == shard))
            .collect()
    }

    fn handle(&mut self, event: Event) -> Result<(), SimError> {
        match event {
            Event::Arrive { at, attempt } => self.arrive(at, attempt),
            Event::ClientCheck { at, attempt } => self.client_check(at, attempt),
            Event::Proposal {
                slice,
                epoch,
                seq,
                decision,
            } => self.proposal(slice, epoch, seq, decision),
            Event::CommitDue {
                slice,
                epoch,
                seq,
                decision,
                resets,
            } => self.commit_due(slice, epoch, seq, decision, resets),
            Event::NodeStart { at, node } => self.node_start(at, node),
            Event::NodeDone { at, node, attempt } => self.node_done(at, node, attempt),
            Event::Restart { shard } => self.restart(shard),
            Event::Lose { shard } => self.lose(shard),
            Event::Takeover { slice } => self.take_over(slice),
            Event::Rebuilt { slice, epoch } => self.rebuilt(slice, epoch),
        }
    }

    fn restart(&mut self, shard: u32) -> Result<(), SimError> {
        let sh = self.shard(shard)?;
        if sh.lost {
            return Ok(());
        }
        sh.up = true;
        let owned = self.owned(shard);
        for &s in &owned {
            let epoch = slot(&mut self.slices, s)?.plane.epoch;
            let (plane, _) =
                Plane::rebuild(&self.store, &self.layout, s, epoch, self.config.workers)?;
            slot(&mut self.slices, s)?.plane = plane;
        }
        for &s in &owned {
            self.assign(s)?;
            self.sequence(s)?;
        }
        Ok(())
    }

    /// The nemesis loses `shard` for good (H11). Its slices' leases run out `lease_ns` after the
    /// last renewal before the loss; then a live shard takes each over.
    fn lose(&mut self, shard: u32) -> Result<(), SimError> {
        let sh = self.shard(shard)?;
        if sh.lost {
            return Ok(());
        }
        sh.lost = true;
        sh.up = false;
        let renewed = self
            .now
            .checked_rem(self.config.renew_ns)
            .and_then(|r| self.now.checked_sub(r))
            .ok_or(SimError::TimeOverflow)?;
        let expires = renewed
            .checked_add(self.config.lease_ns)
            .ok_or(SimError::TimeOverflow)?;
        let detection_ns = expires
            .checked_sub(self.now)
            .ok_or(SimError::TimeOverflow)?;
        for s in self.owned(shard) {
            let entry = self.report.takeovers.len();
            let slice = slot(&mut self.slices, s)?;
            slice.plane.up = false;
            slice.plane.epoch = slice
                .plane
                .epoch
                .checked_add(1)
                .ok_or(SimError::StepBudget(self.config.max_steps))?;
            slice.takeover = Some(entry);
            let running = slice.running;
            self.report.takeovers.push(Takeover {
                slice: s,
                lost: shard,
                by: shard,
                lost_at: self.now,
                detection_ns,
                records: 0,
                rebuild_ns: 0,
                running,
                takeover_ns: None,
                committed_after: 0,
            });
            self.schedule(expires, Event::Takeover { slice: s })?;
        }
        Ok(())
    }

    /// A lost shard's lease on slice `s` has run out: the next live shard after it takes the slice
    /// and rebuilds its plane from the store, which takes `rebuild_ns_per_record` a record read.
    fn take_over(&mut self, s: usize) -> Result<(), SimError> {
        let lost = slot(&mut self.slices, s)?.owner;
        let shards = u32::try_from(self.shards.len()).map_err(|_| SimError::Config("shards"))?;
        let by = (1..shards)
            .filter_map(|k| lost.checked_add(k)?.checked_rem(shards))
            .find(|&x| {
                usize::try_from(x)
                    .ok()
                    .and_then(|at| self.shards.get(at))
                    .is_some_and(|sh| !sh.lost)
            })
            .ok_or(SimError::Config("no live shard to take a lost slice over"))?;
        let epoch = slot(&mut self.slices, s)?.plane.epoch;
        let (mut plane, records) =
            Plane::rebuild(&self.store, &self.layout, s, epoch, self.config.workers)?;
        // It serves once the rebuild has taken its time.
        plane.up = false;
        let rebuild_ns = records
            .checked_mul(self.config.rebuild_ns_per_record)
            .ok_or(SimError::TimeOverflow)?;
        let slice = slot(&mut self.slices, s)?;
        slice.owner = by;
        slice.plane = plane;
        if let Some(t) = slice
            .takeover
            .and_then(|entry| self.report.takeovers.get_mut(entry))
        {
            t.by = by;
            t.records = records;
            t.rebuild_ns = rebuild_ns;
        }
        self.schedule(self.after(rebuild_ns)?, Event::Rebuilt { slice: s, epoch })
    }

    fn rebuilt(&mut self, s: usize, epoch: u64) -> Result<(), SimError> {
        let owner = slot(&mut self.slices, s)?.owner;
        // A crash of the new shard since voids this rebuild; its restart rebuilds the slice.
        if !self.shard(owner)?.up {
            return Ok(());
        }
        let plane = &mut slot(&mut self.slices, s)?.plane;
        if plane.epoch != epoch {
            return Ok(());
        }
        plane.up = true;
        self.assign(s)?;
        self.sequence(s)
    }

    /// Slice `s`'s store write, where the nemesis strikes its shard. `None`: the shard crashed there.
    fn write(&mut self, s: usize, write: Write) -> Result<Option<Outcome>, SimError> {
        let number = self.report.writes;
        self.report.writes = number
            .checked_add(1)
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
        let owner = slot(&mut self.slices, s)?.owner;
        let crash = self.strike(number);
        if crash == Some(When::Before) {
            self.crash(owner, When::Before)?;
            return Ok(None);
        }
        let outcome = self.store.apply(self.now, write)?;
        if let (Outcome::Applied { .. }, Write::Place { workload, node, .. }) = (outcome, write) {
            // The store's feed tells the node, whatever becomes of the plane.
            let at =
                usize::try_from(workload.0).map_err(|_| SimError::UnknownWorkload(workload))?;
            self.schedule(
                self.after(self.config.feed_ns)?,
                Event::NodeStart { at, node },
            )?;
        }
        if crash.is_some() {
            self.crash(owner, When::After)?;
            return Ok(None);
        }
        if let Outcome::Applied { .. } = outcome
            && let Some(t) = slot(&mut self.slices, s)?
                .takeover
                .and_then(|entry| self.report.takeovers.get_mut(entry))
        {
            tally(&mut t.committed_after, 1, self.config.max_steps)?;
            if t.takeover_ns.is_none() {
                t.takeover_ns = Some(
                    self.now
                        .checked_sub(t.lost_at)
                        .ok_or(SimError::TimeOverflow)?,
                );
            }
        }
        Ok(Some(outcome))
    }

    /// Whether the nemesis crashes the plane at write `number`, and on which side of it.
    fn strike(&mut self, number: u64) -> Option<When> {
        match self.config.nemesis {
            Nemesis::None | Nemesis::Lose { .. } => None,
            Nemesis::At(crash) if crash.write == number => Some(crash.when),
            Nemesis::At(_) => None,
            Nemesis::Random {
                first,
                per_mille,
                max_crashes,
                ..
            } => {
                let room = self.report.crashes < u64::from(max_crashes);
                let hit = number == first || (number > first && self.nemesis_rng.chance(per_mille));
                if !(room && hit) {
                    return None;
                }
                Some(if self.nemesis_rng.chance(500) {
                    When::Before
                } else {
                    When::After
                })
            }
        }
    }

    /// `shard` crashes: every slice it holds loses its plane until it restarts.
    fn crash(&mut self, shard: u32, when: When) -> Result<(), SimError> {
        let max = self.config.max_steps;
        let side = match when {
            When::Before => &mut self.report.crashes_before,
            When::After => &mut self.report.crashes_after,
        };
        tally(side, 1, max)?;
        tally(&mut self.report.crashes, 1, max)?;
        self.shard(shard)?.up = false;
        for s in self.owned(shard) {
            let slice = slot(&mut self.slices, s)?;
            tally(&mut self.report.running_through_crashes, slice.running, max)?;
            slice.plane.up = false;
            slice.plane.epoch = slice
                .plane
                .epoch
                .checked_add(1)
                .ok_or(SimError::StepBudget(max))?;
        }
        self.schedule(
            self.after(self.config.restart_ns)?,
            Event::Restart { shard },
        )
    }

    fn arrive(&mut self, at: usize, attempt: u32) -> Result<(), SimError> {
        if attempt == 0 {
            let next = at
                .checked_add(1)
                .ok_or(SimError::Config("workload index"))?;
            if let Some(w) = self.cluster.workloads.get(next) {
                self.schedule(
                    w.arrives.max(self.now),
                    Event::Arrive {
                        at: next,
                        attempt: 0,
                    },
                )?;
            }
        }
        self.schedule(
            self.after(self.config.client_retry_ns)?,
            Event::ClientCheck { at, attempt },
        )?;
        let spec = *self.workload(at)?;
        let route = self.layout.route(&spec);
        let s = route
            .or_else(|| self.layout.first(&spec))
            .ok_or(SimError::Config("slice index"))?;
        if !slot(&mut self.slices, s)?.plane.up {
            return Ok(());
        }
        if route.is_none() {
            if let Some(r) = self.refused.get_mut(at) {
                *r = true;
            }
            return Ok(());
        }
        let Some(outcome) = self.write(s, Write::Submit(spec))? else {
            return Ok(());
        };
        match outcome {
            Outcome::Applied {
                op: crate::store::Op::Submit { seq, .. },
                version,
            } => {
                let plane = &mut slot(&mut self.slices, s)?.plane;
                plane.pending.insert(seq, spec.id);
                plane.versions.insert(spec.id, version);
                plane.undecided.insert(seq);
            }
            // Stored before a crash; the rebuilt plane already queues it.
            Outcome::Duplicate { .. } => {}
            Outcome::Applied { .. } | Outcome::Conflict(_) => {
                return Err(SimError::UnexpectedConflict(spec.id));
            }
        }
        if let Some(a) = self.acked.get_mut(at) {
            *a = true;
        }
        self.assign(s)?;
        self.sequence(s)
    }

    fn client_check(&mut self, at: usize, attempt: u32) -> Result<(), SimError> {
        if self.acked.get(at).copied().unwrap_or(true)
            || self.refused.get(at).copied().unwrap_or(true)
        {
            return Ok(());
        }
        let next = attempt
            .checked_add(1)
            .ok_or(SimError::Config("client attempts"))?;
        if next >= self.config.client_attempts {
            return Err(SimError::ClientGaveUp(self.workload(at)?.id, next));
        }
        self.schedule(self.now, Event::Arrive { at, attempt: next })
    }

    /// Hands slice `s`'s undecided workloads, lowest queue position first, to its idle deciders.
    fn assign(&mut self, s: usize) -> Result<(), SimError> {
        loop {
            let plane = &mut slot(&mut self.slices, s)?.plane;
            if !plane.up || plane.idle_workers == 0 {
                return Ok(());
            }
            let Some(seq) = plane.undecided.pop_first() else {
                return Ok(());
            };
            let Some(&id) = plane.pending.get(&seq) else {
                continue;
            };
            let need = self.demand(id)?;
            if slot(&mut self.slices, s)?.plane.infeasible.contains(&need) {
                // Its class holds nowhere; the committer passes over it until capacity comes back.
                continue;
            }
            let decision = self.decide(s, &need)?;
            let plane = &mut slot(&mut self.slices, s)?.plane;
            plane.deciding.insert(seq);
            plane.idle_workers = plane
                .idle_workers
                .checked_sub(1)
                .ok_or(SimError::Config("idle deciders"))?;
            let epoch = plane.epoch;
            self.schedule(
                self.after(self.config.decide_ns)?,
                Event::Proposal {
                    slice: s,
                    epoch,
                    seq,
                    decision,
                },
            )?;
        }
    }

    fn proposal(
        &mut self,
        s: usize,
        epoch: u64,
        seq: u64,
        decision: Decision,
    ) -> Result<(), SimError> {
        let plane = &mut slot(&mut self.slices, s)?.plane;
        if epoch != plane.epoch || !plane.up {
            return Ok(());
        }
        plane.idle_workers = plane
            .idle_workers
            .checked_add(1)
            .ok_or(SimError::Config("idle deciders"))?;
        plane.deciding.remove(&seq);
        if plane.pending.contains_key(&seq) {
            plane.proposals.insert(seq, decision);
        }
        self.assign(s)?;
        self.sequence(s)
    }

    /// A sampled decision against slice `s`'s view now.
    fn decide(&mut self, s: usize, need: &Demand) -> Result<Decision, SimError> {
        let max = self.config.max_steps;
        tally(&mut self.report.decisions, 1, max)?;
        let plane = &slot(&mut self.slices, s)?.plane;
        let sampled = plane.view.sampled_fit(need, &self.config.sample);
        let snap = plane.change_no;
        let r = &mut self.report.refused;
        for (total, now) in [
            (&mut r.gpu, sampled.refused.gpu),
            (&mut r.shares, sampled.refused.shares),
            (&mut r.hot, sampled.refused.hot),
        ] {
            tally(total, now, max)?;
        }
        Ok(Decision { sampled, snap })
    }

    /// Whether `decision` is still what a scan now would choose: no node in the window it scanned
    /// changed since. A change outside the window leaves the scan's stopping point and every key it saw
    /// as they were, so the same nodes are found and the same one is best.
    fn still_holds(&mut self, s: usize, decision: &Decision) -> Result<bool, SimError> {
        let plane = &slot(&mut self.slices, s)?.plane;
        if decision.snap < plane.forgotten {
            tally(&mut self.report.stale, 1, self.config.max_steps)?;
            return Ok(false);
        }
        let nodes = plane.view.len();
        let voided = plane
            .changes
            .iter()
            .rev()
            .take_while(|(no, _)| *no > decision.snap)
            .any(|(_, key)| decision.sampled.covers(key.node, nodes));
        Ok(!voided)
    }

    /// `decision` if it still holds, else a fresh one (a conflict).
    fn late_bind(
        &mut self,
        s: usize,
        need: &Demand,
        decision: Decision,
    ) -> Result<Decision, SimError> {
        if self.still_holds(s, &decision)? {
            return Ok(decision);
        }
        tally(&mut self.report.conflicts, 1, self.config.max_steps)?;
        self.decide(s, need)
    }

    /// `need`'s class holds on no node of slice `s`: mark it so until capacity comes back.
    fn mark_infeasible(&mut self, s: usize, need: Demand) -> Result<(), SimError> {
        tally(&mut self.report.class_checks, 1, self.config.max_steps)?;
        slot(&mut self.slices, s)?.plane.infeasible.insert(need);
        Ok(())
    }

    /// Slice `s`'s committer: takes the queue in order from past `cursor`, passes over workloads no
    /// node holds, and sends the first placement it reaches; one commit in flight. Each pass visits
    /// each pending workload at most once.
    fn sequence(&mut self, s: usize) -> Result<(), SimError> {
        let budget = slot(&mut self.slices, s)?.plane.pending.len();
        for _ in 0..=budget {
            let plane = &mut slot(&mut self.slices, s)?.plane;
            if !plane.up || plane.committing {
                return Ok(());
            }
            let from = plane.cursor.map_or(Bound::Unbounded, Bound::Excluded);
            let Some((&seq, &id)) = plane.pending.range((from, Bound::Unbounded)).next() else {
                return Ok(());
            };
            let need = self.demand(id)?;
            let plane = &mut slot(&mut self.slices, s)?.plane;
            if plane.infeasible.contains(&need) {
                plane.proposals.remove(&seq);
                plane.cursor = Some(seq);
                tally(&mut self.report.class_skips, 1, self.config.max_steps)?;
                continue;
            }
            let Some(decision) = plane.proposals.remove(&seq) else {
                if !plane.deciding.contains(&seq) {
                    plane.undecided.insert(seq);
                    self.assign(s)?;
                }
                return Ok(());
            };
            let decision = self.late_bind(s, &need, decision)?;
            let plane = &mut slot(&mut self.slices, s)?.plane;
            if decision.node().is_none() {
                plane.cursor = Some(seq);
                self.mark_infeasible(s, need)?;
                continue;
            }
            plane.committing = true;
            let (epoch, resets) = (plane.epoch, plane.resets);
            return self.schedule(
                self.after(self.config.commit_ns)?,
                Event::CommitDue {
                    slice: s,
                    epoch,
                    seq,
                    decision,
                    resets,
                },
            );
        }
        Ok(())
    }

    fn commit_due(
        &mut self,
        s: usize,
        epoch: u64,
        seq: u64,
        decision: Decision,
        resets: u64,
    ) -> Result<(), SimError> {
        let plane = &mut slot(&mut self.slices, s)?.plane;
        if epoch != plane.epoch || !plane.up {
            return Ok(());
        }
        plane.committing = false;
        let Some(&id) = plane.pending.get(&seq) else {
            return self.sequence(s);
        };
        if resets != plane.resets {
            // Capacity came back since it was sent: a workload ahead may hold now. Take the queue again.
            plane.proposals.insert(seq, decision);
            return self.sequence(s);
        }
        let need = self.demand(id)?;
        let decision = self.late_bind(s, &need, decision)?;
        let Some(key) = decision.node() else {
            slot(&mut self.slices, s)?.plane.cursor = Some(seq);
            self.mark_infeasible(s, need)?;
            return self.sequence(s);
        };
        let version = *slot(&mut self.slices, s)?
            .plane
            .versions
            .get(&id)
            .ok_or(SimError::UnknownWorkload(id))?;
        let node = self.layout.global(s, key.node)?;
        let Some(outcome) = self.write(
            s,
            Write::Place {
                workload: id,
                node,
                version,
            },
        )?
        else {
            return Ok(());
        };
        let Outcome::Applied { version, .. } = outcome else {
            return Err(SimError::UnexpectedConflict(id));
        };
        let changed = slot(&mut self.slices, s)?
            .plane
            .view
            .take(key.node, &need)?;
        self.note_change(s, changed)?;
        let plane = &mut slot(&mut self.slices, s)?.plane;
        plane.pending.remove(&seq);
        plane.versions.insert(id, version);
        plane.cursor = Some(seq);
        tally(&mut self.report.placements, 1, self.config.max_steps)?;
        let arrives = self
            .store
            .record(id)
            .map(|r| r.spec.arrives)
            .ok_or(SimError::UnknownWorkload(id))?;
        self.latencies.push(
            self.now
                .checked_sub(arrives)
                .ok_or(SimError::TimeOverflow)?,
        );
        self.sequence(s)
    }

    fn note_change(&mut self, s: usize, key: Key) -> Result<(), SimError> {
        let window = self.config.change_window;
        let plane = &mut slot(&mut self.slices, s)?.plane;
        plane.change_no = plane
            .change_no
            .checked_add(1)
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
        if plane.changes.len() >= window
            && let Some((no, _)) = plane.changes.pop_front()
        {
            plane.forgotten = no;
        }
        plane.changes.push_back((plane.change_no, key));
        Ok(())
    }

    /// The slice holding `node`, for a node agent's report.
    fn slice_of(&self, node: NodeId) -> Result<usize, SimError> {
        self.layout
            .slice_of(node)
            .ok_or(SimError::UnknownNode(node))
    }

    fn node_start(&mut self, at: usize, node: NodeId) -> Result<(), SimError> {
        let starts = self
            .starts
            .get_mut(at)
            .ok_or(SimError::Config("workload index"))?;
        *starts = starts.checked_add(1).ok_or(SimError::Config("starts"))?;
        let ran = self
            .ran
            .get_mut(at)
            .ok_or(SimError::Config("workload index"))?;
        if ran.is_none() {
            *ran = Some(Ran {
                node,
                started: self.now,
                ended: None,
            });
        }
        let s = self.slice_of(node)?;
        let slice = slot(&mut self.slices, s)?;
        slice.running = slice
            .running
            .checked_add(1)
            .ok_or(SimError::Config("running"))?;
        if let Some(runs_for) = self.workload(at)?.runs_for {
            self.schedule(
                self.after(runs_for)?,
                Event::NodeDone {
                    at,
                    node,
                    attempt: 0,
                },
            )?;
        }
        Ok(())
    }

    fn node_done(&mut self, at: usize, node: NodeId, attempt: u32) -> Result<(), SimError> {
        let id = self.workload(at)?.id;
        let s = self.slice_of(node)?;
        if attempt == 0 {
            // The run itself ends here; what follows is its report.
            let slice = slot(&mut self.slices, s)?;
            slice.running = slice
                .running
                .checked_sub(1)
                .ok_or(SimError::Config("running"))?;
            if let Some(Some(ran)) = self.ran.get_mut(at) {
                ran.ended.get_or_insert(self.now);
            }
        }
        let resend = |sim: &mut Self| -> Result<(), SimError> {
            let next = attempt
                .checked_add(1)
                .ok_or(SimError::Config("report attempts"))?;
            if next >= sim.config.report_attempts {
                return Err(SimError::ReportGaveUp(node, id, next));
            }
            sim.schedule(
                sim.after(sim.config.report_retry_ns)?,
                Event::NodeDone {
                    at,
                    node,
                    attempt: next,
                },
            )
        };
        let plane = &slot(&mut self.slices, s)?.plane;
        if !plane.up {
            return resend(self);
        }
        // A workload the plane holds no version for was recorded done already (it rebuilt from a
        // store that has it complete); the store answers a repeat as a duplicate.
        let version = plane.versions.get(&id).copied().unwrap_or(0);
        let Some(outcome) = self.write(
            s,
            Write::Complete {
                workload: id,
                node,
                version,
            },
        )?
        else {
            return resend(self);
        };
        match outcome {
            Outcome::Applied { .. } => {
                let demand = self.demand(id)?;
                let local = self
                    .layout
                    .local(s, node)
                    .ok_or(SimError::UnknownNode(node))?;
                let plane = &mut slot(&mut self.slices, s)?.plane;
                let changed = plane.view.give(local, &demand)?;
                plane.versions.remove(&id);
                plane.cursor = None;
                plane.infeasible.clear();
                plane.resets = plane
                    .resets
                    .checked_add(1)
                    .ok_or(SimError::StepBudget(self.config.max_steps))?;
                self.note_change(s, changed)?;
                self.assign(s)?;
                self.sequence(s)
            }
            Outcome::Duplicate { .. } => {
                slot(&mut self.slices, s)?.plane.versions.remove(&id);
                Ok(())
            }
            Outcome::Conflict(_) => Err(SimError::UnexpectedConflict(id)),
        }
    }
}
