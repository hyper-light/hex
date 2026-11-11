//! The initialized native owner's existing records and method/device turns. A weak
//! reference connects this adapter to Node or Recorded without another record owner.

use std::sync::Weak;

use hex_core::record::{BoxRecord, Phase};

use super::*;
use crate::devices::{self, Books};

pub(super) fn uncertain(id: &BoxId) -> Result<Refusal, Refusal> {
    Ok(Refusal::Lifecycle {
        id: id.try_clone()?,
        cause: hex_api::LifecycleCause::Uncertain,
    })
}

impl GuestRuntime {
    /// Called once after the existing Arc<Node> or Arc<Recorded<WithGuest<P>>>
    /// exists. Coercing that Arc to Books and downgrading allocates no new owner.
    pub fn bind_books(&self, books: Weak<dyn Books>) -> Result<(), Refusal> {
        if books.upgrade().is_none() {
            return Err(Refusal::Broken("guest record authority is gone".into()));
        }
        if let Some(bound) = self.books.get() {
            return if Weak::ptr_eq(bound, &books) {
                Ok(())
            } else {
                Err(Refusal::Broken(
                    "guest record authority cannot be rebound".into(),
                ))
            };
        }
        self.books.set(books).or_else(|books| {
            if self
                .books
                .get()
                .is_some_and(|bound| Weak::ptr_eq(bound, &books))
            {
                Ok(())
            } else {
                Err(Refusal::Broken(
                    "guest record authority changed during initialization".into(),
                ))
            }
        })
    }

    pub fn uses_books(&self) -> bool {
        self.books.get().is_some()
    }

    /// Ordinary CPU OCI retains Recorded's existing lifecycle. Explicit Shards,
    /// including CPU-only boxes, and selected guest OCI use this same Books owner.
    pub fn owns_record(&self, spec: &BoxSpec) -> bool {
        self.uses_books()
            && (matches!(spec.class, ClassSpec::Shards(_)) || !spec.devices.is_empty())
    }

    pub fn with_runtime_cost(mut self, cost: hex_api::RuntimeCost) -> Result<Self, Refusal> {
        cost.check()?;
        let configured = self.dedicated_resources()?;
        if cost.floor.cpu_milli != configured.cpu_milli
            || cost.ceiling.cpu_milli != configured.cpu_milli
            || cost.floor.mem_bytes != configured.mem_bytes
            || cost.ceiling.mem_bytes != configured.mem_bytes
        {
            return Err(Refusal::Invalid {
                what: "Shards runtime cost",
                why: "CPU/memory floor and ceiling must equal the configured VM".into(),
            });
        }
        // The explicit units and measured kernel/relay allowance must fit that
        // same configured envelope. No overhead estimate is invented here.
        cost.allocate(hex_api::CapacityResources::default())?;
        self.runtime_cost = Some(cost);
        Ok(self)
    }

    pub(super) fn check_allocation(&self, spec: &BoxSpec) -> Result<(), Refusal> {
        let cost = self.runtime_cost.ok_or_else(|| Refusal::Invalid {
            what: "Shards runtime cost",
            why: "measured backend cost has not been initialized".into(),
        })?;
        let allocation = spec.allocation.ok_or_else(|| Refusal::Invalid {
            what: "Shards allocation",
            why: "the scheduler must admit the configured VM and measured overhead before boot"
                .into(),
        })?;
        if allocation.cost != cost {
            return Err(Refusal::Invalid {
                what: "Shards allocation",
                why: "admission cost identity or measured profile changed".into(),
            });
        }
        let guest = allocation.guest()?;
        let configured = self.dedicated_resources()?;
        if guest.cpu_milli != configured.cpu_milli || guest.mem_bytes != configured.mem_bytes {
            return Err(Refusal::Invalid {
                what: "Shards allocation",
                why: "admitted guest envelope disagrees with the configured VM".into(),
            });
        }
        spec.check()
    }

    pub(super) fn authority(&self) -> Result<Arc<dyn Books>, Refusal> {
        self.books
            .get()
            .and_then(Weak::upgrade)
            .ok_or_else(|| Refusal::Broken("guest has no live native record authority".into()))
    }

    /// Read the canonical persisted route before touching a native backend. This
    /// snapshot grants no method ownership; each operation rereads under its turn.
    pub fn recorded_box(&self, id: &BoxId) -> Result<Option<BoxRecord>, Refusal> {
        if !self.uses_books() {
            return Ok(None);
        }
        self.authority()?
            .records()?
            .remove(id)
            .map(|record| {
                record.map_err(|_| Refusal::Broken("guest box record is unreadable".into()))
            })
            .transpose()
    }

    pub(super) fn turn_keys(
        &self,
        devices: &[hex_api::DeviceId],
    ) -> Result<Vec<hex_api::DeviceId>, Refusal> {
        if devices.is_empty() {
            return Ok(Vec::new());
        }
        self.gpu_catalog
            .as_ref()
            .ok_or_else(|| Refusal::Broken("guest has no initialized device catalog".into()))?
            .conflict_ids(devices)
    }

    pub(super) fn create_recorded(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        self.check_allocation(&spec)?;
        let budget = managed::Budget::new(self.first_dial_bound()?)?;
        let books = self.books.get().ok_or_else(|| {
            Refusal::Broken("a dedicated guest needs the native record authority".into())
        })?;
        let id = spec.id.try_clone()?;
        let keys = self.turn_keys(&spec.devices)?;
        devices::with_transaction(books, &id, &keys, |transaction| {
            if transaction.read(&id)?.is_some() {
                return Err(Refusal::Exists(id.try_clone()?));
            }
            self.prepare_selected_create(&id)?;
            let records = transaction.records()?;
            if records.len() >= crate::MAX_BOXES {
                return Err(Refusal::Full(crate::MAX_BOXES));
            }
            if !spec.devices.is_empty() {
                let catalog = self.gpu_catalog.as_ref().ok_or_else(|| {
                    Refusal::Broken("guest has no initialized device catalog".into())
                })?;
                let _selection = catalog.prepare(&spec, &records)?;
            }
            // Store the original workload and immutable admission receipt, never
            // the rewritten inner OCI payload. Any failed effect retains Creating.
            budget.remaining()?;
            let record = BoxRecord::new(spec, Phase::Creating, Vec::new());
            transaction.put(&record)?;
            let info = self.create_selected(record.spec, transaction, budget)?;
            let mut record = transaction.read(&id)?.ok_or(uncertain(&id)?)?;
            if record.phase != Phase::Creating
                || info.id != id
                || info.class != record.spec.class.class()
                || info.allocation != record.spec.allocation
                || record.init.is_none()
                || record.native_scope.is_none()
            {
                return Err(uncertain(&id)?);
            }
            record.phase = Phase::Ready;
            transaction.put(&record)?;
            Ok(info)
        })
    }

    pub(super) fn call_recorded<T>(
        &self,
        id: &BoxId,
        ready: bool,
        call: impl FnOnce() -> Result<T, Refusal>,
    ) -> Result<T, Refusal> {
        let Some(books) = self.books.get() else {
            return call();
        };
        let snapshot = self
            .recorded_box(id)?
            .ok_or(Refusal::NoSuchBox(id.try_clone()?))?;
        if !self.owns_record(&snapshot.spec) {
            return call();
        }
        let keys = self.turn_keys(&snapshot.spec.devices)?;
        devices::with_transaction(books, id, &keys, |transaction| {
            let record = transaction
                .read(id)?
                .ok_or(Refusal::NoSuchBox(id.try_clone()?))?;
            if record.spec != snapshot.spec
                || record.init != snapshot.init
                || record.native_scope != snapshot.native_scope
            {
                return Err(uncertain(id)?);
            }
            if ready && record.phase != Phase::Ready {
                return Err(uncertain(id)?);
            }
            self.validate_binding(&record.spec)?;
            self.selected_running_in(id, transaction, &record)?
                .ok_or(uncertain(id)?)?;
            call()
        })
    }

    pub(super) fn delete_recorded(&self, id: &BoxId) -> Result<(), Refusal> {
        let books = self
            .books
            .get()
            .ok_or_else(|| Refusal::Broken("guest has no initialized record authority".into()))?;
        let snapshot = self
            .recorded_box(id)?
            .ok_or(Refusal::NoSuchBox(id.try_clone()?))?;
        let keys = self.turn_keys(&snapshot.spec.devices)?;
        devices::with_transaction(books, id, &keys, |transaction| {
            let mut record = transaction
                .read(id)?
                .ok_or(Refusal::NoSuchBox(id.try_clone()?))?;
            if record.spec != snapshot.spec
                || record.init != snapshot.init
                || record.native_scope != snapshot.native_scope
            {
                return Err(uncertain(id)?);
            }
            record.phase = Phase::Deleting;
            transaction.put(&record)?;
            // Stop uses the immutable VM receipt, even if current bindings changed.
            // Failed/uncertain native observation leaves the sole holder in Deleting.
            self.delete_selected(id, transaction)?;
            self.finalize_selected(id, transaction)?;
            transaction.remove(id)?;
            // The positive Released receipt survives any failed native finalization
            // or Store removal; remove it only after the canonical transaction did.
            self.forget_selected(id)
        })
    }

    pub(super) fn native_capacity(&self) -> Result<hex_api::Capacity, Refusal> {
        let authority = self.authority()?;
        let mut capacity = authority.capacity()?;
        capacity.cost = Some(self.runtime_cost.ok_or_else(|| Refusal::Invalid {
            what: "Shards runtime cost",
            why: "measured backend cost has not been initialized".into(),
        })?);
        capacity.devices = self.attachment_offers()?;
        capacity.check()?;
        Ok(capacity)
    }

    /// Dedicated VM statistics come from the same native scope, including its
    /// helpers. No relay/guest must be usable to observe those retained resources.
    pub(super) fn native_usage(&self, id: &BoxId) -> Result<Option<Usage>, Refusal> {
        let Some(books) = self.books.get() else {
            return Ok(None);
        };
        let snapshot = self
            .recorded_box(id)?
            .ok_or(Refusal::NoSuchBox(id.try_clone()?))?;
        if !self.owns_record(&snapshot.spec) {
            return Ok(None);
        }
        let keys = self.turn_keys(&snapshot.spec.devices)?;
        devices::with_transaction(books, id, &keys, |transaction| {
            let record = transaction
                .read(id)?
                .ok_or(Refusal::NoSuchBox(id.try_clone()?))?;
            if record.native_scope.is_none()
                || record.native_scope != snapshot.native_scope
                || record.spec != snapshot.spec
                || record.init != snapshot.init
            {
                return Err(uncertain(id)?);
            }
            transaction.managed_usage().map(Some)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::{Records, Transaction};
    use hex_api::{Capacity, CapacityResources};
    use std::sync::atomic::AtomicUsize;

    #[derive(Default)]
    struct Owner {
        records: Mutex<Records>,
        calls: AtomicUsize,
    }
    impl Transaction for Owner {
        fn records(&self) -> Result<Records, Refusal> {
            Ok(self.records.lock().unwrap().clone())
        }
        fn read(&self, id: &BoxId) -> Result<Option<BoxRecord>, Refusal> {
            Ok(self
                .records
                .lock()
                .unwrap()
                .get(id)
                .map(|row| row.as_ref().unwrap().clone()))
        }
        fn put(&self, record: &BoxRecord) -> Result<(), Refusal> {
            self.records
                .lock()
                .unwrap()
                .insert(record.spec.id.clone(), Ok(record.clone()));
            Ok(())
        }
        fn remove(&self, id: &BoxId) -> Result<(), Refusal> {
            self.records.lock().unwrap().remove(id);
            Ok(())
        }
        fn finalize_managed(&self, _: Option<&shards_api::Identity>) -> Result<(), Refusal> {
            panic!("pure record control cannot finalize native scope")
        }
        fn managed_usage(&self) -> Result<Usage, Refusal> {
            Err(Refusal::Broken(
                "this pure model has no native counters".into(),
            ))
        }
        fn launch_managed(
            &self,
            _: &crate::devices::ManagedLaunch<'_>,
            _: shards_api::LaunchIo,
            _: Duration,
        ) -> Result<shards_api::ManagedReceipt, Refusal> {
            panic!("pure record control cannot launch")
        }
        fn attach_managed(
            &self,
            _: Option<&shards_api::Identity>,
        ) -> Result<shards_api::ManagedReceipt, Refusal> {
            panic!("pure record control cannot attach")
        }
    }
    impl Books for Owner {
        fn physical_devices(&self) -> Result<Vec<hex_api::DeviceInfo>, Refusal> {
            Ok(Vec::new())
        }
        fn records(&self) -> Result<Records, Refusal> {
            Transaction::records(self)
        }
        fn capacity(&self) -> Result<Capacity, Refusal> {
            Ok(Capacity {
                id: "model-host".into(),
                total: CapacityResources {
                    cpu_milli: 8000,
                    mem_bytes: 1 << 30,
                    disk_bytes: 1 << 30,
                },
                free: CapacityResources {
                    cpu_milli: 8000,
                    mem_bytes: 1 << 30,
                    disk_bytes: 1 << 30,
                },
                devices: Vec::new(),
                cost: None,
            })
        }
        fn with_transaction(
            &self,
            _: &BoxId,
            _: &[hex_api::DeviceId],
            callback: &mut dyn FnMut(&dyn Transaction) -> Result<(), Refusal>,
        ) -> Result<(), Refusal> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            callback(self)
        }
    }

    fn runtime() -> GuestRuntime {
        GuestRuntime::new(
            Arc::new(shards_api::sim::SimGuests::new(|_| {
                shards_api::End::PowerOff
            })),
            super::super::tests::config(),
        )
    }

    fn cost(runtime: &GuestRuntime) -> hex_api::RuntimeCost {
        let floor = runtime.dedicated_resources().unwrap();
        hex_api::RuntimeCost {
            id: [1; 32],
            floor,
            ceiling: floor,
            quantum: CapacityResources {
                cpu_milli: 1000,
                mem_bytes: 1 << 20,
                disk_bytes: 1,
            },
            // Explicit model inputs for a pure test, never a native measurement.
            guest_overhead: CapacityResources {
                cpu_milli: 1,
                mem_bytes: 1,
                disk_bytes: 0,
            },
            overhead: CapacityResources {
                cpu_milli: 1,
                mem_bytes: 1,
                disk_bytes: 0,
            },
        }
    }

    /// AC-T7.4.1: only the same initialized record owner can be bound, and its
    /// death is uncertainty rather than a new Store or permissive empty ledger.
    #[test]
    fn weak_registration_retains_one_canonical_owner() {
        let runtime = runtime();
        let owner: Arc<dyn Books> = Arc::new(Owner::default());
        let another: Arc<dyn Books> = Arc::new(Owner::default());
        runtime.bind_books(Arc::downgrade(&owner)).unwrap();
        runtime.bind_books(Arc::downgrade(&owner)).unwrap();
        assert!(runtime.bind_books(Arc::downgrade(&another)).is_err());
        assert!(runtime.uses_books());
        drop(owner);
        assert!(
            runtime
                .recorded_box(&BoxId::new("no-owner").unwrap())
                .is_err()
        );
    }

    /// AC-T7.4.1: scheduler charge and actual configured VM must be identical;
    /// missing, forged, stale or smaller allocation is refused before a transaction.
    #[test]
    fn dedicated_cost_is_required_and_cannot_shrink_original_workload() {
        let runtime = runtime();
        let profile = cost(&runtime);
        let runtime = runtime.with_runtime_cost(profile).unwrap();
        let mut spec = super::super::tests::oci(hex_api::Rootfs::Dir("/rootfs".into()), Vec::new());
        let ClassSpec::Oci(payload) = spec.class else {
            panic!("fixture OCI")
        };
        spec.class = ClassSpec::Shards(payload);
        spec.bounds.cpu_max_milli = Some(1);
        spec.bounds.memory_max = Some(1);
        assert!(runtime.check_allocation(&spec).is_err());
        spec.allocation = Some(
            profile
                .allocate(CapacityResources {
                    cpu_milli: 1,
                    mem_bytes: 1,
                    disk_bytes: 0,
                })
                .unwrap(),
        );
        runtime.check_allocation(&spec).unwrap();
        let mut forged = spec.clone();
        forged.allocation.as_mut().unwrap().resources.cpu_milli = 1;
        assert!(runtime.check_allocation(&forged).is_err());
        let mut stale = spec.clone();
        stale.allocation.as_mut().unwrap().cost.id = [2; 32];
        assert!(runtime.check_allocation(&stale).is_err());
        spec.bounds.memory_max = Some(profile.ceiling.mem_bytes);
        assert!(
            runtime.check_allocation(&spec).is_err(),
            "guest overhead cannot shrink the original cap"
        );
    }

    /// AC-T7.4.1: canonical recorded class/phase gates every dedicated method,
    /// while ordinary CPU OCI retains its one outer Recorded transaction.
    #[test]
    fn dedicated_methods_enter_canonical_turn_once_and_refuse_nonready_effects() {
        let runtime = runtime();
        let owner = Arc::new(Owner::default());
        let books: Arc<dyn Books> = owner.clone();
        runtime.bind_books(Arc::downgrade(&books)).unwrap();
        let mut spec = super::super::tests::oci(hex_api::Rootfs::Dir("/rootfs".into()), Vec::new());
        let id = spec.id.clone();
        owner
            .put(&BoxRecord::new(spec.clone(), Phase::Ready, Vec::new()))
            .unwrap();
        assert!(!runtime.owns_record(&spec));
        assert_eq!(runtime.call_recorded(&id, true, || Ok(7)), Ok(7));
        assert_eq!(owner.calls.load(Ordering::SeqCst), 0);
        let ClassSpec::Oci(payload) = spec.class else {
            panic!("fixture OCI")
        };
        spec.class = ClassSpec::Shards(payload);
        owner
            .put(&BoxRecord::new(spec.clone(), Phase::Creating, Vec::new()))
            .unwrap();
        assert!(runtime.owns_record(&spec));
        assert!(matches!(
            runtime.call_recorded(&id, true, || -> Result<(), Refusal> {
                panic!("non-ready effect")
            }),
            Err(Refusal::Lifecycle { .. })
        ));
        assert_eq!(owner.calls.load(Ordering::SeqCst), 1);
    }
}
