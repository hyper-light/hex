//! AC-T7.4.3's measure: stranded capacity at 90% load, Hex's scorer against best fit alone (G2 step 4).
//!
//!   cargo bench -p hex-bench --bench sched_packing -- [trace-file]
//!
//! It packs a recorded workload (`hex_sim::replay`'s format; by default the recorded kiln workload,
//! bench/sched/kiln-ci-20261022-20261105.trace) as the requests its legs would declare
//! (`hex_sim::packing::declared`), from 16 starting arrivals spread evenly over the recording (the
//! seeds), each to 90% load through the fast path's sampled choice (`hex_sim::packing::pack`). For
//! each seed and scorer it prints the stranded capacity, the arrivals passed over on the way, the load
//! at the first refusal for room, and Hex's scorer's reduction against each best fit: by free CPU
//! first (`BestFit`) and across resources (`MostAllocated`). It is deterministic: the same trace
//! gives the same table.
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

use hex_sim::index::{Sample, Scorer, Shapes};
use hex_sim::packing::{PackReport, declared, minimal_shapes, pack};

const SEEDS: usize = 16;
const SCORERS: [Scorer; 4] = [
    Scorer::BestFit,
    Scorer::MostAllocated,
    Scorer::Align,
    Scorer::Fragment,
];

fn cut(hex: &PackReport, base: &PackReport) -> f64 {
    let base = base.stranded_per_mille();
    if base == 0 {
        0.0
    } else {
        100.0 * (1.0 - hex.stranded_per_mille() as f64 / base as f64)
    }
}

fn main() {
    let path = std::env::args_os()
        .skip(1)
        .filter_map(|a| a.into_string().ok())
        .find(|a| a != "--bench")
        .unwrap_or_else(|| {
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../bench/sched/kiln-ci-20261022-20261105.trace"
            )
            .to_string()
        });
    let text = std::fs::read_to_string(&path).unwrap();
    let recorded = hex_sim::replay::parse(&text, &hex_sim::replay::Limits::default()).unwrap();
    let cluster = declared(&recorded).unwrap();
    let shapes = minimal_shapes(&cluster);
    let sample = Sample {
        shapes: Shapes::of(shapes.clone()),
        ..Sample::default()
    };
    println!(
        "{path}: {} nodes, {} legs, minimal shapes {:?}",
        cluster.nodes.len(),
        cluster.workloads.len(),
        shapes
    );
    println!(
        "seed from | stranded per-mille (arrivals passed over, load per-mille at the first refusal for room): best_fit most_allocated align fragment | fragment's cut vs best_fit, vs most_allocated"
    );
    let (mut min_best, mut min_most) = (f64::INFINITY, f64::INFINITY);
    let mut passed = [0u64; SCORERS.len()];
    for seed in 0..SEEDS {
        let from = seed * cluster.workloads.len() / SEEDS;
        let r: Vec<PackReport> = SCORERS
            .iter()
            .map(|&scorer| pack(&cluster, &Sample { scorer, ..sample }, 900, from).unwrap())
            .collect();
        let show = |p: &PackReport| {
            format!(
                "{}{} ({}, {})",
                p.stranded_per_mille(),
                if p.reached { "" } else { "*" },
                p.passed_over,
                p.first_full_per_mille
                    .map_or_else(|| "-".to_string(), |l| l.to_string())
            )
        };
        for (total, p) in passed.iter_mut().zip(&r) {
            *total += p.passed_over;
        }
        let (vs_best, vs_most) = (cut(&r[3], &r[0]), cut(&r[3], &r[1]));
        min_best = min_best.min(vs_best);
        min_most = min_most.min(vs_most);
        println!(
            "{seed:>4} {from:>5} | {} {} {} {} | {vs_best:.1}% {vs_most:.1}%",
            show(&r[0]),
            show(&r[1]),
            show(&r[2]),
            show(&r[3])
        );
    }
    println!(
        "fragment's least cut over {SEEDS} seeds: {min_best:.1}% vs best_fit, {min_most:.1}% vs most_allocated (AC-T7.4.3 asks >= 30%; * = 90% load not reached)"
    );
    println!(
        "arrivals passed over on the way, all seeds: best_fit {} most_allocated {} align {} fragment {}",
        passed[0], passed[1], passed[2], passed[3]
    );
}
