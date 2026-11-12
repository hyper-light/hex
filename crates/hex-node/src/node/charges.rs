//! Joint plan step 14 (AC-T7.4.1; `docs/design/learned-charge.md`): the node agent's charge learner, the one authority
//! on what its microVMs take, as the owner of their cgroups. Once a window (C1: `cpu.max`'s period) it reads each
//! running VM's cgroup and learns its charge and booking (`hex_sim::charge`). It then writes the placement that holds
//! the rule (C4, C5): each VM's `cpu.weight` by its booking and `memory.high` at its budget, its fleet's parent
//! (`<node root>/.vm/<node key>`) capped at the fleet's CPU budget and weighed by it on the machine's scale, and `.vm`
//! weighed by the budgets of every fleet under it. A capacity answer for a VM class carries what it learned (C2, C8:
//! `hex_api::Learned`). A Shards guest can't yet be told its room: shards-api has no call that sends init's `ROOM`
//! (D188), so that is refused `Awaits(GuestRoom)` while its VMM's `memory.high` holds it at its budget (C7).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Weak};
use std::time::{Duration, Instant};

use hex_api::{
    Allocation, Bounds, BoxId, CapacityResources, Learned, LearnedHolding, LearnedPrior,
    LearnedUse, MAX_LEARNED_PRIORS, Refusal, RuntimeCost, Upstream, Usage,
};
use hex_core::cgroup::{CPU_PERIOD_US, CpuMax, Write};
use hex_runtime::linux::cgroup::BoxCgroup;
use hex_sim::charge::{self, ChargeError, ClassKey, Cpu, Sample};
use hex_sim::model::Class;

use super::Node;

/// The window the learner reads over (C1): `cpu.max`'s period, the one the kernel enforces a VM's CPU over.
pub(super) const WINDOW: Duration = Duration::from_micros(CPU_PERIOD_US);

/// One CPU in milli-CPU, the unit a vCPU and the machine's CPU are counted in.
const CPU_MILLI: u64 = 1_000;

/// A VM by its native scope: one VM's lifetime.
type Scope = [u8; 16];

/// What the learner reads and writes of cgroups: the kernel's in the node, a record's in its tests.
pub(super) trait Cgroups {
    fn usage(&self, cgroup: &BoxCgroup) -> Result<Usage, Refusal>;
    fn write(&self, cgroup: &BoxCgroup, write: &Write) -> Result<(), Refusal>;
    /// Each fleet's parent under `parent` (`.vm`), with its `cpu.max` as the kernel says it, `None` where it has none.
    fn fleets(&self, parent: &Path) -> Result<Vec<(PathBuf, Option<String>)>, Refusal>;
}

/// The kernel's cgroups.
pub(super) struct Kernel;

impl Cgroups for Kernel {
    fn usage(&self, cgroup: &BoxCgroup) -> Result<Usage, Refusal> {
        cgroup.managed_usage()
    }

    fn write(&self, cgroup: &BoxCgroup, write: &Write) -> Result<(), Refusal> {
        cgroup.set(write)
    }

    fn fleets(&self, parent: &Path) -> Result<Vec<(PathBuf, Option<String>)>, Refusal> {
        let listing = |e| Refusal::os("listing the VM fleets' parent cgroup", e);
        let mut out = Vec::new();
        for entry in std::fs::read_dir(parent).map_err(listing)? {
            let entry = entry.map_err(listing)?;
            if !entry.file_type().map_err(listing)?.is_dir() {
                continue;
            }
            let path = entry.path();
            let max = match std::fs::read_to_string(path.join("cpu.max")) {
                Ok(text) => Some(text),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(Refusal::os("reading a VM fleet's cpu.max", e)),
            };
            out.push((path, max));
        }
        Ok(out)
    }
}

fn refused(why: ChargeError) -> Refusal {
    match why {
        ChargeError::Full { what, bound } => Refusal::Exhausted { what, bound },
        ChargeError::Awaits(needs) => Refusal::Awaits { needs },
        other => Refusal::Invalid {
            what: "a microVM's learned charge",
            why: other.to_string(),
        },
    }
}

/// One VM the learner reads.
struct Vm {
    id: BoxId,
    class: Class,
    /// What its receipt booked: what the ledger holds of it.
    receipt: CapacityResources,
    cgroup: BoxCgroup,
    /// Its VMM's CPU so far, in microseconds, and when it was read.
    last: (u64, Instant),
    /// What its cgroup holds as the learner last wrote it, or as its create wrote it.
    written: Vec<Write>,
}

/// The node's learner (module docs).
pub(super) struct Learner {
    node: charge::Node<Scope>,
    vms: BTreeMap<Scope, Vm>,
    /// The machine's CPU in milli-CPU: the scale every weight is on (C4).
    machine: u64,
    /// This node's fleet's parent and what the learner last wrote there.
    fleet: (BoxCgroup, Vec<Write>),
    /// `.vm`, the parent of every fleet under the node's root, and what the learner last wrote there.
    parent: (BoxCgroup, Vec<Write>),
    /// The capability each Shards guest's room awaits, by its VM (C7).
    rooms: BTreeMap<Scope, Upstream>,
    /// Why the last window failed: until one passes, the capacity answers carry nothing learned.
    fault: Option<Refusal>,
}

/// The most VMs a machine of `machine` could hold, each at least the least reservation placement takes
/// (`MIN_CPU_MILLI` of its CPU, `MIN_MEMORY` of its memory).
fn population(machine: CapacityResources) -> Result<usize, Refusal> {
    let cpus = machine
        .cpu_milli
        .checked_div(u64::from(hex_api::MIN_CPU_MILLI));
    let memory = machine.mem_bytes.checked_div(hex_api::MIN_MEMORY);
    cpus.zip(memory)
        .and_then(|(cpus, memory)| usize::try_from(cpus.min(memory)).ok())
        .ok_or_else(|| Refusal::Invalid {
            what: "a microVM's learned charge",
            why: "the machine's population passes a usize".into(),
        })
}

/// A VM's vCPUs: its admitted guest's CPU in whole CPUs, rounded up.
fn vcpus(allocation: Allocation) -> Result<u32, Refusal> {
    let guest = allocation.guest()?;
    u32::try_from(guest.cpu_milli.div_ceil(CPU_MILLI)).map_err(|_| Refusal::Invalid {
        what: "a microVM's learned charge",
        why: "its guest's vCPUs pass a u32".into(),
    })
}

/// Writes what `bounds` holds that `written` doesn't, in the order the kernel needs, and keeps it as written.
fn rewrite(
    io: &impl Cgroups,
    cgroup: &BoxCgroup,
    bounds: &Bounds,
    written: &mut Vec<Write>,
) -> Result<(), Refusal> {
    let writes = hex_core::cgroup::writes(bounds, &[])?;
    for write in &writes {
        if !written.contains(write) {
            io.write(cgroup, write)?;
        }
    }
    *written = writes;
    Ok(())
}

/// A fleet's CPU budget as its `cpu.max` says it, in milli-CPU: `max`, or none read, is the whole machine.
fn budget_of(max: Option<&str>, machine: u64) -> u64 {
    match max.and_then(CpuMax::parse) {
        Some(CpuMax {
            quota_us: Some(quota),
            period_us,
        }) => u128::from(quota)
            .checked_mul(u128::from(CPU_MILLI))
            .and_then(|n| n.checked_div(u128::from(period_us)))
            .and_then(|n| u64::try_from(n).ok())
            .unwrap_or(machine),
        _ => machine,
    }
}

/// What telling a VM's guest its room takes (C7): a Shards guest needs shards-api to send init's `ROOM` (D188), which
/// it can't yet; a Firecracker guest has no room to be told, its VMM's cgroup alone holding it.
fn tell_room(class: Class) -> Result<(), Upstream> {
    match class {
        Class::Shards => Err(Upstream::GuestRoom),
        _ => Ok(()),
    }
}

impl Learner {
    /// A learner for the fleet under `fleet` (`<node root>/.vm/<node key>`) on a machine of `machine`.
    pub(super) fn new(fleet: &Path, machine: CapacityResources) -> Result<Self, Refusal> {
        let parent = fleet.parent().ok_or_else(|| Refusal::Invalid {
            what: "a microVM fleet's parent cgroup",
            why: format!("{} has no parent", fleet.display()),
        })?;
        let bound = population(machine)?;
        Ok(Self {
            node: charge::Node::new(
                WINDOW,
                machine,
                charge::Bounds {
                    vms: bound,
                    classes: bound,
                },
            )
            .map_err(refused)?,
            vms: BTreeMap::new(),
            machine: machine.cpu_milli,
            fleet: (BoxCgroup::at(fleet.to_path_buf()), Vec::new()),
            parent: (BoxCgroup::at(parent.to_path_buf()), Vec::new()),
            rooms: BTreeMap::new(),
            fault: None,
        })
    }

    /// One window (C1 to C5, C7), read at `now`: the VMs `running`, each new one found by `record` (its class and
    /// receipt; none for a box that isn't a VM with a receipt), within the fleet's `budget`. A failure is kept until a
    /// window passes.
    pub(super) fn window(
        &mut self,
        io: &impl Cgroups,
        running: Vec<(BoxId, Scope, BoxCgroup)>,
        record: impl Fn(&BoxId) -> Result<Option<(Class, Allocation)>, Refusal>,
        budget: CapacityResources,
        now: Instant,
    ) -> Result<(), Refusal> {
        let outcome = self.learn(io, running, record, budget, now);
        self.fault = outcome.as_ref().err().cloned();
        outcome
    }

    fn learn(
        &mut self,
        io: &impl Cgroups,
        running: Vec<(BoxId, Scope, BoxCgroup)>,
        record: impl Fn(&BoxId) -> Result<Option<(Class, Allocation)>, Refusal>,
        budget: CapacityResources,
        now: Instant,
    ) -> Result<(), Refusal> {
        let present: BTreeSet<Scope> = running.iter().map(|(_, scope, _)| *scope).collect();
        let gone: Vec<Scope> = self
            .vms
            .keys()
            .filter(|scope| !present.contains(*scope))
            .copied()
            .collect();
        for scope in gone {
            self.node.end(&scope).map_err(refused)?;
            self.vms.remove(&scope);
            self.rooms.remove(&scope);
        }
        for (id, scope, cgroup) in running {
            // A VM ending between the listing and its read is read no more; the next listing ends it.
            let Ok(usage) = io.usage(&cgroup) else {
                continue;
            };
            if let Some(vm) = self.vms.get_mut(&scope) {
                let (before, at) = vm.last;
                self.node
                    .observe(
                        scope,
                        &Sample {
                            cpu: Cpu::Whole(Duration::from_micros(
                                usage.cpu_usec.saturating_sub(before),
                            )),
                            span: now.saturating_duration_since(at),
                            mem_bytes: usage.memory_bytes,
                            disk_bytes: None,
                        },
                    )
                    .map_err(refused)?;
                vm.last = (usage.cpu_usec, now);
                continue;
            }
            let Some((class, allocation)) = record(&id)? else {
                continue;
            };
            let receipt = allocation.booking();
            self.node
                .track(
                    scope,
                    ClassKey::new(class, allocation.cost.id, allocation.resources),
                    0,
                    vcpus(allocation)?,
                    receipt,
                )
                .map_err(refused)?;
            // Its create wrote its ceiling as its `cpu.max` and `memory.max` (`Node::managed_bounds`).
            let created = Bounds {
                cpu_max_milli: Some(u32::try_from(allocation.resources.cpu_milli).map_err(
                    |_| Refusal::Invalid {
                        what: "a microVM's learned charge",
                        why: "its ceiling's CPU passes cpu.max".into(),
                    },
                )?),
                memory_max: Some(allocation.resources.mem_bytes),
                ..Bounds::default()
            };
            self.vms.insert(
                scope,
                Vm {
                    id,
                    class,
                    receipt,
                    cgroup,
                    last: (usage.cpu_usec, now),
                    written: hex_core::cgroup::writes(&created, &[])?,
                },
            );
        }
        self.node.set_budget(budget);
        self.node.settle().map_err(refused)?;
        self.place(io)?;
        for (scope, vm) in &self.vms {
            match tell_room(vm.class) {
                Ok(()) => {}
                Err(needs) => {
                    self.rooms.insert(*scope, needs);
                }
            }
        }
        Ok(())
    }

    /// Writes the placement (C4, C5): each VM, the fleet's parent, and `.vm` by the sum of its fleets' budgets.
    fn place(&mut self, io: &impl Cgroups) -> Result<(), Refusal> {
        let placed = self.node.placement(self.machine).map_err(refused)?;
        for (scope, bounds) in &placed.vms {
            if let Some(vm) = self.vms.get_mut(scope) {
                rewrite(io, &vm.cgroup, bounds, &mut vm.written)?;
            }
        }
        rewrite(io, &self.fleet.0, &placed.fleet, &mut self.fleet.1)?;
        let mut fleets = 0u64;
        for (path, max) in io.fleets(self.parent.0.dir())? {
            let budget = if path == self.fleet.0.dir() {
                u64::from(placed.fleet.cpu_max_milli.unwrap_or(0))
            } else {
                budget_of(max.as_deref(), self.machine)
            };
            fleets = fleets.saturating_add(budget);
        }
        let weight = charge::fleet_weight(fleets, self.machine).map_err(refused)?;
        let parent = Bounds {
            cpu_weight: Some(weight),
            ..Bounds::default()
        };
        rewrite(io, &self.parent.0, &parent, &mut self.parent.1)
    }

    /// The window's failure is `why`, which no read reached.
    pub(super) fn failed(&mut self, why: Refusal) {
        self.fault = Some(why);
    }

    /// What `class`'s capacity answer carries (C2, C8): the priors of `costs` (its own and its routes'), the class's
    /// learned use, and each of its running VMs' holdings, those most past their receipts first (memory, then CPU,
    /// then disk). None after a failed window, so its VMs are booked whole.
    pub(super) fn learned(
        &self,
        class: Class,
        costs: &[RuntimeCost],
    ) -> Result<Option<Learned>, Refusal> {
        if self.fault.is_some() {
            return Ok(None);
        }
        let mut priors: Vec<LearnedPrior> = Vec::new();
        for cost in costs {
            let ceiling = cost.allocate(CapacityResources::default())?.resources;
            let booking = self.node.prior(&ClassKey::new(class, cost.id, ceiling));
            let known = priors
                .iter()
                .any(|prior| prior.profile == cost.id && prior.ceiling == ceiling);
            if booking != ceiling && !known && priors.len() < MAX_LEARNED_PRIORS {
                priors.push(LearnedPrior {
                    profile: cost.id,
                    ceiling,
                    booking,
                });
            }
        }
        let mut used = LearnedUse::default();
        let mut holdings = Vec::new();
        for (scope, vm) in &self.vms {
            if vm.class != class {
                continue;
            }
            let holding = self
                .node
                .holding(scope)
                .ok_or_else(|| Refusal::Broken("a learned VM without a holding".into()))?;
            used = used
                .checked_add(LearnedUse {
                    booked: vm.receipt,
                    reservation: holding,
                })
                .ok_or_else(|| Refusal::Invalid {
                    what: "a microVM's learned charge",
                    why: "a class's learned use overflows".into(),
                })?;
            let past = (
                holding.mem_bytes.saturating_sub(vm.receipt.mem_bytes),
                holding.cpu_milli.saturating_sub(vm.receipt.cpu_milli),
                holding.disk_bytes.saturating_sub(vm.receipt.disk_bytes),
            );
            holdings.push((
                past,
                LearnedHolding {
                    id: vm.id.try_clone()?,
                    holding,
                },
            ));
        }
        holdings.sort_by(|a, b| b.0.cmp(&a.0));
        Ok(Some(Learned {
            priors,
            used,
            holdings: holdings.into_iter().map(|(_, holding)| holding).collect(),
        }))
    }

    /// The capability each guest's room awaits, by its box (C7).
    #[cfg(test)]
    fn awaits(&self) -> Vec<(&BoxId, Upstream)> {
        self.rooms
            .iter()
            .filter_map(|(scope, needs)| self.vms.get(scope).map(|vm| (&vm.id, *needs)))
            .collect()
    }
}

impl Node {
    /// One window of the node's charge learner (joint plan step 14), its failure kept for the capacity answers.
    fn learn_window(&self) {
        let Some(learner) = self.backends.get().and_then(|b| b.learner.as_ref()) else {
            return;
        };
        let Ok(mut learner) = learner.lock() else {
            return;
        };
        let read = self
            .supervisor
            .running_vms()
            .and_then(|running| Ok((running, self.native_capacity()?.total)));
        let (running, budget) = match read {
            Ok(read) => read,
            Err(why) => return learner.failed(why),
        };
        let record = |id: &BoxId| -> Result<Option<(Class, Allocation)>, Refusal> {
            let Some(record) = self.record(id)? else {
                return Ok(None);
            };
            let class = match record.spec.class.class() {
                hex_api::Class::Shards => Class::Shards,
                hex_api::Class::Firecracker => Class::Firecracker,
                hex_api::Class::Process | hex_api::Class::Oci => return Ok(None),
            };
            Ok(record.spec.allocation.map(|allocation| (class, allocation)))
        };
        // Its outcome is kept as the learner's fault, which the capacity answers read.
        let _ = learner.window(&Kernel, running, record, budget, Instant::now());
    }

    /// What `class`'s capacity answer carries of the learner (C8), fitted to one message.
    pub(super) fn attach_learned(
        &self,
        class: hex_api::Class,
        capacity: &mut hex_api::Capacity,
    ) -> Result<(), Refusal> {
        let class = match class {
            hex_api::Class::Shards => Class::Shards,
            hex_api::Class::Firecracker => Class::Firecracker,
            hex_api::Class::Process | hex_api::Class::Oci => return Ok(()),
        };
        let Some(learner) = self.backends.get().and_then(|b| b.learner.as_ref()) else {
            return Ok(());
        };
        let mut costs = Vec::new();
        costs
            .try_reserve_exact(capacity.routes.len().saturating_add(1))
            .map_err(|_| Refusal::Exhausted {
                what: "a capacity answer's cost profiles",
                bound: capacity.routes.len(),
            })?;
        costs.extend(capacity.cost);
        costs.extend(capacity.routes.iter().filter_map(|route| route.cost));
        capacity.learned = learner
            .lock()
            .map_err(|_| Refusal::Broken("the charge learner's lock was poisoned".into()))?
            .learned(class, &costs)?;
        hex_core::wire::fit_learned(capacity).map_err(|e| Refusal::Invalid {
            what: "a capacity answer",
            why: e.to_string(),
        })
    }
}

/// Starts the node's charge learner (joint plan step 14): a thread woken each window by a monotonic timer, which ends
/// once the node is gone.
pub(super) fn spawn(node: Weak<Node>) -> Result<(), Refusal> {
    use rustix::time::{
        Itimerspec, TimerfdClockId, TimerfdFlags, TimerfdTimerFlags, Timespec, timerfd_create,
        timerfd_settime,
    };
    let timer = timerfd_create(TimerfdClockId::Monotonic, TimerfdFlags::CLOEXEC)
        .map_err(|e| Refusal::os("making the charge learner's timer", e))?;
    let period = Timespec {
        tv_sec: i64::try_from(WINDOW.as_secs()).unwrap_or(i64::MAX),
        tv_nsec: i64::from(WINDOW.subsec_nanos()),
    };
    timerfd_settime(
        &timer,
        TimerfdTimerFlags::empty(),
        &Itimerspec {
            it_interval: period,
            it_value: period,
        },
    )
    .map_err(|e| Refusal::os("setting the charge learner's timer", e))?;
    std::thread::Builder::new()
        .name("hex-charges".to_owned())
        .spawn(move || {
            let mut expirations = [0u8; 8];
            loop {
                match rustix::io::read(&timer, &mut expirations) {
                    Ok(_) | Err(rustix::io::Errno::INTR) => {}
                    Err(_) => return,
                }
                let Some(node) = node.upgrade() else {
                    return;
                };
                node.learn_window();
            }
        })
        .map_err(|e| Refusal::os("starting the charge learner", e))?;
    Ok(())
}

/// The learner's lock, as `Backends` holds it.
pub(super) type Shared = Mutex<Learner>;

#[cfg(test)]
mod tests;
