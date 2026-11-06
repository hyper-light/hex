//! AC-T7.4.3's measure: stranded capacity at 90% load, Hex's scorer against best fit alone (G2 step 4).
//!
//!   cargo bench -p hex-bench --bench sched_packing
//!
//! It packs the recorded workload (bench/sched/huawei_workload.md), each launch an OCI container, a
//! Firecracker microVM or a Shards microVM, on each of its two node models: EC2 m5d.metal hosts that
//! run every class (huawei-r5-day30.trace, the main one) and Hex's fleet's host types five times
//! over, each running the classes its OS runs (huawei-r5-day30.fleet.trace); and, as context, on the
//! fleet as it is, one of each host. On each, it packs the mix and each class alone
//! (`hex_sim::packing::alone`), from 16 starting arrivals spread evenly over the day (the seeds), to
//! 90% load through the fast path's sampled choice (`hex_sim::packing::pack`), three ways: with no
//! completions, so lifetimes don't enter; and with completions, once with lifetimes as recorded and
//! once with Huawei's one-minute keep-alive added (the `.keepalive.trace` beside each).
//! At the recorded speed the day never loads the nodes to 90%, so arrivals come faster: for each
//! set and trace, the slowest speed of [`LADDER`] at which Hex and both best fits reach 90% on every
//! seed, so lifetimes enter as far as the nodes allow. It prints the ladder first, each speed's seeds reaching 90% by arm, its fewest completions on the
//! way, the seeds on which Hex meets the bar, and the seeds on which a best fit strands nothing (so
//! none can be cut).
//!
//! Each clock runs under each arm: best fit by free CPU first (`BestFit`) and across resources
//! (`MostAllocated`), Tetris's alignment (`Align`), and least stranded then a tie: Tetris's
//! alignment (the plan's), least allocated, most allocated, or the fewest classes then most
//! allocated (Hex's, `Scorer::HEX`). For each arm and seed it prints the stranded capacity, the
//! arrivals passed over on the way, the completions, the load at the first refusal for room, and the
//! arrivals passed over below half load with their needs; then each arm's reduction against both
//! best fits. It is deterministic: the same trace gives the same tables.
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
use hex_sim::model::{Class, Cluster, Time};
use hex_sim::packing::{Clock, PackReport, alone, minimal_shapes, pack};

const SEEDS: usize = 16;
/// The speeds tried, as multiples of the recorded arrival rate.
const LADDER: [Time; 14] = [
    10, 20, 30, 40, 50, 60, 75, 100, 150, 200, 300, 500, 1_000, 2_000,
];
const ARMS: [(&str, Scorer); 7] = [
    ("best_fit", Scorer::BestFit),
    ("most_allocated", Scorer::MostAllocated),
    ("align", Scorer::Align),
    ("stranded_then_align", Scorer::Fragment(Tie::Align)),
    ("stranded_then_least", Scorer::Fragment(Tie::LeastAllocated)),
    ("stranded_then_most", Scorer::Fragment(Tie::MostAllocated)),
    ("stranded_then_narrowest (Hex)", Scorer::HEX),
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
    for (model, name, hosts) in [
        ("m5d.metal", "huawei-r5-day30", None),
        ("fleet", "huawei-r5-day30.fleet", None),
        // The fleet as it is, one of each host, as context (bench/sched/packing.md): the fleet trace
        // lists its five copies in order, so its first 10 nodes are one of each.
        ("fleet as it is", "huawei-r5-day30.fleet", Some(10)),
    ] {
        let load = |file: String| {
            let mut day = trace(&file);
            if let Some(hosts) = hosts {
                day.nodes.truncate(hosts);
            }
            day
        };
        let recorded = load(format!("{name}.trace"));
        let kept_alive = load(format!("{name}.keepalive.trace"));
        for (set, class) in [
            ("mix", None),
            ("oci", Some(Class::Oci)),
            ("firecracker", Some(Class::Firecracker)),
            ("shards", Some(Class::Shards)),
        ] {
            let set = format!("{model} {set}");
            let as_set = |day: &Cluster| match class {
                None => day.clone(),
                Some(class) => alone(day, class).unwrap(),
            };
            let (recorded, kept_alive) = (as_set(&recorded), as_set(&kept_alive));
            measure(&set, "no completions", &recorded, Clock::Held);
            for (lives, day) in [
                ("lifetimes as recorded", &recorded),
                ("with the keep-alive", &kept_alive),
            ] {
                let Some(speed) = slowest(&set, lives, day) else {
                    println!("{set}, {lives}: no speed on the ladder reaches 90% on every seed");
                    continue;
                };
                measure(
                    &set,
                    &format!("completions at {speed}x, {lives}"),
                    day,
                    Clock::Compressed(speed),
                );
            }
        }
    }
}

fn sample(cluster: &Cluster) -> Sample {
    Sample {
        shapes: Shapes::of(minimal_shapes(cluster)),
        ..Sample::default()
    }
}

/// Walks [`LADDER`], printing each speed's row, and returns the slowest speed at which Hex and both
/// best fits reach 90% on every seed.
fn slowest(set: &str, lives: &str, cluster: &Cluster) -> Option<Time> {
    println!(
        "ladder, {set}, {lives}: speed | seeds reaching 90% (Hex, best_fit, most_allocated) | fewest completions | seeds where Hex meets the bar | seeds where a best fit strands nothing, so none can be cut"
    );
    let sample = sample(cluster);
    let mut found = None;
    for speed in LADDER {
        let mut reached = [0usize; 3];
        let mut done = [u64::MAX; 3];
        let mut meets = 0;
        let mut nothing = 0;
        for seed in 0..SEEDS {
            let from = seed * cluster.workloads.len() / SEEDS;
            let runs = [Scorer::HEX, Scorer::BestFit, Scorer::MostAllocated].map(|scorer| {
                pack(
                    cluster,
                    &Sample { scorer, ..sample },
                    900,
                    from,
                    Clock::Compressed(speed),
                )
                .unwrap()
            });
            for (i, p) in runs.iter().enumerate() {
                reached[i] += usize::from(p.reached);
                done[i] = done[i].min(p.completions);
            }
            meets += usize::from(runs[1..].iter().all(|b| cut(&runs[0], b) >= 30.0));
            nothing += usize::from(runs[1..].iter().any(|b| b.stranded_per_mille() == 0));
        }
        println!("{speed:>5}x | {reached:?} | {done:?} | {meets}/{SEEDS} | {nothing}");
        if found.is_none() && reached == [SEEDS; 3] {
            found = Some(speed);
        }
    }
    found
}

fn measure(set: &str, name: &str, cluster: &Cluster, clock: Clock) {
    let sample = sample(cluster);
    let shapes = minimal_shapes(cluster);
    println!(
        "== {set}, {name}: {} nodes, {} launches, minimal shapes {shapes:?}",
        cluster.nodes.len(),
        cluster.workloads.len()
    );
    println!(
        "seed from arm | stranded per-mille (* = 90% load not reached) | passed over | completions | first refusal for room at load per-mille | passed over below half load [millicores/MiB@load; f = by a filter]"
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
                "{seed:>4} {from:>5} {arm} | {}{} | {} | {} | {} | {}",
                p.stranded_per_mille(),
                if p.reached { "" } else { "*" },
                p.passed_over,
                p.completions,
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
            "{set}, {name}: {arm} | {meets}/{SEEDS} | {least_best:.1}%, {least_most:.1}% | {passed} | {} | {early}",
            first.map_or_else(|| "-".to_string(), |l| l.to_string())
        );
    }
}
