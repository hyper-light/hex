//! Finite aggregate CPU credit for the existing native watcher. The shell supplies the same
//! Allocation's rate, the kernel hardware ceiling, measured response and exact watcher CPU.
//! No period, sampler, admission row or device ownership is introduced here.
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Failure {
    Envelope,
    Arithmetic,
    Transition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Prepared,
    Running,
    Held,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Decision {
    Prepared,
    Run(Option<Duration>),
    Hold(Duration),
}

/// Enforcement credit, in milli-CPU nanoseconds. Positive credit is bounded; debt is never
/// forgiven by another observation, a late wake, or a change to learned reaction measurements.
#[derive(Debug)]
pub(super) struct Budget {
    rate: i128,
    maximum: i128,
    burst: i128,
    credit: i128,
    mode: Mode,
}

/// The same native watcher's monotonic credit clock. Whole-helper CPU is conservatively charged
/// to each active scope, including RPC and waiter work; shared work is never silently free.
#[derive(Debug)]
pub(super) struct Control {
    budget: Budget,
    clock: Option<(Instant, Duration)>,
    baseline: Duration,
}
impl Control {
    pub(super) fn new(
        rate: u64,
        maximum: u64,
        response: Duration,
        baseline: Duration,
    ) -> Result<Self, Failure> {
        Ok(Self {
            budget: Budget::new(rate, maximum, response)?,
            clock: None,
            baseline,
        })
    }
    pub(super) fn start(&mut self, now: Instant, helper_cpu: Duration) -> Result<(), Failure> {
        self.budget.start()?;
        // No uncharged startup interval: the first scope uses a zero baseline; siblings capture
        // the actual counter before their preparation. Previously charged shared work is not reset.
        self.budget.observe(
            Duration::ZERO,
            helper_cpu
                .checked_sub(self.baseline)
                .ok_or(Failure::Transition)?,
        )?;
        self.clock = Some((now, helper_cpu));
        Ok(())
    }
    pub(super) fn observe(
        &mut self,
        now: Instant,
        helper_cpu: Duration,
    ) -> Result<Decision, Failure> {
        if let Some((before, cpu)) = self.clock {
            self.budget.observe(
                now.checked_duration_since(before)
                    .ok_or(Failure::Transition)?,
                helper_cpu.checked_sub(cpu).ok_or(Failure::Transition)?,
            )?;
            self.clock = Some((now, helper_cpu));
        }
        self.budget.decision()
    }
    pub(super) fn held(&mut self) -> Result<(), Failure> {
        self.budget.held()
    }
    pub(super) fn permit(&mut self) -> Result<(), Failure> {
        match self.budget.mode {
            Mode::Prepared => Err(Failure::Transition),
            Mode::Held => self.budget.resume(),
            Mode::Running => Ok(()),
        }
    }
}

/// AC-T7.4.1: the cycle this control holds a scope that would use every core to, at `rate` milli-CPU of
/// `maximum` with `response`: how long it runs from full credit, then how long its hold lasts until it may
/// run again (burst/(maximum−rate), then 2·burst/rate, burst = maximum×response), taken from the budget's
/// own decisions. A scope at the maximum is never held: no cycle.
pub(super) fn cycle(
    rate: u64,
    maximum: u64,
    response: Duration,
) -> Result<(Duration, Duration), Failure> {
    let mut budget = Budget::new(rate, maximum, response)?;
    budget.start()?;
    let Decision::Run(Some(run)) = budget.decision()? else {
        return Err(Failure::Envelope);
    };
    budget.observe(run, Duration::ZERO)?;
    budget.held()?;
    let Decision::Hold(hold) = budget.decision()? else {
        return Err(Failure::Transition);
    };
    Ok((run, hold))
}

fn nanos(duration: Duration) -> Result<i128, Failure> {
    i128::try_from(duration.as_nanos()).map_err(|_| Failure::Arithmetic)
}
fn product(rate: i128, duration: Duration) -> Result<i128, Failure> {
    rate.checked_mul(nanos(duration)?)
        .ok_or(Failure::Arithmetic)
}
fn until(amount: i128, rate: i128) -> Result<Duration, Failure> {
    if amount <= 0 || rate <= 0 {
        return Err(Failure::Arithmetic);
    }
    let quotient = amount.checked_div(rate).ok_or(Failure::Arithmetic)?;
    let remainder = amount.checked_rem(rate).ok_or(Failure::Arithmetic)?;
    let ceil = if remainder == 0 {
        quotient
    } else {
        quotient.checked_add(1).ok_or(Failure::Arithmetic)?
    };
    let ns = u64::try_from(ceil).map_err(|_| Failure::Arithmetic)?;
    Ok(Duration::from_nanos(ns))
}

impl Budget {
    pub(super) fn new(rate: u64, maximum: u64, response: Duration) -> Result<Self, Failure> {
        if rate == 0 || maximum == 0 || rate > maximum || response.is_zero() {
            return Err(Failure::Envelope);
        }
        let rate = i128::from(rate);
        let maximum = i128::from(maximum);
        let burst = product(maximum, response)?;
        Ok(Self {
            rate,
            maximum,
            burst,
            credit: burst,
            mode: Mode::Prepared,
        })
    }

    /// Called before the first native CONT, not at a later first watcher observation.
    pub(super) fn start(&mut self) -> Result<(), Failure> {
        if self.mode != Mode::Prepared {
            return Err(Failure::Transition);
        }
        self.mode = Mode::Running;
        Ok(())
    }

    /// Running scope consumption is conservatively bounded by all hardware CPUs, covering short
    /// helpers even if they fork and are reaped between snapshots. The observer's own measured
    /// thread CPU is also debited; conservative overlap cannot create extra credit.
    pub(super) fn observe(
        &mut self,
        elapsed: Duration,
        watcher_cpu: Duration,
    ) -> Result<(), Failure> {
        if self.mode == Mode::Prepared {
            return Ok(());
        }
        let granted = product(self.rate, elapsed)?;
        let used = if self.mode == Mode::Running {
            product(self.maximum, elapsed)?
        } else {
            0
        };
        // One CPU is 1000 milli-CPU, the same unit as Allocation.resources.cpu_milli.
        let observer = product(1000, watcher_cpu)?;
        self.credit = self
            .credit
            .checked_add(granted)
            .and_then(|credit| credit.checked_sub(used))
            .and_then(|credit| credit.checked_sub(observer))
            .ok_or(Failure::Arithmetic)?
            .min(self.burst);
        Ok(())
    }

    /// Called after the existing scope hold has been issued successfully. The response allowance
    /// is paid once for this stop transition; accepted delivery alone is not immediate cessation.
    pub(super) fn held(&mut self) -> Result<(), Failure> {
        match self.mode {
            Mode::Prepared => return Err(Failure::Transition),
            Mode::Held => return Ok(()),
            Mode::Running => {}
        }
        self.credit = self
            .credit
            .checked_sub(self.burst)
            .ok_or(Failure::Arithmetic)?;
        self.mode = Mode::Held;
        Ok(())
    }

    /// Only full earned credit permits another pulse. Called before CONT/probing a held member.
    pub(super) fn resume(&mut self) -> Result<(), Failure> {
        if self.mode != Mode::Held || self.credit < self.burst {
            return Err(Failure::Transition);
        }
        self.mode = Mode::Running;
        Ok(())
    }

    pub(super) fn decision(&self) -> Result<Decision, Failure> {
        match self.mode {
            Mode::Prepared => Ok(Decision::Prepared),
            Mode::Held if self.credit < self.burst => Ok(Decision::Hold(until(
                self.burst
                    .checked_sub(self.credit)
                    .ok_or(Failure::Arithmetic)?,
                self.rate,
            )?)),
            Mode::Held | Mode::Running if self.credit > 0 => {
                let depletion = self
                    .maximum
                    .checked_sub(self.rate)
                    .ok_or(Failure::Arithmetic)?;
                let wait = if depletion == 0 {
                    None
                } else {
                    Some(until(self.credit, depletion)?)
                };
                Ok(Decision::Run(wait))
            }
            Mode::Running => Ok(Decision::Hold(until(
                self.burst
                    .checked_sub(self.credit)
                    .ok_or(Failure::Arithmetic)?,
                self.rate,
            )?)),
            Mode::Held => Err(Failure::Transition),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC-T7.4.1: actual helper startup cannot run free before the first VMM pulse. A sibling uses
    /// its measured pre-prepare baseline rather than charging an unrelated historical lifetime.
    #[test]
    fn helper_startup_debt_can_hold_the_initial_native_controller() {
        let at = Instant::now();
        let mut first = Control::new(1000, 4000, Duration::from_nanos(10), Duration::ZERO).unwrap();
        first.start(at, Duration::from_nanos(100)).unwrap();
        assert!(matches!(
            first.observe(at, Duration::from_nanos(100)).unwrap(),
            Decision::Hold(_)
        ));
        first.held().unwrap();
        assert!(first.permit().is_err());
        let mut sibling = Control::new(
            1000,
            4000,
            Duration::from_nanos(10),
            Duration::from_nanos(100),
        )
        .unwrap();
        sibling.start(at, Duration::from_nanos(101)).unwrap();
        assert!(matches!(
            sibling.observe(at, Duration::from_nanos(101)).unwrap(),
            Decision::Run(_)
        ));
        assert!(sibling.observe(at, Duration::from_nanos(100)).is_err());
    }

    /// AC-T7.4.1: a late watcher cannot forgive consumed aggregate CPU or enlarge its response credit.
    #[test]
    fn late_wake_keeps_all_debt_before_another_pulse() {
        let mut budget = Budget::new(1000, 4000, Duration::from_nanos(10)).unwrap();
        budget.start().unwrap();
        assert_eq!(
            budget.decision().unwrap(),
            Decision::Run(Some(Duration::from_nanos(14)))
        );
        budget
            .observe(Duration::from_nanos(20), Duration::ZERO)
            .unwrap();
        budget.held().unwrap();
        assert!(budget.resume().is_err());
        assert_eq!(
            budget.decision().unwrap(),
            Decision::Hold(Duration::from_nanos(100))
        );
        budget
            .observe(Duration::from_nanos(99), Duration::ZERO)
            .unwrap();
        assert!(budget.resume().is_err());
        budget
            .observe(Duration::from_nanos(1), Duration::ZERO)
            .unwrap();
        budget.resume().unwrap();
    }

    /// AC-T7.4.1: the watcher consumes the same admitted credit while the VM members are held.
    #[test]
    fn observer_cpu_is_not_free_or_an_independent_allowance() {
        let mut budget = Budget::new(1000, 4000, Duration::from_nanos(10)).unwrap();
        budget.start().unwrap();
        budget.held().unwrap();
        budget
            .observe(Duration::from_nanos(40), Duration::from_nanos(10))
            .unwrap();
        assert_eq!(
            budget.decision().unwrap(),
            Decision::Hold(Duration::from_nanos(10))
        );
        assert!(budget.resume().is_err());
        budget
            .observe(Duration::from_nanos(10), Duration::ZERO)
            .unwrap();
        budget.resume().unwrap();
    }

    /// AC-T7.4.1: idle time cannot bank an unbounded burst, and invalid control envelopes refuse.
    #[test]
    fn response_credit_is_fixed_and_invalid_rates_do_not_start() {
        assert!(Budget::new(0, 1000, Duration::from_nanos(1)).is_err());
        assert!(Budget::new(1001, 1000, Duration::from_nanos(1)).is_err());
        assert!(Budget::new(1000, 1000, Duration::ZERO).is_err());
        let mut budget = Budget::new(1000, 4000, Duration::from_nanos(10)).unwrap();
        assert_eq!(budget.decision().unwrap(), Decision::Prepared);
        budget.start().unwrap();
        assert!(budget.start().is_err());
        budget.held().unwrap();
        budget
            .observe(Duration::from_secs(3600), Duration::ZERO)
            .unwrap();
        budget.resume().unwrap();
        assert_eq!(
            budget.decision().unwrap(),
            Decision::Run(Some(Duration::from_nanos(14)))
        );
    }

    /// AC-T7.4.1 (guest_cost's macOS window): the cycle of a scope that would use every core, taken from the
    /// budget's own decisions: expect it to run burst/(maximum−rate) from full credit, rounded up, and its
    /// hold to pay that run's overshoot and refill a whole burst at the rate (2·burst/rate and the
    /// overshoot's), after which it may run again; a scope at the maximum is never held, so has none.
    #[test]
    fn the_cycle_is_the_budgets_run_then_its_hold() {
        let (run, hold) = cycle(1000, 4000, Duration::from_nanos(10)).unwrap();
        assert_eq!(
            (run, hold),
            (Duration::from_nanos(14), Duration::from_nanos(82))
        );
        let mut budget = Budget::new(1000, 4000, Duration::from_nanos(10)).unwrap();
        budget.start().unwrap();
        budget.observe(run, Duration::ZERO).unwrap();
        budget.held().unwrap();
        budget
            .observe(hold - Duration::from_nanos(1), Duration::ZERO)
            .unwrap();
        assert!(budget.resume().is_err());
        budget
            .observe(Duration::from_nanos(1), Duration::ZERO)
            .unwrap();
        budget.resume().unwrap();
        assert!(cycle(4000, 4000, Duration::from_nanos(10)).is_err());
    }

    /// AC-T7.4.1: two long-lived members plus a short helper consume one aggregate allocation.
    /// A late wake cannot reset their debt; held-time helper work delays every member's next pulse.
    #[test]
    fn aggregate_members_and_short_helpers_cannot_spend_independent_credit() {
        let at = Instant::now();
        let mut control =
            Control::new(1000, 4000, Duration::from_nanos(10), Duration::ZERO).unwrap();
        control.start(at, Duration::ZERO).unwrap();
        let members_cpu_ns: i128 = [20i128, 20, 10].into_iter().sum();
        let before = control.budget.credit;
        assert!(matches!(
            control
                .observe(at + Duration::from_nanos(20), Duration::ZERO)
                .unwrap(),
            Decision::Hold(_)
        ));
        let charged = before + 20 * 1000 - control.budget.credit;
        assert!(
            charged >= members_cpu_ns * 1000,
            "short helpers must not disappear from aggregate credit"
        );
        control.held().unwrap();
        assert!(control.permit().is_err());
        // A stale notification causes another observation, not a fresh budget/clock/identity.
        assert!(matches!(
            control
                .observe(at + Duration::from_nanos(20), Duration::ZERO)
                .unwrap(),
            Decision::Hold(_)
        ));
        assert!(matches!(
            control
                .observe(at + Duration::from_nanos(120), Duration::from_nanos(1))
                .unwrap(),
            Decision::Hold(_)
        ));
        assert!(control.permit().is_err());
        assert!(matches!(
            control
                .observe(at + Duration::from_nanos(121), Duration::from_nanos(1))
                .unwrap(),
            Decision::Run(_)
        ));
        control.permit().unwrap();
    }
}
