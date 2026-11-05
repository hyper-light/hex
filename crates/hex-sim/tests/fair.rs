//! G4's fair-share bar (CP-T7.5, `docs/design/scheduler-fairness.md`): four repositories' shares of a
//! contended day of CI, measured in hex-sim.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::disallowed_macros
    )
)]

use hex_sim::fair::{self, Policy};
use hex_sim::generate::{self, Day, Repo};
use hex_sim::model::Resources;
use hex_sim::sim::{self, Config};

const GIB: u64 = 1 << 30;
const HOUR: u64 = 3_600_000_000_000;

/// Four equal-weight repositories on 64 nodes of 16 cores and 64 GiB. In the ten busy hours they ask
/// for 60%, 50%, 45% and 45% of the cluster (200% in all), with legs of different shapes (one
/// memory-heavy); out of them, 8% each, so the backlog drains by night. Each asks well past the 28%
/// DRF gives it here (three CPU-bound owners and one memory-bound one share the CPU), so each stays
/// backlogged while all four wait: on a day where two asked for 30%, their arrivals often fell below
/// that 28%, and an owner short of work holds less than its share under any policy
/// (docs/design/scheduler-fairness.md F1, evidence).
fn four_repos() -> Day {
    let repo = |cores, gib, busy_ppm| Repo {
        weight: 1,
        cores,
        gib,
        busy_ppm,
        quiet_ppm: 80_000,
    };
    Day {
        nodes: 64,
        node: Resources {
            cpu_milli: 16_000,
            mem_bytes: 64 * GIB,
            gpus: 0,
        },
        repos: vec![
            repo((1, 1), (1, 2), 600_000),
            repo((4, 4), (4, 8), 500_000),
            repo((2, 2), (12, 16), 450_000),
            repo((1, 8), (2, 16), 450_000),
        ],
        length: 24 * HOUR,
        busy: (9 * HOUR, 19 * HOUR),
        mean_run: 600_000_000_000,
        max_legs: 400_000,
    }
}

/// AC-T7.5.2: do a generated day of four equal-weight repositories, each asking for more than its
/// quarter for ten hours; expect each repository's part of the capacity held while all four wait to
/// be within 2% of 25% (0.5 percentage points) under bands-then-DRF, over at least eight contended
/// hours; and (non-vacuity) submit order alone to miss that bar on the same day.
#[test]
fn four_equal_weight_repositories_each_hold_a_quarter_under_contention() {
    let day = generate::day(4, &four_repos()).unwrap();
    let base = Config {
        workers: 4,
        max_events: 1 << 22,
        ..Config::default()
    };
    let mut worst = Vec::new();
    for policy in [Policy::Drf, Policy::Fifo] {
        let run = sim::run(&day, &Config { policy, ..base }).unwrap();
        let shares = fair::measure(&day, &run.ran, run.report.end).unwrap();
        eprintln!(
            "AC-T7.5.2 {policy:?}: legs {} placements {} contended {:.2} h shares_ppm {:?} of_cluster_ppm {:?} worst {} ppm",
            day.workloads.len(),
            run.report.placements,
            shares.contended_ns as f64 / HOUR as f64,
            shares.shares_ppm,
            shares.of_cluster_ppm,
            shares.worst_from_equal_ppm()
        );
        assert!(shares.contended_ns >= 8 * HOUR, "{policy:?}: {shares:?}");
        worst.push(shares.worst_from_equal_ppm());
    }
    // 2% of 25% is 0.5 percentage points: 5,000 parts per million of the cluster's held share.
    assert!(
        worst[0] <= 5_000,
        "DRF: worst {} ppm from a quarter",
        worst[0]
    );
    assert!(
        worst[1] > 5_000,
        "FIFO met the bar too: the test can't fail"
    );
}
