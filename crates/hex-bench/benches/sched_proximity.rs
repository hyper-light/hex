//! S4/A2's measure (G3): goodput and the completion-time tail of proximity-scored placement against
//! proximity-blind placement on hex-sim's modelled WAN (`hex_sim::proximity`, `hex-sim.md` H21).
//!
//!   cargo bench -p hex-bench --bench sched_proximity
//!
//! Every study is 1,000 nodes over Azure's eight regions and 20,000 workloads of Hex's three kinds,
//! OCI containers, Firecracker microVMs and Shards microVMs (`hex-sim.md` H10), both drawn from G2's
//! recorded day: each job one of its launches (owner, kind by G2's rule, and need with its kind's
//! overhead, Shards' a 5 MiB stand-in), the nodes its node rows repeated. Two fleets, H10's two node
//! models: `huawei-r5-day30.trace`'s m5d.metal hosts, every one running all three kinds, and
//! `huawei-r5-day30.fleet.trace`'s, Hex's own Linux, Windows and macOS hosts. Each study runs under
//! every policy: blind (the fast path's best fit), proximity (hyper-swim's coordinates as an upper
//! bound), the tie-break (proximity only among best fit's ties) and the oracle (the true RTT). It
//! sweeps seeds 1-5, both fleets, homes uniform and skewed, 50% and 90% offered load, and 10 ms, 100 ms
//! and 1 s of compute a round trip. Virtual time: the numbers depend on the seed and the build, not the
//! host's load.
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

use hex_sim::model::Class;
use hex_sim::proximity::{CLASSES, Homes, Policy, PolicyReport, Study, StudyConfig};
use hex_sim::replay::{Limits, parse};
use hex_sim::wan::{Accuracy, WanConfig};

const POLICIES: [Policy; 4] = [
    Policy::Blind,
    Policy::Proximity,
    Policy::TieBreak,
    Policy::Oracle,
];

fn ms(ns: u64) -> f64 {
    ns as f64 / 1e6
}

fn line(r: &PolicyReport) -> String {
    let q = r.completion.unwrap();
    format!(
        "{:<9} goodput {:>8.3}/s useful {:>8.1} cores | completion ms p50 {:>10.1} p99 {:>10.1} p99.9 {:>10.1} max {:>10.1} | rtt {:>6.1} ms moved {:>5} queue {:>5}",
        format!("{:?}", r.policy),
        r.goodput_per_s(),
        r.useful_cores(),
        ms(q.p50),
        ms(q.p99),
        ms(q.p999),
        ms(q.max),
        r.mean_rtt_ms(),
        r.moved,
        r.queue_peak
    )
}

fn class_lines(r: &PolicyReport) -> String {
    CLASSES
        .iter()
        .zip(&r.classes)
        .map(|(class, c)| {
            let q = c.completion.unwrap();
            format!(
                "{:<11} placed {:>5} done {:>5} goodput {:>8.3}/s p50 {:>10.1} p99 {:>10.1} ms",
                format!("{class:?}"),
                c.placements,
                c.done_by_horizon,
                r.class_goodput_per_s(*class),
                ms(q.p50),
                ms(q.p99)
            )
        })
        .collect::<Vec<_>>()
        .join("\n      ")
}

/// Proximity's gain over blind: (goodput %, p99 cut %), overall and for each kind.
fn gains(blind: &PolicyReport, near: &PolicyReport) -> [(f64, f64); 4] {
    let gain = |b: f64, n: f64| 100.0 * (n / b - 1.0);
    let cut = |b: u64, n: u64| 100.0 * (1.0 - n as f64 / b as f64);
    let mut out = [(
        gain(blind.goodput_per_s(), near.goodput_per_s()),
        cut(blind.completion.unwrap().p99, near.completion.unwrap().p99),
    ); 4];
    for (i, class) in CLASSES.iter().enumerate() {
        out[i + 1] = (
            gain(
                blind.class_goodput_per_s(*class),
                near.class_goodput_per_s(*class),
            ),
            cut(
                blind.classes[i].completion.unwrap().p99,
                near.classes[i].completion.unwrap().p99,
            ),
        );
    }
    out
}

fn main() {
    // G2's recorded day on each fleet: its launches (owners, kinds, needs with overheads; Shards' a
    // 5 MiB stand-in until Shards' is measured) and its node rows.
    let fleets = [
        (
            "AllKinds",
            include_str!("../../../bench/sched/huawei-r5-day30.trace"),
        ),
        (
            "Mixed",
            include_str!("../../../bench/sched/huawei-r5-day30.fleet.trace"),
        ),
    ]
    .map(|(name, text)| (name, parse(text, &Limits::default()).unwrap()));
    for (fleet, recorded) in &fleets {
        println!(
            "recorded {fleet}: {} node rows, {} owners, {} launches; overheads oci {} B, firecracker {} B, shards {} B",
            recorded.nodes.len(),
            recorded.owners.len(),
            recorded.workloads.len(),
            recorded.overheads.of(Class::Oci),
            recorded.overheads.of(Class::Firecracker),
            recorded.overheads.of(Class::Shards)
        );
    }
    // Gains of proximity over blind, per configuration: (goodput %, p99 %), over seeds.
    let mut summary = Vec::new();
    for (fleet, recorded) in &fleets {
        for step_ms in [10u64, 100, 1_000] {
            for load in [500u64, 900] {
                for homes in [Homes::Uniform, Homes::Skewed] {
                    let mut gains_of = Vec::new();
                    for seed in 1..=5u64 {
                        let config = StudyConfig {
                            wan: WanConfig {
                                nodes: 1_000,
                                seed,
                                ..WanConfig::default()
                            },
                            workloads: 20_000,
                            load_per_mille: load,
                            step_ns: step_ms * 1_000_000,
                            homes,
                            seed,
                            ..StudyConfig::default()
                        };
                        let study = Study::generate(&config, recorded).unwrap();
                        let accuracy = Accuracy::measure(
                            &study.wan,
                            &study.coordinates,
                            20_000,
                            &mut hex_sim::rng::Rng::new(seed ^ 0xacc),
                        )
                        .unwrap();
                        println!(
                            "== {fleet}, step {step_ms} ms, load {load}‰, homes {homes:?}, seed {seed}: horizon {:.1} s, coordinates median error {:.1}%, region ranked first {}/{}",
                            study.horizon as f64 / 1e9,
                            accuracy.median_relative_error * 100.0,
                            accuracy.region_ranked_first,
                            accuracy.region_trials
                        );
                        let reports: Vec<PolicyReport> =
                            POLICIES.iter().map(|&p| study.run(p).unwrap()).collect();
                        for r in &reports {
                            println!("   {}", line(r));
                            println!("      {}", class_lines(r));
                        }
                        gains_of.push(gains(&reports[0], &reports[1]));
                    }
                    summary.push((fleet, step_ms, load, homes, gains_of));
                }
            }
        }
    }
    println!(
        "\nSUMMARY: proximity against blind, per seed (goodput gain %, p99 completion cut %), all workloads then each kind"
    );
    let mut least = [(f64::MAX, f64::MAX); 4];
    for (fleet, step_ms, load, homes, gains_of) in summary {
        for (k, name) in ["all", "Oci", "Firecracker", "Shards"].iter().enumerate() {
            let cells: Vec<String> = gains_of
                .iter()
                .map(|g| {
                    let (gp, p) = g[k];
                    least[k] = (least[k].0.min(gp), least[k].1.min(p));
                    format!("({gp:+.1}, {p:+.1})")
                })
                .collect();
            println!(
                "{fleet} step {step_ms:>4} ms load {load}‰ {homes:?} {name:<11}: {}",
                cells.join(" ")
            );
        }
    }
    for (k, name) in ["all", "Oci", "Firecracker", "Shards"].iter().enumerate() {
        println!(
            "LEAST {name:<11}: goodput {:+.1}%, p99 cut {:+.1}%",
            least[k].0, least[k].1
        );
    }
}
