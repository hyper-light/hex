//! The learned wait's tests (decision 9; Ada's 2026-11-06 rule and the lead's failure-bound rule; AC-T7.1.1,
//! AC-T7.1.3: every wait a node makes is bounded and learned). Against random records and answers, the wait is twice
//! the larger of the record's slowest and the slowest seen, never shrinks, and lets through every answer no more
//! than twice slower than those, and no other; a measuring wait is unarmed until its first answer.

use std::time::Duration;

use proptest::prelude::*;

use super::*;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

proptest! {
    /// Do: seed a wait from a random record (or none, measuring), record random answers, then ask whether a new
    /// answer would come within it. Expect: after each answer the wait is twice the larger of the record's slowest
    /// and the slowest so far, never less than before; a new answer comes within it exactly when it is at most that.
    #[test]
    fn the_wait_is_twice_the_slowest_recorded_or_seen(
        record in proptest::option::of(0u64..86_400_000_000_000),
        answers in prop::collection::vec(0u64..86_400_000_000_000, 1..100),
        next in 0u64..200_000_000_000_000,
    ) {
        let patience = match record {
            Some(ns) => Patience::seeded("test", Duration::from_nanos(ns)),
            None => Patience::measuring("test"),
        };
        let mut slowest = record.map(Duration::from_nanos);
        let mut before = patience.deadline();
        for &ns in &answers {
            let answer = Duration::from_nanos(ns);
            patience.answered(answer);
            slowest = Some(slowest.map_or(answer, |s| s.max(answer)));
            let got = patience.deadline();
            prop_assert_eq!(got, slowest.map(|s| s * 2));
            prop_assert!(got >= before, "the wait shrank from {:?} to {:?}", before, got);
            before = got;
        }
        let next = Duration::from_nanos(next);
        prop_assert_eq!(next <= patience.deadline().unwrap(), next <= slowest.unwrap() * 2);
    }
}

/// The seed. Do: seed a wait from a record whose slowest answer is 1.918 ms, and ask before any answer. Expect:
/// the bound armed at 3.836 ms, with no calibration read.
#[test]
fn before_any_answer_the_bound_is_twice_the_records_slowest() {
    let p = Patience::seeded("test", Duration::from_micros(1_918));
    assert_eq!(p.deadline(), Some(Duration::from_micros(3_836)));
}

/// The record is a floor the node's answers only raise. Do: seed at 40 ms, answer 200 times in 1 ms, then once in
/// 50 ms. Expect: the wait stays 80 ms through the fast answers, then 100 ms.
#[test]
fn fast_answers_never_shrink_the_wait_and_a_slower_one_raises_it() {
    let p = Patience::seeded("test", ms(40));
    for _ in 0..200 {
        p.answered(ms(1));
    }
    assert_eq!(p.deadline(), Some(ms(80)));
    p.answered(ms(50));
    assert_eq!(p.deadline(), Some(ms(100)));
}

/// Measuring. Do: ask a wait with no record and no answer, then answer once in 3 ms. Expect: unarmed, then 6 ms.
#[test]
fn a_measuring_wait_is_unarmed_until_its_first_answer() {
    let p = Patience::measuring("test");
    assert_eq!((p.deadline(), p.slowest()), (None, None));
    p.answered(ms(3));
    assert_eq!(p.deadline(), Some(ms(6)));
}

/// Shared by threads. Do: from eight threads, answer 1 to 800 ms between them. Expect: the slowest, 800 ms, kept.
#[test]
fn answers_from_many_threads_keep_the_slowest() {
    let p = Patience::measuring("test");
    std::thread::scope(|s| {
        for t in 0..8u64 {
            let p = &p;
            s.spawn(move || {
                for n in 0..100u64 {
                    p.answered(ms(t * 100 + n + 1));
                }
            });
        }
    });
    assert_eq!(p.slowest(), Some(ms(800)));
}

/// The census (the waits census probe, `bench/competitive/probes/waits_census.md`): every answer a wait records
/// reaches the installed sink by the wait's name, from the wait's own timing point. Do: install a sink, answer a wait
/// named `census.test` in 7 ms and 9 ms. Expect: the sink told both, by that name.
#[test]
fn every_answer_reaches_the_census_by_its_waits_name() {
    static TOLD: std::sync::Mutex<Vec<(&'static str, Duration)>> =
        std::sync::Mutex::new(Vec::new());
    fn sink(name: &'static str, took: Duration) {
        TOLD.lock().unwrap().push((name, took));
    }
    assert!(census(sink), "the first install takes");
    let p = Patience::seeded("census.test", ms(1));
    p.answered(ms(7));
    p.answered(ms(9));
    let told: Vec<Duration> = TOLD
        .lock()
        .unwrap()
        .iter()
        .filter(|(name, _)| *name == "census.test")
        .map(|(_, took)| *took)
        .collect();
    assert_eq!(told, [ms(7), ms(9)]);
}
