//! A charged cold observer is a canonical native PoolRecord, never a workload or inventory cache.

use super::*;
use hex_api::{
    BackingGrant, BackingKey, BackingOffer, BackingPhase, CapacityResources, Execution,
    PhysicalDevice, RuntimeCost,
};
use hex_core::record::{NativeRecord, PoolRecord};
use sha2::{Digest, Sha256};

fn refused(key: BackingKey, cause: LifecycleCause) -> Refusal {
    Refusal::Backing { key, cause }
}

fn observer_cost(config: &Config) -> Result<RuntimeCost, Refusal> {
    let cost = RuntimeCost {
        ceiling: config.cost.floor,
        ..config.cost
    };
    cost.check()?;
    let allocation = cost.allocate(CapacityResources::default())?;
    allocation.guest()?;
    Ok(cost)
}

fn observer_configuration(config: &Config, claims: &[DeviceId]) -> Result<[u8; 32], Refusal> {
    config.check()?;
    if claims.len() > hex_api::MAX_DEVICES
        || claims
            .windows(2)
            .any(|pair| matches!(pair, [a,b] if a >= b))
    {
        return Err(invalid(
            "observer claims must be bounded canonical unique GPU identities",
        ));
    }
    let cost = observer_cost(config)?;
    let mut hash = Sha256::new();
    let mut part = |bytes: &[u8]| -> Result<(), Refusal> {
        hash.update(
            u64::try_from(bytes.len())
                .map_err(|_| invalid("observer configuration length overflows"))?
                .to_be_bytes(),
        );
        hash.update(bytes);
        Ok(())
    };
    part(b"hex-firecracker-observer/v1")?;
    part(config.pool_id.as_bytes())?;
    part(&config.binary_sha256)?;
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
        &config.binary,
        &config.kernel,
        &config.initrd,
        &config.state_dir,
    ] {
        part(path.as_os_str().as_encoded_bytes())?;
    }
    for id in claims {
        if id.kind != DeviceKind::Gpu {
            return Err(invalid("observer provisioning claim is not a GPU"));
        }
        part(&id.key.to_be_bytes())?;
    }
    Ok(hash.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn config() -> Config {
        let guest = CapacityResources {
            cpu_milli: 1_000,
            mem_bytes: 64 << 20,
            disk_bytes: 0,
        };
        Config {
            binary: "/fixture/firecracker".into(),
            binary_sha256: [1; 32],
            kernel: "/fixture/kernel".into(),
            initrd: "/fixture/initrd".into(),
            state_dir: "/fixture/state".into(),
            pool_id: "configured fixture pool".into(),
            boot_timeout: Duration::from_secs(3),
            stop_timeout: Duration::from_secs(1),
            operation_timeout: Duration::from_secs(1),
            cost_evidence: "/fixture/evidence".into(),
            cost: RuntimeCost {
                id: [2; 32],
                floor: guest,
                ceiling: CapacityResources {
                    cpu_milli: 2_000,
                    mem_bytes: 128 << 20,
                    disk_bytes: 0,
                },
                quantum: CapacityResources {
                    cpu_milli: 1_000,
                    mem_bytes: 1 << 20,
                    disk_bytes: 1,
                },
                guest_overhead: CapacityResources {
                    mem_bytes: 3 << 20,
                    ..CapacityResources::default()
                },
                overhead: CapacityResources {
                    cpu_milli: 100,
                    mem_bytes: 5 << 20,
                    disk_bytes: 0,
                },
            },
            stage_limits: hex_core::stream::files::Limits {
                entries: 10,
                bytes: 1 << 20,
                path: 100,
            },
        }
    }

    /// AC-T7.4.1: a metadata-only cold observer's fixed charge carries the full physical set;
    /// changing config, executable or a selected identity cannot reuse its configuration key.
    #[test]
    fn cold_observer_configuration_binds_full_claims_binary_and_fixed_profile() {
        let mut config = config();
        let claims = [
            DeviceId {
                kind: DeviceKind::Gpu,
                key: 1,
            },
            DeviceId {
                kind: DeviceKind::Gpu,
                key: 2,
            },
        ];
        let original = observer_configuration(&config, &claims).unwrap();
        assert_ne!(
            original,
            observer_configuration(&config, &claims[..1]).unwrap()
        );
        config.binary_sha256 = [3; 32];
        assert_ne!(original, observer_configuration(&config, &claims).unwrap());
        config.binary_sha256 = [1; 32];
        config.cost.overhead.mem_bytes += 1 << 20;
        assert_ne!(original, observer_configuration(&config, &claims).unwrap());
        assert!(observer_configuration(&config, &[claims[1], claims[0]]).is_err());
        assert!(observer_configuration(&config, &[claims[0], claims[0]]).is_err());
        assert!(
            observer_configuration(
                &config,
                &[DeviceId {
                    kind: DeviceKind::Vfio,
                    key: 1
                }]
            )
            .is_err()
        );
    }

    /// AC-T7.4.1: the configured guest consumes its granted envelope, with one host allowance and
    /// no invented BoxSpec/row. These synthetic unit values are not measured runtime costs.
    #[test]
    fn cold_observer_machine_uses_granted_guest_envelope_and_charges_host_once() {
        let config = config();
        let cost = observer_cost(&config).unwrap();
        assert_eq!(cost.floor, cost.ceiling);
        let allocation = cost.allocate(CapacityResources::default()).unwrap();
        let value = serde_json::to_value(
            config
                .machine_guest(
                    allocation.guest().unwrap(),
                    Path::new("/fixture/vsock"),
                    &[],
                )
                .unwrap(),
        )
        .unwrap();
        assert_eq!(value["machine-config"]["vcpu_count"], 1);
        assert_eq!(value["machine-config"]["mem_size_mib"], 64);
        assert_eq!(allocation.resources.cpu_milli, 1_100);
        assert_eq!(allocation.resources.mem_bytes, 69 << 20);
        let mut malformed = config;
        malformed.cost.floor.cpu_milli = 500;
        assert!(observer_cost(&malformed).is_err());
    }
}

impl FirecrackerRuntime {
    fn observer_claims(&self) -> Result<Vec<DeviceId>, Refusal> {
        let mut claims = Vec::new();
        let count = self
            .catalog
            .entries_for(Route::Vfio)
            .filter(|entry| entry.info.id.kind == DeviceKind::Gpu)
            .count();
        if count > hex_api::MAX_DEVICES {
            return Err(exhausted("observer physical claims", hex_api::MAX_DEVICES));
        }
        claims
            .try_reserve_exact(count)
            .map_err(|_| exhausted("observer physical claims", count))?;
        claims.extend(
            self.catalog
                .entries_for(Route::Vfio)
                .filter(|entry| entry.info.id.kind == DeviceKind::Gpu)
                .map(|entry| entry.info.id),
        );
        claims.sort_unstable();
        Ok(claims)
    }

    /// Called only after global retained preflight. Persists scope/identity authority without
    /// touching a GPU, opening another provider, granting resources or starting a VM.
    pub fn prepare_backing(&self) -> Result<Option<BackingOffer>, Refusal> {
        self.require_initialized()?;
        let claims = self.observer_claims()?;
        if claims.is_empty() {
            return Ok(None);
        }
        let pool = observer_configuration(&self.config, &claims)?;
        let offer = self.authority()?.prepare_pool(
            pool,
            Execution::Firecracker,
            observer_cost(&self.config)?,
            &claims,
        )?;
        if offer.key.pool != pool
            || offer.cost != observer_cost(&self.config)?
            || offer.devices != claims
        {
            return Err(refused(offer.key, LifecycleCause::Uncertain));
        }
        Ok(Some(offer))
    }

    /// Pure second pass for the same owner's all-record preflight, before any native callback.
    pub fn preflight_backing(&self, record: &PoolRecord) -> Result<(), Refusal> {
        record.check()?;
        let bad = || refused(record.key, LifecycleCause::Uncertain);
        if record.execution != Execution::Firecracker
            || record.cost != observer_cost(&self.config)?
            || record.devices != self.observer_claims()?
            || record.key.pool != observer_configuration(&self.config, &record.devices)?
        {
            return Err(bad());
        }
        if let Some(allocation) = record.allocation {
            if allocation != record.cost.allocate(CapacityResources::default())? {
                return Err(bad());
            }
        }
        Ok(())
    }

    pub(super) fn observer_record(&self) -> Result<Option<PoolRecord>, Refusal> {
        let claims = self.observer_claims()?;
        if claims.is_empty() {
            return Ok(None);
        }
        let key = observer_configuration(&self.config, &claims)?;
        let record = self
            .authority()?
            .pool_records()?
            .into_iter()
            .find(|record| record.key.pool == key);
        if let Some(record) = &record {
            self.preflight_backing(record)?;
        }
        Ok(record)
    }

    fn pool_paths(&self, record: &PoolRecord) -> Result<Paths, Refusal> {
        let scope = crate::devices::managed_native_scope_id(NativeRecord::Pool(record))?;
        self.scope_paths(&scope)
    }

    fn pool_identity(&self, record: &PoolRecord) -> Result<Option<shards_api::Identity>, Refusal> {
        record
            .init
            .map(|init| {
                Ok(shards_api::Identity {
                    pid: init.pid,
                    started: init.start_ticks,
                    vsock: self.pool_paths(record)?.vsock,
                })
            })
            .transpose()
    }

    fn pool_client(
        &self,
        record: &PoolRecord,
        transaction: &dyn Transaction,
        until: Option<Instant>,
    ) -> Result<Client, Refusal> {
        self.preflight_backing(record)?;
        if !matches!(
            record.phase,
            BackingPhase::Granted | BackingPhase::Live | BackingPhase::Draining
        ) || record.init.is_none()
        {
            return Err(refused(record.key, LifecycleCause::Pending));
        }
        let identity = self.pool_identity(record)?;
        let native = transaction.attach_managed(identity.as_ref())?;
        if native
            .terminal()
            .map_err(|_| refused(record.key, LifecycleCause::Uncertain))?
            .is_some()
        {
            return Err(refused(record.key, LifecycleCause::Uncertain));
        }
        let link = own_link(transport::Endpoint {
            path: self.pool_paths(record)?.vsock,
            timeout: self.config.operation_timeout,
            create_deadline: until,
        })?;
        Client::connect(link)
    }

    fn pool_observations(
        &self,
        record: &PoolRecord,
        client: &Client,
        until: Option<Instant>,
    ) -> Result<Vec<PhysicalDevice>, Refusal> {
        let paths = self.pool_paths(record)?;
        let mut assignments = Vec::new();
        assignments
            .try_reserve_exact(record.devices.len())
            .map_err(|_| exhausted("observer PCI witnesses", record.devices.len()))?;
        for selected in &record.devices {
            let entry = self
                .catalog
                .entries_for(Route::Vfio)
                .find(|entry| entry.info.id == *selected)
                .ok_or_else(|| refused(record.key, LifecycleCause::Uncertain))?;
            let Some(Binding::Vfio { sbdf, .. }) = entry.binding(Route::Vfio) else {
                return Err(refused(record.key, LifecycleCause::Uncertain));
            };
            let id = vfio::function_id(sbdf)?;
            let timeout = until
                .map(remaining)
                .transpose()?
                .map_or(self.config.operation_timeout, |left| {
                    left.min(self.config.operation_timeout)
                });
            let witness = transport::attachment(&paths.api, &id, timeout)?;
            if witness.id != id || witness.sbdf != *sbdf {
                return Err(refused(record.key, LifecycleCause::Uncertain));
            }
            let guest = hex_gpu::PciAddress::parse(&witness.guest_sbdf)
                .ok_or_else(|| refused(record.key, LifecycleCause::Uncertain))?;
            assignments.push(observations::Assignment {
                host: *selected,
                guest: DeviceId {
                    kind: DeviceKind::Gpu,
                    key: hex_gpu::Identity::Pci(guest).key(),
                },
            });
        }
        let report = client.physical_report()?;
        let joined = observations::join(&assignments, &report)?;
        if let Some(until) = until {
            remaining(until)?;
        }
        Ok(joined)
    }

    /// Transient factual native observation, with no capacity/report recursion or cached inventory.
    /// The outer Node supplies its existing authority; this adapter does not grant any holder here.
    pub fn physical_observations(&self) -> Result<Vec<PhysicalDevice>, Refusal> {
        self.physical_observations_before(None)
    }

    pub(super) fn physical_observations_before(
        &self,
        until: Option<Instant>,
    ) -> Result<Vec<PhysicalDevice>, Refusal> {
        let Some(before) = self.observer_record()? else {
            return Ok(Vec::new());
        };
        if before.phase != BackingPhase::Live {
            return Ok(Vec::new());
        }
        let keys = self.catalog.conflict_ids(&before.devices)?;
        crate::devices::with_pool_transaction(&self.books, &before.key, &keys, |transaction| {
            let record = transaction
                .read_pool(&before.key)?
                .ok_or_else(|| refused(before.key, LifecycleCause::Uncertain))?;
            if record != before {
                return Err(refused(before.key, LifecycleCause::Uncertain));
            }
            let client = self.pool_client(&record, transaction, until)?;
            self.pool_observations(&record, &client, until)
        })
    }

    pub(super) fn initialize_observer(&self, grant: BackingGrant) -> Result<BackingOffer, Refusal> {
        grant.check()?;
        let keys = self.catalog.conflict_ids(&grant.devices)?;
        let until = Instant::now()
            .checked_add(self.config.boot_timeout)
            .ok_or_else(|| invalid("observer boot deadline overflows"))?;
        crate::devices::with_pool_transaction(&self.books, &grant.key, &keys, |transaction| {
            let before = transaction
                .read_pool(&grant.key)?
                .ok_or_else(|| refused(grant.key, LifecycleCause::Pending))?;
            self.preflight_backing(&before)?;
            if before.phase != BackingPhase::Prepared {
                // A repeated/interrupted Intent never starts another incarnation. Only a positively
                // observed retained Live scope may answer idempotently.
                if before.phase != BackingPhase::Live {
                    return Err(refused(grant.key, LifecycleCause::Uncertain));
                }
                let mut accepted = before.offer()?;
                let retained = before
                    .allocation
                    .ok_or_else(|| refused(grant.key, LifecycleCause::Uncertain))?;
                if retained != grant.allocation || before.devices != grant.devices {
                    return Err(refused(grant.key, LifecycleCause::Uncertain));
                }
                let client = self.pool_client(&before, transaction, Some(until))?;
                self.pool_observations(&before, &client, Some(until))?;
                accepted.phase = BackingPhase::Live;
                return Ok(accepted);
            }
            let record = transaction.grant_pool(&grant)?;
            self.preflight_backing(&record)?;
            self.config.verify_inputs()?;
            let plan = vfio::plan(&self.catalog, &grant.devices)?;
            let paths = self.pool_paths(&record)?;
            match fs::symlink_metadata(&paths.dir) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(Refusal::os("checking observer incarnation path", error)),
                Ok(_) => return Err(refused(grant.key, LifecycleCause::Uncertain)),
            }
            let vm = self
                .config
                .machine_guest(grant.allocation.guest()?, &paths.vsock, &plan)?;
            let program = self.vm_program(&paths)?;
            let io = Self::vm_io()?;
            fs::create_dir(&paths.dir)
                .and_then(|()| fs::set_permissions(&paths.dir, fs::Permissions::from_mode(0o700)))
                .map_err(|error| Refusal::os("creating observer incarnation directory", error))?;
            write_config(&paths.config, &vm)?;
            let ready = UnixListener::bind(&paths.ready)
                .map_err(|error| Refusal::os("binding observer readiness", error))?;
            let native = transaction.launch_managed(
                &ManagedLaunch::Firecracker {
                    program: &program,
                    vsock: &paths.vsock,
                },
                io,
                remaining(until)?,
            )?;
            let mut current = transaction
                .read_pool(&grant.key)?
                .ok_or_else(|| refused(grant.key, LifecycleCause::Uncertain))?;
            if current.key != record.key
                || current.cost != record.cost
                || current.devices != record.devices
                || current.allocation != record.allocation
                || current.phase != BackingPhase::Granted
                || current.init.is_none()
            {
                return Err(refused(grant.key, LifecycleCause::Uncertain));
            }
            transport::ready(&ready, native.as_ref(), remaining(until)?)?;
            let client = self.pool_client(&current, transaction, Some(until))?;
            self.pool_observations(&current, &client, Some(until))?;
            current.phase = BackingPhase::Live;
            transaction.put_pool(&current)?;
            current.offer()
        })
    }

    pub(super) fn release_observer(&self, key: &BackingKey) -> Result<BackingOffer, Refusal> {
        self.release_observer_before(key, None)
    }

    pub(super) fn release_observer_before(
        &self,
        key: &BackingKey,
        until: Option<Instant>,
    ) -> Result<BackingOffer, Refusal> {
        let before = self
            .observer_record()?
            .filter(|record| record.key == *key)
            .ok_or_else(|| refused(*key, LifecycleCause::Uncertain))?;
        let keys = self.catalog.conflict_ids(&before.devices)?;
        crate::devices::with_pool_transaction(&self.books, key, &keys, |transaction| {
            let mut record = transaction
                .read_pool(key)?
                .ok_or_else(|| refused(*key, LifecycleCause::Uncertain))?;
            self.preflight_backing(&record)?;
            if record.phase == BackingPhase::Released {
                return record.offer();
            }
            if record.phase == BackingPhase::Prepared {
                record.phase = BackingPhase::Released;
                transaction.put_pool(&record)?;
                return record.offer();
            }
            record.phase = BackingPhase::Draining;
            transaction.put_pool(&record)?;
            let identity = self.pool_identity(&record)?;
            let native = transaction.attach_managed(identity.as_ref())?;
            native
                .stop()
                .map_err(|_| refused(*key, LifecycleCause::Uncertain))?;
            let patience = until
                .map(remaining)
                .transpose()?
                .map_or(self.config.stop_timeout, |left| {
                    left.min(self.config.stop_timeout)
                });
            let terminal = native
                .wait_for(patience)
                .map_err(|_| refused(*key, LifecycleCause::Uncertain))?
                .filter(|end| end.scope == shards_api::TerminalScope::Released)
                .ok_or_else(|| refused(*key, LifecycleCause::Uncertain))?;
            record.exit = Some(exit(terminal.status));
            transaction.put_pool(&record)?;
            transaction.finalize_managed(identity.as_ref())?;
            let paths = self.pool_paths(&record)?;
            match fs::remove_dir_all(&paths.dir) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(Refusal::os("removing released observer directory", error));
                }
            }
            File::open(&self.config.state_dir)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| Refusal::os("syncing released observer directory", error))?;
            record.phase = BackingPhase::Released;
            transaction.put_pool(&record)?;
            record.offer()
        })
    }

    pub(super) fn observer_usage(&self, key: &BackingKey) -> Result<Usage, Refusal> {
        let before = self
            .observer_record()?
            .filter(|record| record.key == *key)
            .ok_or_else(|| refused(*key, LifecycleCause::Uncertain))?;
        let keys = self.catalog.conflict_ids(&before.devices)?;
        crate::devices::with_pool_transaction(&self.books, key, &keys, |transaction| {
            let record = transaction
                .read_pool(key)?
                .ok_or_else(|| refused(*key, LifecycleCause::Uncertain))?;
            self.preflight_backing(&record)?;
            let identity = self.pool_identity(&record)?;
            let _receipt = transaction.attach_managed(identity.as_ref())?;
            transaction.managed_usage()
        })
    }

    /// Recovery never reboots an interrupted Granted intent. A retained Live process is joined;
    /// cleanup releases claims only through the same positive whole-scope terminal witness.
    pub fn recover_backing(&self) -> Result<(), Refusal> {
        self.require_initialized()?;
        let Some(record) = self.observer_record()? else {
            return Ok(());
        };
        match record.phase {
            BackingPhase::Prepared | BackingPhase::Released => Ok(()),
            BackingPhase::Granted | BackingPhase::Draining => {
                self.release_observer(&record.key).map(|_| ())
            }
            BackingPhase::Live => {
                let keys = self.catalog.conflict_ids(&record.devices)?;
                let ended = crate::devices::with_pool_transaction(
                    &self.books,
                    &record.key,
                    &keys,
                    |transaction| {
                        let current = transaction
                            .read_pool(&record.key)?
                            .ok_or_else(|| refused(record.key, LifecycleCause::Uncertain))?;
                        if current != record {
                            return Err(refused(record.key, LifecycleCause::Uncertain));
                        }
                        let identity = self.pool_identity(&current)?;
                        let native = transaction.attach_managed(identity.as_ref())?;
                        Ok(native
                            .terminal()
                            .map_err(|_| refused(record.key, LifecycleCause::Uncertain))?
                            .is_some_and(|terminal| {
                                terminal.scope == shards_api::TerminalScope::Released
                            }))
                    },
                )?;
                if ended {
                    self.release_observer(&record.key)?;
                } else {
                    self.physical_observations()?;
                }
                Ok(())
            }
        }
    }
}
