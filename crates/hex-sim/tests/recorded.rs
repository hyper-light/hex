//! AC-T7.5.2 over a recorded day (CP-T7.5, `docs/design/scheduler-fairness.md` F1): kiln's own CI,
//! replayed in hex-sim as recorded (`bench/sched/kiln_workload.md`).

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

use hex_sim::fair::{self, Policy, ShareReport};
use hex_sim::model::{Cluster, NodeId, NodeSpec, Resources, Time};
use hex_sim::replay;
use hex_sim::sim::{self, Config};

const DAY: u128 = 86_400_000_000_000;
const GIB: f64 = (1u64 << 30) as f64;

fn recorded(name: &str) -> Cluster {
    let text = std::fs::read_to_string(format!("../../bench/sched/{name}")).unwrap();
    replay::parse(&text, &replay::Limits::default()).unwrap()
}

/// Each owner's CPU (millicore-ns) and memory (byte-ns) asked for over the day.
fn asked(day: &Cluster) -> Vec<(u128, u128)> {
    let mut out = vec![(0u128, 0u128); day.owners.len()];
    for w in &day.workloads {
        let runs = u128::from(w.runs_for.unwrap());
        let o = &mut out[usize::try_from(w.tenancy.owner.0).unwrap()];
        o.0 += u128::from(w.need.cpu_milli) * runs;
        o.1 += u128::from(w.need.mem_bytes) * runs;
    }
    out
}

/// The largest cluster of one node at which every owner asks, on average over the day, for more
/// than a quarter of it in CPU or in memory: each owner `o` allows CPU up to `cpu_o` (just under four
/// times its mean millicores) or memory up to `mem_o`. Choosing which owners CPU satisfies (those
/// whose `cpu_o` is at least the CPU chosen) leaves the rest to memory, so the candidates are each
/// owner's `cpu_o` as the CPU, with the least `mem_o` of the owners it leaves as the memory (the
/// recorded cluster's memory if it leaves none). The largest is the one whose smaller fraction of
/// the recorded cluster (CPU or memory) is largest.
fn quarter_cluster(day: &Cluster) -> Resources {
    let total = day.nodes.iter().fold(Resources::default(), |a, n| {
        a.checked_add(&n.capacity).unwrap()
    });
    let limits: Vec<(u128, u128)> = asked(day)
        .iter()
        .map(|&(c, m)| ((4 * c - 1) / DAY, (4 * m - 1) / DAY))
        .collect();
    let mut best: Option<(u128, u128)> = None;
    for &(cpu, _) in &limits {
        let mem = limits
            .iter()
            .filter(|(c, _)| *c < cpu)
            .map(|&(_, m)| m)
            .min()
            .unwrap_or(u128::from(total.mem_bytes));
        let frac = |(c, m): (u128, u128)| {
            // The smaller of c / total cpu and m / total mem, compared by cross-multiplication.
            (c * u128::from(total.mem_bytes)).min(m * u128::from(total.cpu_milli))
        };
        if best.is_none_or(|b| frac((cpu, mem)) > frac(b)) {
            best = Some((cpu, mem));
        }
    }
    let (cpu, mem) = best.unwrap();
    Resources {
        cpu_milli: u64::try_from(cpu).unwrap(),
        mem_bytes: u64::try_from(mem).unwrap(),
        gpus: 0,
    }
}

fn hours(ns: Time) -> f64 {
    ns as f64 / 3.6e12
}

fn describe(label: &str, s: &ShareReport) {
    let mut iv = s.intervals.clone();
    iv.sort_unstable();
    eprintln!(
        "{label}: contended {:.3} h in {} intervals (median {:.1} s, longest {:.1} min); parts of the held share ppm {:?}; dominant share of the cluster ppm {:?}; worst from equal {} ppm",
        hours(s.contended_ns),
        iv.len(),
        iv.get(iv.len() / 2).map_or(0.0, |&t| t as f64 / 1e9),
        iv.last().map_or(0.0, |&t| t as f64 / 6e10),
        s.shares_ppm,
        s.of_cluster_ppm,
        s.worst_from_equal_ppm()
    );
}

fn run(day: &Cluster, policy: Policy) -> (ShareReport, u64) {
    let run = sim::run(
        day,
        &Config {
            policy,
            max_events: 1 << 22,
            ..Config::default()
        },
    )
    .unwrap();
    let refused = u64::try_from(run.refused.iter().filter(|&&r| r).count()).unwrap();
    (
        fair::measure(day, &run.ran, run.report.end).unwrap(),
        refused,
    )
}

/// AC-T7.5.2 over a recorded day, measured for the record (`bench/sched/kiln_workload.md`): do
/// 2026-10-25 of kiln's CI (kiln, slates, hyperscale and hyper-raft as four equal-weight owners;
/// arrivals, run times and needs as recorded) on the largest cluster at which each of the four asks
/// for more than a quarter of it, under bands-then-DRF and under submit order; expect the
/// precondition to hold, the four to contend, and legs too large for the cluster refused at
/// admission. It does not meet the bar (2026-11-05: DRF worst 220,640 ppm from 25%, FIFO 448,999):
/// kiln asks for more than a quarter only through legs the cluster can't hold, and what it can run
/// asks for under 1% of it.
#[test]
fn four_recorded_repositories_on_the_largest_quarter_cluster() {
    let mut day = recorded("kiln-ci-20261025-four-repos.trace");
    let node = quarter_cluster(&day);
    for (o, &(c, m)) in asked(&day).iter().enumerate() {
        let cpu_day = u128::from(node.cpu_milli) * DAY;
        let mem_day = u128::from(node.mem_bytes) * DAY;
        assert!(
            4 * c > cpu_day || 4 * m > mem_day,
            "owner {o} asks for no more than a quarter"
        );
    }
    day.nodes = vec![NodeSpec {
        id: NodeId(0),
        capacity: node,
        background_milli: 0,
    }];
    eprintln!(
        "AC-T7.5.2 recorded 2026-10-25: {} legs, one node of {} millicores and {:.2} GiB",
        day.workloads.len(),
        node.cpu_milli,
        node.mem_bytes as f64 / GIB
    );
    let (drf, refused) = run(&day, Policy::Drf);
    let (fifo, _) = run(&day, Policy::Fifo);
    eprintln!("AC-T7.5.2 recorded: {refused} legs larger than the cluster, refused at admission");
    describe("AC-T7.5.2 recorded DRF", &drf);
    describe("AC-T7.5.2 recorded FIFO", &fifo);
    assert!(
        drf.contended_ns > 0 && fifo.contended_ns > 0,
        "no contention"
    );
    assert!(refused > 0);
}

/// Beside it, for the record: do 2026-10-30, the day of the fourteen with the most kiln-slates
/// contention, with kiln and slates as two equal-weight owners on the 10 runners kiln ran them on,
/// under bands-then-DRF and under submit order, against 50% within 2%; expect them to contend. They
/// barely do (2026-11-05: 560 us in all under DRF, 420 us under FIFO, the control plane's own commit
/// latency): at the real cluster the two never wait together for long.
#[test]
fn kiln_and_slates_at_the_recorded_runners() {
    let day = recorded("kiln-ci-20261030-kiln-slates.trace");
    let (drf, _) = run(&day, Policy::Drf);
    let (fifo, _) = run(&day, Policy::Fifo);
    describe("kiln-slates 2026-10-30 DRF", &drf);
    describe("kiln-slates 2026-10-30 FIFO", &fifo);
    assert!(
        drf.contended_ns > 0 && fifo.contended_ns > 0,
        "no contention"
    );
}
