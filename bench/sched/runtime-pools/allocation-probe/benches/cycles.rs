//! CP-T7.4: explicit pooled admission/placement cycles, sharing the constructor probe's allocator.
//! This is source preparation only; the lead chooses inputs and owns validation/measurement.

use std::any::Any;
use std::ffi::OsString;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use hex_sim::index::{Fit, Key, Pick, Scorer, Shapes, Verdict};
use hex_sim::model::{Class, Classes, Demand, Kind, RuntimePool, WorkloadId};

use super::{
    FitIndex, NodeId, NodeSpec, Resources, Snapshot, Value, emit, json, panic_text, phase,
};

#[derive(Clone, Copy)]
enum Workload {
    Constructor,
    Cycles,
}

#[derive(Clone, Copy)]
enum Decision {
    Verdict,
    Fit,
}

impl Decision {
    fn name(self) -> &'static str {
        match self {
            Self::Verdict => "verdict",
            Self::Fit => "fit",
        }
    }
}

/// Optional extension parsing leaves the original four-flag constructor path intact.
#[derive(Default)]
pub(super) struct Options {
    workload: Option<Workload>,
    decision: Option<Decision>,
    scorer: Option<Scorer>,
    pool_classes: Option<Classes>,
    pool_cpu: Option<u64>,
    pool_mem: Option<u64>,
    class: Option<Class>,
    cpu: Option<u64>,
    mem: Option<u64>,
    repeats: Option<u64>,
    warmup: Option<u64>,
}

fn class(value: &str) -> Result<Class, String> {
    match value {
        "oci" => Ok(Class::Oci),
        "firecracker" => Ok(Class::Firecracker),
        "shards" => Ok(Class::Shards),
        _ => Err("class must be oci, firecracker or shards".into()),
    }
}

fn classes(value: &str) -> Result<Classes, String> {
    let mut out = Classes::NONE;
    for value in value.split(',') {
        let class = class(value)?;
        if out.has(class) {
            return Err("duplicate pool class".into());
        }
        out = out.with(class);
    }
    Ok(out)
}

fn set<T>(slot: &mut Option<T>, value: T, name: &str) -> Result<(), String> {
    if slot.is_some() {
        return Err(format!("duplicate {name}"));
    }
    *slot = Some(value);
    Ok(())
}

impl Options {
    pub(super) fn parse(
        &mut self,
        flag: &str,
        arguments: &mut impl Iterator<Item = OsString>,
    ) -> Result<bool, String> {
        if !matches!(
            flag,
            "--workload"
                | "--decision"
                | "--scorer"
                | "--pool-classes"
                | "--pool-cpu-milli"
                | "--pool-mem-bytes"
                | "--demand-class"
                | "--demand-cpu-milli"
                | "--demand-mem-bytes"
                | "--repeats"
                | "--warmup-repeats"
        ) {
            return Ok(false);
        }
        let value = arguments
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        let value = value.to_str().ok_or("argument value is not UTF-8")?;
        let number = || {
            value
                .parse::<u64>()
                .map_err(|_| format!("unsigned integer required for {flag}"))
        };
        match flag {
            "--workload" => set(
                &mut self.workload,
                match value {
                    "constructor" => Workload::Constructor,
                    "runtime-pool-cycles" => Workload::Cycles,
                    _ => return Err("workload must be constructor or runtime-pool-cycles".into()),
                },
                flag,
            )?,
            "--decision" => set(
                &mut self.decision,
                match value {
                    "verdict" => Decision::Verdict,
                    "fit" => Decision::Fit,
                    _ => return Err("decision must be verdict or fit".into()),
                },
                flag,
            )?,
            "--scorer" => set(
                &mut self.scorer,
                match value {
                    "best-fit" => Scorer::BestFit,
                    "most-allocated" => Scorer::MostAllocated,
                    "align" => Scorer::Align,
                    "hex" => Scorer::HEX,
                    _ => return Err("scorer must be best-fit, most-allocated, align or hex".into()),
                },
                flag,
            )?,
            "--pool-classes" => set(&mut self.pool_classes, classes(value)?, flag)?,
            "--pool-cpu-milli" => set(&mut self.pool_cpu, number()?, flag)?,
            "--pool-mem-bytes" => set(&mut self.pool_mem, number()?, flag)?,
            "--demand-class" => set(&mut self.class, class(value)?, flag)?,
            "--demand-cpu-milli" => set(&mut self.cpu, number()?, flag)?,
            "--demand-mem-bytes" => set(&mut self.mem, number()?, flag)?,
            "--repeats" => set(&mut self.repeats, number()?, flag)?,
            "--warmup-repeats" => set(&mut self.warmup, number()?, flag)?,
            _ => return Err("extension parser rejected its recognized flag".into()),
        }
        Ok(true)
    }

    pub(super) fn finish(self) -> Result<Option<Inputs>, String> {
        let any = self.decision.is_some()
            || self.scorer.is_some()
            || self.pool_classes.is_some()
            || self.pool_cpu.is_some()
            || self.pool_mem.is_some()
            || self.class.is_some()
            || self.cpu.is_some()
            || self.mem.is_some()
            || self.repeats.is_some()
            || self.warmup.is_some();
        if !matches!(self.workload, Some(Workload::Cycles)) {
            if any {
                return Err("cycle flags require --workload runtime-pool-cycles".into());
            }
            return Ok(None);
        }
        let input = Inputs {
            decision: self.decision.ok_or("missing --decision")?,
            scorer: self.scorer.ok_or("missing --scorer")?,
            pool: RuntimePool {
                classes: self.pool_classes.ok_or("missing --pool-classes")?,
                capacity: resources(
                    self.pool_cpu.ok_or("missing --pool-cpu-milli")?,
                    self.pool_mem.ok_or("missing --pool-mem-bytes")?,
                ),
            },
            demand: Demand {
                class: self.class.ok_or("missing --demand-class")?,
                kind: Kind::Plain,
                need: resources(
                    self.cpu.ok_or("missing --demand-cpu-milli")?,
                    self.mem.ok_or("missing --demand-mem-bytes")?,
                ),
                ask: Default::default(),
            },
            repeats: self.repeats.ok_or("missing --repeats")?,
            warmup: self.warmup.ok_or("missing --warmup-repeats")?,
        };
        if input.repeats == 0 {
            return Err("--repeats must be positive".into());
        }
        if input.demand.need == Resources::default() {
            return Err("demand must reserve CPU or memory".into());
        }
        if !input.pool.classes.has(input.demand.class)
            || !input.pool.capacity.holds(&input.demand.need)
        {
            return Err("healthy cycles require the pool to include and hold the demand".into());
        }
        Ok(Some(input))
    }
}

#[derive(Clone, Copy)]
pub(super) struct Inputs {
    decision: Decision,
    scorer: Scorer,
    pool: RuntimePool,
    demand: Demand,
    repeats: u64,
    warmup: u64,
}

fn resources(cpu_milli: u64, mem_bytes: u64) -> Resources {
    Resources {
        cpu_milli,
        mem_bytes,
        gpus: 0,
        disk_bytes: 0,
    }
}

fn scorer_name(scorer: Scorer) -> &'static str {
    match scorer {
        Scorer::BestFit => "best-fit",
        Scorer::MostAllocated => "most-allocated",
        Scorer::Align => "align",
        Scorer::Fragment(_) => "hex",
    }
}

fn class_name(class: Class) -> &'static str {
    match class {
        Class::Oci => "oci",
        Class::Firecracker => "firecracker",
        Class::Shards => "shards",
    }
}

#[derive(Clone, Copy)]
struct Sample {
    before: Snapshot,
    after: Snapshot,
    elapsed: Duration,
}

impl Sample {
    fn valid(self) -> bool {
        !self.before.invalid
            && !self.after.invalid
            && self.after.allocations >= self.before.allocations
            && self.after.deallocations >= self.before.deallocations
            && self.after.reallocations >= self.before.reallocations
            && self.after.requested_bytes >= self.before.requested_bytes
            && self.after.allocation_failures == self.before.allocation_failures
    }
    fn value(self) -> Value {
        json!({"before":self.before.value(), "after":self.after.value(),
            "phase":phase(self.before,self.after,self.elapsed)})
    }
}

type Panicked = Box<dyn Any + Send>;

fn measure<T>(operation: impl FnOnce() -> T) -> (Sample, Result<T, Panicked>) {
    let before = Snapshot::now();
    let began = Instant::now();
    let outcome = catch_unwind(AssertUnwindSafe(operation));
    let elapsed = began.elapsed();
    let after = Snapshot::now();
    (
        Sample {
            before,
            after,
            elapsed,
        },
        outcome,
    )
}

struct Failure {
    operation: &'static str,
    kind: &'static str,
    message: String,
    sample: Option<Box<Sample>>,
}

impl Failure {
    fn new(
        operation: &'static str,
        kind: &'static str,
        message: impl Into<String>,
        sample: Option<Sample>,
    ) -> Self {
        Self {
            operation,
            kind,
            message: message.into(),
            sample: sample.map(Box::new),
        }
    }
    fn value(&self) -> Value {
        json!({"operation":self.operation,"kind":self.kind,"message":self.message,"sample":self.sample.as_deref().copied().map(Sample::value)})
    }
}

fn completed<T>(
    operation: &'static str,
    sample: Sample,
    outcome: Result<T, Panicked>,
) -> Result<T, Failure> {
    let value = outcome.map_err(|payload| {
        Failure::new(
            operation,
            "panic",
            panic_text(payload.as_ref()),
            Some(sample),
        )
    })?;
    if !sample.valid() {
        return Err(Failure::new(
            operation,
            "accounting_error",
            "invalid counters or allocator failure",
            Some(sample),
        ));
    }
    Ok(value)
}

enum Choice {
    Verdict(Result<Verdict, hex_sim::error::SimError>),
    Fit(Fit),
}

fn decide(index: &FitIndex, input: &Inputs, pick: &Pick, candidate: NodeId) -> Choice {
    match input.decision {
        Decision::Verdict => {
            Choice::Verdict(index.verdict(candidate, std::hint::black_box(&input.demand)))
        }
        Decision::Fit => Choice::Fit(index.fit(
            std::hint::black_box(&input.demand),
            std::hint::black_box(pick),
        )),
    }
}

fn selected(
    index: &FitIndex,
    choice: Choice,
    candidate: NodeId,
) -> Result<(Key, Option<u64>), String> {
    match choice {
        Choice::Verdict(Ok(Verdict::Admits)) => Ok((
            index.key(candidate).map_err(|error| error.to_string())?,
            None,
        )),
        Choice::Verdict(Ok(other)) => {
            Err(format!("healthy verdict unexpectedly returned {other:?}"))
        }
        Choice::Verdict(Err(error)) => Err(error.to_string()),
        Choice::Fit(fit) => fit
            .best
            .map(|key| (key, Some(fit.walked)))
            .ok_or_else(|| "healthy fit returned no node".into()),
    }
}

fn candidate(sequence: u64, nodes: u32) -> Result<NodeId, String> {
    let at = sequence
        .checked_rem(u64::from(nodes))
        .ok_or("cycles require at least one node")?;
    Ok(NodeId(
        u32::try_from(at).map_err(|_| "candidate node exceeds u32")?,
    ))
}

/// These warmup operations use the same choice and take/give APIs; their counters/times are not samples.
fn warmup(
    index: &mut FitIndex,
    common: super::Inputs,
    input: Inputs,
    pick: &Pick,
) -> Result<(), Failure> {
    for sequence in 0..input.warmup {
        let candidate = candidate(sequence, common.nodes)
            .map_err(|error| Failure::new("warmup", "input", error, None))?;
        let (expected, _) = selected(index, decide(index, &input, pick, candidate), candidate)
            .map_err(|error| Failure::new("warmup", "unhealthy", error, None))?;
        index
            .take(expected.node, WorkloadId(sequence), &input.demand)
            .map_err(|error| Failure::new("warmup.take", "refused", error.to_string(), None))?;
        let restored = index
            .give(expected.node, WorkloadId(sequence), &input.demand)
            .map_err(|error| Failure::new("warmup.give", "refused", error.to_string(), None))?;
        if restored != expected {
            return Err(Failure::new(
                "warmup.give",
                "unhealthy",
                "key did not restore",
                None,
            ));
        }
    }
    Ok(())
}

struct Cycle {
    sequence: u64,
    node: NodeId,
    walked: Option<u64>,
    decision: Sample,
    take: Sample,
    give: Sample,
}

fn measured_cycles(
    index: &mut FitIndex,
    common: super::Inputs,
    input: Inputs,
    pick: &Pick,
    samples: &mut Vec<Cycle>,
) -> Result<(), Failure> {
    for sequence in 0..input.repeats {
        let candidate = candidate(sequence, common.nodes)
            .map_err(|error| Failure::new("decision", "input", error, None))?;
        let (decision, choice) =
            measure(|| decide(std::hint::black_box(&*index), &input, pick, candidate));
        let choice = completed("decision", decision, choice)?;
        let (expected, walked) = selected(index, choice, candidate)
            .map_err(|error| Failure::new("decision", "unhealthy", error, Some(decision)))?;
        let (take, taken) = measure(|| {
            index.take(
                expected.node,
                WorkloadId(sequence),
                std::hint::black_box(&input.demand),
            )
        });
        let taken = completed("take", take, taken)?
            .map_err(|error| Failure::new("take", "refused", error.to_string(), Some(take)))?;
        std::hint::black_box(taken);
        let (give, restored) = measure(|| {
            index.give(
                expected.node,
                WorkloadId(sequence),
                std::hint::black_box(&input.demand),
            )
        });
        let restored = completed("give", give, restored)?
            .map_err(|error| Failure::new("give", "refused", error.to_string(), Some(give)))?;
        if restored != expected {
            return Err(Failure::new(
                "give",
                "unhealthy",
                "key did not restore",
                Some(give),
            ));
        }
        // Capacity for every sample is reserved before the constructor/operation snapshots.
        samples.push(Cycle {
            sequence,
            node: expected.node,
            walked,
            decision,
            take,
            give,
        });
    }
    Ok(())
}

fn percentile(sorted: &[u128], percentile: usize) -> Option<String> {
    // Nearest-rank percentiles: ceil(count*p/100), one-based. The values label reported statistics,
    // not workload sizes, sampling budgets, warmups or performance thresholds.
    let rank = sorted
        .len()
        .checked_mul(percentile)?
        .checked_add(99)?
        .checked_div(100)?;
    sorted.get(rank.checked_sub(1)?).map(ToString::to_string)
}

fn summary(samples: &[Cycle], sample: impl Fn(&Cycle) -> Sample) -> Result<Value, String> {
    let mut durations = Vec::new();
    durations
        .try_reserve_exact(samples.len())
        .map_err(|error| format!("summary reservation failed: {error}"))?;
    let (
        mut allocations,
        mut deallocations,
        mut reallocations,
        mut requested,
        mut failures,
        mut live,
        mut elapsed,
    ) = (0u64, 0u64, 0u64, 0u64, 0u64, 0i128, 0u128);
    for cycle in samples {
        let sample = sample(cycle);
        let b = sample.before;
        let a = sample.after;
        allocations = a
            .allocations
            .checked_sub(b.allocations)
            .and_then(|n| allocations.checked_add(n))
            .ok_or("allocation sum overflow")?;
        deallocations = a
            .deallocations
            .checked_sub(b.deallocations)
            .and_then(|n| deallocations.checked_add(n))
            .ok_or("deallocation sum overflow")?;
        reallocations = a
            .reallocations
            .checked_sub(b.reallocations)
            .and_then(|n| reallocations.checked_add(n))
            .ok_or("reallocation sum overflow")?;
        requested = a
            .requested_bytes
            .checked_sub(b.requested_bytes)
            .and_then(|n| requested.checked_add(n))
            .ok_or("requested-byte sum overflow")?;
        failures = a
            .allocation_failures
            .checked_sub(b.allocation_failures)
            .and_then(|n| failures.checked_add(n))
            .ok_or("failure sum overflow")?;
        live = i128::from(a.live_bytes)
            .checked_sub(i128::from(b.live_bytes))
            .and_then(|n| live.checked_add(n))
            .ok_or("live-byte sum overflow")?;
        elapsed = elapsed
            .checked_add(sample.elapsed.as_nanos())
            .ok_or("duration sum overflow")?;
        durations.push(sample.elapsed.as_nanos());
    }
    durations.sort_unstable();
    Ok(
        json!({"count":samples.len(),"allocations":allocations,"deallocations":deallocations,
        "reallocations":reallocations,"requested_bytes":requested,"allocation_failures":failures,
        "live_bytes_delta":live.to_string(),"duration_ns_sum":elapsed.to_string(),
        "latency_ns":{"method":"nearest-rank","min":durations.first().map(ToString::to_string),
            "p50":percentile(&durations,50),"p95":percentile(&durations,95),"p99":percentile(&durations,99),
            "max":durations.last().map(ToString::to_string)}}),
    )
}

fn metadata(common: super::Inputs, input: Inputs) -> Value {
    let classes: Vec<_> = Class::ALL
        .into_iter()
        .filter(|class| input.pool.classes.has(*class))
        .map(class_name)
        .collect();
    json!({"nodes":common.nodes,"cpu_milli":common.cpu_milli,"mem_bytes":common.mem_bytes,"rounds":common.rounds,
        "decision":input.decision.name(),"scorer":scorer_name(input.scorer),"scorer_applies":matches!(input.decision,Decision::Fit),
        "pool_classes":classes,"pool_cpu_milli":input.pool.capacity.cpu_milli,"pool_mem_bytes":input.pool.capacity.mem_bytes,
        "demand_class":class_name(input.demand.class),"demand_cpu_milli":input.demand.need.cpu_milli,
        "demand_mem_bytes":input.demand.need.mem_bytes,"kind":"plain","gpu_count":0,"disk_bytes":0,
        "repeats":input.repeats,"warmup_repeats":input.warmup})
}

pub(super) fn run(common: super::Inputs, input: Inputs) -> Result<(), String> {
    if common.nodes == 0 {
        return Err("cycles require --nodes greater than zero".into());
    }
    if !resources(common.cpu_milli, common.mem_bytes).holds(&input.demand.need) {
        return Err("healthy cycles require the host to hold the demand".into());
    }
    let mut nodes: Vec<NodeSpec> = super::input_nodes(common)?;
    for node in &mut nodes {
        node.runtime_pools = vec![input.pool];
    }
    let pick = Pick {
        scorer: input.scorer,
        shapes: Shapes::of(vec![(input.demand.need, input.demand.class)])
            .map_err(|error| error.to_string())?,
    };
    let mut samples = Vec::new();
    let count = usize::try_from(input.repeats).map_err(|_| "repeat count exceeds usize")?;
    samples
        .try_reserve_exact(count)
        .map_err(|error| format!("sample buffer reservation failed (not SUT): {error}"))?;
    let mut output = std::io::stdout().lock();
    for round in 0..common.rounds {
        samples.clear();
        let (constructor, constructed) = measure(|| FitIndex::new(std::hint::black_box(&nodes)));
        let index = completed("constructor", constructor, constructed).and_then(|result| {
            result.map_err(|error| {
                Failure::new(
                    "constructor",
                    "refused",
                    error.to_string(),
                    Some(constructor),
                )
            })
        });
        let mut index = match index {
            Ok(index) => index,
            Err(error) => {
                emit(
                    &mut output,
                    &json!({"event":"runtime_pool_cycles_round","round":round,"status":"failed",
                    "inputs":metadata(common,input),"failure":error.value(),"copied_bytes":null,"copy_measurement":"unmeasured"}),
                )?;
                return Err("pooled constructor failed; see round record".into());
            }
        };
        // Inputs, Pick/Shapes and the sample allocation all remain live across every SUT snapshot.
        let result = warmup(&mut index, common, input, &pick)
            .and_then(|()| measured_cycles(&mut index, common, input, &pick, &mut samples));
        let (drop_sample, dropped) = measure(|| drop(index));
        let drop_result = completed("drop", drop_sample, dropped);
        let retained = constructor
            .after
            .live_bytes
            .checked_sub(constructor.before.live_bytes);
        let valid = constructor.valid()
            && drop_sample.valid()
            && drop_sample.after.allocation_failures == constructor.before.allocation_failures
            && samples
                .iter()
                .all(|cycle| cycle.decision.valid() && cycle.take.valid() && cycle.give.valid())
            && retained.is_some()
            && drop_sample.after.live_bytes >= constructor.before.live_bytes;
        let okay = result.is_ok() && drop_result.is_ok() && valid && samples.len() == count;
        let raw: Vec<_> = samples
            .iter()
            .map(|cycle| {
                json!({"sequence":cycle.sequence,"node":cycle.node.0,"fit_walked":cycle.walked,
            "decision":cycle.decision.value(),"take":cycle.take.value(),"give":cycle.give.value()})
            })
            .collect();
        let failure = result
            .as_ref()
            .err()
            .or_else(|| drop_result.as_ref().err())
            .map(Failure::value);
        emit(
            &mut output,
            &json!({"event":"runtime_pool_cycles_round","round":round,"status":if okay{"ok"}else{"failed"},
            "inputs":metadata(common,input),"decision_scope":match input.decision{Decision::Verdict=>"single-node-admission",Decision::Fit=>"global-fit"},
            "accounting_valid":valid,"constructor":constructor.value(),"constructor_retained_live_bytes":retained,
            "drop":drop_sample.value(),"after_drop_live_bytes_delta":i128::from(drop_sample.after.live_bytes).checked_sub(i128::from(constructor.before.live_bytes)).map(|n|n.to_string()),
            "completed_cycles":samples.len(),"decision":summary(&samples,|cycle|cycle.decision)?,
            "take":summary(&samples,|cycle|cycle.take)?,"give":summary(&samples,|cycle|cycle.give)?,
            "samples":raw,"failure":failure,"copied_bytes":null,"copy_measurement":"unmeasured"}),
        )?;
        if !okay {
            return Err("pooled cycle round failed; see JSON and stderr markers".into());
        }
    }
    std::hint::black_box((&nodes, &pick, &samples));
    Ok(())
}
