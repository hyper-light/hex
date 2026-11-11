//! AC-T7.4.1: backend-owned physical bindings. Records remain the holder authority.
//! The catalog is immutable configuration, not a second admission ledger or discovery cache.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

use hex_api::{BoxId, BoxSpec, DeviceId, DeviceInfo, DeviceKind, Refusal};
use hex_core::record::{BoxRecord, NativeRecord, NativeStage, PoolRecord};

/// The already initialized native adapter supplies its actual payload platform. This helper
/// describes native execution only; it never substitutes a guest for an OCI implementation.
pub fn native_routes(
    capacity: &mut hex_api::Capacity,
    form: hex_api::Class,
    image: hex_api::ImagePlatform,
) -> Result<(), Refusal> {
    capacity.check()?;
    if !matches!(form, hex_api::Class::Process | hex_api::Class::Oci)
        || capacity.cost.is_some()
        || !capacity.routes.is_empty()
        || !capacity.device_routes.is_empty()
        || !capacity.backings.is_empty()
    {
        return Err(invalid(
            "native route producer received another execution topology",
        ));
    }
    let process = hex_api::RouteOffer {
        key: hex_api::RouteKey {
            class: hex_api::Class::Process,
            image,
            execution: hex_api::Execution::Native,
            gpu: hex_api::GpuEligibility::None,
        },
        cost: None,
        backing: None,
    };
    let oci = hex_api::RouteOffer {
        key: hex_api::RouteKey {
            class: hex_api::Class::Oci,
            image,
            execution: hex_api::Execution::Native,
            gpu: hex_api::GpuEligibility::Any,
        },
        cost: None,
        backing: None,
    };
    process.check()?;
    if form == hex_api::Class::Oci {
        oci.check()?;
    }
    capacity
        .routes
        .try_reserve_exact(2)
        .map_err(|_| allocation(2))?;
    capacity
        .device_routes
        .try_reserve_exact(capacity.devices.len())
        .map_err(|_| allocation(capacity.devices.len()))?;
    capacity.cost = None;
    capacity.routes.push(process);
    if form == hex_api::Class::Oci {
        capacity.routes.push(oci);
    }
    // Process never acquires physical devices; bit1 names the actual native OCI route above.
    capacity.device_routes.resize(
        capacity.devices.len(),
        if form == hex_api::Class::Oci { 2 } else { 0 },
    );
    capacity.check()
}

/// A trusted administrator binds one physical resource to an existing native adapter.
/// CDI hooks/paths and HCS location paths are backend configuration, never peer payloads.
#[derive(Debug)]
pub enum Binding {
    Cdi {
        file: String,
        qualified: String,
    },
    /// A dedicated VM's attachment receipt, accepted only with that initialized guest's immutable
    /// boot authority. This recipe is not ordinary host CDI or measured GPU capacity.
    GuestGpu {
        file: String,
        qualified: String,
    },
    Hcs {
        location: String,
    },
    /// Exact native selector through the existing Shards API; the initialized provider must
    /// establish native correspondence before advertising an attachment capability.
    ShardsGpu {
        physical: shards_api::PhysicalGpu,
    },
    /// Prebound Linux PCI function and its administrator-declared IOMMU isolation group.
    Vfio {
        sbdf: String,
        group: u32,
        companions: Vec<String>,
    },
}

/// A native route is configuration only. Its initialized adapter must verify current hardware
/// correspondence before publishing a capability; choosing a route never switches a host driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Cdi,
    GuestGpu,
    Hcs,
    ShardsGpu,
    Vfio,
}

impl Binding {
    pub fn route(&self) -> Route {
        match self {
            Self::Cdi { .. } => Route::Cdi,
            Self::GuestGpu { .. } => Route::GuestGpu,
            Self::Hcs { .. } => Route::Hcs,
            Self::ShardsGpu { .. } => Route::ShardsGpu,
            Self::Vfio { .. } => Route::Vfio,
        }
    }
}

pub struct Entry {
    pub info: DeviceInfo,
    bindings: Vec<Binding>,
}

impl Entry {
    pub fn binding(&self, route: Route) -> Option<&Binding> {
        self.bindings
            .iter()
            .find(|binding| binding.route() == route)
    }

    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    fn group(&self) -> Option<u32> {
        match self.binding(Route::Vfio) {
            Some(Binding::Vfio { group, .. }) => Some(*group),
            _ => None,
        }
    }

    pub fn report(&self, holder: Option<hex_api::DeviceHolder>) -> Result<DeviceInfo, Refusal> {
        Ok(DeviceInfo {
            id: self.info.id,
            model: copy(&self.info.model)?,
            size: self.info.size,
            holder,
        })
    }
}

#[derive(Default)]
pub struct Catalog {
    entries: Vec<Entry>,
}

/// One immutable, fallibly allocated catalog shared by the initialized native adapters.
pub type SharedCatalog = triomphe::Arc<Catalog>;

pub fn shared_catalog(path: &Path) -> Result<SharedCatalog, Refusal> {
    SharedCatalog::try_new(Catalog::open(path)?).map_err(|_| Refusal::Exhausted {
        what: "shared runtime device catalog owners",
        bound: 1,
    })
}

/// Encode the persisted incarnation, never a reusable workload id, as a native path component.
pub fn managed_scope_id(record: &BoxRecord) -> Result<BoxId, Refusal> {
    managed_native_scope_id(NativeRecord::Box(record))
}

pub fn managed_native_scope_id(record: NativeRecord<'_>) -> Result<BoxId, Refusal> {
    let refused = || record.refusal(hex_api::LifecycleCause::Uncertain);
    let scope = record.scope().ok_or_else(refused)?;
    let room = scope.bytes().len().checked_mul(2).ok_or_else(refused)?;
    let mut name = String::new();
    name.try_reserve_exact(room)
        .map_err(|_| Refusal::Exhausted {
            what: "native scope name bytes",
            bound: room,
        })?;
    for byte in scope.bytes() {
        for digit in [byte >> 4, byte & 15] {
            let byte = b"0123456789abcdef"
                .get(usize::from(digit))
                .copied()
                .ok_or_else(refused)?;
            name.push(char::from(byte));
        }
    }
    BoxId::try_new(&name)
}

/// Parsed configuration for one create. No background registry, watcher or mutable copy is kept.
pub struct CdiPlan<'a> {
    specs: Vec<(hex_core::cdi::Spec, Vec<&'a str>)>,
}

pub type Records = BTreeMap<BoxId, Result<BoxRecord, String>>;
pub type PoolRecords = Vec<PoolRecord>;

/// Two actual native command sources, consumed by the SAME canonical record/native owner.
/// This is an internal borrowed launch input, never a peer-supplied payload or backend fallback.
pub enum ManagedLaunch<'a> {
    Shards(&'a shards_api::PreparedBoot),
    Firecracker {
        program: &'a hex_api::Program,
        vsock: &'a Path,
    },
}

impl ManagedLaunch<'_> {
    pub fn class(&self) -> hex_api::Class {
        match self {
            Self::Shards(_) => hex_api::Class::Shards,
            Self::Firecracker { .. } => hex_api::Class::Firecracker,
        }
    }

    pub fn vsock(&self) -> &Path {
        match self {
            Self::Shards(plan) => plan.vsock(),
            Self::Firecracker { vsock, .. } => vsock,
        }
    }
}

/// Shared pre-effect guard for the existing Node/Recorded native transaction consumers. The
/// writer's final OS duplication/rendering stays at its native launcher, never in this metadata.
pub fn check_managed_report(
    catalog: Option<&Catalog>,
    record: NativeRecord<'_>,
    plan: &ManagedLaunch<'_>,
    writer: bool,
    deadline: Instant,
) -> Result<Instant, Refusal> {
    let refused = || record.refusal(hex_api::LifecycleCause::Uncertain);
    match record {
        NativeRecord::Box(record) => record.spec.check()?,
        NativeRecord::Pool(record) => record.check()?,
    }
    if plan.class() != record.class()?
        || record.stage() != NativeStage::Creating
        || record.init().is_some()
        || record.scope().is_none()
    {
        return Err(refused());
    }
    let request = match plan {
        ManagedLaunch::Shards(prepared) => prepared.gpu_report_request(),
        ManagedLaunch::Firecracker { .. } => None,
    };
    let required = matches!(plan, ManagedLaunch::Shards(_)) && !record.devices().is_empty();
    check_report_scope(
        record,
        request.as_ref().map(|request| request.scope()),
        writer,
        required,
    )?;
    let deadline = if let Some(request) = request {
        check_report_selection(catalog.ok_or_else(refused)?, record, request.selected())?;
        deadline.min(request.deadline())
    } else {
        deadline
    };
    if deadline <= Instant::now() {
        return Err(record.refusal(hex_api::LifecycleCause::Pending));
    }
    Ok(deadline)
}

fn check_report_scope(
    record: NativeRecord<'_>,
    requested: Option<[u8; 16]>,
    writer: bool,
    required: bool,
) -> Result<(), Refusal> {
    if requested.is_some() != writer
        || requested.is_some() != required
        || requested.is_some_and(|requested| {
            record
                .scope()
                .is_none_or(|scope| *scope.bytes() != requested)
        })
    {
        return Err(record.refusal(hex_api::LifecycleCause::Uncertain));
    }
    Ok(())
}

fn check_report_selection(
    catalog: &Catalog,
    record: NativeRecord<'_>,
    selected: &[shards_api::PhysicalGpu],
) -> Result<(), Refusal> {
    let refused = || record.refusal(hex_api::LifecycleCause::Uncertain);
    shards_api::physical::check_selection(selected).map_err(|_| refused())?;
    if selected.len() != record.devices().len() {
        return Err(refused());
    }
    let mut indexed = Vec::new();
    indexed
        .try_reserve_exact(catalog.entries().len())
        .map_err(|_| Refusal::Exhausted {
            what: "managed report catalog index",
            bound: hex_api::MAX_DEVICES,
        })?;
    indexed.extend(catalog.entries());
    indexed.sort_unstable_by_key(|entry| entry.info.id);
    let mut expected = Vec::new();
    expected
        .try_reserve_exact(record.devices().len())
        .map_err(|_| Refusal::Exhausted {
            what: "managed report physical selection",
            bound: hex_api::MAX_DEVICES,
        })?;
    for id in record.devices() {
        let entry = indexed
            .binary_search_by_key(id, |entry| entry.info.id)
            .ok()
            .and_then(|at| indexed.get(at))
            .ok_or_else(refused)?;
        let Some(Binding::ShardsGpu { physical }) = entry.binding(Route::ShardsGpu) else {
            return Err(refused());
        };
        expected.push(*physical);
    }
    expected.sort_unstable();
    if expected
        .windows(2)
        .any(|pair| matches!(pair, [a,b] if a == b))
        || selected
            .iter()
            .any(|physical| expected.binary_search(physical).is_err())
    {
        return Err(refused());
    }
    Ok(())
}

/// The existing native record owner, shared by OCI, Firecracker and Shards. The callback is
/// synchronous and invoked exactly once while concrete existing box/device turns live on stack.
/// It performs no implicit commit: every physical/record transition remains explicit to its caller.
pub trait Books: Send + Sync {
    fn records(&self) -> Result<Records, Refusal>;
    fn pool_records(&self) -> Result<PoolRecords, Refusal>;
    fn prepare_pool(
        &self,
        pool: [u8; 32],
        execution: hex_api::Execution,
        cost: hex_api::RuntimeCost,
        devices: &[DeviceId],
    ) -> Result<hex_api::BackingOffer, Refusal>;
    fn capacity(&self) -> Result<hex_api::Capacity, Refusal>;
    /// Direct current native observations from this Node's SAME facts provider. Never reenter
    /// capacity/attachment_offers here: those consumers validate routes against this view.
    fn physical_devices(&self) -> Result<Vec<DeviceInfo>, Refusal>;
    /// In-process native correspondence from the SAME initialized provider. The eight LUID bytes
    /// are current-boot evidence, never a stable DeviceId, holder grant or wire inventory.
    fn physical_luid(&self, id: DeviceId) -> Result<[u8; 8], Refusal> {
        id.check()?;
        Err(broken(
            "the native owner has no verified current LUID mapping for this physical identity",
        ))
    }
    fn with_transaction(
        &self,
        id: &BoxId,
        devices: &[DeviceId],
        callback: &mut dyn FnMut(&dyn Transaction) -> Result<(), Refusal>,
    ) -> Result<(), Refusal>;
    fn with_pool_transaction(
        &self,
        key: &hex_api::BackingKey,
        devices: &[DeviceId],
        callback: &mut dyn FnMut(&dyn Transaction) -> Result<(), Refusal>,
    ) -> Result<(), Refusal>;
    /// One source Pool configuration turn, then the Box/full-source-conflict turns. The same
    /// concrete Box transaction validates/persists its subset relation; no native effect is implicit.
    fn with_handoff_transaction(
        &self,
        id: &BoxId,
        source: &hex_api::BackingKey,
        full_conflicts: &[DeviceId],
        callback: &mut dyn FnMut(&dyn Transaction) -> Result<(), Refusal>,
    ) -> Result<(), Refusal>;
}

pub trait Transaction {
    fn records(&self) -> Result<Records, Refusal>;
    fn pool_records(&self) -> Result<PoolRecords, Refusal>;
    fn read_pool(&self, key: &hex_api::BackingKey) -> Result<Option<PoolRecord>, Refusal>;
    fn put_pool(&self, record: &PoolRecord) -> Result<(), Refusal>;
    fn grant_pool(&self, grant: &hex_api::BackingGrant) -> Result<PoolRecord, Refusal>;
    fn remove_pool(&self, key: &hex_api::BackingKey) -> Result<(), Refusal>;
    fn read(&self, id: &BoxId) -> Result<Option<BoxRecord>, Refusal>;
    fn put(&self, record: &BoxRecord) -> Result<(), Refusal>;
    /// Called only after physical termination/absence and owned state removal are proved.
    fn remove(&self, id: &BoxId) -> Result<(), Refusal>;
    /// Launch in the original record's admitted native envelope from the first instruction.
    /// The existing native receipt alone owns observation/reaping; no Process record is added.
    fn launch_managed(
        &self,
        plan: &ManagedLaunch<'_>,
        io: shards_api::LaunchIo,
        patience: Duration,
    ) -> Result<shards_api::ManagedReceipt, Refusal>;
    /// Reattach the exact persisted native identity and envelope without creating a process.
    /// None is cleanup-only for Creating/Deleting before identity publication, never a usable
    /// guest. A controller exit cannot prove that helpers or physical grants have been released.
    fn attach_managed(
        &self,
        identity: Option<&shards_api::Identity>,
    ) -> Result<shards_api::ManagedReceipt, Refusal>;
    /// Dispose the same native receipt only after the backend durably records its positive whole
    /// scope release. A controller exit alone is insufficient; matching identity/owner guards ABA.
    fn finalize_managed(&self, identity: Option<&shards_api::Identity>) -> Result<(), Refusal>;
    /// Read this canonical scope's native counters directly, without runtime/capacity recursion.
    fn managed_usage(&self) -> Result<hex_api::Usage, Refusal>;
}

/// Upgrade the same Node authority without allocating an owner or transaction. The operation's
/// result stays on the caller's stack; a bad provider cannot invoke a physical effect twice.
pub fn with_transaction<T>(
    books: &std::sync::Weak<dyn Books>,
    id: &BoxId,
    devices: &[DeviceId],
    operation: impl FnOnce(&dyn Transaction) -> Result<T, Refusal>,
) -> Result<T, Refusal> {
    transaction(books, operation, |books, callback| {
        books.with_transaction(id, devices, callback)
    })
}

pub fn with_pool_transaction<T>(
    books: &std::sync::Weak<dyn Books>,
    key: &hex_api::BackingKey,
    devices: &[DeviceId],
    operation: impl FnOnce(&dyn Transaction) -> Result<T, Refusal>,
) -> Result<T, Refusal> {
    transaction(books, operation, |books, callback| {
        books.with_pool_transaction(key, devices, callback)
    })
}

/// The SAME callback-once stack facade, with the existing owner holding both handoff turn kinds.
pub fn with_handoff_transaction<T>(
    books: &std::sync::Weak<dyn Books>,
    id: &BoxId,
    source: &hex_api::BackingKey,
    full_conflicts: &[DeviceId],
    operation: impl FnOnce(&dyn Transaction) -> Result<T, Refusal>,
) -> Result<T, Refusal> {
    transaction(books, operation, |books, callback| {
        books.with_handoff_transaction(id, source, full_conflicts, callback)
    })
}

fn transaction<T>(
    books: &std::sync::Weak<dyn Books>,
    operation: impl FnOnce(&dyn Transaction) -> Result<T, Refusal>,
    enter: impl FnOnce(
        &dyn Books,
        &mut dyn FnMut(&dyn Transaction) -> Result<(), Refusal>,
    ) -> Result<(), Refusal>,
) -> Result<T, Refusal> {
    let books = books
        .upgrade()
        .ok_or_else(|| broken("native record authority is gone"))?;
    let mut operation = Some(operation);
    let mut result = None;
    enter(books.as_ref(), &mut |transaction| {
        let operation = operation
            .take()
            .ok_or_else(|| broken("record transaction callback invoked more than once"))?;
        result = Some(operation(transaction));
        Ok(())
    })?;
    result.ok_or_else(|| broken("record transaction callback was not invoked"))?
}

fn broken(why: &'static str) -> Refusal {
    copy(why).map_or_else(|error| error, Refusal::Broken)
}

fn invalid(why: &'static str) -> Refusal {
    match copy(why) {
        Ok(why) => Refusal::Invalid {
            what: "runtime device catalog",
            why,
        },
        Err(error) => error,
    }
}

fn allocation(bound: usize) -> Refusal {
    Refusal::Exhausted {
        what: "runtime device catalog bytes",
        bound,
    }
}

fn copy(value: &str) -> Result<String, Refusal> {
    let mut owned = String::new();
    owned
        .try_reserve_exact(value.len())
        .map_err(|_| allocation(value.len()))?;
    owned.push_str(value);
    Ok(owned)
}

impl Catalog {
    /// An absent catalog advertises no attachment capability. Read at backend startup, so a
    /// configuration change takes effect on an ordinary backend restart, never halfway through create.
    pub fn open(path: &Path) -> Result<Self, Refusal> {
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(Refusal::os("opening runtime device catalog", error)),
        };
        // One excess byte distinguishes an exact-bound file from a truncated oversized file.
        // Reserve before reading; even a concurrently growing file cannot trigger an implicit growth.
        let bound = hex_core::bundle::MAX_CONFIG_BYTES;
        let room = bound.checked_add(1).ok_or_else(|| allocation(bound))?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(room)
            .map_err(|_| allocation(room))?;
        bytes.resize(room, 0);
        let mut used = 0usize;
        while used < room {
            let tail = bytes.get_mut(used..).ok_or_else(|| allocation(room))?;
            match file.read(tail) {
                Ok(0) => break,
                Ok(count) => used = used.checked_add(count).ok_or_else(|| allocation(room))?,
                Err(error) => return Err(Refusal::os("reading runtime device catalog", error)),
            }
        }
        if used > bound {
            return Err(invalid("configuration exceeds the OCI config byte bound"));
        }
        bytes.truncate(used);
        Self::parse(&bytes)
    }

    /// A flat, versioned administrator file; tabs delimit seven fields and cannot occur in values.
    /// This finite syntax avoids an arbitrary-depth JSON tree and fallible-string gaps in this path.
    pub fn parse(bytes: &[u8]) -> Result<Self, Refusal> {
        if bytes.len() > hex_core::bundle::MAX_CONFIG_BYTES {
            return Err(invalid("configuration exceeds the OCI config byte bound"));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| invalid("configuration is not UTF-8"))?;
        if text.contains('\0') {
            return Err(invalid("configuration contains NUL"));
        }
        let mut lines = text.lines();
        if lines.next() != Some("hex-devices-1") {
            return Err(invalid("unsupported or absent configuration version"));
        }
        let mut rows = Vec::new();
        for line in lines {
            // The existing scheduler uses u16 device indices; it cannot represent another entry.
            if rows.len() >= hex_api::MAX_DEVICES {
                return Err(invalid(
                    "configuration exceeds the scheduler device index space",
                ));
            }
            let mut fields = line.split('\t');
            let kind = match fields.next() {
                Some("gpu") => DeviceKind::Gpu,
                Some("vfio") => DeviceKind::Vfio,
                Some("block") => DeviceKind::Block,
                Some("net") => DeviceKind::Net,
                _ => return Err(invalid("unknown device kind")),
            };
            let key = fields
                .next()
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or_else(|| invalid("invalid physical device key"))?;
            let model = fields
                .next()
                .ok_or_else(|| invalid("missing model field"))?;
            let size = fields
                .next()
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or_else(|| invalid("invalid device size"))?;
            let adapter = fields
                .next()
                .ok_or_else(|| invalid("missing adapter field"))?;
            let source = fields
                .next()
                .ok_or_else(|| invalid("missing binding source"))?;
            let selector = fields
                .next()
                .ok_or_else(|| invalid("missing selector field"))?;
            if fields.next().is_some() {
                return Err(invalid("excess configuration fields"));
            }
            let id = DeviceId { kind, key };
            id.check()?;
            let binding = match adapter {
                "cdi" if source.starts_with('/') && !selector.is_empty() => Binding::Cdi {
                    file: copy(source)?,
                    qualified: copy(selector)?,
                },
                "guest-gpu"
                    if source.starts_with('/')
                        && kind == DeviceKind::Gpu
                        && model == "shards-renderer"
                        && size == 0
                        && selector
                            .strip_prefix("hex.shards/gpu=")
                            .and_then(|value| value.parse::<u64>().ok())
                            == Some(key) =>
                {
                    Binding::GuestGpu {
                        file: copy(source)?,
                        qualified: copy(selector)?,
                    }
                }
                "hcs" if !source.is_empty() && selector.is_empty() && kind == DeviceKind::Gpu => {
                    Binding::Hcs {
                        location: copy(source)?,
                    }
                }
                "shards-gpu" if kind == DeviceKind::Gpu && selector.is_empty() => {
                    let physical = source
                        .parse::<shards_api::PhysicalGpu>()
                        .map_err(|_| invalid("invalid Shards physical GPU selector"))?;
                    let native_key = match physical {
                        shards_api::PhysicalGpu::Pci {
                            domain,
                            bus,
                            device,
                            function,
                        } => Some(
                            (u64::from(domain) << 16)
                                | (u64::from(bus) << 8)
                                | (u64::from(device) << 3)
                                | u64::from(function),
                        ),
                        shards_api::PhysicalGpu::MetalRegistry(id) => Some(id),
                        // A LUID is current-boot identity, not the stable PCI DeviceId. L2 must
                        // prove the native PNP/interface/KMT/UUID join before offering this route.
                        shards_api::PhysicalGpu::Luid(_) => None,
                    };
                    if native_key.is_some_and(|native_key| native_key != key) {
                        return Err(invalid(
                            "Shards selector differs from its physical catalog identity",
                        ));
                    }
                    Binding::ShardsGpu { physical }
                }
                "vfio"
                    if !source.is_empty() && matches!(kind, DeviceKind::Gpu | DeviceKind::Vfio) =>
                {
                    let mut functions = source.split(',');
                    let sbdf = copy(
                        functions
                            .next()
                            .ok_or_else(|| invalid("missing VFIO primary function"))?,
                    )?;
                    let mut companions = Vec::new();
                    for function in functions {
                        if function.is_empty() || companions.len() >= hex_api::MAX_DEVICES {
                            return Err(invalid("invalid or excessive VFIO companion functions"));
                        }
                        companions
                            .try_reserve(1)
                            .map_err(|_| allocation(source.len()))?;
                        companions.push(copy(function)?);
                    }
                    Binding::Vfio {
                        sbdf,
                        companions,
                        group: selector
                            .parse::<u32>()
                            .map_err(|_| invalid("invalid VFIO isolation group"))?,
                    }
                }
                _ => return Err(invalid("unsupported or malformed native binding")),
            };
            let info = DeviceInfo {
                id,
                model: copy(model)?,
                size,
                holder: None,
            };
            info.check()?;
            let mut bindings = Vec::new();
            bindings.try_reserve_exact(1).map_err(|_| allocation(1))?;
            bindings.push(binding);
            rows.try_reserve(1).map_err(|_| allocation(bytes.len()))?;
            rows.push((rows.len(), Entry { info, bindings }));
        }
        // Sort once and coalesce explicit aliases; no per-row insertion shifts or second registry.
        // First-report catalog order is restored after normalization. Routes per ID are bounded by
        // distinct enum variants, and duplicate routes are refused even if text happens to agree.
        rows.sort_unstable_by_key(|(order, entry)| (entry.info.id, *order));
        let mut normalized: Vec<(usize, Entry)> = Vec::new();
        normalized
            .try_reserve_exact(rows.len())
            .map_err(|_| allocation(rows.len()))?;
        for (order, mut entry) in rows {
            if let Some((_, previous)) = normalized
                .last_mut()
                .filter(|(_, previous)| previous.info.id == entry.info.id)
            {
                if previous.info.model != entry.info.model || previous.info.size != entry.info.size
                {
                    return Err(invalid("physical aliases disagree on model or size"));
                }
                for binding in &entry.bindings {
                    if previous.binding(binding.route()).is_some() {
                        return Err(invalid("duplicate native route for one physical identity"));
                    }
                }
                previous
                    .bindings
                    .try_reserve_exact(entry.bindings.len())
                    .map_err(|_| allocation(entry.bindings.len()))?;
                previous.bindings.append(&mut entry.bindings);
            } else {
                normalized.push((order, entry));
            }
        }
        normalized.sort_unstable_by_key(|(order, _)| *order);
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(normalized.len())
            .map_err(|_| allocation(normalized.len()))?;
        entries.extend(normalized.into_iter().map(|(_, entry)| entry));
        Ok(Self { entries })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Inspect only this adapter's configured routes. Absence grants no capability; another
    /// backend's valid configuration must not make this adapter reject the whole catalog.
    pub fn entries_for(&self, route: Route) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(move |entry| entry.binding(route).is_some())
    }

    /// Canonical existing turn keys for a selection, including every catalogued alias of an
    /// inseparable IOMMU group. Acquire these BEFORE holder inspection and Creating publication.
    /// This expands locks only; the durable BoxRecord remains the sole holder authority.
    pub fn conflict_ids(&self, devices: &[DeviceId]) -> Result<Vec<DeviceId>, Refusal> {
        if devices.len() > hex_api::MAX_DEVICES {
            return Err(invalid("selection exceeds the device address space"));
        }
        let mut selected = Vec::new();
        selected
            .try_reserve_exact(devices.len())
            .map_err(|_| allocation(devices.len()))?;
        for id in devices {
            id.check()?;
            selected.push(*id);
        }
        selected.sort_unstable();
        if selected.windows(2).any(|pair| pair.first() == pair.last()) {
            return Err(invalid("duplicate selected device identity"));
        }
        let mut indexed = Vec::new();
        indexed
            .try_reserve_exact(self.entries.len())
            .map_err(|_| allocation(self.entries.len()))?;
        indexed.extend(self.entries.iter());
        indexed.sort_unstable_by_key(|entry| entry.info.id);
        let mut groups = Vec::new();
        groups
            .try_reserve_exact(selected.len())
            .map_err(|_| allocation(selected.len()))?;
        for id in &selected {
            let at = indexed
                .binary_search_by_key(id, |entry| entry.info.id)
                .map_err(|_| invalid("selected physical identity has no binding"))?;
            let entry = indexed
                .get(at)
                .ok_or_else(|| invalid("device identity index disagrees"))?;
            if let Some(group) = entry.group() {
                groups.push(group);
            }
        }
        groups.sort_unstable();
        groups.dedup();
        let mut keys = Vec::new();
        keys.try_reserve_exact(indexed.len())
            .map_err(|_| allocation(indexed.len()))?;
        for entry in indexed {
            let group_alias = entry
                .group()
                .is_some_and(|group| groups.binary_search(&group).is_ok());
            if group_alias || selected.binary_search(&entry.info.id).is_ok() {
                keys.push(entry.info.id);
            }
        }
        Ok(keys)
    }

    /// Each file is read once per create; all selected files share the native config byte budget.
    /// A spec's globals are selected once, with its requested devices in request order.
    pub fn cdi<'a>(&'a self, devices: &[DeviceId]) -> Result<CdiPlan<'a>, Refusal> {
        self.cdi_for(devices, Route::Cdi)
    }

    /// Dedicated guest receipts never enter ordinary CDI selection. The boot owner/full set check
    /// lives here as well as in Node's native render-node validation, before any device effects.
    pub fn guest_cdi<'a>(
        &'a self,
        spec: &BoxSpec,
        authority: &crate::guest::gpu::Authority,
    ) -> Result<CdiPlan<'a>, Refusal> {
        authority.check(spec)?;
        self.cdi_for(&spec.devices, Route::GuestGpu)
    }

    fn cdi_for<'a>(&'a self, devices: &[DeviceId], route: Route) -> Result<CdiPlan<'a>, Refusal> {
        if devices.len() > hex_api::MAX_DEVICES {
            return Err(invalid(
                "selection exceeds the scheduler device index space",
            ));
        }
        let mut groups: Vec<(&str, Vec<&str>)> = Vec::new();
        groups
            .try_reserve_exact(devices.len())
            .map_err(|_| allocation(devices.len()))?;
        for id in devices {
            let entry = self
                .entries
                .iter()
                .find(|entry| entry.info.id == *id)
                .ok_or_else(|| invalid("requested physical device has no native binding"))?;
            let (file, qualified) = match entry.binding(route) {
                Some(Binding::Cdi { file, qualified }) if route == Route::Cdi => (file, qualified),
                Some(Binding::GuestGpu { file, qualified }) if route == Route::GuestGpu => {
                    (file, qualified)
                }
                _ => {
                    return Err(Refusal::Platform {
                        what: "device attachment",
                        platform: "Linux OCI",
                        why: "selected binding is not CDI".into(),
                    });
                }
            };
            if let Some((_, names)) = groups.iter_mut().find(|(path, _)| *path == file.as_str()) {
                names
                    .try_reserve(1)
                    .map_err(|_| allocation(devices.len()))?;
                names.push(qualified);
            } else {
                let mut names = Vec::new();
                names.try_reserve_exact(1).map_err(|_| allocation(1))?;
                names.push(qualified.as_str());
                groups.push((file, names));
            }
        }
        let mut specs = Vec::new();
        specs
            .try_reserve_exact(groups.len())
            .map_err(|_| allocation(groups.len()))?;
        let mut left = hex_core::bundle::MAX_CONFIG_BYTES;
        for (path, names) in groups {
            let bytes = read_bounded(Path::new(path), left)?;
            left = left
                .checked_sub(bytes.len())
                .ok_or_else(|| allocation(left))?;
            let parsed = hex_core::cdi::Spec::parse(&bytes, bytes.len()).map_err(|error| {
                Refusal::Invalid {
                    what: "CDI device configuration",
                    why: error.to_string(),
                }
            })?;
            specs.push((parsed, names));
        }
        Ok(CdiPlan { specs })
    }

    /// Creating, Ready and Deleting records all retain the resource. Only physical removal followed
    /// by record removal makes it free. Malformed records cannot be treated as empty capacity.
    pub fn holder(
        &self,
        id: DeviceId,
        records: &Records,
        pools: &[PoolRecord],
    ) -> Result<Option<hex_api::DeviceHolder>, Refusal> {
        let mut holder = None;
        for (box_id, record) in records {
            let record = record.as_ref().map_err(|_| {
                broken("cannot establish device holders from a malformed box record")
            })?;
            if record.spec.id != *box_id || record.spec.check().is_err() {
                return Err(broken(
                    "cannot establish device holders from an inconsistent box record",
                ));
            }
            if record
                .spec
                .devices
                .iter()
                .any(|claim| self.conflicts(*claim, id))
            {
                if holder.is_some() {
                    return Err(broken("a physical device has multiple recorded holders"));
                }
                holder = Some(hex_api::DeviceHolder::Box(box_id.try_clone()?));
            }
        }
        for record in pools {
            record.check()?;
            if matches!(
                record.phase,
                hex_api::BackingPhase::Prepared | hex_api::BackingPhase::Released
            ) || !record
                .devices
                .iter()
                .any(|claim| self.conflicts(*claim, id))
            {
                continue;
            }
            match holder.as_ref() {
                None => holder = Some(hex_api::DeviceHolder::Backing(record.key)),
                Some(hex_api::DeviceHolder::Box(id)) => {
                    let boxed = records
                        .get(id)
                        .and_then(|record| record.as_ref().ok())
                        .ok_or_else(|| record_error(record.key))?;
                    if !matches!(
                        boxed.phase,
                        hex_core::record::Phase::Creating | hex_core::record::Phase::Deleting
                    ) || boxed.init.is_some()
                        || boxed.run.is_some()
                        || boxed.exit.is_some()
                        || !self.handoff(&boxed.spec, record)?
                    {
                        return Err(record_error(record.key));
                    }
                    // The pending Box is only a consuming reference. The original native
                    // observer still owns every full-P conflict until its actual scope Released.
                    holder = Some(hex_api::DeviceHolder::Backing(record.key));
                }
                Some(hex_api::DeviceHolder::Backing(_)) => return Err(record_error(record.key)),
            }
        }
        Ok(holder)
    }

    fn handoff(&self, spec: &BoxSpec, pool: &PoolRecord) -> Result<bool, Refusal> {
        let Some(route) = spec.route.filter(|route| route.backing == Some(pool.key)) else {
            return Ok(false);
        };
        if route.key.backend_class() != NativeRecord::Pool(pool).class()?
            || route.key.execution == hex_api::Execution::ShardsShared
            || pool.allocation.is_none()
            || !matches!(
                pool.phase,
                hex_api::BackingPhase::Live
                    | hex_api::BackingPhase::Draining
                    | hex_api::BackingPhase::Released
            )
            || pool.phase == hex_api::BackingPhase::Released && pool.exit.is_none()
        {
            return Ok(false);
        }
        route.check_backing_parts(
            spec.allocation.map(|allocation| allocation.cost),
            pool.key,
            pool.cost,
            &pool.devices,
            &spec.devices,
        )?;
        Ok(true)
    }

    /// Physical identities may be in one inseparable IOMMU group. The same recorded claims then
    /// retain the entire group; a second backend cannot assign another function from it.
    fn conflicts(&self, first: DeviceId, second: DeviceId) -> bool {
        if first == second {
            return true;
        }
        let group = |id| {
            self.entries
                .iter()
                .find(|entry| entry.info.id == id)
                .and_then(Entry::group)
        };
        group(first).zip(group(second)).is_some_and(|(a, b)| a == b)
    }

    /// Called while the existing per-device turns are held, before a Creating record or OS effects.
    /// It grants no new authority: an unknown identity or any retained holder refuses the create.
    pub fn prepare<'a>(
        &'a self,
        spec: &BoxSpec,
        records: &Records,
        pools: &[PoolRecord],
    ) -> Result<Vec<&'a Entry>, Refusal> {
        spec.check()?;
        if let Some(source) = spec
            .route
            .filter(|route| route.key.execution != hex_api::Execution::ShardsShared)
            .and_then(|route| route.backing)
        {
            let pool = pools
                .iter()
                .find(|pool| pool.key == source)
                .ok_or_else(|| record_error(source))?;
            if !self.handoff(spec, pool)? {
                return Err(record_error(source));
            }
            for (other, record) in records {
                let record = record.as_ref().map_err(|_| record_error(source))?;
                if other != &spec.id
                    && record.spec.route.is_some_and(|route| {
                        route.backing == Some(source)
                            && route.key.execution != hex_api::Execution::ShardsShared
                    })
                {
                    return Err(Refusal::Backing {
                        key: source,
                        cause: hex_api::LifecycleCause::Busy,
                    });
                }
            }
        }
        let mut selected = Vec::new();
        selected
            .try_reserve_exact(spec.devices.len())
            .map_err(|_| allocation(spec.devices.len()))?;
        for id in &spec.devices {
            let entry = self
                .entries
                .iter()
                .find(|entry| entry.info.id == *id)
                .ok_or_else(|| invalid("requested physical device has no native binding"))?;
            if let Some(holder) = self.holder(*id, records, pools)? {
                let handoff = match holder {
                    hex_api::DeviceHolder::Backing(key) => pools
                        .iter()
                        .find(|record| {
                            record.key == key
                                && matches!(
                                    record.phase,
                                    hex_api::BackingPhase::Live | hex_api::BackingPhase::Draining
                                )
                        })
                        .map(|pool| self.handoff(spec, pool))
                        .transpose()?
                        .unwrap_or(false),
                    hex_api::DeviceHolder::Box(id) => {
                        id == spec.id
                            && spec
                                .route
                                .and_then(|route| route.backing)
                                .is_some_and(|key| {
                                    pools.iter().any(|pool| {
                                        pool.key == key
                                            && pool.phase == hex_api::BackingPhase::Released
                                    })
                                })
                            && records
                                .get(&id)
                                .and_then(|record| record.as_ref().ok())
                                .is_some_and(|record| record.spec == *spec)
                    }
                };
                if !handoff {
                    return Err(invalid("requested physical device is already held"));
                }
            }
            selected.push(entry);
        }
        Ok(selected)
    }
}

fn record_error(key: hex_api::BackingKey) -> Refusal {
    Refusal::Backing {
        key,
        cause: hex_api::LifecycleCause::Uncertain,
    }
}

impl CdiPlan<'_> {
    pub fn edits(&self) -> Result<hex_core::cdi::Edits<'_>, Refusal> {
        let mut all = hex_core::cdi::Edits::default();
        for (spec, names) in &self.specs {
            let edits = spec
                .select(names, names.len())
                .map_err(|error| Refusal::Invalid {
                    what: "CDI device selection",
                    why: error.to_string(),
                })?;
            append(&mut all.env, edits.env)?;
            append(&mut all.device_nodes, edits.device_nodes)?;
            append(&mut all.mounts, edits.mounts)?;
            append(&mut all.hooks, edits.hooks)?;
            append(&mut all.net_devices, edits.net_devices)?;
            append(&mut all.additional_gids, edits.additional_gids)?;
            if edits.intel_rdt.is_some() {
                all.intel_rdt = edits.intel_rdt;
            }
        }
        Ok(all)
    }
}

fn append<T>(to: &mut Vec<T>, from: Vec<T>) -> Result<(), Refusal> {
    to.try_reserve_exact(from.len())
        .map_err(|_| allocation(from.len()))?;
    to.extend(from);
    Ok(())
}

fn read_bounded(path: &Path, bound: usize) -> Result<Vec<u8>, Refusal> {
    let mut file =
        File::open(path).map_err(|error| Refusal::os("opening CDI device configuration", error))?;
    let room = bound.checked_add(1).ok_or_else(|| allocation(bound))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(room)
        .map_err(|_| allocation(room))?;
    bytes.resize(room, 0);
    let mut used = 0usize;
    while used < room {
        let tail = bytes.get_mut(used..).ok_or_else(|| allocation(room))?;
        let count = file
            .read(tail)
            .map_err(|error| Refusal::os("reading CDI device configuration", error))?;
        if count == 0 {
            break;
        }
        used = used.checked_add(count).ok_or_else(|| allocation(room))?;
    }
    if used > bound {
        return Err(invalid(
            "selected CDI input exceeds the native config byte budget",
        ));
    }
    bytes.truncate(used);
    Ok(bytes)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod native_route_tests {
    use super::*;

    #[test]
    fn native_container_and_process_keep_the_actual_platform_and_physical_mask() {
        let room = hex_api::CapacityResources {
            cpu_milli: 4_000,
            mem_bytes: 4 << 30,
            disk_bytes: 0,
        };
        for os in [hex_api::ImageOs::Linux, hex_api::ImageOs::Windows] {
            let image = hex_api::ImagePlatform {
                os,
                arch: hex_api::ImageArch::Arm64,
            };
            let mut capacity = hex_api::Capacity {
                id: "actual native fixture".into(),
                total: room,
                free: room,
                devices: vec![DeviceInfo {
                    id: DeviceId {
                        kind: DeviceKind::Gpu,
                        key: 1,
                    },
                    model: "measured fixture".into(),
                    size: 1024,
                    holder: None,
                }],
                cost: None,
                routes: Vec::new(),
                device_routes: Vec::new(),
                backings: Vec::new(),
            };
            native_routes(&mut capacity, hex_api::Class::Oci, image).unwrap();
            assert_eq!(capacity.device_routes, vec![2]);
            assert!(capacity.routes.iter().all(|offer| offer.key.image == image
                && offer.key.execution == hex_api::Execution::Native));
            assert_eq!(
                capacity.routes.first().unwrap().key.class,
                hex_api::Class::Process
            );
            assert_eq!(
                capacity.routes.first().unwrap().key.gpu,
                hex_api::GpuEligibility::None
            );
            assert_eq!(
                capacity.routes.get(1).unwrap().key.class,
                hex_api::Class::Oci
            );
            assert_eq!(capacity.total, room);
            assert!(native_routes(&mut capacity, hex_api::Class::Oci, image).is_err());
        }
    }
}

#[cfg(test)]
mod authority_tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    enum Mode {
        Once,
        Missing,
        Double,
        Refused,
    }
    struct Owner(Mode);
    struct TransactionView;
    impl Transaction for TransactionView {
        fn records(&self) -> Result<Records, Refusal> {
            Ok(Records::new())
        }
        fn pool_records(&self) -> Result<PoolRecords, Refusal> {
            Ok(Vec::new())
        }
        fn read_pool(&self, _: &hex_api::BackingKey) -> Result<Option<PoolRecord>, Refusal> {
            Ok(None)
        }
        fn put_pool(&self, _: &PoolRecord) -> Result<(), Refusal> {
            Err(invalid("fixture has no pool"))
        }
        fn grant_pool(&self, _: &hex_api::BackingGrant) -> Result<PoolRecord, Refusal> {
            Err(invalid("fixture has no pool"))
        }
        fn remove_pool(&self, _: &hex_api::BackingKey) -> Result<(), Refusal> {
            Err(invalid("fixture has no pool"))
        }
        fn read(&self, _: &BoxId) -> Result<Option<BoxRecord>, Refusal> {
            Ok(None)
        }
        fn put(&self, _: &BoxRecord) -> Result<(), Refusal> {
            Ok(())
        }
        fn remove(&self, _: &BoxId) -> Result<(), Refusal> {
            Ok(())
        }
        fn launch_managed(
            &self,
            _: &ManagedLaunch<'_>,
            _: shards_api::LaunchIo,
            _: Duration,
        ) -> Result<shards_api::ManagedReceipt, Refusal> {
            Err(invalid("fixture has no native launcher"))
        }
        fn attach_managed(
            &self,
            _: Option<&shards_api::Identity>,
        ) -> Result<shards_api::ManagedReceipt, Refusal> {
            Err(invalid("fixture has no native launcher"))
        }
        fn finalize_managed(&self, _: Option<&shards_api::Identity>) -> Result<(), Refusal> {
            Err(invalid("fixture has no native launcher"))
        }
        fn managed_usage(&self) -> Result<hex_api::Usage, Refusal> {
            Err(invalid("fixture has no native scope"))
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
            Ok(Records::new())
        }
        fn pool_records(&self) -> Result<PoolRecords, Refusal> {
            Ok(Vec::new())
        }
        fn prepare_pool(
            &self,
            _: [u8; 32],
            _: hex_api::Execution,
            _: hex_api::RuntimeCost,
            _: &[DeviceId],
        ) -> Result<hex_api::BackingOffer, Refusal> {
            Err(invalid("fixture has no pool"))
        }
        fn with_pool_transaction(
            &self,
            key: &hex_api::BackingKey,
            _: &[DeviceId],
            callback: &mut dyn FnMut(&dyn Transaction) -> Result<(), Refusal>,
        ) -> Result<(), Refusal> {
            match self.0 {
                Mode::Once => callback(&TransactionView),
                Mode::Missing => Ok(()),
                Mode::Double => {
                    callback(&TransactionView)?;
                    callback(&TransactionView)
                }
                Mode::Refused => Err(Refusal::Backing {
                    key: *key,
                    cause: hex_api::LifecycleCause::Busy,
                }),
            }
        }
        fn capacity(&self) -> Result<hex_api::Capacity, Refusal> {
            Err(invalid("fixture has no hardware"))
        }
        fn physical_devices(&self) -> Result<Vec<DeviceInfo>, Refusal> {
            Ok(Vec::new())
        }
        fn with_transaction(
            &self,
            id: &BoxId,
            _: &[DeviceId],
            callback: &mut dyn FnMut(&dyn Transaction) -> Result<(), Refusal>,
        ) -> Result<(), Refusal> {
            match self.0 {
                Mode::Once => callback(&TransactionView),
                Mode::Missing => Ok(()),
                Mode::Double => {
                    callback(&TransactionView)?;
                    callback(&TransactionView)
                }
                Mode::Refused => Err(Refusal::NoSuchBox(id.try_clone()?)),
            }
        }
    }

    /// AC-T7.4.1: path authority is the canonical incarnation, never the reusable public box id.
    #[test]
    fn native_scope_path_keeps_all_bits_and_refuses_legacy_absence() {
        let mut record = BoxRecord::new(
            BoxSpec {
                id: BoxId::new("reused").unwrap(),
                class: hex_api::ClassSpec::Process,
                bounds: hex_api::Bounds::default(),
                owner: None,
                devices: Vec::new(),
                allocation: None,
                route: None,
            },
            hex_core::record::Phase::Creating,
            Vec::new(),
        );
        assert!(managed_scope_id(&record).is_err());
        record.native_scope = Some(hex_core::record::NativeScope::from_bytes([0xab; 16]));
        let first = managed_scope_id(&record).unwrap();
        assert_eq!(first.as_str(), "abababababababababababababababab");
        record.native_scope = Some(hex_core::record::NativeScope::from_bytes([0xac; 16]));
        assert_ne!(managed_scope_id(&record).unwrap(), first);
        record.native_scope = Some(hex_core::record::NativeScope::from_bytes([0xab; 16]));
        record.spec.id = BoxId::new("another-public-name").unwrap();
        assert_eq!(managed_scope_id(&record).unwrap(), first);
    }

    /// AC-T7.4.1: no callback protocol failure may duplicate a physical operation or mask refusal.
    #[test]
    fn shared_transaction_keeps_one_owner_and_exactly_one_effect() {
        let id = BoxId::new("callback").unwrap();
        for (mode, calls, succeeds) in [
            (Mode::Once, 1, true),
            (Mode::Missing, 0, false),
            (Mode::Double, 1, false),
            (Mode::Refused, 0, false),
        ] {
            let owner: Arc<dyn Books> = Arc::new(Owner(mode));
            let effects = AtomicUsize::new(0);
            let result = with_transaction(&Arc::downgrade(&owner), &id, &[], |_| {
                effects.fetch_add(1, Ordering::SeqCst);
                Ok(7)
            });
            assert_eq!(effects.load(Ordering::SeqCst), calls);
            assert_eq!(result.is_ok(), succeeds);
            let key = hex_api::BackingKey {
                pool: [1; 32],
                generation: [2; 16],
            };
            let effects = AtomicUsize::new(0);
            let result = with_pool_transaction(&Arc::downgrade(&owner), &key, &[], |_| {
                effects.fetch_add(1, Ordering::SeqCst);
                Ok(7)
            });
            assert_eq!(effects.load(Ordering::SeqCst), calls);
            assert_eq!(result.is_ok(), succeeds);
        }
        let owner: Arc<dyn Books> = Arc::new(Owner(Mode::Once));
        let weak = Arc::downgrade(&owner);
        drop(owner);
        assert!(
            with_transaction(&weak, &id, &[], |_| -> Result<(), Refusal> {
                panic!("dead owner invoked operation")
            })
            .is_err()
        );
        assert!(
            with_pool_transaction(
                &weak,
                &hex_api::BackingKey {
                    pool: [1; 32],
                    generation: [2; 16]
                },
                &[],
                |_| -> Result<(), Refusal> { panic!("dead owner invoked pool operation") },
            )
            .is_err()
        );
        let owner: Arc<dyn Books> = Arc::new(Owner(Mode::Once));
        let refusal = Refusal::Exists(id.clone());
        assert_eq!(
            with_transaction(
                &Arc::downgrade(&owner),
                &id,
                &[],
                |_| -> Result<(), Refusal> { Err(refusal.clone()) }
            ),
            Err(refusal)
        );
    }

    /// AC-T7.4.1: all same-group identities use the same sorted existing turn keys.
    #[test]
    fn isolation_group_aliases_expand_to_identical_turn_keys() {
        let catalog = Catalog::parse(b"hex-devices-1\ngpu\t256\ta\t4096\tvfio\t0000:01:00.0\t7\ngpu\t512\tb\t4096\tvfio\t0000:02:00.0\t7\ngpu\t768\tc\t4096\tvfio\t0000:03:00.0\t8\n").unwrap();
        let id = |key| DeviceId {
            kind: DeviceKind::Gpu,
            key,
        };
        assert_eq!(
            catalog.conflict_ids(&[id(256)]).unwrap(),
            vec![id(256), id(512)]
        );
        assert_eq!(
            catalog.conflict_ids(&[id(512)]).unwrap(),
            vec![id(256), id(512)]
        );
        assert_eq!(catalog.conflict_ids(&[id(768)]).unwrap(), vec![id(768)]);
        assert!(catalog.conflict_ids(&[id(256), id(256)]).is_err());
    }

    /// AC-T7.4.1: adapter selectors preserve native identity widths and never accept ordinals.
    #[test]
    fn shards_selector_uses_checked_native_identity_grammar() {
        for (key, source) in [
            (u64::MAX, "metal:ffffffffffffffff"),
            (u64::MAX, "luid:ffffffffffffffff"),
            (0xffff_ffff_ffff, "pci:ffffffff:ff:1f.7"),
        ] {
            let text =
                format!("hex-devices-1\ngpu\t{key}\tfixture\t4096\tshards-gpu\t{source}\t\n");
            let catalog = Catalog::parse(text.as_bytes()).unwrap();
            assert!(matches!(
                catalog.entries().first().unwrap().binding(Route::ShardsGpu),
                Some(Binding::ShardsGpu { .. })
            ));
        }
        for source in ["0", "NVIDIA", "metal:0", "pci:0000:01:20.0", "metal:2"] {
            let text = format!("hex-devices-1\ngpu\t1\tfixture\t4096\tshards-gpu\t{source}\t\n");
            assert!(Catalog::parse(text.as_bytes()).is_err());
        }
    }
}

#[cfg(test)]
mod report_tests {
    use super::*;
    use hex_api::{
        BackingKey, BackingPhase, CapacityResources, Execution, LifecycleCause, RuntimeCost,
    };
    use hex_core::record::{PoolRecord, RECORD_VERSION};

    #[test]
    fn managed_report_scope_and_writer_parity_refuse_before_native_effects() {
        let guest = CapacityResources {
            cpu_milli: 1_000,
            mem_bytes: 64 << 20,
            disk_bytes: 0,
        };
        let cost = RuntimeCost {
            id: [1; 32],
            floor: guest,
            ceiling: guest,
            quantum: CapacityResources {
                cpu_milli: 1_000,
                mem_bytes: 1,
                disk_bytes: 1,
            },
            guest_overhead: CapacityResources::default(),
            overhead: CapacityResources::default(),
        };
        let pool = PoolRecord {
            version: RECORD_VERSION,
            key: BackingKey {
                pool: [2; 32],
                generation: [3; 16],
            },
            execution: Execution::ShardsDedicated,
            cost,
            allocation: Some(cost.allocate(CapacityResources::default()).unwrap()),
            devices: vec![DeviceId {
                kind: hex_api::DeviceKind::Gpu,
                key: 7,
            }],
            phase: BackingPhase::Granted,
            init: None,
            exit: None,
        };
        let record = NativeRecord::Pool(&pool);
        check_report_scope(record, Some([3; 16]), true, true).unwrap();
        for (scope, writer, required) in [
            (Some([4; 16]), true, true),
            (Some([3; 16]), false, true),
            (None, true, false),
            (None, false, true),
        ] {
            assert!(
                matches!(check_report_scope(record, scope, writer, required), Err(Refusal::Backing { key, cause:LifecycleCause::Uncertain }) if key == pool.key)
            );
        }
        // CPU/Firecracker's separate factual RPC path supplies no Shards writer or request.
        check_report_scope(record, None, false, false).unwrap();
    }
}
