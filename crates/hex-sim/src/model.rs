//! What a simulated cluster is made of: nodes with a capacity, workloads with a need.

/// Virtual nanoseconds since the run began.
pub type Time = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkloadId(pub u64);

/// Ordered field by field, so a need can key an equivalence class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Resources {
    pub cpu_milli: u64,
    pub mem_bytes: u64,
    pub gpus: u32,
}

impl Resources {
    /// Whether `need` fits in what this is.
    pub fn holds(&self, need: &Resources) -> bool {
        need.cpu_milli <= self.cpu_milli
            && need.mem_bytes <= self.mem_bytes
            && need.gpus <= self.gpus
    }

    pub fn checked_sub(&self, other: &Resources) -> Option<Resources> {
        Some(Resources {
            cpu_milli: self.cpu_milli.checked_sub(other.cpu_milli)?,
            mem_bytes: self.mem_bytes.checked_sub(other.mem_bytes)?,
            gpus: self.gpus.checked_sub(other.gpus)?,
        })
    }

    pub fn checked_add(&self, other: &Resources) -> Option<Resources> {
        Some(Resources {
            cpu_milli: self.cpu_milli.checked_add(other.cpu_milli)?,
            mem_bytes: self.mem_bytes.checked_add(other.mem_bytes)?,
            gpus: self.gpus.checked_add(other.gpus)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeSpec {
    pub id: NodeId,
    pub capacity: Resources,
    /// CPU in use by what runs there outside Hex's placements (co-tenants, the host itself), which
    /// the 90% CPU rule counts beside what Hex placed.
    pub background_milli: u64,
    /// The classes it can run.
    pub classes: Classes,
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
    /// Whether it can be given a GPU (docs/research/scheduler/agentic-workloads.md, finding 8: GPU
    /// access is a property of the node and the class). An OCI container can: its runtime passes the
    /// device in, as Kubernetes' device plugins and CDI do ([21][55] there). A Firecracker microVM
    /// can't: its mainline emulates six devices and passes none through ([25]-[28] there). Nor, yet,
    /// can a Shards microVM: Shards' guest spec names disks, shares and vsock ports, and no device
    /// (shards-api draft, hyper-light/shards a0cced87, `crates/api/src/lib.rs:41-62`).
    pub fn takes_gpus(self) -> bool {
        matches!(self, Class::Oci)
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
/// (a container's shim, a microVM's VMM). A workload's need includes its class's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Overheads([u64; 3]);

impl Overheads {
    pub fn of(self, class: Class) -> u64 {
        self.0.get(Self::at(class)).copied().unwrap_or(0)
    }

    pub fn set(&mut self, class: Class, bytes: u64) {
        if let Some(slot) = self.0.get_mut(Self::at(class)) {
            *slot = bytes;
        }
    }

    fn at(class: Class) -> usize {
        match class {
            Class::Oci => 0,
            Class::Firecracker => 1,
            Class::Shards => 2,
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

/// A workload's equivalence class: what it needs, what it is and what runs it. Workloads of one
/// class are placed alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Demand {
    pub need: Resources,
    pub kind: Kind,
    pub class: Class,
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
}

impl WorkloadSpec {
    pub fn demand(&self) -> Demand {
        Demand {
            need: self.need,
            kind: self.kind,
            class: self.class,
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
        }
    }
}
