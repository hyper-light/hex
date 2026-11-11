//! The borrowed canonical transaction is the native launcher. The API owns only
//! its existing protocol/drain workers; the native receipt owns scope and reaping.

use std::cell::RefCell;

use super::*;
use crate::devices::Transaction;

struct Launcher<'a> {
    transaction: &'a dyn Transaction,
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
        self.remember(self.transaction.launch_managed(plan, io))
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
    operation: impl FnOnce(&mut dyn shards_api::ManagedLauncher) -> Result<T, shards_api::Refusal>,
) -> Result<T, Refusal> {
    let mut launcher = Launcher {
        transaction,
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
) -> Result<shards_api::ManagedGuest, Refusal> {
    launch(transaction, |launcher| {
        shards_api::boot_managed(plan, launcher)
    })
}

pub(super) fn attach(
    transaction: &dyn Transaction,
    plan: shards_api::PreparedBoot,
    identity: &shards_api::Identity,
) -> Result<shards_api::ManagedGuest, Refusal> {
    launch(transaction, |launcher| {
        shards_api::attach_managed(plan, identity, launcher)
    })
}

pub(super) fn same_init(
    record: &hex_core::record::BoxRecord,
    identity: &shards_api::Identity,
) -> Result<(), Refusal> {
    if identity.pid == 0
        || identity.started == 0
        || record.init
            != Some(hex_core::record::ProcId {
                pid: identity.pid,
                start_ticks: identity.started,
            })
    {
        return Err(Refusal::Lifecycle {
            id: record.spec.id.try_clone()?,
            cause: hex_api::LifecycleCause::Uncertain,
        });
    }
    Ok(())
}

/// A missing controller never prevents cleanup of its already-owned scope.
/// Only Released, including every helper, permits canonical record removal.
pub(super) fn stop(
    transaction: &dyn Transaction,
    id: &BoxId,
    identity: Option<&shards_api::Identity>,
    patience: Duration,
) -> Result<shards_api::ManagedEnd, Refusal> {
    let process = transaction.attach_managed(identity)?;
    let stopped = process.stop();
    let observed = process
        .wait_for(patience)
        .map_err(|why| refusal("observing complete Shards scope termination", &why))?;
    if let Some(end) = observed.filter(|end| end.scope == shards_api::TerminalScope::Released) {
        return Ok(end);
    }
    if let Err(why) = stopped {
        return Err(refusal("stopping managed Shards scope", &why));
    }
    Err(Refusal::Lifecycle {
        id: id.try_clone()?,
        cause: hex_api::LifecycleCause::Uncertain,
    })
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
}
