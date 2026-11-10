//! What a simulated cluster is made of: nodes with a capacity, a platform and devices, workloads with
//! a need and an ask of their node.

/// Virtual nanoseconds since the run began.
pub type Time = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkloadId(pub u64);

/// What a node reserves for a workload until it is deleted: CPU, memory, disk and GPUs (kiln's needs,
/// `crates/kiln-ci/src/sched/needs.rs`: `cpus=`, `mem=`, `disk=`, `gpus=`). Ordered field by field, so
/// a need can key an equivalence class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Resources {
    pub cpu_milli: u64,
    pub mem_bytes: u64,
    pub gpus: u32,
    pub disk_bytes: u64,
}

impl Resources {
    /// Whether `need` fits in what this is.
    pub fn holds(&self, need: &Resources) -> bool {
        need.cpu_milli <= self.cpu_milli
            && need.mem_bytes <= self.mem_bytes
            && need.gpus <= self.gpus
            && need.disk_bytes <= self.disk_bytes
    }

    pub fn checked_sub(&self, other: &Resources) -> Option<Resources> {
        Some(Resources {
            cpu_milli: self.cpu_milli.checked_sub(other.cpu_milli)?,
            mem_bytes: self.mem_bytes.checked_sub(other.mem_bytes)?,
            gpus: self.gpus.checked_sub(other.gpus)?,
            disk_bytes: self.disk_bytes.checked_sub(other.disk_bytes)?,
        })
    }

    pub fn checked_add(&self, other: &Resources) -> Option<Resources> {
        Some(Resources {
            cpu_milli: self.cpu_milli.checked_add(other.cpu_milli)?,
            mem_bytes: self.mem_bytes.checked_add(other.mem_bytes)?,
            gpus: self.gpus.checked_add(other.gpus)?,
            disk_bytes: self.disk_bytes.checked_add(other.disk_bytes)?,
        })
    }
}

/// An operating system, as Rust names it (kiln `crates/kiln-ci/src/sched/needs.rs` `os_name`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Os {
    #[default]
    Linux,
    Macos,
    Windows,
    Freebsd,
}

impl Os {
    /// The names people and tools use for it (kiln's `os_name`).
    pub fn named(s: &str) -> Option<Os> {
        Some(match s.to_ascii_lowercase().as_str() {
            "linux" | "gnu" => Os::Linux,
            "macos" | "mac" | "darwin" | "osx" | "apple" => Os::Macos,
            "windows" | "win" | "win32" | "win64" => Os::Windows,
            "freebsd" => Os::Freebsd,
            _ => return None,
        })
    }

    /// The classes a node of this OS runs with `virt` (H10, `huawei_trace.py`): Linux OCI containers,
    /// and Firecracker and Shards microVMs where KVM is usable (both VMMs run on it); Windows OCI
    /// containers and Shards microVMs (on the Windows Hypervisor Platform, which kiln's facts don't
    /// probe); macOS Shards microVMs where Hypervisor.framework is supported; FreeBSD none Hex places.
    pub fn classes(self, virt: Virt) -> Classes {
        match self {
            Os::Linux if virt.kvm => Classes::ALL,
            Os::Linux => Classes::NONE.with(Class::Oci),
            Os::Windows => Classes::NONE.with(Class::Oci).with(Class::Shards),
            Os::Macos if virt.hvf => Classes::NONE.with(Class::Shards),
            Os::Macos | Os::Freebsd => Classes::NONE,
        }
    }
}

/// A processor architecture, as Rust names it (kiln's `arch_name`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Arch {
    #[default]
    X86_64,
    Aarch64,
    X86,
    Arm,
    Riscv64,
}

impl Arch {
    /// The names people and tools use for it (kiln's `arch_name`).
    pub fn named(s: &str) -> Option<Arch> {
        Some(match s.to_ascii_lowercase().as_str() {
            "x86_64" | "amd64" | "x64" | "x86-64" | "intel" => Arch::X86_64,
            "aarch64" | "arm64" | "armv8" | "apple-silicon" => Arch::Aarch64,
            "x86" | "i386" | "i686" => Arch::X86,
            "arm" | "armv7" | "armhf" => Arch::Arm,
            "riscv64" => Arch::Riscv64,
            _ => return None,
        })
    }
}

/// A node's OS and architecture. The default is EC2 m5d.metal's, G2's main node model: Linux on
/// x86_64.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Platform {
    pub os: Os,
    pub arch: Arch,
}

/// The virtualization a node offers (kiln's facts' `virt`): `/dev/kvm` usable, nested virtualization
/// on (its guests may use KVM themselves), and macOS's Hypervisor.framework supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Virt {
    pub kvm: bool,
    pub nested: bool,
    pub hvf: bool,
}

/// A device a box holds alone, from its start until it is deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeviceKind {
    Gpu,
    /// A PCI function bound to `vfio-pci`, given whole.
    Vfio,
    /// A block device given whole.
    Block,
    /// A network device given whole (an SR-IOV function, a NIC).
    Net,
}

/// A GPU's load from what isn't Hex's (hex-gpu's `sample()`): its utilisation in per mille, and its
/// memory in use. Injected in the simulator; a node's agent reads it from hex-gpu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct GpuLoad {
    pub busy_per_mille: u16,
    pub memory_used: u64,
}

/// A device model's name, interned in its cluster ([`Cluster::model`]): GPU models are compared by
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModelId(pub u32);

impl ModelId {
    /// A device whose model its source doesn't say (a trace that counts GPUs): no ask names it.
    pub const UNNAMED: ModelId = ModelId(u32::MAX);
}

/// One of a node's devices: its kind, its model, its size (a GPU's memory, a block device's bytes) and,
/// for a GPU, its load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Device {
    pub kind: DeviceKind,
    pub model: ModelId,
    pub size: u64,
    pub load: GpuLoad,
}

impl Device {
    /// A GPU of `model` with `memory` bytes, idle.
    pub fn gpu(model: ModelId, memory: u64) -> Self {
        Self {
            kind: DeviceKind::Gpu,
            model,
            size: memory,
            load: GpuLoad::default(),
        }
    }

    /// A GPU's memory free of what isn't Hex's.
    pub fn memory_free(&self) -> u64 {
        self.size.saturating_sub(self.load.memory_used)
    }
}

/// The most devices a node has: one for each PCI function address of a segment (bus, device and
/// function: 8, 5 and 3 bits), so a device's place on its node is a u16.
pub const DEVICES: usize = 1 << 16;

/// What a workload asks of its node beyond room (kiln's needs, adapted): its image's OS and
/// architecture, and the devices it is given. Its GPUs' count is its need's
/// ([`Resources::gpus`]); `gpu_model` and `gpu_memory` constrain each of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Ask {
    pub os: Option<Os>,
    pub arch: Option<Arch>,
    pub gpu_model: Option<ModelId>,
    /// Each GPU's memory free, at least.
    pub gpu_memory: u64,
    pub vfio: u16,
    pub block: u16,
    /// Each block device's size, at least.
    pub block_size: u64,
    pub net: u16,
    /// `/dev/kvm` in the box: shared, not held.
    pub kvm: bool,
}

impl Ask {
    /// Whether it asks a device of any kind, or KVM.
    pub fn devices(&self) -> bool {
        self.vfio > 0 || self.block > 0 || self.net > 0 || self.kvm
    }
}

/// The devices a workload holds on its node, by their places in the node's devices.
pub type Held = Vec<u16>;

/// The devices `ask` and `gpus` take of `devices`, where `free(i)` says whether device `i` is free:
/// for each kind, that many free devices of the kind that match the ask. GPUs go least loaded first
/// (utilisation, then memory in use), so the box gets the idlest; block devices smallest that hold
/// the size first, so a larger one is kept for a larger ask; the rest by place. `None` where too few
/// are free. Each device is read once a kind, so a node's devices bound it.
pub fn bind(
    devices: &[Device],
    free: impl Fn(usize) -> bool,
    gpus: u32,
    ask: &Ask,
) -> Option<Held> {
    let mut held = Held::new();
    let wants = [
        (DeviceKind::Gpu, u64::from(gpus)),
        (DeviceKind::Vfio, u64::from(ask.vfio)),
        (DeviceKind::Block, u64::from(ask.block)),
        (DeviceKind::Net, u64::from(ask.net)),
    ];
    for (kind, n) in wants {
        if n == 0 {
            continue;
        }
        let mut fit: Vec<(u64, u64, usize)> = devices
            .iter()
            .enumerate()
            .filter(|&(i, d)| d.kind == kind && free(i) && matches(d, ask))
            .map(|(i, d)| match kind {
                DeviceKind::Gpu => (u64::from(d.load.busy_per_mille), d.load.memory_used, i),
                DeviceKind::Block => (d.size, 0, i),
                DeviceKind::Vfio | DeviceKind::Net => (0, 0, i),
            })
            .collect();
        fit.sort_unstable();
        let n = usize::try_from(n).ok()?;
        if fit.len() < n {
            return None;
        }
        for &(_, _, i) in fit.iter().take(n) {
            held.push(u16::try_from(i).ok()?);
        }
    }
    Some(held)
}

/// Whether device `d` is one `ask` can be given, by its model and size.
pub fn matches(d: &Device, ask: &Ask) -> bool {
    match d.kind {
        DeviceKind::Gpu => {
            ask.gpu_model.is_none_or(|m| m == d.model) && d.memory_free() >= ask.gpu_memory
        }
        DeviceKind::Block => d.size >= ask.block_size,
        DeviceKind::Vfio | DeviceKind::Net => true,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSpec {
    pub id: NodeId,
    pub capacity: Resources,
    /// CPU in use by what runs there outside Hex's placements (co-tenants, the host itself), which
    /// the 90% CPU rule counts beside what Hex placed.
    pub background_milli: u64,
    /// The classes it can run.
    pub classes: Classes,
    pub platform: Platform,
    pub virt: Virt,
    /// What boxes may be given whole; its GPUs are [`Resources::gpus`] of its capacity.
    pub devices: Vec<Device>,
}

impl NodeSpec {
    /// A Linux x86_64 node with KVM and `capacity`, running every class, its GPUs unnamed and idle,
    /// with nothing outside Hex.
    pub fn new(id: NodeId, capacity: Resources) -> Self {
        Self {
            id,
            capacity,
            background_milli: 0,
            classes: Classes::ALL,
            platform: Platform::default(),
            virt: Virt {
                kvm: true,
                ..Virt::default()
            },
            devices: unnamed_gpus(capacity.gpus),
        }
    }

    /// Whether its devices agree with its capacity: as many GPUs listed as it has, and no more
    /// devices than [`DEVICES`].
    pub fn devices_agree(&self) -> bool {
        let gpus = self
            .devices
            .iter()
            .filter(|d| d.kind == DeviceKind::Gpu)
            .count();
        self.devices.len() <= DEVICES && u32::try_from(gpus) == Ok(self.capacity.gpus)
    }
}

/// `n` GPUs whose model and memory a source doesn't say, idle.
pub fn unnamed_gpus(n: u32) -> Vec<Device> {
    (0..n).map(|_| Device::gpu(ModelId::UNNAMED, 0)).collect()
}

/// What runs a workload. Hex places three: OCI containers, Firecracker microVMs and Shards
/// microVMs (Ada, 2026-11-06), each only on a node that can run it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Class {
    #[default]
    Oci,
    Firecracker,
    Shards,
}

impl Class {
    /// Whether it can be given a device of `kind` on a node of `os`
    /// (docs/research/scheduler/agentic-workloads.md §6 and finding 8: device access is a property of
    /// the node and the class). An OCI container on Linux can be given each kind: its runtime passes
    /// the device in by CDI, as Kubernetes' device plugins do ([21][55] there), a VFIO group, a block
    /// device or a network device moved into its namespace. On Windows, a process-isolated container
    /// is given a GPU by its device interface class (the class Hex's Windows `oci` grants), and no
    /// device whole. A Firecracker microVM can be given none: its mainline emulates six devices and
    /// passes none through ([25]-[28] there). Nor, yet, can a Shards microVM placed by Hex: Shards'
    /// VMM gives a guest a shared Venus GPU, or on Linux a PCI function by VFIO, but shards-api,
    /// Hex's one seam to it, names no device ([58]-[60] there).
    pub fn exposes(self, os: Os, kind: DeviceKind) -> bool {
        match (self, os) {
            (Class::Oci, Os::Linux) => true,
            (Class::Oci, Os::Windows) => kind == DeviceKind::Gpu,
            _ => false,
        }
    }

    /// Whether it can be given `/dev/kvm` on a node of `os`: an OCI container on Linux, by the device
    /// node. A microVM would need its VMM to pass virtualization through to its guest, which neither
    /// Firecracker's mainline nor shards-api does.
    pub fn gives_kvm(self, os: Os) -> bool {
        matches!((self, os), (Class::Oci, Os::Linux))
    }

    /// Its place among the three, as reports and per-class arrays give them: OCI containers,
    /// Firecracker microVMs, Shards microVMs.
    pub fn index(self) -> usize {
        match self {
            Class::Oci => 0,
            Class::Firecracker => 1,
            Class::Shards => 2,
        }
    }
}

/// The classes a node can run, a bit each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Classes(u8);

impl Classes {
    pub const ALL: Self = Self(0b111);
    pub const NONE: Self = Self(0);

    pub fn with(self, class: Class) -> Self {
        Self(self.0 | Self::bit(class))
    }

    pub fn has(self, class: Class) -> bool {
        self.0 & Self::bit(class) != 0
    }

    /// Whether the two share a class.
    pub fn meets(self, other: Classes) -> bool {
        self.0 & other.0 != 0
    }

    /// How many classes it holds.
    pub fn count(self) -> u32 {
        self.0.count_ones()
    }

    fn bit(class: Class) -> u8 {
        match class {
            Class::Oci => 1,
            Class::Firecracker => 2,
            Class::Shards => 4,
        }
    }
}

/// What each class costs its node beside what a workload asks, in bytes of memory: its runtime's own
/// (a container's shim, a microVM's VMM and the kernel's share of it). A workload's need includes its
/// class's.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Overheads([Overhead; 3]);

/// One class's overhead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overhead {
    /// The same for every workload of the class.
    Flat(u64),
    /// By the workload's own memory, where it was measured by guest size: (its memory, the overhead),
    /// ascending. A trace's rows bound it (`replay::Limits`).
    BySize(Vec<(u64, u64)>),
}

impl Default for Overhead {
    fn default() -> Self {
        Self::Flat(0)
    }
}

impl Overheads {
    /// A workload's overhead, `class`'s for its own memory `mem`; none where the class's overhead goes
    /// by size and names no row for `mem`.
    pub fn of(&self, class: Class, mem: u64) -> Option<u64> {
        match self.0.get(class.index())? {
            Overhead::Flat(bytes) => Some(*bytes),
            Overhead::BySize(sizes) => sizes
                .binary_search_by_key(&mem, |&(m, _)| m)
                .ok()
                .and_then(|i| sizes.get(i))
                .map(|&(_, bytes)| bytes),
        }
    }

    /// A workload's own memory, from its need as `class`: what [`Overheads::of`] added taken off.
    pub fn own(&self, class: Class, need: u64) -> Option<u64> {
        match self.0.get(class.index())? {
            Overhead::Flat(bytes) => need.checked_sub(*bytes),
            Overhead::BySize(sizes) => sizes
                .iter()
                .find(|&&(m, bytes)| m.checked_add(bytes) == Some(need))
                .map(|&(m, _)| m),
        }
    }

    pub fn set(&mut self, class: Class, bytes: u64) {
        if let Some(slot) = self.0.get_mut(class.index()) {
            *slot = Overhead::Flat(bytes);
        }
    }

    /// Adds `class`'s overhead for workloads whose own memory is `mem`, making its overhead go by size;
    /// refused, with why, where `mem` has one already.
    pub fn set_size(&mut self, class: Class, mem: u64, bytes: u64) -> Result<(), &'static str> {
        let slot = self
            .0
            .get_mut(class.index())
            .ok_or("a class with no slot")?;
        if let Overhead::Flat(_) = slot {
            *slot = Overhead::BySize(Vec::new());
        }
        let Overhead::BySize(sizes) = slot else {
            return Err("a class's overhead not by size");
        };
        match sizes.binary_search_by_key(&mem, |&(m, _)| m) {
            Ok(_) => Err("a class's overhead given twice for one size"),
            Err(at) => {
                sizes.insert(at, (mem, bytes));
                Ok(())
            }
        }
    }
}

impl Default for Classes {
    /// A node that says nothing runs every class, so a trace written before classes reads as it did.
    fn default() -> Self {
        Self::ALL
    }
}

/// What a workload is, for the filters that keep work off hosts it would harm or be harmed by
/// (`scheduler-architecture.md` S3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Kind {
    #[default]
    Plain,
    /// Changes its host while it runs (a build): never beside a benchmark.
    Build,
    /// Interference-sensitive (a benchmark): never beside a build.
    Benchmark,
    /// An agent session: never on a node above 90% CPU (`AC-T7.6.3`).
    Session,
}

/// A workload's equivalence class: what it needs, what it is, what runs it and what it asks of its
/// node. Workloads of one class are placed alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Demand {
    pub need: Resources,
    pub kind: Kind,
    pub class: Class,
    pub ask: Ask,
}

/// Who a workload's capacity is charged to: the tenant or app an OCI container, a Firecracker microVM
/// or a Shards microVM runs for (`AC-T7.5.2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct OwnerId(pub u32);

/// Workloads admitted whole or not at all: workloads that only work together (`AC-T7.5.1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GangId(pub u64);

/// Workloads that share one disruption budget (`AC-T7.5.3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GroupId(pub u32);

/// How a workload competes for capacity (`docs/design/scheduler-fairness.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tenancy {
    pub owner: OwnerId,
    /// Its priority band: a higher band goes first, and only a strictly higher one preempts it.
    pub band: u8,
    pub gang: Option<GangId>,
    pub group: Option<GroupId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkloadSpec {
    pub id: WorkloadId,
    pub need: Resources,
    pub kind: Kind,
    /// When its client first submits it.
    pub arrives: Time,
    /// How long it runs once started; `None` runs until the run ends.
    pub runs_for: Option<Time>,
    pub tenancy: Tenancy,
    pub class: Class,
    pub ask: Ask,
}

impl WorkloadSpec {
    pub fn demand(&self) -> Demand {
        Demand {
            need: self.need,
            kind: self.kind,
            class: self.class,
            ask: self.ask,
        }
    }
}

/// An owner's weight in weighted DRF: its fair share is its weight over the sum of the weights of
/// the owners with work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Owner {
    pub weight: u32,
}

impl Default for Owner {
    fn default() -> Self {
        Self { weight: 1 }
    }
}

/// A disruption budget: at most `budget` of the group's members preempted and not yet running
/// again at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group {
    pub budget: u32,
}

/// A run's input: its nodes (ids dense from 0), its workloads (ids dense from 0, by arrival), its
/// owners and its disruption groups (ids by position).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cluster {
    pub nodes: Vec<NodeSpec>,
    pub workloads: Vec<WorkloadSpec>,
    pub owners: Vec<Owner>,
    pub groups: Vec<Group>,
    /// Already counted in each workload's need; kept to change a workload's class.
    pub overheads: Overheads,
    /// Device models' names, by [`ModelId`]: lowercase, words joined by `-`.
    pub models: Vec<String>,
}

impl Cluster {
    /// One owner of weight 1, no groups and no class overheads: every workload's default tenancy.
    pub fn single_owner(nodes: Vec<NodeSpec>, workloads: Vec<WorkloadSpec>) -> Self {
        Self {
            nodes,
            workloads,
            owners: vec![Owner::default()],
            groups: Vec::new(),
            overheads: Overheads::default(),
            models: Vec::new(),
        }
    }

    /// The id of the device model `name`, as its cluster spells it ([`model_name`]), added if new.
    pub fn model(&mut self, name: &str) -> Result<ModelId, crate::SimError> {
        let name = model_name(name);
        if let Some(at) = self.models.iter().position(|m| *m == name) {
            return u32::try_from(at)
                .map(ModelId)
                .map_err(|_| crate::SimError::Config("a model id past u32"));
        }
        let id = u32::try_from(self.models.len())
            .ok()
            .filter(|&at| at != ModelId::UNNAMED.0)
            .map(ModelId)
            .ok_or(crate::SimError::Full {
                what: "a cluster's device models",
                bound: usize::try_from(ModelId::UNNAMED.0).unwrap_or(usize::MAX),
            })?;
        self.models.push(name);
        Ok(id)
    }
}

/// A device model's name as a needs word can spell it (words are split at commas and spaces):
/// lowercase, its words joined by `-`. `NVIDIA GeForce RTX 5060 Ti` is `nvidia-geforce-rtx-5060-ti`.
pub fn model_name(name: &str) -> String {
    name.split(|c: char| c.is_whitespace() || c == '_' || c == '-')
        .filter(|w| !w.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join("-")
}
