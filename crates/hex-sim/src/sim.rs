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
use crate::index::{FitIndex, Key};
use crate::model::{Cluster, NodeId, Resources, Time, WorkloadId, WorkloadSpec};
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
    node: Option<Key>,
    snap: u64,
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
    NodeStart {
        at: usize,
        node: NodeId,
    },
    NodeDone {
        at: usize,
        node: NodeId,
        attempt: u32,
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
                decision.node.map_or(u64::MAX, |k| u64::from(k.node.0)),
            ],
            Event::CommitDue { seq, decision, .. } => [
                4,
                seq,
                decision.node.map_or(u64::MAX, |k| u64::from(k.node.0)),
            ],
            Event::NodeStart { at: a, node } => [5, at(a), u64::from(node.0)],
            Event::NodeDone { at: a, node, .. } => [6, at(a), u64::from(node.0)],
            Event::Restart => [7, 0, 0],
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
}

impl Plane {
    fn rebuild(store: &Store, epoch: u64, workers: u32) -> Result<Self, SimError> {
        let mut pending = BTreeMap::new();
        let mut versions = BTreeMap::new();
        for record in store.records() {
            match record.state {
                State::Pending => {
                    pending.insert(record.seq, record.spec.id);
                    versions.insert(record.spec.id, record.version);
                }
                State::Placed(_) => {
                    versions.insert(record.spec.id, record.version);
                }
                State::Complete(_) => {}
            }
        }
        Ok(Self {
            up: true,
            epoch,
            view: FitIndex::new(&store.free()?)?,
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
        })
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
    plane: Plane,
    starts: Vec<u32>,
    acked: Vec<bool>,
    refused: Vec<bool>,
    ran: Vec<Option<Ran>>,
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
    // The log holds a submit, a placement and a completion per workload.
    let max_log = n
        .checked_mul(3)
        .ok_or(SimError::Config("too many workloads"))?;
    let store = Store::new(&cluster.nodes, n, max_log)?;
    let plane = Plane::rebuild(&store, 0, config.workers)?;
    let mut sim = Sim {
        cluster,
        config: *config,
        now: 0,
        queue: BinaryHeap::new(),
        order: 0,
        empty: FitIndex::new(&cluster.nodes)?,
        store,
        plane,
        starts: vec![0; n],
        acked: vec![false; n],
        refused: vec![false; n],
        ran: vec![None; n],
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
    })
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

    fn need(&self, id: WorkloadId) -> Result<Resources, SimError> {
        self.store
            .record(id)
            .map(|r| r.spec.need)
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
            Event::NodeStart { at, node } => self.node_start(at, node),
            Event::NodeDone { at, node, attempt } => self.node_done(at, node, attempt),
            Event::Restart => {
                self.plane = Plane::rebuild(&self.store, self.plane.epoch, self.config.workers)?;
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
        if self.empty.best_fit(&spec.need).is_none() {
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
                self.plane.pending.insert(seq, spec.id);
                self.plane.versions.insert(spec.id, version);
                self.plane.undecided.insert(seq);
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

    /// Hands undecided workloads, lowest queue position first, to idle deciders.
    fn assign(&mut self) -> Result<(), SimError> {
        while self.plane.up && self.plane.idle_workers > 0 {
            let Some(seq) = self.plane.undecided.pop_first() else {
                return Ok(());
            };
            let Some(&id) = self.plane.pending.get(&seq) else {
                continue;
            };
            let decision = Decision {
                node: self.plane.view.best_fit(&self.need(id)?),
                snap: self.plane.change_no,
            };
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
        if self.plane.pending.contains_key(&seq) {
            self.plane.proposals.insert(seq, decision);
        }
        self.assign()?;
        self.sequence()
    }

    /// Whether `decision` is still the best fit for `need`: no node changed since it was made that
    /// could now come first, nor the node it chose. Free capacity only moves by changes, so a node
    /// unchanged since keeps the place it had.
    fn still_holds(&mut self, need: &Resources, decision: &Decision) -> Result<bool, SimError> {
        if decision.snap < self.plane.forgotten {
            self.report.stale = self
                .report
                .stale
                .checked_add(1)
                .ok_or(SimError::StepBudget(self.config.max_steps))?;
            return Ok(false);
        }
        let voided = self
            .plane
            .changes
            .iter()
            .rev()
            .take_while(|(no, _)| *no > decision.snap)
            .any(|(_, key)| {
                let chosen_changed = decision.node.is_some_and(|k| k.node == key.node);
                let now_first = key.holds(need) && decision.node.is_none_or(|k| *key < k);
                chosen_changed || now_first
            });
        Ok(!voided)
    }

    /// `decision` if it still holds, else a fresh one (a conflict).
    fn late_bind(&mut self, need: &Resources, decision: Decision) -> Result<Decision, SimError> {
        if self.still_holds(need, &decision)? {
            return Ok(decision);
        }
        self.report.conflicts = self
            .report
            .conflicts
            .checked_add(1)
            .ok_or(SimError::StepBudget(self.config.max_steps))?;
        Ok(Decision {
            node: self.plane.view.best_fit(need),
            snap: self.plane.change_no,
        })
    }

    /// The committer: takes the queue in order from past `cursor`, passes over workloads no node
    /// holds, and sends the first placement it reaches; one commit in flight. Each pass visits each
    /// pending workload at most once.
    fn sequence(&mut self) -> Result<(), SimError> {
        let budget = self.plane.pending.len();
        for _ in 0..=budget {
            if !self.plane.up || self.plane.committing {
                return Ok(());
            }
            let from = self.plane.cursor.map_or(Bound::Unbounded, Bound::Excluded);
            let Some((&seq, &id)) = self.plane.pending.range((from, Bound::Unbounded)).next()
            else {
                return Ok(());
            };
            let Some(decision) = self.plane.proposals.remove(&seq) else {
                if !self.plane.deciding.contains(&seq) {
                    self.plane.undecided.insert(seq);
                    self.assign()?;
                }
                return Ok(());
            };
            let decision = self.late_bind(&self.need(id)?, decision)?;
            if decision.node.is_none() {
                self.plane.cursor = Some(seq);
                continue;
            }
            self.plane.committing = true;
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
        let Some(&id) = self.plane.pending.get(&seq) else {
            return self.sequence();
        };
        if resets != self.plane.resets {
            // Capacity came back since it was sent: a workload ahead may hold now. Take the queue again.
            self.plane.proposals.insert(seq, decision);
            return self.sequence();
        }
        let need = self.need(id)?;
        let decision = self.late_bind(&need, decision)?;
        let Some(key) = decision.node else {
            self.plane.cursor = Some(seq);
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
        self.plane.pending.remove(&seq);
        self.plane.versions.insert(id, version);
        self.plane.cursor = Some(seq);
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
                },
            )?;
        }
        Ok(())
    }

    fn node_done(&mut self, at: usize, node: NodeId, attempt: u32) -> Result<(), SimError> {
        let id = self.workload(at)?.id;
        if attempt == 0 {
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
                let changed = self.plane.view.give(node, &self.need(id)?)?;
                self.note_change(changed)?;
                self.plane.versions.remove(&id);
                self.plane.cursor = None;
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
            Outcome::Conflict(_) => Err(SimError::UnexpectedConflict(id)),
        }
    }
}
