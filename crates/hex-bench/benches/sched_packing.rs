//! AC-T7.4.3's measure: stranded capacity at 90% load, Hex's scorer against best fit alone (G2 step 4).
//!
//!   cargo bench -p hex-bench --bench sched_packing
//!
//! It packs the recorded kiln workload (bench/sched/kiln-ci-20261022-20261105.trace) as the requests its
//! legs would declare (`hex_sim::packing::whole_requests`), from 16 starting arrivals spread evenly over
//! the recording (the seeds), each to 90% load through the fast path's sampled choice
//! (`hex_sim::packing::pack`), under each arm: best fit by free CPU first (`BestFit`) and across
//! resources (`MostAllocated`), Tetris's alignment (`Align`), and least stranded then a tie: Tetris's
//! alignment (the plan's), least allocated (Hex's, `Scorer::HEX`) or most allocated. For each arm and
//! seed it prints the stranded capacity, the arrivals passed over on the way, the load at the first
//! refusal for room, and the arrivals passed over below half load with their needs; then each arm's
//! reduction against both best fits. It is deterministic: the same trace gives the same tables.
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

use hex_sim::index::{Sample, Scorer, Shapes, Tie};
use hex_sim::packing::{GIB, PackReport, minimal_shapes, pack, whole_requests};

const SEEDS: usize = 16;
const ARMS: [(&str, Scorer); 6] = [
    ("best_fit", Scorer::BestFit),
    ("most_allocated", Scorer::MostAllocated),
    ("align", Scorer::Align),
    ("stranded_then_align", Scorer::Fragment(Tie::Align)),
    ("stranded_then_least (Hex)", Scorer::HEX),
    ("stranded_then_most", Scorer::Fragment(Tie::MostAllocated)),
];

fn cut(arm: &PackReport, base: &PackReport) -> f64 {
    let base = base.stranded_per_mille();
    if base == 0 {
        0.0
    } else {
        100.0 * (1.0 - arm.stranded_per_mille() as f64 / base as f64)
    }
}

fn early(p: &PackReport) -> String {
    let kept: Vec<String> = p
        .early
        .iter()
        .flatten()
        .map(|e| {
            format!(
                "{}c/{}G@{}{}",
                e.need.cpu_milli / 1000,
                e.need.mem_bytes / GIB,
                e.load_per_mille,
                if e.for_room { "" } else { "f" }
            )
        })
        .collect();
    format!("{} [{}]", p.early_passed_over, kept.join(" "))
}

fn main() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../bench/sched/kiln-ci-20261022-20261105.trace"
    );
    let text = std::fs::read_to_string(path).unwrap();
    let recorded = hex_sim::replay::parse(&text, &hex_sim::replay::Limits::default()).unwrap();
    let cluster = whole_requests(&recorded).unwrap();
    let shapes = minimal_shapes(&cluster);
    let sample = Sample {
        shapes: Shapes::of(shapes.clone()),
        ..Sample::default()
    };
    println!(
        "{path}: {} nodes, {} legs, minimal shapes {shapes:?}",
        cluster.nodes.len(),
        cluster.workloads.len()
    );
    println!(
        "seed from arm | stranded per-mille (* = 90% load not reached) | passed over | first refusal for room at load per-mille | passed over below half load [cores/GiB@load; f = by a filter]"
    );
    let runs: Vec<Vec<PackReport>> = (0..SEEDS)
        .map(|seed| {
            let from = seed * cluster.workloads.len() / SEEDS;
            ARMS.iter()
                .map(|&(_, scorer)| {
                    pack(&cluster, &Sample { scorer, ..sample }, 900, from).unwrap()
                })
                .collect()
        })
        .collect();
    for (seed, arms) in runs.iter().enumerate() {
        let from = seed * cluster.workloads.len() / SEEDS;
        for ((name, _), p) in ARMS.iter().zip(arms) {
            println!(
                "{seed:>4} {from:>5} {name} | {}{} | {} | {} | {}",
                p.stranded_per_mille(),
                if p.reached { "" } else { "*" },
                p.passed_over,
                p.first_full_per_mille
                    .map_or_else(|| "-".to_string(), |l| l.to_string()),
                early(p)
            );
        }
    }
    println!(
        "arm | seeds at least 30% below both best fits | least cut vs best_fit, vs most_allocated | passed over | earliest first refusal for room | passed over below half load"
    );
    for (i, (name, _)) in ARMS.iter().enumerate() {
        let cuts: Vec<(f64, f64)> = runs
            .iter()
            .map(|arms| (cut(&arms[i], &arms[0]), cut(&arms[i], &arms[1])))
            .collect();
        let meets = cuts
            .iter()
            .filter(|(b, m)| *b >= 30.0 && *m >= 30.0)
            .count();
        let least_best = cuts.iter().map(|c| c.0).fold(f64::INFINITY, f64::min);
        let least_most = cuts.iter().map(|c| c.1).fold(f64::INFINITY, f64::min);
        let passed: u64 = runs.iter().map(|arms| arms[i].passed_over).sum();
        let first = runs
            .iter()
            .filter_map(|arms| arms[i].first_full_per_mille)
            .min();
        let early: u64 = runs.iter().map(|arms| arms[i].early_passed_over).sum();
        println!(
            "{name} | {meets}/{SEEDS} | {least_best:.1}%, {least_most:.1}% | {passed} | {} | {early}",
            first.map_or_else(|| "-".to_string(), |l| l.to_string())
        );
    }
}
