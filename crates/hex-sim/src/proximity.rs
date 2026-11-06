//! Proximity-scored placement against proximity-blind placement on a modelled WAN (scheduler design
//! S4 and A2; `hex-sim.md` H21): the goodput and completion-time tail each buys.
//!
//! **Workloads** are Hex's three kinds (`hex-sim.md` H10): each is an OCI container, a Firecracker
//! microVM or a Shards microVM, by G2's rule (`bench/sched/huawei_trace.py:31-35`, [`classes_by_owner`]):
//! a workload's kind is its owner's, and owners go to kinds so each kind has a third of the launches.
//! Each workload's owner is drawn in proportion to the launches of the recorded workload's owners, and
//! its memory need carries its kind's runtime overhead as that workload states it (`Cluster::overheads`):
//! [`Study::generate`] takes it, and the bench and the tests pass G2's of record, Huawei Cloud's Region
//! 5, day 30 (`bench/sched/huawei-r5-day30.trace`: 339 owners; OCI 0.42 MiB, Firecracker 5 MiB, Shards
//! 5 MiB as a stand-in until Shards' own is measured). Each workload is placed
//! only on a node that runs its kind. Nodes are H10's two models: every node Linux with KVM running
//! all three ([`Fleet::AllKinds`], the main one), or Hex's fleet's mix of Linux, Windows and macOS
//! hosts ([`Fleet::Mixed`]). Each workload talks to a home, the node holding its data or service, and makes its round
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
use crate::model::{
    Class, Classes, Cluster, Demand, Kind, NodeId, NodeSpec, OwnerId, Resources, Time,
};
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
/// The most owners a study draws from, as `replay::Limits::max_owners`.
pub const MAX_OWNERS: usize = 100_000;
/// Hex's three kinds, in the order reports give them.
pub const CLASSES: [Class; 3] = [Class::Oci, Class::Firecracker, Class::Shards];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Policy {
    Blind,
    Proximity,
    TieBreak,
    Oracle,
}

/// Which kinds each node runs (`hex-sim.md` H10's two node models).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fleet {
    /// Every node Linux x86_64 with KVM, running all three kinds.
    AllKinds,
    /// Hex's fleet's mix, drawn per node: six in ten Linux (all three kinds), two x86_64 Windows (OCI
    /// containers and Shards microVMs), two arm64 macOS (Shards microVMs alone).
    Mixed,
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
    /// Each node's capacity.
    pub node: Resources,
    pub workloads: usize,
    /// Offered CPU, per mille of the cluster's, against blind placement's expected occupancy.
    pub load_per_mille: u64,
    /// Compute before each round trip.
    pub step_ns: Time,
    pub homes: Homes,
    pub fleet: Fleet,
    pub sample: Sample,
    pub seed: u64,
}

impl Default for StudyConfig {
    /// 400 nodes of 16 cores and 64 GiB over the eight regions; 5,000 workloads at 90% load, 100 ms
    /// of compute a step. Chosen for test runs, not measured.
    fn default() -> Self {
        Self {
            wan: WanConfig {
                nodes: 400,
                ..WanConfig::default()
            },
            node: Resources {
                cpu_milli: 16_000,
                mem_bytes: 64 << 30,
                gpus: 0,
            },
            workloads: 5_000,
            load_per_mille: 900,
            step_ns: 100_000_000,
            homes: Homes::Uniform,
            fleet: Fleet::AllKinds,
            sample: Sample::default(),
            seed: 1,
        }
    }
}

/// One workload of a study.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Job {
    /// What it asks, its kind's overhead included.
    pub need: Resources,
    pub owner: OwnerId,
    /// Its owner's kind ([`classes_by_owner`]).
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
    /// A study whose workloads' owners are drawn in proportion to their launches in `recorded`, and
    /// whose kinds' overheads are `recorded`'s.
    pub fn generate(config: &StudyConfig, recorded: &Cluster) -> Result<Self, SimError> {
        if config.workloads == 0 || config.workloads > MAX_WORKLOADS {
            return Err(SimError::Config(
                "a study runs 1 to MAX_WORKLOADS workloads",
            ));
        }
        let mut owner_launches: Vec<u64> = Vec::new();
        for workload in &recorded.workloads {
            let at = usize::try_from(workload.tenancy.owner.0)
                .ok()
                .filter(|&at| at < MAX_OWNERS)
                .ok_or(SimError::Config(
                    "a study draws from at most MAX_OWNERS owners",
                ))?;
            if owner_launches.len() <= at {
                owner_launches.resize(at.saturating_add(1), 0);
            }
            if let Some(n) = owner_launches.get_mut(at) {
                *n = n.saturating_add(1);
            }
        }
        // Cumulative weights, to draw an owner by its share.
        let mut cumulative = Vec::with_capacity(owner_launches.len());
        let mut total: u64 = 0;
        for &launches in &owner_launches {
            total = total
                .checked_add(launches)
                .ok_or(SimError::Config("owner launches"))?;
            cumulative.push(total);
        }
        if total == 0 {
            return Err(SimError::Config("a recorded workload of no launches"));
        }
        if config.load_per_mille == 0 || config.step_ns == 0 {
            return Err(SimError::Config(
                "a study offers some load, steps of some compute",
            ));
        }
        let mut rng = Rng::new(config.seed);
        let wan = Wan::generate(&config.wan, &mut rng)?;
        let coordinates = Coordinates::converge(&wan, &config.wan, &mut rng)?;
        let mut nodes = Vec::with_capacity(wan.len());
        for at in 0..wan.len() {
            let classes = match config.fleet {
                Fleet::AllKinds => Classes::ALL,
                Fleet::Mixed => match rng.range(0, 9) {
                    0..6 => Classes::ALL,
                    6..8 => Classes::NONE.with(Class::Oci).with(Class::Shards),
                    _ => Classes::NONE.with(Class::Shards),
                },
            };
            nodes.push(NodeSpec {
                id: node_id(at)?,
                capacity: config.node,
                background_milli: 0,
                classes,
            });
        }
        let overheads = recorded.overheads;
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
        let cores = config.node.cpu_milli.checked_div(1_000).unwrap_or(0);
        let mut jobs = Vec::with_capacity(config.workloads);
        // Blind placement's expected occupancy: milli-CPU nanoseconds over all jobs, each with the
        // mean RTT from its home to every node.
        let mut occupancy: u128 = 0;
        let last_region = u64::try_from(regions)
            .ok()
            .and_then(|r| r.checked_sub(1))
            .ok_or(SimError::Config("regions"))?;
        for _ in 0..config.workloads {
            let draw = rng.range(0, total.saturating_sub(1));
            let owner = cumulative.partition_point(|&c| c <= draw);
            let cpu = rng.range(1, cores.clamp(1, 4)).saturating_mul(1_000);
            let need = Resources {
                cpu_milli: cpu,
                // 4 GiB a core; its kind's overhead is added once the kinds are known.
                mem_bytes: cpu.checked_div(1_000).unwrap_or(0).saturating_mul(4 << 30),
                gpus: 0,
            };
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
            jobs.push(Job {
                need,
                owner: OwnerId(u32::try_from(owner).map_err(|_| SimError::Config("owners"))?),
                // Its owner's kind, set below once every owner's launches are counted.
                class: Class::Oci,
                home,
                round_trips,
                arrives: 0,
            });
        }
        // Each owner's kind, by its launches in this study, then each job's need with its kind's
        // overhead, and blind placement's occupancy over the nodes that run its kind.
        let mut launches = vec![0u64; owner_launches.len()];
        for job in &jobs {
            let n = launches
                .get_mut(usize::try_from(job.owner.0).map_err(|_| SimError::Config("owners"))?)
                .ok_or(SimError::Config("owners"))?;
            *n = n.saturating_add(1);
        }
        let classes = classes_by_owner(&launches);
        for job in &mut jobs {
            job.class = *classes
                .get(usize::try_from(job.owner.0).map_err(|_| SimError::Config("owners"))?)
                .ok_or(SimError::Config("owners"))?;
            job.need.mem_bytes = job
                .need
                .mem_bytes
                .checked_add(overheads.of(job.class))
                .ok_or(SimError::Config("memory"))?;
            let mean_rtt = mean_rtt_ns(&wan, &nodes, job.home, job.class)?;
            let run = run_ns(job.round_trips, config.step_ns, mean_rtt)?;
            occupancy = occupancy
                .checked_add(u128::from(run).saturating_mul(u128::from(job.need.cpu_milli)))
                .ok_or(SimError::TimeOverflow)?;
        }
        // horizon = occupancy / (capacity × load).
        let capacity = u128::from(config.node.cpu_milli)
            .checked_mul(u128::try_from(wan.len()).map_err(|_| SimError::Config("nodes"))?)
            .and_then(|c| c.checked_mul(u128::from(config.load_per_mille)))
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

/// G2's kind rule (`bench/sched/huawei_trace.py:31-35` and its `classes_by_owner`): each owner's
/// workloads are one kind, and owners go to kinds so each kind has a third of the launches: the owner
/// with the most launches first (ties by owner), each to the kind with the fewest so far, ties OCI,
/// then Firecracker, then Shards. `launches` is each owner's count; the result is each owner's kind.
pub fn classes_by_owner(launches: &[u64]) -> Vec<Class> {
    let mut order: Vec<usize> = (0..launches.len()).collect();
    order.sort_by_key(|&o| (Reverse(launches.get(o).copied().unwrap_or(0)), o));
    let mut got = [0u64; 3];
    let mut out = vec![Class::Oci; launches.len()];
    for owner in order {
        let mut pick = 0;
        for (at, &n) in got.iter().enumerate() {
            if got.get(pick).is_some_and(|&least| n < least) {
                pick = at;
            }
        }
        let class = CLASSES.get(pick).copied().unwrap_or(Class::Oci);
        if let Some(slot) = out.get_mut(owner) {
            *slot = class;
        }
        if let Some(sum) = got.get_mut(pick) {
            *sum = sum.saturating_add(launches.get(owner).copied().unwrap_or(0));
        }
    }
    out
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

    const POLICIES: [Policy; 4] = [
        Policy::Blind,
        Policy::Proximity,
        Policy::TieBreak,
        Policy::Oracle,
    ];

    /// G2's recorded workload, its owners and overheads.
    fn recorded() -> &'static Cluster {
        static RECORDED: std::sync::OnceLock<Cluster> = std::sync::OnceLock::new();
        RECORDED.get_or_init(|| {
            crate::replay::parse(
                include_str!("../../../bench/sched/huawei-r5-day30.trace"),
                &crate::replay::Limits::default(),
            )
            .unwrap()
        })
    }

    fn generate(config: &StudyConfig) -> Study {
        Study::generate(config, recorded()).unwrap()
    }

    fn small(seed: u64, load_per_mille: u64, homes: Homes) -> StudyConfig {
        on(seed, load_per_mille, homes, Fleet::AllKinds)
    }

    fn on(seed: u64, load_per_mille: u64, homes: Homes, fleet: Fleet) -> StudyConfig {
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
            fleet,
            seed,
            ..StudyConfig::default()
        }
    }

    /// S4, A2 (G3 step 2): do run every policy over one study twice; expect the same report twice,
    /// every job placed once, the queue drained, and completion quantiles for all of them.
    #[test]
    fn every_policy_runs_every_job_deterministically() {
        let study = generate(&small(5, 900, Homes::Uniform));
        let again = generate(&small(5, 900, Homes::Uniform));
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
                assert!(jobs > 800, "{class:?}: {jobs} of 3,000");
                assert_eq!(got.placements, u64::try_from(jobs).unwrap(), "{class:?}");
                assert_eq!(got.completion.unwrap().count, jobs, "{class:?}");
            }
            assert_eq!(
                a.classes.iter().map(|c| c.done_by_horizon).sum::<u64>(),
                a.done_by_horizon
            );
        }
    }

    /// AC-T7.4.3's kinds (H10), S4: do generate a study; expect each job's memory to carry its
    /// kind's overhead over its whole-GiB request, and on the mixed fleet some nodes that run only
    /// some kinds, every kind on some node.
    #[test]
    fn workloads_are_hexs_three_kinds() {
        let study = generate(&on(3, 900, Homes::Uniform, Fleet::Mixed));
        for job in &study.jobs {
            assert_eq!(
                job.need.mem_bytes % (1 << 30),
                recorded().overheads.of(job.class)
            );
        }
        assert!(study.nodes.iter().any(|n| n.classes != Classes::ALL));
        for class in CLASSES {
            assert!(
                study.nodes.iter().any(|n| n.classes.has(class)),
                "{class:?}"
            );
        }
        let all = generate(&small(3, 900, Homes::Uniform));
        assert!(all.nodes.iter().all(|n| n.classes == Classes::ALL));
    }

    /// AC-T7.4.3's kinds (H10): do assign kinds to owners by G2's rule; expect it to match
    /// `huawei_trace.py`'s worked order (the most launches first, each to the kind with the fewest so
    /// far, ties OCI, Firecracker, Shards), and on the recorded day each kind within one owner's
    /// launches of a third, as the trace's own header records (oci 8432, firecracker 8432, shards 8431).
    #[test]
    fn kinds_go_to_owners_by_g2s_rule() {
        assert_eq!(
            classes_by_owner(&[5, 9, 2, 2, 0]),
            [
                Class::Firecracker,
                Class::Oci,
                Class::Shards,
                Class::Shards,
                Class::Shards
            ]
        );
        let mut launches = vec![0u64; recorded().owners.len()];
        for w in &recorded().workloads {
            launches[usize::try_from(w.tenancy.owner.0).unwrap()] += 1;
        }
        let kinds = classes_by_owner(&launches);
        let mut got = [0u64; 3];
        for (owner, n) in launches.iter().enumerate() {
            got[class_at(kinds[owner])] += n;
        }
        assert_eq!(got, [8432, 8432, 8431]);
        for w in &recorded().workloads {
            assert_eq!(kinds[usize::try_from(w.tenancy.owner.0).unwrap()], w.class);
        }
    }

    /// AC-T7.4.3's kinds (H10), S4: do generate a study from the recorded day's owners; expect each
    /// owner's jobs one kind, and each kind between a fifth and a half of the jobs (the day's largest
    /// owner alone launched 30% of it, so a study's draw can't split finer than its owners).
    #[test]
    fn a_studys_kinds_follow_its_owners() {
        let study = generate(&small(7, 900, Homes::Uniform));
        let mut kind_of = std::collections::HashMap::new();
        let mut count = [0usize; 3];
        for job in &study.jobs {
            assert_eq!(*kind_of.entry(job.owner).or_insert(job.class), job.class);
            count[class_at(job.class)] += 1;
        }
        for (class, n) in CLASSES.iter().zip(count) {
            assert!(n * 5 > 3_000 && n * 2 < 3_000, "{class:?}: {n} of 3,000");
        }
    }

    /// AC-T7.4.3's kinds (H10), S4: do run a study in which no node runs Firecracker microVMs;
    /// expect every policy to refuse it with a typed error, the Firecracker workloads left queued,
    /// never placed on a node that can't run them.
    #[test]
    fn a_kind_no_node_runs_is_refused_not_misplaced() {
        let mut study = generate(&on(4, 500, Homes::Uniform, Fleet::Mixed));
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

    /// S4, A2 (G3 step 2) non-vacuity: proximity and the oracle each chose a node blind placement
    /// wouldn't have in more than a tenth of decisions, the tie-break in more than a fiftieth; blind
    /// never "moved". The tie-break acts only where best fit's free capacity ties exactly, and the
    /// kinds' overheads (0.42 MiB against 5 MiB) leave nodes' free memory apart by odd bytes, so it
    /// finds fewer ties than over generic jobs (235 of 3,000 decisions here, seed 6, against over 300
    /// before the kinds). The fiftieth is a sanity floor: what shows the tie-break's effect can fail is
    /// `the_tie_break_places_nearer_than_blind`, which both tie-break mutants fail.
    #[test]
    fn proximity_changes_decisions() {
        let study = generate(&small(6, 900, Homes::Skewed));
        for policy in POLICIES {
            let r = study.run(policy).unwrap();
            let floor = match policy {
                Policy::Blind => {
                    assert_eq!(r.moved, 0);
                    continue;
                }
                Policy::TieBreak => 50,
                Policy::Proximity | Policy::Oracle => 10,
            };
            assert!(
                r.moved * floor > r.placements,
                "{policy:?} moved {}",
                r.moved
            );
        }
    }

    /// S4, A2 (G3 step 2) sanity: the oracle, scoring by the true RTT, places no farther from home
    /// on average than coordinates do, and coordinates no farther than blind placement.
    #[test]
    fn the_oracle_bounds_the_coordinates() {
        for seed in 1..=3 {
            for homes in [Homes::Uniform, Homes::Skewed] {
                let study = generate(&small(seed, 500, homes));
                let [blind, near, tie, oracle] = POLICIES.map(|p| study.run(p).unwrap());
                for r in [&blind, &near, &tie, &oracle] {
                    eprintln!(
                        "G3 seed {seed} {homes:?} {:?}: rtt {:.1} ms, goodput {:.2}/s, {:.1} cores, {:?}",
                        r.policy,
                        r.mean_rtt_ms(),
                        r.goodput_per_s(),
                        r.useful_cores(),
                        r.completion
                    );
                }
                assert!(
                    oracle.mean_rtt_ms() <= near.mean_rtt_ms() * 1.001,
                    "{seed} {homes:?}"
                );
                assert!(near.mean_rtt_ms() < blind.mean_rtt_ms(), "{seed} {homes:?}");
            }
        }
    }

    /// S4, A2 (G3 step 2): do run blind placement and the tie-break, Hex's three kinds, on five
    /// seeds, both fleets, homes uniform and skewed, at 50% and 90% load; expect the tie-break to
    /// place nearer home on average than blind on every one: among best fit's exact ties it takes the
    /// least predicted RTT, so its placements' mean true RTT is lower. Mutants `no-tie-break-term`
    /// (its score 0, best fit's own order) and `random-tie-break` (a node's hash, not its RTT) fail it.
    #[test]
    fn the_tie_break_places_nearer_than_blind() {
        for seed in 1..=5 {
            for fleet in [Fleet::AllKinds, Fleet::Mixed] {
                for homes in [Homes::Uniform, Homes::Skewed] {
                    for load in [500, 900] {
                        let study = generate(&on(seed, load, homes, fleet));
                        let blind = study.run(Policy::Blind).unwrap();
                        let tie = study.run(Policy::TieBreak).unwrap();
                        assert!(
                            tie.mean_rtt_ms() < blind.mean_rtt_ms(),
                            "seed {seed} {fleet:?} {homes:?} load {load}: tie-break {:.1} ms, blind {:.1} ms",
                            tie.mean_rtt_ms(),
                            blind.mean_rtt_ms()
                        );
                    }
                }
            }
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
                        let study = generate(&on(seed, load, homes, fleet));
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
                Study::generate(&bad, recorded()),
                Err(SimError::Config(_))
            ));
        }
        let empty = Cluster {
            workloads: Vec::new(),
            ..recorded().clone()
        };
        assert!(matches!(
            Study::generate(&StudyConfig::default(), &empty),
            Err(SimError::Config(_))
        ));
    }
}
