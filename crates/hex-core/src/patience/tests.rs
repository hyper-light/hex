//! The learned wait's tests (decision 9; Ada's 2026-11-06 rule and the lead's failure-bound rule; AC-T7.1.1,
//! AC-T7.1.3: every wait a node makes is bounded and learned). Against random answers, the wait is twice the
//! slowest, never shrinks, and lets through every answer no more than twice slower than any seen, and no other.

use std::time::Duration;

use proptest::prelude::*;

use super::*;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

proptest! {
    /// Do: record random answers, then ask whether a new answer would come within the wait. Expect: after each
    /// answer the wait is twice the slowest so far, never less than before; a new answer comes within it exactly
    /// when it is at most twice the slowest.
    #[test]
    fn the_wait_is_twice_the_slowest_answer(
        answers in prop::collection::vec(0u64..86_400_000_000_000, 1..100),
        next in 0u64..200_000_000_000_000,
    ) {
        let mut patience = Patience::new();
        let (mut before, mut slowest) = (Duration::ZERO, Duration::ZERO);
        for &ns in &answers {
            let answer = Duration::from_nanos(ns);
            patience.answered(answer);
            slowest = slowest.max(answer);
            let got = patience.deadline().unwrap();
            prop_assert_eq!(got, slowest * 2);
            prop_assert!(got >= before, "the wait shrank from {:?} to {:?}", before, got);
            before = got;
        }
        prop_assert_eq!(patience.slowest(), Some(slowest));
        let next = Duration::from_nanos(next);
        prop_assert_eq!(next <= patience.deadline().unwrap(), next <= slowest * 2);
    }
}

/// Calibration. Do: ask a wait with no answer recorded. Expect: no bound to arm, so the shell's first wait is its
/// calibration read.
#[test]
fn with_no_answer_there_is_no_bound() {
    let p = Patience::new();
    assert_eq!(p.deadline(), None);
    assert_eq!(p.slowest(), None);
}

/// The calibration read arms the bound. Do: record one answer of 3 ms. Expect: the wait 6 ms.
#[test]
fn the_calibration_read_arms_the_bound_at_twice_its_time() {
    let mut p = Patience::new();
    p.answered(ms(3));
    assert_eq!(p.deadline(), Some(ms(6)));
}

/// A running maximum. Do: answer in 40 ms, then 200 times in 1 ms. Expect: the wait stays 80 ms (a slow answer that
/// came is a fact about the host), and one slower answer, 50 ms, raises it to 100 ms.
#[test]
fn fast_answers_never_shrink_the_wait_and_a_slower_one_raises_it() {
    let mut p = Patience::new();
    p.answered(ms(40));
    for _ in 0..200 {
        p.answered(ms(1));
    }
    assert_eq!(p.deadline(), Some(ms(80)));
    p.answered(ms(50));
    assert_eq!(p.deadline(), Some(ms(100)));
}
