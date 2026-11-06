//! The fault nemesis (`docs/design/failure-isolation.md`, scheduler implementation goal 5): each fault
//! class injected into a simulated cell from a seed, the cell driving `hex-health` as the scheduler
//! does, and each fault's detection and isolation time measured against the bound the design states.
//! One seed, virtual time, one thread (hex-sim H1): a run is a function of its seed.
//!
//! The crash class is [`crate::membership`]'s. Here:
//! - [`gray`]: one node's work runs slower than its peers' ([`hex_health::grade`]).
//! - [`disk_stall`]: one node's disk probe stops completing ([`hex_health::probe`]).
//! - [`antagonist`]: a co-tenant raises its neighbours' CPI ([`hex_health::antagonist`]).
//! - [`stuck_loop`], [`quota`], [`provider_outage`]: the agent faults ([`hex_health::agent`]).
//! - [`dangerous`]: a security signal, per kind Hex places ([`crate::danger`]).
//! - [`correlated`]: many nodes slow at once, against the ejection cap, and a mass loss against the
//!   eviction rate ([`hex_health::penalty`]).

use hex_health::agent::{Cause, LoopDetector, ProviderError, ProviderHealth, Stuck};
use hex_health::antagonist::{Antagonists, Sample};
use hex_health::danger::Signal;
use hex_health::grade::{self, PeerGrade};
use hex_health::penalty::{Ejector, EvictionLimiter};
use hex_health::probe::{self, DiskProbe};
use hex_health::{HealthError, Level};

use crate::danger::{self, Isolation, Refusal, STRONGER};
use crate::model::{Class, Classes, Overheads, Resources};
use crate::rng::Rng;

pub const MS: u64 = 1_000_000;
pub const S: u64 = 1_000_000_000;

/// Every way a fault run refuses to go on, typed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FaultError {
    #[error("the health core refused: {0}")]
    Health(#[from] HealthError),
    #[error("the run took more than its budget of {0} steps")]
    StepBudget(u64),
    #[error("virtual time overflowed")]
    TimeOverflow,
}

/// One injected fault's measured times, from its onset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Measured {
    pub detected_after_ns: Option<u64>,
    pub isolated_after_ns: Option<u64>,
    /// Verdicts against what wasn't at fault: a healthy node suspected, a bystander throttled, a node
    /// charged with its provider's or account's fault.
    pub wrong_verdicts: u64,
}

/// A switch for mutation tests: each turns one safeguard off, and the test that guards it must fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mutant {
    None,
    /// The gray detector off: the grade's verdicts are never acted on.
    NoDetector,
    /// The ejection cap ignored: every drain ejects.
    NoEjectionCap,
    /// The eviction rate ignored: every dead node's work moves at once.
    NoEvictionRate,
    /// Provider errors fed to the node grade as slowness.
    ProviderErrorsAsNodeSlowness,
}

fn after(onset: u64, at: u64) -> Option<u64> {
    at.checked_sub(onset)
}

fn add(a: u64, b: u64) -> Result<u64, FaultError> {
    a.checked_add(b).ok_or(FaultError::TimeOverflow)
}

/// A slowdown around 1x with the spread of ordinary work: 0.7x to 1.5x, in thousandths.
fn noise(rng: &mut Rng) -> u64 {
    rng.range(700, 1500)
}

/// Gray / fail-slow (F2): a cell of `nodes`, each reporting about ten pieces of work a second; from a
/// seeded onset one node's work runs `factor_milli` / 1000 times slower. Detected at its `Suspect`,
/// isolated at its `Drain` taken by the ejector.
pub fn gray(seed: u64) -> Result<(Measured, u64), FaultError> {
    gray_with(seed, Mutant::None)
}

/// [`gray`] with a mutant switched on.
pub fn gray_with(seed: u64, mutant: Mutant) -> Result<(Measured, u64), FaultError> {
    let mut rng = Rng::new(seed);
    let nodes = rng.range(4, 200);
    let bad = rng.range(0, nodes.saturating_sub(1));
    let factor = rng.range(2_500, 8_000);
    let onset = rng.range(5 * S, 20 * S);
    let end = add(onset, 10 * S)?;
    let mut g = PeerGrade::new(usize::try_from(nodes).unwrap_or(usize::MAX), 0);
    let mut ejector = Ejector::new(nodes, usize::try_from(nodes).unwrap_or(usize::MAX));
    let mut m = Measured::default();
    let mut changes = Vec::new();
    // Every node reports at 100 ms steps, with a seeded phase each, so windows close in time order.
    let mut t: u64 = 0;
    let mut steps: u64 = 0;
    while t < end {
        for n in 0..nodes {
            let at = add(t, n.saturating_mul(97) % (100 * MS))?;
            let mut slowdown = noise(&mut rng);
            if n == bad && at >= onset {
                slowdown = slowdown.saturating_mul(factor) / 1000;
            }
            changes.clear();
            g.observe(n, at, slowdown, &mut changes)?;
            if mutant == Mutant::NoDetector {
                changes.clear();
            }
            for c in &changes {
                if c.node != bad || at < onset {
                    m.wrong_verdicts = m.wrong_verdicts.saturating_add(1);
                    continue;
                }
                if c.level >= Level::Suspect && m.detected_after_ns.is_none() {
                    m.detected_after_ns = after(onset, at);
                }
                if c.level == Level::Drain
                    && m.isolated_after_ns.is_none()
                    && ejector.eject(c.node, at)?.is_ok()
                {
                    m.isolated_after_ns = after(onset, at);
                }
            }
        }
        t = add(t, 100 * MS)?;
        steps = steps.saturating_add(1);
        if steps > 1_000_000 {
            return Err(FaultError::StepBudget(steps));
        }
    }
    Ok((m, factor))
}

/// Disk stall (F3, `AC-T7.10.3`): a node probes its disk every `period` with 1-5 ms flushes; from a
/// seeded onset the outstanding write never completes. Returns the probe's timeout at the onset.
pub fn disk_stall(seed: u64) -> Result<(Measured, u64, u64), FaultError> {
    let mut rng = Rng::new(seed);
    let period = rng.range(100 * MS, 500 * MS);
    let onset_probe = rng.range(20, 200);
    let mut p = DiskProbe::new(MS);
    let mut t: u64 = 0;
    for _ in 0..onset_probe {
        p.start(t)?;
        let lat = rng.range(MS, 5 * MS);
        p.finish(add(t, lat)?)?;
        t = add(t, period)?;
    }
    let timeout = p.timeout_ns()?;
    // The stall: the write the node starts at its next probe after `onset` never finishes.
    let onset = add(t, rng.range(0, period.saturating_sub(1)))?;
    let start = add(t, period)?;
    p.start(start)?;
    let mut m = Measured::default();
    let mut now = start;
    let mut steps: u64 = 0;
    // The driver looks again each millisecond, as a node agent's timer would at its own granularity.
    while m.isolated_after_ns.is_none() {
        now = add(now, MS)?;
        match p.level(now)? {
            Level::Healthy => {}
            Level::Suspect => {
                m.detected_after_ns.get_or_insert(now.saturating_sub(onset));
            }
            Level::Drain => {
                m.detected_after_ns.get_or_insert(now.saturating_sub(onset));
                m.isolated_after_ns = Some(now.saturating_sub(onset));
            }
        }
        steps = steps.saturating_add(1);
        if steps > 10_000_000 {
            return Err(FaultError::StepBudget(steps));
        }
    }
    Ok((m, timeout, period))
}

/// Antagonist (F4): a node of 5-10 latency-sensitive tasks of one job and one batch co-tenant,
/// sampled once a minute (CPI²'s cadence); from a seeded onset after the job has its spec, the
/// co-tenant's CPU use rises tenfold and its neighbours' CPI by `factor_milli` / 1000 until it is
/// capped. Detected and isolated at the cap.
pub fn antagonist(seed: u64) -> Result<(Measured, u64), FaultError> {
    const MIN: u64 = 60 * S;
    let mut rng = Rng::new(seed);
    let tasks = rng.range(5, 10);
    // The class the design states: the neighbours' CPI at least doubled.
    let factor = rng.range(2_000, 3_000);
    let onset_min = rng.range(110, 140);
    let best_effort = rng.chance(500);
    let mut a = Antagonists::new(64);
    let mut m = Measured::default();
    let onset = onset_min.saturating_mul(MIN);
    for minute in 0..onset_min.saturating_add(30) {
        let t = minute.saturating_mul(MIN);
        let hot = t >= onset && !a.throttled(99, t);
        // The node's samples of one minute go in together, as CPI²'s per-machine agent takes them.
        let mut batch = Vec::with_capacity(usize::try_from(tasks).unwrap_or(0).saturating_add(1));
        for task in 0..tasks {
            let base = rng.range(1000, 1060);
            let cpi = if hot {
                base.saturating_mul(factor) / 1000
            } else {
                base
            };
            batch.push(Sample {
                task,
                job: 1,
                node: 0,
                at_ns: t,
                cpi_milli: cpi,
                cpu_milli: rng.range(600, 900),
                best_effort: false,
            });
        }
        batch.push(Sample {
            task: 99,
            job: 2,
            node: 0,
            at_ns: t,
            cpi_milli: 1500,
            cpu_milli: if hot {
                rng.range(2500, 4000)
            } else {
                rng.range(250, 400)
            },
            best_effort,
        });
        if let Some(th) = a.sample(&batch)? {
            if th.antagonist != 99 || t < onset {
                m.wrong_verdicts = m.wrong_verdicts.saturating_add(1);
            } else if m.isolated_after_ns.is_none() {
                m.detected_after_ns = after(onset, t);
                m.isolated_after_ns = after(onset, t);
            }
        }
    }
    Ok((m, factor))
}

/// Stuck tool loop (F5): `sessions` agents make calls a few seconds apart, with progress every few
/// calls; from a seeded onset one of them repeats its last call without progress. Detected and
/// isolated (stopped) at the stuck verdict. Returns, beside the times, how many calls the loop made
/// before it was stopped.
pub fn stuck_loop(seed: u64) -> Result<(Measured, u32), FaultError> {
    let mut rng = Rng::new(seed);
    let sessions = rng.range(1, 50);
    let looping = rng.range(0, sessions.saturating_sub(1));
    let onset = rng.range(30 * S, 120 * S);
    let mut d = LoopDetector::new(64);
    let mut m = Measured::default();
    let mut last_call = vec![0u64; usize::try_from(sessions).unwrap_or(0)];
    let mut t: u64 = 0;
    let mut loop_calls: u32 = 0;
    while t < add(onset, 300 * S)? && m.isolated_after_ns.is_none() {
        for s in 0..sessions {
            let idx = usize::try_from(s).unwrap_or(usize::MAX);
            let in_loop = s == looping && t >= onset;
            let call = if in_loop {
                loop_calls = loop_calls.saturating_add(1);
                last_call.get(idx).copied().unwrap_or(0)
            } else {
                rng.next_u64()
            };
            if let Some(slot) = last_call.get_mut(idx) {
                *slot = call;
            }
            if let Some(stuck) = d.call(s, call)? {
                if !in_loop {
                    m.wrong_verdicts = m.wrong_verdicts.saturating_add(1);
                } else if let Stuck::Repeated { .. } = stuck {
                    m.detected_after_ns = after(onset, t);
                    m.isolated_after_ns = after(onset, t);
                    d.end(s);
                }
            }
            if !in_loop && rng.chance(300) {
                d.progress(s);
            }
        }
        t = add(t, rng.range(S, 10 * S))?;
    }
    Ok((m, loop_calls))
}

/// The account and provider half of a run: calls from `nodes` nodes to `providers` providers, each
/// call's error fed to [`ProviderHealth`] and, under the mutant, to the node grade.
struct Calls {
    nodes: u64,
    health: ProviderHealth,
    grade: PeerGrade,
    causes: Vec<Cause>,
    changes: Vec<grade::Change>,
    mutant: Mutant,
}

impl Calls {
    fn new(nodes: u64, mutant: Mutant) -> Self {
        let n = usize::try_from(nodes).unwrap_or(usize::MAX);
        Self {
            nodes,
            health: ProviderHealth::new(8, n, 0),
            grade: PeerGrade::new(n, 0),
            causes: Vec::new(),
            changes: Vec::new(),
            mutant,
        }
    }

    /// One call; local work beside it is reported to the node grade as ordinary.
    fn call(
        &mut self,
        rng: &mut Rng,
        provider: u64,
        node: u64,
        at: u64,
        outcome: Result<(), ProviderError>,
    ) -> Result<(), FaultError> {
        self.causes.clear();
        self.changes.clear();
        self.health
            .call(provider, node, node, at, outcome, &mut self.causes)?;
        let slowdown = match (outcome, self.mutant) {
            (Err(_), Mutant::ProviderErrorsAsNodeSlowness) => 10_000,
            _ => noise(rng),
        };
        self.grade.observe(node, at, slowdown, &mut self.changes)?;
        Ok(())
    }

    fn node_verdicts(&self) -> u64 {
        let charged = self
            .causes
            .iter()
            .filter(|c| matches!(c, Cause::NodePath { .. }))
            .count();
        let suspected = self
            .changes
            .iter()
            .filter(|c| c.level != Level::Healthy)
            .count();
        u64::try_from(charged.saturating_add(suspected)).unwrap_or(u64::MAX)
    }
}

/// Quota shown as slowness (F5): nodes call a provider on many accounts; from a seeded onset one
/// account's calls are refused as rate-limited or out of quota. Detected and isolated when the
/// account is held; any node verdict is wrong.
pub fn quota(seed: u64, mutant: Mutant) -> Result<Measured, FaultError> {
    let mut rng = Rng::new(seed);
    let nodes = rng.range(4, 64);
    let onset = rng.range(5 * S, 20 * S);
    let account_node = rng.range(0, nodes.saturating_sub(1));
    let mut c = Calls::new(nodes, mutant);
    let mut m = Measured::default();
    let mut t: u64 = 0;
    while t < add(onset, 10 * S)? {
        for n in 0..c.nodes {
            let at = add(t, n % (100 * MS))?;
            let out = if n == account_node && at >= onset {
                if rng.chance(500) {
                    Err(ProviderError::QuotaExhausted)
                } else {
                    Err(ProviderError::RateLimited {
                        retry_after_ns: Some(rng.range(S, 60 * S)),
                    })
                }
            } else {
                Ok(())
            };
            c.call(&mut rng, 1, n, at, out)?;
            if c.causes
                .iter()
                .any(|x| matches!(x, Cause::Account { account, .. } if *account == account_node))
                && m.detected_after_ns.is_none()
            {
                m.detected_after_ns = after(onset, at);
                m.isolated_after_ns = after(onset, at);
            }
            m.wrong_verdicts = m.wrong_verdicts.saturating_add(c.node_verdicts());
        }
        t = add(t, 100 * MS)?;
    }
    Ok(m)
}

/// Partial provider outage (F5): from a seeded onset a provider overloads or times out for a
/// majority of its callers (60-100%). Detected and isolated when it is marked degraded; any node
/// verdict is wrong.
pub fn provider_outage(seed: u64, mutant: Mutant) -> Result<Measured, FaultError> {
    let mut rng = Rng::new(seed);
    let nodes = rng.range(4, 64);
    let onset = rng.range(5 * S, 20 * S);
    let share = rng.range(600, 1000);
    let mut c = Calls::new(nodes, mutant);
    let mut m = Measured::default();
    let mut t: u64 = 0;
    while t < add(onset, 10 * S)? {
        for n in 0..c.nodes {
            let at = add(t, n % (100 * MS))?;
            let failing = n.saturating_mul(1000) < nodes.saturating_mul(share);
            let out = if failing && at >= onset {
                if rng.chance(500) {
                    Err(ProviderError::Overloaded)
                } else {
                    Err(ProviderError::Timeout)
                }
            } else {
                Ok(())
            };
            c.call(&mut rng, 1, n, at, out)?;
            if c.causes
                .iter()
                .any(|x| matches!(x, Cause::Provider { provider: 1 }))
                && m.detected_after_ns.is_none()
            {
                m.detected_after_ns = after(onset, at);
                m.isolated_after_ns = after(onset, at);
            }
            m.wrong_verdicts = m.wrong_verdicts.saturating_add(c.node_verdicts());
        }
        t = add(t, 100 * MS)?;
    }
    Ok(m)
}

/// The kinds' overheads as G2's traces state them (`docs/design/hex-sim.md`, the recorded workload):
/// OCI 0.42 MiB, measured (`bench/competitive/RESULT.md:95`); Firecracker 5 MiB, its specification
/// (Agache et al., NSDI '20); Shards 5 MiB, a stand-in until Shards' own is measured.
pub fn kind_overheads() -> Overheads {
    let mut o = Overheads::default();
    o.set(Class::Oci, 440_402);
    o.set(Class::Firecracker, 5_242_880);
    o.set(Class::Shards, 5_242_880);
    o
}

/// Dangerous (F7): a workload of kind `kind` raises a seeded trip signal on a node that runs its kind
/// and a seeded set of the others, with seeded free capacity and a seeded need (GPUs only for an OCI
/// container, the one kind that takes them). Isolated at the signal: moved to the first stronger kind
/// the node runs that holds it, or refused for the reason that holds; anything else is wrong.
pub fn dangerous(seed: u64, kind: Class) -> Result<(Measured, Isolation), FaultError> {
    const MIB: u64 = 1 << 20;
    let mut rng = Rng::new(seed);
    let mut classes = Classes::NONE.with(kind);
    for c in [Class::Oci, Class::Firecracker, Class::Shards] {
        if rng.chance(600) {
            classes = classes.with(c);
        }
    }
    let signal = match rng.range(0, 2) {
        0 => Signal::SeccompKill,
        1 => Signal::EgressRefused,
        _ => Signal::PrivilegeEscalation,
    };
    let need = Resources {
        cpu_milli: rng.range(250, 8_000),
        mem_bytes: rng.range(128, 16_384).saturating_mul(MIB),
        gpus: u32::from(kind == Class::Oci && rng.chance(150)),
    };
    // What the node has free beside the workload: from nothing to plenty, so some moves fit and some
    // don't.
    let free = Resources {
        cpu_milli: rng.range(0, 16_000),
        mem_bytes: rng.range(0, 32_768).saturating_mul(MIB),
        gpus: u32::try_from(rng.range(0, 1)).unwrap_or(0),
    };
    let overheads = kind_overheads();
    let onset = rng.range(S, 100 * S);
    let iso = danger::isolate(kind, signal, classes, free, need, overheads);
    // The oracle, from the rule's statement: room is what's free once the workload gives back its need
    // and its kind's overhead; a microVM holds it when it takes its GPUs and its need plus the
    // microVM's overhead fits in the room.
    let room = Resources {
        cpu_milli: free.cpu_milli.saturating_add(need.cpu_milli),
        mem_bytes: free
            .mem_bytes
            .saturating_add(need.mem_bytes)
            .saturating_add(overheads.of(kind)),
        gpus: free.gpus.saturating_add(need.gpus),
    };
    let fits = |c: Class| {
        classes.has(c)
            && (need.gpus == 0 || c.takes_gpus())
            && need.cpu_milli <= room.cpu_milli
            && need.mem_bytes.saturating_add(overheads.of(c)) <= room.mem_bytes
    };
    let first = STRONGER.into_iter().find(|c| fits(*c));
    let right = match iso {
        Isolation::Move { from, to } => from == kind && kind == Class::Oci && first == Some(to),
        Isolation::Refuse { from, why } => {
            from == kind
                && match why {
                    Refusal::InMicroVm => kind != Class::Oci,
                    Refusal::NoMicroVmOnNode => {
                        kind == Class::Oci && !STRONGER.iter().any(|c| classes.has(*c))
                    }
                    Refusal::DoesNotFit => {
                        kind == Class::Oci
                            && STRONGER.iter().any(|c| classes.has(*c))
                            && first.is_none()
                    }
                }
        }
        Isolation::None => false,
    };
    let mut m = Measured::default();
    if right {
        m.detected_after_ns = after(onset, onset);
        m.isolated_after_ns = after(onset, onset);
    } else {
        m.wrong_verdicts = 1;
    }
    Ok((m, iso))
}

/// What a correlated fault did to a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Correlated {
    pub nodes: u64,
    pub cap: u64,
    /// The most nodes ejected at once.
    pub max_ejected: u64,
    /// Drains the cap refused (non-vacuity: the cap was reached).
    pub refused: u64,
    /// Nodes whose work was moved after a mass loss, and the most moved in any `window` stretch.
    pub moved: u64,
    pub max_moved_in_window: u64,
    pub window_ns: u64,
    pub allowed_in_window: u64,
}

/// Correlated faults (F6): a third to two fifths of a cell turns slow at once (a bad rollout, a
/// noisy shared dependency), then most of the cell is lost at once (a partition). The slow nodes'
/// drains go through the ejector, the lost nodes' work through the eviction limiter.
pub fn correlated(seed: u64, mutant: Mutant) -> Result<Correlated, FaultError> {
    let mut rng = Rng::new(seed);
    let nodes = rng.range(20, 200);
    let slow_share = rng.range(300, 400);
    let mut g = PeerGrade::new(usize::try_from(nodes).unwrap_or(usize::MAX), 0);
    let mut ejector = Ejector::new(nodes, usize::try_from(nodes).unwrap_or(usize::MAX));
    let mut out = Correlated {
        nodes,
        cap: ejector.cap(),
        ..Correlated::default()
    };
    let mut ejected_by_mutant: u64 = 0;
    let mut changes = Vec::new();
    let mut t: u64 = 0;
    while t < 10 * S {
        for n in 0..nodes {
            let at = add(t, n % (100 * MS))?;
            let slow = n.saturating_mul(1000) < nodes.saturating_mul(slow_share) && at >= 2 * S;
            let s = if slow {
                noise(&mut rng).saturating_mul(4)
            } else {
                noise(&mut rng)
            };
            changes.clear();
            g.observe(n, at, s, &mut changes)?;
            for c in changes.iter().filter(|c| c.level == Level::Drain) {
                if mutant == Mutant::NoEjectionCap {
                    ejected_by_mutant = ejected_by_mutant.saturating_add(1);
                } else if ejector.eject(c.node, at)?.is_err() {
                    out.refused = out.refused.saturating_add(1);
                }
            }
            let now = ejector.ejected_at(at).saturating_add(ejected_by_mutant);
            out.max_ejected = out.max_ejected.max(now);
        }
        t = add(t, 100 * MS)?;
    }
    // The partition: 55-90% of the cell declared dead at one instant; their work moves through the
    // limiter, which is asked again each 100 ms for 10 minutes.
    let lost = nodes.saturating_mul(rng.range(550, 900)) / 1000;
    let mut limiter = EvictionLimiter::new(t);
    let mut pending = lost;
    let mut moves: Vec<u64> = Vec::new();
    let stop = add(t, 600 * S)?;
    while t < stop && pending > 0 {
        let granted = mutant == Mutant::NoEvictionRate || limiter.take(t, nodes, lost)?.is_ok();
        if granted {
            pending = pending.saturating_sub(1);
            moves.push(t);
            continue;
        }
        t = add(t, 100 * MS)?;
    }
    out.moved = u64::try_from(moves.len()).unwrap_or(u64::MAX);
    out.window_ns = 100 * S;
    let rate = EvictionLimiter::rate(nodes, lost);
    // One token to start with, then the rate's worth in a window.
    out.allowed_in_window = 1u64.saturating_add(rate.saturating_mul(100) / 1000);
    let mut lo = 0usize;
    for (hi, &at) in moves.iter().enumerate() {
        while moves
            .get(lo)
            .is_some_and(|&first| first.saturating_add(out.window_ns) <= at)
        {
            lo = lo.saturating_add(1);
        }
        let in_window = u64::try_from(hi.saturating_sub(lo).saturating_add(1)).unwrap_or(u64::MAX);
        out.max_moved_in_window = out.max_moved_in_window.max(in_window);
    }
    Ok(out)
}

/// The disk stall's bounds (F3): suspect within one timeout and one period of the onset, drain within
/// three timeouts and one period.
pub fn disk_stall_bounds(timeout: u64, period: u64) -> (u64, u64) {
    let suspect = timeout.saturating_add(period).saturating_add(MS);
    let drain = timeout
        .saturating_mul(probe::DRAIN_TIMEOUTS)
        .saturating_add(period)
        .saturating_add(MS);
    (suspect, drain)
}
