//! How many cases a property test runs: the fewest that meet each kind of case it counts at least once with
//! probability [`MEET`], from the kind's share of the generated cases, `(1 - share)^n <= 1 - MEET`, as step
//! 7's suite sizes its runs (0.95^60). The shares are measured (a recorder test) or follow from the
//! strategy exactly; neither count is picked.

/// The chance a property test meets each kind of case it counts.
pub(crate) const MEET: f64 = 0.95;

/// The fewest cases that meet a kind of case of share `share` with probability [`MEET`].
pub(crate) fn cases_to_meet(share: f64) -> u32 {
    // A kind never met can't be sized for: the most cases a run can name.
    if share <= 0.0 {
        return u32::MAX;
    }
    let (mut n, mut missed) = (0u32, 1.0_f64);
    while missed > 1.0 - MEET {
        missed *= 1.0 - share;
        n = n.saturating_add(1);
    }
    n
}

/// Do size a run for a share of 5%; expect 59 cases (0.95^59 = 4.8% <= 5% < 0.95^58), the arithmetic step 7
/// cites for its sixty runs.
#[test]
fn a_five_percent_kind_takes_fifty_nine_cases() {
    assert_eq!(cases_to_meet(0.05), 59);
}
