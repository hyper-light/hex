//! AC-T7.4.1: initialized backend routing on the existing Node/Store/Turns owner.

use super::*;
use hex_api::{Capacity, DeviceId, LifecycleCause};
use hex_runtime::devices::{Books, Catalog, Records, Transaction};
use hex_runtime::firecracker::FirecrackerRuntime;
use hex_runtime::guest::GuestRuntime;
use std::sync::Weak;

pub(super) struct Backends {
    firecracker: Option<Arc<FirecrackerRuntime>>,
    shards: Option<Arc<GuestRuntime>>,
}

impl Node {
    fn managed_bounds(spec: &BoxSpec) -> Result<hex_api::Bounds, Refusal> {
        if !matches!(spec.class, ClassSpec::Shards(_)) {
            return Err(uncertain(&spec.id));
        }
        let allocation = spec.allocation.ok_or_else(|| uncertain(&spec.id))?;
        allocation.guest()?;
        Ok(hex_api::Bounds {
            cpu_max_milli: Some(
                u32::try_from(allocation.resources.cpu_milli).map_err(|_| uncertain(&spec.id))?,
            ),
            memory_max: Some(allocation.resources.mem_bytes),
            ..hex_api::Bounds::default()
        })
    }

    fn launch_managed(
        &self,
        spec: &BoxSpec,
        plan: &shards_api::PreparedBoot,
        io: shards_api::LaunchIo,
        publish: &mut dyn FnMut(&shards_api::Identity) -> Result<(), Refusal>,
    ) -> Result<shards_api::ManagedReceipt, Refusal> {
        spec.check()?;
        let bounds = Self::managed_bounds(spec)?;
        let guest = self
            .backends
            .get()
            .and_then(|backends| backends.shards.as_ref())
            .ok_or_else(|| uncertain(&spec.id))?;
        let gate_budget = guest.first_dial_bound()?;
        let mut program = Program::default();
        let count = plan
            .arguments()
            .len()
            .checked_add(1)
            .ok_or_else(|| uncertain(&spec.id))?;
        program
            .args
            .try_reserve_exact(count)
            .map_err(|_| Refusal::Full(count))?;
        program
            .args
            .push(native_argument(plan.binary().as_os_str())?);
        for argument in plan.arguments() {
            program.args.push(native_argument(argument)?);
        }
        // A native binary's carried helper cache is in this same Node's owned state, selected
        // explicitly rather than inheriting workload variables or credentials from its caller.
        let cache = self
            .bundles
            .parent()
            .ok_or_else(|| uncertain(&spec.id))?
            .join("shards-native-cache");
        program
            .env
            .try_reserve_exact(1)
            .map_err(|_| Refusal::Full(1))?;
        program.env.push((
            native_argument(std::ffi::OsStr::new("SHARDS_CACHE"))?,
            native_argument(cache.as_os_str())?,
        ));
        program.check()?;
        program.fits(&process::exec_limit())?;
        let cgroup = self.root.create(&spec.id, &bounds)?;
        let native = self
            .supervisor
            .reserve_managed(&spec.id, cgroup, plan.vsock())?;
        let stdio = Stdio {
            stdin: None,
            stdout: Some(io.stdout),
            stderr: Some(io.stderr),
            ..Stdio::default()
        };
        self.supervisor
            .bind_managed(&native, &program, stdio, gate_budget, publish)
    }

    fn attach_managed(
        &self,
        spec: &BoxSpec,
        identity: Option<&shards_api::Identity>,
    ) -> Result<shards_api::ManagedReceipt, Refusal> {
        spec.check()?;
        Self::managed_bounds(spec)?;
        if let Some(native) = self.supervisor.observe_managed(&spec.id, identity)? {
            return Ok(native);
        }
        // A reusable box id, socket or cgroup path cannot prove an old native incarnation.
        // Cold recovery retains the claims until the canonical NativeScope receipt is joined.
        Err(uncertain(&spec.id))
    }

    fn finalize_managed(
        &self,
        spec: &BoxSpec,
        identity: Option<&shards_api::Identity>,
    ) -> Result<(), Refusal> {
        spec.check()?;
        Self::managed_bounds(spec)?;
        self.supervisor.finalize_managed(&spec.id, identity)?;
        self.root.forget(spec.id.as_str());
        Ok(())
    }

    /// Supply the one native provider before initializing any backend. A separate scheduler
    /// process obtains its physical report over Runtime; it never opens a second Gpus allocation.
    /// This never opens a Gpus instance or calls a capacity/attachment route recursively.
    pub fn bind_prober(&self, prober: crate::facts::SharedProber) -> Result<(), Refusal> {
        if self.backends.get().is_some() {
            return Err(Refusal::Broken(
                "physical provider must be supplied before backend initialization".into(),
            ));
        }
        self.prober
            .set(prober)
            .map_err(|_| Refusal::Broken("physical facts provider is already supplied".into()))
    }
    /// Reuse this Arc's authority; the backend retains only Weak, never another owner or cycle.
    pub fn backend_books(self: &Arc<Self>) -> Weak<dyn Books> {
        let books: Arc<dyn Books> = self.clone();
        Arc::downgrade(&books)
    }

    pub fn device_catalog(&self) -> Arc<Catalog> {
        Arc::clone(&self.attachments)
    }

    /// A sibling outside the native supervisor/orphan prefix, with the same Node identity.
    pub fn firecracker_cgroups(&self) -> Result<BoxRoot, Refusal> {
        let mut disks = Vec::new();
        disks
            .try_reserve_exact(self.root.disks().len())
            .map_err(|_| Refusal::Exhausted {
                what: "VM disk controller identities",
                bound: self.root.disks().len(),
            })?;
        disks.extend_from_slice(self.root.disks());
        let parent = self
            .root
            .dir()
            .parent()
            .ok_or_else(|| Refusal::Broken("native box root has no delegated parent".into()))?;
        BoxRoot::open(
            &parent.join(format!("@hex-firecracker-{}", self.key)),
            disks,
        )
    }

    /// ROOT calls this once after Arc<Node> and before recovery/serve. No class is advertised while
    /// its configured adapter/cost is uninitialized; absence keeps the native-only Node.
    pub fn install_backends(
        self: &Arc<Self>,
        firecracker: Option<Arc<FirecrackerRuntime>>,
        shards: Option<Arc<GuestRuntime>>,
    ) -> Result<(), Refusal> {
        if self.backends.get().is_some() {
            return Err(Refusal::Broken(
                "node backends are already initialized".into(),
            ));
        }
        if let Some(guest) = &shards {
            guest.bind_books(self.backend_books())?;
        }
        for (class, backend) in [
            (
                Class::Firecracker,
                firecracker
                    .as_deref()
                    .map(|backend| backend as &dyn Runtime),
            ),
            (
                Class::Shards,
                shards.as_deref().map(|backend| backend as &dyn Runtime),
            ),
        ] {
            if let Some(backend) = backend {
                if !backend.classes().contains(&class) {
                    return Err(missing(class));
                }
                let capacity = backend.capacity(class)?.ok_or_else(|| missing(class))?;
                capacity.check()?;
                if capacity.cost.is_none() {
                    return Err(missing(class));
                }
                // Pool identity names a resource envelope, not the physical-provider authority.
                // Guest pools may differ; Books above binds their physical claims to this Node.
            }
        }
        self.backends
            .set(Backends {
                firecracker,
                shards,
            })
            .map_err(|_| {
                Refusal::Broken("node backend initialization raced another initializer".into())
            })
    }

    pub(super) fn native_capacity(&self) -> Result<Capacity, Refusal> {
        crate::facts::capacity(&self.bundles, format!("node:{}", self.key))
    }

    pub(super) fn native_physical_report(&self) -> Result<hex_api::PhysicalReport, Refusal> {
        let provider = self.prober.get().ok_or_else(|| {
            Refusal::Broken("the native physical provider is not initialized".into())
        })?;
        let mut authority = String::new();
        let bytes = "node:"
            .len()
            .checked_add(self.key.len())
            .ok_or_else(|| Refusal::Full(MAX_BOXES))?;
        authority
            .try_reserve_exact(bytes)
            .map_err(|_| Refusal::Exhausted {
                what: "native authority identity bytes",
                bound: bytes,
            })?;
        authority.push_str("node:");
        authority.push_str(&self.key);
        provider
            .lock()
            .map_err(|_| Refusal::Broken("physical facts provider lock poisoned".into()))?
            .physical_report(authority)
    }

    pub(super) fn backend(&self, class: Class) -> Result<Option<&dyn Runtime>, Refusal> {
        match class {
            Class::Process | Class::Oci => Ok(None),
            Class::Firecracker => self
                .backends
                .get()
                .and_then(|backends| backends.firecracker.as_deref())
                .map(|backend| Some(backend as &dyn Runtime))
                .ok_or_else(|| missing(class)),
            Class::Shards => self
                .backends
                .get()
                .and_then(|backends| backends.shards.as_deref())
                .map(|backend| Some(backend as &dyn Runtime))
                .ok_or_else(|| missing(class)),
        }
    }

    /// Persisted class dispatch happens before native container/cgroup paths or method turns.
    pub(super) fn backend_record(&self, id: &BoxId) -> Result<Option<&dyn Runtime>, Refusal> {
        let Some(record) = self.store.read_lifecycle(id)? else {
            return Ok(None);
        };
        self.backend(record.spec.class.class())
            .map_err(|_| uncertain(id))
    }

    pub(super) fn initialized_classes(&self) -> Vec<Class> {
        let mut classes = vec![Class::Process, Class::Oci];
        if let Some(backends) = self.backends.get() {
            if backends.firecracker.is_some() {
                classes.push(Class::Firecracker);
            }
            if backends.shards.is_some() {
                classes.push(Class::Shards);
            }
        }
        classes
    }

    /// Preflight every backend before recovery is allowed to mutate ANY native resource. Unknown
    /// records cannot be treated as native Process/OCI cleanup authority.
    pub(super) fn recover_backends(&self, records: &Records) -> Result<RecoverySummary, Refusal> {
        for (id, record) in records {
            let record = record.as_ref().map_err(|_| uncertain(id))?;
            if record.spec.id != *id {
                return Err(uncertain(id));
            }
            record.spec.check()?;
            if matches!(
                record.spec.class,
                ClassSpec::Firecracker(_) | ClassSpec::Shards(_)
            ) && record.spec.allocation.is_none()
            {
                return Err(uncertain(id));
            }
            self.backend(record.spec.class.class())
                .map_err(|_| uncertain(id))?;
        }
        let mut summary = RecoverySummary::default();
        summary
            .adopted
            .try_reserve_exact(records.len())
            .map_err(|_| Refusal::Full(MAX_BOXES))?;
        summary
            .removed
            .try_reserve_exact(records.len())
            .map_err(|_| Refusal::Full(MAX_BOXES))?;
        for (id, record) in records {
            let record = record.as_ref().map_err(|_| uncertain(id))?;
            let Some(backend) = self
                .backend(record.spec.class.class())
                .map_err(|_| uncertain(id))?
            else {
                continue;
            };
            match record.phase {
                Phase::Creating | Phase::Deleting => {
                    backend.delete(id)?;
                    summary.removed.push((
                        id.try_clone()?,
                        "actual VM backend completed retained cleanup".into(),
                    ));
                }
                Phase::Ready => {
                    match record.spec.class.class() {
                        Class::Firecracker => self
                            .backends
                            .get()
                            .and_then(|backends| backends.firecracker.as_ref())
                            .ok_or_else(|| uncertain(id))?
                            .validate_binding(&record.spec)?,
                        Class::Shards => self
                            .backends
                            .get()
                            .and_then(|backends| backends.shards.as_ref())
                            .ok_or_else(|| uncertain(id))?
                            .recover_managed(&record.spec)?,
                        Class::Process | Class::Oci => return Err(uncertain(id)),
                    }
                    let actual = backend.inspect(id)?;
                    if actual.class != record.spec.class.class()
                        || actual.allocation != record.spec.allocation
                    {
                        return Err(uncertain(id));
                    }
                    summary.adopted.push(id.try_clone()?);
                }
            }
        }
        Ok(summary)
    }
}

fn native_argument(argument: &std::ffi::OsStr) -> Result<String, Refusal> {
    let argument = argument.to_str().ok_or_else(|| Refusal::Invalid {
        what: "managed native program",
        why: "configured native argument is not UTF-8".into(),
    })?;
    let mut value = String::new();
    value
        .try_reserve_exact(argument.len())
        .map_err(|_| Refusal::Exhausted {
            what: "managed native argument bytes",
            bound: argument.len(),
        })?;
    value.push_str(argument);
    Ok(value)
}

impl Books for Node {
    fn records(&self) -> Result<Records, Refusal> {
        self.store.all()
    }
    fn capacity(&self) -> Result<Capacity, Refusal> {
        self.native_capacity()
    }
    fn physical_devices(&self) -> Result<Vec<hex_api::DeviceInfo>, Refusal> {
        let provider = self.prober.get().ok_or_else(|| {
            Refusal::Broken("the Node physical facts provider was not initialized".into())
        })?;
        crate::facts::physical_devices(provider)
    }
    fn with_transaction(
        &self,
        id: &BoxId,
        devices: &[DeviceId],
        callback: &mut dyn FnMut(&dyn Transaction) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        let _method = self.turns.try_take(id)?;
        let keys = self.attachments.conflict_ids(devices)?;
        let _devices = self.hold_attachments(&keys)?;
        let launch =
            |spec: &BoxSpec,
             plan: &shards_api::PreparedBoot,
             io: shards_api::LaunchIo,
             publish: &mut dyn FnMut(&shards_api::Identity) -> Result<(), Refusal>| {
                self.launch_managed(spec, plan, io, publish)
            };
        let attach = |spec: &BoxSpec, identity: Option<&shards_api::Identity>| {
            self.attach_managed(spec, identity)
        };
        let finalize = |spec: &BoxSpec, identity: Option<&shards_api::Identity>| {
            self.finalize_managed(spec, identity)
        };
        crate::recorded::transaction(
            &self.store,
            id,
            &keys,
            &crate::recorded::Native {
                launch: &launch,
                attach: &attach,
                finalize: &finalize,
            },
            callback,
        )
    }
}

pub(super) fn uncertain(id: &BoxId) -> Refusal {
    match id.try_clone() {
        Ok(id) => Refusal::Lifecycle {
            id,
            cause: LifecycleCause::Uncertain,
        },
        Err(refusal) => refusal,
    }
}
fn missing(class: Class) -> Refusal {
    Refusal::Platform {
        what: "initialized VM backend",
        platform: "Linux node",
        why: format!(
            "{} backend or verified cost is not initialized",
            class.name()
        ),
    }
}
