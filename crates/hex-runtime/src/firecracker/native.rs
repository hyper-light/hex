use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;

use hex_api::{
    BoxId, BoxInfo, BoxSpec, Class, ClassSpec, DeviceId, DeviceKind, LifecycleCause, Proc, Program,
    Refusal, Runtime, Signal, Stdio, Usage,
};
use hex_client::guest::{Client, Link, stage};
use hex_core::record::{BoxRecord, Phase, ProcId};

use super::{Books, Config, Transaction, exhausted, invalid, transport, vfio};
use crate::devices::{Binding, Catalog, Route};
use crate::guest::rewrite;
use crate::linux::{cgroup::BoxRoot, oci::state::start_time, proc::PidfdProc, process};

/// One initialized Linux KVM backend. Claims live only in `books`; `children` owns unreaped
/// direct-child pidfds, bounded by the same node box budget, and is never admission authority.
pub struct FirecrackerRuntime {
    config: Config,
    cgroups: BoxRoot,
    catalog: Arc<Catalog>,
    books: Weak<dyn Books>,
    children: Mutex<Vec<Child>>,
}

struct Child {
    id: BoxId,
    state: ChildState,
}

enum ChildState {
    Reserved,
    Observing,
    Live(PidfdProc),
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
        cgroups: BoxRoot,
        catalog: Arc<Catalog>,
        books: Weak<dyn Books>,
    ) -> Result<Self, Refusal> {
        config.verify_inputs()?;
        let capacity = books
            .upgrade()
            .ok_or_else(|| Refusal::Broken("native node authority is gone".into()))?
            .capacity()?;
        capacity.check()?;
        if capacity.id != config.pool_id {
            return Err(invalid(
                "configured VM pool differs from the existing Node physical pool",
            ));
        }
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
            .map_err(|e| Refusal::os("opening Firecracker KVM backend", e))?;
        for path in [&config.binary, &config.kernel, &config.initrd] {
            if !fs::metadata(path)
                .map_err(|e| Refusal::os("inspecting Firecracker boot input", e))?
                .is_file()
            {
                return Err(invalid("boot input is not a regular file"));
            }
        }
        fs::create_dir_all(&config.state_dir)
            .and_then(|()| {
                fs::set_permissions(&config.state_dir, fs::Permissions::from_mode(0o700))
            })
            .map_err(|e| Refusal::os("opening private Firecracker state", e))?;
        for entry in catalog
            .entries()
            .iter()
            .filter(|entry| entry.binding(Route::Vfio).is_some())
        {
            vfio::validate(entry)?;
        }
        Ok(Self {
            config,
            cgroups,
            catalog,
            books,
            children: Mutex::new(Vec::new()),
        })
    }

    fn paths(&self, id: &BoxId) -> Paths {
        let dir = self.config.state_dir.join(id.as_str());
        let vsock = dir.join("vsock");
        let mut ready = vsock.as_os_str().to_os_string();
        ready.push(format!("_{}", hex_core::stream::GUEST_READY_PORT));
        Paths {
            config: dir.join("config.json"),
            api: dir.join("api"),
            ready: ready.into(),
            vsock,
            dir,
        }
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
                .ok_or_else(|| Refusal::NoSuchBox(id.clone()))
        })
    }

    /// Keep this exact VM's method/device receipts through protocol setup and reply. Releasing the
    /// method turn after only reading a record would allow delete/recreate at the same socket path.
    fn with_record<T>(
        &self,
        id: &BoxId,
        operation: impl FnOnce(&BoxRecord) -> Result<T, Refusal>,
    ) -> Result<T, Refusal> {
        let before = self.record(id)?;
        let keys = self.catalog.conflict_ids(&before.spec.devices)?;
        crate::devices::with_transaction(&self.books, id, &keys, |transaction| {
            let record = transaction
                .read(id)?
                .ok_or_else(|| Refusal::NoSuchBox(id.clone()))?;
            if record.spec != before.spec
                || record.init != before.init
                || !matches!(record.spec.class, ClassSpec::Firecracker(_))
            {
                return Err(lifecycle(id, LifecycleCause::Uncertain)?);
            }
            operation(&record)
        })
    }

    fn link(&self, id: &BoxId) -> Arc<dyn Link> {
        Arc::new(transport::Endpoint {
            path: self.paths(id).vsock,
            timeout: self.config.operation_timeout,
            create_deadline: None,
        })
    }

    fn client(&self, record: &BoxRecord) -> Result<Client, Refusal> {
        if record.phase != Phase::Ready {
            return Err(lifecycle(&record.spec.id, LifecycleCause::Pending)?);
        }
        self.check_process(record)?;
        Client::connect(self.link(&record.spec.id))
    }

    fn check_process(&self, record: &BoxRecord) -> Result<(), Refusal> {
        let trace = record
            .init
            .ok_or_else(|| invalid("VM record has no process identity"))?;
        let raw = i32::try_from(trace.pid)
            .map_err(|_| invalid("VM process identity is unrepresentable"))?;
        if !recorded_running(&record.spec.id, trace)? {
            return Err(Refusal::NotRunning(
                record.spec.id.try_clone()?,
                "guest protocol",
            ));
        }
        let cgroup =
            crate::linux::cgroup::BoxCgroup::at(self.cgroups.dir().join(record.spec.id.as_str()));
        if cgroup.populated_if_present()?.is_none() {
            return Err(lifecycle(&record.spec.id, LifecycleCause::Uncertain)?);
        }
        if start_time(raw) != Some(trace.start_ticks) || !cgroup.procs()?.contains(&trace.pid) {
            return Err(lifecycle(&record.spec.id, LifecycleCause::Uncertain)?);
        }
        Ok(())
    }

    fn translate(
        &self,
        spec: &BoxSpec,
        client: &Client,
        until: Option<Instant>,
    ) -> Result<Vec<DeviceId>, Refusal> {
        if spec.devices.is_empty() {
            return Ok(Vec::new());
        }
        let capacity = client
            .capacity(Class::Oci)?
            .ok_or_else(|| invalid("guest has no initialized native device adapter"))?;
        capacity.check()?;
        let mut translated = Vec::new();
        translated
            .try_reserve_exact(spec.devices.len())
            .map_err(|_| exhausted("guest device assignments", spec.devices.len()))?;
        for selected in &spec.devices {
            let entry = self
                .catalog
                .entries()
                .iter()
                .find(|entry| entry.info.id == *selected)
                .ok_or_else(|| invalid("recorded physical binding is absent"))?;
            let Some(Binding::Vfio { sbdf, .. }) = entry.binding(Route::Vfio) else {
                return Err(invalid("recorded binding is no longer VFIO"));
            };
            let id = vfio::function_id(sbdf)?;
            let timeout = until
                .map(remaining)
                .transpose()?
                .map_or(self.config.operation_timeout, |left| {
                    left.min(self.config.operation_timeout)
                });
            let witness = transport::attachment(&self.paths(&spec.id).api, &id, timeout)?;
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
                let found = capacity
                    .devices
                    .iter()
                    .find(|offer| offer.id == guest_id)
                    .ok_or_else(|| {
                        invalid("assigned GPU has no matching guest native attachment")
                    })?;
                if found.size != entry.info.size {
                    return Err(invalid(
                        "actual assigned guest GPU metadata differs from the physical selection",
                    ));
                }
                if found
                    .holder
                    .as_ref()
                    .is_some_and(|holder| holder != &spec.id)
                {
                    return Err(lifecycle(&spec.id, LifecycleCause::Uncertain)?);
                }
                translated.push(guest_id);
            }
        }
        Ok(translated)
    }

    fn create_inner(
        &self,
        spec: &BoxSpec,
        transaction: &dyn Transaction,
        paths: &Paths,
    ) -> Result<BoxInfo, Refusal> {
        let until = Instant::now()
            .checked_add(self.config.boot_timeout)
            .ok_or_else(|| invalid("whole create deadline is unrepresentable"))?;
        self.config.verify_inputs()?;
        let records = transaction.records()?;
        for path in [&paths.dir, &self.cgroups.dir().join(spec.id.as_str())] {
            match fs::symlink_metadata(path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(Refusal::os("checking unclaimed VM paths", error)),
                Ok(_) => {
                    return Err(invalid(
                        "unclaimed native VM state already exists; its ownership is uncertain",
                    ));
                }
            }
        }
        if records.len() >= crate::MAX_BOXES {
            return Err(Refusal::Full(crate::MAX_BOXES));
        }
        let _selection = self.catalog.prepare(spec, &records)?;
        let plan = vfio::plan(&self.catalog, &spec.devices)?;
        let vm = self.config.machine(spec, &paths.vsock, &plan)?;
        let mut record = BoxRecord::new(spec.clone(), Phase::Creating, vec![paths.dir.clone()]);
        self.reserve_child(&spec.id)?;
        transaction.put(&record)?;
        fs::create_dir(&paths.dir)
            .and_then(|()| fs::set_permissions(&paths.dir, fs::Permissions::from_mode(0o700)))
            .map_err(|e| Refusal::os("creating private VM directory", e))?;
        write_config(&paths.config, &vm)?;
        let listener = UnixListener::bind(&paths.ready)
            .map_err(|e| Refusal::os("binding private guest readiness endpoint", e))?;
        let allocation = spec
            .allocation
            .ok_or_else(|| invalid("admitted runtime allocation required"))?;
        let host_bounds = hex_api::Bounds {
            cpu_max_milli: Some(
                u32::try_from(allocation.resources.cpu_milli)
                    .map_err(|_| invalid("admitted host CPU bound is unrepresentable"))?,
            ),
            memory_max: Some(allocation.resources.mem_bytes),
            ..hex_api::Bounds::default()
        };
        // The workload's memory.high remains inside the guest; applying it to the VMM would throttle
        // resources already charged for guest kernel and host allowance.
        let cgroup = self.cgroups.create(&spec.id, &host_bounds)?;
        let binary = self
            .config
            .binary
            .to_str()
            .ok_or_else(|| invalid("VMM binary path is not UTF-8"))?;
        let config = paths
            .config
            .to_str()
            .ok_or_else(|| invalid("VM config path is not UTF-8"))?;
        let api = paths
            .api
            .to_str()
            .ok_or_else(|| invalid("VM API path is not UTF-8"))?;
        let program = Program::of(&[
            binary,
            "--enable-pci",
            "--api-sock",
            api,
            "--config-file",
            config,
        ]);
        let mut process = process::exec(&cgroup, &program, Stdio::null())?;
        let pid = process.pid();
        let trace = i32::try_from(pid)
            .ok()
            .and_then(start_time)
            .map(|start_ticks| ProcId { pid, start_ticks });
        // Own the pidfd before any fallible post-spawn write. Failure keeps the Creating record;
        // the deterministic cgroup closes the crash-before-PID-record window.
        let result = (|| {
            record.init = Some(
                trace.ok_or_else(|| invalid("cannot establish spawned VMM process identity"))?,
            );
            transaction.put(&record)?;
            transport::ready(&listener, &mut process, remaining(until)?)?;
            let link: Arc<dyn Link> = Arc::new(transport::Endpoint {
                path: paths.vsock.clone(),
                timeout: self.config.operation_timeout,
                create_deadline: Some(until),
            });
            let client = Client::connect(Arc::clone(&link))?;
            let mut guest_spec = spec.clone();
            let ClassSpec::Firecracker(workload) = guest_spec.class else {
                return Err(invalid("wrong payload backend"));
            };
            guest_spec.class = ClassSpec::Oci(workload);
            // This nested native container is the original workload inside the already charged VM.
            // Its parent host allocation is not a second per-container resource reservation.
            guest_spec.allocation = None;
            guest_spec.devices = self.translate(spec, &client, Some(until))?;
            let memory = self
                .config
                .machine_memory(spec)?
                .checked_mul(1 << 20)
                .ok_or_else(|| invalid("guest memory overflow"))?;
            let rewritten =
                rewrite::rewrite(guest_spec, &[], stage::STAGED, &rewrite::Known::new())?;
            let mut limits = self.config.stage_limits;
            limits.bytes = limits.bytes.min(memory);
            stage::stage(link.as_ref(), &spec.id, &rewritten.items, &limits)?;
            remaining(until)?;
            let mut info = client.create(rewritten.spec)?;
            record.phase = Phase::Ready;
            transaction.put(&record)?;
            info.class = Class::Firecracker;
            info.bounds = spec.bounds.clone();
            info.allocation = spec.allocation;
            Ok(info)
        })();
        self.retain_child(&spec.id, process)?;
        result
    }

    fn reserve_child(&self, id: &BoxId) -> Result<(), Refusal> {
        let copied = id.try_clone()?;
        let mut children = self
            .children
            .lock()
            .map_err(|_| Refusal::Broken("VM child ownership lock poisoned".into()))?;
        if children.iter().any(|child| child.id == *id) {
            return Err(invalid("VM process owner already reserved"));
        }
        if children.len() >= crate::MAX_BOXES {
            return Err(Refusal::Full(crate::MAX_BOXES));
        }
        children
            .try_reserve(1)
            .map_err(|_| exhausted("VM child owners", crate::MAX_BOXES))?;
        children.push(Child {
            id: copied,
            state: ChildState::Reserved,
        });
        Ok(())
    }

    fn forget_reserved_child(&self, id: &BoxId) -> Result<(), Refusal> {
        let mut children = self
            .children
            .lock()
            .map_err(|_| Refusal::Broken("VM child ownership lock poisoned".into()))?;
        if let Some(index) = children.iter().position(|child| child.id == *id) {
            if !children
                .get(index)
                .is_some_and(|child| matches!(child.state, ChildState::Reserved))
            {
                return Err(invalid("unrecorded child ownership remains uncertain"));
            }
            let _reservation = children.swap_remove(index);
        }
        Ok(())
    }

    fn retain_child(&self, id: &BoxId, mut process: PidfdProc) -> Result<(), Refusal> {
        // Even a poisoned owner lock must retain an already spawned child before returning its
        // typed error. There is no post-spawn allocation, and no mutex is held across OS spawn.
        let (mut children, poisoned) = match self.children.lock() {
            Ok(guard) => (guard, false),
            Err(error) => (error.into_inner(), true),
        };
        if let Some(child) = children.iter_mut().find(|child| child.id == *id) {
            child.state = ChildState::Live(process);
            if poisoned {
                return Err(Refusal::Broken(
                    "VM child owner lock poisoned after spawn; child retained".into(),
                ));
            }
            return Ok(());
        }
        drop(children);
        process.kill()?;
        if process.wait_for(self.config.stop_timeout)?.is_none() {
            return Err(invalid("spawned child termination remains uncertain"));
        }
        Err(Refusal::Broken("VM process reservation disappeared".into()))
    }

    fn remove(&self, id: &BoxId, transaction: &dyn Transaction) -> Result<(), Refusal> {
        let mut record = transaction
            .read(id)?
            .ok_or_else(|| Refusal::NoSuchBox(id.clone()))?;
        if !matches!(record.spec.class, ClassSpec::Firecracker(_)) {
            return Err(invalid("record belongs to another backend"));
        }
        record.phase = Phase::Deleting;
        transaction.put(&record)?;
        // Query the deterministic kernel path directly: a name cache cannot prove absence.
        let cgroup = crate::linux::cgroup::BoxCgroup::at(self.cgroups.dir().join(id.as_str()));
        let present = cgroup.populated_if_present()?;
        if present.is_none()
            && let Some(trace) = record.init
        {
            if recorded_running(id, trace)? {
                return Err(lifecycle(id, LifecycleCause::Uncertain)?);
            }
        }
        if present.is_some() {
            if let Some(trace) = record.init {
                let members = cgroup.procs()?;
                if members.iter().any(|pid| *pid != trace.pid)
                    || (recorded_running(id, trace)? && !members.contains(&trace.pid))
                {
                    return Err(lifecycle(id, LifecycleCause::Uncertain)?);
                }
            }
            cgroup.kill()?;
            if !cgroup.wait_empty(self.config.stop_timeout)? {
                return Err(lifecycle(id, LifecycleCause::Uncertain)?);
            }
            cgroup.remove()?;
            self.cgroups.forget(id.as_str());
        }
        let process = {
            let mut children = self
                .children
                .lock()
                .map_err(|_| Refusal::Broken("VM child ownership lock poisoned".into()))?;
            match children.iter_mut().find(|child| child.id == *id) {
                Some(child) => match std::mem::replace(&mut child.state, ChildState::Observing) {
                    ChildState::Live(process) => Some(process),
                    ChildState::Reserved => {
                        child.state = ChildState::Reserved;
                        None
                    }
                    ChildState::Observing => return Err(lifecycle(id, LifecycleCause::Busy)?),
                },
                None => None,
            }
        };
        if let Some(mut process) = process {
            let reaped = process.wait_for(self.config.stop_timeout);
            self.retain_child(id, process)?;
            if reaped?.is_none() {
                return Err(lifecycle(id, LifecycleCause::Uncertain)?);
            }
        }
        {
            let mut children = self
                .children
                .lock()
                .map_err(|_| Refusal::Broken("VM child ownership lock poisoned".into()))?;
            if let Some(index) = children.iter().position(|child| child.id == *id) {
                let _reaped = children.swap_remove(index);
            }
        }
        let dir = self.paths(id).dir;
        match fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Refusal::os("removing owned VM state", e)),
        }
        File::open(&self.config.state_dir)
            .and_then(|file| file.sync_all())
            .map_err(|e| Refusal::os("syncing VM state removal", e))?;
        transaction.remove(id)
    }

    /// Recovery uses the same host claims and actual VMM attachment witnesses, never a new pick.
    pub fn validate_binding(&self, expected: &BoxSpec) -> Result<(), Refusal> {
        let keys = self.catalog.conflict_ids(&expected.devices)?;
        crate::devices::with_transaction(&self.books, &expected.id, &keys, |transaction| {
            let record = transaction
                .read(&expected.id)?
                .ok_or_else(|| Refusal::NoSuchBox(expected.id.clone()))?;
            if record.spec != *expected
                || record.phase != Phase::Ready
                || !matches!(record.spec.class, ClassSpec::Firecracker(_))
            {
                return Err(lifecycle(&expected.id, LifecycleCause::Uncertain)?);
            }
            expected.check()?;
            let trace = record
                .init
                .ok_or(lifecycle(&expected.id, LifecycleCause::Uncertain)?)?;
            if !recorded_running(&expected.id, trace)? {
                return Ok(());
            }
            self.check_process(&record)?;
            let client = self.client(&record)?;
            self.translate(expected, &client, None)?;
            let actual = client.inspect(&expected.id)?;
            if actual.class != Class::Oci || actual.bounds != expected.bounds {
                return Err(lifecycle(&expected.id, LifecycleCause::Uncertain)?);
            }
            Ok(())
        })
    }

    /// Standalone recovery, also usable by an embedding service. Node routes records by actual
    /// backend before any native cleanup and may instead call validate_binding/delete individually.
    pub fn recover(&self) -> Result<(), Refusal> {
        let records = self.authority()?.records()?;
        for (id, record) in records {
            let record = record.map_err(|_| {
                Refusal::Broken("unreadable native record prevents VM recovery".into())
            })?;
            if !matches!(record.spec.class, ClassSpec::Firecracker(_)) {
                continue;
            }
            let keys = self.catalog.conflict_ids(&record.spec.devices)?;
            crate::devices::with_transaction(&self.books, &id, &keys, |transaction| {
                match record.phase {
                    Phase::Creating | Phase::Deleting => self.remove(&id, transaction)?,
                    Phase::Ready => {
                        let trace = record
                            .init
                            .ok_or(lifecycle(&id, LifecycleCause::Uncertain)?)?;
                        if !recorded_running(&id, trace)? {
                            return Ok(());
                        }
                        self.check_process(&record)?;
                        let client = self.client(&record)?;
                        self.translate(&record.spec, &client, None)?;
                        let actual = client.inspect(&id)?;
                        if actual.class != Class::Oci {
                            return Err(invalid(
                                "recovered guest container belongs to another native backend",
                            ));
                        }
                    }
                }
                Ok(())
            })?;
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

fn lifecycle(id: &BoxId, cause: LifecycleCause) -> Result<Refusal, Refusal> {
    Ok(Refusal::Lifecycle {
        id: id.try_clone()?,
        cause,
    })
}

/// Observation only: a reused numeric PID is never signal authority. A retained process which
/// escaped/disagrees with its owned cgroup is uncertainty, not absence and not a foreign kill.
fn recorded_running(id: &BoxId, trace: ProcId) -> Result<bool, Refusal> {
    use rustix::{
        event::{PollFd, PollFlags, Timespec},
        process::{Pid, PidfdFlags},
    };
    use std::os::fd::AsFd;
    let raw =
        i32::try_from(trace.pid).map_err(|_| invalid("recorded process ID is unrepresentable"))?;
    let pid = Pid::from_raw(raw).ok_or_else(|| invalid("invalid recorded process ID"))?;
    let descriptor = match rustix::process::pidfd_open(pid, PidfdFlags::empty()) {
        Ok(descriptor) => descriptor,
        Err(rustix::io::Errno::SRCH) => return Ok(false),
        Err(error) => return Err(Refusal::os("observing recorded VMM identity", error)),
    };
    let borrowed = descriptor.as_fd();
    let mut observed = [PollFd::new(&borrowed, PollFlags::IN)];
    let zero = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    rustix::event::poll(&mut observed, Some(&zero))
        .map_err(|e| Refusal::os("observing VMM termination", e))?;
    let flags = observed
        .first()
        .ok_or_else(|| invalid("VMM observation descriptor absent"))?
        .revents();
    if flags.intersects(PollFlags::IN | PollFlags::HUP) {
        return Ok(false);
    }
    if !flags.is_empty() {
        return Err(invalid("VMM termination observation failed"));
    }
    if trace.start_ticks == 0 {
        return Err(lifecycle(id, LifecycleCause::Uncertain)?);
    }
    let current = start_time(raw).ok_or(lifecycle(id, LifecycleCause::Uncertain)?)?;
    if current != trace.start_ticks {
        return Ok(false);
    }
    Ok(true)
}

impl Runtime for FirecrackerRuntime {
    fn classes(&self) -> Vec<Class> {
        vec![Class::Firecracker]
    }

    fn capacity(&self, class: Class) -> Result<Option<hex_api::Capacity>, Refusal> {
        if class != Class::Firecracker {
            return Err(invalid("capacity requested for another backend"));
        }
        let records = self.authority()?.records()?;
        let mut capacity = self.authority()?.capacity()?;
        let observed = if self
            .catalog
            .entries_for(Route::Vfio)
            .any(|entry| entry.info.id.kind == DeviceKind::Gpu)
        {
            self.authority()?.physical_devices()?
        } else {
            Vec::new()
        };
        capacity.id = self.config.pool_id.clone();
        capacity.cost = Some(self.config.cost);
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
            vfio::validate(entry)?;
            if entry.info.id.kind == DeviceKind::Gpu {
                // An administrator's catalog is not current VRAM/load evidence. Prebound VFIO
                // may hide host NVML: only the same initialized facts provider may supply this
                // actual identity/metadata observation (including a witnessed live guest).
                let Some(actual) = observed.iter().find(|actual| actual.id == entry.info.id) else {
                    continue;
                };
                if actual.model != entry.info.model || actual.size != entry.info.size {
                    return Err(invalid(
                        "current GPU observation disagrees with its native route",
                    ));
                }
            }
            capacity
                .devices
                .push(entry.report(self.catalog.holder(entry.info.id, &records)?)?);
        }
        capacity.check()?;
        Ok(Some(capacity))
    }

    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        spec.check()?;
        if !matches!(spec.class, ClassSpec::Firecracker(_)) {
            return Err(invalid(
                "endpoint only executes explicit Firecracker payloads",
            ));
        }
        let keys = self.catalog.conflict_ids(&spec.devices)?;
        crate::devices::with_transaction(&self.books, &spec.id, &keys, |transaction| {
            if transaction.read(&spec.id)?.is_some() {
                return Err(Refusal::Exists(spec.id.try_clone()?));
            }
            match self.create_inner(&spec, transaction, &self.paths(&spec.id)) {
                Ok(info) => Ok(info),
                Err(error) => match transaction.read(&spec.id)? {
                    Some(_) => match self.remove(&spec.id, transaction) {
                        Ok(()) => Err(error),
                        Err(cleanup) => Err(Refusal::Broken(format!(
                            "VM create refused ({error}); physical cleanup remains uncertain ({cleanup})"
                        ))),
                    },
                    None => {
                        self.forget_reserved_child(&spec.id)?;
                        Err(error)
                    }
                },
            }
        })
    }

    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        self.with_record(id, |record| self.client(record)?.start(id))
    }
    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        self.with_record(id, |record| self.client(record)?.exec(id, program, stdio))
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
        self.with_record(id, |record| self.client(record)?.signal(id, signal))
    }
    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
        self.with_record(id, |record| {
            if record.phase == Phase::Ready {
                let trace = record
                    .init
                    .ok_or(lifecycle(id, LifecycleCause::Uncertain)?)?;
                if !recorded_running(id, trace)? {
                    return Ok(BoxInfo {
                        id: id.try_clone()?,
                        class: Class::Firecracker,
                        state: hex_api::BoxState::Empty,
                        bounds: record.spec.bounds.clone(),
                        allocation: record.spec.allocation,
                        adopted: false,
                        membership: None,
                        exit: record.exit,
                        abandoned: record.abandoned,
                    });
                }
            }
            let mut info = self.client(record)?.inspect(id)?;
            info.class = Class::Firecracker;
            info.bounds = record.spec.bounds.clone();
            info.allocation = record.spec.allocation;
            Ok(info)
        })
    }
    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        self.with_record(id, |record| {
            self.check_process(record)?;
            self.cgroups
                .get(id)
                .map(|cg| cg.usage())
                .ok_or_else(|| Refusal::NoSuchBox(id.clone()))
        })
    }
    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        let records = self.authority()?.records()?;
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
