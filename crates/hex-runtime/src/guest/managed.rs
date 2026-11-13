//! The borrowed canonical transaction is the native launcher. The API owns only
//! its existing protocol/drain workers; the native receipt owns scope and reaping.

use std::cell::RefCell;

use super::*;
use crate::devices::{ManagedLaunch, Transaction};
use hex_core::patience::Patience;

/// One create/attach attempt spends one existing measured boot bound. Native
/// launch and the first relay dial consume its remainder rather than restarting it.
#[derive(Clone, Copy)]
pub(super) struct Budget {
    deadline: Instant,
}

impl Budget {
    pub fn new(patience: Duration) -> Result<Self, Refusal> {
        if patience.is_zero() {
            return Err(Refusal::Invalid {
                what: "managed guest budget",
                why: "the measured boot bound must be finite and positive".into(),
            });
        }
        Ok(Self {
            deadline: Instant::now()
                .checked_add(patience)
                .ok_or_else(|| Refusal::Invalid {
                    what: "managed guest budget",
                    why: "the measured boot bound exceeds a finite native deadline".into(),
                })?,
        })
    }

    pub fn remaining(self) -> Result<Duration, Refusal> {
        self.remaining_at(Instant::now())
    }

    pub fn deadline(self) -> Instant {
        self.deadline
    }

    fn remaining_at(self, now: Instant) -> Result<Duration, Refusal> {
        self.deadline
            .checked_duration_since(now)
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| {
                Refusal::os(
                    "spending the original managed guest create budget",
                    std::io::ErrorKind::TimedOut,
                )
            })
    }
}

struct Launcher<'a> {
    transaction: &'a dyn Transaction,
    budget: Budget,
    refused: RefCell<Option<Refusal>>,
}

impl Launcher<'_> {
    fn remember<T>(&self, result: Result<T, Refusal>) -> Result<T, shards_api::Refusal> {
        result.map_err(|why| {
            match self.refused.try_borrow_mut() {
                Ok(mut refused) => *refused = Some(why),
                Err(_) => return shards_api::Refusal::Launch(shards_api::LaunchError::Observer),
            }
            shards_api::Refusal::Launch(shards_api::LaunchError::NativeRefused)
        })
    }
}

impl shards_api::ManagedLauncher for Launcher<'_> {
    fn launch(
        &mut self,
        plan: &shards_api::PreparedBoot,
        io: shards_api::LaunchIo,
    ) -> Result<shards_api::ManagedReceipt, shards_api::Refusal> {
        self.remember(self.budget.remaining().and_then(|remaining| {
            self.transaction
                .launch_managed(&ManagedLaunch::Shards(plan), io, remaining)
        }))
    }

    fn attach(
        &mut self,
        identity: &shards_api::Identity,
    ) -> Result<shards_api::ManagedReceipt, shards_api::Refusal> {
        self.remember(self.transaction.attach_managed(Some(identity)))
    }
}

fn launch<T>(
    transaction: &dyn Transaction,
    budget: Budget,
    operation: impl FnOnce(&mut dyn shards_api::ManagedLauncher) -> Result<T, shards_api::Refusal>,
) -> Result<T, Refusal> {
    let mut launcher = Launcher {
        transaction,
        budget,
        refused: RefCell::new(None),
    };
    let result = operation(&mut launcher);
    if let Some(why) = launcher.refused.into_inner() {
        return Err(why);
    }
    result.map_err(|why| refusal("managed Shards native scope", &why))
}

pub(super) fn boot(
    transaction: &dyn Transaction,
    plan: shards_api::PreparedBoot,
    budget: Budget,
) -> Result<shards_api::ManagedGuest, Refusal> {
    budget.remaining()?;
    launch(transaction, budget, |launcher| {
        shards_api::boot_managed(plan, launcher)
    })
}

pub(super) fn attach(
    transaction: &dyn Transaction,
    plan: shards_api::PreparedBoot,
    identity: &shards_api::Identity,
    budget: Budget,
) -> Result<shards_api::ManagedGuest, Refusal> {
    budget.remaining()?;
    launch(transaction, budget, |launcher| {
        shards_api::attach_managed(plan, identity, launcher)
    })
}

pub(super) fn same_init(
    record: &hex_core::record::BoxRecord,
    identity: &shards_api::Identity,
) -> Result<(), Refusal> {
    same_native(hex_core::record::NativeRecord::Box(record), identity)
}

pub(super) fn same_native(
    record: hex_core::record::NativeRecord<'_>,
    identity: &shards_api::Identity,
) -> Result<(), Refusal> {
    if record.scope().is_none()
        || identity.pid == 0
        || identity.started == 0
        || record.init()
            != Some(hex_core::record::ProcId {
                pid: identity.pid,
                start_ticks: identity.started,
            })
    {
        return Err(record.uncertain("its published controller isn't its record's"));
    }
    Ok(())
}

/// A managed scope's stop (decision 9; ROOT, 2026-11-13): from its stop sent until its scope's end is seen, a wait of
/// its own, measuring, never a boot's record. The slowest stop this process has seen bounds it. Before its first, and
/// from a run-out (told) on, it follows the scope by the scope's own party: `ManagedProcess::wait_for` with
/// `Duration::MAX` lasts until the scope's end, or refuses once its observer fails (on macOS the helper that keeps the
/// scope, or the scope's watcher in it), so no wait is given up on a guess while the party that ends it lives.
static STOP: Patience = Patience::measuring("managed.stop");

/// A missing controller never prevents cleanup of its already-owned scope.
/// Only Released, including every helper, permits canonical record removal.
pub(super) fn stop(
    transaction: &dyn Transaction,
    id: &BoxId,
    identity: Option<&shards_api::Identity>,
) -> Result<shards_api::ManagedEnd, Refusal> {
    stop_scope(
        transaction,
        identity,
        super::books::uncertain(
            id,
            "its managed scope's stop ended with its scope unreleased",
        )?,
    )
}

/// Stops the managed scope `identity` names (or the one with none published yet) and waits for its end ([`STOP`]): its
/// end once Released. A stop that wasn't sent follows nothing (nothing would end the scope it waits on): the scope as
/// it is then, or the stop's refusal.
pub(super) fn stop_scope(
    transaction: &dyn Transaction,
    identity: Option<&shards_api::Identity>,
    uncertain: Refusal,
) -> Result<shards_api::ManagedEnd, Refusal> {
    let process = transaction.attach_managed(identity)?;
    let before = process.terminal();
    if let Ok(Some(end)) = &before
        && end.scope == shards_api::TerminalScope::Released
    {
        return Ok(*end);
    }
    let began = Instant::now();
    if let Err(why) = process.stop() {
        return match process.terminal() {
            Ok(Some(end)) if end.scope == shards_api::TerminalScope::Released => Ok(end),
            _ => Err(refusal("stopping managed Shards scope", &why)),
        };
    }
    let wait = |patience| {
        process
            .wait_for(patience)
            .map_err(|why| refusal("observing complete Shards scope termination", &why))
    };
    let observed = match STOP.deadline() {
        Some(bound) => match wait(bound)? {
            Some(end) => Some(end),
            None => {
                STOP.ran_out(bound);
                wait(Duration::MAX)?
            }
        },
        None => wait(Duration::MAX)?,
    };
    if let Some(end) = observed.filter(|end| end.scope == shards_api::TerminalScope::Released) {
        STOP.answered(began.elapsed());
        return Ok(end);
    }
    if let Err(why) = before {
        return Err(refusal("observing original managed native scope", &why));
    }
    Err(uncertain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_core::record::{BoxRecord, Phase, ProcId};

    /// AC-T7.4.1: a VM becomes usable only after the native launcher's
    /// before-resume publication matches its actual PID and birth exactly.
    #[test]
    fn canonical_native_publication_must_match_the_managed_controller() {
        let spec = super::super::tests::oci(hex_api::Rootfs::Dir("/rootfs".into()), Vec::new());
        let mut record = BoxRecord::new(spec, Phase::Creating, Vec::new());
        record.native_scope = Some(hex_core::record::NativeScope::from_bytes([1; 16]));
        let identity = shards_api::Identity {
            pid: 42,
            started: 7,
            vsock: "/scope/v".into(),
        };
        assert!(same_init(&record, &identity).is_err());
        record.init = Some(ProcId {
            pid: 42,
            start_ticks: 7,
        });
        same_init(&record, &identity).unwrap();
        record.init = Some(ProcId {
            pid: 42,
            start_ticks: 8,
        });
        assert!(same_init(&record, &identity).is_err());
    }

    /// AC-T7.4.1: no stage restarts a create budget after scope publication.
    #[test]
    fn managed_budget_is_one_absolute_finite_deadline() {
        assert!(Budget::new(Duration::ZERO).is_err());
        assert!(Budget::new(Duration::MAX).is_err());
        let start = Instant::now();
        let deadline = start.checked_add(Duration::from_secs(1)).unwrap();
        let budget = Budget { deadline };
        assert_eq!(budget.remaining_at(start).unwrap(), Duration::from_secs(1));
        assert!(budget.remaining_at(deadline).is_err());
        assert!(
            budget
                .remaining_at(deadline.checked_add(Duration::from_nanos(1)).unwrap())
                .is_err()
        );
    }
}
