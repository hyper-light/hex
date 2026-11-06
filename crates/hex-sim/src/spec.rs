//! The scheduler's serial specification (`scheduler-architecture.md` S6): one scheduler, one cycle.
//! Workloads queue in the order the store accepted them; whenever state changes, the scheduler takes
//! the queue in order and puts each workload on the best fit its class's sample finds
//! ([`FitIndex::sampled_fit`], the fast path's own choice), passing over one no node holds now. A placement is correct when this specification could have made it.
//!
//! [`serial_place`] runs the specification directly on a cluster whose capacity only shrinks.
//! [`check_log`] replays a run's store log and refuses the first write the specification couldn't
//! have made: the model-based oracle of `T-T7.4.1`, in the shape of QuickCheck's state-machine model
//! checked at every step (DS[10], DS[11] in `docs/research/scheduler/deterministic-simulation.md`).
//! [`check_end`] counts what `AC-T7.3.3` holds to zero: workloads lost or started twice.
//!
//! Counters here saturate: each counts a run's workloads or writes, bounded by the store far below
//! `u64::MAX`, so saturation is the unreachable case's stand-in, never a value a report shows.

use std::collections::BTreeMap;

use crate::error::SimError;
use crate::index::{FitIndex, Sample};
use crate::model::{Cluster, Demand, Kind, NodeId, NodeSpec, Resources, WorkloadId, WorkloadSpec};
use crate::sim::Run;
use crate::store::{Op, State};

/// A write the serial specification couldn't have made.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Violation {
    #[error("{0:?} was submitted twice")]
    SubmittedTwice(WorkloadId),
    #[error("{0:?} was placed, completed or checked without having been submitted")]
    NotSubmitted(WorkloadId),
    #[error("{0:?} was placed while not pending")]
    NotPending(WorkloadId),
    #[error(
        "{placed:?} was placed while {ahead:?}, ahead of it in the queue, had a node that held it"
    )]
    JumpedQueue {
        placed: WorkloadId,
        ahead: WorkloadId,
    },
    #[error("{workload:?} went to {chose:?}; the specification puts it on {spec:?}")]
    NotSerialChoice {
        workload: WorkloadId,
        chose: NodeId,
        spec: Option<NodeId>,
    },
    #[error("{0:?} completed on a node it wasn't placed on")]
    CompletedElsewhere(WorkloadId),
    #[error("{0:?} was left pending while a node held it")]
    LeftPending(WorkloadId),
    #[error("the replay itself failed: {0}")]
    Replay(SimError),
}

impl From<SimError> for Violation {
    fn from(e: SimError) -> Self {
        Violation::Replay(e)
    }
}

/// The specification run directly: each workload, in order, on its sampled best fit or none.
pub fn serial_place(
    nodes: &[NodeSpec],
    workloads: &[WorkloadSpec],
    sample: &Sample,
) -> Result<Vec<Option<NodeId>>, SimError> {
    let mut index = FitIndex::new(nodes)?;
    let mut out = Vec::with_capacity(workloads.len());
    for w in workloads {
        let choice = index.sampled_fit(&w.demand(), sample).best.map(|k| k.node);
        if let Some(node) = choice {
            index.take(node, &w.demand())?;
        }
        out.push(choice);
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LogReport {
    pub submits: u64,
    pub placements: u64,
    pub completions: u64,
    /// Workloads still pending at the end that no node holds: waiting, correctly.
    pub waiting: u64,
}

/// Replays `run`'s store log against the specification. Each placement must be the best fit at that
/// point, no earlier-queued pending workload may have had a node, and nothing may be left pending
/// that a node holds at the end. Its cost is each placement times the pending workloads ahead of it
/// times the node count: an oracle for test-sized runs.
pub fn check_log(nodes: &[NodeSpec], run: &Run) -> Result<LogReport, Violation> {
    let mut index = FitIndex::new(nodes)?;
    let mut pending: BTreeMap<u64, (WorkloadId, Demand)> = BTreeMap::new();
    let mut seq_of: BTreeMap<WorkloadId, u64> = BTreeMap::new();
    let mut placed: BTreeMap<WorkloadId, NodeId> = BTreeMap::new();
    let mut report = LogReport::default();
    for entry in run.store.log() {
        match entry.op {
            Op::Submit { workload, seq } => {
                let need = run
                    .store
                    .record(workload)
                    .ok_or(Violation::NotSubmitted(workload))?
                    .spec
                    .demand();
                if seq_of.insert(workload, seq).is_some() {
                    return Err(Violation::SubmittedTwice(workload));
                }
                pending.insert(seq, (workload, need));
                report.submits = report.submits.saturating_add(1);
            }
            Op::Place { workload, node } => {
                let seq = *seq_of
                    .get(&workload)
                    .ok_or(Violation::NotSubmitted(workload))?;
                let (_, need) = *pending.get(&seq).ok_or(Violation::NotPending(workload))?;
                if let Some((_, (ahead, _))) = pending
                    .range(..seq)
                    .find(|(_, (_, d))| index.admitted_anywhere(d))
                {
                    return Err(Violation::JumpedQueue {
                        placed: workload,
                        ahead: *ahead,
                    });
                }
                let spec = index.sampled_fit(&need, &run.sample).best.map(|k| k.node);
                if spec != Some(node) {
                    return Err(Violation::NotSerialChoice {
                        workload,
                        chose: node,
                        spec,
                    });
                }
                index.take(node, &need)?;
                pending.remove(&seq);
                placed.insert(workload, node);
                report.placements = report.placements.saturating_add(1);
            }
            Op::Complete { workload, node } => {
                if placed.remove(&workload) != Some(node) {
                    return Err(Violation::CompletedElsewhere(workload));
                }
                let need = run
                    .store
                    .record(workload)
                    .ok_or(Violation::NotSubmitted(workload))?
                    .spec
                    .demand();
                index.give(node, &need)?;
                report.completions = report.completions.saturating_add(1);
            }
        }
    }
    for (workload, need) in pending.values() {
        if index.admitted_anywhere(need) {
            return Err(Violation::LeftPending(*workload));
        }
        report.waiting = report.waiting.saturating_add(1);
    }
    Ok(report)
}

/// What `AC-T7.3.3` counts, over every workload of the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EndReport {
    /// Acknowledged to its client, then never placed nor waiting, or placed and its run never
    /// recorded as done.
    pub lost: u64,
    /// Started on a node more than once.
    pub started_twice: u64,
    /// Refused at admission because no node could ever hold it.
    pub refused: u64,
    pub complete: u64,
    pub running: u64,
    pub waiting: u64,
}

impl EndReport {
    pub fn lost_or_double(&self) -> u64 {
        self.lost.saturating_add(self.started_twice)
    }
}

pub fn check_end(cluster: &Cluster, run: &Run) -> EndReport {
    let mut r = EndReport::default();
    for (at, w) in cluster.workloads.iter().enumerate() {
        let starts = run.starts.get(at).copied().unwrap_or(0);
        let acked = run.acked.get(at).copied().unwrap_or(false);
        let refused = run.refused.get(at).copied().unwrap_or(false);
        if starts > 1 {
            r.started_twice = r.started_twice.saturating_add(1);
        }
        let ok = match run.store.record(w.id).map(|rec| rec.state) {
            None => {
                r.refused = r.refused.saturating_add(u64::from(refused));
                refused && starts == 0
            }
            Some(State::Pending) => {
                r.waiting = r.waiting.saturating_add(1);
                acked && starts == 0
            }
            Some(State::Placed(_)) => {
                r.running = r.running.saturating_add(1);
                acked && starts == 1 && w.runs_for.is_none()
            }
            Some(State::Complete(_)) => {
                r.complete = r.complete.saturating_add(1);
                acked && starts == 1
            }
        };
        if !ok && starts <= 1 {
            r.lost = r.lost.saturating_add(1);
        }
    }
    r
}

/// What `AC-T7.3.4` holds: control-plane restarts leave running work running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeptReport {
    /// Workloads that ran, each started once on the node it was placed on, ran its whole run time
    /// there, and is recorded done there (or still running there, for one that never ends).
    pub kept: u64,
    /// Workloads that ran and broke any of that.
    pub disturbed: u64,
}

pub fn check_kept_running(cluster: &Cluster, run: &Run) -> KeptReport {
    let mut r = KeptReport::default();
    for (at, w) in cluster.workloads.iter().enumerate() {
        let Some(Some(ran)) = run.ran.get(at).copied() else {
            continue;
        };
        let once = run.starts.get(at).copied() == Some(1);
        let state = run.store.record(w.id).map(|rec| rec.state);
        let whole = match w.runs_for {
            Some(d) => {
                ran.started
                    .checked_add(d)
                    .is_some_and(|end| ran.ended == Some(end))
                    && state == Some(State::Complete(ran.node))
            }
            None => ran.ended.is_none() && state == Some(State::Placed(ran.node)),
        };
        if once && whole {
            r.kept = r.kept.saturating_add(1);
        } else {
            r.disturbed = r.disturbed.saturating_add(1);
        }
    }
    r
}

/// What step 2's filters hold to zero over a run's placements (`AC-T7.6.3`, S3), counted by a replay
/// of the log against the cluster's facts alone, apart from [`FitIndex`]'s own verdicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FilterReport {
    pub placements: u64,
    /// GPU work placed on a node without GPUs.
    pub gpu_off_gpu: u64,
    /// A benchmark placed beside a running build, or a build beside a running benchmark.
    pub benchmark_with_build: u64,
    /// A session placed on a node above 90% CPU while another node had the room and wasn't.
    pub hot_sessions: u64,
    /// A workload placed on a node that can't run its class.
    pub class_off_node: u64,
}

impl FilterReport {
    pub fn violations(&self) -> u64 {
        self.gpu_off_gpu
            .saturating_add(self.benchmark_with_build)
            .saturating_add(self.hot_sessions)
            .saturating_add(self.class_off_node)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Seen {
    used: Resources,
    builds: u64,
    benchmarks: u64,
}

/// Above 90%: busy * 10 > capacity * 9, in u128 so neither side can overflow.
fn over_ninety(capacity: &NodeSpec, used: &Resources) -> bool {
    let busy = u128::from(used.cpu_milli).saturating_add(u128::from(capacity.background_milli));
    busy.saturating_mul(10) > u128::from(capacity.capacity.cpu_milli).saturating_mul(9)
}

pub fn check_filters(cluster: &Cluster, run: &Run) -> Result<FilterReport, Violation> {
    let mut seen = vec![Seen::default(); cluster.nodes.len()];
    let mut r = FilterReport::default();
    let at = |node: NodeId| {
        usize::try_from(node.0).map_err(|_| Violation::Replay(SimError::UnknownNode(node)))
    };
    for entry in run.store.log() {
        let (workload, node, placing) = match entry.op {
            Op::Submit { .. } => continue,
            Op::Place { workload, node } => (workload, node, true),
            Op::Complete { workload, node } => (workload, node, false),
        };
        let spec = run
            .store
            .record(workload)
            .ok_or(Violation::NotSubmitted(workload))?
            .spec;
        let i = at(node)?;
        let facts = cluster
            .nodes
            .get(i)
            .ok_or(Violation::Replay(SimError::UnknownNode(node)))?;
        if placing {
            let now = seen.get(i).copied().unwrap_or_default();
            r.placements = r.placements.saturating_add(1);
            if spec.need.gpus > 0 && facts.capacity.gpus == 0 {
                r.gpu_off_gpu = r.gpu_off_gpu.saturating_add(1);
            }
            if !facts.classes.has(spec.class) {
                r.class_off_node = r.class_off_node.saturating_add(1);
            }
            let shares = match spec.kind {
                Kind::Benchmark => now.builds > 0,
                Kind::Build => now.benchmarks > 0,
                Kind::Plain | Kind::Session => false,
            };
            if shares {
                r.benchmark_with_build = r.benchmark_with_build.saturating_add(1);
            }
            if spec.kind == Kind::Session && over_ninety(facts, &now.used) {
                let elsewhere = cluster.nodes.iter().zip(&seen).any(|(other, o)| {
                    other.id != node
                        && other
                            .capacity
                            .checked_sub(&o.used)
                            .is_some_and(|free| free.holds(&spec.need))
                        && !over_ninety(other, &o.used)
                });
                if elsewhere {
                    r.hot_sessions = r.hot_sessions.saturating_add(1);
                }
            }
        }
        let slot = seen
            .get_mut(i)
            .ok_or(Violation::Replay(SimError::UnknownNode(node)))?;
        let step = |n: u64| {
            if placing {
                n.saturating_add(1)
            } else {
                n.saturating_sub(1)
            }
        };
        slot.used = if placing {
            slot.used.checked_add(&spec.need)
        } else {
            slot.used.checked_sub(&spec.need)
        }
        .ok_or(Violation::Replay(SimError::Resources(node)))?;
        match spec.kind {
            Kind::Build => slot.builds = step(slot.builds),
            Kind::Benchmark => slot.benchmarks = step(slot.benchmarks),
            Kind::Plain | Kind::Session => {}
        }
    }
    Ok(r)
}
