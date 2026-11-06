//! A side measurement, not of record: G4's bars run on days of kiln's own CI, replayed in hex-sim
//! as recorded (`bench/sched/kiln_workload.md`). Hex schedules microVMs, not CI jobs (Ada,
//! 2026-11-06: "You are NOT Kiln's load balancer for jobs, CI, and general work. Your scheduler is
//! for *microvms*."), so these days stand for no workload of Hex's; G4's bars are met on microVM
//! days (`tests/fair.rs`, `tests/burst.rs`). Kept for what they showed about the bars' preconditions.

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
use hex_sim::generate;
use hex_sim::model::{Cluster, NodeId, NodeSpec, OwnerId, Resources, Time};
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

/// Side measurement (kiln CI, not of record): AC-T7.5.2 over a recorded CI day, measured (`bench/sched/kiln_workload.md`): do
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
        "kiln-CI side: AC-T7.5.2 2026-10-25: {} legs, one node of {} millicores and {:.2} GiB",
        day.workloads.len(),
        node.cpu_milli,
        node.mem_bytes as f64 / GIB
    );
    let (drf, refused) = run(&day, Policy::Drf);
    let (fifo, _) = run(&day, Policy::Fifo);
    eprintln!(
        "kiln-CI side: AC-T7.5.2: {refused} legs larger than the cluster, refused at admission"
    );
    describe("kiln-CI side: AC-T7.5.2 DRF", &drf);
    describe("kiln-CI side: AC-T7.5.2 FIFO", &fifo);
    assert!(
        drf.contended_ns > 0 && fifo.contended_ns > 0,
        "no contention"
    );
    assert!(refused > 0);
}

/// Side measurement (kiln CI, not of record): do 2026-10-30, the day of the fourteen with the most kiln-slates
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

/// Each of the first `owners` owners' 95th-percentile leg-start latency (first start less arrival; a
/// leg never started counts as waiting for ever).
fn p95s(day: &Cluster, started: &[Option<Time>], owners: u32) -> Vec<Time> {
    (0..owners)
        .map(|o| {
            let mut waits: Vec<Time> = day
                .workloads
                .iter()
                .zip(started)
                .filter(|(w, _)| w.tenancy.owner == OwnerId(o))
                .map(|(w, s)| s.map_or(Time::MAX, |s| s - w.arrives))
                .collect();
            waits.sort_unstable();
            waits[(waits.len() * 95).div_ceil(100) - 1]
        })
        .collect()
}

fn starts(day: &Cluster, policy: Policy, preemptions: u64) -> (Vec<Option<Time>>, u64) {
    let run = sim::run(
        day,
        &Config {
            policy,
            preemptions,
            max_events: 1 << 22,
            ..Config::default()
        },
    )
    .unwrap();
    (
        run.ran.iter().map(|r| r.map(|r| r.started)).collect(),
        run.report.preempted,
    )
}

/// Side measurement (kiln CI, not of record): T-T7.5.1 on a recorded CI day: do 2026-10-30 of kiln's CI as recorded (focal, hyperscale, kiln and
/// slates as owners, on the 10 runners kiln ran it on), then the same day with 1,000 legs from a
/// repository that ran nothing that day, all arriving at the start of the day's busiest hour (the
/// hour, counted from its first leg, in which most legs arrived), each leg's need, run time and kind
/// drawn from the day's own legs; expect submit order (FIFO) to let the burst delay the others, and
/// Hex's policy (bands, weighted DRF, preemption of over-share work) and plain DRF to be measured
/// beside it for the record (`bench/sched/kiln_workload.md`). Measured 2026-11-05: Hex's policy keeps
/// focal's, hyperscale's and kiln's p95 as they were and slates' 20 us longer (380 to 400 us: one
/// commit, as a slates leg arrived while a burst leg's commit was in flight), so it misses "at most"
/// by that; plain DRF delays kiln by 167 s and slates by 87 s; FIFO by 3.5 h and 1.5 h.
#[test]
fn a_burst_on_a_recorded_day_leaves_the_others_start_p95_as_it_was() {
    const HOUR: Time = 3_600_000_000_000;
    let usual = recorded("kiln-ci-20261030.trace");
    let owners = u32::try_from(usual.owners.len()).unwrap();
    let mut per_hour = std::collections::BTreeMap::<Time, u64>::new();
    for w in &usual.workloads {
        *per_hour.entry(w.arrives / HOUR).or_default() += 1;
    }
    let (&busiest, &count) = per_hour
        .iter()
        .max_by_key(|(h, n)| (**n, std::cmp::Reverse(**h)))
        .unwrap();
    let burst =
        generate::burst_from(usual.clone(), 7, OwnerId(owners), busiest * HOUR, 1_000).unwrap();
    eprintln!(
        "kiln-CI side: T-T7.5.1 2026-10-30: {} legs of {owners} repositories, busiest hour {busiest} ({count} legs); with the burst {} legs",
        usual.workloads.len(),
        burst.workloads.len()
    );
    let mut rows = Vec::new();
    for (name, policy, preemptions) in [
        ("hex", Policy::Drf, 100_000),
        ("drf", Policy::Drf, 0),
        ("fifo", Policy::Fifo, 0),
    ] {
        let (without, _) = starts(&usual, policy, preemptions);
        let (with, preempted) = starts(&burst, policy, preemptions);
        let (a, b) = (p95s(&usual, &without, owners), p95s(&burst, &with, owners));
        let holds = b.iter().zip(&a).all(|(w, wo)| w <= wo);
        eprintln!(
            "kiln-CI side: T-T7.5.1 {name}: p95 ns without {a:?} with {b:?}; preempted {preempted}; holds {holds}"
        );
        rows.push(holds);
    }
    assert!(!rows[2], "FIFO kept the bar: the test can't fail");
}
