//! AC-T7.4.3's measure: stranded capacity at 90% load, Hex's scorer against best fit alone (G2 step 4).
//!
//!   cargo bench -p hex-bench --bench sched_packing
//!
//! It packs the recorded microVM workload (bench/sched/huawei_workload.md) from 16 starting arrivals
//! spread evenly over the day (the seeds), to 90% load through the fast path's sampled choice
//! (`hex_sim::packing::pack`), three ways: with no completions, so lifetimes don't enter; and with
//! completions, its arrivals 500 times faster than recorded, once with its sandboxes' lifetimes as
//! recorded (huawei-r5-day30.trace) and once with Huawei's one-minute keep-alive added
//! (huawei-r5-day30.keepalive.trace). Each runs under each arm: best fit by free CPU first (`BestFit`) and across resources (`MostAllocated`), Tetris's
//! alignment (`Align`), and least stranded then a tie: Tetris's alignment (the plan's), least
//! allocated, or most allocated (Hex's, `Scorer::HEX`). For each arm and seed it prints the stranded
//! capacity, the arrivals passed over on the way, the load at the first refusal for room, and the
//! arrivals passed over below half load with their needs; then each arm's reduction against both best
//! fits. It is deterministic: the same trace gives the same tables.
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
use hex_sim::model::{Cluster, Time};
use hex_sim::packing::{Clock, PackReport, minimal_shapes, pack};

const SEEDS: usize = 16;
/// The slowest of 20, 50, 100, 200, 500 and 1,000 times the recorded speed at which every seed reaches
/// 90% load with lifetimes as recorded (a model of this packing, 2026-11-06).
const SPEED: Time = 500;
const ARMS: [(&str, Scorer); 6] = [
    ("best_fit", Scorer::BestFit),
    ("most_allocated", Scorer::MostAllocated),
    ("align", Scorer::Align),
    ("stranded_then_align", Scorer::Fragment(Tie::Align)),
    ("stranded_then_least", Scorer::Fragment(Tie::LeastAllocated)),
    ("stranded_then_most (Hex)", Scorer::HEX),
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
                "{}m/{}Mi@{}{}",
                e.need.cpu_milli,
                e.need.mem_bytes >> 20,
                e.load_per_mille,
                if e.for_room { "" } else { "f" }
            )
        })
        .collect();
    format!("{} [{}]", p.early_passed_over, kept.join(" "))
}

fn trace(name: &str) -> Cluster {
    let path = format!("{}/../../bench/sched/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap();
    hex_sim::replay::parse(&text, &hex_sim::replay::Limits::default()).unwrap()
}

fn main() {
    let recorded = trace("huawei-r5-day30.trace");
    measure("no completions", &recorded, Clock::Held);
    measure(
        "completions at 500x, lifetimes as recorded",
        &recorded,
        Clock::Compressed(SPEED),
    );
    measure(
        "completions at 500x, with the keep-alive",
        &trace("huawei-r5-day30.keepalive.trace"),
        Clock::Compressed(SPEED),
    );
}

fn measure(name: &str, cluster: &Cluster, clock: Clock) {
    let shapes = minimal_shapes(cluster);
    let sample = Sample {
        shapes: Shapes::of(shapes.clone()),
        ..Sample::default()
    };
    println!(
        "== {name}: {} nodes, {} sandboxes, minimal shapes {shapes:?}",
        cluster.nodes.len(),
        cluster.workloads.len()
    );
    println!(
        "seed from arm | stranded per-mille (* = 90% load not reached) | passed over | first refusal for room at load per-mille | passed over below half load [millicores/MiB@load; f = by a filter]"
    );
    let runs: Vec<Vec<PackReport>> = (0..SEEDS)
        .map(|seed| {
            let from = seed * cluster.workloads.len() / SEEDS;
            ARMS.iter()
                .map(|&(_, scorer)| {
                    pack(cluster, &Sample { scorer, ..sample }, 900, from, clock).unwrap()
                })
                .collect()
        })
        .collect();
    for (seed, arms) in runs.iter().enumerate() {
        let from = seed * cluster.workloads.len() / SEEDS;
        for ((arm, _), p) in ARMS.iter().zip(arms) {
            println!(
                "{seed:>4} {from:>5} {arm} | {}{} | {} | {} | {}",
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
    for (i, (arm, _)) in ARMS.iter().enumerate() {
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
            "{arm} | {meets}/{SEEDS} | {least_best:.1}%, {least_most:.1}% | {passed} | {} | {early}",
            first.map_or_else(|| "-".to_string(), |l| l.to_string())
        );
    }
}
