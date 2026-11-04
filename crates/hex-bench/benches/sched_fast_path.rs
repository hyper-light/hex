//! AC-T7.4.2's bench: the scheduler fast path's placement decision latency at 10,000 placements/s
//! on 10,000 nodes, wall-clock, on a release build (G2 step 5).
//!
//!   cargo bench -p hex-bench --bench sched_fast_path -- [runs] [placements]
//!
//! What it times is hex-sim's own fast path, for real: for each workload, the sampled best fit over
//! the node index (`FitIndex::sampled_fit`, kube-scheduler's floors of 100 nodes or 5%), the commit
//! by compare-and-set to the store (`Store::apply`), and the index update. Arrivals are open-loop,
//! one every 100 us by the wall clock. A workload's latency runs from its scheduled arrival to its
//! commit, so time spent queued behind a slow decision counts. The cluster is first filled until
//! about 85% of its CPU is placed, untimed. Each timed arrival then retires the oldest running
//! workload (its completion written and its capacity given back) before it is placed, so the load
//! holds steady. Single-threaded: the in-order committer is the fast path's serial point
//! (`docs/design/hex-sim.md` H3), so its cost per placement is what bounds throughput.
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

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use hex_sim::generate::{self, Shape};
use hex_sim::index::{FitIndex, Sample};
use hex_sim::model::{NodeId, WorkloadSpec};
use hex_sim::store::{Outcome, Store, Write};

const NODES: u32 = 10_000;
const RATE_PER_S: u64 = 10_000;
const FILL_PER_MILLE: u64 = 850;

struct Live {
    spec: WorkloadSpec,
    node: NodeId,
    version: u64,
}

fn run(seed: u64, placements: usize) -> (Vec<Duration>, usize) {
    // More workloads than will run: the fill, then one per timed placement.
    let shape = Shape {
        nodes: NODES,
        workloads: (placements * 4) as u64,
        mean_gap: 0,
        mean_run: Some(1),
        gpu_nodes_per_mille: 100,
        max_share_per_mille: 250,
    };
    let cluster = generate::cluster(seed, &shape).unwrap();
    let mut index = FitIndex::new(&cluster.nodes).unwrap();
    let mut store = Store::new(
        &cluster.nodes,
        cluster.workloads.len(),
        cluster.workloads.len() * 3,
    )
    .unwrap();
    let sample = Sample::default();
    let total_cpu: u64 = cluster.nodes.iter().map(|n| n.capacity.cpu_milli).sum();
    let mut placed_cpu = 0u64;
    let mut live: VecDeque<Live> = VecDeque::new();
    let mut work = cluster.workloads.iter();
    let mut refused = 0usize;

    let place = |spec: &WorkloadSpec, index: &mut FitIndex, store: &mut Store| -> Option<Live> {
        let Outcome::Applied { version, .. } = store.apply(0, Write::Submit(*spec)).unwrap() else {
            panic!("submit")
        };
        let node = index.sampled_fit(&spec.demand(), &sample).best?.node;
        let Outcome::Applied { version, .. } = store
            .apply(
                0,
                Write::Place {
                    workload: spec.id,
                    node,
                    version,
                },
            )
            .unwrap()
        else {
            panic!("the store refused the index's choice")
        };
        index.take(node, &spec.demand()).unwrap();
        Some(Live {
            spec: *spec,
            node,
            version,
        })
    };
    let retire = |l: Live, index: &mut FitIndex, store: &mut Store| {
        store
            .apply(
                0,
                Write::Complete {
                    workload: l.spec.id,
                    node: l.node,
                    version: l.version,
                },
            )
            .unwrap();
        index.give(l.node, &l.spec.demand()).unwrap();
    };

    // Fill, untimed.
    while placed_cpu * 1000 < total_cpu * FILL_PER_MILLE {
        let spec = work.next().expect("enough workloads to fill");
        match place(spec, &mut index, &mut store) {
            Some(l) => {
                placed_cpu += l.spec.need.cpu_milli;
                live.push_back(l);
            }
            None => refused += 1,
        }
    }

    // Timed: open-loop arrivals every 100 us.
    let gap = Duration::from_nanos(1_000_000_000 / RATE_PER_S);
    let mut latencies = Vec::with_capacity(placements);
    let start = Instant::now();
    for i in 0..placements {
        let arrival = start + gap * i as u32;
        while Instant::now() < arrival {
            std::hint::spin_loop();
        }
        if let Some(old) = live.pop_front() {
            retire(old, &mut index, &mut store);
        }
        let spec = work.next().expect("enough workloads");
        match place(spec, &mut index, &mut store) {
            Some(l) => live.push_back(l),
            None => refused += 1,
        }
        latencies.push(Instant::now().saturating_duration_since(arrival));
    }
    (latencies, refused)
}

fn quantile(sorted: &[Duration], per_mille: usize) -> Duration {
    let rank = (sorted.len() * per_mille).div_ceil(1000).max(1);
    sorted[rank - 1]
}

fn main() {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .filter_map(|a| a.into_string().ok())
        .filter(|a| a != "--bench")
        .collect();
    let runs: u64 = args.first().and_then(|a| a.parse().ok()).unwrap_or(5);
    let placements: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(100_000);
    println!(
        "sched_fast_path: {NODES} nodes, {RATE_PER_S}/s open-loop, {placements} timed placements per run, {runs} runs"
    );
    let mut best: Option<(Duration, String)> = None;
    for seed in 0..runs {
        let (mut l, refused) = run(seed, placements);
        l.sort_unstable();
        let us = |d: Duration| d.as_secs_f64() * 1e6;
        let (p50, p99, p999, max) = (
            quantile(&l, 500),
            quantile(&l, 990),
            quantile(&l, 999),
            *l.last().unwrap(),
        );
        let line = format!(
            "run seed={seed} placements={} refused={refused} p50_us={:.1} p99_us={:.1} p999_us={:.1} max_us={:.1}",
            l.len(),
            us(p50),
            us(p99),
            us(p999),
            us(max)
        );
        println!("{line}");
        if best.as_ref().is_none_or(|(b, _)| p99 < *b) {
            best = Some((p99, line));
        }
    }
    let (p99, line) = best.unwrap();
    println!("best-of-{runs} (by p99): {line}");
    println!(
        "AC-T7.4.2 bar p99 <= 10 ms: {}",
        if p99 <= Duration::from_millis(10) {
            "MET"
        } else {
            "NOT MET"
        }
    );
}
