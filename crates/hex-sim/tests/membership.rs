//! S8's membership bars in the model (`hex_sim::membership`): a crash is found within SWIM's bounds,
//! and Lifeguard cuts the false positives slow members cause (arXiv:1707.00788v2).

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
use hex_sim::membership::{self, Config, Fault};
use hex_sim::rng::Rng;
use hex_sim::trace::Latencies;

const SECOND: u64 = 1_000_000_000;

fn crash_case(seed: u64) -> (u32, u32, u64) {
    let mut r = Rng::new(seed ^ 0xc7a5);
    let n = u32::try_from(r.range(8, 128)).unwrap();
    let member = u32::try_from(r.range(0, u64::from(n) - 1)).unwrap();
    // Warm-up: every member has probed for 5 s before the crash.
    let at_ns = r.range(5 * SECOND, 30 * SECOND);
    (n, member, at_ns)
}

/// S8 (crash detection, implementation goal 5): do 1,000 seeds of 8 to 128 members with one member
/// crashed after warm-up, with Lifeguard and as the SWIM baseline; expect the first suspicion within
/// 6 probe periods at the p99 and within SWIM's round-robin bound of 2·(n−1) periods + the timeout at
/// the max, the first death within the first suspicion + the suspicion's
/// max timeout + one period, and (non-vacuity) every crash detected and dead at every live member.
#[test]
fn a_crashed_member_is_suspected_and_declared_dead_within_its_bound() {
    for lifeguard in [true, false] {
        let (mut suspected, mut dead, mut all_dead) = (Vec::new(), Vec::new(), Vec::new());
        for seed in 0..1_000 {
            let (n, member, at_ns) = crash_case(seed);
            let fault = Fault::Crash { member, at_ns };
            let mut config = Config::lan(n, lifeguard);
            let (_, max) = config.suspicion_bounds().unwrap();
            let period = config.probe_interval_ns;
            let timeout = config.probe_timeout_ns;
            let round_robin = 2 * (u64::from(n) - 1) * period + timeout;
            config.end_ns = at_ns + round_robin + max + 2 * period;
            config.until_detected = true;
            let report = membership::run(&config, &[fault], seed).unwrap();
            let d = report.crashes[0];
            let first = d
                .first_suspected
                .unwrap_or_else(|| panic!("seed {seed}: never suspected: {d:?}"));
            let first_dead = d
                .first_dead
                .unwrap_or_else(|| panic!("seed {seed}: never declared dead: {d:?}"));
            let everyone = d
                .all_dead
                .unwrap_or_else(|| panic!("seed {seed}: not dead everywhere: {d:?}"));
            assert!(
                first - at_ns <= round_robin,
                "seed {seed}, n {n}: suspected {} ns after the crash",
                first - at_ns
            );
            assert!(
                first_dead - first <= max + period,
                "seed {seed}, n {n}: dead {} ns after the first suspicion",
                first_dead - first
            );
            assert_eq!(report.false_positives, 0, "seed {seed}: {report:?}");
            suspected.push(first - at_ns);
            dead.push(first_dead - at_ns);
            all_dead.push(everyone - at_ns);
        }
        let suspected = Latencies::new(suspected).quantiles().unwrap();
        let dead = Latencies::new(dead).quantiles().unwrap();
        let all_dead = Latencies::new(all_dead).quantiles().unwrap();
        let ms = |ns: u64| ns as f64 / 1e6;
        println!(
            "lifeguard {lifeguard}: suspected p50 {:.1} ms p99 {:.1} ms max {:.1} ms; first dead p50 {:.1} p99 {:.1} max {:.1} ms; dead at all p50 {:.1} p99 {:.1} max {:.1} ms (after the crash)",
            ms(suspected.p50),
            ms(suspected.p99),
            ms(suspected.max),
            ms(dead.p50),
            ms(dead.p99),
            ms(dead.max),
            ms(all_dead.p50),
            ms(all_dead.p99),
            ms(all_dead.max),
        );
        // The n − 1 live members probe the crashed one about once a period between them, so no probe
        // reaches it in k periods with probability about e^-k (SWIM §3.1: e/(e−1) periods to the first
        // probe on average), and the prober suspects it a whole period after its probe began, when the
        // ping-reqs' wait ends (memberlist's probeNode). The p99 is then ln(100) + 1 = 5.6 periods,
        // within 6; 5 periods + the probe timeout (5.5) is below it, measured 5.60 s.
        let config = Config::lan(8, lifeguard);
        assert!(
            suspected.p99 <= 6 * config.probe_interval_ns,
            "p99 first suspicion {} ns",
            suspected.p99
        );
    }
}

/// Lifeguard's scenario: `n` members, 4% of them (at least one) slow from 2 s to the end, each
/// processing every message it receives `delay` late, `delay` drawn in 1-30 s: seconds-long, past
/// SWIM's suspicion timeout (6.0-8.4 s at 32-128 members) so the baseline does accuse, and below
/// Lifeguard's maximum (36.1-50.6 s). Past that maximum dynamic suspicion can't outwait the slow
/// member and only the local health multiplier's slower probing helps: drawn in 1-60 s, Lifeguard's
/// false positives were 2.6% of SWIM's (622 of 23,834; 618 of the 622 with a delay of 30 s or more).
fn slow_case(seed: u64, lifeguard: bool) -> (Config, Vec<Fault>, u64) {
    let mut r = Rng::new(seed ^ 0x510e);
    let n = u32::try_from(r.range(32, 128)).unwrap();
    let slow = (n * 4 / 100).max(1);
    let mut config = Config::lan(n, lifeguard);
    config.end_ns = 120 * SECOND;
    let mut members: Vec<u32> = (0..n).collect();
    let mut faults = Vec::new();
    let delay_ns = r.range(SECOND, 30 * SECOND);
    for i in 0..usize::try_from(slow).unwrap() {
        let j = usize::try_from(r.range(u64::try_from(i).unwrap(), u64::from(n) - 1)).unwrap();
        members.swap(i, j);
        faults.push(Fault::Slow {
            member: members[i],
            from_ns: 2 * SECOND,
            until_ns: config.end_ns,
            delay_ns,
        });
    }
    (config, faults, delay_ns)
}

/// S8 (Lifeguard cuts slow-member false positives 50-100x, PF[9]): do 100 seeds of 32 to 128
/// members with 4% slow, as the SWIM baseline and with Lifeguard; expect SWIM's false positives
/// (healthy members declared dead) above zero (non-vacuity) and Lifeguard's at most 2% of SWIM's.
#[test]
fn lifeguard_cuts_false_positives_from_slow_members_to_under_two_percent_of_swim() {
    let (mut swim, mut lifeguard) = (0u64, 0u64);
    let (mut swim_members, mut lifeguard_members) = (0u64, 0u64);
    let mut by_delay = [[0u64; 2]; 3];
    for seed in 0..100 {
        for on in [false, true] {
            let (config, faults, delay) = slow_case(seed, on);
            let report = membership::run(&config, &faults, seed).unwrap();
            by_delay[usize::try_from((delay / (10 * SECOND)).min(2)).unwrap()][usize::from(on)] +=
                report.false_positives;
            if on {
                lifeguard += report.false_positives;
                lifeguard_members += report.false_positive_members;
            } else {
                swim += report.false_positives;
                swim_members += report.false_positive_members;
            }
        }
    }
    println!(
        "false positives: SWIM {swim} ({swim_members} members), Lifeguard {lifeguard} ({lifeguard_members} members); by 10 s delay bucket [swim, lifeguard]: {by_delay:?}"
    );
    assert!(swim > 0, "the SWIM baseline made no false positives");
    assert!(
        lifeguard * 50 <= swim,
        "Lifeguard {lifeguard} is more than 2% of SWIM's {swim}"
    );
}

/// S8, step 3's case (Lifeguard's, PF[9]): do 200 seeds of 32 to 128 members with one member slow
/// from 2 s, its delay drawn from 1 s up to half Lifeguard's suspicion max timeout; expect no healthy
/// member declared dead with Lifeguard, and some with it off (non-vacuity). Every declaration against
/// a healthy member is the slow member's own: its suspicion is refuted only once its gossip reached
/// the target and the refutation came back, each processed a delay late, so it holds a false
/// suspicion only past 2·delay. Under half the max timeout, Lifeguard's dynamic suspicion outwaits
/// that; SWIM's fixed timeout (its min, 6.0-8.4 s) doesn't from 3-4.2 s on.
#[test]
fn a_slow_member_makes_lifeguard_declare_no_healthy_member_dead() {
    let (mut swim, mut lifeguard) = (0u64, 0u64);
    for seed in 0..200 {
        let mut r = Rng::new(seed ^ 0x51_0e01);
        let n = u32::try_from(r.range(32, 128)).unwrap();
        let slow = u32::try_from(r.range(0, u64::from(n) - 1)).unwrap();
        let (_, max) = Config::lan(n, true).suspicion_bounds().unwrap();
        let delay_ns = r.range(SECOND, max / 2);
        for on in [false, true] {
            let mut config = Config::lan(n, on);
            config.end_ns = 120 * SECOND;
            let fault = Fault::Slow {
                member: slow,
                from_ns: 2 * SECOND,
                until_ns: config.end_ns,
                delay_ns,
            };
            let report = membership::run(&config, &[fault], seed).unwrap();
            if on {
                lifeguard += report.false_positives;
            } else {
                swim += report.false_positives;
            }
        }
    }
    println!("one slow member, delay under half the max timeout: SWIM {swim}, Lifeguard {lifeguard}");
    assert!(swim > 0, "the SWIM baseline made no false positives");
    assert_eq!(lifeguard, 0);
}

/// H1 (a run is a function of its seed), for S8's model: do one crash-and-slow run twice from one
/// seed and once from another; expect equal reports, and a different trace hash from the other seed.
#[test]
fn a_run_is_a_function_of_its_seed() {
    let (mut config, mut faults, _) = slow_case(7, true);
    config.end_ns = 40 * SECOND;
    faults.push(Fault::Crash {
        member: 0,
        at_ns: 10 * SECOND,
    });
    let a = membership::run(&config, &faults, 7).unwrap();
    let b = membership::run(&config, &faults, 7).unwrap();
    let c = membership::run(&config, &faults, 8).unwrap();
    assert_eq!(a, b);
    assert_ne!(a.trace_hash, c.trace_hash);
    assert!(a.messages > 0 && a.crashes[0].first_dead.is_some());
}

/// H5 (bounded everywhere), for S8's model: do a run past each bound; expect its typed refusal.
#[test]
fn bounds_refuse_typed() {
    let crash = [Fault::Crash {
        member: 1,
        at_ns: 5 * SECOND,
    }];
    let mut config = Config::lan(16, true);
    config.end_ns = 30 * SECOND;
    config.max_events = 8;
    assert_eq!(
        membership::run(&config, &crash, 1),
        Err(SimError::Full {
            what: "the event queue",
            bound: 8
        })
    );
    let mut config = Config::lan(16, true);
    config.end_ns = 30 * SECOND;
    config.max_steps = 100;
    assert_eq!(
        membership::run(&config, &crash, 1),
        Err(SimError::StepBudget(100))
    );
    let mut config = Config::lan(16, true);
    config.end_ns = 30 * SECOND;
    config.max_relayed = 0;
    assert_eq!(
        membership::run(&config, &crash, 1),
        Err(SimError::Full {
            what: "a member's relayed probes",
            bound: 0
        })
    );
    for members in [1, membership::MAX_MEMBERS + 1] {
        assert!(matches!(
            membership::run(&Config::lan(members, true), &[], 1),
            Err(SimError::Config(_))
        ));
    }
    assert!(matches!(
        membership::run(
            &Config::lan(4, true),
            &[Fault::Crash {
                member: 4,
                at_ns: 0
            }],
            1
        ),
        Err(SimError::Config(_))
    ));
}
