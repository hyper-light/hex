//! A node's own disk probe (`AC-T7.10.3`): the node agent writes and flushes a small record on its
//! state disk every period, and a stall is judged by the outstanding write's age, as hyper-raft trusts a
//! node only while its log device takes a write and flush within the period ("a node whose disk stalls
//! stops being trusted, as a crashed one does", PF[26]).
//!
//! Its wait is a learned one, by Hex's rule for learned waits (the lead, 2026-11-06):
//!
//! - Before any sample on this node, the slowest answer is the slowest in the recorded distribution
//!   for its host class, from a named record (the shell's to give): no calibration read, and no
//!   unbounded first wait.
//! - Then it learns: the slowest is the greater of the record's and this node's own slowest answer,
//!   and it only rises.
//! - `Suspect` (no new placements) once the outstanding write is slower than any answer seen; `Drain`
//!   (retire the node, its work moved) once it outlives [`BOUND_FACTOR`] times that, the bound.
//!
//! A write that outlived the bound before it answered is no answer: the wait had given up on it, and
//! taking it would let one stall raise the bound that judges the next (Karn's rule, RFC 6298 §3). A
//! disk that answers ever more slowly raises its own bound; that fail-slow case is the peer grade's
//! (F2), which judges a node against its peers, not against its own history.

use crate::{HealthError, Level};

/// The bound is this many times the slowest answer: Hex's rule for learned waits (the lead,
/// 2026-11-06).
pub const BOUND_FACTOR: u64 = 2;

#[derive(Debug, Clone)]
pub struct DiskProbe {
    /// The slowest answer this probe's wait has seen or been seeded with, nanoseconds.
    slowest_ns: u64,
    outstanding: Option<u64>,
    last_ns: u64,
}

impl DiskProbe {
    /// A probe seeded with the slowest answer in its host class's recorded distribution. A seed of
    /// zero is no record: refused.
    pub fn new(recorded_slowest_ns: u64) -> Result<Self, HealthError> {
        if recorded_slowest_ns == 0 {
            return Err(HealthError::Config(
                "a disk probe needs its host class's recorded slowest answer",
            ));
        }
        Ok(Self {
            slowest_ns: recorded_slowest_ns,
            outstanding: None,
            last_ns: 0,
        })
    }

    /// The slowest answer seen, the record's included: past it, `Suspect`.
    pub fn slowest_ns(&self) -> u64 {
        self.slowest_ns
    }

    /// The bound: [`BOUND_FACTOR`] times the slowest answer; past it, `Drain`.
    pub fn bound_ns(&self) -> Result<u64, HealthError> {
        self.slowest_ns
            .checked_mul(BOUND_FACTOR)
            .ok_or(HealthError::Overflow("probe bound"))
    }

    /// A write and flush began at `now_ns`.
    pub fn start(&mut self, now_ns: u64) -> Result<(), HealthError> {
        self.advance(now_ns)?;
        if self.outstanding.is_some() {
            return Err(HealthError::ProbeOutstanding);
        }
        self.outstanding = Some(now_ns);
        Ok(())
    }

    /// The outstanding write and flush finished at `now_ns`; returns its latency. Within the bound it
    /// is an answer, and raises the slowest if it is slower.
    pub fn finish(&mut self, now_ns: u64) -> Result<u64, HealthError> {
        self.advance(now_ns)?;
        let began = self.outstanding.ok_or(HealthError::NoProbe)?;
        let latency = now_ns
            .checked_sub(began)
            .ok_or(HealthError::Overflow("probe latency"))?;
        let bound = self.bound_ns()?;
        self.outstanding = None;
        if latency <= bound {
            self.slowest_ns = self.slowest_ns.max(latency);
        }
        Ok(latency)
    }

    /// The node's grade at `now_ns`.
    pub fn level(&self, now_ns: u64) -> Result<Level, HealthError> {
        if now_ns < self.last_ns {
            return Err(HealthError::TimeBackwards {
                what: "disk probe",
                now_ns,
                last_ns: self.last_ns,
            });
        }
        let age = match self.outstanding {
            Some(began) => now_ns
                .checked_sub(began)
                .ok_or(HealthError::Overflow("probe age"))?,
            None => 0,
        };
        Ok(if age > self.bound_ns()? {
            Level::Drain
        } else if age > self.slowest_ns {
            Level::Suspect
        } else {
            Level::Healthy
        })
    }

    /// When the outstanding write, if it stays outstanding, next changes the grade: the instant a
    /// driver should look again.
    pub fn next_change(&self) -> Result<Option<u64>, HealthError> {
        let Some(began) = self.outstanding else {
            return Ok(None);
        };
        let overflow = HealthError::Overflow("probe deadline");
        let suspect = began.checked_add(self.slowest_ns).ok_or(overflow.clone())?;
        let drain = began.checked_add(self.bound_ns()?).ok_or(overflow)?;
        Ok(Some(if self.last_ns <= suspect {
            suspect.saturating_add(1)
        } else {
            drain.saturating_add(1)
        }))
    }

    fn advance(&mut self, now_ns: u64) -> Result<(), HealthError> {
        if now_ns < self.last_ns {
            return Err(HealthError::TimeBackwards {
                what: "disk probe",
                now_ns,
                last_ns: self.last_ns,
            });
        }
        self.last_ns = now_ns;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;
    const S: u64 = 1_000_000_000;
    /// The macOS/APFS record's slowest answer (`bench/health/records/2026-11-06-probe-floor-apfs.md`).
    const RECORD: u64 = 147_330 * 1_000;

    /// A probe that has answered `n` writes of `latency` each, 100 ms apart.
    fn answered(latency: u64, n: u32) -> (DiskProbe, u64) {
        let mut p = DiskProbe::new(RECORD).unwrap();
        let mut t = 0;
        for _ in 0..n {
            p.start(t).unwrap();
            t += latency;
            p.finish(t).unwrap();
            t += 100 * MS;
        }
        (p, t)
    }

    /// AC-T7.10.3, before any sample: do stall the very first write; expect `Suspect` past the
    /// record's slowest and `Drain` past twice it: armed from the start, bounded from the start.
    #[test]
    fn the_first_wait_is_bounded_by_the_record() {
        let mut p = DiskProbe::new(RECORD).unwrap();
        assert_eq!(p.bound_ns().unwrap(), 2 * RECORD);
        p.start(0).unwrap();
        assert_eq!(p.level(RECORD).unwrap(), Level::Healthy);
        assert_eq!(p.level(RECORD + 1).unwrap(), Level::Suspect);
        assert_eq!(p.level(2 * RECORD).unwrap(), Level::Suspect);
        assert_eq!(p.level(2 * RECORD + 1).unwrap(), Level::Drain);
    }

    /// AC-T7.10.3, it learns and only rises: do answer a write slower than the record (within the
    /// bound); expect the bound twice that answer; then faster answers; expect it not to fall.
    #[test]
    fn the_bound_rises_with_the_slowest_answer_and_never_falls() {
        let (mut p, mut t) = answered(4 * MS, 10);
        assert_eq!(p.bound_ns().unwrap(), 2 * RECORD);
        let slow = RECORD + 50 * MS;
        p.start(t).unwrap();
        t += slow;
        p.finish(t).unwrap();
        assert_eq!(p.bound_ns().unwrap(), 2 * slow);
        for _ in 0..100 {
            p.start(t).unwrap();
            t += MS;
            p.finish(t).unwrap();
        }
        assert_eq!(p.bound_ns().unwrap(), 2 * slow);
    }

    /// AC-T7.10.3, Karn: a write that answers only after outliving the bound (a stall that ended)
    /// doesn't raise the bound that judges the next stall.
    #[test]
    fn a_stall_that_ends_is_no_answer() {
        let (mut p, t) = answered(4 * MS, 10);
        let before = p.bound_ns().unwrap();
        p.start(t).unwrap();
        p.finish(t + 60 * S).unwrap();
        assert_eq!(p.bound_ns().unwrap(), before);
    }

    /// A steady disk is healthy at its own pace: answers at the record's slowest are no suspicion.
    #[test]
    fn a_disk_at_its_recorded_pace_is_healthy() {
        let (mut p, t) = answered(RECORD, 20);
        p.start(t).unwrap();
        assert_eq!(p.level(t + RECORD).unwrap(), Level::Healthy);
    }

    /// Hostile input: no record, time going backwards, a second start, a finish with none
    /// outstanding, and a seed whose bound passes u64 are typed refusals, never a panic.
    #[test]
    fn hostile_input_is_refused_typed() {
        assert!(matches!(DiskProbe::new(0), Err(HealthError::Config(_))));
        let mut p = DiskProbe::new(RECORD).unwrap();
        p.start(5 * S).unwrap();
        assert_eq!(p.start(6 * S), Err(HealthError::ProbeOutstanding));
        assert!(matches!(
            p.finish(S),
            Err(HealthError::TimeBackwards { .. })
        ));
        assert!(matches!(p.level(S), Err(HealthError::TimeBackwards { .. })));
        p.finish(6 * S).unwrap();
        assert_eq!(p.finish(7 * S), Err(HealthError::NoProbe));
        let huge = DiskProbe::new(u64::MAX).unwrap();
        assert_eq!(huge.bound_ns(), Err(HealthError::Overflow("probe bound")));
    }

    /// The driver's wake-up: one past the suspect deadline, then one past the drain deadline.
    #[test]
    fn next_change_names_each_deadline() {
        let (mut p, t) = answered(4 * MS, 10);
        assert_eq!(p.next_change().unwrap(), None);
        p.start(t).unwrap();
        assert_eq!(p.next_change().unwrap(), Some(t + RECORD + 1));
    }
}
