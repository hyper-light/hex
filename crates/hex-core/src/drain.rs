//! How long a group of processes gets to empty after `SIGKILL`: a failure bound, derived from measurement
//! (Ada's rule, 2026-11-06: no picked numbers). A kill ends its members in milliseconds; past this bound a
//! drain is a fault reported by name, never a limit raised (CLAUDE.md §4).

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// The record's envelope, doubled: 2 x (0.883 ms + 0.298 ms per member past the first)
    /// (bench/native/macos/records/killscale-a84e094-20261106T160339Z.txt). Do ask for 1, 32, 512 and 6,000
    /// members; expect 1.766 ms, 20.242 ms, 306.322 ms and 3.577 s.
    #[test]
    fn the_bound_is_the_measured_envelope_doubled() {
        assert_eq!(bound(1), Duration::from_nanos(1_766_000));
        assert_eq!(bound(32), Duration::from_nanos(20_242_000));
        assert_eq!(bound(512), Duration::from_nanos(306_322_000));
        assert_eq!(bound(6_000), Duration::from_nanos(3_577_170_000));
    }

    /// A group listed empty still gets the one-member bound (the kill may race the listing), and the bound
    /// never shrinks as a group grows, nor overflows (CLAUDE.md §1). Do ask for 0 and for the most a usize
    /// holds; expect the one-member bound and a finite bound.
    #[test]
    fn the_bound_grows_with_the_group_and_never_overflows() {
        assert_eq!(bound(0), bound(1));
        let mut last = Duration::ZERO;
        for n in [1, 2, 31, 32, 33, 1000, 100_000] {
            assert!(bound(n) >= last);
            last = bound(n);
        }
        assert!(bound(usize::MAX) >= last);
    }
}
