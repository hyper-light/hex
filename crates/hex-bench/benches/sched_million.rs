//! AC-T7.11.2's bench: wall time to place 1,000,000 pending workloads on 10,000 nodes, on a release
//! build, against a bar of 120 s (G6 step 4; the method is `docs/design/hex-sim.md` H13).
//!
//!   cargo bench -p hex-bench --bench sched_million -- [runs] [slices] [pods|mixed] [check]
//!
//! The cluster is H13's: 10,000 nodes of 11 CPU and 55 GiB (`scheduler_perf`'s node sized to hold
//! its 110-pod limit of its pod), no co-tenants. Row `pods` is 1,000,000 of `scheduler_perf`'s pause
//! pod, 100m and 500Mi, filling the 1,100,000 slots to 91%. Row `mixed` draws each need uniformly
//! from 1-199m and 1-999 MiB (the same means, so the same budget) and its kind as hex-sim's generator
//! does (40% plain, 25% builds, 15% benchmarks, 20% sessions), so H9's filters run and a one-class
//! best case never stands alone.
//!
//! The control plane is sharded as H10 decides, by hex-sim's own `sim::Layout`: the nodes are split
//! into `slices` contiguous ranges, and each workload is routed to the first slice, from slice
//! `id mod slices` on, holding a node that would admit it empty. Every workload is submitted to the one store before the clock starts, so all
//! 1,000,000 are pending at once and none ends. Then one thread per slice runs hex-sim's own fast path
//! over its slice's pending workloads in submit order: the sampled best fit over the slice's index
//! (`FitIndex::sampled_fit`, kube-scheduler's floors of 100 nodes or 5%), the commit to the one store
//! by compare-and-set on the version the submit returned (`Store::apply`, behind a mutex: the store is
//! the planes' one shared point, as Amphora's will be), and the index update. The time runs from the
//! first decision to the last commit, wall-clock.
//!
//! With `check`, after each timed run (untimed), each slice's part of the store's log is replayed, in
//! the store's order, into a store of that slice alone, and the serial specification judges it
//! (`spec::check_log`, H4 per slice): each placement the slice's sampled best fit at its point, no
//! queue jumped, nothing left pending that a node holds.
// A bench (not production, CLAUDE.md): it takes the test-style allowances, as the other benches do.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::disallowed_macros,
    clippy::print_stdout,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]

use std::fmt::Write as _;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use hex_sim::index::{FitIndex, Sample};
use hex_sim::model::{Kind, NodeId, NodeSpec, Resources, WorkloadId, WorkloadSpec};
use hex_sim::rng::Rng;
use hex_sim::sim::{Layout, Report, Run};
use hex_sim::spec;
use hex_sim::store::{Op, Outcome, State, Store, Write};

const NODES: u32 = 10_000;
const WORKLOADS: u64 = 1_000_000;
const MIB: u64 = 1 << 20;
const NODE: Resources = Resources {
    cpu_milli: 11_000,
    mem_bytes: 55 * 1024 * MIB,
    gpus: 0,
};
const BAR: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    Pods,
    Mixed,
}

fn workloads(row: Row, seed: u64) -> Vec<WorkloadSpec> {
    let mut rng = Rng::new(seed);
    (0..WORKLOADS)
        .map(|id| {
            let (need, kind) = match row {
                Row::Pods => (
                    Resources {
                        cpu_milli: 100,
                        mem_bytes: 500 * MIB,
                        gpus: 0,
                    },
                    Kind::Plain,
                ),
                Row::Mixed => {
                    let need = Resources {
                        cpu_milli: rng.range(1, 199),
                        mem_bytes: rng.range(1, 999) * MIB,
                        gpus: 0,
                    };
                    let kind = match rng.range(0, 999) {
                        0..400 => Kind::Plain,
                        400..650 => Kind::Build,
                        650..800 => Kind::Benchmark,
                        _ => Kind::Session,
                    };
                    (need, kind)
                }
            };
            WorkloadSpec {
                id: WorkloadId(id),
                need,
                kind,
                arrives: 0,
                runs_for: None,
            }
        })
        .collect()
}

/// One slice: its first global node id, its nodes (ids dense from 0 within it), and its pending
/// workloads in submit order with the version each submit returned.
struct Slice {
    base: u32,
    nodes: Vec<NodeSpec>,
    pending: Vec<(WorkloadSpec, u64)>,
}

struct Placed {
    placed: usize,
    unplaced: usize,
    began: Instant,
    ended: Instant,
}

/// The nodes, the slices with their routed workloads, and the one store with every workload
/// submitted.
fn setup(row: Row, seed: u64, slices: u32) -> (Vec<NodeSpec>, Vec<Slice>, Store) {
    let nodes: Vec<NodeSpec> = (0..NODES)
        .map(|id| NodeSpec {
            id: NodeId(id),
            capacity: NODE,
            background_milli: 0,
        })
        .collect();
    let layout = Layout::new(&nodes, slices).unwrap();
    let mut base = 0;
    let mut parts: Vec<Slice> = (0..layout.len())
        .map(|s| {
            let nodes = layout.nodes(s).unwrap().to_vec();
            let part = Slice {
                base,
                nodes,
                pending: Vec::new(),
            };
            base += part.nodes.len() as u32;
            part
        })
        .collect();
    let work = workloads(row, seed);
    let mut store = Store::new(&nodes, work.len(), work.len() * 2).unwrap();
    for w in work {
        let to = layout.route(&w).expect("every workload fits an empty node");
        let Outcome::Applied { version, .. } = store.apply(0, Write::Submit(w)).unwrap() else {
            panic!("submit of {:?}", w.id)
        };
        parts[to].pending.push((w, version));
    }
    (nodes, parts, store)
}

/// The fast path over one slice's pending workloads, in submit order.
fn place(slice: &Slice, store: &Mutex<Store>, sample: &Sample) -> Placed {
    let mut index = FitIndex::new(&slice.nodes).unwrap();
    let began = Instant::now();
    let (mut placed, mut unplaced) = (0, 0);
    for (spec, version) in &slice.pending {
        let demand = spec.demand();
        let Some(best) = index.sampled_fit(&demand, sample).best else {
            unplaced += 1;
            continue;
        };
        let write = Write::Place {
            workload: spec.id,
            node: NodeId(slice.base + best.node.0),
            version: *version,
        };
        match store.lock().unwrap().apply(0, write).unwrap() {
            Outcome::Applied { .. } => {}
            other => panic!(
                "the store refused the index's choice for {:?}: {other:?}",
                spec.id
            ),
        }
        index.take(best.node, &demand).unwrap();
        placed += 1;
    }
    Placed {
        placed,
        unplaced,
        began,
        ended: Instant::now(),
    }
}

/// The serial specification over one slice's part of the store's log, replayed into a store of that
/// slice alone; and every workload routed there placed.
fn check(slice: &Slice, store: &Store, sample: Sample) -> Result<u64, String> {
    let hi = slice.base + slice.nodes.len() as u32;
    let ours = |w: WorkloadId| {
        slice
            .pending
            .binary_search_by_key(&w, |(s, _)| s.id)
            .is_ok()
    };
    let mut own = Store::new(&slice.nodes, slice.pending.len(), slice.pending.len() * 2).unwrap();
    let mut versions = std::collections::HashMap::with_capacity(slice.pending.len());
    for entry in store.log() {
        match entry.op {
            Op::Submit { workload, .. } if ours(workload) => {
                let spec = store.record(workload).unwrap().spec;
                let Outcome::Applied { version, .. } =
                    own.apply(entry.at, Write::Submit(spec)).unwrap()
                else {
                    return Err(format!("{workload:?} submitted twice"));
                };
                versions.insert(workload, version);
            }
            Op::Place { workload, node }
                if ours(workload) != (slice.base..hi).contains(&node.0) =>
            {
                return Err(format!(
                    "{workload:?} placed on {node:?} across a slice's edge"
                ));
            }
            Op::Place { workload, node } if ours(workload) => {
                let write = Write::Place {
                    workload,
                    node: NodeId(node.0.checked_sub(slice.base).ok_or("node below slice")?),
                    version: versions[&workload],
                };
                match own.apply(entry.at, write).unwrap() {
                    Outcome::Applied { version, .. } => versions.insert(workload, version),
                    other => return Err(format!("replay of {workload:?}: {other:?}")),
                };
            }
            Op::Submit { .. } | Op::Place { .. } => {}
            Op::Complete { .. } => return Err("a completion in a run where none ends".into()),
        }
    }
    let run = Run {
        report: Report::default(),
        store: own,
        starts: Vec::new(),
        acked: Vec::new(),
        refused: Vec::new(),
        ran: Vec::new(),
        sample,
        planes: Vec::new(),
    };
    let report = spec::check_log(&slice.nodes, &run).map_err(|v| v.to_string())?;
    if report.submits != slice.pending.len() as u64 || report.placements != report.submits {
        return Err(format!(
            "{} routed, {} submitted, {} placed",
            slice.pending.len(),
            report.submits,
            report.placements
        ));
    }
    Ok(report.placements)
}

fn main() {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .filter_map(|a| a.into_string().ok())
        .filter(|a| a != "--bench")
        .collect();
    let runs: u64 = args.first().and_then(|a| a.parse().ok()).unwrap_or(5);
    let slices: u32 = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(8);
    let row = if args.iter().any(|a| a == "mixed") {
        Row::Mixed
    } else {
        Row::Pods
    };
    let checking = args.iter().any(|a| a == "check");
    let cpus = std::thread::available_parallelism().map_or(0, std::num::NonZero::get);
    let sample = Sample::default();
    println!(
        "sched_million: {WORKLOADS} pending {} on {NODES} nodes of 11 CPU/55 GiB, {slices} slices on {cpus} CPUs, {runs} runs, check={checking}",
        if row == Row::Pods {
            "pods of 100m/500Mi"
        } else {
            "mixed workloads"
        }
    );
    let mut best: Option<(Duration, String)> = None;
    let mut all_ok = true;
    for seed in 0..runs {
        let built = Instant::now();
        let (_nodes, parts, store) = setup(row, seed, slices);
        let setup_s = built.elapsed().as_secs_f64();
        let store = Mutex::new(store);
        let done: Vec<Placed> = std::thread::scope(|s| {
            let handles: Vec<_> = parts
                .iter()
                .map(|part| {
                    let (store, sample) = (&store, &sample);
                    s.spawn(move || place(part, store, sample))
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let store = store.into_inner().unwrap();
        let began = done.iter().map(|p| p.began).min().unwrap();
        let ended = done.iter().map(|p| p.ended).max().unwrap();
        let wall = ended.saturating_duration_since(began);
        let slowest = done
            .iter()
            .map(|p| p.ended.saturating_duration_since(p.began))
            .max()
            .unwrap();
        let placed: usize = done.iter().map(|p| p.placed).sum();
        let unplaced: usize = done.iter().map(|p| p.unplaced).sum();
        let routed: Vec<usize> = parts.iter().map(|p| p.pending.len()).collect();
        let in_store = store
            .records()
            .filter(|r| matches!(r.state, State::Placed(_)))
            .count();
        let mut line = format!(
            "run seed={seed} placed={placed} unplaced={unplaced} store_placed={in_store} wall_s={:.3} slowest_slice_s={:.3} placements_per_s={:.0} setup_s={setup_s:.1} routed={routed:?}",
            wall.as_secs_f64(),
            slowest.as_secs_f64(),
            placed as f64 / wall.as_secs_f64(),
        );
        let mut ok = unplaced == 0 && placed as u64 == WORKLOADS && in_store as u64 == WORKLOADS;
        if checking {
            let checked = Instant::now();
            let verdicts: Vec<Result<u64, String>> = std::thread::scope(|s| {
                let handles: Vec<_> = parts
                    .iter()
                    .map(|part| {
                        let store = &store;
                        s.spawn(move || check(part, store, sample))
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap()).collect()
            });
            let placements: u64 = verdicts.iter().filter_map(|v| v.as_ref().ok()).sum();
            let failed: Vec<String> = verdicts.into_iter().filter_map(Result::err).collect();
            ok &= failed.is_empty() && placements == WORKLOADS;
            let verdict = if failed.is_empty() {
                "accepted".to_string()
            } else {
                format!("REJECTED {failed:?}")
            };
            write!(
                line,
                " spec_placements={placements} spec={verdict} check_s={:.1}",
                checked.elapsed().as_secs_f64()
            )
            .unwrap();
        }
        all_ok &= ok;
        println!("{line}");
        if ok && best.as_ref().is_none_or(|(b, _)| wall < *b) {
            best = Some((wall, line));
        }
    }
    match best {
        Some((wall, line)) => {
            println!("best-of-{runs} (by wall time): {line}");
            println!(
                "AC-T7.11.2 bar wall <= {} s, every workload placed{} in every run: {}",
                BAR.as_secs(),
                if checking { ", spec accepted" } else { "" },
                if wall <= BAR && all_ok {
                    "MET"
                } else {
                    "NOT MET"
                }
            );
        }
        None => println!("AC-T7.11.2: NOT MET (no run placed every workload)"),
    }
}
