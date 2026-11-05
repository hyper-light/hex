//! The sharded control plane (`docs/design/hex-sim.md` H10, H11): a lost shard's slices taken over
//! by a live shard within `T-T7.11.1`'s 5 s, its running work undisturbed.

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

use std::time::Instant;

use hex_sim::generate::{self, Shape};
use hex_sim::model::{Cluster, Kind};
use hex_sim::sim::{self, Config, ControlPlane, Layout, Nemesis, Run};
use hex_sim::spec;

const SECOND: u64 = 1_000_000_000;

/// 1,000,000 workloads on 10,000 nodes, arriving over about 20 virtual seconds and running about 4 s
/// each: some 200,000 run at once, 20 a node, at a tenth of the cluster's CPU, so the queue stays
/// short and the serial specification's replay stays affordable. No GPU nodes and no benchmarks: at
/// 20 workloads a node a node's few GPUs are taken and nearly every node runs a build, which leaves
/// GPU work and benchmarks queued (S3's filters, tested in `tests/sim.rs`), not what this test is
/// about.
fn million() -> Cluster {
    let shape = Shape {
        nodes: 10_000,
        workloads: 1_000_000,
        mean_gap: 20_000,
        mean_run: Some(4 * SECOND),
        gpu_nodes_per_mille: 0,
        max_share_per_mille: 10,
    };
    let mut cluster = generate::cluster(7, &shape).unwrap();
    for w in &mut cluster.workloads {
        if w.kind == Kind::Benchmark {
            w.kind = Kind::Plain;
        }
    }
    cluster
}

/// Eight shards; node agents resend a done report every 500 ms up to 16 times (8 s, past the 5 s
/// takeover bound), so a report made while its shard is lost waits for the shard that takes over.
fn eight_shards(lose: Nemesis) -> Config {
    Config {
        shards: 8,
        workers: 16,
        report_retry_ns: 500_000_000,
        report_attempts: 16,
        client_retry_ns: 500_000_000,
        client_attempts: 16,
        max_events: 1 << 24,
        nemesis: lose,
        ..Config::default()
    }
}

/// When shard 3 is lost: after every workload has been submitted, with most of the last ones running.
fn lose_after_submits(cluster: &Cluster) -> Nemesis {
    let last = cluster.workloads.last().unwrap().arrives;
    Nemesis::Lose {
        shard: 3,
        at: last + SECOND / 2 + 12_345,
    }
}

/// T-T7.11.1: do 1,000,000 workloads submitted on 10,000 nodes under 8 shards and lose shard 3 for
/// good; expect lost-or-double 0, the lost shard's running workloads undisturbed (kept-running 0),
/// every slice's log one the serial specification makes, and its slice rebuilt by a live shard and
/// committing at most 5 s after the loss; and (non-vacuity) tens of thousands running on the lost
/// slice when it was lost and completions it recorded after the takeover.
#[test]
fn a_lost_shard_is_taken_over_within_five_seconds() {
    let cluster = million();
    let config = eight_shards(lose_after_submits(&cluster));
    let run = sim::run(&cluster, &config).unwrap();
    check_takeover(&cluster, &config, &run);
}

fn check_takeover(cluster: &Cluster, config: &Config, run: &Run) {
    let end = spec::check_end(cluster, run);
    assert_eq!(end.lost_or_double(), 0, "{end:?}");
    assert_eq!(end.complete, 1_000_000, "{end:?}");
    let kept = spec::check_kept_running(cluster, run);
    assert_eq!(kept.disturbed, 0, "{kept:?}");
    let slices = spec::check_slices(cluster, run).unwrap();
    assert_eq!(slices.len(), 8);
    // A run that kept no planes is checked against the layout it was made on, to the same verdicts.
    let layout = Layout::new(&cluster.nodes, config.shards).unwrap();
    let bare = Run {
        planes: Vec::new(),
        ..run.clone()
    };
    assert_eq!(spec::check_layout(&layout, &bare).unwrap(), slices);
    assert!(slices.iter().all(|s| s.placements > 100_000), "{slices:?}");
    assert!(run.planes.iter().all(|p| p.pending() == 0));

    let [t] = run.report.takeovers.as_slice() else {
        panic!("takeovers: {:?}", run.report.takeovers);
    };
    assert_eq!((t.slice, t.lost), (3, 3), "{t:?}");
    assert_ne!(t.by, 3, "{t:?}");
    assert!(
        t.detection_ns <= config.lease_ns && t.detection_ns > config.lease_ns - config.renew_ns,
        "{t:?}"
    );
    assert_eq!(
        t.rebuild_ns,
        t.records * config.rebuild_ns_per_record,
        "{t:?}"
    );
    let takeover = t
        .takeover_ns
        .expect("the slice never committed after its takeover");
    assert!(takeover >= t.detection_ns + t.rebuild_ns, "{t:?}");
    assert!(takeover <= 5 * SECOND, "T-T7.11.1: {t:?}");
    // Non-vacuity: the lost slice held running work, and recorded it done after the takeover.
    assert!(t.running > 10_000, "{t:?}");
    assert!(t.records > 10_000, "{t:?}");
    assert!(t.committed_after > 10_000, "{t:?}");
    println!(
        "T-T7.11.1: detection {} ms, rebuild of {} records {} ms, takeover {} ms; {} running on the \
         lost slice, {} commits after",
        t.detection_ns / 1_000_000,
        t.records,
        t.rebuild_ns / 1_000_000,
        takeover / 1_000_000,
        t.running,
        t.committed_after,
    );
}

/// The measurement behind `sim::REBUILD_NS_PER_RECORD` (CLAUDE.md §5): do 1,000,000 workloads that
/// never end on 10,000 nodes under 8 shards, so each slice holds an eighth of them placed, then rebuild
/// slice 3's plane from that store five times; print the best wall time per record read. Run in release
/// on fedora: `cargo test -p hex-sim --release --test shards -- --ignored --nocapture rebuild_cost`.
#[test]
#[ignore = "a wall-clock measurement, run by hand in release"]
fn rebuild_cost() {
    let mut cluster = million();
    for w in &mut cluster.workloads {
        w.runs_for = None;
    }
    let config = eight_shards(Nemesis::None);
    let run = sim::run(&cluster, &config).unwrap();
    let layout = Layout::new(&cluster.nodes, config.shards).unwrap();
    let mut best = u128::MAX;
    let mut records = 0;
    for _ in 0..5 {
        let start = Instant::now();
        let (plane, n) = ControlPlane::rebuild(&run.store, &layout, 3, config.workers).unwrap();
        best = best.min(start.elapsed().as_nanos());
        records = n;
        drop(plane);
    }
    assert!(records > 100_000, "{records} records");
    println!(
        "rebuild of slice 3: {records} of {} records, best of 5 {best} ns, {} ns a record",
        run.store.records().count(),
        best / u128::from(records)
    );
}
