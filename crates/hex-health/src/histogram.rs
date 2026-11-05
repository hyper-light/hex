//! A fixed-size log histogram of positive values: four buckets per power of two, so a quantile read
//! from it is within 25% of the true value with a fixed 1 KiB per histogram however many values come.
//! Nothing is dropped: every value is counted in its bucket (CLAUDE.md §3).

use crate::HealthError;

/// Sub-buckets per power of two.
const SUB_BITS: u32 = 2;
const SUB: usize = 1 << SUB_BITS;
/// 64 powers of two of `SUB` buckets each cover every `u64`.
pub const BUCKETS: usize = 64 * SUB;

#[derive(Debug, Clone)]
pub struct Histogram {
    counts: Vec<u32>,
    total: u32,
}

impl Default for Histogram {
    fn default() -> Self {
        Self::new()
    }
}

impl Histogram {
    pub fn new() -> Self {
        Self {
            counts: vec![0; BUCKETS],
            total: 0,
        }
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    pub fn clear(&mut self) {
        self.counts.iter_mut().for_each(|c| *c = 0);
        self.total = 0;
    }

    pub fn record(&mut self, value: u64) -> Result<(), HealthError> {
        let full = HealthError::Full {
            what: "histogram count",
            bound: usize::try_from(u32::MAX).unwrap_or(usize::MAX),
        };
        let slot = self
            .counts
            .get_mut(bucket(value))
            .ok_or(HealthError::Overflow("histogram bucket"))?;
        *slot = slot.checked_add(1).ok_or(full.clone())?;
        self.total = self.total.checked_add(1).ok_or(full)?;
        Ok(())
    }

    /// The lower bound of the bucket holding the median, or `None` when empty.
    pub fn median(&self) -> Option<u64> {
        if self.total == 0 {
            return None;
        }
        // The lower median: the value at rank ceil(total / 2).
        let rank = self.total.div_ceil(2);
        let mut seen: u32 = 0;
        for (i, c) in self.counts.iter().enumerate() {
            seen = seen.saturating_add(*c);
            if seen >= rank {
                return Some(lower_bound(i));
            }
        }
        None
    }
}

fn bucket(value: u64) -> usize {
    if value < SUB as u64 {
        // 0..SUB map to themselves.
        return usize::try_from(value).unwrap_or(0);
    }
    let msb = 63u32.saturating_sub(value.leading_zeros());
    let shift = msb.saturating_sub(SUB_BITS);
    let sub = (value >> shift) & ((SUB as u64).saturating_sub(1));
    let index = u64::from(msb)
        .saturating_sub(u64::from(SUB_BITS))
        .saturating_add(1)
        .saturating_mul(SUB as u64)
        .saturating_add(sub);
    usize::try_from(index).unwrap_or(BUCKETS.saturating_sub(1))
}

fn lower_bound(index: usize) -> u64 {
    let i = u64::try_from(index).unwrap_or(u64::MAX);
    if i < SUB as u64 {
        return i;
    }
    let group = i >> SUB_BITS;
    let sub = i & ((SUB as u64).saturating_sub(1));
    let msb = group.saturating_add(u64::from(SUB_BITS)).saturating_sub(1);
    let shift = u32::try_from(msb.saturating_sub(u64::from(SUB_BITS))).unwrap_or(0);
    let lead = (SUB as u64).saturating_add(sub);
    lead.checked_shl(shift).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bucket of every value holds it, and its lower bound is within 25% below it.
    #[test]
    fn every_value_lands_in_a_bucket_within_a_quarter() {
        let mut v: u64 = 1;
        while v < u64::MAX / 3 {
            for x in [v, v + 1, v + v / 4, v + v / 2] {
                let b = bucket(x);
                assert!(b < BUCKETS, "{x}");
                let lo = lower_bound(b);
                assert!(lo <= x, "{x} {lo}");
                assert!(x - lo <= x / 4 + 1, "{x} {lo}");
            }
            v = v + v / 2 + 1;
        }
        assert!(bucket(u64::MAX) < BUCKETS);
        assert_eq!(bucket(0), 0);
    }

    /// The median of 1..=99 read from the histogram is within 25% of 50.
    #[test]
    fn the_median_is_within_a_quarter() {
        let mut h = Histogram::new();
        assert_eq!(h.median(), None);
        for x in 1..=99u64 {
            h.record(x * 1000).unwrap();
        }
        let m = h.median().unwrap();
        assert!((37_500..=50_000).contains(&m), "{m}");
        h.clear();
        assert_eq!(h.total(), 0);
    }
}
