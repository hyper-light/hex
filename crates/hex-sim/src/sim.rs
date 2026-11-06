//! The simulated cluster: one event loop over virtual time, every participant a state machine.
//!
//! - **Clients** submit each workload when it arrives and resubmit it, under the same id, until the
//!   control plane acknowledges it or refuses it; a resubmit of a stored workload is a duplicate.
//! - **The control plane** admits workloads (refusing one no empty node could hold), decides
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
//!   when the Borgmaster is down (Verma et al., EuroSys 2015, p. 6).
//!
//! Every queue and map is bounded by the run's workloads or by a [`Config`] field, and the run by
//! `max_steps` (CLAUDE.md §2). Nothing in it calls itself (§11): each event is handled by a function
//! that schedules further events rather than calling their handlers.

use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};
use std::ops::Bound;

use crate::error::SimError;
use crate::fair::{self, Passed, Placed, Policy, Preemption, Queue, Shares};
use crate::index::{FitIndex, Key, Refusals, Sample, Sampled};
use crate::model::{
    Cluster, Demand, GangId, Group, GroupId, NodeId, Owner, Time, WorkloadId, WorkloadSpec,
};
use crate::rng::Rng;
use crate::store::{GangOutcome, Outcome, PreemptOutcome, State, Store, Write};
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
}

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
    /// How pending work is ordered; the serial specification orders the same way.
    pub policy: Policy,
    /// Workloads the control plane may preempt over the run (`docs/design/scheduler-fairness.md`
    /// F4): 0 turns preemption off. Past it, nothing more is preempted and the passes are counted;
    /// it bounds the store's log, which keeps a preemption and a placement again per victim.
    pub preemptions: u64,
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
            policy: Policy::default(),
            preemptions: 0,
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
    /// Gangs placed, each in one write.
    pub gangs: u64,
    /// Workloads preempted, and the writes that did it.
    pub preempted: u64,
    pub preemptions: u64,
    /// Candidates passed over because their group was at its disruption budget.
    pub budget_passes: u64,
    /// Preemptions not made because the run's budget of them was spent.
    pub preemptions_refused: u64,
    /// Completion reports dropped because the store had preempted the run they report.
    pub reports_superseded: u64,
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
    /// The order the run's committer took pending work in, and the owners it weighed.
    pub policy: Policy,
    pub owners: Vec<Owner>,
    /// Each gang's members, in id order.
    pub gangs: BTreeMap<GangId, Vec<WorkloadId>>,
    /// Whether the run preempted, and its groups' disruption budgets.
    pub preemption: bool,
    pub groups: Vec<Group>,
}

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
        epoch: u64,
        seq: u64,
        decision: Decision,
    },
    CommitDue {
        epoch: u64,
        seq: u64,
        decision: Decision,
        resets: u64,
    },
    /// A gang's commit, decided whole when it comes due.
    GangDue {
        epoch: u64,
        seq: u64,
        resets: u64,
    },
    /// A preemption's commit, chosen again when it comes due.
    PreemptDue {
        epoch: u64,
        seq: u64,
        resets: u64,
    },
    NodeStart {
        at: usize,
        node: NodeId,
    },
    /// The node agent told to stop a preempted workload's run.
    NodeStop {
        at: usize,
        node: NodeId,
    },
    /// Run `run` (its start count when it started) ended, or its report is sent again.
    NodeDone {
        at: usize,
        node: NodeId,
        attempt: u32,
        run: u32,
    },
    Restart,
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
            Event::Restart => [7, 0, 0],
            Event::GangDue { seq, .. } => [8, seq, 0],
            Event::PreemptDue { seq, .. } => [9, seq, 0],
            Event::NodeStop { at: a, node } => [10, at(a), u64::from(node.0)],
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

/// What the control plane holds in memory, lost at a crash.
#[derive(Debug, Clone)]
struct Plane {
    up: bool,
    epoch: u64,
    view: FitIndex,
    policy: Policy,
    /// Pending workloads by queue, then by submit sequence; no queue is kept empty.
    pending: BTreeMap<Queue, BTreeMap<u64, WorkloadId>>,
    /// The queue each pending workload waits in, by its submit sequence.
    queue_of: BTreeMap<u64, Queue>,
    /// What each owner holds placed, for the order between queues.
    shares: Shares,
    /// The record version the plane last wrote or read, for each workload not yet done.
    versions: BTreeMap<WorkloadId, u64>,
    undecided: BTreeMap<Queue, BTreeSet<u64>>,
    deciding: BTreeSet<u64>,
    proposals: BTreeMap<u64, Decision>,
    idle_workers: u32,
    /// In each queue, every pending workload at or before this sequence holds on no node now.
    cursors: BTreeMap<Queue, u64>,
    /// Changes since which a commit already sent must be checked again: capacity given back (which
    /// voids `cursors`' claims), or a workload submitted ahead of the one in flight.
    resets: u64,
    /// The workload whose commit is in flight, and its queue.
    in_flight: Option<(Queue, u64)>,
    /// Node changes `(number, new key)`, the last `change_window` of them.
    changes: VecDeque<(u64, Key)>,
    change_no: u64,
    /// Changes up to this number have left the window.
    forgotten: u64,
    committing: bool,
    /// Equivalence classes (needs) no node holds, since capacity last came back.
    infeasible: BTreeSet<Demand>,
    /// What is placed, for preemption.
    placed: BTreeMap<WorkloadId, Placed>,
    /// Each group's members preempted and not placed again, and those workloads.
    disrupted: BTreeMap<GroupId, u32>,
    preempted: BTreeSet<WorkloadId>,
}

impl Plane {
    fn rebuild(
        store: &Store,
        cluster: &Cluster,
        epoch: u64,
        workers: u32,
        policy: Policy,
    ) -> Result<Self, SimError> {
        let mut plane = Self {
            up: true,
            epoch,
            view: FitIndex::new(&cluster.nodes)?,
            policy,
            pending: BTreeMap::new(),
            queue_of: BTreeMap::new(),
            shares: Shares::of(cluster)?,
            versions: BTreeMap::new(),
            undecided: BTreeMap::new(),
            deciding: BTreeSet::new(),
            proposals: BTreeMap::new(),
            idle_workers: workers,
            cursors: BTreeMap::new(),
            resets: 0,
            in_flight: None,
            changes: VecDeque::new(),
            change_no: 0,
            forgotten: 0,
            committing: false,
            infeasible: BTreeSet::new(),
            placed: BTreeMap::new(),
            disrupted: BTreeMap::new(),
            preempted: BTreeSet::new(),
        };
        for record in store.records() {
            match record.state {
                State::Pending => {
                    plane.enqueue(&record.spec, record.seq);
                    plane.versions.insert(record.spec.id, record.version);
                    if record.preempted {
                        plane.disrupt(&record.spec)?;
                    }
                }
                State::Placed(node) => {
                    plane.view.take(node, &record.spec.demand())?;
                    plane
                        .shares
                        .take(record.spec.tenancy.owner, &record.spec.need)?;
                    plane.versions.insert(record.spec.id, record.version);
                    plane.placed.insert(
                        record.spec.id,
                        placed(&record.spec, node, record.placed_write),
                    );
                }
                State::Complete(_) => {}
            }
        }
        Ok(plane)
    }

    fn enqueue(&mut self, spec: &WorkloadSpec, seq: u64) {
        let queue = self.policy.queue(spec);
        self.pending.entry(queue).or_default().insert(seq, spec.id);
        self.queue_of.insert(seq, queue);
        self.undecided.entry(queue).or_default().insert(seq);
    }

    /// Takes `seq` out of the pending queues.
    fn dequeue(&mut self, seq: u64) {
        let Some(queue) = self.queue_of.remove(&seq) else {
            return;
        };
        if let Some(q) = self.pending.get_mut(&queue) {
            q.remove(&seq);
            if q.is_empty() {
                self.pending.remove(&queue);
            }
        }
        if let Some(u) = self.undecided.get_mut(&queue) {
            u.remove(&seq);
            if u.is_empty() {
                self.undecided.remove(&queue);
            }
        }
    }

    fn pending_id(&self, seq: u64) -> Option<WorkloadId> {
        let queue = self.queue_of.get(&seq)?;
        self.pending.get(queue)?.get(&seq).copied()
    }

    fn mark_undecided(&mut self, seq: u64) {
        if let Some(&queue) = self.queue_of.get(&seq) {
            self.undecided.entry(queue).or_default().insert(seq);
        }
    }

    /// The queues with pending work, in policy order.
    fn order(&self) -> Vec<Queue> {
        self.shares.order(self.policy, self.pending.keys().copied())
    }

    /// `spec` preempted: back to pending, and one of its group's disruptions.
    fn disrupt(&mut self, spec: &WorkloadSpec) -> Result<(), SimError> {
        self.preempted.insert(spec.id);
        if let Some(g) = spec.tenancy.group {
            let n = self.disrupted.entry(g).or_default();
            *n = n.checked_add(1).ok_or(SimError::Config("disruptions"))?;
        }
        Ok(())
    }

    /// `spec` placed on `node` by write `write`: what its own placements change beyond the view
    /// and shares. With preemption on, every other queue's cursor goes: a share that grew may make
    /// work behind a cursor able to preempt now; and if the placement ended a disruption, its own
    /// queue's goes too, as a group back under its budget may let work behind it preempt. Whether
    /// it did: then the placing queue's cursor must not be set past the placement either.
    fn note_placed(
        &mut self,
        spec: &WorkloadSpec,
        node: NodeId,
        write: u64,
        preemption: bool,
    ) -> bool {
        self.placed.insert(spec.id, placed(spec, node, write));
        // A disrupted member placed again ends its disruption (the store's rule).
        let mut ended = false;
        if self.preempted.remove(&spec.id)
            && let Some(g) = spec.tenancy.group
            && let Some(n) = self.disrupted.get_mut(&g)
        {
            ended = true;
            *n = n.saturating_sub(1);
            if *n == 0 {
                self.disrupted.remove(&g);
            }
        }
        if preemption {
            let own = self.policy.queue(spec);
            self.cursors.retain(|q, _| *q == own && !ended);
        }
        preemption && ended
    }
}

/// A placed workload as preemption weighs it.
fn placed(spec: &WorkloadSpec, node: NodeId, write: u64) -> Placed {
    Placed {
        id: spec.id,
        node,
        demand: spec.demand(),
        owner: spec.tenancy.owner,
        band: spec.tenancy.band,
        group: spec.tenancy.group,
        in_gang: spec.tenancy.gang.is_some(),
        write,
    }
}

struct Sim<'a> {
    cluster: &'a Cluster,
    config: Config,
    now: Time,
    queue: BinaryHeap<Reverse<Scheduled>>,
    order: u64,
    store: Store,
    /// Empty nodes, for admission: a workload none of them holds is refused.
    empty: FitIndex,
    /// Each gang's members, by id; and whether the empty cluster holds the gang whole (admission).
    gangs: BTreeMap<GangId, (Vec<WorkloadId>, bool)>,
    plane: Plane,
    starts: Vec<u32>,
    acked: Vec<bool>,
    refused: Vec<bool>,
    ran: Vec<Option<Ran>>,
    /// Whether each workload's current run is going on its node.
    live: Vec<bool>,
    /// Each workload's runs up to this one were stopped by preemption: their reports are dropped.
    stopped: Vec<u32>,
    /// Workloads running on nodes now.
    running: u64,
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
    // The log holds a submit, a placement and a completion per workload, and a preemption and a
    // placement again per workload preempted.
    let max_log = u64::try_from(n)
        .ok()
        .and_then(|n| n.checked_mul(3))
        .and_then(|l| l.checked_add(config.preemptions.checked_mul(2)?))
        .and_then(|l| usize::try_from(l).ok())
        .ok_or(SimError::Config("too many workloads"))?;
    let store = Store::new(&cluster.nodes, n, max_log)?;
    for w in &cluster.workloads {
        if !usize::try_from(w.tenancy.owner.0).is_ok_and(|o| o < cluster.owners.len()) {
            return Err(SimError::Config(
                "a workload names an owner the cluster doesn't",
            ));
        }
        if let Some(g) = w.tenancy.group
            && !usize::try_from(g.0).is_ok_and(|g| g < cluster.groups.len())
        {
            return Err(SimError::Config(
                "a workload names a group the cluster doesn't",
            ));
        }
    }
    let gangs = gangs_of(cluster, &config.sample)?;
    let plane = Plane::rebuild(&store, cluster, 0, config.workers, config.policy)?;
    let mut sim = Sim {
        cluster,
        config: *config,
        now: 0,
        queue: BinaryHeap::new(),
        order: 0,
        empty: FitIndex::new(&cluster.nodes)?,
        gangs,
        store,
        plane,
        starts: vec![0; n],
        acked: vec![false; n],
        refused: vec![false; n],
        ran: vec![None; n],
        live: vec![false; n],
        stopped: vec![0; n],
        running: 0,
        nemesis_rng: Rng::new(match config.nemesis {
            Nemesis::Random { seed, .. } => seed,
            Nemesis::None | Nemesis::At(_) => 0,
        }),
        latencies: Vec::with_capacity(n),
        hash: TraceHash::default(),
        report: Report::default(),
    };
    if let Some(first) = cluster.workloads.first() {
        sim.schedule(first.arrives, Event::Arrive { at: 0, attempt: 0 })?;
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
        policy: sim.config.policy,
        owners: sim.cluster.owners.clone(),
        gangs: sim
            .gangs
            .into_iter()
            .map(|(g, (members, _))| (g, members))
            .collect(),
        preemption: sim.config.preemptions > 0,
        groups: sim.cluster.groups.clone(),
    })
}

/// Each gang's members in id order, and whether an empty cluster holds it whole. A gang's members
/// share one owner and band (one queue, so placed at one place in it) and one group.
fn gangs_of(
    cluster: &Cluster,
    sample: &Sample,
) -> Result<BTreeMap<GangId, (Vec<WorkloadId>, bool)>, SimError> {
    let mut members: BTreeMap<GangId, Vec<&WorkloadSpec>> = BTreeMap::new();
    for w in &cluster.workloads {
        if let Some(g) = w.tenancy.gang {
            members.entry(g).or_default().push(w);
        }
    }
    let mut out = BTreeMap::new();
    for (gang, ws) in members {
        let first = ws
            .first()
            .ok_or(SimError::Config("a gang with no members"))?;
        if ws.iter().any(|w| {
            (w.tenancy.owner, w.tenancy.band, w.tenancy.group)
                != (first.tenancy.owner, first.tenancy.band, first.tenancy.group)
        }) {
            return Err(SimError::Config(
                "a gang's members differ in owner, band or group",
            ));
        }
        let mut empty = FitIndex::new(&cluster.nodes)?;
        let demands: Vec<Demand> = ws.iter().map(|w| w.demand()).collect();
        let fits = fit_whole(&mut empty, &demands, sample)?.is_some();
        out.insert(gang, (ws.iter().map(|w| w.id).collect(), fits));
    }
    Ok(out)
}

/// Where each of `demands`, in order, goes on `view` (each taking its room before the next is
/// sought), or `None` if one fits nowhere; `view` is left as it was.
pub fn fit_whole(
    view: &mut FitIndex,
    demands: &[Demand],
    sample: &Sample,
) -> Result<Option<Vec<NodeId>>, SimError> {
    let mut taken: Vec<(NodeId, Demand)> = Vec::with_capacity(demands.len());
    let mut whole = true;
    for d in demands {
        let Some(key) = view.sampled_fit(d, sample).best else {
            whole = false;
            break;
        };
        view.take(key.node, d)?;
        taken.push((key.node, *d));
    }
    for (node, d) in &taken {
        view.give(*node, d)?;
    }
    Ok(whole.then(|| taken.into_iter().map(|(n, _)| n).collect()))
}

/// A gang member as its commit writes it: the workload, its node, the record version read and its
/// submit sequence.
type GangMember = (WorkloadId, NodeId, u64, u64);

/// Where a gang stands for the committer.
enum GangState {
    /// A member isn't submitted yet: what it waits for is its members, not capacity.
    Incomplete,
    /// Every member is pending; this is the first of them in its queue.
    First,
    /// Every member is pending; another comes first and is placed with this one.
    Later,
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

    fn spec(&self, id: WorkloadId) -> Result<WorkloadSpec, SimError> {
        self.store
            .record(id)
            .map(|r| r.spec)
            .ok_or(SimError::UnknownWorkload(id))
    }

    /// A gang's members with their submit sequences, in id order.
    fn members(&self, gang: GangId) -> Result<Vec<(WorkloadId, Option<u64>)>, SimError> {
        let (ids, _) = self
            .gangs
            .get(&gang)
            .ok_or(SimError::Config("a gang the run doesn't know"))?;
        Ok(ids
            .iter()
            .map(|id| (*id, self.store.record(*id).map(|r| r.seq)))
            .collect())
    }

    fn gang_state(&self, gang: GangId, seq: u64) -> Result<GangState, SimError> {
        let members = self.members(gang)?;
        let mut first = u64::MAX;
        for (_, s) in members {
            match s {
                Some(s) if self.plane.queue_of.contains_key(&s) => first = first.min(s),
                _ => return Ok(GangState::Incomplete),
            }
        }
        Ok(if first == seq {
            GangState::First
        } else {
            GangState::Later
        })
    }

    /// The gang's members, their versions and where each goes now, or `None` if the view doesn't
    /// hold it whole.
    fn decide_gang(&mut self, gang: GangId) -> Result<Option<Vec<GangMember>>, SimError> {
        let members = self.members(gang)?;
        let mut demands = Vec::with_capacity(members.len());
        for (id, _) in &members {
            demands.push(self.demand(*id)?);
        }
        self.report.decisions = self
            .report
            .decisions
            .checked_add(u64::try_from(members.len()).unwrap_or(u64::MAX))
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
        let Some(nodes) = fit_whole(&mut self.plane.view, &demands, &self.config.sample)? else {
            return Ok(None);
        };
        let mut out = Vec::with_capacity(members.len());
        for ((id, seq), node) in members.into_iter().zip(nodes) {
            let version = *self
                .plane
                .versions
                .get(&id)
                .ok_or(SimError::UnknownWorkload(id))?;
            out.push((id, node, version, seq.ok_or(SimError::UnknownWorkload(id))?));
        }
        Ok(Some(out))
    }

    fn demand(&self, id: WorkloadId) -> Result<Demand, SimError> {
        self.store
            .record(id)
            .map(|r| r.spec.demand())
            .ok_or(SimError::UnknownWorkload(id))
    }

    fn handle(&mut self, event: Event) -> Result<(), SimError> {
        match event {
            Event::Arrive { at, attempt } => self.arrive(at, attempt),
            Event::ClientCheck { at, attempt } => self.client_check(at, attempt),
            Event::Proposal {
                epoch,
                seq,
                decision,
            } => self.proposal(epoch, seq, decision),
            Event::CommitDue {
                epoch,
                seq,
                decision,
                resets,
            } => self.commit_due(epoch, seq, decision, resets),
            Event::GangDue { epoch, seq, resets } => self.gang_due(epoch, seq, resets),
            Event::PreemptDue { epoch, seq, resets } => self.preempt_due(epoch, seq, resets),
            Event::NodeStart { at, node } => self.node_start(at, node),
            Event::NodeStop { at, node } => self.node_stop(at, node),
            Event::NodeDone {
                at,
                node,
                attempt,
                run,
            } => self.node_done(at, node, attempt, run),
            Event::Restart => {
                self.plane = Plane::rebuild(
                    &self.store,
                    self.cluster,
                    self.plane.epoch,
                    self.config.workers,
                    self.config.policy,
                )?;
                self.assign()?;
                self.sequence()
            }
        }
    }

    /// The control plane's store write, where the nemesis strikes. `None`: the plane crashed there.
    fn write(&mut self, write: Write) -> Result<Option<Outcome>, SimError> {
        let number = self.report.writes;
        self.report.writes = number
            .checked_add(1)
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
        let crash = self.strike(number);
        if crash == Some(When::Before) {
            self.crash(When::Before)?;
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
            self.crash(When::After)?;
            return Ok(None);
        }
        Ok(Some(outcome))
    }

    /// Whether the nemesis crashes the plane at write `number`, and on which side of it.
    fn strike(&mut self, number: u64) -> Option<When> {
        match self.config.nemesis {
            Nemesis::None => None,
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

    fn crash(&mut self, when: When) -> Result<(), SimError> {
        let side = match when {
            When::Before => &mut self.report.crashes_before,
            When::After => &mut self.report.crashes_after,
        };
        *side = side
            .checked_add(1)
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
        self.report.running_through_crashes = self
            .report
            .running_through_crashes
            .checked_add(self.running)
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
        self.report.crashes = self
            .report
            .crashes
            .checked_add(1)
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
        self.plane.up = false;
        self.plane.epoch = self
            .plane
            .epoch
            .checked_add(1)
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
        self.schedule(self.after(self.config.restart_ns)?, Event::Restart)
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
        if !self.plane.up {
            return Ok(());
        }
        let spec = *self.workload(at)?;
        let gang_fits = spec
            .tenancy
            .gang
            .is_none_or(|g| self.gangs.get(&g).is_some_and(|(_, fits)| *fits));
        if !gang_fits || !self.empty.admitted_anywhere(&spec.demand()) {
            if let Some(r) = self.refused.get_mut(at) {
                *r = true;
            }
            return Ok(());
        }
        let Some(outcome) = self.write(Write::Submit(spec))? else {
            return Ok(());
        };
        match outcome {
            Outcome::Applied {
                op: crate::store::Op::Submit { seq, .. },
                version,
            } => {
                self.plane.enqueue(&spec, seq);
                self.plane.versions.insert(spec.id, version);
                if spec.tenancy.gang.is_some() {
                    // A gang this member completes may hold where its queue's cursor has passed.
                    let queue = self.plane.policy.queue(&spec);
                    self.plane.cursors.remove(&queue);
                }
                self.note_ahead(&spec)?;
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
        self.assign()?;
        self.sequence()
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

    /// A workload submitted ahead of the one whose commit is in flight sends that commit back to
    /// the queue when it comes due, as capacity given back does: one in a queue the policy puts
    /// first, or a gang's last member, which makes the gang whole at its first member's place, so
    /// perhaps ahead of the one in flight in its own queue.
    fn note_ahead(&mut self, spec: &WorkloadSpec) -> Result<(), SimError> {
        let Some((queue, _)) = self.plane.in_flight else {
            return Ok(());
        };
        let policy = self.plane.policy;
        let new = policy.queue(spec);
        let ahead = if new == queue {
            spec.tenancy.gang.is_some()
        } else {
            self.plane.shares.rank(policy, new) < self.plane.shares.rank(policy, queue)
        };
        if ahead {
            self.plane.resets = self
                .plane
                .resets
                .checked_add(1)
                .ok_or(SimError::StepBudget(self.config.max_steps))?;
        }
        Ok(())
    }

    /// The next undecided workload, its queue first in policy order, lowest sequence first.
    fn next_undecided(&mut self) -> Option<u64> {
        let order = self.plane.order();
        for queue in order {
            let Some(u) = self.plane.undecided.get_mut(&queue) else {
                continue;
            };
            let seq = u.pop_first();
            if u.is_empty() {
                self.plane.undecided.remove(&queue);
            }
            if seq.is_some() {
                return seq;
            }
        }
        None
    }

    /// Hands undecided workloads, in policy order, to idle deciders.
    fn assign(&mut self) -> Result<(), SimError> {
        while self.plane.up && self.plane.idle_workers > 0 {
            let Some(seq) = self.next_undecided() else {
                return Ok(());
            };
            let Some(id) = self.plane.pending_id(seq) else {
                continue;
            };
            if self.spec(id)?.tenancy.gang.is_some() {
                // A gang is decided whole by the committer when it comes due.
                continue;
            }
            let need = self.demand(id)?;
            if self.plane.infeasible.contains(&need) {
                // Its class holds nowhere; the committer passes over it until capacity comes back.
                continue;
            }
            let decision = self.decide(&need)?;
            self.plane.deciding.insert(seq);
            self.plane.idle_workers = self
                .plane
                .idle_workers
                .checked_sub(1)
                .ok_or(SimError::Config("idle deciders"))?;
            let epoch = self.plane.epoch;
            self.schedule(
                self.after(self.config.decide_ns)?,
                Event::Proposal {
                    epoch,
                    seq,
                    decision,
                },
            )?;
        }
        Ok(())
    }

    fn proposal(&mut self, epoch: u64, seq: u64, decision: Decision) -> Result<(), SimError> {
        if epoch != self.plane.epoch || !self.plane.up {
            return Ok(());
        }
        self.plane.idle_workers = self
            .plane
            .idle_workers
            .checked_add(1)
            .ok_or(SimError::Config("idle deciders"))?;
        self.plane.deciding.remove(&seq);
        if self.plane.queue_of.contains_key(&seq) {
            self.plane.proposals.insert(seq, decision);
        }
        self.assign()?;
        self.sequence()
    }

    /// A sampled decision against the plane's view now.
    fn decide(&mut self, need: &Demand) -> Result<Decision, SimError> {
        self.report.decisions = self
            .report
            .decisions
            .checked_add(1)
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
        let sampled = self.plane.view.sampled_fit(need, &self.config.sample);
        let r = &mut self.report.refused;
        for (total, now) in [
            (&mut r.gpu, sampled.refused.gpu),
            (&mut r.shares, sampled.refused.shares),
            (&mut r.hot, sampled.refused.hot),
        ] {
            *total = total
                .checked_add(now)
                .ok_or(SimError::StepBudget(self.config.max_steps))?;
        }
        Ok(Decision {
            sampled,
            snap: self.plane.change_no,
        })
    }

    /// Whether `decision` is still what a scan now would choose: no node in the window it scanned
    /// changed since. A change outside the window leaves the scan's stopping point and every key it saw
    /// as they were, so the same nodes are found and the same one is best.
    fn still_holds(&mut self, decision: &Decision) -> Result<bool, SimError> {
        if decision.snap < self.plane.forgotten {
            self.report.stale = self
                .report
                .stale
                .checked_add(1)
                .ok_or(SimError::StepBudget(self.config.max_steps))?;
            return Ok(false);
        }
        let nodes = self.plane.view.len();
        let voided = self
            .plane
            .changes
            .iter()
            .rev()
            .take_while(|(no, _)| *no > decision.snap)
            .any(|(_, key)| decision.sampled.covers(key.node, nodes));
        Ok(!voided)
    }

    /// `decision` if it still holds, else a fresh one (a conflict).
    fn late_bind(&mut self, need: &Demand, decision: Decision) -> Result<Decision, SimError> {
        if self.still_holds(&decision)? {
            return Ok(decision);
        }
        self.report.conflicts = self
            .report
            .conflicts
            .checked_add(1)
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
        self.decide(need)
    }

    /// `need`'s class holds on no node: mark it so until capacity comes back.
    fn mark_infeasible(&mut self, need: Demand) -> Result<(), SimError> {
        self.report.class_checks = self
            .report
            .class_checks
            .checked_add(1)
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
        self.plane.infeasible.insert(need);
        Ok(())
    }

    /// The committer: takes the first queue in policy order from past its cursor, passes over
    /// workloads no node holds, and sends the first placement it reaches; one commit in flight. When
    /// none of that queue's work holds on any node now, no queue after it is placed either: DRF's
    /// allocation stops at the lowest share whose next task doesn't fit (NSDI 2011, Algorithm 1),
    /// so smaller work of larger shares can't keep taking what frees up. Each pass visits each
    /// pending workload of the queue at most once.
    fn sequence(&mut self) -> Result<(), SimError> {
        let Some(q) = self.plane.order().first().copied() else {
            return Ok(());
        };
        let budget = self.plane.pending.get(&q).map_or(0, BTreeMap::len);
        // Where the pass is: past the cursor, and past gang members that don't move it.
        let mut from = self
            .plane
            .cursors
            .get(&q)
            .map_or(Bound::Unbounded, |&c| Bound::Excluded(c));
        for _ in 0..=budget {
            if !self.plane.up || self.plane.committing {
                return Ok(());
            }
            let Some((&seq, &id)) = self
                .plane
                .pending
                .get(&q)
                .and_then(|p| p.range((from, Bound::Unbounded)).next())
            else {
                return Ok(());
            };
            from = Bound::Excluded(seq);
            if let Some(gang) = self.spec(id)?.tenancy.gang {
                match self.gang_state(gang, seq)? {
                    GangState::Incomplete | GangState::Later => continue,
                    GangState::First => {}
                }
                if self.decide_gang(gang)?.is_none() {
                    self.plane.cursors.insert(q, seq);
                    continue;
                }
                self.plane.committing = true;
                self.plane.in_flight = Some((q, seq));
                let (epoch, resets) = (self.plane.epoch, self.plane.resets);
                return self.schedule(
                    self.after(self.config.commit_ns)?,
                    Event::GangDue { epoch, seq, resets },
                );
            }
            let need = self.demand(id)?;
            if self.plane.infeasible.contains(&need) {
                self.plane.proposals.remove(&seq);
                if self.send_preemption(q, seq, id)? {
                    return Ok(());
                }
                self.plane.cursors.insert(q, seq);
                self.report.class_skips = self
                    .report
                    .class_skips
                    .checked_add(1)
                    .ok_or(SimError::StepBudget(self.config.max_steps))?;
                continue;
            }
            let Some(decision) = self.plane.proposals.remove(&seq) else {
                if !self.plane.deciding.contains(&seq) {
                    self.plane.mark_undecided(seq);
                    self.assign()?;
                }
                return Ok(());
            };
            let decision = self.late_bind(&need, decision)?;
            if decision.node().is_none() {
                self.mark_infeasible(need)?;
                if self.send_preemption(q, seq, id)? {
                    return Ok(());
                }
                self.plane.cursors.insert(q, seq);
                continue;
            }
            self.plane.committing = true;
            self.plane.in_flight = Some((q, seq));
            let (epoch, resets) = (self.plane.epoch, self.plane.resets);
            return self.schedule(
                self.after(self.config.commit_ns)?,
                Event::CommitDue {
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
        epoch: u64,
        seq: u64,
        decision: Decision,
        resets: u64,
    ) -> Result<(), SimError> {
        if epoch != self.plane.epoch || !self.plane.up {
            return Ok(());
        }
        self.plane.committing = false;
        self.plane.in_flight = None;
        let (Some(id), Some(&queue)) = (self.plane.pending_id(seq), self.plane.queue_of.get(&seq))
        else {
            return self.sequence();
        };
        if resets != self.plane.resets {
            // Capacity came back, or work was submitted ahead, since it was sent: a workload ahead
            // may hold now. Take the queues again.
            self.plane.proposals.insert(seq, decision);
            return self.sequence();
        }
        let need = self.demand(id)?;
        let decision = self.late_bind(&need, decision)?;
        let Some(key) = decision.node() else {
            self.plane.cursors.insert(queue, seq);
            self.mark_infeasible(need)?;
            return self.sequence();
        };
        let version = *self
            .plane
            .versions
            .get(&id)
            .ok_or(SimError::UnknownWorkload(id))?;
        let Some(outcome) = self.write(Write::Place {
            workload: id,
            node: key.node,
            version,
        })?
        else {
            return Ok(());
        };
        let Outcome::Applied { version, .. } = outcome else {
            return Err(SimError::UnexpectedConflict(id));
        };
        let changed = self.plane.view.take(key.node, &need)?;
        self.note_change(changed)?;
        let (spec, write) = self
            .store
            .record(id)
            .map(|r| (r.spec, r.placed_write))
            .ok_or(SimError::UnknownWorkload(id))?;
        self.plane.shares.take(spec.tenancy.owner, &need.need)?;
        let preemption = self.config.preemptions > 0;
        let reopened = self.plane.note_placed(&spec, key.node, write, preemption);
        self.plane.dequeue(seq);
        self.plane.versions.insert(id, version);
        if !reopened {
            self.plane.cursors.insert(queue, seq);
        }
        self.report.placements = self
            .report
            .placements
            .checked_add(1)
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
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
        self.sequence()
    }

    /// A gang's commit come due: decided whole again against the view now (late binding), and
    /// written in one store write.
    fn gang_due(&mut self, epoch: u64, seq: u64, resets: u64) -> Result<(), SimError> {
        if epoch != self.plane.epoch || !self.plane.up {
            return Ok(());
        }
        self.plane.committing = false;
        self.plane.in_flight = None;
        let (Some(id), Some(&queue)) = (self.plane.pending_id(seq), self.plane.queue_of.get(&seq))
        else {
            return self.sequence();
        };
        if resets != self.plane.resets {
            return self.sequence();
        }
        let gang = self
            .spec(id)?
            .tenancy
            .gang
            .ok_or(SimError::Config("a gang commit for a workload in no gang"))?;
        let Some(members) = self.decide_gang(gang)? else {
            self.plane.cursors.insert(queue, seq);
            return self.sequence();
        };
        let write: Vec<(WorkloadId, NodeId, u64)> =
            members.iter().map(|&(w, n, v, _)| (w, n, v)).collect();
        let Some(versions) = self.write_gang(&write)? else {
            return Ok(());
        };
        let mut reopened = false;
        for (&(member, node, _, member_seq), version) in members.iter().zip(versions) {
            let spec = self.spec(member)?;
            let changed = self.plane.view.take(node, &spec.demand())?;
            self.note_change(changed)?;
            self.plane.shares.take(spec.tenancy.owner, &spec.need)?;
            let write = self.store.record(member).map_or(0, |r| r.placed_write);
            let preemption = self.config.preemptions > 0;
            reopened |= self.plane.note_placed(&spec, node, write, preemption);
            self.plane.dequeue(member_seq);
            self.plane.proposals.remove(&member_seq);
            self.plane.versions.insert(member, version);
            self.report.placements = self
                .report
                .placements
                .checked_add(1)
                .ok_or(SimError::StepBudget(self.config.max_steps))?;
            self.latencies.push(
                self.now
                    .checked_sub(spec.arrives)
                    .ok_or(SimError::TimeOverflow)?,
            );
        }
        self.report.gangs = self
            .report
            .gangs
            .checked_add(1)
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
        if !reopened {
            self.plane.cursors.insert(queue, seq);
        }
        self.sequence()
    }

    /// Victims for `id` against the plane's view now, within the run's budget of preemptions.
    fn choose(&mut self, id: WorkloadId) -> Result<Option<Preemption>, SimError> {
        if self.config.preemptions == 0 {
            return Ok(None);
        }
        let spec = self.spec(id)?;
        let placed: Vec<Placed> = self.plane.placed.values().copied().collect();
        let mut passed = Passed::default();
        let chosen = fair::choose_victims(
            &mut self.plane.view,
            &placed,
            &spec,
            &self.plane.shares,
            &self.cluster.groups,
            &self.plane.disrupted,
            &mut passed,
        )?;
        self.report.budget_passes = self
            .report
            .budget_passes
            .checked_add(passed.for_budget)
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
        let Some(chosen) = chosen else {
            return Ok(None);
        };
        let after = u64::try_from(chosen.victims.len())
            .ok()
            .and_then(|n| self.report.preempted.checked_add(n));
        if after.is_none_or(|n| n > self.config.preemptions) {
            self.report.preemptions_refused = self
                .report
                .preemptions_refused
                .checked_add(1)
                .ok_or(SimError::StepBudget(self.config.max_steps))?;
            return Ok(None);
        }
        Ok(Some(chosen))
    }

    /// Sends a preemption for `seq` of queue `q` if one can be made: `true` when one is in flight.
    fn send_preemption(&mut self, q: Queue, seq: u64, id: WorkloadId) -> Result<bool, SimError> {
        if self.choose(id)?.is_none() {
            return Ok(false);
        }
        self.plane.committing = true;
        self.plane.in_flight = Some((q, seq));
        let (epoch, resets) = (self.plane.epoch, self.plane.resets);
        self.schedule(
            self.after(self.config.commit_ns)?,
            Event::PreemptDue { epoch, seq, resets },
        )?;
        Ok(true)
    }

    /// A preemption's commit come due: its victims chosen again against the view now (late
    /// binding), and written with the preemptor's placement in one store write.
    fn preempt_due(&mut self, epoch: u64, seq: u64, resets: u64) -> Result<(), SimError> {
        if epoch != self.plane.epoch || !self.plane.up {
            return Ok(());
        }
        self.plane.committing = false;
        self.plane.in_flight = None;
        let Some(id) = self.plane.pending_id(seq) else {
            return self.sequence();
        };
        if resets != self.plane.resets {
            return self.sequence();
        }
        let Some(chosen) = self.choose(id)? else {
            return self.sequence();
        };
        let version = |plane: &Plane, w: WorkloadId| {
            plane
                .versions
                .get(&w)
                .copied()
                .ok_or(SimError::UnknownWorkload(w))
        };
        let mut victims = Vec::with_capacity(chosen.victims.len());
        for &v in &chosen.victims {
            let node = self
                .plane
                .placed
                .get(&v)
                .map(|p| p.node)
                .ok_or(SimError::UnknownWorkload(v))?;
            victims.push((v, node, version(&self.plane, v)?));
        }
        let head = (id, chosen.node, version(&self.plane, id)?);
        let Some((versions, placed)) = self.write_preempt(&victims, head)? else {
            return Ok(());
        };
        for (&(v, node, _), new) in victims.iter().zip(versions) {
            let spec = self.spec(v)?;
            let changed = self.plane.view.give(node, &spec.demand())?;
            self.note_change(changed)?;
            self.plane.shares.give(spec.tenancy.owner, &spec.need)?;
            self.plane.versions.insert(v, new);
            self.plane.placed.remove(&v);
            let victim_seq = self
                .store
                .record(v)
                .map(|r| r.seq)
                .ok_or(SimError::UnknownWorkload(v))?;
            self.plane.enqueue(&spec, victim_seq);
            self.plane.disrupt(&spec)?;
            self.report.preempted = self
                .report
                .preempted
                .checked_add(1)
                .ok_or(SimError::StepBudget(self.config.max_steps))?;
        }
        let spec = self.spec(id)?;
        let changed = self.plane.view.take(chosen.node, &spec.demand())?;
        self.note_change(changed)?;
        self.plane.shares.take(spec.tenancy.owner, &spec.need)?;
        let write = self.store.record(id).map_or(0, |r| r.placed_write);
        // Every cursor goes below, whatever this placement ended.
        self.plane.note_placed(&spec, chosen.node, write, true);
        self.plane.dequeue(seq);
        self.plane.proposals.remove(&seq);
        self.plane.versions.insert(id, placed);
        self.report.placements = self
            .report
            .placements
            .checked_add(1)
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
        self.report.preemptions = self
            .report
            .preemptions
            .checked_add(1)
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
        self.latencies.push(
            self.now
                .checked_sub(spec.arrives)
                .ok_or(SimError::TimeOverflow)?,
        );
        // Victims are pending again and may hold elsewhere: every claim of holding nowhere goes.
        self.plane.cursors.clear();
        self.plane.infeasible.clear();
        self.plane.resets = self
            .plane
            .resets
            .checked_add(1)
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
        self.assign()?;
        self.sequence()
    }

    /// A preemption's store write, where the nemesis strikes as at any write: the victims' new
    /// versions and the preemptor's, or `None` if the plane crashed there.
    fn write_preempt(
        &mut self,
        victims: &[(WorkloadId, NodeId, u64)],
        head: (WorkloadId, NodeId, u64),
    ) -> Result<Option<(Vec<u64>, u64)>, SimError> {
        let number = self.report.writes;
        self.report.writes = number
            .checked_add(1)
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
        let crash = self.strike(number);
        if crash == Some(When::Before) {
            self.crash(When::Before)?;
            return Ok(None);
        }
        let outcome = self
            .store
            .apply_preempt(self.now, victims, head, &self.cluster.groups)?;
        let PreemptOutcome::Applied {
            victims: new,
            placed,
        } = outcome
        else {
            return Err(SimError::UnexpectedConflict(head.0));
        };
        // The store's feed tells the nodes, whatever becomes of the plane: the victims' first, so
        // a node frees the room before its new run starts.
        for &(workload, node, _) in victims {
            let at =
                usize::try_from(workload.0).map_err(|_| SimError::UnknownWorkload(workload))?;
            self.schedule(
                self.after(self.config.feed_ns)?,
                Event::NodeStop { at, node },
            )?;
        }
        let at = usize::try_from(head.0.0).map_err(|_| SimError::UnknownWorkload(head.0))?;
        self.schedule(
            self.after(self.config.feed_ns)?,
            Event::NodeStart { at, node: head.1 },
        )?;
        if crash.is_some() {
            self.crash(When::After)?;
            return Ok(None);
        }
        Ok(Some((new, placed)))
    }

    /// A gang's store write, where the nemesis strikes as at any write. `None`: the plane crashed
    /// there.
    fn write_gang(
        &mut self,
        members: &[(WorkloadId, NodeId, u64)],
    ) -> Result<Option<Vec<u64>>, SimError> {
        let number = self.report.writes;
        self.report.writes = number
            .checked_add(1)
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
        let crash = self.strike(number);
        if crash == Some(When::Before) {
            self.crash(When::Before)?;
            return Ok(None);
        }
        let outcome = self.store.apply_gang(self.now, members)?;
        let GangOutcome::Applied { versions } = outcome else {
            let id = members.first().map_or(WorkloadId(u64::MAX), |m| m.0);
            return Err(SimError::UnexpectedConflict(id));
        };
        for &(workload, node, _) in members {
            let at =
                usize::try_from(workload.0).map_err(|_| SimError::UnknownWorkload(workload))?;
            self.schedule(
                self.after(self.config.feed_ns)?,
                Event::NodeStart { at, node },
            )?;
        }
        if crash.is_some() {
            self.crash(When::After)?;
            return Ok(None);
        }
        Ok(Some(versions))
    }

    fn note_change(&mut self, key: Key) -> Result<(), SimError> {
        self.plane.change_no = self
            .plane
            .change_no
            .checked_add(1)
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
        if self.plane.changes.len() >= self.config.change_window
            && let Some((no, _)) = self.plane.changes.pop_front()
        {
            self.plane.forgotten = no;
        }
        self.plane.changes.push_back((self.plane.change_no, key));
        Ok(())
    }

    fn node_start(&mut self, at: usize, node: NodeId) -> Result<(), SimError> {
        let starts = self
            .starts
            .get_mut(at)
            .ok_or(SimError::Config("workload index"))?;
        *starts = starts.checked_add(1).ok_or(SimError::Config("starts"))?;
        let run = *starts;
        if let Some(live) = self.live.get_mut(at) {
            *live = true;
        }
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
        self.running = self
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
                    run,
                },
            )?;
        }
        Ok(())
    }

    /// The node agent stops a preempted workload's run: it ends now, and its report, if one is
    /// still to come or being sent again, is dropped, as the run was stopped rather than done.
    fn node_stop(&mut self, at: usize, _node: NodeId) -> Result<(), SimError> {
        let starts = self.starts.get(at).copied().unwrap_or(0);
        if let Some(stopped) = self.stopped.get_mut(at) {
            *stopped = starts;
        }
        if self.live.get(at).copied().unwrap_or(false) {
            if let Some(live) = self.live.get_mut(at) {
                *live = false;
            }
            self.running = self
                .running
                .checked_sub(1)
                .ok_or(SimError::Config("running"))?;
            if let Some(Some(ran)) = self.ran.get_mut(at) {
                ran.ended.get_or_insert(self.now);
            }
        }
        Ok(())
    }

    fn node_done(
        &mut self,
        at: usize,
        node: NodeId,
        attempt: u32,
        run: u32,
    ) -> Result<(), SimError> {
        let id = self.workload(at)?.id;
        if self.stopped.get(at).is_some_and(|&s| run <= s) {
            return Ok(());
        }
        if attempt == 0 {
            if let Some(live) = self.live.get_mut(at) {
                *live = false;
            }
            // The run itself ends here; what follows is its report.
            self.running = self
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
                    run,
                },
            )
        };
        if !self.plane.up {
            return resend(self);
        }
        // A workload the plane holds no version for was recorded done already (it rebuilt from a
        // store that has it complete); the store answers a repeat as a duplicate.
        let version = self.plane.versions.get(&id).copied().unwrap_or(0);
        let Some(outcome) = self.write(Write::Complete {
            workload: id,
            node,
            version,
        })?
        else {
            return resend(self);
        };
        match outcome {
            Outcome::Applied { .. } => {
                let spec = self
                    .store
                    .record(id)
                    .map(|r| r.spec)
                    .ok_or(SimError::UnknownWorkload(id))?;
                let changed = self.plane.view.give(node, &spec.demand())?;
                self.note_change(changed)?;
                self.plane.shares.give(spec.tenancy.owner, &spec.need)?;
                self.plane.versions.remove(&id);
                self.plane.placed.remove(&id);
                self.plane.cursors.clear();
                self.plane.infeasible.clear();
                self.plane.resets = self
                    .plane
                    .resets
                    .checked_add(1)
                    .ok_or(SimError::StepBudget(self.config.max_steps))?;
                self.assign()?;
                self.sequence()
            }
            Outcome::Duplicate { .. } => {
                self.plane.versions.remove(&id);
                Ok(())
            }
            Outcome::Conflict(_) => {
                // The store preempted this run after it started (its record back in pending, or
                // placed again since) before the node saw the order to stop it: the run was
                // stopped, not done, and the workload runs again. Any other conflict is a bug.
                let superseded = self.store.record(id).is_some_and(|r| {
                    r.placements > run
                        || (r.placements == run && r.preempted && r.state == State::Pending)
                });
                if !superseded {
                    return Err(SimError::UnexpectedConflict(id));
                }
                if let Some(stopped) = self.stopped.get_mut(at) {
                    *stopped = (*stopped).max(run);
                }
                self.report.reports_superseded = self
                    .report
                    .reports_superseded
                    .checked_add(1)
                    .ok_or(SimError::StepBudget(self.config.max_steps))?;
                Ok(())
            }
        }
    }
}
