//! Scheduler implementation goal 5 (`docs/design/failure-isolation.md`, S8): every fault class the
//! nemesis injects is detected and isolated within the bound the design states for it, on every one of
//! 1,000 seeds, with no verdict against what wasn't at fault; and each safeguard's mutant breaks its
//! bound (non-vacuity). The crash class is `tests/membership.rs`'s.

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

use hex_health::agent::REPEAT_LIMIT;
use hex_health::danger::Isolation;
use hex_health::grade::WINDOW_NS;
use hex_sim::faults::{self, MS, Mutant, S};

const SEEDS: u64 = 1_000;

fn p(xs: &mut [u64], q: usize) -> u64 {
    xs.sort_unstable();
    xs[(xs.len() - 1) * q / 100]
}

/// S8 gray/fail-slow (F2): a node 2.5-8x slower than its peers is suspected within 2 windows and
/// drained within 4, both under Panorama's 7 s, and no healthy node is suspected.
#[test]
fn a_slow_node_is_suspected_in_2s_and_drained_in_4s() {
    let (mut det, mut iso) = (Vec::new(), Vec::new());
    for seed in 0..SEEDS {
        let (m, factor) = faults::gray(seed).unwrap();
        let d = m
            .detected_after_ns
            .unwrap_or_else(|| panic!("seed {seed} (x{factor}) undetected"));
        let i = m
            .isolated_after_ns
            .unwrap_or_else(|| panic!("seed {seed} (x{factor}) not drained"));
        assert!(d <= 2 * WINDOW_NS, "seed {seed}: suspected after {d} ns");
        assert!(i <= 4 * WINDOW_NS, "seed {seed}: drained after {i} ns");
        assert!(i < 7 * S);
        assert_eq!(m.wrong_verdicts, 0, "seed {seed}");
        det.push(d);
        iso.push(i);
    }
    println!(
        "gray: suspect p50 {} p99 {} max {} ms; drain p50 {} p99 {} max {} ms",
        p(&mut det, 50) / MS,
        p(&mut det, 99) / MS,
        p(&mut det, 100) / MS,
        p(&mut iso, 50) / MS,
        p(&mut iso, 99) / MS,
        p(&mut iso, 100) / MS,
    );
}

/// AC-T7.10.3 in the model (F3): a stalled disk is suspected within one timeout and one probe period,
/// and drained within three timeouts and one period.
#[test]
fn a_stalled_disk_is_suspected_then_drained_within_its_bound() {
    for seed in 0..SEEDS {
        let (m, timeout, period) = faults::disk_stall(seed).unwrap();
        let (suspect, drain) = faults::disk_stall_bounds(timeout, period);
        let d = m.detected_after_ns.unwrap();
        let i = m.isolated_after_ns.unwrap();
        assert!(d <= suspect, "seed {seed}: {d} > {suspect}");
        assert!(i <= drain, "seed {seed}: {i} > {drain}");
        assert!(
            d > timeout - period,
            "seed {seed}: suspected before its timeout"
        );
    }
}

/// S8 antagonist (F4): an antagonist is capped within 3 CPI samples (CPI²'s one a minute) and a second
/// of its onset, and no bystander is.
#[test]
fn an_antagonist_is_capped_within_three_samples() {
    let mut caught = Vec::new();
    for seed in 0..SEEDS {
        let (m, factor) = faults::antagonist(seed).unwrap();
        let i = m
            .isolated_after_ns
            .unwrap_or_else(|| panic!("seed {seed} (x{factor}): not capped"));
        assert!(
            i <= 3 * 60 * S + S,
            "seed {seed} (x{factor}): capped after {} s",
            i / S
        );
        assert_eq!(m.wrong_verdicts, 0, "seed {seed}");
        caught.push(i / S);
    }
    println!(
        "antagonist: capped p50 {} s, max {} s",
        p(&mut caught, 50),
        p(&mut caught, 100)
    );
}

/// A4 stuck loop (F5): a looping session is stopped by its third identical call, and no healthy
/// session is.
#[test]
fn a_stuck_loop_is_stopped_by_its_third_identical_call() {
    for seed in 0..SEEDS {
        let (m, calls) = faults::stuck_loop(seed).unwrap();
        assert!(m.isolated_after_ns.is_some(), "seed {seed}: not stopped");
        assert!(
            (1..=REPEAT_LIMIT).contains(&calls),
            "seed {seed}: {calls} loop calls"
        );
        assert_eq!(m.wrong_verdicts, 0, "seed {seed}");
    }
}

/// A4 quota shown as slowness (F5): the account is held at its first typed error and no node is
/// suspected or charged.
#[test]
fn quota_holds_the_account_at_once_and_charges_no_node() {
    for seed in 0..SEEDS {
        let m = faults::quota(seed, Mutant::None).unwrap();
        assert!(m.detected_after_ns.unwrap() < 100 * MS, "seed {seed}");
        assert_eq!(m.wrong_verdicts, 0, "seed {seed}");
    }
}

/// A4 partial provider outage (F5): the provider is marked degraded within 2 windows and no node is
/// suspected or charged.
#[test]
fn a_provider_outage_is_marked_in_2s_and_charges_no_node() {
    for seed in 0..SEEDS {
        let m = faults::provider_outage(seed, Mutant::None).unwrap();
        let d = m
            .detected_after_ns
            .unwrap_or_else(|| panic!("seed {seed}: not marked"));
        assert!(d <= 2 * WINDOW_NS, "seed {seed}: marked after {d} ns");
        assert_eq!(m.wrong_verdicts, 0, "seed {seed}");
    }
}

/// Non-vacuity of F5's separation: fed to the node grade as slowness, provider errors do suspect
/// nodes.
#[test]
fn the_mutant_that_reads_provider_errors_as_slowness_suspects_nodes() {
    let wrong: u64 = (0..100)
        .map(|seed| {
            faults::quota(seed, Mutant::ProviderErrorsAsNodeSlowness)
                .unwrap()
                .wrong_verdicts
                + faults::provider_outage(seed, Mutant::ProviderErrorsAsNodeSlowness)
                    .unwrap()
                    .wrong_verdicts
        })
        .sum();
    assert!(wrong > 0);
}

/// S8 dangerous (F7): every trip is isolated at its signal, to a stronger offered class or refused;
/// both outcomes occur.
#[test]
fn a_dangerous_workload_moves_up_or_is_refused_at_its_signal() {
    let (mut moved, mut refused) = (0, 0);
    for seed in 0..SEEDS {
        let (m, iso) = faults::dangerous(seed).unwrap();
        assert_eq!(m.wrong_verdicts, 0, "seed {seed}: {iso:?}");
        assert_eq!(m.isolated_after_ns, Some(0));
        match iso {
            Isolation::Move { .. } => moved += 1,
            Isolation::Refuse { .. } => refused += 1,
            Isolation::None => panic!("seed {seed}"),
        }
    }
    assert!(moved > 0 && refused > 0, "{moved} {refused}");
}

/// S8 ejection cap and mass loss (F6): a cell-wide fault ejects no more than the cap at once (and does
/// reach it), and a partition moves work no faster than the eviction rate.
#[test]
fn a_correlated_fault_never_passes_the_cap_or_the_rate() {
    let mut refused = 0;
    for seed in 0..SEEDS {
        let c = faults::correlated(seed, Mutant::None).unwrap();
        assert!(c.max_ejected <= c.cap, "seed {seed}: {c:?}");
        assert!(
            c.max_moved_in_window <= c.allowed_in_window,
            "seed {seed}: {c:?}"
        );
        assert!(c.moved >= 1, "seed {seed}");
        refused += c.refused;
    }
    assert!(refused > 0);
}

/// Non-vacuity of F6: with the cap or the rate switched off, the same faults break them.
#[test]
fn without_the_cap_or_the_rate_a_correlated_fault_breaks_them() {
    let over_cap = (0..100).any(|seed| {
        let c = faults::correlated(seed, Mutant::NoEjectionCap).unwrap();
        c.max_ejected > c.cap
    });
    let over_rate = (0..100).any(|seed| {
        let c = faults::correlated(seed, Mutant::NoEvictionRate).unwrap();
        c.max_moved_in_window > c.allowed_in_window
    });
    assert!(over_cap && over_rate, "{over_cap} {over_rate}");
}

/// H1: a fault run is a function of its seed.
#[test]
fn a_fault_run_is_a_function_of_its_seed() {
    assert_eq!(faults::gray(7).unwrap(), faults::gray(7).unwrap());
    assert_eq!(
        faults::correlated(7, Mutant::None).unwrap(),
        faults::correlated(7, Mutant::None).unwrap()
    );
}
