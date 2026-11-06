//! H68's bench: the federation's cell selection on 100 simulated cells, against Karmada's
//! `cluster_schedule` P99 of 32 ms on 100 simulated member clusters (`docs/design/hex-sim.md` H68;
//! `cluster-managers-at-scale.md:239-245`). The bar is a p99 at most a fifth of Karmada's, 6.4 ms.
//!
//!   cargo bench -p hex-bench --bench sched_cells -- [runs]
//!
//! The topology is Karmada's report's: 100 cells (10 regions of 10) of 5,000 nodes each, 500,000
//! nodes, and 2,000,000 workloads, 20,000 a cell on average. The node and the pod are H63's: 11 CPU
//! and 55 GiB, and `scheduler_perf`'s pause pod, 100m and 500Mi. In Hex's classes (H69): each region
//! one platform, six Linux (all three classes), two Windows (OCI, Shards), two macOS (Shards); each
//! pod an OCI container, a Firecracker or a Shards microVM by owner, a third each, with its class's
//! overhead, selected only into a cell that runs its class. Each workload's selection is timed alone,
//! wall-clock, on a release build: `federation::Capacity::admit`, which routes it to its home region
//! and cell and on to the first with room, reads that cell's live free nodes by the cells' sampled
//! best fit, and deducts the node it takes (H66's check, run for every workload). The record gives
//! p50, p90, p99, p99.9 and max of each run, over all and per class, and the run of record is the one
//! with the best p99 over all.
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
use hex_sim::generate;
use hex_sim::index::Sample;
use hex_sim::model::{
    Class, Cluster, Kind, NodeId, NodeSpec, Resources, Tenancy, WorkloadId, WorkloadSpec,
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

/// Each region one platform, in the generator's proportions over the ten regions (H69): regions 0-5
/// Linux (all three classes), 6-7 Windows (OCI and Shards), 8-9 macOS (Shards).
fn nodes() -> Vec<NodeSpec> {
    let per_region = TOPOLOGY.cells * NODES_PER_CELL;
    (0..TOPOLOGY.regions * per_region)
        .map(|id| NodeSpec {
            id: NodeId(id),
            capacity: NODE,
            background_milli: 0,
            classes: generate::platform(id / per_region),
        })
        .collect()
}

/// The workloads, each one of G2's classes by owner, a third each, with its class's overhead.
fn workloads() -> Vec<WorkloadSpec> {
    let work = (0..WORKLOADS)
        .map(|id| WorkloadSpec {
            id: WorkloadId(id),
            need: POD,
            kind: Kind::Plain,
            arrives: 0,
            runs_for: None,
            tenancy: Tenancy::default(),
            class: Class::Oci,
        })
        .collect();
    let mut c = Cluster::single_owner(Vec::new(), work);
    generate::owners(&mut c, 7, 75).unwrap();
    generate::by_owner(&mut c, generate::overheads()).unwrap();
    c.workloads
}

const CLASSES: [Class; 3] = [Class::Oci, Class::Firecracker, Class::Shards];

fn line(took: &mut [Duration]) -> String {
    took.sort_unstable();
    format!(
        "n={} p50_us={:.2} p90_us={:.2} p99_us={:.2} p999_us={:.2} max_us={:.2}",
        took.len(),
        pct(took, 500).as_secs_f64() * 1e6,
        pct(took, 900).as_secs_f64() * 1e6,
        pct(took, 990).as_secs_f64() * 1e6,
        pct(took, 999).as_secs_f64() * 1e6,
        took[took.len() - 1].as_secs_f64() * 1e6,
    )
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
    let work = workloads();
    let cells = TOPOLOGY.regions * TOPOLOGY.cells;
    println!(
        "sched_cells: {WORKLOADS} workloads of {POD:?} over {cells} cells of {NODES_PER_CELL} nodes of {NODE:?}, {runs} runs"
    );
    let mut best: Option<Duration> = None;
    for run in 0..runs {
        let mut capacity = Capacity::empty(&nodes, TOPOLOGY, Sample::default()).unwrap();
        let mut took = Vec::with_capacity(WORKLOADS as usize);
        let mut by_class: [Vec<Duration>; 3] = Default::default();
        let mut per_cell = vec![0u64; cells as usize];
        for w in &work {
            let t = Instant::now();
            let got = capacity.admit(w).unwrap();
            let d = t.elapsed();
            took.push(d);
            by_class[w.class as usize].push(d);
            let a = got.unwrap_or_else(|| panic!("workload {:?} found no room", w.id));
            let cell = (a.region * TOPOLOGY.cells + a.cell) as usize;
            assert!(
                generate::platform(a.region).has(w.class),
                "{:?} placed in region {} which doesn't run {:?}",
                w.id,
                a.region,
                w.class
            );
            per_cell[cell] += 1;
        }
        let all = line(&mut took);
        let p99 = pct(&took, 990);
        println!(
            "run={run} all {all} cells_min={} cells_max={}",
            per_cell.iter().min().unwrap(),
            per_cell.iter().max().unwrap(),
        );
        for (c, t) in CLASSES.iter().zip(by_class.iter_mut()) {
            println!("run={run} class={c:?} {}", line(t));
        }
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
