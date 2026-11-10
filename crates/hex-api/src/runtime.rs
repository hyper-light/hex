use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{Bounds, BoxId, BoxSpec, Class, DeviceInfo, Program, Stdio};

/// A runtime: boxes of one or more classes on one node. Calls are synchronous and short (a create
/// takes milliseconds); waiting for a program is [`Proc`]'s, with a deadline. Every refusal is
/// typed and says what and why (CLAUDE.md §3).
pub trait Runtime: Send + Sync {
    /// The classes it runs.
    fn classes(&self) -> Vec<Class>;

    /// AC-T7.4.1: the resource pool backing this class. `None` explicitly selects the caller's
    /// same-host physical budget; a guest must report its own pool rather than host capacity.
    fn capacity(&self, _class: Class) -> Result<Option<Capacity>, Refusal> {
        Ok(None)
    }

    /// Makes a box with its bounds in force. A `process` box is empty when this returns; an `oci`
    /// box's init is made and waits for [`Runtime::start`], [`BoxState::Created`].
    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal>;

    /// Lets a created `oci` box's init run its program: the runtime-spec's `start`, which `create`
    /// leaves to a later call so a box can be made before it's needed. A `process` box has no init to
    /// start, and an `oci` box that has started already can't start again: each is refused.
    fn start(&self, id: &BoxId) -> Result<(), Refusal>;

    /// Runs `program` in box `id` with `stdio`: a host process in a `process` box, a process in the
    /// container's namespaces in an `oci` box.
    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal>;

    /// Makes a `process` box from `spec` and runs `program` in it with `stdio`, as one call: the box a
    /// program runs in for its life, with no process beside it (decision 7 of
    /// docs/design/runtime-architecture.md). The runtime removes the box once the program has ended,
    /// ending anything it left running there, whether or not the caller still holds the [`Proc`]; the
    /// `Proc` reports the end only once the box is gone. Until then the box is any box: listed,
    /// signalled, inspected, deleted. An `oci` spec is refused ([`BoxSpec::check_run`]): its box has an
    /// init to create and start. A class whose node has nothing that removes a box when its program ends
    /// refuses the run as [`Refusal::Platform`], naming its platform.
    fn run(&self, spec: BoxSpec, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal>;

    /// Sends `signal` to every process in the box.
    fn signal(&self, id: &BoxId, signal: Signal) -> Result<(), Refusal>;

    /// The box as it is now.
    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal>;

    /// What the box has used and holds now.
    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal>;

    /// Every box on the node.
    fn list(&self) -> Result<Vec<BoxInfo>, Refusal>;

    /// Ends every process in the box and removes it, with any writable layer it owns.
    fn delete(&self, id: &BoxId) -> Result<(), Refusal>;
}

/// CPU milli, memory bytes and writable-volume bytes; devices are not inferred from a pool.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapacityResources {
    pub cpu_milli: u64,
    pub mem_bytes: u64,
    pub disk_bytes: u64,
}

impl CapacityResources {
    pub fn holds(self, need: Self) -> bool {
        self.cpu_milli >= need.cpu_milli
            && self.mem_bytes >= need.mem_bytes
            && self.disk_bytes >= need.disk_bytes
    }

    pub fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            cpu_milli: self.cpu_milli.checked_add(other.cpu_milli)?,
            mem_bytes: self.mem_bytes.checked_add(other.mem_bytes)?,
            disk_bytes: self.disk_bytes.checked_add(other.disk_bytes)?,
        })
    }

    pub fn checked_sub(self, other: Self) -> Option<Self> {
        Some(Self {
            cpu_milli: self.cpu_milli.checked_sub(other.cpu_milli)?,
            mem_bytes: self.mem_bytes.checked_sub(other.mem_bytes)?,
            disk_bytes: self.disk_bytes.checked_sub(other.disk_bytes)?,
        })
    }
}

/// Initialized per-box cost, fixed to reviewed source/config/measurement evidence by `id`.
/// Bounds describe the actual guest envelope. `guest_overhead` pays kernel/relay resources inside
/// that envelope; `overhead` pays the VMM on its host, outside it. Neither has an invented default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCost {
    pub id: [u8; 32],
    pub floor: CapacityResources,
    pub ceiling: CapacityResources,
    /// Explicit positive physical/API unit in every dimension, including unchanged units of one.
    pub quantum: CapacityResources,
    pub guest_overhead: CapacityResources,
    pub overhead: CapacityResources,
}

/// The immutable host charge admitted for the original workload, with its exact cost profile.
/// Persisted in BoxSpec and its existing BoxRecord, never a second resource ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Allocation {
    pub resources: CapacityResources,
    pub cost: RuntimeCost,
}

fn invalid_cost(why: &'static str) -> Refusal {
    Refusal::Invalid {
        what: "runtime allocation",
        why: why.into(),
    }
}

impl RuntimeCost {
    pub fn check(self) -> Result<(), Refusal> {
        if self.id == [0; 32]
            || !self.ceiling.holds(self.floor)
            || !self.ceiling.holds(self.guest_overhead)
            || self.quantum.cpu_milli == 0
            || self.quantum.mem_bytes == 0
            || self.quantum.disk_bytes == 0
        {
            return Err(invalid_cost(
                "missing cost identity, inconsistent bounds or zero quantum",
            ));
        }
        self.ceiling
            .checked_add(self.overhead)
            .ok_or_else(|| invalid_cost("host ceiling overflows"))?;
        Ok(())
    }

    /// One formula for candidate fit, receiver admission, durable recovery and runtime checks.
    pub fn allocate(self, workload: CapacityResources) -> Result<Allocation, Refusal> {
        self.check()?;
        let inside = workload
            .checked_add(self.guest_overhead)
            .ok_or_else(|| invalid_cost("workload plus guest allowance overflows"))?;
        let round = |value: u64, floor: u64, quantum: u64| {
            let value = value.max(floor);
            let remainder = value.checked_rem(quantum)?;
            if remainder == 0 {
                Some(value)
            } else {
                value.checked_add(quantum.checked_sub(remainder)?)
            }
        };
        let guest = CapacityResources {
            cpu_milli: round(
                inside.cpu_milli,
                self.floor.cpu_milli,
                self.quantum.cpu_milli,
            )
            .ok_or_else(|| invalid_cost("CPU quantization overflows"))?,
            mem_bytes: round(
                inside.mem_bytes,
                self.floor.mem_bytes,
                self.quantum.mem_bytes,
            )
            .ok_or_else(|| invalid_cost("memory quantization overflows"))?,
            disk_bytes: round(
                inside.disk_bytes,
                self.floor.disk_bytes,
                self.quantum.disk_bytes,
            )
            .ok_or_else(|| invalid_cost("disk quantization overflows"))?,
        };
        if !self.ceiling.holds(guest) {
            return Err(invalid_cost(
                "workload and guest allowance exceed the initialized runtime ceiling",
            ));
        }
        Ok(Allocation {
            resources: guest
                .checked_add(self.overhead)
                .ok_or_else(|| invalid_cost("host charge overflows"))?,
            cost: self,
        })
    }
}

impl Allocation {
    /// Recheck the original durable demand rather than recalculating against a newer cost profile.
    pub fn check(self, workload: CapacityResources) -> Result<(), Refusal> {
        if self.cost.allocate(workload)? != self {
            return Err(invalid_cost(
                "charge disagrees with its immutable workload and cost profile",
            ));
        }
        Ok(())
    }

    /// Actual VM configuration is derived from the admitted host charge, paying host overhead once.
    pub fn guest(self) -> Result<CapacityResources, Refusal> {
        self.cost.check()?;
        let guest = self
            .resources
            .checked_sub(self.cost.overhead)
            .ok_or_else(|| invalid_cost("host charge cannot hold its VMM allowance"))?;
        if !guest.holds(self.cost.floor)
            || !self.cost.ceiling.holds(guest)
            || guest.cpu_milli.checked_rem(self.cost.quantum.cpu_milli) != Some(0)
            || guest.mem_bytes.checked_rem(self.cost.quantum.mem_bytes) != Some(0)
            || guest.disk_bytes.checked_rem(self.cost.quantum.disk_bytes) != Some(0)
        {
            return Err(invalid_cost(
                "admitted guest envelope violates the initialized bounds or units",
            ));
        }
        Ok(guest)
    }

    /// BoxSpec has workload caps, while its scheduler row also retains the full original demand.
    /// A reservation may exceed those caps; it may never shrink them to fit the VM.
    pub fn covers(self, workload: CapacityResources) -> Result<(), Refusal> {
        let inside = workload
            .checked_add(self.cost.guest_overhead)
            .ok_or_else(|| invalid_cost("workload plus guest allowance overflows"))?;
        if !self.guest()?.holds(inside) {
            return Err(invalid_cost(
                "admitted guest envelope does not cover unchanged workload caps",
            ));
        }
        Ok(())
    }
}

/// A stable resource-pool identity and its physical ceiling/current room. Reservations remain the
/// scheduler's authority; these facts do not grant resources or guess unread measurements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capacity {
    pub id: String,
    pub total: CapacityResources,
    pub free: CapacityResources,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub devices: Vec<DeviceInfo>,
    /// None is the legacy native identity cost. A VM adapter must supply its verified profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<RuntimeCost>,
}

impl Capacity {
    pub fn check(&self) -> Result<(), Refusal> {
        if self.id.is_empty()
            || self.free.cpu_milli > self.total.cpu_milli
            || self.free.mem_bytes > self.total.mem_bytes
            || self.free.disk_bytes > self.total.disk_bytes
        {
            return Err(Refusal::Invalid {
                what: "runtime capacity",
                why: "pool identity is empty or free resources exceed their totals".to_owned(),
            });
        }
        if let Some(cost) = self.cost {
            cost.check()?;
            if !self
                .total
                .holds(cost.allocate(CapacityResources::default())?.resources)
            {
                return Err(invalid_cost(
                    "minimum initialized runtime charge exceeds its physical pool",
                ));
            }
        }
        crate::devices::check_ids(self.devices.iter().map(|device| device.id))
    }
}

/// A program started in a box.
pub trait Proc: Send {
    /// Its process id on the node (0 in a simulator).
    fn pid(&self) -> u32;

    /// Its end if it has ended; never blocks.
    fn try_wait(&mut self) -> Result<Option<Exit>, Refusal>;

    /// Waits at most `timeout` for its end; `Ok(None)` when it is still running.
    fn wait_for(&mut self, timeout: Duration) -> Result<Option<Exit>, Refusal>;

    /// Ends this program, and only it (its box and the box's other programs run on), as KILL does: its end
    /// then reads as `Exit::Signal(9)`, unless it had ended first. `Ok` once it is sent, or when the program
    /// has ended already; it never reaches a process given the program's pid since.
    fn kill(&mut self) -> Result<(), Refusal>;

    /// The caller's ends of the streams its exec asked for ([`crate::Stdio::ask`]), once: a later call gives
    /// none.
    fn streams(&mut self) -> Result<crate::Given, Refusal>;
}

/// A program and the caller's ends of the streams its exec asked for ([`crate::Stdio::make_asked`]), held
/// until [`Proc::streams`] takes them: a runtime whose programs take pipes wraps its own `Proc` in it.
pub struct Asked<P> {
    proc: P,
    given: Option<crate::Given>,
}

impl<P: Proc> Asked<P> {
    pub fn new(proc: P, given: crate::Given) -> Self {
        Self {
            proc,
            given: Some(given),
        }
    }
}

impl<P: Proc> Proc for Asked<P> {
    fn pid(&self) -> u32 {
        self.proc.pid()
    }

    fn try_wait(&mut self) -> Result<Option<Exit>, Refusal> {
        self.proc.try_wait()
    }

    fn wait_for(&mut self, timeout: Duration) -> Result<Option<Exit>, Refusal> {
        self.proc.wait_for(timeout)
    }

    fn kill(&mut self) -> Result<(), Refusal> {
        self.proc.kill()
    }

    fn streams(&mut self) -> Result<crate::Given, Refusal> {
        Ok(self.given.take().unwrap_or_default())
    }
}

impl Proc for Box<dyn Proc> {
    fn pid(&self) -> u32 {
        (**self).pid()
    }

    fn try_wait(&mut self) -> Result<Option<Exit>, Refusal> {
        (**self).try_wait()
    }

    fn wait_for(&mut self, timeout: Duration) -> Result<Option<Exit>, Refusal> {
        (**self).wait_for(timeout)
    }

    fn kill(&mut self) -> Result<(), Refusal> {
        (**self).kill()
    }

    fn streams(&mut self) -> Result<crate::Given, Refusal> {
        (**self).streams()
    }
}

/// How a program ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Exit {
    /// It exited with this status.
    Code(i32),
    /// This signal ended it.
    Signal(i32),
    /// It ended where its node agent couldn't collect how: while the agent was down, or (an `oci`
    /// box's init) after a restart, when the init is no longer the agent's child.
    Unknown,
}

/// A signal to send. Its number is the Linux one; Windows holds only KILL and TERM, both ending
/// the box's job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Signal(u8);

impl Signal {
    pub const HUP: Self = Self(1);
    pub const INT: Self = Self(2);
    pub const QUIT: Self = Self(3);
    pub const KILL: Self = Self(9);
    pub const USR1: Self = Self(10);
    pub const USR2: Self = Self(12);
    pub const TERM: Self = Self(15);

    /// Signal number `n`, 1 to 64.
    pub fn new(n: i64) -> Result<Self, Refusal> {
        u8::try_from(n)
            .ok()
            .filter(|n| (1..=64).contains(n))
            .map(Self)
            .ok_or(Refusal::Invalid {
                what: "signal",
                why: format!("{n} is outside 1..=64"),
            })
    }

    pub fn number(self) -> i32 {
        i32::from(self.0)
    }
}

/// Whether anything runs in a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BoxState {
    /// Nothing runs in it.
    Empty,
    /// At least one process runs in it.
    Running,
    /// An `oci` box whose init waits for [`Runtime::start`].
    Created,
}

/// How a macOS `process` box holds its members, which its node reports so a caller knows which (the
/// native plan's step 2). `None` where it doesn't apply: Linux and Windows boxes, and the simulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Membership {
    /// A process group, bounded by the node's polling watcher — the rootless class, with no helper
    /// installed (step 1).
    ProcessGroup,
    /// A user of its own from the installer's pool, started and bounded by the privileged helper (step 2).
    BoxUser,
}

/// A box as its runtime holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoxInfo {
    pub id: BoxId,
    pub class: Class,
    pub state: BoxState,
    pub bounds: Bounds,
    /// Taken back after its node agent restarted, rather than made by this one.
    pub adopted: bool,
    /// How a macOS `process` box holds its members (the helper's box user, or the rootless process
    /// group), so its inspect says which; `None` off macOS. Added after the first release, so absent on
    /// the wire means `None` (append-only, CLAUDE.md).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub membership: Option<Membership>,
    /// How an `oci` box's init ended, once it has: the container's own exit. `None` while it runs,
    /// and for a `process` box, which has no init. Added after the first release, so absent on the
    /// wire means `None` (append-only, CLAUDE.md).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<Exit>,
    /// Programs exec'd in the box that its node agent ended because the caller watching them went
    /// away: its connection to the node agent's socket closed while they ran. Each was signalled KILL
    /// and reaped, never left running unwatched (decision 3 of docs/design/runtime-architecture.md).
    /// Added after the first release, so absent on the wire means 0 (append-only, CLAUDE.md).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub abandoned: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// What a box has used, and holds now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub memory_bytes: u64,
    pub memory_peak_bytes: u64,
    /// Processes the kernel killed in the box for its memory bound.
    pub oom_kills: u64,
    pub pids: u64,
    /// Forks the kernel refused the box for its process bound.
    pub pids_refused: u64,
    pub cpu_usec: u64,
    pub io_read_bytes: u64,
    pub io_write_bytes: u64,
}

/// Why a request was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("{what} isn't valid: {why}")]
    Invalid { what: &'static str, why: String },
    #[error("no box {0}")]
    NoSuchBox(BoxId),
    #[error("box {0} exists already")]
    Exists(BoxId),
    #[error("box {0} has nothing running to {1}")]
    NotRunning(BoxId, &'static str),
    #[error("{what} isn't available on {platform}: {why}")]
    Platform {
        what: &'static str,
        platform: &'static str,
        why: String,
    },
    #[error("the node holds its most boxes ({0})")]
    Full(usize),
    /// A per-node budget other than the box count is spent: `what` names it, `bound` is its size.
    #[error("the node holds its most {what} ({bound})")]
    Exhausted { what: &'static str, bound: usize },
    #[error("{op} failed: {why}")]
    Os { op: &'static str, why: String },
    #[error("the node agent's state is unusable: {0}")]
    Broken(String),
    /// The named box retains its resources while an operation is pending or its physical result
    /// is uncertain. Busy names exclusive lifecycle ownership, never absence or completion.
    #[error("box {id}: {cause}")]
    Lifecycle { id: BoxId, cause: LifecycleCause },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "lowercase")]
pub enum LifecycleCause {
    #[error("native operation is pending")]
    Pending,
    #[error("physical termination or absence has not been proved")]
    Uncertain,
    #[error("another operation owns this box's lifecycle")]
    Busy,
}

impl Refusal {
    /// An operating system call that failed, with what was being done.
    pub fn os(op: &'static str, err: impl std::fmt::Display) -> Self {
        Self::Os {
            op,
            why: err.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resources(cpu_milli: u64, mem_bytes: u64, disk_bytes: u64) -> CapacityResources {
        CapacityResources {
            cpu_milli,
            mem_bytes,
            disk_bytes,
        }
    }

    fn cost() -> RuntimeCost {
        // Fixture data exercises the formula, not measured hardware allowances.
        RuntimeCost {
            id: [1; 32],
            floor: resources(1000, 64 << 20, 0),
            ceiling: resources(4000, 256 << 20, 1 << 30),
            quantum: resources(1000, 1 << 20, 1),
            guest_overhead: resources(50, 3 << 20, 7),
            overhead: resources(25, 5 << 20, 11),
        }
    }

    /// AC-T7.4.1: admission and runtime pay guest/kernel and host/VMM costs once, preserving workload.
    #[test]
    fn runtime_cost_quantizes_guest_then_adds_host_allowance_exactly_once() {
        let cost = cost();
        let workload = resources(1500, (65 << 20) + 1, 100);
        let allocation = cost.allocate(workload).unwrap();
        assert_eq!(allocation.guest().unwrap(), resources(2000, 69 << 20, 107));
        assert_eq!(allocation.resources, resources(2025, 74 << 20, 118));
        allocation.check(workload).unwrap();
        allocation.covers(workload).unwrap();
        assert!(allocation.check(resources(1500, 65 << 20, 100)).is_err());
        assert_eq!(workload, resources(1500, (65 << 20) + 1, 100));
        let encoded = serde_json::to_vec(&allocation).unwrap();
        assert_eq!(
            serde_json::from_slice::<Allocation>(&encoded).unwrap(),
            allocation
        );
        let mut changed = allocation;
        changed.resources.mem_bytes += 1;
        assert!(changed.guest().is_err());
        assert!(changed.check(workload).is_err());
    }

    /// AC-T7.4.1: CPU-only dedicated Shards boxes pay the entire configured VM before placement.
    #[test]
    fn fixed_guest_floor_and_ceiling_refuse_workloads_that_cannot_fit_with_guest_overhead() {
        let mut cost = cost();
        cost.floor = resources(4000, 256 << 20, 1 << 30);
        cost.ceiling = cost.floor;
        let small = resources(10, 1 << 20, 0);
        let allocation = cost.allocate(small).unwrap();
        assert_eq!(allocation.guest().unwrap(), cost.floor);
        assert_eq!(
            allocation.resources,
            resources(4025, 261 << 20, (1 << 30) + 11)
        );
        assert!(cost.allocate(cost.floor).is_err());
        assert!(allocation.covers(cost.floor).is_err());
    }

    /// AC-T7.4.1: every absent/zero unit, ceiling violation and overflow is a typed refusal.
    #[test]
    fn runtime_cost_refuses_unknown_profile_bad_units_ceiling_and_arithmetic_overflow() {
        for quantum in [resources(0, 1, 1), resources(1, 0, 1), resources(1, 1, 0)] {
            let mut changed = cost();
            changed.quantum = quantum;
            assert!(matches!(
                changed.allocate(resources(1, 1, 1)),
                Err(Refusal::Invalid {
                    what: "runtime allocation",
                    ..
                })
            ));
        }
        let mut changed = cost();
        changed.id = [0; 32];
        assert!(changed.check().is_err());
        changed = cost();
        changed.floor.cpu_milli = 5000;
        assert!(changed.check().is_err());
        assert!(cost().allocate(resources(u64::MAX, 1, 1)).is_err());
        assert!(cost().allocate(resources(4000, 1, 1)).is_err());
        changed = cost();
        changed.ceiling.disk_bytes = u64::MAX;
        assert!(changed.check().is_err());
        changed.overhead.disk_bytes = 0;
        changed.guest_overhead.disk_bytes = 0;
        changed.quantum.disk_bytes = 2;
        assert!(changed.allocate(resources(1, 1, u64::MAX)).is_err());
    }

    #[test]
    fn a_signal_number_outside_the_kernels_range_is_refused() {
        assert!(Signal::new(0).is_err());
        assert!(Signal::new(65).is_err());
        assert!(Signal::new(-9).is_err());
        assert_eq!(Signal::new(9).unwrap(), Signal::KILL);
        assert_eq!(Signal::new(64).unwrap().number(), 64);
    }

    /// Append-only wire (CLAUDE.md): a `BoxInfo` written before `exit` existed reads as `None`, and
    /// one with no exit is written without the field, so an older reader sees what it always did.
    #[test]
    fn a_box_info_without_an_exit_reads_and_writes_as_before() {
        let old = r#"{"id":"b","class":"oci","state":"empty","bounds":{},"adopted":false}"#;
        let info: BoxInfo = serde_json::from_str(old).unwrap();
        assert_eq!(info.exit, None);
        assert!(!serde_json::to_string(&info).unwrap().contains("exit"));
        let ended = BoxInfo {
            membership: None,
            exit: Some(Exit::Signal(9)),
            ..info
        };
        let back: BoxInfo = serde_json::from_str(&serde_json::to_string(&ended).unwrap()).unwrap();
        assert_eq!(back.exit, Some(Exit::Signal(9)));
    }

    /// HEX-SOCK-6, append-only: a `BoxInfo` written before `abandoned` existed reads as 0, one with
    /// none abandoned is written without the field, and a count round-trips.
    #[test]
    fn a_box_info_without_abandoned_programs_reads_and_writes_as_before() {
        let old = r#"{"id":"b","class":"process","state":"running","bounds":{},"adopted":false}"#;
        let info: BoxInfo = serde_json::from_str(old).unwrap();
        assert_eq!(info.abandoned, 0);
        assert!(!serde_json::to_string(&info).unwrap().contains("abandoned"));
        let left = BoxInfo {
            membership: None,
            abandoned: 2,
            ..info
        };
        let back: BoxInfo = serde_json::from_str(&serde_json::to_string(&left).unwrap()).unwrap();
        assert_eq!(back.abandoned, 2);
    }
}
