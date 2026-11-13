//! The learned wait's tests (decision 9; Ada's rules of 2026-11-06 and 2026-11-13, no picked constant and no record
//! baked in; AC-T7.1.1, AC-T7.1.3: every wait a node makes is bounded and learned). Against random answers, the wait is
//! the slowest seen, never shrinks, and lets through every answer no slower than that, and no other; before its first
//! answer, and after a run-out, it follows its party: alive and advancing it is waited for, dead, reset or stalled
//! past the longest gap its own progress showed it is refused, typed.

use std::time::Duration;

use proptest::prelude::*;

use super::*;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// A party alive at `progress`.
fn alive(progress: u64) -> Party {
    Party::Alive { progress }
}

proptest! {
    /// Do: record random answers on a wait with none, then ask whether a new answer would come within it. Expect:
    /// before the first, no bound (it follows its party); after each, the slowest so far, never less than before, and
    /// a new answer comes within it exactly when it is at most that.
    #[test]
    fn the_wait_is_the_slowest_answer_seen(
        answers in prop::collection::vec(0u64..86_400_000_000_000, 1..100),
        next in 0u64..200_000_000_000_000,
    ) {
        let patience = Patience::measuring("test");
        prop_assert_eq!(patience.deadline(), None);
        let mut slowest: Option<Duration> = None;
        let mut before = None;
        for &ns in &answers {
            let answer = Duration::from_nanos(ns);
            patience.answered(answer);
            slowest = Some(slowest.map_or(answer, |s| s.max(answer)));
            let got = patience.deadline();
            prop_assert_eq!(got, slowest);
            prop_assert!(got >= before, "the wait shrank from {:?} to {:?}", before, got);
            before = got;
        }
        let next = Duration::from_nanos(next);
        prop_assert_eq!(next <= patience.deadline().unwrap(), next <= slowest.unwrap());
    }

    /// The stall bound is what the wait measures. Do: look at a live party at random times, its progress advancing at
    /// random looks. Expect: never refused while it advances; refused as stalled exactly at a look past the longest gap
    /// between its advances measured so far, none before a gap is measured.
    #[test]
    fn a_stall_is_judged_by_the_waits_own_measured_gaps(
        looks in prop::collection::vec((1u64..1_000_000, any::<bool>()), 1..200),
    ) {
        let mut follow = Follow::new();
        let (mut now, mut progress) = (Duration::ZERO, 0u64);
        let mut last: Option<Duration> = None;
        let mut longest = Duration::ZERO;
        for &(step, advances) in &looks {
            now += Duration::from_nanos(step);
            if advances || last.is_none() {
                progress += 1;
                if let Some(at) = last {
                    longest = longest.max(now - at);
                }
                last = Some(now);
                prop_assert_eq!(follow.look(alive(progress), now), Ok(()));
                continue;
            }
            let at = last.unwrap();
            let got = follow.look(alive(progress), now);
            if !longest.is_zero() && now - at > longest {
                prop_assert_eq!(got, Err(Stopped::Stalled { since: at, window: longest }));
                break;
            }
            prop_assert_eq!(got, Ok(()));
        }
    }
}

proptest! {
    /// Past W* (the person's ruling, 2026-11-13: W* is a bound only while answers come within it). Do: learn W* from a
    /// random answer, run a wait out at it, then follow its party at random looks, its progress advancing at random
    /// looks. Expect: never refused while it advances, however long past W* it goes; refused as stalled exactly at a
    /// look more than a whole W* past its last advance, the window W* itself, not a gap this wait measured.
    #[test]
    fn past_its_bound_a_wait_is_refused_only_over_a_whole_window_without_progress(
        answer in 1u64..1_000_000,
        looks in prop::collection::vec((1u64..1_000_000, any::<bool>()), 1..200),
    ) {
        let patience = Patience::measuring("test");
        let w = Duration::from_nanos(answer);
        patience.answered(w);
        patience.ran_out(w);
        let mut follow = patience.follow();
        let (mut now, mut progress) = (Duration::ZERO, 0u64);
        let mut last: Option<Duration> = None;
        for &(step, advances) in &looks {
            now += Duration::from_nanos(step);
            if advances || last.is_none() {
                progress += 1;
                last = Some(now);
                prop_assert_eq!(follow.look(alive(progress), now), Ok(()));
                continue;
            }
            let at = last.unwrap();
            let got = follow.look(alive(progress), now);
            if now - at > w {
                prop_assert_eq!(got, Err(Stopped::Stalled { since: at, window: w }));
                break;
            }
            prop_assert_eq!(got, Ok(()));
        }
    }
}

/// AC-T7.1.3, decision 9 (original; the person's ruling of 2026-11-13): a run-out at W* never refuses by itself. Do: answer
/// a wait in 5 ms, run the next out at 5 ms, then follow its party past it: advancing every 4 ms for a minute, then
/// standing still. Expect: the run-out no sample (W* still 5 ms); the party waited for the whole minute, far past W*;
/// then refused, typed, as stalled once a whole 5 ms window shows no progress, and not before; the stall's window W*.
#[test]
fn original_a_run_out_at_the_bound_never_refuses_by_itself() {
    let p = Patience::measuring("test");
    p.answered(ms(5));
    p.ran_out(ms(5));
    assert_eq!(p.slowest(), Some(ms(5)), "a run-out is no sample");
    let mut follow = p.follow();
    let mut t = 0;
    while t <= 60_000 {
        assert_eq!(
            follow.look(alive(t), ms(t)),
            Ok(()),
            "refused at {t} ms while advancing"
        );
        t += 4;
    }
    let last = t - 4;
    assert_eq!(follow.stalls_after(), Some(ms(last + 5)));
    assert_eq!(follow.look(alive(last), ms(last + 5)), Ok(()));
    assert_eq!(
        follow.look(alive(last), ms(last + 6)),
        Err(Stopped::Stalled {
            since: ms(last),
            window: ms(5)
        })
    );
}

/// Decision 9: a party whose progress its shell can't count is followed by its life alone. Do: past W*, follow a live
/// party with no count for an hour, then one that ends. Expect: every look `Ok`, never stalled; the end refused, typed.
#[test]
fn a_party_with_no_progress_count_is_followed_by_its_life_alone() {
    let mut follow = Follow::past(ms(5));
    for second in 0..=3_600u64 {
        assert_eq!(follow.look(Party::Live, ms(second * 1_000)), Ok(()));
    }
    assert_eq!(follow.stalls_after(), None);
    assert_eq!(
        follow.look(Party::Ended, ms(3_600_001)),
        Err(Stopped::Ended)
    );
}

/// Decision 9: the follow a wait takes. Do: ask a wait with no answer, then one that has seen 7 ms. Expect: the cold
/// start's, judged by its own measured gaps (no stall before one is measured); then one past W*, over a 7 ms window.
#[test]
fn a_wait_follows_by_its_own_gaps_at_the_cold_start_and_over_w_star_past_it() {
    let p = Patience::measuring("test");
    let mut cold = p.follow();
    assert_eq!(cold.look(alive(1), Duration::ZERO), Ok(()));
    assert_eq!(
        cold.look(alive(1), ms(3_600_000)),
        Ok(()),
        "no gap measured, no stall"
    );
    p.answered(ms(7));
    let mut past = p.follow();
    assert_eq!(past.look(alive(1), Duration::ZERO), Ok(()));
    assert_eq!(past.stalls_after(), Some(ms(7)));
    assert_eq!(
        past.look(alive(1), ms(8)),
        Err(Stopped::Stalled {
            since: Duration::ZERO,
            window: ms(7)
        })
    );
}

/// The cold start (decision 9): no record is read, so before its first answer a wait has no bound and follows its
/// party. Do: on a wait with no answer, follow a party that dies; then one alive and advancing for a minute, far past
/// any answer this wait has seen, until it answers. Expect: the dead one refused, typed, as ended; the slow live one
/// waited for, every look `Ok`; its answer then the wait's bound.
#[test]
fn a_cold_wait_refuses_a_dead_party_and_waits_for_a_slow_live_one() {
    let p = Patience::measuring("test");
    assert_eq!(
        p.deadline(),
        None,
        "no record: the first wait follows its party"
    );
    let mut dead = Follow::new();
    assert_eq!(dead.look(alive(0), Duration::ZERO), Ok(()));
    assert_eq!(dead.look(Party::Ended, ms(1)), Err(Stopped::Ended));
    let mut slow = Follow::new();
    for second in 0..=60u64 {
        assert_eq!(slow.look(alive(second), ms(second * 1_000)), Ok(()));
    }
    p.answered(ms(60_000));
    assert_eq!(p.deadline(), Some(ms(60_000)));
}

/// A stall of the party's own measured progress. Do: follow a party advancing every 10 ms, then standing still.
/// Expect: waited for 10 ms past its last advance, refused, typed, as stalled at the first look past it; and a party
/// that has shown no gap yet judged by its end alone, however long it stands still.
#[test]
fn a_live_party_whose_progress_stalls_past_its_longest_gap_is_refused() {
    let mut follow = Follow::new();
    for t in [0, 10, 20] {
        assert_eq!(follow.look(alive(t), ms(t)), Ok(()));
    }
    assert_eq!(follow.stalls_after(), Some(ms(30)));
    assert_eq!(follow.look(alive(20), ms(30)), Ok(()));
    assert_eq!(
        follow.look(alive(20), ms(31)),
        Err(Stopped::Stalled {
            since: ms(20),
            window: ms(10)
        })
    );
    let mut first = Follow::new();
    assert_eq!(first.look(alive(7), Duration::ZERO), Ok(()));
    assert_eq!(first.stalls_after(), None);
    assert_eq!(first.look(alive(7), ms(3_600_000)), Ok(()));
    assert_eq!(first.look(Party::Ended, ms(3_600_001)), Err(Stopped::Ended));
}

/// After a run-out (decision 9; Karn's point: no sample, the tail unknown, the cold start's case). Do: answer a wait
/// in 5 ms, so bounded at 5 ms; run it out there; follow a slow live party to its answer in 30 ms; then run it out again
/// and follow a party that dies. Expect: the run-out no sample (slowest still 5 ms) and the next wait following, no
/// bound; the slow answer waited for and learned, the bound 30 ms; the dead party refused, typed; no factor anywhere.
#[test]
fn after_a_run_out_the_next_wait_follows_and_learns_its_uncensored_answer() {
    let p = Patience::measuring("test");
    p.answered(ms(5));
    assert_eq!(p.deadline(), Some(ms(5)));
    p.ran_out(ms(5));
    assert_eq!((p.deadline(), p.slowest()), (None, Some(ms(5))));
    let mut follow = Follow::new();
    for t in [0, 10, 20, 30] {
        assert_eq!(follow.look(alive(t), ms(t)), Ok(()));
    }
    p.answered(ms(30));
    assert_eq!(p.deadline(), Some(ms(30)));
    p.ran_out(ms(30));
    assert_eq!(p.deadline(), None);
    let mut dead = Follow::new();
    assert_eq!(dead.look(alive(0), Duration::ZERO), Ok(()));
    assert_eq!(dead.look(Party::Ended, ms(2)), Err(Stopped::Ended));
}

/// A party taken for dead or hung at the bound (a seed's hang bound): no sample, nothing for the census, but the next
/// wait follows its party. Do: answer a wait in 5 ms, then take its party for hung. Expect: no bound, the slowest still
/// 5 ms; the next answer, 1 ms, bounding it at the slowest seen, 5 ms, again.
#[test]
fn a_party_taken_for_hung_makes_the_next_wait_follow_without_a_sample() {
    let p = Patience::measuring("test");
    p.answered(ms(5));
    p.expired();
    assert_eq!((p.deadline(), p.slowest()), (None, Some(ms(5))));
    p.answered(ms(1));
    assert_eq!(p.deadline(), Some(ms(5)));
}

/// A host slower than any answer seen. Do: answer a wait in 1 ms, then make calls that each take 25 ms, every wait
/// answered within its bound, run out at it, or followed to its answer. Expect: one run-out, the next wait following to
/// the 25 ms answer, and none after; the bound the slowest answer, 25 ms, with nothing added.
#[test]
fn a_host_slower_than_any_answer_seen_runs_out_once() {
    let p = Patience::measuring("test");
    p.answered(ms(1));
    let mut ran_out = 0u32;
    for _ in 0..100 {
        match p.deadline() {
            Some(bound) if ms(25) > bound => {
                p.ran_out(bound);
                ran_out += 1;
            }
            _ => p.answered(ms(25)),
        }
    }
    assert_eq!((ran_out, p.deadline()), (1, Some(ms(25))));
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

/// The census (the waits census probe, `bench/competitive/probes/waits_census.md`): every answer a wait records, and
/// every time it runs out, reaches the installed sink by the wait's name, from the wait's own timing point; a run-out is
/// no sample (Karn's point), told apart from the answers, and raises nothing; a party taken for hung tells it nothing.
/// Do: install a sink, answer a wait named `census.test` in 7 ms and 9 ms, run it out at 20 ms, then take its party for
/// hung. Expect: the sink told the two answers and the run-out, by that name, and nothing of the hang; the slowest 9 ms.
#[test]
fn every_answer_and_run_out_reaches_the_census_by_its_waits_name() {
    static TOLD: std::sync::Mutex<Vec<(&'static str, Sample)>> = std::sync::Mutex::new(Vec::new());
    fn sink(name: &'static str, sample: Sample) {
        TOLD.lock().unwrap().push((name, sample));
    }
    assert!(census(sink), "the first install takes");
    let p = Patience::measuring("census.test");
    p.answered(ms(7));
    p.answered(ms(9));
    p.ran_out(ms(20));
    p.expired();
    let told: Vec<Sample> = TOLD
        .lock()
        .unwrap()
        .iter()
        .filter(|(name, _)| *name == "census.test")
        .map(|(_, sample)| *sample)
        .collect();
    assert_eq!(
        told,
        [
            Sample::Answered(ms(7)),
            Sample::Answered(ms(9)),
            Sample::RanOut(ms(20))
        ]
    );
    assert_eq!(p.slowest(), Some(ms(9)), "a run-out is no sample");
}

/// The census's file form, which `waits_census.py` reads: an answer `name nanos`, a run-out `name >nanos`.
#[test]
fn an_answer_and_a_run_out_are_census_lines_of_their_own() {
    assert_eq!(
        census_line("a.wait", Sample::Answered(Duration::from_nanos(41))),
        "a.wait 41\n"
    );
    assert_eq!(
        census_line("a.wait", Sample::RanOut(ms(440))),
        "a.wait >440000000\n"
    );
}

/// AC-T7.4.1: typed refusal census identity must not alter the existing online learning rule.
#[test]
fn refusal_census_preserves_learning_and_marks_only_the_optional_record() {
    let answered = Patience::measuring("outcome.answer");
    let refused = Patience::measuring("outcome.refused");
    for took in [ms(7), ms(3), ms(12)] {
        answered.answered(took);
        refused.refused(took);
        assert_eq!(answered.slowest(), refused.slowest());
        assert_eq!(answered.deadline(), refused.deadline());
    }
    assert_eq!(refused.slowest(), Some(ms(12)));
    assert_eq!(refused.deadline(), Some(ms(12)));
    assert_eq!(
        census_line("a.wait", Sample::Refused(Duration::from_nanos(41))),
        "a.wait !41\n"
    );
    assert_eq!(
        census_line("a.wait", Sample::Answered(Duration::from_nanos(41))),
        "a.wait 41\n"
    );
    assert_eq!(
        census_line("a.wait", Sample::RanOut(Duration::from_nanos(41))),
        "a.wait >41\n"
    );
}
