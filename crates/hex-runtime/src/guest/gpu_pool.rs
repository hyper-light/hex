//! The already-admitted GPU observer VM: canonical PoolRecord holds the one
//! grant; the existing route table retains only its actual native/client owner.

use hex_api::{
    BackingGrant, BackingKey, BackingOffer, BackingPhase, LifecycleCause, Refusal, Usage,
};
use hex_core::record::{NativeRecord, OwnerRef, PoolRecord};

use super::dedicated::{Guest, Operation};
use super::pool::{self, Receipt, refused};
use super::selected::Phase;
use super::shared::{Binding, GpuBacking};
use super::{Booted, GuestRuntime, managed, observer, refusal};
use crate::devices::{self, Transaction};

impl GuestRuntime {
    /// Service calls this only AFTER its SAME Index observes this exact old
    /// Released generation. No retained Box may reference it during rearm.
    pub(super) fn acknowledge_gpu_release(&self, key: BackingKey) -> Result<BackingOffer, Refusal> {
        let definition = self.gpu_definition_for(key)?;
        let books = self
            .books
            .get()
            .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
        let keys = self.turn_keys(&definition.devices)?;
        let mut reservation = self.reserve_owner(OwnerRef::Pool(&key.pool), Operation::Stopping)?;
        let offer = devices::with_pool_transaction(books, &key, &keys, |transaction| {
            let record = transaction
                .read_pool(&key)?
                .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
            self.preflight_backing(&record)?;
            if record.phase != BackingPhase::Released {
                return Err(refused(key, LifecycleCause::Pending));
            }
            for boxed in transaction.records()?.values() {
                let boxed = boxed
                    .as_ref()
                    .map_err(|_| refused(key, LifecycleCause::Uncertain))?;
                if boxed
                    .spec
                    .route
                    .is_some_and(|route| route.backing == Some(key))
                {
                    return Err(refused(key, LifecycleCause::Busy));
                }
            }
            let offer = record.offer()?;
            if let Some(receipt) = pool::read(&definition.path)? {
                receipt.matches(&record)?;
                if !matches!(receipt.phase, Phase::Released { .. }) {
                    return Err(refused(key, LifecycleCause::Uncertain));
                }
                transaction.finalize_managed(receipt.released_identity()?)?;
                super::held::remove(&definition.path)?;
            }
            transaction.remove_pool(&key)?;
            Ok(offer)
        })?;
        reservation.previous.take();
        reservation.publish(None)?;
        // Fresh Prepared generation is created by the existing Store authority,
        // never before Index acknowledgement or a remaining Box reference.
        self.authority()?.prepare_pool(
            definition.pool,
            hex_api::Execution::ShardsDedicated,
            definition.cost,
            &definition.devices,
        )?;
        Ok(offer)
    }

    pub(super) fn stop_gpu_for_handoff(
        &self,
        key: BackingKey,
        boxed: &hex_core::record::BoxRecord,
        budget: managed::Budget,
    ) -> Result<BackingOffer, Refusal> {
        let definition = self.gpu_definition_for(key)?;
        if boxed.spec.route.and_then(|route| route.backing) != Some(key)
            || boxed.spec.devices != definition.devices
            || boxed.phase != hex_core::record::Phase::Creating
            || boxed.init.is_some()
            || boxed.native_scope.is_none()
        {
            return Err(refused(key, LifecycleCause::Uncertain));
        }
        let keys = self.turn_keys(&definition.devices)?;
        let books = self
            .books
            .get()
            .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
        let mut reservation = self.reserve_owner(OwnerRef::Pool(&key.pool), Operation::Stopping)?;
        let offer = devices::with_pool_transaction(books, &key, &keys, |transaction| {
            let mut record = transaction
                .read_pool(&key)?
                .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
            self.preflight_backing(&record)?;
            let current = transaction
                .read(&boxed.spec.id)?
                .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
            if current.spec != boxed.spec
                || current.native_scope != boxed.native_scope
                || current.init.is_some()
            {
                return Err(refused(key, LifecycleCause::Uncertain));
            }
            let mut receipt = pool::read(&definition.path)?
                .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
            receipt.matches(&record)?;
            if record.phase == BackingPhase::Released {
                return record.offer();
            }
            if record.phase != BackingPhase::Live && record.phase != BackingPhase::Draining {
                return Err(refused(key, LifecycleCause::Uncertain));
            }
            let identity = super::shared::copy_identity(receipt.identity()?)?;
            managed::same_native(NativeRecord::Pool(&record), &identity)?;
            record.phase = BackingPhase::Draining;
            transaction.put_pool(&record)?;
            receipt.phase = Phase::Stopping(super::shared::copy_identity(&identity)?);
            pool::write(&definition.path, &receipt)?;
            let terminal = managed::stop_scope(
                transaction,
                Some(&identity),
                budget.remaining()?,
                refused(key, LifecycleCause::Uncertain),
            )?;
            receipt.phase = Phase::Released {
                identity: Some(identity),
                status: terminal.status,
            };
            pool::write(&definition.path, &receipt)?;
            transaction.finalize_managed(receipt.released_identity()?)?;
            record.exit = Some(super::shared::exit_of(terminal.status));
            record.phase = BackingPhase::Released;
            transaction.put_pool(&record)?;
            record.offer()
        })?;
        reservation.previous.take();
        reservation.publish(None)?;
        Ok(offer)
    }

    pub(super) fn initialize_gpu_pool(&self, grant: BackingGrant) -> Result<BackingOffer, Refusal> {
        grant.check()?;
        // This one finite absolute budget is captured before grant persistence,
        // native effects, report observation, guest workload or publication.
        let budget = managed::Budget::new(self.first_dial_bound()?)?;
        let definition = self.gpu_definition_for(grant.key)?;
        if grant.devices != definition.devices
            || grant.allocation != self.configured_allocation()?
        {
            return Err(refused(grant.key, LifecycleCause::Uncertain));
        }
        let observer = self
            .gpu_observer
            .as_ref()
            .ok_or_else(|| refused(grant.key, LifecycleCause::Pending))?;
        observer.verify(&self.config)?;
        budget.remaining()?;
        let keys = self.turn_keys(&definition.devices)?;
        let books = self
            .books
            .get()
            .ok_or_else(|| refused(grant.key, LifecycleCause::Uncertain))?;
        let mut reservation =
            self.reserve_owner(OwnerRef::Pool(&grant.key.pool), Operation::Creating)?;
        devices::with_pool_transaction(books, &grant.key, &keys, |transaction| {
            let mut record = transaction.grant_pool(&grant)?;
            self.preflight_backing(&record)?;
            let booted = match reservation.previous.as_ref() {
                Some(known) => {
                    self.matches_pool(&known.booted, &record)?;
                    let identity = known.booted.guest.identity().map_err(|why| {
                        refusal("checking the same GPU backing publication", &why)
                    })?;
                    managed::same_native(NativeRecord::Pool(&record), &identity)?;
                    self.pool_alive(&known.booted, record.key)?;
                    let receipt = pool::read(&definition.path)?
                        .ok_or_else(|| refused(record.key, LifecycleCause::Uncertain))?;
                    receipt.matches(&record)?;
                    receipt.checked_report()?;
                    self.check_pool_native_report(&receipt)?;
                    receipt.checked_readiness()?;
                    triomphe::Arc::clone(&known.booted)
                }
                None => self.boot_gpu_pool(transaction, &definition, record, budget)?,
            };
            record = transaction
                .read_pool(&grant.key)?
                .ok_or_else(|| refused(grant.key, LifecycleCause::Uncertain))?;
            self.matches_pool(&booted, &record)?;
            let receipt = pool::read(&definition.path)?
                .ok_or_else(|| refused(grant.key, LifecycleCause::Uncertain))?;
            receipt.matches(&record)?;
            receipt.checked_readiness()?;
            self.pool_alive(&booted, grant.key)?;
            budget.remaining()?;
            if record.phase == BackingPhase::Granted {
                record.phase = BackingPhase::Live;
                transaction.put_pool(&record)?;
            }
            if record.phase != BackingPhase::Live {
                return Err(refused(grant.key, LifecycleCause::Uncertain));
            }
            let offer = record.offer()?;
            reservation.previous = Some(Guest { booted });
            Ok(offer)
        })
        .and_then(|offer| {
            let known = reservation.previous.take();
            reservation.publish(known)?;
            Ok(offer)
        })
    }

    fn gpu_definition_for(&self, key: BackingKey) -> Result<GpuBacking, Refusal> {
        self.gpu_backing_definition()?
            .filter(|definition| definition.pool == key.pool)
            .ok_or_else(|| refused(key, LifecycleCause::Uncertain))
    }

    fn boot_gpu_pool(
        &self,
        transaction: &dyn Transaction,
        definition: &GpuBacking,
        record: PoolRecord,
        budget: managed::Budget,
    ) -> Result<triomphe::Arc<Booted>, Refusal> {
        let key = record.key;
        let retained = pool::read(&definition.path)?;
        let fresh = retained.is_none();
        let mut receipt = match retained {
            Some(receipt) => {
                receipt.matches(&record)?;
                receipt
            }
            None if record.phase == BackingPhase::Granted && record.init.is_none() => {
                Receipt::intent(
                    record,
                    self.catalog_physical(&definition.devices)?,
                    &self.config,
                )?
            }
            None => return Err(refused(key, LifecycleCause::Uncertain)),
        };
        let binding = Binding {
            key,
            commitment: receipt.original.native_commitment()?,
        };
        let mut spec = self.pool_spec(&receipt)?;
        spec.gpu = true;
        spec.gpu_devices
            .try_reserve_exact(receipt.physical.len())
            .map_err(|_| Refusal::Full(hex_api::MAX_DEVICES))?;
        spec.gpu_devices.extend_from_slice(&receipt.physical);
        let socket = self.pool_socket(key)?;
        let plan = self.managed_plan(&spec, &socket)?;
        let guest = match &receipt.phase {
            Phase::Intent if fresh => {
                let plan = plan
                    .with_gpu_report(*receipt.original.scope().bytes(), budget.deadline())
                    .map_err(|why| refusal("preparing charged GPU observer report", &why))?;
                pool::write(&definition.path, &receipt)?;
                managed::boot(transaction, plan, budget)?
            }
            Phase::Running(identity) => {
                receipt.checked_report()?;
                self.check_pool_native_report(&receipt)?;
                receipt.checked_readiness()?;
                let canonical = transaction
                    .read_pool(&key)?
                    .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
                managed::same_native(NativeRecord::Pool(&canonical), identity)?;
                if identity.vsock != socket {
                    return Err(refused(key, LifecycleCause::Uncertain));
                }
                managed::attach(transaction, plan, identity, budget)?
            }
            // An interrupted Intent/report/workload is uncertainty, never a
            // second VM/program launch or an automatic grant refund.
            _ => return Err(refused(key, LifecycleCause::Uncertain)),
        };
        let identity = shards_api::Guest::identity(&guest)
            .map_err(|why| refusal("reading charged GPU observer identity", &why))?;
        let canonical = transaction
            .read_pool(&key)?
            .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
        receipt.matches(&canonical)?;
        managed::same_native(NativeRecord::Pool(&canonical), &identity)?;
        if identity.vsock != socket {
            return Err(refused(key, LifecycleCause::Uncertain));
        }
        if fresh {
            receipt.report = Some(
                guest
                    .take_gpu_report(budget.remaining()?)
                    .map_err(|why| refusal("observing same-scope full renderer ACK", &why))?,
            );
            receipt.checked_report()?;
            self.check_pool_native_report(&receipt)?;
        }
        receipt.phase = Phase::Running(identity);
        pool::write(&definition.path, &receipt)?;
        let booted = self.connect_managed(guest, None, Some(binding), budget)?;
        if fresh {
            let observer = self
                .gpu_observer
                .as_ref()
                .ok_or_else(|| refused(key, LifecycleCause::Pending))?;
            observer::observe(observer, &booted, &mut receipt, &definition.path, budget)?;
        }
        receipt.checked_readiness()?;
        self.check_pool_native_report(&receipt)?;
        budget.remaining()?;
        Ok(booted)
    }

    fn check_pool_native_report(&self, receipt: &Receipt) -> Result<(), Refusal> {
        let report = receipt
            .checked_report()?
            .ok_or_else(|| refused(receipt.original.key, LifecycleCause::Uncertain))?;
        self.check_native_report(
            &receipt.original.devices,
            &receipt.physical,
            receipt.original.scope(),
            report,
        )
    }

    pub(super) fn recover_gpu_backing(&self) -> Result<(), Refusal> {
        let Some(definition) = self.gpu_backing_definition()? else {
            return Ok(());
        };
        let authority = self.authority()?;
        let records = authority.pool_records()?;
        for record in records
            .iter()
            .filter(|record| record.key.pool == definition.pool)
        {
            self.preflight_backing(record)?;
            if matches!(
                record.phase,
                BackingPhase::Granted | BackingPhase::Live | BackingPhase::Draining
            ) && self.recover_gpu_terminal(&definition, record)?
            {
                continue;
            }
            match record.phase {
                BackingPhase::Prepared | BackingPhase::Released => {}
                BackingPhase::Live => {
                    let allocation = record
                        .allocation
                        .ok_or_else(|| refused(record.key, LifecycleCause::Uncertain))?;
                    self.initialize_gpu_pool(BackingGrant {
                        key: record.key,
                        allocation,
                        devices: self.gpu_ids(&record.devices)?,
                    })?;
                }
                _ => return Err(refused(record.key, LifecycleCause::Uncertain)),
            }
        }
        Ok(())
    }

    fn recover_gpu_terminal(
        &self,
        definition: &GpuBacking,
        snapshot: &PoolRecord,
    ) -> Result<bool, Refusal> {
        let key = snapshot.key;
        let budget = managed::Budget::new(self.first_dial_bound()?)?;
        let keys = self.turn_keys(&definition.devices)?;
        let books = self
            .books
            .get()
            .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
        let mut reservation = self.reserve_owner(OwnerRef::Pool(&key.pool), Operation::Loading)?;
        let released = devices::with_pool_transaction(books, &key, &keys, |transaction| {
            let mut record = transaction
                .read_pool(&key)?
                .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
            if record.native_commitment()? != snapshot.native_commitment()?
                || record.init != snapshot.init
            {
                return Err(refused(key, LifecycleCause::Uncertain));
            }
            let mut receipt = match pool::read(&definition.path)? {
                Some(receipt) => {
                    receipt.matches(&record)?;
                    receipt
                }
                None if record.phase == BackingPhase::Granted && record.init.is_none() => {
                    Receipt::retained(
                        PoolRecord::decode(&record.encode()?)?,
                        self.catalog_physical(&record.devices)?,
                    )?
                }
                None => return Err(refused(key, LifecycleCause::Uncertain)),
            };
            let identity = match &receipt.phase {
                Phase::Running(identity)
                | Phase::Stopping(identity)
                | Phase::Released {
                    identity: Some(identity),
                    ..
                } => {
                    managed::same_native(NativeRecord::Pool(&record), identity)?;
                    Some(super::shared::copy_identity(identity)?)
                }
                // Local publication can lag the canonical native init. None is
                // cleanup-only against this SAME record's scope/commitment; it
                // never reconstructs a Guest or a socket-derived identity.
                Phase::Intent | Phase::Released { identity: None, .. } => None,
            };
            let native = transaction.attach_managed(identity.as_ref())?;
            let observed = native.terminal().map_err(|why| {
                refusal("observing original GPU backing scope during recovery", &why)
            })?;
            if record.phase == BackingPhase::Live && observed.is_none() {
                // Usable recovery additionally checks original report/readiness
                // and connects the SAME native identity; this is no fresh boot.
                return Ok(false);
            }
            record.phase = BackingPhase::Draining;
            transaction.put_pool(&record)?;
            if let Some(identity) = &identity {
                receipt.phase = Phase::Stopping(super::shared::copy_identity(identity)?);
            }
            pool::write(&definition.path, &receipt)?;
            let terminal = match observed {
                Some(end) if end.scope == shards_api::TerminalScope::Released => end,
                _ => managed::stop_scope(
                    transaction,
                    identity.as_ref(),
                    budget.remaining()?,
                    refused(key, LifecycleCause::Uncertain),
                )?,
            };
            receipt.phase = Phase::Released {
                identity,
                status: terminal.status,
            };
            pool::write(&definition.path, &receipt)?;
            transaction.finalize_managed(receipt.released_identity()?)?;
            record.phase = BackingPhase::Released;
            record.exit = Some(super::shared::exit_of(terminal.status));
            transaction.put_pool(&record)?;
            Ok(true)
        })?;
        let previous = if released {
            None
        } else {
            reservation.previous.take()
        };
        reservation.publish(previous)?;
        Ok(released)
    }

    fn gpu_ids(&self, ids: &[hex_api::DeviceId]) -> Result<Vec<hex_api::DeviceId>, Refusal> {
        let mut copy = Vec::new();
        copy.try_reserve_exact(ids.len())
            .map_err(|_| Refusal::Full(hex_api::MAX_DEVICES))?;
        copy.extend_from_slice(ids);
        Ok(copy)
    }

    pub(super) fn gpu_backing_usage(&self, key: BackingKey) -> Result<Usage, Refusal> {
        let definition = self.gpu_definition_for(key)?;
        let books = self
            .books
            .get()
            .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
        let keys = self.turn_keys(&definition.devices)?;
        devices::with_pool_transaction(books, &key, &keys, |transaction| {
            let record = transaction
                .read_pool(&key)?
                .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
            self.preflight_backing(&record)?;
            if !matches!(
                record.phase,
                BackingPhase::Granted | BackingPhase::Live | BackingPhase::Draining
            ) {
                return Err(refused(key, LifecycleCause::Pending));
            }
            transaction.managed_usage()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::{Books, PoolRecords, Records};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    fn refused_model() -> Refusal {
        Refusal::Invalid {
            what: "cold observer model",
            why: "effects are deliberately unavailable".into(),
        }
    }

    struct Cold {
        row: Mutex<Option<PoolRecord>>,
        effects: AtomicUsize,
        queries: AtomicUsize,
        native: Option<triomphe::Arc<RecoveringNative>>,
        finalized: AtomicUsize,
    }
    impl Books for Cold {
        fn records(&self) -> Result<Records, Refusal> {
            Ok(Records::new())
        }
        fn pool_records(&self) -> Result<PoolRecords, Refusal> {
            self.row
                .lock()
                .unwrap()
                .as_ref()
                .map(|row| PoolRecord::decode(&row.encode()?))
                .transpose()
                .map(|row| row.into_iter().collect())
        }
        fn prepare_pool(
            &self,
            pool: [u8; 32],
            execution: hex_api::Execution,
            cost: hex_api::RuntimeCost,
            devices: &[hex_api::DeviceId],
        ) -> Result<BackingOffer, Refusal> {
            let mut row = self.row.lock().unwrap();
            if row.is_none() {
                *row = Some(PoolRecord {
                    version: hex_core::record::RECORD_VERSION,
                    key: BackingKey {
                        pool,
                        generation: [7; 16],
                    },
                    execution,
                    cost,
                    devices: devices.to_vec(),
                    allocation: None,
                    phase: BackingPhase::Prepared,
                    init: None,
                    exit: None,
                });
            }
            row.as_ref().ok_or_else(refused_model)?.offer()
        }
        fn capacity(&self) -> Result<hex_api::Capacity, Refusal> {
            let room = hex_api::CapacityResources {
                cpu_milli: 8000,
                mem_bytes: 1 << 30,
                disk_bytes: 1 << 30,
            };
            Ok(hex_api::Capacity {
                id: "cold-model".into(),
                total: room,
                free: room,
                devices: Vec::new(),
                routes: Vec::new(),
                device_routes: Vec::new(),
                backings: Vec::new(),
                cost: None,
            })
        }
        fn physical_devices(&self) -> Result<Vec<hex_api::DeviceInfo>, Refusal> {
            self.queries.fetch_add(1, Ordering::SeqCst);
            Err(refused_model())
        }
        fn physical_luid(&self, _: hex_api::DeviceId) -> Result<[u8; 8], Refusal> {
            self.queries.fetch_add(1, Ordering::SeqCst);
            Err(refused_model())
        }
        fn with_transaction(
            &self,
            _: &hex_api::BoxId,
            _: &[hex_api::DeviceId],
            _: &mut dyn FnMut(&dyn Transaction) -> Result<(), Refusal>,
        ) -> Result<(), Refusal> {
            self.effects.fetch_add(1, Ordering::SeqCst);
            Err(refused_model())
        }
        fn with_pool_transaction(
            &self,
            key: &BackingKey,
            devices: &[hex_api::DeviceId],
            callback: &mut dyn FnMut(&dyn Transaction) -> Result<(), Refusal>,
        ) -> Result<(), Refusal> {
            self.effects.fetch_add(1, Ordering::SeqCst);
            if self.native.is_none() {
                return Err(refused_model());
            }
            {
                let row = self.row.lock().unwrap();
                let row = row.as_ref().unwrap();
                assert_eq!(row.key, *key);
                assert!(row.devices.iter().all(|id| devices.contains(id)));
            }
            callback(self)
        }
    }

    struct RecoveringNative {
        identity: shards_api::Identity,
        release_on_stop: bool,
        released: std::sync::atomic::AtomicBool,
        attaches: AtomicUsize,
        stops: AtomicUsize,
    }

    impl shards_api::ManagedProcess for RecoveringNative {
        fn identity(&self) -> Result<&shards_api::Identity, shards_api::Refusal> {
            Ok(&self.identity)
        }
        fn stop(&self) -> Result<(), shards_api::Refusal> {
            self.stops.fetch_add(1, Ordering::SeqCst);
            if self.release_on_stop {
                self.released.store(true, Ordering::SeqCst);
            }
            Ok(())
        }
        fn terminal(&self) -> Result<Option<shards_api::ManagedEnd>, shards_api::Refusal> {
            Ok(Some(shards_api::ManagedEnd {
                status: shards_api::ManagedStatus::Code(7),
                scope: if self.released.load(Ordering::SeqCst) {
                    shards_api::TerminalScope::Released
                } else {
                    shards_api::TerminalScope::Process
                },
            }))
        }
        fn wait_for(
            &self,
            _: std::time::Duration,
        ) -> Result<Option<shards_api::ManagedEnd>, shards_api::Refusal> {
            Ok(self
                .terminal()?
                .filter(|end| end.scope == shards_api::TerminalScope::Released))
        }
    }

    impl Transaction for Cold {
        fn records(&self) -> Result<Records, Refusal> {
            Books::records(self)
        }
        fn pool_records(&self) -> Result<PoolRecords, Refusal> {
            Books::pool_records(self)
        }
        fn read_pool(&self, key: &BackingKey) -> Result<Option<PoolRecord>, Refusal> {
            Ok(Books::pool_records(self)?
                .into_iter()
                .find(|row| row.key == *key))
        }
        fn put_pool(&self, next: &PoolRecord) -> Result<(), Refusal> {
            let mut row = self.row.lock().unwrap();
            row.as_ref().unwrap().accepts(next)?;
            *row = Some(PoolRecord::decode(&next.encode()?)?);
            Ok(())
        }
        fn grant_pool(&self, _: &BackingGrant) -> Result<PoolRecord, Refusal> {
            Err(refused_model())
        }
        fn remove_pool(&self, _: &BackingKey) -> Result<(), Refusal> {
            Err(refused_model())
        }
        fn read(&self, _: &hex_api::BoxId) -> Result<Option<hex_core::record::BoxRecord>, Refusal> {
            Err(refused_model())
        }
        fn put(&self, _: &hex_core::record::BoxRecord) -> Result<(), Refusal> {
            Err(refused_model())
        }
        fn remove(&self, _: &hex_api::BoxId) -> Result<(), Refusal> {
            Err(refused_model())
        }
        fn launch_managed(
            &self,
            _: &devices::ManagedLaunch<'_>,
            _: shards_api::LaunchIo,
            _: std::time::Duration,
        ) -> Result<shards_api::ManagedReceipt, Refusal> {
            Err(refused_model())
        }
        fn attach_managed(
            &self,
            identity: Option<&shards_api::Identity>,
        ) -> Result<shards_api::ManagedReceipt, Refusal> {
            let native = self.native.as_ref().ok_or_else(refused_model)?;
            assert!(
                identity.is_none(),
                "the retained Intent has no local controller publication"
            );
            let row = self.row.lock().unwrap();
            let row = row.as_ref().unwrap();
            managed::same_native(NativeRecord::Pool(row), &native.identity)?;
            native.attaches.fetch_add(1, Ordering::SeqCst);
            Ok(shards_api::managed_receipt(native.clone()))
        }
        fn finalize_managed(&self, identity: Option<&shards_api::Identity>) -> Result<(), Refusal> {
            let native = self.native.as_ref().ok_or_else(refused_model)?;
            assert!(identity.is_none());
            assert!(native.released.load(Ordering::SeqCst));
            self.finalized.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn managed_usage(&self) -> Result<Usage, Refusal> {
            Err(refused_model())
        }
    }

    struct NoEffects(AtomicUsize);
    impl shards_api::Guests for NoEffects {
        fn backend(&self) -> Result<shards_api::Backend, shards_api::Refusal> {
            Ok(shards_api::Backend::Kvm)
        }
        fn probe_gpu(
            &self,
            _: &shards_api::GuestSpec,
            _: std::time::Duration,
        ) -> Result<shards_api::GpuProbe, shards_api::Refusal> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(shards_api::Refusal::GpuProbe(
                shards_api::GpuProbeError::Unavailable,
            ))
        }
        fn boot(
            &self,
            _: &shards_api::GuestSpec,
        ) -> Result<Box<dyn shards_api::Guest>, shards_api::Refusal> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(shards_api::Refusal::Launch(
                shards_api::LaunchError::Observer,
            ))
        }
        fn attach(
            &self,
            _: &shards_api::Identity,
            _: &shards_api::GuestSpec,
        ) -> Result<Box<dyn shards_api::Guest>, shards_api::Refusal> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(shards_api::Refusal::Launch(
                shards_api::LaunchError::Observer,
            ))
        }
    }

    /// AC-T7.4.1: crash after the SAME native owner publishes its controller,
    /// before local report/Running, still cleans that admitted scope. Controller
    /// exit alone retains the grant; only a whole-scope witness releases it.
    /// Original production rejection is deliberately unchanged by this control.
    #[test]
    fn published_gpu_scope_with_retained_intent_recovers_without_a_second_launch() {
        for release_on_stop in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let (config, input, allocation) = observer::config::tests::fixture(dir.path());
            let standalone = Arc::new(NoEffects(AtomicUsize::new(0)));
            let catalog = devices::Catalog::parse(
                b"hex-devices-1\ngpu\t256\tunmeasured\t777\tshards-gpu\tpci:0000:01:00.0\t\n",
            )
            .unwrap();
            let runtime = GuestRuntime::new(standalone.clone(), config)
                .recorded_at(dir.path().join("guest"))
                .with_runtime_cost(allocation.cost)
                .unwrap()
                .with_runtime_paths(
                    shards_api::RuntimePaths::new(
                        dir.path().join("cache"),
                        dir.path().join("home"),
                    )
                    .unwrap(),
                )
                .with_gpu_catalog(triomphe::Arc::try_new(catalog).unwrap())
                .with_gpu_observer(input)
                .unwrap();
            let native = triomphe::Arc::try_new(RecoveringNative {
                identity: shards_api::Identity {
                    pid: 42,
                    started: 7,
                    vsock: "/model/v".into(),
                },
                release_on_stop,
                released: std::sync::atomic::AtomicBool::new(false),
                attaches: AtomicUsize::new(0),
                stops: AtomicUsize::new(0),
            })
            .unwrap();
            let owner = Arc::new(Cold {
                row: Mutex::new(None),
                effects: AtomicUsize::new(0),
                queries: AtomicUsize::new(0),
                native: Some(native.clone()),
                finalized: AtomicUsize::new(0),
            });
            let books: Arc<dyn Books> = owner.clone();
            runtime.bind_books(Arc::downgrade(&books)).unwrap();
            let prepared = runtime.prepare_gpu_backing().unwrap().unwrap();
            let mut original = Books::pool_records(owner.as_ref()).unwrap().pop().unwrap();
            original
                .accept_grant(&BackingGrant {
                    key: prepared.key,
                    allocation,
                    devices: prepared.devices.clone(),
                })
                .unwrap();
            let definition = runtime.gpu_backing_definition().unwrap().unwrap();
            let receipt = Receipt::intent(
                PoolRecord::decode(&original.encode().unwrap()).unwrap(),
                definition.physical,
                &runtime.config,
            )
            .unwrap();
            pool::write(&definition.path, &receipt).unwrap();
            // This is the precise publication/crash cut. No native process or
            // guest Program runs; the model's SAME row supplies canonical scope.
            original.init = Some(hex_core::record::ProcId {
                pid: native.identity.pid,
                start_ticks: native.identity.started,
            });
            *owner.row.lock().unwrap() = Some(original);
            let recovered = runtime.recover_gpu_backing();
            let row = Books::pool_records(owner.as_ref()).unwrap().pop().unwrap();
            let retained = pool::read(&definition.path).unwrap().unwrap();
            assert!(
                native.attaches.load(Ordering::SeqCst) > 0,
                "Intent must not prevent cleanup of the original published scope"
            );
            assert_eq!(native.stops.load(Ordering::SeqCst), 1);
            assert_eq!(
                standalone.0.load(Ordering::SeqCst),
                0,
                "recovery cannot boot or probe again"
            );
            assert_eq!(owner.queries.load(Ordering::SeqCst), 0);
            assert_eq!(row.devices, prepared.devices);
            assert_eq!(row.allocation, Some(allocation));
            if release_on_stop {
                assert!(recovered.is_ok());
                assert_eq!(row.phase, BackingPhase::Released);
                assert!(matches!(retained.phase, Phase::Released { .. }));
                assert_eq!(owner.finalized.load(Ordering::SeqCst), 1);
            } else {
                assert!(recovered.is_err());
                assert_eq!(row.phase, BackingPhase::Draining);
                assert!(matches!(retained.phase, Phase::Intent));
                assert_eq!(
                    owner.finalized.load(Ordering::SeqCst),
                    0,
                    "controller-only exit cannot release physical claims"
                );
            }
        }
    }

    /// AC-T7.4.1: cold full-set provisioning reaches the real capacity/bootstrap
    /// entrypoints without GPU observations, a boot or invented DeviceInfo.
    #[test]
    fn cold_gpu_backing_reserves_only_configured_claims_before_any_observation() {
        use hex_api::Runtime as _;
        let dir = tempfile::tempdir().unwrap();
        let (config, input, allocation) = observer::config::tests::fixture(dir.path());
        let native = Arc::new(NoEffects(AtomicUsize::new(0)));
        let catalog = devices::Catalog::parse(b"hex-devices-1\ngpu\t256\tunmeasured-config-only\t777\tshards-gpu\tpci:0000:01:00.0\t\n").unwrap();
        let runtime = GuestRuntime::new(native.clone(), config)
            .recorded_at(dir.path().join("guest"))
            .with_runtime_cost(allocation.cost)
            .unwrap()
            .with_runtime_paths(
                shards_api::RuntimePaths::new(dir.path().join("cache"), dir.path().join("home"))
                    .unwrap(),
            )
            .with_gpu_catalog(triomphe::Arc::try_new(catalog).unwrap())
            .with_gpu_observer(input)
            .unwrap();
        let owner = Arc::new(Cold {
            row: Mutex::new(None),
            effects: AtomicUsize::new(0),
            queries: AtomicUsize::new(0),
            native: None,
            finalized: AtomicUsize::new(0),
        });
        let books: Arc<dyn Books> = owner.clone();
        runtime.bind_books(Arc::downgrade(&books)).unwrap();
        let prepared = runtime.prepare_gpu_backing().unwrap().unwrap();
        assert_eq!(prepared.phase, BackingPhase::Prepared);
        assert_eq!(
            prepared.devices,
            vec![hex_api::DeviceId {
                kind: hex_api::DeviceKind::Gpu,
                key: 256
            }]
        );
        let capacity = runtime.capacity(hex_api::Class::Shards).unwrap().unwrap();
        assert!(
            capacity.devices.is_empty(),
            "declared model/777 bytes are not observed facts"
        );
        assert!(
            capacity
                .routes
                .iter()
                .any(|route| route.key.gpu == hex_api::GpuEligibility::Required
                    && route.backing == Some(prepared.key))
        );
        assert_eq!(capacity.backings.len(), 1);
        assert_eq!(capacity.backings.first().unwrap().key, prepared.key);
        let bad = BackingGrant {
            key: prepared.key,
            allocation,
            devices: Vec::new(),
        };
        assert!(runtime.initialize_gpu_pool(bad).is_err());
        assert_eq!(owner.effects.load(Ordering::SeqCst), 0);
        assert_eq!(owner.queries.load(Ordering::SeqCst), 0);
        assert_eq!(native.0.load(Ordering::SeqCst), 0);
        assert!(
            owner
                .row
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .allocation
                .is_none()
        );
        assert!(
            !runtime
                .gpu_backing_definition()
                .unwrap()
                .unwrap()
                .path
                .exists()
        );
    }
}
