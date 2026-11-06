//! Proximity-scored placement against proximity-blind placement on a modelled WAN (scheduler design
//! S4 and A2; `hex-sim.md` H21): the goodput and completion-time tail each buys.
//!
//! **Workloads and nodes are recorded data** (`hex-sim.md` H10). [`Study::generate`] takes a recorded
//! workload, G2's of record (Huawei Cloud's Region 5, day 30, `bench/sched/huawei-r5-day30.trace`, and
//! its `.fleet` twin), and draws each job as one of its launches, uniformly: the launch's owner, its
//! kind (an OCI container, a Firecracker microVM or a Shards microVM, given by G2's rule: a kind per
//! owner, in thirds of the day's launches) and its need, its kind's overhead included as the trace
//! states it (Shards' a 5 MiB stand-in until Shards' own is measured). The study's nodes repeat the
//! recorded cluster's node rows in order, each with its capacity and the kinds it runs: ten EC2
//! m5d.metal hosts running all three kinds in the main trace, Hex's own fleet's Linux, Windows and
//! macOS hosts in the `.fleet` one. A job is placed only on a node that runs its kind.
//!
//! Each job talks to a home, the node holding its data or service, and makes its round
//! trips to it in sequence, each after a step of compute: it runs `round_trips × (step + RTT)`, the
//! RTT the WAN's true one from the node it was placed on to its home. Agent sessions are such
//! workloads: TraceLab counts 101.4 tool calls a session on average, p50 25 and p99 1,438
//! (`docs/research/scheduler/agentic-workloads.md` §1, its [5]), and tool calls are 30-85% of
//! production requests' latency (its [34]); round trips are drawn log-normal through that p50 and p99.
//! Homes sit in a region drawn uniformly, or skewed, half of them in the first region and the rest
//! spread evenly, so that proximity has to choose between near and free.
//!
//! **Policies**, each over the fast path's own sample ([`FitIndex::sampled_fit`]'s window and its
//! admitting nodes, so they see the same candidates):
//! - [`Policy::Blind`]: the fast path's best fit, proximity ignored.
//! - [`Policy::Proximity`]: the least predicted RTT to the workload's home from hyper-swim's
//!   coordinates, read as an upper bound, `rtt × (1 + K_SIGMA·(e_node + e_home)/2)`, with Vivaldi's
//!   relative errors standing in for σ (Hyperscale's `rtt_ucb = rtt_hat + K_SIGMA·σ`, `K_SIGMA = 2`,
//!   `proximity-and-failure-containment.md` §1.5); best fit breaks ties. A soft score: every admitting
//!   node stays a candidate.
//! - [`Policy::TieBreak`]: best fit, and among the nodes whose free capacity ties the best, the
//!   least predicted RTT: what S4's falsifier would leave.
//! - [`Policy::Oracle`]: as `Proximity` with the true RTT: the most a perfect coordinate could buy.
//!
//! **Measures.** Arrivals are uniform order statistics over `[0, horizon]` (a Poisson process given
//! its count), at a rate that offers `load_per_mille` of the cluster's CPU to blind placement's
//! expected occupancy. Goodput is the work finished by the horizon: workloads, and useful CPU time
//! (CPU × compute, the round trips' waits excluded). Completion time is arrival to end, every
//! workload run to its end. The queue is first come, first served: the head waits until it fits.
//! Each measure is reported for all workloads and for each kind.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};
use std::time::Duration;

use crate::error::SimError;
use crate::index::{FitIndex, Key, Sample, Verdict};
use crate::model::{Class, Cluster, Demand, Kind, NodeId, NodeSpec, OwnerId, Resources, Time};
use crate::rng::Rng;
use crate::trace::{Latencies, Quantiles, TraceHash};
use crate::wan::{Coordinates, Wan, WanConfig, node_id};

/// Hyperscale's confidence multiplier on a coordinate's error (`K_SIGMA = 2`, its [33]).
pub const K_SIGMA: f64 = 2.0;
/// TraceLab's tool calls per session, p50 and p99 (agentic-workloads.md §1, [5]).
pub const ROUND_TRIPS_P50: f64 = 25.0;
pub const ROUND_TRIPS_P99: f64 = 1_438.0;
/// The standard normal's 99th percentile.
const Z99: f64 = 2.326_347_874;
/// The most round trips a workload makes: ten times TraceLab's p99, a bound, not a measurement.
pub const MAX_ROUND_TRIPS: u64 = 14_380;
/// The most workloads a study runs; the queue and the event heap are bounded by it.
pub const MAX_WORKLOADS: usize = 1 << 20;
/// Hex's three kinds, in the order reports give them.
pub const CLASSES: [Class; 3] = [Class::Oci, Class::Firecracker, Class::Shards];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Policy {
    Blind,
    Proximity,
    TieBreak,
    Oracle,
}

/// Where workloads' homes are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Homes {
    /// A region drawn uniformly.
    Uniform,
    /// Half in the first region, the rest uniformly over all of them.
    Skewed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StudyConfig {
    pub wan: WanConfig,
    pub workloads: usize,
    /// Offered CPU, per mille of the cluster's, against blind placement's expected occupancy.
    pub load_per_mille: u64,
    /// Compute before each round trip.
    pub step_ns: Time,
    pub homes: Homes,
    pub sample: Sample,
    pub seed: u64,
}

impl Default for StudyConfig {
    /// 400 nodes over the eight regions; 5,000 workloads at 90% load, 100 ms of compute a step.
    /// Chosen for test runs, not measured.
    fn default() -> Self {
        Self {
            wan: WanConfig {
                nodes: 400,
                ..WanConfig::default()
            },
            workloads: 5_000,
            load_per_mille: 900,
            step_ns: 100_000_000,
            homes: Homes::Uniform,
            sample: Sample::default(),
            seed: 1,
        }
    }
}

/// One workload of a study.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Job {
    /// What its recorded launch asked, its kind's overhead included.
    pub need: Resources,
    pub owner: OwnerId,
    pub class: Class,
    pub home: NodeId,
    pub round_trips: u64,
    pub arrives: Time,
}

/// A study's cluster: its WAN, the coordinates its nodes learned, and its workloads, shared by every
/// policy run over it.
pub struct Study {
    pub config: StudyConfig,
    pub wan: Wan,
    pub coordinates: Coordinates,
    pub nodes: Vec<NodeSpec>,
    pub jobs: Vec<Job>,
    pub horizon: Time,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PolicyReport {
    pub policy: Policy,
    pub placements: u64,
    /// Decisions whose node differed from the one blind placement would have chosen then.
    pub moved: u64,
    /// Workloads, and useful CPU time (milli-CPU nanoseconds), finished by the horizon.
    pub done_by_horizon: u64,
    pub useful_by_horizon: u128,
    pub horizon: Time,
    /// Arrival to end, every workload.
    pub completion: Option<Quantiles>,
    /// The same two measures for each kind, in [`CLASSES`]' order.
    pub classes: [ClassReport; 3],
    /// The true RTT to home of each placement, summed.
    pub rtt_sum_ns: u128,
    /// The longest the queue grew.
    pub queue_peak: usize,
    pub hash: u64,
}

/// One kind's share of a [`PolicyReport`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ClassReport {
    pub placements: u64,
    pub done_by_horizon: u64,
    pub completion: Option<Quantiles>,
}

impl PolicyReport {
    /// One kind's workloads finished by the horizon per second of it.
    pub fn class_goodput_per_s(&self, class: Class) -> f64 {
        self.classes.get(class_at(class)).map_or(0.0, |c| {
            c.done_by_horizon as f64 / (self.horizon as f64 / 1e9)
        })
    }

    /// Workloads finished by the horizon per second of it.
    pub fn goodput_per_s(&self) -> f64 {
        self.done_by_horizon as f64 / (self.horizon as f64 / 1e9)
    }

    /// Useful CPU cores kept busy by work finished by the horizon, averaged over it.
    pub fn useful_cores(&self) -> f64 {
        self.useful_by_horizon as f64 / 1e3 / self.horizon as f64
    }

    pub fn mean_rtt_ms(&self) -> f64 {
        if self.placements == 0 {
            return 0.0;
        }
        self.rtt_sum_ns as f64 / self.placements as f64 / 1e6
    }
}

impl Study {
    /// A study whose nodes repeat `recorded`'s and whose jobs are its launches, drawn uniformly.
    pub fn generate(config: &StudyConfig, recorded: &Cluster) -> Result<Self, SimError> {
        if config.workloads == 0 || config.workloads > MAX_WORKLOADS {
            return Err(SimError::Config(
                "a study runs 1 to MAX_WORKLOADS workloads",
            ));
        }
        if config.load_per_mille == 0 || config.step_ns == 0 {
            return Err(SimError::Config(
                "a study offers some load, steps of some compute",
            ));
        }
        if recorded.nodes.is_empty() || recorded.workloads.is_empty() {
            return Err(SimError::Config(
                "a recorded workload with nodes and launches",
            ));
        }
        let mut rng = Rng::new(config.seed);
        let wan = Wan::generate(&config.wan, &mut rng)?;
        let coordinates = Coordinates::converge(&wan, &config.wan, &mut rng)?;
        let mut nodes = Vec::with_capacity(wan.len());
        for (at, row) in (0..wan.len()).zip(recorded.nodes.iter().cycle()) {
            nodes.push(NodeSpec {
                id: node_id(at)?,
                ..*row
            });
        }
        // Each region's nodes, to draw a home in a region.
        let regions = config.wan.regions;
        let mut by_region: Vec<Vec<NodeId>> = vec![Vec::new(); regions];
        for node in &nodes {
            let site = wan.site(node.id)?;
            by_region
                .get_mut(site.region)
                .ok_or(SimError::UnknownNode(node.id))?
                .push(node.id);
        }
        let sigma = (ROUND_TRIPS_P99 / ROUND_TRIPS_P50).ln() / Z99;
        let last_launch = u64::try_from(recorded.workloads.len())
            .ok()
            .and_then(|n| n.checked_sub(1))
            .ok_or(SimError::Config("launches"))?;
        let mut jobs = Vec::with_capacity(config.workloads);
        // Blind placement's expected occupancy: milli-CPU nanoseconds over all jobs, each with the
        // mean RTT from its home to every node that runs its kind.
        let mut occupancy: u128 = 0;
        let last_region = u64::try_from(regions)
            .ok()
            .and_then(|r| r.checked_sub(1))
            .ok_or(SimError::Config("regions"))?;
        for _ in 0..config.workloads {
            let launch = recorded
                .workloads
                .get(
                    usize::try_from(rng.range(0, last_launch))
                        .map_err(|_| SimError::Config("launches"))?,
                )
                .ok_or(SimError::Config("launches"))?;
            let region = match config.homes {
                Homes::Skewed if rng.chance(500) => 0,
                Homes::Uniform | Homes::Skewed => rng.range(0, last_region),
            };
            let members = by_region
                .get(usize::try_from(region).map_err(|_| SimError::Config("regions"))?)
                .ok_or(SimError::Config("regions"))?;
            let last = u64::try_from(members.len())
                .ok()
                .and_then(|n| n.checked_sub(1))
                .ok_or(SimError::Config("a region with no nodes"))?;
            let home = *members
                .get(usize::try_from(rng.range(0, last)).map_err(|_| SimError::Config("ids"))?)
                .ok_or(SimError::Config("regions"))?;
            let z = normal(&mut rng);
            let round_trips = whole(ROUND_TRIPS_P50 * (sigma * z).exp())
                .unwrap_or(MAX_ROUND_TRIPS)
                .clamp(1, MAX_ROUND_TRIPS);
            let mean_rtt = mean_rtt_ns(&wan, &nodes, home, launch.class)?;
            let run = run_ns(round_trips, config.step_ns, mean_rtt)?;
            occupancy = occupancy
                .checked_add(u128::from(run).saturating_mul(u128::from(launch.need.cpu_milli)))
                .ok_or(SimError::TimeOverflow)?;
            jobs.push(Job {
                need: launch.need,
                owner: launch.tenancy.owner,
                class: launch.class,
                home,
                round_trips,
                arrives: 0,
            });
        }
        // horizon = occupancy / (capacity × load).
        let cpu: u128 = nodes.iter().map(|n| u128::from(n.capacity.cpu_milli)).sum();
        let capacity = cpu
            .checked_mul(u128::from(config.load_per_mille))
            .ok_or(SimError::TimeOverflow)?;
        let horizon = occupancy
            .checked_mul(1_000)
            .and_then(|o| o.checked_div(capacity))
            .and_then(|h| Time::try_from(h).ok())
            .ok_or(SimError::TimeOverflow)?;
        let mut arrivals: Vec<Time> = (0..jobs.len()).map(|_| rng.range(0, horizon)).collect();
        arrivals.sort_unstable();
        for (job, at) in jobs.iter_mut().zip(arrivals) {
            job.arrives = at;
        }
        Ok(Self {
            config: *config,
            wan,
            coordinates,
            nodes,
            jobs,
            horizon,
        })
    }

    /// Runs every job under `policy` to its end.
    pub fn run(&self, policy: Policy) -> Result<PolicyReport, SimError> {
        let mut index = FitIndex::new(&self.nodes)?;
        let mut report = PolicyReport {
            policy,
            placements: 0,
            moved: 0,
            done_by_horizon: 0,
            useful_by_horizon: 0,
            horizon: self.horizon,
            completion: None,
            classes: [ClassReport::default(); 3],
            rtt_sum_ns: 0,
            queue_peak: 0,
            hash: 0,
        };
        let mut hash = TraceHash::default();
        let mut queue: VecDeque<usize> = VecDeque::with_capacity(self.jobs.len());
        // Ends: (time, job, node). Bounded by the jobs, each placed once.
        let mut ends: BinaryHeap<Reverse<(Time, usize, NodeId)>> =
            BinaryHeap::with_capacity(self.jobs.len());
        let mut completion = Vec::with_capacity(self.jobs.len());
        let mut by_class: [Vec<Time>; 3] = Default::default();
        let mut next = 0usize;
        // Each job arrives once and ends once: the loop's turns are bounded by twice the jobs.
        let budget = self.jobs.len().saturating_mul(2).saturating_add(1);
        for _ in 0..budget {
            let arrival = self.jobs.get(next).map(|j| j.arrives);
            let end = ends.peek().map(|Reverse((t, _, _))| *t);
            let now = match (arrival, end) {
                (None, None) => break,
                // An end at the same time as an arrival goes first: its room is free for it.
                (Some(a), Some(e)) if e <= a => self.end(&mut ends, &mut index)?,
                (Some(a), _) => {
                    queue.push_back(next);
                    next = next.saturating_add(1);
                    report.queue_peak = report.queue_peak.max(queue.len());
                    a
                }
                (None, Some(_)) => self.end(&mut ends, &mut index)?,
            };
            // Place from the head while it fits.
            while let Some(&job) = queue.front() {
                let spec = self.jobs.get(job).ok_or(SimError::Config("jobs"))?;
                let demand = demand(spec);
                let Some(node) = self.choose(policy, &index, &demand, spec.home, &mut report)?
                else {
                    break;
                };
                queue.pop_front();
                index.take(node, &demand)?;
                let rtt = self.wan.rtt_ns(node, spec.home)?;
                let ends_at = now
                    .checked_add(run_ns(spec.round_trips, self.config.step_ns, rtt)?)
                    .ok_or(SimError::TimeOverflow)?;
                ends.push(Reverse((ends_at, job, node)));
                let took = ends_at
                    .checked_sub(spec.arrives)
                    .ok_or(SimError::TimeOverflow)?;
                completion.push(took);
                let at = class_at(spec.class);
                by_class
                    .get_mut(at)
                    .ok_or(SimError::Config("classes"))?
                    .push(took);
                let class = report
                    .classes
                    .get_mut(at)
                    .ok_or(SimError::Config("classes"))?;
                class.placements = class.placements.saturating_add(1);
                if ends_at <= self.horizon {
                    class.done_by_horizon = class.done_by_horizon.saturating_add(1);
                }
                report.placements = report.placements.saturating_add(1);
                report.rtt_sum_ns = report.rtt_sum_ns.saturating_add(u128::from(rtt));
                if ends_at <= self.horizon {
                    report.done_by_horizon = report.done_by_horizon.saturating_add(1);
                    let compute = u128::from(spec.round_trips)
                        .saturating_mul(u128::from(self.config.step_ns))
                        .saturating_mul(u128::from(spec.need.cpu_milli));
                    report.useful_by_horizon = report.useful_by_horizon.saturating_add(compute);
                }
                for word in [
                    now,
                    u64::try_from(job).unwrap_or(u64::MAX),
                    u64::from(node.0),
                ] {
                    hash.word(word);
                }
            }
        }
        if !queue.is_empty() || next < self.jobs.len() || !ends.is_empty() {
            return Err(SimError::StepBudget(
                u64::try_from(budget).unwrap_or(u64::MAX),
            ));
        }
        report.completion = Latencies::new(completion).quantiles();
        for (class, times) in report.classes.iter_mut().zip(by_class) {
            class.completion = Latencies::new(times).quantiles();
        }
        report.hash = hash.finish();
        Ok(report)
    }

    fn end(
        &self,
        ends: &mut BinaryHeap<Reverse<(Time, usize, NodeId)>>,
        index: &mut FitIndex,
    ) -> Result<Time, SimError> {
        let Reverse((at, job, node)) = ends.pop().ok_or(SimError::Config("no end to take"))?;
        let spec = self.jobs.get(job).ok_or(SimError::Config("jobs"))?;
        index.give(node, &demand(spec))?;
        Ok(at)
    }

    /// The node `policy` places `demand` on now, or none while no sampled node admits it.
    fn choose(
        &self,
        policy: Policy,
        index: &FitIndex,
        demand: &Demand,
        home: NodeId,
        report: &mut PolicyReport,
    ) -> Result<Option<NodeId>, SimError> {
        let sampled = index.sampled_fit(demand, &self.config.sample);
        let Some(best) = sampled.best else {
            return Ok(None);
        };
        if policy == Policy::Blind {
            return Ok(Some(best.node));
        }
        let n = index.len();
        // (score, key): the least wins.
        let mut chosen: Option<(f64, Key)> = None;
        for offset in 0..sampled.span {
            let at = sampled
                .start
                .checked_add(offset)
                .and_then(|x| x.checked_rem(n))
                .ok_or(SimError::Config("window"))?;
            let node = node_id(at)?;
            if index.verdict(node, demand)? != Verdict::Admits {
                continue;
            }
            let key = index.key(node)?;
            let score = match policy {
                Policy::Blind => 0.0,
                Policy::Proximity => self.upper_bound(node, home)?,
                Policy::Oracle => self.wan.rtt_ns(node, home)? as f64 / 1e9,
                Policy::TieBreak => {
                    if (key.cpu_milli, key.mem_bytes, key.gpus)
                        != (best.cpu_milli, best.mem_bytes, best.gpus)
                    {
                        continue;
                    }
                    self.upper_bound(node, home)?
                }
            };
            let wins = chosen.is_none_or(|(s, k)| score < s || (score == s && key < k));
            if wins {
                chosen = Some((score, key));
            }
        }
        let node = chosen.map_or(best.node, |(_, k)| k.node);
        if node != best.node {
            report.moved = report.moved.saturating_add(1);
        }
        Ok(Some(node))
    }

    /// The predicted RTT from `node` to `home`, seconds, widened by both coordinates' errors.
    fn upper_bound(&self, node: NodeId, home: NodeId) -> Result<f64, SimError> {
        let predicted = self.coordinates.predict(node, home)?;
        let error = (self.coordinates.coordinate(node)?.error
            + self.coordinates.coordinate(home)?.error)
            / 2.0;
        Ok(predicted * (1.0 + K_SIGMA * error))
    }
}

/// `round_trips × (step + rtt)`.
fn run_ns(round_trips: u64, step_ns: Time, rtt_ns: Time) -> Result<Time, SimError> {
    step_ns
        .checked_add(rtt_ns)
        .and_then(|per| per.checked_mul(round_trips))
        .ok_or(SimError::TimeOverflow)
}

/// What a job asks of the index: its need, as one of its kind.
fn demand(job: &Job) -> Demand {
    Demand {
        need: job.need,
        kind: Kind::Plain,
        class: job.class,
    }
}

/// Where `class` sits in [`CLASSES`] and a report's `classes`.
fn class_at(class: Class) -> usize {
    match class {
        Class::Oci => 0,
        Class::Firecracker => 1,
        Class::Shards => 2,
    }
}

/// The mean true RTT from `home` to every node that runs `class`.
fn mean_rtt_ns(
    wan: &Wan,
    nodes: &[NodeSpec],
    home: NodeId,
    class: Class,
) -> Result<Time, SimError> {
    let mut sum: u128 = 0;
    let mut count: u128 = 0;
    for node in nodes.iter().filter(|n| n.classes.has(class)) {
        sum = sum.saturating_add(u128::from(wan.rtt_ns(home, node.id)?));
        count = count.saturating_add(1);
    }
    sum.checked_div(count)
        .and_then(|m| Time::try_from(m).ok())
        .ok_or(SimError::Config("a kind no node runs"))
}

/// A standard normal draw (Box and Muller, 1958).
fn normal(rng: &mut Rng) -> f64 {
    let unit = |rng: &mut Rng| {
        // 53 random bits, in (0, 1].
        ((rng.next_u64() >> 11) as f64 + 1.0) / (1u64 << 53) as f64
    };
    let (u, v) = (unit(rng), unit(rng));
    (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
}

/// `x` rounded down to a whole number, or none when it is negative, not a number or past u64. Clippy
/// denies `as` from a float to an integer; `Duration`'s checked conversion reads seconds the same way.
fn whole(x: f64) -> Option<u64> {
    Duration::try_from_secs_f64(x).ok().map(|d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Classes;

    const POLICIES: [Policy; 4] = [
        Policy::Blind,
        Policy::Proximity,
        Policy::TieBreak,
        Policy::Oracle,
    ];

    /// H10's two node models, each G2's recorded day on its own hosts.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Fleet {
        /// `huawei-r5-day30.trace`: ten EC2 m5d.metal hosts, each running all three kinds.
        AllKinds,
        /// `huawei-r5-day30.fleet.trace`: Hex's own fleet's hosts, some running only some kinds.
        Mixed,
    }

    fn recorded(fleet: Fleet) -> &'static Cluster {
        static ALL: std::sync::OnceLock<Cluster> = std::sync::OnceLock::new();
        static MIXED: std::sync::OnceLock<Cluster> = std::sync::OnceLock::new();
        let (cell, text) = match fleet {
            Fleet::AllKinds => (
                &ALL,
                include_str!("../../../bench/sched/huawei-r5-day30.trace"),
            ),
            Fleet::Mixed => (
                &MIXED,
                include_str!("../../../bench/sched/huawei-r5-day30.fleet.trace"),
            ),
        };
        cell.get_or_init(|| crate::replay::parse(text, &crate::replay::Limits::default()).unwrap())
    }

    fn generate(config: &StudyConfig, fleet: Fleet) -> Study {
        Study::generate(config, recorded(fleet)).unwrap()
    }

    fn small(seed: u64, load_per_mille: u64, homes: Homes) -> StudyConfig {
        StudyConfig {
            wan: WanConfig {
                nodes: 200,
                rounds: 48,
                seed,
                ..WanConfig::default()
            },
            workloads: 3_000,
            load_per_mille,
            homes,
            seed,
            ..StudyConfig::default()
        }
    }

    /// S4, A2 (G3 step 2): do run every policy over one study twice; expect the same report twice,
    /// every job placed once, the queue drained, and completion quantiles for all of them.
    #[test]
    fn every_policy_runs_every_job_deterministically() {
        let study = generate(&small(5, 900, Homes::Uniform), Fleet::AllKinds);
        let again = generate(&small(5, 900, Homes::Uniform), Fleet::AllKinds);
        assert_eq!(study.jobs, again.jobs);
        for policy in POLICIES {
            let a = study.run(policy).unwrap();
            let b = again.run(policy).unwrap();
            assert_eq!(a, b, "{policy:?}");
            assert_eq!(a.placements, 3_000);
            assert_eq!(a.completion.unwrap().count, 3_000);
            assert!(a.done_by_horizon > 0 && a.done_by_horizon < 3_000);
            for (class, got) in CLASSES.iter().zip(&a.classes) {
                let jobs = study.jobs.iter().filter(|j| j.class == *class).count();
                assert!(jobs > 0, "{class:?}");
                assert_eq!(got.placements, u64::try_from(jobs).unwrap(), "{class:?}");
                assert_eq!(got.completion.unwrap().count, jobs, "{class:?}");
            }
            assert_eq!(
                a.classes.iter().map(|c| c.done_by_horizon).sum::<u64>(),
                a.done_by_horizon
            );
        }
    }

    /// AC-T7.4.3's kinds (H10), S4: do generate a study on each recorded fleet; expect every job to
    /// be one of the recorded day's launches (its owner, kind and need, overhead included), every node
    /// the recorded cluster's row at its position, repeated, and on the fleet trace some nodes that
    /// run only some kinds, every kind on some node.
    #[test]
    fn jobs_and_nodes_are_the_recorded_ones() {
        for fleet in [Fleet::AllKinds, Fleet::Mixed] {
            let rec = recorded(fleet);
            let launches: std::collections::BTreeSet<_> = rec
                .workloads
                .iter()
                .map(|w| (w.tenancy.owner, w.class, w.need))
                .collect();
            let study = generate(&small(3, 900, Homes::Uniform), fleet);
            for job in &study.jobs {
                assert!(
                    launches.contains(&(job.owner, job.class, job.need)),
                    "{fleet:?}"
                );
            }
            for (at, node) in study.nodes.iter().enumerate() {
                let row = rec.nodes[at % rec.nodes.len()];
                assert_eq!((node.capacity, node.classes), (row.capacity, row.classes));
            }
            for class in CLASSES {
                assert!(
                    study.nodes.iter().any(|n| n.classes.has(class)),
                    "{fleet:?} {class:?}"
                );
            }
        }
        assert!(
            recorded(Fleet::Mixed)
                .nodes
                .iter()
                .any(|n| n.classes != Classes::ALL)
        );
        assert!(
            recorded(Fleet::AllKinds)
                .nodes
                .iter()
                .all(|n| n.classes == Classes::ALL)
        );
    }

    /// AC-T7.4.3's kinds (H10): do read the recorded day; expect G2's rule in it: each owner's
    /// launches one kind, the kinds in the thirds the trace's header records (oci 8432, firecracker
    /// 8432, shards 8431); and a study drawn from it keeps each owner's jobs one kind.
    #[test]
    fn kinds_are_g2s_by_owner_in_thirds() {
        let mut kind_of = std::collections::HashMap::new();
        let mut got = [0u64; 3];
        for w in &recorded(Fleet::AllKinds).workloads {
            assert_eq!(*kind_of.entry(w.tenancy.owner).or_insert(w.class), w.class);
            got[class_at(w.class)] += 1;
        }
        assert_eq!(got, [8432, 8432, 8431]);
        let study = generate(&small(7, 900, Homes::Uniform), Fleet::AllKinds);
        for job in &study.jobs {
            assert_eq!(kind_of[&job.owner], job.class);
        }
    }

    /// AC-T7.4.3's kinds (H10), S4: do run a study in which no node runs Firecracker microVMs;
    /// expect every policy to refuse it with a typed error, the Firecracker workloads left queued,
    /// never placed on a node that can't run them.
    #[test]
    fn a_kind_no_node_runs_is_refused_not_misplaced() {
        let mut study = generate(&small(4, 500, Homes::Uniform), Fleet::Mixed);
        for node in &mut study.nodes {
            node.classes = Classes::NONE.with(Class::Oci).with(Class::Shards);
        }
        for policy in POLICIES {
            assert!(
                matches!(study.run(policy), Err(SimError::StepBudget(_))),
                "{policy:?}"
            );
        }
    }

    /// S4, A2 (G3 step 2): do run blind placement; expect it never to count a decision as moved
    /// from itself. (That each other policy's effect can fail its test is shown by its mutants alone:
    /// `no-proximity-term` fails `proximity_beats_blind_on_every_seed`; `no-tie-break-term` and
    /// `random-tie-break` fail `the_tie_break_takes_the_nearest_of_best_fits_ties`.)
    #[test]
    fn blind_never_moves_from_itself() {
        let study = generate(&small(6, 900, Homes::Skewed), Fleet::AllKinds);
        assert_eq!(study.run(Policy::Blind).unwrap().moved, 0);
    }

    /// S4, A2 (G3 step 2): do run blind placement, proximity and the oracle on three seeds, homes
    /// uniform and skewed; expect proximity and the oracle each to place nearer home on average than
    /// blind on every one. (How near proximity comes to the oracle is measured, not asserted:
    /// `bench/sched/proximity.md`.)
    #[test]
    fn proximity_and_the_oracle_place_nearer_than_blind() {
        for seed in 1..=3 {
            for homes in [Homes::Uniform, Homes::Skewed] {
                let study = generate(&small(seed, 500, homes), Fleet::AllKinds);
                let [blind, near, _, oracle] = POLICIES.map(|p| study.run(p).unwrap());
                assert!(near.mean_rtt_ms() < blind.mean_rtt_ms(), "{seed} {homes:?}");
                assert!(
                    oracle.mean_rtt_ms() < blind.mean_rtt_ms(),
                    "{seed} {homes:?}"
                );
            }
        }
    }

    /// S4, A2 (G3 step 2), an oracle test: do place a study's jobs one after another on each
    /// recorded fleet, and at every decision find best fit's exact ties in the fast path's window
    /// independently; expect the tie-break to choose the one of them with the least predicted RTT to
    /// the job's home (the lower key among equals). Mutants `no-tie-break-term` (its score 0) and `random-tie-break` (a node's hash, not
    /// its RTT) fail it. Whether the tie-break then places nearer than blind over a whole run is
    /// measured, not asserted (`bench/sched/proximity.md`: on 116 of 120 studies).
    #[test]
    fn the_tie_break_takes_the_nearest_of_best_fits_ties() {
        for fleet in [Fleet::AllKinds, Fleet::Mixed] {
            let study = generate(&small(1, 900, Homes::Uniform), fleet);
            let mut report = study.run(Policy::Blind).unwrap();
            let mut index = FitIndex::new(&study.nodes).unwrap();
            let mut decisions = 0u64;
            for job in &study.jobs {
                let demand = demand(job);
                let sampled = index.sampled_fit(&demand, &study.config.sample);
                let Some(best) = sampled.best else {
                    break;
                };
                let mut want: Option<(f64, Key)> = None;
                for offset in 0..sampled.span {
                    let node = node_id((sampled.start + offset) % index.len()).unwrap();
                    if index.verdict(node, &demand).unwrap() != Verdict::Admits {
                        continue;
                    }
                    let key = index.key(node).unwrap();
                    if (key.cpu_milli, key.mem_bytes, key.gpus)
                        != (best.cpu_milli, best.mem_bytes, best.gpus)
                    {
                        continue;
                    }
                    let score = study.upper_bound(node, job.home).unwrap();
                    if want.is_none_or(|(s, k)| score < s || (score == s && key < k)) {
                        want = Some((score, key));
                    }
                }
                let got = study
                    .choose(Policy::TieBreak, &index, &demand, job.home, &mut report)
                    .unwrap()
                    .unwrap();
                assert_eq!(got, want.unwrap().1.node, "{fleet:?} decision {decisions}");
                index.take(got, &demand).unwrap();
                decisions += 1;
            }
            assert!(decisions > 0, "{fleet:?}");
        }
    }

    /// S4, A2 (G3 step 3): do run blind and proximity-scored placement of Hex's three kinds on five
    /// seeds, both fleets, homes uniform and skewed, at 50% and 90% load; expect proximity's goodput
    /// higher and its p99 completion time lower on every one, over all workloads and for each kind,
    /// the direction `bench/sched/proximity.md` records at full size. Mutant `no-proximity-term`
    /// (proximity scored as 0) fails it.
    #[test]
    fn proximity_beats_blind_on_every_seed() {
        for seed in 1..=5 {
            for fleet in [Fleet::AllKinds, Fleet::Mixed] {
                for homes in [Homes::Uniform, Homes::Skewed] {
                    for load in [500, 900] {
                        let study = generate(&small(seed, load, homes), fleet);
                        let blind = study.run(Policy::Blind).unwrap();
                        let near = study.run(Policy::Proximity).unwrap();
                        let at = format!("seed {seed} {fleet:?} {homes:?} load {load}");
                        assert!(near.goodput_per_s() > blind.goodput_per_s(), "{at}");
                        assert!(
                            near.completion.unwrap().p99 < blind.completion.unwrap().p99,
                            "{at}"
                        );
                        for (class, (b, n)) in
                            CLASSES.iter().zip(blind.classes.iter().zip(&near.classes))
                        {
                            assert!(n.done_by_horizon > b.done_by_horizon, "{at} {class:?}");
                            assert!(
                                n.completion.unwrap().p99 < b.completion.unwrap().p99,
                                "{at} {class:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn hostile_configurations_are_refused() {
        for bad in [
            StudyConfig {
                workloads: 0,
                ..StudyConfig::default()
            },
            StudyConfig {
                load_per_mille: 0,
                ..StudyConfig::default()
            },
            StudyConfig {
                step_ns: 0,
                ..StudyConfig::default()
            },
        ] {
            assert!(matches!(
                Study::generate(&bad, recorded(Fleet::AllKinds)),
                Err(SimError::Config(_))
            ));
        }
        let empty = Cluster {
            workloads: Vec::new(),
            ..recorded(Fleet::AllKinds).clone()
        };
        assert!(matches!(
            Study::generate(&StudyConfig::default(), &empty),
            Err(SimError::Config(_))
        ));
    }
}
