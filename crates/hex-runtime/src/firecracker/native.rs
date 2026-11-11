use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::Instant;

use hex_api::{
    BoxId, BoxInfo, BoxSpec, Class, ClassSpec, DeviceId, DeviceKind, LifecycleCause, Proc, Program,
    Refusal, Runtime, Signal, Stdio, Usage,
};
use hex_client::guest::{Client, LinkOwner, own_link, stage};
use hex_core::record::{BoxRecord, Phase};

use super::{Books, Config, Transaction, exhausted, invalid, observations, transport, vfio};
use crate::devices::{Binding, ManagedLaunch, Route, SharedCatalog, managed_scope_id};

#[path = "pool.rs"]
mod pool;
use crate::guest::rewrite;

/// One initialized Linux KVM backend. The same Node/Store/Turns/Supervisor owns every native
/// receipt and physical claim. This adapter keeps no child table or independent reaper.
pub struct FirecrackerRuntime {
    config: Config,
    catalog: SharedCatalog,
    books: Weak<dyn Books>,
    initialized: std::sync::OnceLock<()>,
}

struct Paths {
    dir: PathBuf,
    config: PathBuf,
    vsock: PathBuf,
    api: PathBuf,
    ready: PathBuf,
}

impl FirecrackerRuntime {
    /// Uses the native service's existing delegated cgroup root and record/attachment turns.
    /// Configuration never binds a host driver, installs a helper or starts a guest here.
    pub fn open(
        config: Config,
        catalog: SharedCatalog,
        books: Weak<dyn Books>,
    ) -> Result<Self, Refusal> {
        config.verify_inputs()?;
        Ok(Self {
            config,
            catalog,
            books,
            initialized: std::sync::OnceLock::new(),
        })
    }

    /// Called after the native owner's whole retained-state/configuration barrier. This exact
    /// configured adapter publishes no class before these actual native prerequisites succeed.
    pub fn initialize_native(&self) -> Result<(), Refusal> {
        if self.initialized.get().is_some() {
            return Ok(());
        }
        self.config.verify_inputs()?;
        let config = &self.config;
        let capacity = self.authority()?.capacity()?;
        capacity.check()?;
        if !capacity.total.holds(
            config
                .cost
                .allocate(hex_api::CapacityResources::default())?
                .resources,
        ) {
            return Err(invalid(
                "configured minimum VM charge exceeds the physical pool",
            ));
        }
        let _kvm = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/kvm")
            .map_err(|error| Refusal::os("opening Firecracker KVM backend", error))?;
        for path in [&config.binary, &config.kernel, &config.initrd] {
            if !fs::metadata(path)
                .map_err(|error| Refusal::os("inspecting Firecracker boot input", error))?
                .is_file()
            {
                return Err(invalid("boot input is not a regular file"));
            }
        }
        fs::create_dir_all(&config.state_dir)
            .and_then(|()| {
                fs::set_permissions(&config.state_dir, fs::Permissions::from_mode(0o700))
            })
            .map_err(|error| Refusal::os("opening private Firecracker state", error))?;
        self.initialized.set(()).or_else(|()| {
            if self.initialized.get().is_some() {
                Ok(())
            } else {
                Err(invalid("Firecracker initialization was not published"))
            }
        })
    }

    fn require_initialized(&self) -> Result<(), Refusal> {
        if self.initialized.get().is_none() {
            return Err(invalid("Firecracker native adapter is not initialized"));
        }
        Ok(())
    }

    fn paths(&self, record: &BoxRecord) -> Result<Paths, Refusal> {
        let scope = managed_scope_id(record)?;
        self.scope_paths(&scope)
    }

    fn scope_paths(&self, scope: &BoxId) -> Result<Paths, Refusal> {
        let dir = path(&self.config.state_dir, scope.as_str())?;
        let vsock = path(&dir, "vsock")?;
        let mut suffix = String::new();
        use std::fmt::Write;
        // Port is the fixed u32 protocol field: decimal needs at most ten digits and an underscore.
        suffix
            .try_reserve_exact(11)
            .map_err(|_| exhausted("readiness suffix bytes", 11))?;
        write!(&mut suffix, "_{}", hex_core::stream::GUEST_READY_PORT)
            .map_err(|_| invalid("readiness suffix cannot be encoded"))?;
        let mut ready = PathBuf::new();
        let room = vsock
            .as_os_str()
            .len()
            .checked_add(suffix.len())
            .ok_or_else(|| invalid("readiness path length overflow"))?;
        ready
            .try_reserve_exact(room)
            .map_err(|_| exhausted("readiness path bytes", room))?;
        ready.push(&vsock);
        ready.as_mut_os_string().push(suffix);
        Ok(Paths {
            config: path(&dir, "config.json")?,
            api: path(&dir, "api")?,
            ready,
            vsock,
            dir,
        })
    }

    fn vm_program(&self, paths: &Paths) -> Result<Program, Refusal> {
        let mut program = Program::default();
        program
            .args
            .try_reserve_exact(6)
            .map_err(|_| exhausted("VMM arguments", 6))?;
        for argument in [
            self.config.binary.as_os_str(),
            std::ffi::OsStr::new("--enable-pci"),
            std::ffi::OsStr::new("--api-sock"),
            paths.api.as_os_str(),
            std::ffi::OsStr::new("--config-file"),
            paths.config.as_os_str(),
        ] {
            program.args.push(text(
                argument
                    .to_str()
                    .ok_or_else(|| invalid("VMM argument is not UTF-8"))?,
            )?);
        }
        Ok(program)
    }

    fn vm_io() -> Result<shards_api::LaunchIo, Refusal> {
        let output = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/null")
            .map_err(|e| Refusal::os("opening VMM output sink", e))?;
        let error = output
            .try_clone()
            .map_err(|e| Refusal::os("duplicating VMM output sink", e))?;
        Ok(shards_api::LaunchIo {
            stdout: std::io::PipeWriter::from(std::os::fd::OwnedFd::from(output)),
            stderr: std::io::PipeWriter::from(std::os::fd::OwnedFd::from(error)),
            gpu_report: None,
        })
    }

    fn authority(&self) -> Result<Arc<dyn Books>, Refusal> {
        self.books
            .upgrade()
            .ok_or_else(|| Refusal::Broken("native node authority is gone".into()))
    }

    fn record(&self, id: &BoxId) -> Result<BoxRecord, Refusal> {
        crate::devices::with_transaction(&self.books, id, &[], |transaction| {
            transaction
                .read(id)?
                .filter(|r| matches!(r.spec.class, ClassSpec::Firecracker(_)))
                .ok_or_else(|| missing(id))
        })
    }

    /// Keep this exact VM's method/device receipts through protocol setup and reply. Releasing the
    /// method turn after only reading a record would allow delete/recreate at the same socket path.
    fn with_record<T>(
        &self,
        id: &BoxId,
        operation: impl FnOnce(&BoxRecord, &dyn Transaction) -> Result<T, Refusal>,
    ) -> Result<T, Refusal> {
        let before = self.record(id)?;
        let keys = self.catalog.conflict_ids(&before.spec.devices)?;
        crate::devices::with_transaction(&self.books, id, &keys, |transaction| {
            let record = transaction.read(id)?.ok_or_else(|| missing(id))?;
            if record.spec != before.spec
                || record.init != before.init
                || record.native_scope != before.native_scope
                || !matches!(record.spec.class, ClassSpec::Firecracker(_))
            {
                return Err(lifecycle(id, LifecycleCause::Uncertain)?);
            }
            operation(&record, transaction)
        })
    }

    fn link(&self, record: &BoxRecord, until: Option<Instant>) -> Result<LinkOwner, Refusal> {
        own_link(transport::Endpoint {
            path: self.paths(record)?.vsock,
            timeout: self.config.operation_timeout,
            create_deadline: until,
        })
    }

    fn native_identity(&self, record: &BoxRecord) -> Result<Option<shards_api::Identity>, Refusal> {
        record
            .init
            .map(|trace| {
                Ok(shards_api::Identity {
                    pid: trace.pid,
                    started: trace.start_ticks,
                    vsock: self.paths(record)?.vsock,
                })
            })
            .transpose()
    }

    fn native(
        &self,
        record: &BoxRecord,
        transaction: &dyn Transaction,
    ) -> Result<shards_api::ManagedReceipt, Refusal> {
        let identity = self.native_identity(record)?;
        transaction.attach_managed(identity.as_ref())
    }

    fn check_process(
        &self,
        record: &BoxRecord,
        transaction: &dyn Transaction,
    ) -> Result<(), Refusal> {
        let native = self.native(record, transaction)?;
        if native
            .terminal()
            .map_err(|_| uncertain(&record.spec.id))?
            .is_some()
        {
            return Err(Refusal::NotRunning(
                record.spec.id.try_clone()?,
                "guest protocol",
            ));
        }
        Ok(())
    }

    fn client(&self, record: &BoxRecord, transaction: &dyn Transaction) -> Result<Client, Refusal> {
        self.client_before(record, transaction, None)
    }

    fn client_before(
        &self,
        record: &BoxRecord,
        transaction: &dyn Transaction,
        until: Option<Instant>,
    ) -> Result<Client, Refusal> {
        if record.phase != Phase::Ready {
            return Err(lifecycle(&record.spec.id, LifecycleCause::Pending)?);
        }
        self.check_process(record, transaction)?;
        let link = self.link(record, until)?;
        match until {
            Some(until) => Client::connect_before(link, until),
            None => Client::connect(link),
        }
    }

    fn translate(
        &self,
        record: &BoxRecord,
        client: &Client,
        until: Option<Instant>,
    ) -> Result<Vec<DeviceId>, Refusal> {
        let spec = &record.spec;
        let paths = self.paths(record)?;
        if spec.devices.is_empty() {
            return Ok(Vec::new());
        }
        let mut capacity = match until {
            Some(until) => client.capacity_before(Class::Oci, until),
            None => client.capacity(Class::Oci),
        }?
        .ok_or_else(|| invalid("guest has no initialized native device adapter"))?;
        capacity.check()?;
        capacity.devices.sort_unstable_by_key(|device| device.id);
        let mut catalog = Vec::new();
        catalog
            .try_reserve_exact(self.catalog.entries().len())
            .map_err(|_| exhausted("selected VFIO catalog index", self.catalog.entries().len()))?;
        catalog.extend(self.catalog.entries().iter());
        catalog.sort_unstable_by_key(|entry| entry.info.id);
        let mut assignments = Vec::new();
        assignments
            .try_reserve_exact(spec.devices.len())
            .map_err(|_| exhausted("guest PCI assignment witnesses", spec.devices.len()))?;
        let mut translated = Vec::new();
        translated
            .try_reserve_exact(spec.devices.len())
            .map_err(|_| exhausted("guest device assignments", spec.devices.len()))?;
        for selected in &spec.devices {
            let at = catalog
                .binary_search_by_key(selected, |entry| entry.info.id)
                .map_err(|_| invalid("recorded physical binding is absent"))?;
            let entry = catalog
                .get(at)
                .ok_or_else(|| invalid("recorded physical binding is absent"))?;
            let Some(Binding::Vfio { sbdf, .. }) = entry.binding(Route::Vfio) else {
                return Err(invalid("recorded binding is no longer VFIO"));
            };
            let id = vfio::function_id(sbdf)?;
            let witness =
                transport::attachment(&paths.api, &id, self.config.operation_timeout, until)?;
            if witness.id != id || witness.sbdf != *sbdf {
                return Err(invalid(
                    "VMM attachment witness names different host hardware",
                ));
            }
            let guest = hex_gpu::PciAddress::parse(&witness.guest_sbdf)
                .ok_or_else(|| invalid("VMM reported malformed guest PCI identity"))?;
            // A group request is fulfilled by its whole native VMM assignment. The container's
            // GPU claims still name each exact guest function, independently checked by the node.
            if selected.kind == DeviceKind::Gpu {
                let guest_id = DeviceId {
                    kind: DeviceKind::Gpu,
                    key: hex_gpu::Identity::Pci(guest).key(),
                };
                let at = capacity
                    .devices
                    .binary_search_by_key(&guest_id, |offer| offer.id)
                    .map_err(|_| invalid("assigned GPU has no matching guest native attachment"))?;
                let found = capacity.devices.get(at).ok_or_else(|| {
                    invalid("assigned GPU has no matching guest native attachment")
                })?;
                if found.size == 0 {
                    return Err(invalid(
                        "assigned GPU has no current native memory observation",
                    ));
                }
                if found
                    .holder
                    .as_ref()
                    .is_some_and(|holder| holder.box_id() != Some(&spec.id))
                {
                    return Err(lifecycle(&spec.id, LifecycleCause::Uncertain)?);
                }
                translated.push(guest_id);
                assignments.push(observations::Assignment {
                    host: *selected,
                    guest: guest_id,
                });
            }
        }
        // This is the initialized guest Node's SAME native provider, not a second host Gpus open
        // or a catalog substitute. The VMM witness is the only host/guest identity translation.
        let report = match until {
            Some(until) => client.physical_report_before(until),
            None => client.physical_report(),
        }?;
        let observed = observations::join(&assignments, &report)?;
        for actual in observed {
            if catalog
                .binary_search_by_key(&actual.info.id, |entry| entry.info.id)
                .is_err()
            {
                return Err(invalid(
                    "observed GPU is outside the original physical selection",
                ));
            }
            // Model/VRAM/counters are this actual native provider's facts, never admin constants.
        }
        if let Some(until) = until {
            remaining(until)?;
        }
        Ok(translated)
    }

    fn create_inner(
        &self,
        spec: BoxSpec,
        expected_scope: Option<hex_core::record::NativeScope>,
        transaction: &dyn Transaction,
        until: Instant,
    ) -> Result<BoxInfo, Refusal> {
        remaining(until)?;
        self.config.verify_inputs()?;
        let records = transaction.records()?;
        if expected_scope.is_none() && records.len() >= crate::MAX_BOXES {
            return Err(Refusal::Full(crate::MAX_BOXES));
        }
        let pools = transaction.pool_records()?;
        let _selection = self.catalog.prepare(&spec, &records, &pools)?;
        let id = spec.id.try_clone()?;
        // The optional handoff already persisted this SAME scope before stopping its observer.
        if let Some(scope) = expected_scope {
            let current = transaction.read(&id)?.ok_or_else(|| uncertain(&id))?;
            if current.spec != spec
                || current.native_scope != Some(scope)
                || current.phase != Phase::Creating
                || current.init.is_some()
            {
                return Err(uncertain(&id));
            }
        } else {
            transaction.put(&BoxRecord::new(spec, Phase::Creating, Vec::new()))?;
        }
        let mut record = transaction.read(&id)?.ok_or_else(|| uncertain(&id))?;
        let plan = vfio::plan(&self.catalog, &record.spec.devices)?;
        let paths = self.paths(&record)?;
        match fs::symlink_metadata(&paths.dir) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(Refusal::os("checking unclaimed VM path", error)),
            Ok(_) => {
                return Err(invalid(
                    "this native incarnation already has unclaimed VM state",
                ));
            }
        }
        record
            .dirs
            .try_reserve_exact(1)
            .map_err(|_| exhausted("VM owned directories", 1))?;
        record.dirs.push(copy_path(&paths.dir)?);
        transaction.put(&record)?;
        let vm = self.config.machine(&record.spec, &paths.vsock, &plan)?;
        // Prepare the original inner workload and endpoint before creating a native process.
        let mut guest_spec = transaction
            .read(&id)?
            .ok_or_else(|| invalid("Creating record disappeared"))?
            .spec;
        let ClassSpec::Firecracker(workload) = guest_spec.class else {
            return Err(invalid("wrong payload backend"));
        };
        guest_spec.class = ClassSpec::Oci(workload);
        guest_spec.allocation = None;
        guest_spec.route = None;
        let link = own_link(transport::Endpoint {
            path: copy_path(&paths.vsock)?,
            timeout: self.config.operation_timeout,
            create_deadline: Some(until),
        })?;
        fs::create_dir(&paths.dir)
            .and_then(|()| fs::set_permissions(&paths.dir, fs::Permissions::from_mode(0o700)))
            .map_err(|e| Refusal::os("creating private VM directory", e))?;
        write_config(&paths.config, &vm)?;
        let listener = UnixListener::bind(&paths.ready)
            .map_err(|e| Refusal::os("binding private guest readiness endpoint", e))?;
        let program = self.vm_program(&paths)?;
        let io = Self::vm_io()?;
        let native = transaction.launch_managed(
            &ManagedLaunch::Firecracker {
                program: &program,
                vsock: &paths.vsock,
            },
            io,
            remaining(until)?,
        )?;
        // Publication happened while the child was held. Read the SAME immutable scope/spec back.
        let current = transaction
            .read(&id)?
            .ok_or_else(|| invalid("published VM record disappeared"))?;
        if current.spec != record.spec
            || current.native_scope != record.native_scope
            || current.init.is_none()
        {
            return Err(lifecycle(&id, LifecycleCause::Uncertain)?);
        }
        record = current;
        transport::ready(&listener, native.as_ref(), until)?;
        let client = Client::connect_before(link.clone(), until)?;
        guest_spec.devices = self.translate(&record, &client, Some(until))?;
        let memory = self
            .config
            .machine_memory(&record.spec)?
            .checked_mul(1 << 20)
            .ok_or_else(|| invalid("guest memory overflow"))?;
        let rewritten = rewrite::rewrite(guest_spec, &[], stage::STAGED, &rewrite::Known::new())?;
        let mut limits = self.config.stage_limits;
        limits.bytes = limits.bytes.min(memory);
        stage::stage(link.as_ref(), &id, &rewritten.items, &limits)?;
        remaining(until)?;
        let mut info = client.create_before(rewritten.spec, until)?;
        record.phase = Phase::Ready;
        transaction.put(&record)?;
        info.class = record.spec.class.class();
        info.bounds = record.spec.bounds;
        info.allocation = record.spec.allocation;
        info.route = record.spec.route;
        Ok(info)
    }

    fn remove(&self, id: &BoxId, transaction: &dyn Transaction) -> Result<(), Refusal> {
        let mut record = transaction
            .read(id)?
            .ok_or(Refusal::NoSuchBox(id.try_clone()?))?;
        if !matches!(record.spec.class, ClassSpec::Firecracker(_)) {
            return Err(invalid("record belongs to another backend"));
        }
        let paths = self.paths(&record)?;
        record.phase = Phase::Deleting;
        transaction.put(&record)?;
        let identity = self.native_identity(&record)?;
        let native = transaction.attach_managed(identity.as_ref())?;
        native.stop().map_err(|_| uncertain(id))?;
        let end = native
            .wait_for(self.config.stop_timeout)
            .map_err(|_| uncertain(id))?
            .filter(|end| end.scope == shards_api::TerminalScope::Released)
            .ok_or(lifecycle(id, LifecycleCause::Uncertain)?)?;
        record.exit = Some(exit(end.status));
        transaction.put(&record)?;
        // Persisted exit is not itself absence proof. Finalize checks the same native receipt and
        // actual empty kernel scope before removing it; the record/physical claims still live here.
        transaction.finalize_managed(identity.as_ref())?;
        match fs::remove_dir_all(&paths.dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Refusal::os("removing owned VM state", e)),
        }
        File::open(&self.config.state_dir)
            .and_then(|file| file.sync_all())
            .map_err(|e| Refusal::os("syncing VM state removal", e))?;
        transaction.remove(id)
    }

    /// Recovery uses the canonical native scope and VMM witnesses, never another backend or pick.
    pub fn validate_binding(&self, expected: &BoxSpec) -> Result<(), Refusal> {
        let keys = self.catalog.conflict_ids(&expected.devices)?;
        crate::devices::with_transaction(&self.books, &expected.id, &keys, |transaction| {
            let record = transaction
                .read(&expected.id)?
                .ok_or(Refusal::NoSuchBox(expected.id.try_clone()?))?;
            if record.spec != *expected
                || record.phase != Phase::Ready
                || !matches!(record.spec.class, ClassSpec::Firecracker(_))
            {
                return Err(lifecycle(&expected.id, LifecycleCause::Uncertain)?);
            }
            expected.check()?;
            let native = self.native(&record, transaction)?;
            if native
                .terminal()
                .map_err(|_| uncertain(&expected.id))?
                .is_some()
            {
                return Ok(());
            }
            let client = self.client(&record, transaction)?;
            self.translate(&record, &client, None)?;
            let actual = client.inspect(&expected.id)?;
            if actual.class != Class::Oci
                || actual.bounds != expected.bounds
                || actual.allocation.is_some()
            {
                return Err(lifecycle(&expected.id, LifecycleCause::Uncertain)?);
            }
            Ok(())
        })
    }

    /// Pure retained-configuration check for the common all-record preflight. No paths, probes,
    /// protocol clients, native receipts or current capacity are opened here.
    pub fn preflight_record(&self, record: &BoxRecord) -> Result<(), Refusal> {
        record.spec.check()?;
        self.config.check()?;
        if record.spec.backend_class()? != Class::Firecracker
            || !matches!(record.spec.class, ClassSpec::Firecracker(_))
            || record.native_scope.is_none()
        {
            return Err(uncertain(&record.spec.id));
        }
        self.config
            .guest(&record.spec)
            .map_err(|_| uncertain(&record.spec.id))?;
        self.config
            .machine_memory(&record.spec)
            .map_err(|_| uncertain(&record.spec.id))?;
        let paths = self.paths(record)?;
        if record.dirs.iter().any(|owned| owned != &paths.dir) {
            return Err(uncertain(&record.spec.id));
        }
        Ok(())
    }

    fn current_capacity(
        &self,
        class: Class,
        until: Option<Instant>,
    ) -> Result<Option<hex_api::Capacity>, Refusal> {
        self.require_initialized()?;
        if class != Class::Firecracker {
            return Err(invalid("capacity requested for another backend"));
        }
        let authority = self.authority()?;
        let records = authority.records()?;
        let pools = authority.pool_records()?;
        let mut capacity = self.authority()?.capacity()?;
        let observer = self.observer_record()?;
        let guest_observed = self.physical_observations_before(until)?;
        let mut observed: Vec<hex_api::DeviceInfo> = Vec::new();
        observed
            .try_reserve_exact(guest_observed.len())
            .map_err(|_| exhausted("current observer facts", guest_observed.len()))?;
        for actual in guest_observed {
            if let Some(host) = observed.iter_mut().find(|host| host.id == actual.info.id) {
                *host = actual.info;
            } else {
                observed.push(actual.info);
            }
        }
        capacity.cost = None;
        capacity.routes.clear();
        capacity.device_routes.clear();
        capacity.backings.clear();
        capacity.devices.clear();
        capacity
            .devices
            .try_reserve_exact(self.catalog.entries().len())
            .map_err(|_| exhausted("VFIO capacity", self.catalog.entries().len()))?;
        for entry in self
            .catalog
            .entries()
            .iter()
            .filter(|entry| entry.binding(Route::Vfio).is_some())
        {
            if entry.info.id.kind == DeviceKind::Gpu {
                // An administrator's catalog is not current VRAM/load evidence. Prebound VFIO
                // may hide host NVML: only the same initialized facts provider may supply this
                // actual identity/metadata observation (including a witnessed live guest).
                let Some(actual) = observed.iter().find(|actual| actual.id == entry.info.id) else {
                    continue;
                };
                vfio::validate(entry)?;
                let mut actual = actual.try_clone()?;
                actual.holder = self.catalog.holder(entry.info.id, &records, &pools)?;
                capacity.devices.push(actual);
                continue;
            }
            vfio::validate(entry)?;
            capacity.devices.push(entry.report(self.catalog.holder(
                entry.info.id,
                &records,
                &pools,
            )?)?);
        }
        let arch = match std::env::consts::ARCH {
            "x86_64" => hex_api::ImageArch::Amd64,
            "aarch64" => hex_api::ImageArch::Arm64,
            _ => {
                return Err(invalid(
                    "native architecture is outside the supported Firecracker targets",
                ));
            }
        };
        let cpu = hex_api::RouteOffer {
            key: hex_api::RouteKey {
                class: Class::Firecracker,
                image: hex_api::ImagePlatform {
                    os: hex_api::ImageOs::Linux,
                    arch,
                },
                execution: hex_api::Execution::Firecracker,
                gpu: hex_api::GpuEligibility::None,
            },
            cost: Some(self.config.cost),
            backing: None,
        };
        capacity
            .routes
            .try_reserve_exact(2)
            .map_err(|_| exhausted("Firecracker initialized routes", 2))?;
        capacity
            .device_routes
            .try_reserve_exact(capacity.devices.len())
            .map_err(|_| exhausted("Firecracker device eligibility", capacity.devices.len()))?;
        capacity.routes.push(cpu);
        let gpu = observer.is_some()
            || capacity
                .devices
                .iter()
                .any(|device| device.id.kind == DeviceKind::Gpu);
        let backing = observer.as_ref().map(|record| record.key);
        if let Some(observer) = observer {
            capacity
                .backings
                .try_reserve_exact(1)
                .map_err(|_| exhausted("Firecracker observer offer", 1))?;
            capacity.backings.push(observer.offer()?);
        }
        if gpu {
            capacity.routes.push(hex_api::RouteOffer {
                key: hex_api::RouteKey {
                    gpu: hex_api::GpuEligibility::Required,
                    ..cpu.key
                },
                backing,
                ..cpu
            });
        }
        capacity
            .device_routes
            .extend(capacity.devices.iter().map(|device| {
                if device.id.kind == DeviceKind::Gpu {
                    2
                } else if gpu {
                    3
                } else {
                    1
                }
            }));
        capacity.check()?;
        Ok(Some(capacity))
    }

    pub fn recover(&self) -> Result<(), Refusal> {
        for (id, record) in self.authority()?.records()? {
            let record = record.map_err(|_| uncertain(&id))?;
            if !matches!(record.spec.class, ClassSpec::Firecracker(_)) {
                continue;
            }
            match record.phase {
                Phase::Ready => self.validate_binding(&record.spec)?,
                Phase::Creating | Phase::Deleting => self.delete(&id)?,
            }
        }
        Ok(())
    }
}

fn remaining(until: Instant) -> Result<std::time::Duration, Refusal> {
    let left = until.saturating_duration_since(Instant::now());
    if left.is_zero() {
        Err(Refusal::os(
            "creating Firecracker VM",
            "whole create deadline elapsed",
        ))
    } else {
        Ok(left)
    }
}

fn missing(id: &BoxId) -> Refusal {
    match id.try_clone() {
        Ok(id) => Refusal::NoSuchBox(id),
        Err(error) => error,
    }
}

fn uncertain(id: &BoxId) -> Refusal {
    match id.try_clone() {
        Ok(id) => Refusal::Lifecycle {
            id,
            cause: LifecycleCause::Uncertain,
        },
        Err(error) => error,
    }
}

fn lifecycle(id: &BoxId, cause: LifecycleCause) -> Result<Refusal, Refusal> {
    Ok(Refusal::Lifecycle {
        id: id.try_clone()?,
        cause,
    })
}

impl Runtime for FirecrackerRuntime {
    fn classes(&self) -> Vec<Class> {
        if self.initialized.get().is_some() {
            vec![Class::Firecracker]
        } else {
            Vec::new()
        }
    }
    fn initialize_backing(
        &self,
        grant: hex_api::BackingGrant,
    ) -> Result<hex_api::BackingOffer, Refusal> {
        self.require_initialized()?;
        self.initialize_observer(grant)
    }
    fn release_backing(&self, key: hex_api::BackingKey) -> Result<hex_api::BackingOffer, Refusal> {
        self.require_initialized()?;
        let released = self.release_observer(&key)?;
        // The caller first consumed the canonical Released capacity receipt. Its explicit release
        // acknowledgement may now rearm only after every original Box reference is gone.
        self.prepare_backing()?;
        Ok(released)
    }
    fn backing_usage(&self, key: &hex_api::BackingKey) -> Result<Usage, Refusal> {
        self.require_initialized()?;
        self.observer_usage(key)
    }

    fn capacity(&self, class: Class) -> Result<Option<hex_api::Capacity>, Refusal> {
        self.current_capacity(class, None)
    }

    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        self.require_initialized()?;
        let until = Instant::now()
            .checked_add(self.config.boot_timeout)
            .ok_or_else(|| invalid("whole create deadline is unrepresentable"))?;
        spec.check()?;
        if !matches!(spec.class, ClassSpec::Firecracker(_)) {
            return Err(invalid(
                "endpoint only executes explicit Firecracker payloads",
            ));
        }
        if spec.route.is_some() {
            self.current_capacity(Class::Firecracker, Some(until))?
                .ok_or_else(|| invalid("Firecracker endpoint has no initialized capacity"))?
                .validate_route(&spec)?;
        }
        remaining(until)?;
        let keys = self.catalog.conflict_ids(&spec.devices)?;
        let id = spec.id.try_clone()?;
        let (spec, scope) = if let Some(key) = spec.route.and_then(|route| route.backing) {
            let pending =
                crate::devices::with_transaction(&self.books, &id, &keys, |transaction| {
                    if transaction.read(&id)?.is_some() {
                        return Err(Refusal::Exists(id.try_clone()?));
                    }
                    let records = transaction.records()?;
                    if records.len() >= crate::MAX_BOXES {
                        return Err(Refusal::Full(crate::MAX_BOXES));
                    }
                    let pools = transaction.pool_records()?;
                    self.catalog.prepare(&spec, &records, &pools)?;
                    transaction.put(&BoxRecord::new(spec, Phase::Creating, Vec::new()))?;
                    transaction.read(&id)?.ok_or_else(|| uncertain(&id))
                })?;
            // No Box/device turn is held while entering this SAME owner's Pool transaction.
            // A failed or controller-only stop retains both canonical handoff claims.
            let released = self.release_observer_before(&key, Some(until))?;
            if released.key != key
                || released.phase != hex_api::BackingPhase::Released
                || released.devices != pending.spec.devices
            {
                return Err(uncertain(&id));
            }
            let scope = pending.native_scope.ok_or_else(|| uncertain(&id))?;
            (pending.spec, Some(scope))
        } else {
            (spec, None)
        };
        remaining(until)?;
        crate::devices::with_transaction(&self.books, &id, &keys, |transaction| {
            if scope.is_none() && transaction.read(&id)?.is_some() {
                return Err(Refusal::Exists(id.try_clone()?));
            }
            match self.create_inner(spec, scope, transaction, until) {
                Ok(info) => Ok(info),
                Err(error) => match transaction.read(&id)? {
                    Some(_) => match self.remove(&id, transaction) {
                        Ok(()) => Err(error),
                        Err(_) => Err(uncertain(&id)),
                    },
                    None => Err(error),
                },
            }
        })
    }

    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        self.with_record(id, |record, transaction| {
            self.client(record, transaction)?.start(id)
        })
    }
    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        self.with_record(id, |record, transaction| {
            self.client(record, transaction)?.exec(id, program, stdio)
        })
    }
    fn run(
        &self,
        spec: BoxSpec,
        _program: Program,
        _stdio: Stdio,
    ) -> Result<Box<dyn Proc>, Refusal> {
        spec.check_run()?;
        Err(invalid(
            "Firecracker init is created and started explicitly",
        ))
    }
    fn signal(&self, id: &BoxId, signal: Signal) -> Result<(), Refusal> {
        self.with_record(id, |record, transaction| {
            self.client(record, transaction)?.signal(id, signal)
        })
    }
    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
        self.with_record(id, |record, transaction| {
            if record.phase == Phase::Ready
                && let Some(end) = self
                    .native(record, transaction)?
                    .terminal()
                    .map_err(|_| uncertain(id))?
            {
                return Ok(BoxInfo {
                    id: id.try_clone()?,
                    class: record.spec.class.class(),
                    state: hex_api::BoxState::Empty,
                    bounds: record.spec.bounds.clone(),
                    allocation: record.spec.allocation,
                    route: record.spec.route,
                    adopted: false,
                    membership: None,
                    exit: Some(exit(end.status)),
                    abandoned: record.abandoned,
                });
            }
            let mut info = self.client(record, transaction)?.inspect(id)?;
            info.class = record.spec.class.class();
            info.bounds = record.spec.bounds.clone();
            info.allocation = record.spec.allocation;
            info.route = record.spec.route;
            Ok(info)
        })
    }
    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        self.with_record(id, |record, transaction| {
            let _native = self.native(record, transaction)?;
            transaction.managed_usage()
        })
    }
    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        let authority = self.authority()?;
        let records = authority.records()?;
        let mut boxes = Vec::new();
        boxes
            .try_reserve_exact(records.len())
            .map_err(|_| exhausted("VM list", records.len()))?;
        for (id, record) in records {
            let record = record.map_err(|_| {
                Refusal::Broken("unreadable native record prevents listing VMs".into())
            })?;
            if matches!(record.spec.class, ClassSpec::Firecracker(_)) {
                boxes.push(self.inspect(&id)?);
            }
        }
        Ok(boxes)
    }
    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        let record = self.record(id)?;
        let keys = self.catalog.conflict_ids(&record.spec.devices)?;
        crate::devices::with_transaction(&self.books, id, &keys, |transaction| {
            self.remove(id, transaction)
        })
    }
}

fn exit(status: shards_api::ManagedStatus) -> hex_api::Exit {
    match status {
        shards_api::ManagedStatus::Code(code) => hex_api::Exit::Code(code),
        shards_api::ManagedStatus::Signal(signal) => hex_api::Exit::Signal(signal),
        shards_api::ManagedStatus::Unknown => hex_api::Exit::Unknown,
    }
}

pub(super) fn text(value: &str) -> Result<String, Refusal> {
    let mut owned = String::new();
    owned
        .try_reserve_exact(value.len())
        .map_err(|_| exhausted("VMM argument bytes", value.len()))?;
    owned.push_str(value);
    Ok(owned)
}

fn copy_path(value: &Path) -> Result<PathBuf, Refusal> {
    let mut owned = PathBuf::new();
    owned
        .try_reserve_exact(value.as_os_str().len())
        .map_err(|_| exhausted("VM path bytes", value.as_os_str().len()))?;
    owned.push(value);
    Ok(owned)
}

pub(super) fn path(parent: &Path, child: &str) -> Result<PathBuf, Refusal> {
    let room = parent
        .as_os_str()
        .len()
        .checked_add(child.len())
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| invalid("VM path length overflow"))?;
    let mut owned = PathBuf::new();
    owned
        .try_reserve_exact(room)
        .map_err(|_| exhausted("VM path bytes", room))?;
    owned.push(parent);
    owned.push(child);
    Ok(owned)
}

fn write_config(path: &Path, value: &impl serde::Serialize) -> Result<(), Refusal> {
    struct Buffer(Vec<u8>);
    impl Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self
                .0
                .len()
                .checked_add(bytes.len())
                .is_none_or(|n| n > hex_core::bundle::MAX_CONFIG_BYTES)
            {
                return Err(std::io::Error::other("VM config exceeds native byte bound"));
            }
            self.0
                .try_reserve_exact(bytes.len())
                .map_err(std::io::Error::other)?;
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut buffer = Buffer(Vec::new());
    serde_json::to_writer(&mut buffer, value).map_err(|e| Refusal::os("encoding VM config", e))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| Refusal::os("creating VM config", e))?;
    file.write_all(&buffer.0)
        .and_then(|()| file.sync_all())
        .map_err(|e| Refusal::os("writing VM config", e))?;
    let parent = path
        .parent()
        .ok_or_else(|| invalid("VM config has no directory"))?;
    File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(|e| Refusal::os("syncing VM config directory", e))
}

#[cfg(test)]
mod io_tests {
    use super::*;
    use std::os::fd::AsRawFd;

    /// AC-T7.4.1: configured VMM output must accept both streams, and both exact owned
    /// descriptors close on drop. No Config, guest, process, cgroup, KVM or GPU is created.
    #[test]
    fn configured_vmm_output_sinks_accept_bytes_and_close_on_drop() {
        let shards_api::LaunchIo {
            mut stdout,
            mut stderr,
            gpu_report,
        } = FirecrackerRuntime::vm_io().unwrap();
        assert!(gpu_report.is_none());
        let descriptors = [stdout.as_raw_fd(), stderr.as_raw_fd()];
        assert_ne!(descriptors[0], descriptors[1]);
        let output = stdout.write_all(b"configured-vmm-stdout\n");
        let error = stderr.write_all(b"configured-vmm-stderr\n");
        drop(stdout);
        drop(stderr);
        for descriptor in descriptors {
            let path = Path::new("/proc/self/fd").join(descriptor.to_string());
            assert!(
                matches!(fs::read_link(path), Err(error) if error.kind() == std::io::ErrorKind::NotFound),
                "owned sink descriptor must close before the control returns"
            );
        }
        assert!(
            output.is_ok() && error.is_ok(),
            "stdout={output:?}; stderr={error:?}"
        );
    }
}
