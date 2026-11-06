//! A node's own disk probe (`AC-T7.10.3`): the node agent writes and flushes a small record on its
//! state disk every period, and a stall is judged by the outstanding write's age, as hyper-raft trusts a
//! node only while its log device takes a write and flush within the period ("a node whose disk stalls
//! stops being trusted, as a crashed one does", PF[26]). The timeout is RFC 6298's retransmission
//! timeout over the probe's own completed latencies, as hyper-swim's detector times its peers
//! (`detector.rs`, `SRTT + max(G, 4·RTTVAR)`, at least a second).
//!
//! - Unarmed until the first probe, the calibration write, completes: its latency seeds the timer, so
//!   no prior is assumed, and nothing is judged before it (the node agent keeps the node out of
//!   placement until then).
//! - No ceiling: a disk is judged by its own latencies, however slow it steadily is.
//!
//! - Suspect when the outstanding write outlives one timeout.
//! - Drain when it outlives the timer's second expiry, the first one doubled (RFC 6298 §5.5): three
//!   timeouts in all.
//! - Drain also after [`SLOW_STREAK`] completed probes in a row each slower than the timeout: a disk
//!   that answers, but slowly, is failing slow.
//!
//! A probe that outlived its timeout is not a sample (Karn's algorithm, RFC 6298 §3), so a stall never
//! raises the baseline that judges it.

use crate::{HealthError, Level};

/// RFC 6298 §2.4: "if it is less than 1 second, then the RTO SHOULD be rounded up to 1 second".
pub const MIN_TIMEOUT_NS: u64 = 1_000_000_000;
/// The timer's expiries before a drain: the first at one timeout, the second after it doubled (RFC
/// 6298 §5.5), so three timeouts from the write.
pub const DRAIN_TIMEOUTS: u64 = 3;
/// Completed probes in a row slower than the timeout that make the disk fail-slow: CPI²'s "flagged
/// at least 3 times" before acting (PF[16]).
pub const SLOW_STREAK: u32 = 3;

#[derive(Debug, Clone)]
pub struct DiskProbe {
    /// The clock granularity G of RFC 6298 §2.
    granularity_ns: u64,
    srtt_ns: Option<u64>,
    rttvar_ns: u64,
    outstanding: Option<u64>,
    slow_streak: u32,
    last_ns: u64,
}

impl DiskProbe {
    pub fn new(granularity_ns: u64) -> Self {
        Self {
            granularity_ns,
            srtt_ns: None,
            rttvar_ns: 0,
            outstanding: None,
            slow_streak: 0,
            last_ns: 0,
        }
    }

    /// Whether the calibration write has completed: nothing is judged before.
    pub fn armed(&self) -> bool {
        self.srtt_ns.is_some()
    }

    /// The current timeout: RFC 6298 §2.3's `SRTT + max(G, K·RTTVAR)` with K = 4, at least
    /// [`MIN_TIMEOUT_NS`]; `None` until armed.
    pub fn timeout_ns(&self) -> Result<Option<u64>, HealthError> {
        let Some(srtt) = self.srtt_ns else {
            return Ok(None);
        };
        let spread = self
            .rttvar_ns
            .checked_mul(4)
            .ok_or(HealthError::Overflow("probe timeout"))?
            .max(self.granularity_ns);
        let rto = srtt
            .checked_add(spread)
            .ok_or(HealthError::Overflow("probe timeout"))?;
        Ok(Some(rto.max(MIN_TIMEOUT_NS)))
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

    /// The outstanding write and flush finished at `now_ns`; returns its latency.
    pub fn finish(&mut self, now_ns: u64) -> Result<u64, HealthError> {
        self.advance(now_ns)?;
        let began = self.outstanding.ok_or(HealthError::NoProbe)?;
        let latency = now_ns
            .checked_sub(began)
            .ok_or(HealthError::Overflow("probe latency"))?;
        let timeout = self.timeout_ns()?;
        self.outstanding = None;
        // Unarmed, this is the calibration write: always a sample.
        if timeout.is_some_and(|t| latency > t) {
            self.slow_streak = self.slow_streak.saturating_add(1);
            return Ok(latency);
        }
        self.slow_streak = 0;
        self.sample(latency)?;
        Ok(latency)
    }

    /// RFC 6298 §2.2 and §2.3, with α = 1/8 and β = 1/4.
    fn sample(&mut self, r: u64) -> Result<(), HealthError> {
        let overflow = HealthError::Overflow("probe estimator");
        match self.srtt_ns {
            None => {
                self.srtt_ns = Some(r);
                self.rttvar_ns = r / 2;
            }
            Some(srtt) => {
                let diff = srtt.abs_diff(r);
                self.rttvar_ns = self
                    .rttvar_ns
                    .checked_mul(3)
                    .and_then(|v| v.checked_add(diff))
                    .ok_or(overflow.clone())?
                    / 4;
                self.srtt_ns = Some(
                    srtt.checked_mul(7)
                        .and_then(|v| v.checked_add(r))
                        .ok_or(overflow)?
                        / 8,
                );
            }
        }
        Ok(())
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
        let Some(timeout) = self.timeout_ns()? else {
            return Ok(Level::Healthy);
        };
        let drain_at = timeout
            .checked_mul(DRAIN_TIMEOUTS)
            .ok_or(HealthError::Overflow("probe drain"))?;
        let age = match self.outstanding {
            Some(began) => now_ns
                .checked_sub(began)
                .ok_or(HealthError::Overflow("probe age"))?,
            None => 0,
        };
        Ok(if age > drain_at || self.slow_streak >= SLOW_STREAK {
            Level::Drain
        } else if age > timeout || self.slow_streak > 0 {
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
        let Some(timeout) = self.timeout_ns()? else {
            return Ok(None);
        };
        let overflow = HealthError::Overflow("probe deadline");
        let suspect = began.checked_add(timeout).ok_or(overflow.clone())?;
        let drain = timeout
            .checked_mul(DRAIN_TIMEOUTS)
            .and_then(|d| began.checked_add(d))
            .ok_or(overflow)?;
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

    fn warmed(latency: u64) -> DiskProbe {
        let mut p = DiskProbe::new(MS);
        let mut t = 0;
        for _ in 0..32 {
            p.start(t).unwrap();
            t += latency;
            p.finish(t).unwrap();
            t += 100 * MS;
        }
        p
    }

    /// AC-T7.10.3: a disk whose flushes take 2 ms, then stalls: suspect after one timeout (the 1 s
    /// floor), drained after three, healthy before.
    #[test]
    fn a_stalled_write_suspects_at_one_timeout_and_drains_at_three() {
        let mut p = warmed(2 * MS);
        assert_eq!(p.timeout_ns().unwrap(), Some(MIN_TIMEOUT_NS));
        let t0 = 10 * S;
        p.start(t0).unwrap();
        assert_eq!(p.level(t0 + 999 * MS).unwrap(), Level::Healthy);
        assert_eq!(p.level(t0 + S + 1).unwrap(), Level::Suspect);
        assert_eq!(p.level(t0 + 3 * S).unwrap(), Level::Suspect);
        assert_eq!(p.level(t0 + 3 * S + 1).unwrap(), Level::Drain);
    }

    /// AC-T7.10.3, Karn: a stall's latency doesn't raise the timeout that judges the next stall.
    #[test]
    fn a_stall_is_not_a_sample() {
        let mut p = warmed(2 * MS);
        let before = p.timeout_ns().unwrap();
        p.start(10 * S).unwrap();
        p.finish(40 * S).unwrap();
        assert_eq!(p.timeout_ns().unwrap(), before);
    }

    /// AC-T7.10.3 (fail-slow): three completed flushes in a row each slower than the timeout drain the
    /// node; a fast one in between forgives the streak.
    #[test]
    fn three_slow_flushes_in_a_row_drain() {
        let mut p = warmed(2 * MS);
        let mut t = 10 * S;
        for i in 0..SLOW_STREAK {
            p.start(t).unwrap();
            t += 2 * S;
            p.finish(t).unwrap();
            let want = if i + 1 == SLOW_STREAK {
                Level::Drain
            } else {
                Level::Suspect
            };
            assert_eq!(p.level(t).unwrap(), want);
            t += 100 * MS;
        }
        p.start(t).unwrap();
        t += 2 * MS;
        p.finish(t).unwrap();
        assert_eq!(p.level(t).unwrap(), Level::Healthy);
    }

    /// A slow but steady disk (300 ms flushes, a spinning disk under load) is its own baseline: no
    /// suspicion while it stays as it is.
    #[test]
    fn a_steady_slow_disk_is_its_own_baseline() {
        let mut p = warmed(300 * MS);
        let t0 = 100 * S;
        p.start(t0).unwrap();
        assert_eq!(p.level(t0 + 300 * MS).unwrap(), Level::Healthy);
        p.finish(t0 + 300 * MS).unwrap();
        assert_eq!(p.level(t0 + 300 * MS).unwrap(), Level::Healthy);
    }

    /// Hostile input: time going backwards, a second start, a finish with none outstanding, and a
    /// calibration write taking until the last instant are typed refusals, never a panic.
    #[test]
    fn hostile_times_are_refused_typed() {
        let mut p = DiskProbe::new(MS);
        p.start(5 * S).unwrap();
        assert_eq!(p.start(6 * S), Err(HealthError::ProbeOutstanding));
        assert!(matches!(
            p.finish(S),
            Err(HealthError::TimeBackwards { .. })
        ));
        assert!(matches!(p.level(S), Err(HealthError::TimeBackwards { .. })));
        // The calibration write took until the last instant: its sample is the timer's, and the
        // timeout it makes passes u64, a typed refusal.
        p.finish(u64::MAX).unwrap();
        assert_eq!(p.finish(u64::MAX), Err(HealthError::NoProbe));
        assert_eq!(
            p.level(u64::MAX),
            Err(HealthError::Overflow("probe timeout"))
        );
    }

    /// AC-T7.10.3, the calibration write (no prior constant): do start a probe and hold it for 1,000
    /// s before any probe has completed; expect detection off (`Healthy`, not armed, no wake-up asked);
    /// once it completes, expect the probe armed with a timeout from that one sample.
    #[test]
    fn detection_is_off_until_the_calibration_sample() {
        let mut p = DiskProbe::new(MS);
        assert!(!p.armed());
        assert_eq!(p.timeout_ns().unwrap(), None);
        p.start(0).unwrap();
        assert_eq!(p.level(1_000 * S).unwrap(), Level::Healthy);
        assert_eq!(p.next_change().unwrap(), None);
        p.finish(1_000 * S + 4 * MS).unwrap();
        assert!(p.armed());
        assert!(p.timeout_ns().unwrap().is_some());
    }

    /// No cap: a disk whose flushes steadily take 70 s is judged by its own latency, not a 60 s ceiling
    /// (RFC 6298's "MAY", copied from TCP), which would suspect every one of its healthy flushes: its
    /// timeout is past 60 s, and a 70 s flush is no suspicion.
    #[test]
    fn a_very_slow_disk_is_judged_by_its_own_latency() {
        let mut p = warmed(70 * S);
        let timeout = p.timeout_ns().unwrap().unwrap();
        assert!(timeout > 60 * S, "{timeout}");
        let t0 = 10_000 * S;
        p.start(t0).unwrap();
        assert_eq!(p.level(t0 + 70 * S).unwrap(), Level::Healthy);
    }

    /// The driver's wake-up: one past the suspect deadline, then one past the drain deadline.
    #[test]
    fn next_change_names_each_deadline() {
        let mut p = warmed(2 * MS);
        assert_eq!(p.next_change().unwrap(), None);
        p.start(10 * S).unwrap();
        assert_eq!(p.next_change().unwrap(), Some(11 * S + 1));
    }
}
