//! The learned wait's tests (decision 9; Ada's 2026-11-06 rule; AC-T7.1.1, AC-T7.1.3: every wait a node makes is
//! bounded and learned). Against an oracle that computes RFC 6298 §2 and §5.5 in exact rationals, under random
//! answers and expiries, the wait is the oracle's to the nanosecond's rounding, and never leaves its bounds; and each
//! rule the RFC states, the first wait, the first answer, convergence, the back-off and its collapse, is shown alone.

use std::time::Duration;

use proptest::prelude::*;

use super::*;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// Bounds wide enough that only the estimator shows: a nanosecond to a day.
const WIDE: Bounds = Bounds {
    first: Duration::from_secs(1),
    floor: Duration::from_nanos(1),
    ceiling: Duration::from_secs(86_400),
};

/// RFC 6298 as the RFC writes it, in f64 nanoseconds (exact for the sub-day times drawn here, 2^53 ns being 104
/// days), clamped to the same bounds.
#[derive(Debug)]
struct Oracle {
    bounds: Bounds,
    srtt: Option<(f64, f64)>,
    rto: f64,
}

impl Oracle {
    fn new(bounds: Bounds) -> Self {
        let mut o = Self {
            bounds,
            srtt: None,
            rto: 0.0,
        };
        o.rto = o.clamp(bounds.first.as_nanos() as f64);
        o
    }

    fn clamp(&self, ns: f64) -> f64 {
        ns.max(self.bounds.floor.as_nanos() as f64)
            .min(self.bounds.ceiling.as_nanos() as f64)
    }

    fn answered(&mut self, r: Duration) {
        let r = r.as_nanos() as f64;
        let (srtt, rttvar) = match self.srtt {
            None => (r, r / 2.0),
            Some((srtt, rttvar)) => {
                let rttvar = 0.75 * rttvar + 0.25 * (srtt - r).abs();
                (0.875 * srtt + 0.125 * r, rttvar)
            }
        };
        self.srtt = Some((srtt, rttvar));
        self.rto = self.clamp(srtt + 4.0 * rttvar);
    }

    fn expired(&mut self) {
        self.rto = self.clamp(self.rto * 2.0);
    }
}

#[derive(Debug, Clone, Copy)]
enum Event {
    Answered(u64),
    Expired,
}

fn events() -> impl Strategy<Value = Vec<Event>> {
    prop::collection::vec(
        prop_oneof![
            8 => (0u64..86_400_000_000_000).prop_map(Event::Answered),
            8 => (0u64..10_000_000).prop_map(Event::Answered),
            1 => Just(Event::Expired),
        ],
        0..200,
    )
}

fn bounds() -> impl Strategy<Value = Bounds> {
    prop_oneof![
        Just(RFC_6298),
        Just(WIDE),
        (0u64..120_000, 0u64..120_000, 0u64..120_000).prop_map(|(a, b, c)| {
            let (floor, ceiling) = (a.min(b), a.max(b));
            Bounds {
                first: ms(c),
                floor: ms(floor),
                ceiling: ms(ceiling),
            }
        }),
    ]
}

proptest! {
    /// Do: play random answers and expiries on a wait and on the oracle. Expect: after each, the wait is the
    /// oracle's within 64 ns, doubled for each expiry since the last answer (each step rounds a part down by under a
    /// nanosecond, and the smoothing forgets old rounding by 7/8 and 3/4 a step, so it stays under 8 ns in SRTT and
    /// 12 in RTTVAR, 56 in the wait; a back-off doubles it with the wait), and within its bounds.
    #[test]
    fn the_wait_is_rfc_6298s_within_its_bounds(bounds in bounds(), events in events()) {
        let mut patience = Patience::new(bounds);
        let mut oracle = Oracle::new(bounds);
        let mut slack = 64.0;
        for event in events {
            match event {
                Event::Answered(ns) => {
                    patience.answered(Duration::from_nanos(ns));
                    oracle.answered(Duration::from_nanos(ns));
                    slack = 64.0;
                }
                Event::Expired => {
                    patience.expired();
                    oracle.expired();
                    slack *= 2.0;
                }
            }
            let got = patience.deadline();
            let off = (got.as_nanos() as f64 - oracle.rto).abs();
            prop_assert!(off <= slack, "{:?} against {} ns ({:?})", got, oracle.rto, event);
            prop_assert!(got >= bounds.floor.min(bounds.ceiling) && got <= bounds.ceiling, "{:?} out of {:?}", got, bounds);
        }
    }
}

/// RFC 6298 (2.1). Do: ask a wait with no answer seen. Expect: its first bound, 1 s under RFC 6298's.
#[test]
fn before_any_answer_the_wait_is_the_first() {
    assert_eq!(Patience::new(RFC_6298).deadline(), Duration::from_secs(1));
    assert_eq!(Patience::new(WIDE).smoothed(), None);
}

/// RFC 6298 (2.2). Do: answer once in 100 ms. Expect: SRTT 100 ms, RTTVAR 50 ms, the wait 300 ms.
#[test]
fn the_first_answer_sets_srtt_and_rttvar() {
    let mut p = Patience::new(WIDE);
    p.answered(ms(100));
    assert_eq!(p.smoothed(), Some((ms(100), ms(50))));
    assert_eq!(p.deadline(), ms(300));
}

/// RFC 6298 (2.3), RTTVAR from the SRTT before. Do: answer in 100 ms, then 200 ms. Expect: RTTVAR 3/4·50 +
/// 1/4·|100 − 200| = 62.5 ms, SRTT 7/8·100 + 1/8·200 = 112.5 ms, the wait 112.5 + 4·62.5 = 362.5 ms.
#[test]
fn a_later_answer_smooths_rttvar_first() {
    let mut p = Patience::new(WIDE);
    p.answered(ms(100));
    p.answered(ms(200));
    assert_eq!(
        p.smoothed(),
        Some((
            Duration::from_micros(112_500),
            Duration::from_micros(62_500)
        ))
    );
    assert_eq!(p.deadline(), Duration::from_micros(362_500));
}

/// Learned, not picked: do answer 200 times in 3 ms. Expect: the wait comes down from 9 ms to within a microsecond
/// of 3 ms, the answers' own time, as RTTVAR decays by 3/4 an answer (non-vacuity: it started three times higher).
#[test]
fn steady_answers_bring_the_wait_to_their_time() {
    let mut p = Patience::new(WIDE);
    p.answered(ms(3));
    assert_eq!(p.deadline(), ms(9));
    for _ in 0..200 {
        p.answered(ms(3));
    }
    assert!(
        p.deadline().abs_diff(ms(3)) < Duration::from_micros(1),
        "{:?}",
        p.deadline()
    );
}

/// RFC 6298 (2.4), (2.5). Do: answer in a microsecond, then in an hour, under RFC 6298's bounds. Expect: the
/// wait is 1 s after the first and 60 s after the second.
#[test]
fn the_wait_stays_within_the_floor_and_the_ceiling() {
    let mut p = Patience::new(RFC_6298);
    p.answered(Duration::from_micros(1));
    assert_eq!(p.deadline(), Duration::from_secs(1));
    p.answered(Duration::from_secs(3_600));
    assert_eq!(p.deadline(), Duration::from_secs(60));
}

/// RFC 6298 (5.5) and §5's note after (5.7). Do: under RFC 6298's bounds, let seven waits run out, then answer
/// in 10 ms after many such answers. Expect: 2, 4, 8, 16, 32, 60, 60 s, then the wait collapses to the floor.
#[test]
fn a_wait_that_runs_out_doubles_the_next_until_an_answer() {
    let mut p = Patience::new(RFC_6298);
    let waits: Vec<u64> = (0..7)
        .map(|_| {
            p.expired();
            p.deadline().as_secs()
        })
        .collect();
    assert_eq!(waits, [2, 4, 8, 16, 32, 60, 60]);
    for _ in 0..50 {
        p.answered(ms(10));
    }
    assert_eq!(p.deadline(), Duration::from_secs(1));
}

/// A fixed wait never moves. Do: answer and expire a wait fixed at 50 ms. Expect: 50 ms throughout.
#[test]
fn a_fixed_wait_never_moves() {
    let mut p = Patience::new(Bounds::fixed(ms(50)));
    p.answered(ms(1));
    p.expired();
    p.answered(Duration::from_secs(9));
    assert_eq!(p.deadline(), ms(50));
}
