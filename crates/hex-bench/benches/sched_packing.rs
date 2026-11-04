//! AC-T7.4.3's measure: stranded capacity at 90% load, Hex's scorer against best fit alone (G2 step 4).
//!
//!   cargo bench -p hex-bench --bench sched_packing -- [trace-file]
//!
//! With a trace (a recorded workload, `hex_sim::replay`'s format) it packs that. Without one it packs
//! generated clusters with needs rounded to power-of-two cores and GiB, which show how the scorers
//! compare but are not the number of record (the bar names a recorded kiln workload). Each scorer packs the same arrivals through the fast path's
//! sampled choice to 90% of the CPU (`hex_sim::packing::pack`), and the stranded CPU of each is
//! printed with the reduction.
// A bench (not production, CLAUDE.md): it takes the test-style allowances, as the other benches do.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::disallowed_macros,
    clippy::print_stdout,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss
)]

use hex_sim::generate::{self, Shape};
use hex_sim::index::{Sample, Scorer, Shapes};
use hex_sim::model::Cluster;
use hex_sim::packing::{minimal_shapes, pack, stranded_per_mille};

fn compare(name: &str, cluster: &Cluster) -> f64 {
    let base = Sample::default();
    let shapes = Shapes::of(minimal_shapes(cluster));
    let run = |scorer| {
        pack(
            cluster,
            &Sample {
                scorer,
                shapes,
                ..base
            },
            900,
        )
        .unwrap()
    };
    let best = run(Scorer::BestFit);
    let cut = |r: &hex_sim::packing::PackReport| {
        if best.stranded_cpu_milli == 0 {
            0.0
        } else {
            100.0 * (1.0 - r.stranded_cpu_milli as f64 / best.stranded_cpu_milli as f64)
        }
    };
    let align = run(Scorer::Align);
    let fragment = run(Scorer::Fragment);
    println!(
        "{name}: nodes={} shapes={} best_fit stranded={}‰ (reached {}) | align {}‰ (reached {}) cut {:.1}% | fragment {}‰ (reached {}) cut {:.1}%",
        cluster.nodes.len(),
        shapes.iter().count(),
        stranded_per_mille(&best),
        best.reached,
        stranded_per_mille(&align),
        align.reached,
        cut(&align),
        stranded_per_mille(&fragment),
        fragment.reached,
        cut(&fragment)
    );
    cut(&fragment)
}

fn main() {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .filter_map(|a| a.into_string().ok())
        .filter(|a| a != "--bench")
        .collect();
    if let Some(path) = args.first() {
        let text = std::fs::read_to_string(path).unwrap();
        let cluster = hex_sim::replay::parse(&text, &hex_sim::replay::Limits::default()).unwrap();
        compare(path, &cluster);
        return;
    }
    let mut cuts = Vec::new();
    for seed in 0..8 {
        let shape = Shape {
            nodes: 1_000,
            workloads: 200_000,
            mean_gap: 1,
            mean_run: None,
            gpu_nodes_per_mille: 100,
            max_share_per_mille: 400,
        };
        let mut cluster = generate::cluster(seed, &shape).unwrap();
        // CI legs ask for whole cores and round memory: round each need up to a power of two cores
        // and GiB, so the shapes are few and coarse, as a recorded workload's would be.
        for w in &mut cluster.workloads {
            w.need.cpu_milli = w.need.cpu_milli.div_ceil(1000).next_power_of_two() * 1000;
            w.need.mem_bytes = w.need.mem_bytes.div_ceil(1 << 30).next_power_of_two() << 30;
        }
        cuts.push(compare(&format!("generated seed={seed}"), &cluster));
    }
    let min = cuts.iter().copied().fold(f64::INFINITY, f64::min);
    println!(
        "generated: reduction min {min:.1}% over {} seeds (AC-T7.4.3 asks >= 30% on a recorded kiln workload)",
        cuts.len()
    );
}
