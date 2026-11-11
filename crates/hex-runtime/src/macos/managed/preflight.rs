//! Metadata-only checks at the SAME ProcessBox/native-owner seam.
use hex_api::{Class, Execution, ImageArch, ImageOs};
use hex_core::record::{BoxRecord, NativeStage, OwnerRef, PoolRecord};

use super::*;

impl MacRuntime {
    pub fn preflight_record(&self, record: &BoxRecord) -> Result<(), Refusal> {
        record.spec.check()?;
        let bad = || NativeRecord::Box(record).refusal(LifecycleCause::Uncertain);
        if record.spec.class.class() != Class::Process
            || !record.spec.devices.is_empty()
            || record.native_scope.is_some()
        {
            return Err(bad());
        }
        if let Some(route) = record.spec.route
            && (route.key.execution != Execution::Native
                || route.key.image.os != ImageOs::Macos
                || route.key.image.arch != ImageArch::Arm64)
        {
            return Err(bad());
        }
        let boxes = self.boxes()?;
        if let Ok(at) = native_owner_index(&boxes, OwnerRef::Box(&record.spec.id)) {
            let boxed = boxes.get(at).map(|(_, boxed)| boxed).ok_or_else(bad)?;
            if boxed.scope().is_some() {
                return Err(bad());
            }
            if let Some(init) = record.init {
                let pid = i32::try_from(init.pid).map_err(|_| bad())?;
                if boxed.group()? != Some((pid, init.start_ticks)) {
                    return Err(bad());
                }
            }
        }
        Ok(())
    }

    pub fn preflight_managed(&self, record: NativeRecord<'_>) -> Result<(), Refusal> {
        let bad = || record.refusal(LifecycleCause::Uncertain);
        record.scope().ok_or_else(bad)?;
        if record.class()? != Class::Shards {
            return Err(bad());
        }
        let boxes = self.boxes()?;
        let Ok(at) = native_owner_index(&boxes, record.owner()) else {
            return if matches!(
                record.stage(),
                NativeStage::Prepared | NativeStage::Released
            ) {
                // No scope authority is synthesized here. The backend independently validates its
                // exact no-effect/Released receipt; no native cleanup is performed by preflight.
                Ok(())
            } else {
                // Cold PGID/controller observation supplies no persistent native cleanup owner.
                // Until the same helper receipt is available, retain uncertainty BEFORE siblings.
                Err(bad())
            };
        };
        let boxed = boxes.get(at).map(|(_, boxed)| boxed).ok_or_else(bad)?;
        let scope = boxed.scope().ok_or_else(bad)?;
        if scope.ownership != Ownership::Child || !scope.matches(record, record.commitment()?) {
            return Err(bad());
        }
        if let Some(init) = record.init()
            && boxed.identity.get().is_none_or(|identity| {
                identity.pid != init.pid || identity.started != init.start_ticks
            })
        {
            return Err(bad());
        }
        if matches!(
            *scope.lock().map_err(|_| bad())?,
            Phase::Uncertain(_) | Phase::Observing
        ) {
            return Err(bad());
        }
        Ok(())
    }

    pub fn preflight_retained(
        &self,
        records: &crate::devices::Records,
        pools: &[PoolRecord],
    ) -> Result<(), Refusal> {
        // Capture only keys/receipts already retained by this native owner; no second native table.
        // Never recursively take its mutex from a per-owner check.
        let boxes = self.boxes()?;
        for (owner, boxed) in boxes.iter() {
            let record = match owner.as_ref() {
                OwnerRef::Box(id) => NativeRecord::Box(
                    records
                        .get(id)
                        .and_then(|record| record.as_ref().ok())
                        .ok_or_else(|| uncertain(id))?,
                ),
                OwnerRef::Pool(pool) => {
                    let generation = *boxed
                        .scope()
                        .ok_or_else(|| invalid("a retained native pool has no managed scope"))?
                        .scope
                        .bytes();
                    let at = pools
                        .binary_search_by_key(pool, |record| record.key.pool)
                        .map_err(|_| Refusal::Backing {
                            key: hex_api::BackingKey {
                                pool: *pool,
                                generation,
                            },
                            cause: LifecycleCause::Uncertain,
                        })?;
                    NativeRecord::Pool(
                        pools
                            .get(at)
                            .ok_or_else(|| native_allocation(pools.len()))?,
                    )
                }
            };
            match boxed.scope() {
                Some(scope) if scope.matches(record, record.commitment()?) => {}
                None if matches!(record, NativeRecord::Box(record) if record.spec.class.class() == Class::Process && record.native_scope.is_none()) =>
                    {}
                _ => return Err(record.refusal(LifecycleCause::Uncertain)),
            }
        }
        Ok(())
    }
}

fn invalid(message: &'static str) -> Refusal {
    let mut why = String::new();
    if why.try_reserve_exact(message.len()).is_err() {
        return native_allocation(message.len());
    }
    why.push_str(message);
    Refusal::Invalid {
        what: "retained native metadata",
        why,
    }
}
