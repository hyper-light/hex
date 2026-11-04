//! hex-sim's done condition (implementation goal 0, `docs/design/hex-sim.md`): determinism at 10,000
//! nodes, a crash at every control-plane write, and the model-based equivalence of `T-T7.4.1`.

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

use hex_sim::SimError;
use hex_sim::generate::{self, Shape};
use hex_sim::model::{Cluster, NodeId, NodeSpec, Resources, WorkloadId, WorkloadSpec};
use hex_sim::rng::Rng;
use hex_sim::sim::{self, Config, Crash, Report, Run, When};
use hex_sim::spec::{self, Violation};
use hex_sim::store::{State, Store, Write};

fn small(seed: u64) -> Shape {
    let mut r = Rng::new(seed ^ 0x5eed);
    Shape {
        nodes: u32::try_from(r.range(1, 24)).unwrap(),
        workloads: r.range(1, 160),
        mean_gap: r.range(0, 40_000),
        mean_run: if r.chance(150) {
            None
        } else {
            Some(r.range(1, 2_000_000))
        },
        gpu_nodes_per_mille: r.range(0, 500),
        max_share_per_mille: r.range(50, 1000),
    }
}

fn config(seed: u64) -> Config {
    let mut r = Rng::new(seed ^ 0xc0f1);
    Config {
        workers: u32::try_from(r.range(1, 8)).unwrap(),
        decide_ns: r.range(0, 200_000),
        commit_ns: r.range(0, 100_000),
        change_window: usize::try_from(r.range(1, 64)).unwrap(),
        ..Config::default()
    }
}

/// T-T7.4.1: do 1,000 generated clusters and workloads, each run by 1 to 8 parallel deciders with
/// late-binding commit; expect every run's committed log to be one the serial specification makes,
/// nothing lost or started twice, and (non-vacuity) late binding to have redone decisions in some runs.
#[test]
fn placements_equal_the_serial_specification_on_generated_clusters() {
    let (mut conflicts, mut stale, mut placements, mut waited) = (0u64, 0u64, 0u64, 0u64);
    for seed in 0..1_000 {
        let cluster = generate::cluster(seed, &small(seed)).unwrap();
        let run = sim::run(&cluster, &config(seed)).unwrap();
        let log =
            spec::check_log(&cluster.nodes, &run).unwrap_or_else(|v| panic!("seed {seed}: {v}"));
        let end = spec::check_end(&cluster, &run);
        assert_eq!(end.lost_or_double(), 0, "seed {seed}: {end:?}");
        conflicts += run.report.conflicts;
        stale += run.report.stale;
        placements += log.placements;
        waited += log.waiting;
    }
    assert!(
        conflicts > 0,
        "no decision was ever made stale: the late binding went untested"
    );
    assert!(stale > 0, "no decision ever outlived the change window");
    assert!(placements > 50_000, "{placements} placements");
    assert!(waited > 0, "no run left work waiting for room");
}

/// T-T7.4.1, directly: do a cluster whose workloads never end; expect each workload on exactly the
/// node the specification run serially puts it on.
#[test]
fn with_capacity_only_shrinking_placements_equal_a_serial_run() {
    for seed in 0..200 {
        let shape = Shape {
            mean_run: None,
            ..small(seed)
        };
        let cluster = generate::cluster(seed, &shape).unwrap();
        let run = sim::run(&cluster, &config(seed)).unwrap();
        let serial = spec::serial_place(&cluster.nodes, &cluster.workloads).unwrap();
        for (w, want) in cluster.workloads.iter().zip(serial) {
            let got = match run.store.record(w.id).unwrap().state {
                State::Placed(n) => Some(n),
                State::Pending => None,
                State::Complete(_) => panic!("seed {seed}: a workload that never ends completed"),
            };
            assert_eq!(got, want, "seed {seed}, {:?}", w.id);
        }
    }
}

fn crash_cluster() -> Cluster {
    let shape = Shape {
        nodes: 6,
        workloads: 40,
        mean_gap: 20_000,
        mean_run: Some(400_000),
        gpu_nodes_per_mille: 300,
        max_share_per_mille: 700,
    };
    generate::cluster(11, &shape).unwrap()
}

/// AC-T7.3.3's nemesis, at every write: do the same run once per control-plane store write, crashing
/// the control plane before that write and again after it; expect each run's log to satisfy the
/// specification and nothing lost or started twice, and (non-vacuity) exactly one crash per run and
/// crashes injected == 2 x writes seen.
#[test]
fn a_crash_at_every_control_plane_write_loses_and_doubles_nothing() {
    let cluster = crash_cluster();
    let base = Config {
        workers: 3,
        ..Config::default()
    };
    let clean = sim::run(&cluster, &base).unwrap();
    assert_eq!(clean.report.crashes, 0);
    let writes = clean.report.writes;
    assert!(
        writes >= 3 * u64::try_from(cluster.workloads.len()).unwrap(),
        "{writes} writes"
    );
    let (mut injected, mut changed_history) = (0u64, 0u64);
    for write in 0..writes {
        for when in [When::Before, When::After] {
            let run = sim::run(
                &cluster,
                &Config {
                    crash: Some(Crash { write, when }),
                    ..base
                },
            )
            .unwrap();
            assert_eq!(run.report.crashes, 1, "write {write} {when:?}");
            injected += run.report.crashes;
            spec::check_log(&cluster.nodes, &run)
                .unwrap_or_else(|v| panic!("write {write} {when:?}: {v}"));
            let end = spec::check_end(&cluster, &run);
            assert_eq!(end.lost_or_double(), 0, "write {write} {when:?}: {end:?}");
            assert_eq!(
                end.complete,
                u64::try_from(cluster.workloads.len()).unwrap(),
                "write {write} {when:?}: {end:?}"
            );
            if run.store.log() != clean.store.log() {
                changed_history += 1;
            }
        }
    }
    assert_eq!(injected, 2 * writes);
    assert!(
        changed_history > writes,
        "crashes changed only {changed_history} of {} histories",
        2 * writes
    );
}

fn same(a: &Report, b: &Report) -> bool {
    (
        a.trace_hash,
        a.writes,
        a.placements,
        a.conflicts,
        a.end,
        &a.latencies,
    ) == (
        b.trace_hash,
        b.writes,
        b.placements,
        b.conflicts,
        b.end,
        &b.latencies,
    )
}

/// Implementation goal 0 (a): do 10,000 fake nodes and 100,000 workloads twice from one seed; expect
/// the same trace hash and report, a different seed a different one, and the run correct by the
/// specification.
#[test]
fn ten_thousand_nodes_run_deterministically() {
    let shape = Shape {
        nodes: 10_000,
        workloads: 100_000,
        mean_gap: 20_000,
        mean_run: Some(100_000_000),
        gpu_nodes_per_mille: 100,
        max_share_per_mille: 250,
    };
    let config = Config {
        workers: 16,
        ..Config::default()
    };
    let cluster = generate::cluster(42, &shape).unwrap();
    let a = sim::run(&cluster, &config).unwrap();
    let b = sim::run(&cluster, &config).unwrap();
    assert!(same(&a.report, &b.report));
    assert_eq!(a.store.log(), b.store.log());
    let other = sim::run(&generate::cluster(43, &shape).unwrap(), &config).unwrap();
    assert_ne!(a.report.trace_hash, other.report.trace_hash);
    let end = spec::check_end(&cluster, &a);
    assert_eq!(end.lost_or_double(), 0, "{end:?}");
    assert_eq!(end.complete, 100_000);
    spec::check_log(&cluster.nodes, &a).unwrap();
    let q = a.report.latencies.quantiles().unwrap();
    assert_eq!(q.count, 100_000);
    assert!(q.p50 <= q.p99 && q.p99 <= q.p999 && q.p999 <= q.max);
}

// The oracle's own non-vacuity: hand it histories the specification can't make and expect each refused.

fn res(cpu: u64) -> Resources {
    Resources {
        cpu_milli: cpu,
        mem_bytes: 1,
        gpus: 0,
    }
}

fn two_nodes() -> Vec<NodeSpec> {
    vec![
        NodeSpec {
            id: NodeId(0),
            capacity: res(1000),
        },
        NodeSpec {
            id: NodeId(1),
            capacity: res(4000),
        },
    ]
}

fn work(id: u64, cpu: u64) -> WorkloadSpec {
    WorkloadSpec {
        id: WorkloadId(id),
        need: res(cpu),
        arrives: 0,
        runs_for: None,
    }
}

fn run_of(store: Store, n: usize) -> Run {
    Run {
        report: Report::default(),
        store,
        starts: vec![1; n],
        acked: vec![true; n],
        refused: vec![false; n],
    }
}

#[test]
fn the_oracle_refuses_a_placement_that_is_not_the_best_fit() {
    let mut s = Store::new(&two_nodes(), 4, 16).unwrap();
    s.apply(0, Write::Submit(work(0, 500))).unwrap();
    s.apply(
        1,
        Write::Place {
            workload: WorkloadId(0),
            node: NodeId(1),
            version: 1,
        },
    )
    .unwrap();
    let v = spec::check_log(&two_nodes(), &run_of(s, 1)).unwrap_err();
    assert_eq!(
        v,
        Violation::NotSerialChoice {
            workload: WorkloadId(0),
            chose: NodeId(1),
            spec: Some(NodeId(0))
        }
    );
}

#[test]
fn the_oracle_refuses_a_placement_that_jumps_the_queue() {
    let mut s = Store::new(&two_nodes(), 4, 16).unwrap();
    s.apply(0, Write::Submit(work(0, 3000))).unwrap();
    s.apply(0, Write::Submit(work(1, 500))).unwrap();
    s.apply(
        1,
        Write::Place {
            workload: WorkloadId(1),
            node: NodeId(0),
            version: 1,
        },
    )
    .unwrap();
    let v = spec::check_log(&two_nodes(), &run_of(s, 2)).unwrap_err();
    assert_eq!(
        v,
        Violation::JumpedQueue {
            placed: WorkloadId(1),
            ahead: WorkloadId(0)
        }
    );
}

#[test]
fn the_oracle_refuses_work_left_pending_with_room() {
    let mut s = Store::new(&two_nodes(), 4, 16).unwrap();
    s.apply(0, Write::Submit(work(0, 500))).unwrap();
    let v = spec::check_log(&two_nodes(), &run_of(s, 1)).unwrap_err();
    assert_eq!(v, Violation::LeftPending(WorkloadId(0)));
}

/// AC-T7.3.3's counter isn't vacuous: a workload started twice and one acknowledged but never
/// stored are each counted.
#[test]
fn the_end_check_counts_doubles_and_losses() {
    let nodes = two_nodes();
    let cluster = Cluster {
        nodes: nodes.clone(),
        workloads: vec![work(0, 500), work(1, 500)],
    };
    let mut s = Store::new(&nodes, 4, 16).unwrap();
    s.apply(0, Write::Submit(work(0, 500))).unwrap();
    s.apply(
        1,
        Write::Place {
            workload: WorkloadId(0),
            node: NodeId(0),
            version: 1,
        },
    )
    .unwrap();
    let mut run = run_of(s, 2);
    run.starts = vec![2, 0];
    let end = spec::check_end(&cluster, &run);
    assert_eq!((end.started_twice, end.lost), (1, 1));
}

/// CLAUDE.md §2: every bound refuses, typed.
#[test]
fn bounds_refuse_typed() {
    let cluster = crash_cluster();
    assert_eq!(
        sim::run(
            &cluster,
            &Config {
                workers: 0,
                ..Config::default()
            }
        )
        .unwrap_err(),
        SimError::Config("the control plane has at least one decider")
    );
    assert_eq!(
        sim::run(
            &cluster,
            &Config {
                max_steps: 10,
                ..Config::default()
            }
        )
        .unwrap_err(),
        SimError::StepBudget(10)
    );
    assert!(matches!(
        sim::run(
            &cluster,
            &Config {
                max_events: 1,
                ..Config::default()
            }
        )
        .unwrap_err(),
        SimError::Full {
            what: "the event queue",
            ..
        }
    ));
    let restart_forever = Config {
        crash: Some(Crash {
            write: 0,
            when: When::Before,
        }),
        restart_ns: u64::MAX / 2,
        client_attempts: 2,
        ..Config::default()
    };
    assert!(matches!(
        sim::run(&cluster, &restart_forever).unwrap_err(),
        SimError::ClientGaveUp(WorkloadId(0), 2)
    ));
}
