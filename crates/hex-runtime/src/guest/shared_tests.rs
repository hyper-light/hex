//! AC-T7.4.1: real production pool entrypoints over a controlled same-owner Books.
//! Counters are model oracles; no native process, VM or renderer is started here.

use super::*;
use crate::devices::{Books, PoolRecords, Records, Transaction};
use hex_api::{Capacity, CapacityResources};
use hex_core::record::{BoxRecord, RECORD_VERSION};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Default)]
struct Native {
    released: AtomicBool,
    stops: AtomicUsize,
}

impl shards_api::ManagedProcess for Native {
    fn identity(&self) -> Result<&shards_api::Identity, shards_api::Refusal> {
        Err(shards_api::Refusal::Launch(
            shards_api::LaunchError::Observer,
        ))
    }
    fn stop(&self) -> Result<(), shards_api::Refusal> {
        self.stops.fetch_add(1, Ordering::SeqCst);
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
    fn wait_for(&self, _: Duration) -> Result<Option<shards_api::ManagedEnd>, shards_api::Refusal> {
        Ok(self
            .terminal()?
            .filter(|end| end.scope == shards_api::TerminalScope::Released))
    }
}

struct Owner {
    pool: Mutex<Option<PoolRecord>>,
    boxes: Mutex<Records>,
    launches: AtomicUsize,
    finalized: AtomicUsize,
    fail_finalize: AtomicBool,
    native: triomphe::Arc<Native>,
}

impl Owner {
    fn new() -> Self {
        Self {
            pool: Mutex::new(None),
            boxes: Mutex::new(Records::new()),
            launches: AtomicUsize::new(0),
            finalized: AtomicUsize::new(0),
            fail_finalize: AtomicBool::new(false),
            native: triomphe::Arc::try_new(Native::default()).unwrap(),
        }
    }
    fn row(&self) -> PoolRecord {
        let held = self.pool.lock().unwrap();
        let row = held.as_ref().unwrap();
        PoolRecord::decode(&row.encode().unwrap()).unwrap()
    }
}

impl Transaction for Owner {
    fn records(&self) -> Result<Records, Refusal> {
        Ok(self.boxes.lock().unwrap().clone())
    }
    fn pool_records(&self) -> Result<PoolRecords, Refusal> {
        Ok(self
            .pool
            .lock()
            .unwrap()
            .as_ref()
            .map(|row| PoolRecord::decode(&row.encode().unwrap()).unwrap())
            .into_iter()
            .collect())
    }
    fn read_pool(&self, key: &BackingKey) -> Result<Option<PoolRecord>, Refusal> {
        Ok(Transaction::pool_records(self)?
            .into_iter()
            .find(|row| row.key == *key))
    }
    fn put_pool(&self, row: &PoolRecord) -> Result<(), Refusal> {
        row.check()?;
        let mut held = self.pool.lock().unwrap();
        held.as_ref().unwrap().accepts(row)?;
        *held = Some(PoolRecord::decode(&row.encode()?)?);
        Ok(())
    }
    fn grant_pool(&self, grant: &BackingGrant) -> Result<PoolRecord, Refusal> {
        let mut row = self.row();
        row.accept_grant(grant)?;
        self.put_pool(&row)?;
        Ok(row)
    }
    fn remove_pool(&self, _: &BackingKey) -> Result<(), Refusal> {
        panic!("these controls retain the canonical terminal row")
    }
    fn read(&self, id: &BoxId) -> Result<Option<BoxRecord>, Refusal> {
        Ok(self
            .boxes
            .lock()
            .unwrap()
            .get(id)
            .map(|row| row.as_ref().unwrap().clone()))
    }
    fn put(&self, row: &BoxRecord) -> Result<(), Refusal> {
        self.boxes
            .lock()
            .unwrap()
            .insert(row.spec.id.clone(), Ok(row.clone()));
        Ok(())
    }
    fn remove(&self, id: &BoxId) -> Result<(), Refusal> {
        self.boxes.lock().unwrap().remove(id);
        Ok(())
    }
    fn launch_managed(
        &self,
        plan: &devices::ManagedLaunch<'_>,
        _: shards_api::LaunchIo,
        _: Duration,
    ) -> Result<shards_api::ManagedReceipt, Refusal> {
        assert!(matches!(plan, devices::ManagedLaunch::Shards(_)));
        assert_eq!(self.row().phase, BackingPhase::Granted);
        self.launches.fetch_add(1, Ordering::SeqCst);
        Err(refused(self.row().key, LifecycleCause::Uncertain))
    }
    fn attach_managed(
        &self,
        identity: Option<&shards_api::Identity>,
    ) -> Result<shards_api::ManagedReceipt, Refusal> {
        assert!(
            identity.is_none(),
            "no controller was published in this model"
        );
        Ok(shards_api::managed_receipt(self.native.clone()))
    }
    fn finalize_managed(&self, identity: Option<&shards_api::Identity>) -> Result<(), Refusal> {
        assert!(identity.is_none());
        assert!(self.native.released.load(Ordering::SeqCst));
        if self.fail_finalize.load(Ordering::SeqCst) {
            return Err(refused(self.row().key, LifecycleCause::Uncertain));
        }
        self.finalized.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn managed_usage(&self) -> Result<Usage, Refusal> {
        panic!("no native counters are measured by these controls")
    }
}

impl Books for Owner {
    fn with_handoff_transaction(
        &self,
        _: &hex_api::BoxId,
        _: &hex_api::BackingKey,
        _: &[hex_api::DeviceId],
        _: &mut dyn FnMut(&dyn crate::devices::Transaction) -> Result<(), hex_api::Refusal>,
    ) -> Result<(), hex_api::Refusal> {
        panic!("existing test owner must not enter a subset handoff")
    }

    fn records(&self) -> Result<Records, Refusal> {
        Transaction::records(self)
    }
    fn pool_records(&self) -> Result<PoolRecords, Refusal> {
        Transaction::pool_records(self)
    }
    fn prepare_pool(
        &self,
        pool: [u8; 32],
        execution: Execution,
        cost: hex_api::RuntimeCost,
        devices: &[hex_api::DeviceId],
    ) -> Result<BackingOffer, Refusal> {
        let mut held = self.pool.lock().unwrap();
        if let Some(row) = held.as_ref() {
            assert_eq!(row.key.pool, pool);
            return row.offer();
        }
        let row = PoolRecord {
            version: RECORD_VERSION,
            key: BackingKey {
                pool,
                generation: [7; 16],
            }, // Deterministic model input, not an entropy implementation.
            execution,
            cost,
            allocation: None,
            devices: devices.to_vec(),
            phase: BackingPhase::Prepared,
            init: None,
            exit: None,
        };
        let offer = row.offer()?;
        *held = Some(row);
        Ok(offer)
    }
    fn physical_devices(&self) -> Result<Vec<hex_api::DeviceInfo>, Refusal> {
        Ok(Vec::new())
    }
    fn capacity(&self) -> Result<Capacity, Refusal> {
        let room = CapacityResources {
            cpu_milli: 8000,
            mem_bytes: 1 << 30,
            disk_bytes: 1 << 30,
        };
        Ok(Capacity {
            id: "same-model-native-owner".into(),
            total: room,
            free: room,
            cost: None,
            devices: Vec::new(),
            routes: Vec::new(),
            device_routes: Vec::new(),
            backings: Vec::new(),
        })
    }
    fn with_transaction(
        &self,
        _: &BoxId,
        _: &[hex_api::DeviceId],
        callback: &mut dyn FnMut(&dyn Transaction) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        callback(self)
    }
    fn with_pool_transaction(
        &self,
        key: &BackingKey,
        devices: &[hex_api::DeviceId],
        callback: &mut dyn FnMut(&dyn Transaction) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        assert_eq!(self.row().key, *key);
        assert!(devices.is_empty(), "these shared CPU controls claim no GPU");
        callback(self)
    }
}

// These two controls must reach the native model callback. Its real prepared
// socket uses the platform's 103-byte minimum sun_path bound; a cohort's long
// TMPDIR is not a valid socket fixture. TempDir still owns/removes this directory.
fn socket_fixture() -> tempfile::TempDir {
    #[cfg(unix)]
    return tempfile::tempdir_in("/tmp").unwrap();
    #[cfg(not(unix))]
    tempfile::tempdir().unwrap()
}

fn configured(path: &Path) -> (GuestRuntime, Arc<Owner>) {
    let mut config = super::super::tests::config();
    config.binary = path.join("shards");
    config.kernel = path.join("kernel");
    config.initrd = path.join("initrd");
    config.shares.clear();
    let floor = selected::resources(&config).unwrap();
    let cost = hex_api::RuntimeCost {
        id: [1; 32],
        floor,
        ceiling: floor,
        quantum: CapacityResources {
            cpu_milli: 1000,
            mem_bytes: 1 << 20,
            disk_bytes: 1,
        },
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
    };
    let runtime = GuestRuntime::new(
        Arc::new(shards_api::sim::SimGuests::new(|_| {
            shards_api::End::PowerOff
        })),
        config,
    )
    .recorded_at(path.join("@guest"))
    .with_runtime_paths(
        shards_api::RuntimePaths::new(path.join("cache"), path.join("home")).unwrap(),
    )
    .with_runtime_cost(cost)
    .unwrap();
    let owner = Arc::new(Owner::new());
    let books: Arc<dyn Books> = owner.clone();
    runtime.bind_books(Arc::downgrade(&books)).unwrap();
    (runtime, owner)
}

fn grant(offer: &BackingOffer) -> BackingGrant {
    BackingGrant {
        key: offer.key,
        allocation: offer.allocation().unwrap(),
        devices: Vec::new(),
    }
}

#[test]
fn shared_offer_prepares_before_grant_without_a_boot_or_fake_workload_row() {
    let dir = tempfile::tempdir().unwrap();
    let (runtime, owner) = configured(dir.path());
    let offer = runtime.prepare_backing().unwrap();
    assert_eq!(offer.phase, BackingPhase::Prepared);
    let capacity = runtime.capacity(Class::Oci).unwrap().unwrap();
    assert_eq!(capacity.backings.len(), 1);
    assert_eq!(capacity.backings[0].key, offer.key);
    assert_eq!(capacity.routes[0].key.execution, Execution::ShardsShared);
    assert_eq!(capacity.routes[0].cost, None);
    assert!(capacity.devices.is_empty());
    assert_eq!(owner.launches.load(Ordering::SeqCst), 0);
    assert!(owner.boxes.lock().unwrap().is_empty());
    assert_eq!(owner.row().allocation, None);
    assert!(pool::read(&dir.path().join("@guest")).unwrap().is_none());
}

#[test]
fn shared_retained_preflight_rejects_replaced_profile_key_or_generation_before_effects() {
    let dir = tempfile::tempdir().unwrap();
    let (runtime, owner) = configured(dir.path());
    runtime.prepare_backing().unwrap();
    runtime.preflight_backing(&owner.row()).unwrap();
    let mut changed = owner.row();
    changed.key.pool = [8; 32];
    assert!(runtime.preflight_backing(&changed).is_err());
    changed = owner.row();
    changed.key.generation = [8; 16];
    assert!(runtime.preflight_backing(&changed).is_err());
    changed = owner.row();
    changed.cost.id = [8; 32];
    assert!(runtime.preflight_backing(&changed).is_err());
    assert_eq!(owner.launches.load(Ordering::SeqCst), 0);
    assert!(owner.boxes.lock().unwrap().is_empty());
    assert!(pool::read(&dir.path().join("@guest")).unwrap().is_none());
}

/// A retained configuration key includes its original cost; it must ALSO
/// match the currently initialized measured cost before any owner effect.
#[test]
fn retained_backing_refuses_a_replaced_initialized_cost_without_inner_boxes() {
    let dir = tempfile::tempdir().unwrap();
    let (runtime, owner) = configured(dir.path());
    runtime.prepare_backing().unwrap();
    let original = owner.row();
    let (mut replaced, replacement_owner) = configured(dir.path());
    replaced.runtime_cost.as_mut().unwrap().id = [8; 32];
    assert!(replaced.preflight_backing(&original).is_err());
    assert_eq!(owner.launches.load(Ordering::SeqCst), 0);
    assert_eq!(replacement_owner.launches.load(Ordering::SeqCst), 0);
    assert!(owner.boxes.lock().unwrap().is_empty());
}

#[test]
fn interrupted_shared_launch_keeps_the_exact_charge_and_never_reboots_an_intent() {
    let dir = socket_fixture();
    let (runtime, owner) = configured(dir.path());
    let offer = runtime.prepare_backing().unwrap();
    let mut wrong = grant(&offer);
    wrong.key.generation = [8; 16];
    assert!(runtime.initialize_backing(wrong).is_err());
    assert_eq!(owner.launches.load(Ordering::SeqCst), 0);
    assert!(runtime.initialize_backing(grant(&offer)).is_err());
    assert_eq!(owner.launches.load(Ordering::SeqCst), 1);
    let receipt = pool::read(&dir.path().join("@guest")).unwrap().unwrap();
    assert_eq!(
        receipt.original.allocation,
        Some(offer.allocation().unwrap())
    );
    assert_eq!(receipt.original.scope(), owner.row().scope());
    assert!(matches!(receipt.phase, Phase::Intent));
    assert!(runtime.initialize_backing(grant(&offer)).is_err());
    assert_eq!(owner.launches.load(Ordering::SeqCst), 1);
    assert_eq!(owner.row().phase, BackingPhase::Granted);
    assert!(owner.boxes.lock().unwrap().is_empty());
}

#[test]
fn shared_charge_survives_controller_only_exit_and_failed_native_finalization() {
    let dir = tempfile::tempdir().unwrap();
    let (runtime, owner) = configured(dir.path());
    let offer = runtime.prepare_backing().unwrap();
    assert!(runtime.initialize_backing(grant(&offer)).is_err());
    assert!(runtime.release_backing(offer.key).is_err());
    assert_eq!(owner.row().phase, BackingPhase::Draining);
    assert_eq!(owner.row().allocation, Some(offer.allocation().unwrap()));
    owner.native.released.store(true, Ordering::SeqCst);
    owner.fail_finalize.store(true, Ordering::SeqCst);
    assert!(runtime.release_backing(offer.key).is_err());
    let stops = owner.native.stops.load(Ordering::SeqCst);
    assert_eq!(owner.row().phase, BackingPhase::Draining);
    assert!(matches!(
        pool::read(&dir.path().join("@guest"))
            .unwrap()
            .unwrap()
            .phase,
        Phase::Released { .. }
    ));
    owner.fail_finalize.store(false, Ordering::SeqCst);
    let released = runtime.release_backing(offer.key).unwrap();
    assert_eq!(released.phase, BackingPhase::Released);
    assert_eq!(owner.row().exit, Some(hex_api::Exit::Code(7)));
    assert_eq!(owner.native.stops.load(Ordering::SeqCst), stops);
    assert_eq!(owner.finalized.load(Ordering::SeqCst), 1);
}

#[test]
fn shared_workload_reference_refuses_release_before_any_native_stop() {
    let dir = tempfile::tempdir().unwrap();
    let (runtime, owner) = configured(dir.path());
    let offer = runtime.prepare_backing().unwrap();
    assert!(runtime.initialize_backing(grant(&offer)).is_err());
    let mut spec = super::super::tests::oci(
        hex_api::Rootfs::Dir(dir.path().join("rootfs.tar")),
        Vec::new(),
    );
    spec.route = Some(hex_api::RouteReceipt {
        key: runtime.capacity(Class::Oci).unwrap().unwrap().routes[0].key,
        backing: Some(offer.key),
    });
    Transaction::put(
        owner.as_ref(),
        &BoxRecord::new(spec, hex_core::record::Phase::Creating, Vec::new()),
    )
    .unwrap();
    assert!(matches!(
        runtime.release_backing(offer.key),
        Err(Refusal::Backing {
            cause: LifecycleCause::Busy,
            ..
        })
    ));
    assert_eq!(owner.native.stops.load(Ordering::SeqCst), 0);
    assert_eq!(owner.row().phase, BackingPhase::Granted);
}

#[test]
fn ended_shared_scope_allows_inner_cleanup_while_the_original_grant_stays_charged() {
    let dir = socket_fixture();
    let (runtime, owner) = configured(dir.path());
    let offer = runtime.prepare_backing().unwrap();
    assert!(runtime.initialize_backing(grant(&offer)).is_err());
    let mut spec = super::super::tests::oci(
        hex_api::Rootfs::Dir(dir.path().join("rootfs.tar")),
        Vec::new(),
    );
    spec.route = Some(hex_api::RouteReceipt {
        key: runtime.capacity(Class::Oci).unwrap().unwrap().routes[0].key,
        backing: Some(offer.key),
    });
    let id = spec.id.clone();
    Transaction::put(
        owner.as_ref(),
        &BoxRecord::new(spec, hex_core::record::Phase::Creating, Vec::new()),
    )
    .unwrap();
    owner.native.released.store(true, Ordering::SeqCst);
    runtime.recover_backing().unwrap();
    assert_eq!(owner.row().phase, BackingPhase::Draining);
    assert_eq!(owner.row().allocation, Some(offer.allocation().unwrap()));
    assert_eq!(runtime.inspect(&id), Err(Refusal::NoSuchBox(id.clone())));
    runtime.delete(&id).unwrap();
    Transaction::remove(owner.as_ref(), &id).unwrap();
    runtime.release_backing(offer.key).unwrap();
    assert_eq!(
        owner.native.stops.load(Ordering::SeqCst),
        0,
        "no stop of a scope already proved empty"
    );
    assert_eq!(
        owner.launches.load(Ordering::SeqCst),
        1,
        "recovery never boots another VM"
    );
}

/// AC-T7.4.1: a checked renderer/native join can supply a factual device when
/// the host resource plane has no native-driver slot. No catalog placeholder
/// may replace it, and a conflicting existing identity must remain a refusal.
/// Original production is unchanged until this source control is reproduced.
#[test]
fn checked_gpu_facts_create_a_missing_capacity_slot_and_refuse_conflicting_metadata() {
    use hex_api::{
        DeviceHolder, DeviceId, DeviceInfo, DeviceKind, GpuEligibility, ImageOs, ImagePlatform,
        RouteKey, RouteOffer,
    };
    let dir = tempfile::tempdir().unwrap();
    let (runtime, owner) = configured(dir.path());
    let allocation = runtime.configured_allocation().unwrap();
    let id = DeviceId {
        kind: DeviceKind::Gpu,
        key: 256,
    };
    let key = BackingKey {
        pool: [8; 32],
        generation: [9; 16],
    };
    let backing = PoolRecord {
        version: RECORD_VERSION,
        key,
        execution: Execution::ShardsDedicated,
        cost: allocation.cost,
        allocation: Some(allocation),
        devices: vec![id],
        phase: BackingPhase::Live,
        init: Some(hex_core::record::ProcId {
            pid: 42,
            start_ticks: 7,
        }),
        exit: None,
    }
    .offer()
    .unwrap();
    let image = ImagePlatform {
        os: ImageOs::Linux,
        arch: route::host_platform().unwrap().arch,
    };
    let native = RouteOffer {
        key: RouteKey {
            class: Class::Process,
            image,
            execution: Execution::Native,
            gpu: GpuEligibility::None,
        },
        cost: None,
        backing: None,
    };
    let selected = RouteOffer {
        key: RouteKey {
            class: Class::Shards,
            image,
            execution: Execution::ShardsDedicated,
            gpu: GpuEligibility::Required,
        },
        cost: Some(allocation.cost),
        backing: Some(key),
    };
    // Explicit input from the checked-facts boundary, not a catalog or native
    // renderer invocation. This unit tests publication of those exact values.
    let observed = DeviceInfo {
        id,
        model: "checked renderer model input".into(),
        size: hex_api::MIN_MEMORY,
        holder: Some(DeviceHolder::Backing(key)),
    };
    observed.check().unwrap();
    for previous in [
        None,
        Some(observed.try_clone().unwrap()),
        Some(DeviceInfo {
            model: "conflicting native model input".into(),
            ..observed.try_clone().unwrap()
        }),
    ] {
        let conflict = previous
            .as_ref()
            .is_some_and(|info| info.model != observed.model);
        let mut capacity = Books::capacity(owner.as_ref()).unwrap();
        capacity.routes = vec![native];
        capacity.devices = previous.into_iter().collect();
        capacity.device_routes = vec![0; capacity.devices.len()];
        capacity.check().unwrap();
        // add_offers stages the canonical backing immediately before its route;
        // that intermediate pair is checked only after push_offer completes it.
        capacity.backings = vec![backing.try_clone().unwrap()];
        let result = push_offer(&mut capacity, selected, &[observed.try_clone().unwrap()]);
        if conflict {
            assert!(
                result.is_err(),
                "the same physical identity accepted conflicting measured metadata"
            );
        } else {
            result.unwrap();
            assert_eq!(capacity.routes, vec![native, selected]);
            assert_eq!(
                capacity.devices,
                vec![observed.try_clone().unwrap()],
                "checked factual GPU disappeared because the host resource plane had no slot"
            );
            assert_eq!(capacity.device_routes, vec![2]);
            assert_eq!(capacity.backings, vec![backing.try_clone().unwrap()]);
            capacity.check().unwrap();
        }
    }
    assert_eq!(owner.launches.load(Ordering::SeqCst), 0);
    assert!(owner.pool.lock().unwrap().is_none());
    assert!(owner.boxes.lock().unwrap().is_empty());
}
