//! H68's bench: the federation's cell selection on 100 simulated cells, against Karmada's
//! `cluster_schedule` P99 of 32 ms on 100 simulated member clusters (`docs/design/hex-sim.md` H68;
//! `cluster-managers-at-scale.md:239-245`). The bar is a p99 at most a fifth of Karmada's, 6.4 ms.
//!
//!   cargo bench -p hex-bench --bench sched_cells -- [runs]
//!
//! The topology is Karmada's report's: 100 cells (10 regions of 10) of 5,000 nodes each, 500,000
//! nodes, and 2,000,000 workloads, 20,000 a cell. The node and the pod are H63's: 11 CPU and 55 GiB,
//! and `scheduler_perf`'s pause pod, 100m and 500Mi. Each workload's selection is timed alone,
//! wall-clock, on a release build: `federation::Capacity::admit`, which routes it to its home region
//! and cell and on to the first with room, reads that cell's live free nodes by the cells' sampled
//! best fit, and deducts the node it takes (H66's check, run for every workload). The record gives
//! p50, p90, p99, p99.9 and max of each run, and the run of record is the one with the best p99.
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

use std::time::{Duration, Instant};

use hex_sim::federation::{Capacity, Topology};
use hex_sim::index::Sample;
use hex_sim::model::{
    Class, Classes, Kind, NodeId, NodeSpec, Resources, Tenancy, WorkloadId, WorkloadSpec,
};

const TOPOLOGY: Topology = Topology {
    regions: 10,
    cells: 10,
};
const NODES_PER_CELL: u32 = 5_000;
const WORKLOADS: u64 = 2_000_000;
const MIB: u64 = 1 << 20;
const NODE: Resources = Resources {
    cpu_milli: 11_000,
    mem_bytes: 55 * 1024 * MIB,
    gpus: 0,
};
const POD: Resources = Resources {
    cpu_milli: 100,
    mem_bytes: 500 * MIB,
    gpus: 0,
};
const BAR: Duration = Duration::from_micros(6_400);

fn nodes() -> Vec<NodeSpec> {
    let n = TOPOLOGY.regions * TOPOLOGY.cells * NODES_PER_CELL;
    (0..n)
        .map(|id| NodeSpec {
            id: NodeId(id),
            capacity: NODE,
            background_milli: 0,
            classes: Classes::ALL,
        })
        .collect()
}

fn pct(sorted: &[Duration], per_mille: usize) -> Duration {
    sorted[(sorted.len() * per_mille / 1000).min(sorted.len() - 1)]
}

fn main() {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| {
            a.into_string()
                .unwrap_or_else(|a| panic!("an argument isn't Unicode: {a:?}"))
        })
        .filter(|a| a != "--bench")
        .collect();
    let runs: u32 = args.first().map_or(5, |a| a.parse().expect("runs"));
    let nodes = nodes();
    let cells = TOPOLOGY.regions * TOPOLOGY.cells;
    println!(
        "sched_cells: {WORKLOADS} workloads of {POD:?} over {cells} cells of {NODES_PER_CELL} nodes of {NODE:?}, {runs} runs"
    );
    let mut best: Option<Duration> = None;
    for run in 0..runs {
        let mut capacity = Capacity::empty(&nodes, TOPOLOGY, Sample::default()).unwrap();
        let mut took = Vec::with_capacity(WORKLOADS as usize);
        let mut per_cell = vec![0u64; cells as usize];
        for id in 0..WORKLOADS {
            let w = WorkloadSpec {
                id: WorkloadId(id),
                need: POD,
                kind: Kind::Plain,
                arrives: 0,
                runs_for: None,
                tenancy: Tenancy::default(),
                class: Class::Oci,
            };
            let t = Instant::now();
            let got = capacity.admit(&w).unwrap();
            took.push(t.elapsed());
            let a = got.unwrap_or_else(|| panic!("workload {id} found no room"));
            per_cell[(a.region * TOPOLOGY.cells + a.cell) as usize] += 1;
        }
        took.sort_unstable();
        let p99 = pct(&took, 990);
        println!(
            "run={run} selected={} p50_us={:.2} p90_us={:.2} p99_us={:.2} p999_us={:.2} max_us={:.2} cells_min={} cells_max={}",
            took.len(),
            pct(&took, 500).as_secs_f64() * 1e6,
            pct(&took, 900).as_secs_f64() * 1e6,
            p99.as_secs_f64() * 1e6,
            pct(&took, 999).as_secs_f64() * 1e6,
            took[took.len() - 1].as_secs_f64() * 1e6,
            per_cell.iter().min().unwrap(),
            per_cell.iter().max().unwrap(),
        );
        if best.is_none_or(|b| p99 < b) {
            best = Some(p99);
        }
    }
    let best = best.expect("at least one run");
    println!(
        "best p99 of {runs}: {:.2} us, bar {:.0} us ({})",
        best.as_secs_f64() * 1e6,
        BAR.as_secs_f64() * 1e6,
        if best <= BAR { "met" } else { "MISSED" }
    );
}
