//! The tests' following waits (decision 9, the person's ruling of 2026-11-13): a fact past W* waited for while its
//! party advances, a party that stands still a whole window or ends refused by type, a call sliced at W* and at each
//! window, and a thread read as a party.

use super::*;

/// A wait that has seen one answer of `slowest`, as a test's own would after its first.
fn learned(name: &'static str, slowest: Duration) -> Patience {
    let wait = Patience::measuring(name);
    wait.answered(slowest);
    wait
}

/// Do: wait for a fact already true. Expect: one look, its answer the wait's first sample.
#[test]
fn a_fact_already_true_takes_one_look_and_is_its_waits_sample() {
    let wait = Patience::measuring("test.testkit.follow.at_once");
    assert_eq!(
        until(&wait, "a fact already true", || Party::Live, || true),
        Ok(1)
    );
    assert!(wait.slowest().is_some());
}

/// Decision 9: past W* a test's wait goes on while its party advances. Do: a wait whose W* is 1 ns, a party whose count
/// rises at each look, and a fact that comes at the thousandth look. Expect: the thousand looks, never refused; the
/// run-out told on the way (the wait following), then the answer its sample, W* risen to it and bounding again.
#[test]
fn past_w_star_a_wait_goes_on_while_its_party_advances() {
    let learned_at = Duration::from_nanos(1);
    let wait = learned("test.testkit.follow.advancing", learned_at);
    let mut progress = 0u64;
    let mut looks = 0;
    let got = until(
        &wait,
        "a fact at the thousandth look",
        || {
            progress += 1;
            Party::Alive { progress }
        },
        || {
            looks += 1;
            looks == 1_000
        },
    );
    assert_eq!(got, Ok(1_000));
    assert!(wait.slowest().is_some_and(|slowest| slowest > learned_at));
    assert_eq!(
        wait.deadline(),
        wait.slowest(),
        "W* bounds again after the answer"
    );
}

/// Decision 9: a party that stands still over a whole window past W* fails the wait, typed. Do: a wait whose W* is
/// 1 ms, a party alive whose count never rises, and a fact that never comes. Expect: unmet, stalled over a window of
/// W*, never before a whole window past W* (2 ms).
#[test]
fn a_party_standing_still_a_whole_window_past_w_star_fails_the_wait() {
    let window = Duration::from_millis(1);
    let wait = learned("test.testkit.follow.stalled", window);
    let unmet = until(
        &wait,
        "a fact that never comes",
        || Party::Alive { progress: 7 },
        || false,
    )
    .unwrap_err();
    assert!(
        matches!(unmet.stopped, Stopped::Stalled { window: w, .. } if w == window),
        "{unmet}"
    );
    assert!(unmet.after >= window * 2, "{unmet}");
    assert_eq!(wait.slowest(), Some(window), "no sample");
}

/// A party that ends fails the wait at once, within W* or past it, once a last look hasn't found the fact. Do: a
/// measuring wait whose party ends at its third look, the fact never coming; then one whose fact comes as its party
/// ends. Expect: the first unmet, its party ended; the second answered.
#[test]
fn a_party_that_ends_ends_the_wait_unless_its_fact_came_as_it_ended() {
    let wait = Patience::measuring("test.testkit.follow.ended");
    let mut looks = 0;
    let unmet = until(
        &wait,
        "a fact its party never makes",
        || {
            looks += 1;
            if looks < 3 { Party::Live } else { Party::Ended }
        },
        || false,
    )
    .unwrap_err();
    assert_eq!(unmet.stopped, Stopped::Ended, "{unmet}");
    let ended = std::cell::Cell::new(false);
    let got = until(
        &wait,
        "a fact made as its party ends",
        || {
            ended.set(true);
            Party::Ended
        },
        || ended.get(),
    );
    assert_eq!(got, Ok(2));
}

/// A call that takes its own deadline is given W*'s end, then each window's, its party looked at between. Do: a wait
/// whose W* is 1 ms; a call that answers at its fourth slice, its party advancing. Expect: the answer; the first slice
/// given W*'s end from the start, each later one a window past the look before it.
#[test]
fn a_call_is_sliced_at_w_star_and_at_each_window_past_it() {
    let window = Duration::from_millis(1);
    let wait = learned("test.testkit.follow.within", window);
    let began = Instant::now();
    let mut slices = Vec::new();
    let mut progress = 0u64;
    let got = within(
        &wait,
        "an answer at the fourth slice",
        || {
            progress += 1;
            Party::Alive { progress }
        },
        |by| {
            slices.push(by);
            if slices.len() == 4 {
                return Some("answered");
            }
            let at = by.unwrap();
            while Instant::now() < at {
                std::thread::yield_now();
            }
            None
        },
    );
    assert_eq!(got, Ok("answered"));
    let first = slices[0].unwrap();
    assert!(first >= began + window, "{slices:?}");
    for pair in slices.windows(2) {
        let (Some(was), Some(next)) = (pair[0], pair[1]) else {
            panic!("a slice with no end past W*: {slices:?}");
        };
        assert!(next >= was, "{slices:?}");
    }
}

/// A thread as a party: alive with its CPU time while it sleeps, ended once it has gone. Do: a thread that names
/// itself, then sleeps on a channel; read it until it reads asleep, twice, then wake it to its end and join it.
/// Expect: asleep, its count the same both times; ended once joined.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn a_thread_reads_alive_while_it_sleeps_and_ended_once_gone() {
    use std::sync::mpsc;
    /// Longest this test waits for the thread to read asleep: a bound, so a reading that never comes fails the test
    /// instead of hanging it.
    const PATIENCE: Duration = Duration::from_secs(30);
    let (named, name) = mpsc::sync_channel(1);
    let (wake, woken) = mpsc::sync_channel::<()>(1);
    let sleeper = std::thread::spawn(move || {
        named.send(Thread::current().unwrap()).unwrap();
        let _ = woken.recv();
    });
    let thread = name.recv().unwrap();
    let began = Instant::now();
    let asleep = loop {
        if let Party::Alive { progress } = thread.party() {
            break progress;
        }
        assert!(began.elapsed() < PATIENCE, "{thread:?} never read asleep");
        std::thread::yield_now();
    };
    assert_eq!(thread.party(), Party::Alive { progress: asleep });
    wake.send(()).unwrap();
    sleeper.join().unwrap();
    assert_eq!(thread.party(), Party::Ended);
}
