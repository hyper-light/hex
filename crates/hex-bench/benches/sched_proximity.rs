//! S4/A2's measure (G3): goodput and the completion-time tail of proximity-scored placement against
//! proximity-blind placement on hex-sim's modelled WAN (`hex_sim::proximity`, `hex-sim.md` H13).
//!
//!   cargo bench -p hex-bench --bench sched_proximity
//!
//! Every study is 1,000 nodes over Azure's eight regions and 20,000 workloads, run under each policy:
//! blind (the fast path's best fit), proximity (hyper-swim's coordinates as an upper bound), the
//! tie-break (proximity only among best fit's ties) and the oracle (the true RTT). It sweeps seeds 1-5,
//! homes uniform and skewed, 50% and 90% offered load, and 10 ms, 100 ms and 1 s of compute a round
//! trip. Virtual time: the numbers depend on the seed and the build, not the host's load.
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

use hex_sim::proximity::{Homes, Policy, PolicyReport, Study, StudyConfig};
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

fn main() {
    // Gains of proximity over blind, per configuration: (goodput %, p99 %), over seeds.
    let mut summary = Vec::new();
    for step_ms in [10u64, 100, 1_000] {
        for load in [500u64, 900] {
            for homes in [Homes::Uniform, Homes::Skewed] {
                let mut gains = Vec::new();
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
                    let study = Study::generate(&config).unwrap();
                    let accuracy = Accuracy::measure(
                        &study.wan,
                        &study.coordinates,
                        20_000,
                        &mut hex_sim::rng::Rng::new(seed ^ 0xacc),
                    )
                    .unwrap();
                    println!(
                        "== step {step_ms} ms, load {load}‰, homes {homes:?}, seed {seed}: horizon {:.1} s, coordinates median error {:.1}%, region ranked first {}/{}",
                        study.horizon as f64 / 1e9,
                        accuracy.median_relative_error * 100.0,
                        accuracy.region_ranked_first,
                        accuracy.region_trials
                    );
                    let reports: Vec<PolicyReport> =
                        POLICIES.iter().map(|&p| study.run(p).unwrap()).collect();
                    for r in &reports {
                        println!("   {}", line(r));
                    }
                    let (blind, near) = (&reports[0], &reports[1]);
                    let goodput = 100.0 * (near.goodput_per_s() / blind.goodput_per_s() - 1.0);
                    let p99 = 100.0
                        * (1.0
                            - near.completion.unwrap().p99 as f64
                                / blind.completion.unwrap().p99 as f64);
                    gains.push((goodput, p99));
                }
                summary.push((step_ms, load, homes, gains));
            }
        }
    }
    println!("\nSUMMARY: proximity against blind, per seed (goodput gain %, p99 completion cut %)");
    for (step_ms, load, homes, gains) in summary {
        let cells: Vec<String> = gains
            .iter()
            .map(|(g, p)| format!("({g:+.1}, {p:+.1})"))
            .collect();
        println!(
            "step {step_ms:>4} ms load {load}‰ {homes:?}: {}",
            cells.join(" ")
        );
    }
}
