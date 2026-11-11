//! The one shared CPU VM consumes a real Index grant in the same Store/native
//! transaction as dedicated VMs. Its process receipt is not another resource ledger.

use std::path::Path;

use hex_api::{BackingGrant, BackingKey, BackingOffer, BackingPhase, Execution, LifecycleCause};
use hex_core::record::{NativeRecord, PoolRecord};
use sha2::{Digest, Sha256};

use super::pool::{self, Receipt, refused};
use super::selected::Phase;
use super::*;
use crate::devices::{self, Transaction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Binding {
    pub key: BackingKey,
    pub commitment: [u8; 32],
}

impl GuestRuntime {
    /// Read-only route/profile validation for the SAME owner's retained-record
    /// preflight. This performs no GPU query, file cleanup, launch or adoption.
    pub fn preflight_backing(&self, record: &PoolRecord) -> Result<(), Refusal> {
        record.check()?;
        let key = record.key;
        if record.execution != Execution::ShardsShared
            || !record.devices.is_empty()
            || self.shared_pool.get().is_some_and(|bound| *bound != key)
            || key.pool != self.pool_configuration(record.cost)?
        {
            return Err(refused(key, LifecycleCause::Uncertain));
        }
        let allocation = record
            .cost
            .allocate(hex_api::CapacityResources::default())?;
        let guest = allocation.guest()?;
        let configured = selected::resources(&self.config)?;
        if record
            .allocation
            .is_some_and(|retained| retained != allocation)
            || guest.cpu_milli != configured.cpu_milli
            || guest.mem_bytes != configured.mem_bytes
        {
            return Err(refused(key, LifecycleCause::Uncertain));
        }
        Ok(())
    }

    pub(super) fn managed_shared(&self) -> bool {
        self.uses_books() || self.runtime_cost.is_some()
    }

    fn pool_path(&self) -> Result<&Path, Refusal> {
        self.record.as_deref().ok_or_else(|| {
            route::invalid("managed shared backing needs its durable guest receipt path")
        })
    }

    fn bind_pool(&self, key: BackingKey) -> Result<(), Refusal> {
        if let Some(bound) = self.shared_pool.get() {
            return if *bound == key {
                Ok(())
            } else {
                Err(refused(key, LifecycleCause::Uncertain))
            };
        }
        self.shared_pool.set(key).or_else(|key| {
            if self.shared_pool.get() == Some(&key) {
                Ok(())
            } else {
                Err(refused(key, LifecycleCause::Uncertain))
            }
        })
    }

    /// Called after registration of the SAME Books owner, before capacity/recovery.
    /// Store persists Prepared/scope before Index may issue the grant. No VM starts.
    pub fn prepare_backing(&self) -> Result<BackingOffer, Refusal> {
        let authority = self.authority()?;
        if let Some(key) = self.shared_pool.get() {
            return self.pool_record(*key)?.offer();
        }
        let path = self.pool_path()?;
        if let Some(receipt) = pool::read(path)? {
            let key = receipt.original.key;
            let record = self.pool_record(key)?;
            receipt.matches(&record)?;
            self.preflight_backing(&record)?;
            if record.phase == BackingPhase::Released {
                if !matches!(receipt.phase, Phase::Released { .. }) {
                    return Err(refused(key, LifecycleCause::Uncertain));
                }
                let books = self
                    .books
                    .get()
                    .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
                devices::with_pool_transaction(books, &key, &[], |transaction| {
                    let current = transaction
                        .read_pool(&key)?
                        .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
                    receipt.matches(&current)?;
                    if current.phase != BackingPhase::Released {
                        return Err(refused(key, LifecycleCause::Uncertain));
                    }
                    remove_receipt(path)
                })?;
            } else {
                if record.execution != Execution::ShardsShared || !record.devices.is_empty() {
                    return Err(refused(key, LifecycleCause::Uncertain));
                }
                self.bind_pool(key)?;
                return record.offer();
            }
        }
        let allocation = self.configured_allocation()?;
        let pool = self.pool_configuration(allocation.cost)?;
        let offer = authority.prepare_pool(pool, Execution::ShardsShared, allocation.cost, &[])?;
        if offer.key.pool != pool || offer.cost != allocation.cost || !offer.devices.is_empty() {
            return Err(refused(offer.key, LifecycleCause::Uncertain));
        }
        self.preflight_backing(&self.pool_record(offer.key)?)?;
        self.bind_pool(offer.key)?;
        Ok(offer)
    }

    fn pool_configuration(&self, cost: hex_api::RuntimeCost) -> Result<[u8; 32], Refusal> {
        if self.config.shares.len() > shares::MAX_SHARES {
            return Err(route::invalid(
                "shared backing exceeds the configured share bound",
            ));
        }
        let paths = self.native_paths.as_ref().ok_or_else(|| {
            route::invalid("shared backing needs the configured native/probe path pair")
        })?;
        let mut hash = Sha256::new();
        let mut part = |bytes: &[u8]| -> Result<(), Refusal> {
            hash.update(
                u64::try_from(bytes.len())
                    .map_err(|_| route::invalid("backing configuration length overflows"))?
                    .to_be_bytes(),
            );
            hash.update(bytes);
            Ok(())
        };
        // Versioned domain separation is protocol identity, not a resource allowance.
        part(b"hex-shards-shared-backing/v1")?;
        part(&cost.id)?;
        for resources in [
            cost.floor,
            cost.ceiling,
            cost.quantum,
            cost.guest_overhead,
            cost.overhead,
        ] {
            for value in [
                resources.cpu_milli,
                resources.mem_bytes,
                resources.disk_bytes,
            ] {
                part(&value.to_be_bytes())?;
            }
        }
        for path in [
            &self.config.binary,
            &self.config.kernel,
            &self.config.initrd,
        ] {
            part(path.as_os_str().as_encoded_bytes())?;
        }
        part(paths.cache().as_os_str().as_encoded_bytes())?;
        part(paths.home().as_os_str().as_encoded_bytes())?;
        part(&self.config.vcpus.to_be_bytes())?;
        part(&self.config.memory_mib.to_be_bytes())?;
        part(self.config.staged.as_bytes())?;
        for share in &self.config.shares {
            part(share.dir.as_os_str().as_encoded_bytes())?;
            part(share.at.as_bytes())?;
            part(&[u8::from(share.read_only)])?;
        }
        Ok(hash.finalize().into())
    }

    fn pool_record(&self, key: BackingKey) -> Result<PoolRecord, Refusal> {
        self.authority()?
            .pool_records()?
            .into_iter()
            .find(|record| record.key == key)
            .ok_or_else(|| refused(key, LifecycleCause::Uncertain))
    }

    /// Recovery consumes the original already-admitted grant. It never invents
    /// a new generation or restarts an interrupted native launch.
    pub fn recover_backing(&self) -> Result<(), Refusal> {
        let key = *self
            .shared_pool
            .get()
            .ok_or_else(|| route::invalid("prepare the shared backing before recovery"))?;
        let mut record = self.pool_record(key)?;
        self.preflight_backing(&record)?;
        if matches!(
            record.phase,
            BackingPhase::Granted | BackingPhase::Live | BackingPhase::Draining
        ) && self.observe_ended_pool(&mut record)?
        {
            // Keep the native grant while ordinary recovery removes each inner
            // record using that same positive parent-scope absence witness.
            return Ok(());
        }
        match (record.phase, record.allocation) {
            (BackingPhase::Prepared | BackingPhase::Released, _) => Ok(()),
            (BackingPhase::Granted | BackingPhase::Live, Some(allocation)) => {
                if !record.devices.is_empty() {
                    return Err(refused(key, LifecycleCause::Uncertain));
                }
                self.initialize_pool(BackingGrant {
                    key,
                    allocation,
                    devices: Vec::new(),
                })?;
                Ok(())
            }
            _ => Err(refused(key, LifecycleCause::Uncertain)),
        }
    }

    fn observe_ended_pool(&self, record: &mut PoolRecord) -> Result<bool, Refusal> {
        let key = record.key;
        let books = self
            .books
            .get()
            .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
        devices::with_pool_transaction(books, &key, &[], |transaction| {
            let mut current = transaction
                .read_pool(&key)?
                .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
            if current.native_commitment()? != record.native_commitment()? {
                return Err(refused(key, LifecycleCause::Uncertain));
            }
            let Some(receipt) = pool::read(self.pool_path()?)? else {
                return Ok(false);
            };
            receipt.matches(&current)?;
            let identity = match &receipt.phase {
                Phase::Running(identity)
                | Phase::Stopping(identity)
                | Phase::Released {
                    identity: Some(identity),
                    ..
                } => {
                    managed::same_native(NativeRecord::Pool(&current), identity)?;
                    Some(identity)
                }
                Phase::Intent | Phase::Released { identity: None, .. }
                    if current.init.is_none() =>
                {
                    None
                }
                _ => return Err(refused(key, LifecycleCause::Uncertain)),
            };
            let native = transaction.attach_managed(identity)?;
            let terminal = native
                .terminal()
                .map_err(|why| refusal("observing shared backing during recovery", &why))?;
            let Some(terminal) =
                terminal.filter(|end| end.scope == shards_api::TerminalScope::Released)
            else {
                return Ok(false);
            };
            current.phase = BackingPhase::Draining;
            current.exit = Some(exit_of(terminal.status));
            transaction.put_pool(&current)?;
            *record = current;
            Ok(true)
        })
    }

    pub(super) fn initialize_pool(&self, grant: BackingGrant) -> Result<BackingOffer, Refusal> {
        grant.check()?;
        let key = grant.key;
        if self.shared_pool.get() != Some(&key) {
            return Err(refused(key, LifecycleCause::Pending));
        }
        let budget = managed::Budget::new(self.first_dial_bound()?)?;
        let books = self
            .books
            .get()
            .ok_or_else(|| refused(key, LifecycleCause::Pending))?;
        devices::with_pool_transaction(books, &key, &[], |transaction| {
            let record = transaction.grant_pool(&grant)?;
            self.preflight_backing(&record)?;
            if record.execution != Execution::ShardsShared || !record.devices.is_empty() {
                return Err(refused(key, LifecycleCause::Uncertain));
            }
            let booted = match self.running()? {
                Some(booted) => {
                    self.matches_pool(&booted, &record)?;
                    self.pool_alive(&booted, key)?;
                    booted
                }
                None => self.boot_pool(transaction, record, budget)?,
            };
            let mut record = transaction
                .read_pool(&key)?
                .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
            self.matches_pool(&booted, &record)?;
            if record.phase == BackingPhase::Granted {
                record.phase = BackingPhase::Live;
                transaction.put_pool(&record)?;
            }
            if record.phase != BackingPhase::Live {
                return Err(refused(key, LifecycleCause::Uncertain));
            }
            let offer = record.offer()?;
            let mut state = self.lock()?;
            if state
                .as_ref()
                .is_some_and(|known| known.backing != booted.backing)
            {
                return Err(refused(key, LifecycleCause::Uncertain));
            }
            *state = Some(booted);
            Ok(offer)
        })
    }

    fn boot_pool(
        &self,
        transaction: &dyn Transaction,
        record: PoolRecord,
        budget: managed::Budget,
    ) -> Result<triomphe::Arc<Booted>, Refusal> {
        let path = self.pool_path()?;
        let key = record.key;
        let mut receipt = match pool::read(path)? {
            Some(receipt) => {
                receipt.matches(&record)?;
                receipt
            }
            None if record.phase == BackingPhase::Granted && record.init.is_none() => {
                if record.allocation != Some(self.configured_allocation()?) {
                    return Err(refused(key, LifecycleCause::Uncertain));
                }
                Receipt::intent(record, Vec::new(), &self.config)?
            }
            None => return Err(refused(key, LifecycleCause::Uncertain)),
        };
        let binding = Binding {
            key,
            commitment: receipt.original.native_commitment()?,
        };
        let spec = self.pool_spec(&receipt)?;
        let socket = self.pool_socket(key)?;
        let plan = self.managed_plan(&spec, &socket)?;
        let guest = match &receipt.phase {
            Phase::Running(identity) => {
                let canonical = transaction
                    .read_pool(&key)?
                    .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
                managed::same_native(NativeRecord::Pool(&canonical), identity)?;
                if identity.vsock != socket {
                    return Err(refused(key, LifecycleCause::Uncertain));
                }
                managed::attach(transaction, plan, identity, budget)?
            }
            Phase::Intent => {
                // An existing Intent is an interrupted attempt, not permission
                // to spawn another controller or discard uncertain helpers.
                if pool::read(path)?.is_some() {
                    return Err(refused(key, LifecycleCause::Uncertain));
                }
                budget.remaining()?;
                pool::write(path, &receipt)?;
                managed::boot(transaction, plan, budget)?
            }
            Phase::Stopping(_) | Phase::Released { .. } => {
                return Err(refused(key, LifecycleCause::Uncertain));
            }
        };
        let identity = shards_api::Guest::identity(&guest)
            .map_err(|why| refusal("reading managed shared guest identity", &why))?;
        let canonical = transaction
            .read_pool(&key)?
            .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
        receipt.matches(&canonical)?;
        managed::same_native(NativeRecord::Pool(&canonical), &identity)?;
        if identity.vsock != socket {
            return Err(refused(key, LifecycleCause::Uncertain));
        }
        receipt.phase = Phase::Running(identity);
        pool::write(path, &receipt)?;
        let booted = self.connect_managed(guest, None, Some(binding), budget)?;
        let guest_capacity = booted
            .client
            .capacity(Class::Oci)?
            .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
        guest_capacity.check()?;
        let allocation = receipt
            .original
            .allocation
            .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
        let room = allocation
            .guest()?
            .checked_sub(allocation.cost.guest_overhead)
            .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
        if !guest_capacity.total.holds(room) {
            return Err(refused(key, LifecycleCause::Uncertain));
        }
        budget.remaining()?;
        Ok(booted)
    }

    fn pool_spec(&self, receipt: &Receipt) -> Result<shards_api::GuestSpec, Refusal> {
        receipt.check()?;
        let guest = receipt
            .original
            .allocation
            .ok_or_else(|| refused(receipt.original.key, LifecycleCause::Uncertain))?
            .guest()?;
        if guest.cpu_milli.checked_rem(1000) != Some(0)
            || guest.mem_bytes.checked_rem(1 << 20) != Some(0)
        {
            return Err(refused(receipt.original.key, LifecycleCause::Uncertain));
        }
        let mut spec = self.cpu_spec();
        spec.vcpus = u32::try_from(
            guest
                .cpu_milli
                .checked_div(1000)
                .ok_or_else(|| refused(receipt.original.key, LifecycleCause::Uncertain))?,
        )
        .map_err(|_| refused(receipt.original.key, LifecycleCause::Uncertain))?;
        spec.memory_mib = guest
            .mem_bytes
            .checked_div(1 << 20)
            .ok_or_else(|| refused(receipt.original.key, LifecycleCause::Uncertain))?;
        Ok(spec)
    }

    fn pool_socket(&self, key: BackingKey) -> Result<PathBuf, Refusal> {
        use std::fmt::Write as _;
        let parent = self
            .pool_path()?
            .parent()
            .ok_or_else(|| route::invalid("shared backing receipt has no parent"))?;
        let mut path = super::dedicated::copy_path(parent)?;
        let mut name = String::new();
        let digits = key
            .generation
            .len()
            .checked_mul(2)
            .ok_or_else(|| route::invalid("pool scope name overflows"))?;
        let length = "hex-pool-"
            .len()
            .checked_add(digits)
            .and_then(|length| length.checked_add(".sock".len()))
            .ok_or_else(|| route::invalid("pool socket name overflows"))?;
        name.try_reserve_exact(length)
            .map_err(|_| Refusal::Exhausted {
                what: "shared guest socket name",
                bound: length,
            })?;
        name.push_str("hex-pool-");
        for byte in key.generation {
            write!(&mut name, "{byte:02x}")
                .map_err(|_| route::invalid("pool scope cannot be formatted"))?;
        }
        name.push_str(".sock");
        path.try_reserve_exact(
            length
                .checked_add(1)
                .ok_or_else(|| route::invalid("pool socket path overflows"))?,
        )
        .map_err(|_| Refusal::Exhausted {
            what: "shared guest socket path",
            bound: length,
        })?;
        path.push(name);
        Ok(path)
    }

    fn matches_pool(&self, booted: &Booted, record: &PoolRecord) -> Result<(), Refusal> {
        if booted.backing
            != Some(Binding {
                key: record.key,
                commitment: record.native_commitment()?,
            })
        {
            return Err(refused(record.key, LifecycleCause::Uncertain));
        }
        Ok(())
    }

    fn pool_alive(&self, booted: &Booted, key: BackingKey) -> Result<(), Refusal> {
        let exited = booted
            .guest
            .process_exited()
            .map_err(|why| refusal("observing shared controller", &why))?;
        let scope = booted
            .guest
            .terminal_scope()
            .map_err(|why| refusal("observing shared native scope", &why))?;
        if exited || scope.is_some() {
            return Err(refused(key, LifecycleCause::Uncertain));
        }
        Ok(())
    }

    pub(super) fn shared_running(&self) -> Result<triomphe::Arc<Booted>, Refusal> {
        let key = *self
            .shared_pool
            .get()
            .ok_or_else(|| route::invalid("shared backing has not been prepared and granted"))?;
        let record = self.pool_record(key)?;
        if record.phase != BackingPhase::Live {
            return Err(refused(key, LifecycleCause::Pending));
        }
        let booted = self
            .running()?
            .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
        self.matches_pool(&booted, &record)?;
        self.pool_alive(&booted, key)?;
        Ok(booted)
    }

    /// A retained canonical parent scope, rather than an absent client/cache or
    /// controller PID, proves an inner payload gone after an unexpected VM end.
    pub(super) fn shared_for_record(
        &self,
        boxed: &hex_core::record::BoxRecord,
    ) -> Result<Option<triomphe::Arc<Booted>>, Refusal> {
        let key = boxed
            .spec
            .route
            .filter(|route| route.key.execution == Execution::ShardsShared)
            .and_then(|route| route.backing)
            .ok_or(super::books::uncertain(&boxed.spec.id)?)?;
        if self.shared_pool.get() != Some(&key) {
            return Err(super::books::uncertain(&boxed.spec.id)?);
        }
        let record = self.pool_record(key)?;
        record.check()?;
        if record.execution != Execution::ShardsShared || !record.devices.is_empty() {
            return Err(refused(key, LifecycleCause::Uncertain));
        }
        if record.phase == BackingPhase::Released {
            return Ok(None);
        }
        if let Some(booted) = self.running()? {
            self.matches_pool(&booted, &record)?;
            let scope = booted
                .guest
                .terminal_scope()
                .map_err(|why| refusal("observing original shared parent scope", &why))?;
            if scope == Some(shards_api::TerminalScope::Released) {
                return Ok(None);
            }
            if record.phase != BackingPhase::Live {
                return Err(refused(key, LifecycleCause::Pending));
            }
            self.pool_alive(&booted, key)?;
            return Ok(Some(booted));
        }
        let books = self
            .books
            .get()
            .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
        devices::with_pool_transaction(books, &key, &[], |transaction| {
            let current = transaction
                .read_pool(&key)?
                .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
            if current.native_commitment()? != record.native_commitment()? {
                return Err(refused(key, LifecycleCause::Uncertain));
            }
            let receipt = pool::read(self.pool_path()?)?
                .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
            receipt.matches(&current)?;
            let identity = match &receipt.phase {
                Phase::Running(identity)
                | Phase::Stopping(identity)
                | Phase::Released {
                    identity: Some(identity),
                    ..
                } => {
                    managed::same_native(NativeRecord::Pool(&current), identity)?;
                    Some(identity)
                }
                Phase::Intent | Phase::Released { identity: None, .. }
                    if current.init.is_none() =>
                {
                    None
                }
                _ => return Err(refused(key, LifecycleCause::Uncertain)),
            };
            let native = transaction.attach_managed(identity)?;
            let terminal = native
                .terminal()
                .map_err(|why| refusal("observing cold shared parent scope", &why))?;
            if terminal.is_some_and(|end| end.scope == shards_api::TerminalScope::Released) {
                return Ok(None);
            }
            Err(refused(key, LifecycleCause::Uncertain))
        })
    }

    /// Native VM/helpers counters for the existing host usage sampler. They are
    /// a different measurement plane from each inner OCI workload's counters.
    pub fn backing_usage(&self, key: &BackingKey) -> Result<Usage, Refusal> {
        let key = *key;
        if self.shared_pool.get() != Some(&key) {
            return Err(refused(key, LifecycleCause::Uncertain));
        }
        let books = self
            .books
            .get()
            .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
        devices::with_pool_transaction(books, &key, &[], |transaction| {
            let record = transaction
                .read_pool(&key)?
                .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
            record.check()?;
            if !matches!(
                record.phase,
                BackingPhase::Granted | BackingPhase::Live | BackingPhase::Draining
            ) {
                return Err(refused(key, LifecycleCause::Pending));
            }
            transaction.managed_usage()
        })
    }

    pub(super) fn add_offers(
        &self,
        capacity: &mut hex_api::Capacity,
        class: Class,
    ) -> Result<(), Refusal> {
        use hex_api::{GpuEligibility, ImageOs, ImagePlatform, RouteKey, RouteOffer};
        capacity.check()?;
        let allocation = self.configured_allocation()?;
        let image = ImagePlatform {
            os: ImageOs::Linux,
            arch: route::host_platform()?.arch,
        };
        if capacity.routes.is_empty() {
            capacity
                .device_routes
                .try_reserve_exact(capacity.devices.len())
                .map_err(|_| Refusal::Exhausted {
                    what: "physical route eligibility",
                    bound: hex_api::MAX_DEVICES,
                })?;
            capacity.device_routes.resize(capacity.devices.len(), 0);
        }
        if class == Class::Oci {
            let key = *self.shared_pool.get().ok_or_else(|| {
                route::invalid("prepare shared backing before advertising its route")
            })?;
            let record = self.pool_record(key)?;
            if record.execution != Execution::ShardsShared || !record.devices.is_empty() {
                return Err(refused(key, LifecycleCause::Uncertain));
            }
            if !capacity.backings.iter().any(|backing| backing.key == key) {
                capacity
                    .backings
                    .try_reserve_exact(1)
                    .map_err(|_| Refusal::Exhausted {
                        what: "initialized native backings",
                        bound: hex_api::MAX_RUNTIME_ROUTES,
                    })?;
                capacity.backings.push(record.offer()?);
            }
            push_offer(
                capacity,
                RouteOffer {
                    key: RouteKey {
                        class,
                        image,
                        execution: Execution::ShardsShared,
                        gpu: GpuEligibility::None,
                    },
                    cost: None,
                    backing: Some(key),
                },
                &[],
            )?;
        } else if class == Class::Shards {
            push_offer(
                capacity,
                RouteOffer {
                    key: RouteKey {
                        class,
                        image,
                        execution: Execution::ShardsDedicated,
                        gpu: GpuEligibility::None,
                    },
                    cost: Some(allocation.cost),
                    backing: None,
                },
                &[],
            )?;
        } else {
            return Err(route::invalid(
                "Shards offers require OCI or explicit Shards class",
            ));
        }
        // Cold provisioning IDs are separate from this measured canonical list.
        // They get no model/size/eligibility until the real backing join reports it.
        let mut ids = Vec::new();
        if self.gpu_catalog.is_some() {
            let entries = self.physical_entries()?;
            ids.try_reserve_exact(capacity.devices.len())
                .map_err(|_| Refusal::Exhausted {
                    what: "Shards eligibility join",
                    bound: hex_api::MAX_DEVICES,
                })?;
            ids.extend(
                capacity
                    .devices
                    .iter()
                    .filter(|device| {
                        entries
                            .binary_search_by_key(&device.id, |entry| entry.info.id)
                            .is_ok()
                    })
                    .map(|device| device.id),
            );
        }
        ids.sort_unstable();
        let physical = self.catalog_physical_if_any(&ids)?;
        let eligible = self.checked_physical(&ids, &physical)?;
        if !eligible.is_empty() {
            push_offer(
                capacity,
                RouteOffer {
                    key: RouteKey {
                        class,
                        image,
                        execution: Execution::ShardsDedicated,
                        gpu: GpuEligibility::Required,
                    },
                    cost: Some(allocation.cost),
                    backing: None,
                },
                &eligible,
            )?;
        }
        // There is no class-wide VM cost: explicit route+backing describes it.
        capacity.cost = None;
        capacity.check()
    }

    fn catalog_physical_if_any(
        &self,
        ids: &[hex_api::DeviceId],
    ) -> Result<Vec<shards_api::PhysicalGpu>, Refusal> {
        if ids.is_empty() {
            Ok(Vec::new())
        } else {
            self.catalog_physical(ids)
        }
    }

    pub(super) fn create_shared(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        let route = spec
            .route
            .ok_or_else(|| route::invalid("shared OCI needs its admitted host route"))?;
        let key = route
            .backing
            .ok_or_else(|| route::invalid("shared OCI needs its admitted backing generation"))?;
        if route.key.execution != Execution::ShardsShared
            || self.shared_pool.get() != Some(&key)
            || !spec.devices.is_empty()
            || spec.allocation.is_some()
        {
            return Err(refused(key, LifecycleCause::Uncertain));
        }
        let books = self
            .books
            .get()
            .ok_or_else(|| refused(key, LifecycleCause::Pending))?;
        devices::with_pool_transaction(books, &key, &[], |transaction| {
            let record = transaction
                .read_pool(&key)?
                .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
            if record.phase != BackingPhase::Live {
                return Err(refused(key, LifecycleCause::Pending));
            }
            let booted = self.shared_running()?;
            self.matches_pool(&booted, &record)?;
            let rewritten = self.rewrite_spec(spec)?;
            self.create_in(&booted, rewritten)
        })
    }

    pub(super) fn release_pool(&self, key: BackingKey) -> Result<BackingOffer, Refusal> {
        if self.shared_pool.get() != Some(&key) {
            return Err(refused(key, LifecycleCause::Uncertain));
        }
        let books = self
            .books
            .get()
            .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
        devices::with_pool_transaction(books, &key, &[], |transaction| {
            let mut record = transaction
                .read_pool(&key)?
                .ok_or_else(|| refused(key, LifecycleCause::Uncertain))?;
            if record.execution != Execution::ShardsShared || !record.devices.is_empty() {
                return Err(refused(key, LifecycleCause::Uncertain));
            }
            if record.phase == BackingPhase::Released {
                return record.offer();
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
            let path = self.pool_path()?;
            if record.phase == BackingPhase::Prepared {
                if pool::read(path)?.is_some() {
                    return Err(refused(key, LifecycleCause::Uncertain));
                }
                record.phase = BackingPhase::Released;
                transaction.put_pool(&record)?;
                return record.offer();
            }
            let mut receipt = match pool::read(path)? {
                Some(receipt) => receipt,
                None if record.init.is_none() => {
                    let mut original = PoolRecord::decode(&record.encode()?)?;
                    original.phase = BackingPhase::Granted;
                    original.exit = None;
                    Receipt::retained(original, Vec::new())?
                }
                None => return Err(refused(key, LifecycleCause::Uncertain)),
            };
            receipt.matches(&record)?;
            record.phase = BackingPhase::Draining;
            transaction.put_pool(&record)?;
            let identity = match &receipt.phase {
                Phase::Running(identity)
                | Phase::Stopping(identity)
                | Phase::Released {
                    identity: Some(identity),
                    ..
                } => {
                    managed::same_native(NativeRecord::Pool(&record), identity)?;
                    Some(identity)
                }
                Phase::Intent | Phase::Released { identity: None, .. } if record.init.is_none() => {
                    None
                }
                _ => return Err(refused(key, LifecycleCause::Uncertain)),
            };
            let terminal = match &receipt.phase {
                Phase::Released { status, .. } => *status,
                _ => {
                    let identity = identity.map(copy_identity).transpose()?;
                    if let Some(identity) = &identity {
                        receipt.phase = Phase::Stopping(copy_identity(identity)?);
                        pool::write(path, &receipt)?;
                    }
                    let terminal = managed::stop_scope(
                        transaction,
                        identity.as_ref(),
                        self.first_dial_bound()?,
                        refused(key, LifecycleCause::Uncertain),
                    )?;
                    receipt.phase = Phase::Released {
                        identity,
                        status: terminal.status,
                    };
                    pool::write(path, &receipt)?;
                    terminal.status
                }
            };
            let identity = match &receipt.phase {
                Phase::Released { identity, .. } => identity.as_ref(),
                _ => return Err(refused(key, LifecycleCause::Uncertain)),
            };
            transaction.finalize_managed(identity)?;
            record.exit = Some(exit_of(terminal));
            record.phase = BackingPhase::Released;
            transaction.put_pool(&record)?;
            {
                let mut state = self.lock()?;
                if state
                    .as_ref()
                    .is_some_and(|booted| booted.backing.is_some_and(|backing| backing.key == key))
                {
                    *state = None;
                }
            }
            remove_receipt(path)?;
            record.offer()
        })
    }
}

fn remove_receipt(path: &Path) -> Result<(), Refusal> {
    held::remove(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| route::invalid("shared backing receipt has no parent"))?;
    held::sync_folder(parent)
        .map_err(|why| Refusal::os("syncing removed shared guest receipt", why))
}

fn copy_identity(identity: &shards_api::Identity) -> Result<shards_api::Identity, Refusal> {
    Ok(shards_api::Identity {
        pid: identity.pid,
        started: identity.started,
        vsock: super::dedicated::copy_path(&identity.vsock)?,
    })
}

fn exit_of(status: shards_api::ManagedStatus) -> hex_api::Exit {
    match status {
        shards_api::ManagedStatus::Code(code) => hex_api::Exit::Code(code),
        shards_api::ManagedStatus::Signal(signal) => hex_api::Exit::Signal(signal),
        shards_api::ManagedStatus::Unknown => hex_api::Exit::Unknown,
    }
}

fn push_offer(
    capacity: &mut hex_api::Capacity,
    offer: hex_api::RouteOffer,
    eligible: &[hex_api::DeviceInfo],
) -> Result<(), Refusal> {
    offer.check()?;
    if capacity.routes.iter().any(|known| known.key == offer.key) {
        return Err(route::invalid(
            "an initialized route was advertised more than once",
        ));
    }
    if capacity.routes.len() >= hex_api::MAX_RUNTIME_ROUTES {
        return Err(Refusal::Exhausted {
            what: "initialized runtime routes",
            bound: hex_api::MAX_RUNTIME_ROUTES,
        });
    }
    let bit = u32::try_from(capacity.routes.len())
        .ok()
        .and_then(|position| 1u64.checked_shl(position))
        .ok_or_else(|| route::invalid("route eligibility word overflows"))?;
    capacity
        .routes
        .try_reserve_exact(1)
        .map_err(|_| Refusal::Exhausted {
            what: "initialized runtime routes",
            bound: hex_api::MAX_RUNTIME_ROUTES,
        })?;
    capacity.routes.push(offer);
    if capacity.device_routes.len() != capacity.devices.len() {
        return Err(route::invalid(
            "route eligibility is not paired with canonical devices",
        ));
    }
    for (device, mask) in capacity.devices.iter().zip(&mut capacity.device_routes) {
        if eligible
            .binary_search_by_key(&device.id, |candidate| candidate.id)
            .is_ok()
        {
            *mask |= bit;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "shared_tests.rs"]
mod tests;
