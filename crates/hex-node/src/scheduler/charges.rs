//! Joint plan step 14 (AC-T7.4.1; `docs/design/learned-charge.md`): the node's shell around `hex_sim::charge`. Once
//! a window it reads every microVM the node runs on its own allocation ([`crate::placement::Driver::measured_vms`])
//! and learns their charges and bookings (C1 to C3). On each refresh it gives the node's facts the priors it learned,
//! which the placement index books a new VM's receipt at (C2), and moves the node's capacity by what the ledger's
//! receipts book past the learned bookings, so the ledger's free room is the budget less what the VMs are learned to
//! take (C3, C6).

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use hex_api::{CapacityResources, Refusal};
use hex_sim::charge::{self, Bounds, ChargeError, ClassKey, Cpu, Sample};
use hex_sim::model::{Class, NodeSpec, Prior};
use hex_sim::peer::Key;

use crate::placement::MeasuredVm;

/// One CPU in milli-CPU, the unit a vCPU is counted in.
const CPU_MILLI: u64 = 1_000;

pub(super) struct Charges {
    node: charge::Node<Key>,
    /// Each VM's last reading: its VMM's CPU so far, in microseconds, and when.
    last: BTreeMap<Key, (u64, Instant)>,
    /// What each VM's receipt booked: what the ledger holds of it.
    receipts: BTreeMap<Key, CapacityResources>,
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

/// The most VMs the node could ever hold, each at least the least reservation placement takes
/// (`MIN_CPU_MILLI` of the machine's CPUs, `MIN_MEMORY` of its memory), as `placement::budget` counts a node's
/// population, from the machine's totals rather than its free room, so it holds through every refresh.
pub(super) fn bound(facts: &hex_sim::facts::Facts) -> Result<usize, Refusal> {
    let cpus = u64::from(facts.cpu.threads)
        .checked_mul(CPU_MILLI)
        .and_then(|cpu| cpu.checked_div(u64::from(hex_api::MIN_CPU_MILLI)));
    let memory = facts.mem.total.checked_div(hex_api::MIN_MEMORY);
    cpus.zip(memory)
        .and_then(|(cpus, memory)| usize::try_from(cpus.min(memory)).ok())
        .ok_or_else(|| Refusal::Invalid {
            what: "a microVM's learned charge",
            why: "the machine's population passes a usize".into(),
        })
}

/// A VM's vCPUs: its admitted guest's CPU in whole CPUs, rounded up.
fn vcpus(allocation: hex_api::Allocation) -> Result<u32, Refusal> {
    let guest = allocation.guest()?;
    u32::try_from(guest.cpu_milli.div_ceil(CPU_MILLI)).map_err(|_| Refusal::Invalid {
        what: "a microVM's learned charge",
        why: "its guest's vCPUs pass a u32".into(),
    })
}

fn sum(a: CapacityResources, b: CapacityResources) -> Result<CapacityResources, Refusal> {
    a.checked_add(b).ok_or_else(|| Refusal::Invalid {
        what: "a microVM's learned charge",
        why: "the receipts' bookings overflow".into(),
    })
}

impl Charges {
    /// A node enforcing CPU over `window`, holding at most `bound` VMs and as many classes.
    pub(super) fn new(window: Duration, bound: usize) -> Result<Self, Refusal> {
        Ok(Self {
            node: charge::Node::new(
                window,
                CapacityResources::default(),
                Bounds {
                    vms: bound,
                    classes: bound,
                },
            )
            .map_err(refused)?,
            last: BTreeMap::new(),
            receipts: BTreeMap::new(),
        })
    }

    /// One window's readings of the node's microVMs, read at `now` (C1, C3). A VM read for the first time starts its
    /// windows, booked at what its receipt booked; one no longer read has ended.
    pub(super) fn observe(&mut self, vms: &[MeasuredVm], now: Instant) -> Result<(), Refusal> {
        let present: BTreeSet<Key> = vms.iter().map(|vm| vm.key).collect();
        let gone: Vec<Key> = self
            .node
            .ids()
            .filter(|key| !present.contains(key))
            .copied()
            .collect();
        for key in gone {
            self.node.end(&key).map_err(refused)?;
            self.last.remove(&key);
            self.receipts.remove(&key);
        }
        for vm in vms {
            if !self.node.holds(&vm.key) {
                let class = ClassKey::new(
                    vm.class,
                    vm.allocation.cost.id,
                    vm.allocation.resources,
                );
                self.node
                    .track(
                        vm.key,
                        class,
                        0,
                        vcpus(vm.allocation)?,
                        vm.allocation.booking(),
                    )
                    .map_err(refused)?;
                self.receipts.insert(vm.key, vm.allocation.booking());
                self.last.insert(vm.key, (vm.usage.cpu_usec, now));
                continue;
            }
            let (before, at) = self
                .last
                .get(&vm.key)
                .copied()
                .unwrap_or((vm.usage.cpu_usec, now));
            self.node
                .observe(
                    vm.key,
                    &Sample {
                        cpu: Cpu::Whole(Duration::from_micros(
                            vm.usage.cpu_usec.saturating_sub(before),
                        )),
                        span: now.saturating_duration_since(at),
                        mem_bytes: vm.usage.memory_bytes,
                        disk_bytes: None,
                    },
                )
                .map_err(refused)?;
            self.last.insert(vm.key, (vm.usage.cpu_usec, now));
        }
        self.node.settle().map_err(refused)
    }

    /// Gives `local` what the node learned (C2, C3, C6): its budget is `local`'s capacity, each VM class's prior is
    /// set where its class has warmed evidence, and its capacity is moved by what the receipts book past the learned
    /// bookings and what VMs hold past those, so the ledger, booking the receipts, leaves the budget less what the VMs
    /// are learned to take.
    pub(super) fn apply(&mut self, local: &mut NodeSpec) -> Result<(), Refusal> {
        let budget = CapacityResources {
            cpu_milli: local.capacity.cpu_milli,
            mem_bytes: local.capacity.mem_bytes,
            disk_bytes: local.capacity.disk_bytes,
        };
        self.node.set_budget(budget);
        let mut profiles: Vec<(Class, hex_api::RuntimeCost)> = Vec::new();
        for class in [Class::Shards, Class::Firecracker] {
            if let Some(cost) = local.costs.get(class.index()).copied().flatten() {
                profiles.push((class, cost));
            }
        }
        if let Some(routes) = &local.routes {
            for offer in &routes.offers {
                let class = match offer.key.class {
                    hex_api::Class::Shards => Class::Shards,
                    hex_api::Class::Firecracker => Class::Firecracker,
                    hex_api::Class::Process | hex_api::Class::Oci => continue,
                };
                if let Some(cost) = offer.cost {
                    profiles.push((class, cost));
                }
            }
        }
        local.priors = Default::default();
        for (class, cost) in profiles {
            let ceiling = cost.allocate(CapacityResources::default())?.resources;
            let prior = self.node.prior(&ClassKey::new(class, cost.id, ceiling));
            let slot = local
                .priors
                .get_mut(class.index())
                .ok_or_else(|| Refusal::Broken("a runtime class past the classes".into()))?;
            if prior != ceiling && slot.is_none() {
                *slot = Some(Prior {
                    profile: cost.id,
                    booking: prior,
                });
            }
        }
        let receipts = self
            .receipts
            .values()
            .try_fold(CapacityResources::default(), |total, more| {
                sum(total, *more)
            })?;
        let learned = self.node.reservation().map_err(refused)?;
        let moved = |capacity: u64, receipt: u64, taken: u64| {
            capacity.saturating_add(receipt).saturating_sub(taken)
        };
        local.capacity.cpu_milli = moved(
            local.capacity.cpu_milli,
            receipts.cpu_milli,
            learned.cpu_milli,
        );
        local.capacity.mem_bytes = moved(
            local.capacity.mem_bytes,
            receipts.mem_bytes,
            learned.mem_bytes,
        );
        local.capacity.disk_bytes = moved(
            local.capacity.disk_bytes,
            receipts.disk_bytes,
            learned.disk_bytes,
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests;
