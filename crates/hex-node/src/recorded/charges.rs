//! Joint plan step 14 on macOS and Windows (AC-T7.4.1; `docs/design/learned-charge.md` C1 to C5, C8): the charge
//! learner of a node whose platform caps a managed microVM's CPU rather than weighs it. Once a window, the platform's
//! own ([`crate::recorded::Recover::charge_window`]), it reads each running VM's usage through its native owner,
//! learns its charge and booking (`hex_sim::charge`), and caps it at its water-filled share (`charge::Node::rates`):
//! macOS's managed control's rate, Windows' job's hard cap ([`crate::recorded::Recover::set_managed_rate`]). Every
//! VM so keeps at least its booking and the caps fit the fleet's CPU budget, as Linux's weights hold them (C4).
//! Neither platform can hold a VM's memory at a budget below its ceiling: macOS ends a process past its footprint
//! limit and Windows refuses a commit past a job's, and neither throttles at a level as `memory.high` does, so each
//! VM's budget is refused, typed, naming that (C5). Its last resort ends the VM most past its own budget through its
//! managed receipt ([`crate::recorded::Recover::stop_managed`]), by the state every OS's learner shares
//! ([`LastResort`]), as Linux's kills its cgroup. Its capacity answers carry what it learned, as Linux's do (C8).

use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

use hex_api::{
    BoxId, BoxInfo, CapacityResources, Learned, LearnedEnd, LearnedUse, MAX_LEARNED_HOLDINGS,
    Refusal, RuntimeCost, Upstream, Usage,
};
use hex_core::record::{BoxRecord, Phase};
use hex_sim::charge::{self, ChargeError, ClassKey, Cpu, Excess, Sample};
use hex_sim::model::Class;

use crate::store::Store;

/// A VM by its native scope: one VM's lifetime.
pub(crate) type Scope = [u8; 16];

/// One CPU in milli-CPU, the unit a vCPU and the machine's CPU are counted in.
const CPU_MILLI: u64 = 1_000;

/// What the learner reads and writes of a node's managed VMs: their native owner's in the node, a record's in tests.
pub(crate) trait Managed {
    fn usage(&self, record: &BoxRecord) -> Result<Usage, Refusal>;
    /// Caps the VM at `rate` milli-CPU.
    fn set_rate(&self, record: &BoxRecord, rate: u64) -> Result<(), Refusal>;
    /// Ends the VM through its managed receipt: the last resort (C5).
    fn stop(&self, record: &BoxRecord) -> Result<(), Refusal>;
    /// Writes `end`, why the last resort is ending the VM of `scope`, into box `id`'s record where it is still that VM's
    /// lifetime, before its end (C5, intent first): whether it was written.
    fn intend(&self, id: &BoxId, scope: Scope, end: &LearnedEnd) -> Result<bool, Refusal>;
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

/// Whether `record` is a managed microVM that runs now: made, with its native lifetime, its receipt and its controller,
/// and not ended.
pub(crate) fn running(record: &BoxRecord) -> bool {
    record.phase == Phase::Ready
        && record.native_scope.is_some()
        && record.spec.allocation.is_some()
        && record.init.is_some()
        && record.exit.is_none()
        && matches!(
            record.spec.class.class(),
            hex_api::Class::Shards | hex_api::Class::Firecracker
        )
}

/// What moving a VM of `class` to another machine instead of ending it awaits, where that move is another product's
/// (C5): a Shards VM saved whole and resumed elsewhere is Shards' D176, which shards-api's image run (D231) doesn't
/// offer. A Firecracker VM's move would be Hex's own, through Firecracker's snapshot, which Hex doesn't drive yet: none
/// to await.
fn move_awaits(class: Class) -> Option<Upstream> {
    match class {
        Class::Shards => Some(Upstream::LiveMove),
        _ => None,
    }
}

/// The last resort's state from window to window (C5), explicit (R04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Resort {
    /// What the fleet's VMs hold, with the fastest growth on record in one window, is within its budget.
    Held,
    /// It wasn't: the VM `id` held most past its own budget, lowest band first, and was ended for `why`. None other is
    /// ended until it is gone, so one end is never two.
    Ending {
        scope: Scope,
        id: BoxId,
        why: Excess,
    },
    /// It isn't, and the node can't act: every VM holds within its own budget, growth being no fact yet, or ending
    /// the VM the rule chose was refused. Kept until a window moves it.
    Refused(Refusal),
}

/// The last resort and its record of why (C5), the same on every OS: one VM ended at a time, as the core's verdict
/// (`charge::Node::last_resort`) chooses it, and why kept for its class's capacity answers and its box's record.
pub(crate) struct LastResort {
    resort: Resort,
    /// The VMs it ended and why, by class, oldest first: at most `ends` of them, the oldest forgotten first, a machine
    /// never holding more VMs than its population at once.
    ended: VecDeque<(Class, LearnedEnd)>,
    ends: usize,
    /// The same ends not yet in their boxes' records, oldest first, by box and VM lifetime: the node writes each into
    /// its record after the window ([`write_ends`]) and keeps one whose write failed for the next, at most `ends` of
    /// them, the oldest forgotten first.
    unrecorded: VecDeque<(BoxId, Scope, LearnedEnd)>,
}

impl LastResort {
    /// A last resort keeping at most `ends` ends: the machine's VM population, at most a node's boxes.
    pub(crate) fn new(ends: usize) -> Self {
        Self {
            resort: Resort::Held,
            ended: VecDeque::new(),
            ends,
            unrecorded: VecDeque::new(),
        }
    }

    /// One window's last resort, once the placement holds what it can: while a VM it ended still runs (`vm` names it),
    /// nothing more, so one end is never two; else the core's verdict on `node`, the VM it chooses ended by `end` and
    /// why recorded, or refused, typed, where the node can't act. `vm` names each VM the learner holds: its class and box.
    pub(crate) fn window<'a>(
        &mut self,
        node: &charge::Node<Scope>,
        vm: impl Fn(&Scope) -> Option<(Class, &'a BoxId)>,
        end: impl FnOnce(&Scope) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        if let Resort::Ending { scope, .. } = &self.resort
            && vm(scope).is_some()
        {
            return Ok(());
        }
        self.resort = match node.last_resort().map_err(refused)? {
            charge::Resort::Held => Resort::Held,
            charge::Resort::Within(fleet) => Resort::Refused(Refusal::Exhausted {
                what: "a microVM fleet's memory budget, every VM within its own",
                bound: usize::try_from(fleet.budget).unwrap_or(usize::MAX),
            }),
            charge::Resort::End { vm: scope, why } => {
                let (class, id) = vm(&scope).ok_or_else(|| {
                    Refusal::Broken("the last resort chose a VM the learner doesn't hold".into())
                })?;
                let id = id.try_clone()?;
                match end(&scope) {
                    Ok(()) => {
                        self.record_end(class, &id, scope, why)?;
                        Resort::Ending { scope, id, why }
                    }
                    Err(refusal) => Resort::Refused(refusal),
                }
            }
        };
        Ok(())
    }

    /// Keeps why the last resort ended the VM `id` of lifetime `scope`, for its class's capacity answers and for its
    /// box's record, forgetting the oldest end past the bound.
    fn record_end(
        &mut self,
        class: Class,
        id: &BoxId,
        scope: Scope,
        why: Excess,
    ) -> Result<(), Refusal> {
        let end = LearnedEnd {
            id: id.try_clone()?,
            held: why.held,
            budget: why.budget,
            band: why.band,
            fleet_held: why.fleet.held,
            fleet_growth: why.fleet.growth,
            fleet_budget: why.fleet.budget,
            unmoved: move_awaits(class),
        };
        let bound = self.ends;
        let full = |what| Refusal::Exhausted { what, bound };
        if self.ended.len() >= bound {
            self.ended.pop_front();
        }
        if self.unrecorded.len() >= bound {
            self.unrecorded.pop_front();
        }
        self.ended
            .try_reserve(1)
            .map_err(|_| full("the last resort's ends"))?;
        self.unrecorded
            .try_reserve(1)
            .map_err(|_| full("the last resort's unrecorded ends"))?;
        self.unrecorded
            .push_back((id.try_clone()?, scope, end.try_clone()?));
        self.ended.push_back((class, end));
        Ok(())
    }

    /// The VMs of `class` it ended and why, latest first (C5, C8).
    pub(crate) fn ended(&self, class: Class) -> Result<Vec<LearnedEnd>, Refusal> {
        let mut ended = Vec::new();
        for (of, end) in self.ended.iter().rev() {
            if *of == class {
                ended.try_reserve(1).map_err(|_| Refusal::Exhausted {
                    what: "a class's learned ends",
                    bound: self.ends,
                })?;
                ended.push(end.try_clone()?);
            }
        }
        Ok(ended)
    }

    /// Takes the ends not yet in their boxes' records, oldest first (C5).
    pub(crate) fn unrecorded(&mut self) -> VecDeque<(BoxId, Scope, LearnedEnd)> {
        std::mem::take(&mut self.unrecorded)
    }

    /// Keeps `left`, ends whose record writes failed, for the next window, before any made since and within the bound.
    pub(crate) fn keep_unrecorded(&mut self, mut left: VecDeque<(BoxId, Scope, LearnedEnd)>) {
        left.append(&mut self.unrecorded);
        while left.len() > self.ends {
            left.pop_front();
        }
        self.unrecorded = left;
    }

    /// The last resort's state (C5).
    #[cfg(test)]
    pub(crate) fn resort(&self) -> &Resort {
        &self.resort
    }
}

/// Writes why the last resort ended VM `id` into its record in `store`, where the record is still that VM's lifetime
/// (`scope`): false where none is, the box deleted since or made again.
pub(crate) fn record_last_resort(
    store: &Store,
    id: &BoxId,
    scope: Scope,
    end: &LearnedEnd,
) -> Result<bool, Refusal> {
    let end = end.try_clone()?;
    store.update(id, move |record| {
        if record.native_scope.as_ref().map(|s| *s.bytes()) != Some(scope) {
            return false;
        }
        record.last_resort = Some(end);
        true
    })
}

/// Writes each of `ends` into its box's record in `store` (C5), off the learner's lock, as the node does after each
/// window: the ends whose writes failed, which the node keeps for the next window, and the first failure, which fails
/// this one.
pub(crate) fn write_ends(
    store: &Store,
    mut ends: VecDeque<(BoxId, Scope, LearnedEnd)>,
) -> (VecDeque<(BoxId, Scope, LearnedEnd)>, Option<Refusal>) {
    let mut failed = None;
    ends.retain(
        |(id, scope, end)| match record_last_resort(store, id, *scope, end) {
            Ok(_) => false,
            Err(why) => {
                failed.get_or_insert(why);
                true
            }
        },
    );
    (ends, failed)
}

/// A VM box's inspect from its backend, with why the last resort ended it where its record says so (C5), across the
/// node's restarts.
pub(crate) fn with_last_resort(info: &mut BoxInfo, record: &BoxRecord) -> Result<(), Refusal> {
    info.last_resort = record
        .last_resort
        .as_ref()
        .map(LearnedEnd::try_clone)
        .transpose()?;
    Ok(())
}

/// One VM the learner holds.
struct Vm {
    id: BoxId,
    class: Class,
    /// What its receipt booked: what the ledger holds of it.
    receipt: CapacityResources,
    /// Its VMM's CPU so far, in microseconds, and when it was read.
    last: (u64, Instant),
    /// The cap it runs under: its allocation's CPU until the learner first caps it.
    rate: u64,
}

/// The node's learner (module docs).
pub(crate) struct Learner {
    node: charge::Node<Scope>,
    vms: BTreeMap<Scope, Vm>,
    /// Why the last window failed: until one passes, the capacity answers carry nothing learned.
    fault: Option<Refusal>,
    /// Why no VM's memory budget is held on this platform (C5), typed.
    unheld: Refusal,
    /// The last resort (C5).
    last: LastResort,
}

/// The most VMs a machine of `machine` could hold, each at least the least reservation placement takes.
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
fn vcpus(allocation: hex_api::Allocation) -> Result<u32, Refusal> {
    let guest = allocation.guest()?;
    u32::try_from(guest.cpu_milli.div_ceil(CPU_MILLI)).map_err(|_| Refusal::Invalid {
        what: "a microVM's learned charge",
        why: "its guest's vCPUs pass a u32".into(),
    })
}

impl Learner {
    /// A learner over `window` on a machine of `machine`.
    pub(crate) fn new(window: Duration, machine: CapacityResources) -> Result<Self, Refusal> {
        let bound = population(machine)?;
        Ok(Self {
            node: charge::Node::new(
                window,
                machine,
                charge::Bounds {
                    vms: bound,
                    classes: bound,
                },
            )
            .map_err(refused)?,
            vms: BTreeMap::new(),
            fault: None,
            last: LastResort::new(bound.min(MAX_LEARNED_HOLDINGS)),
            unheld: Refusal::Platform {
                what: "memory_high",
                platform: std::env::consts::OS,
                why: "a managed VM's memory is held at its ceiling alone: the platform ends (macOS's footprint \
                      limit) or refuses (Windows' job commit limit) past a bound, and throttles at no level below it"
                    .to_owned(),
            },
        })
    }

    /// One window (C1 to C4) read at `now`: the managed VMs `running` now (their records), within the fleet's `budget`.
    /// A failure is kept until a window passes.
    pub(crate) fn window(
        &mut self,
        io: &impl Managed,
        running: &[BoxRecord],
        budget: CapacityResources,
        now: Instant,
    ) -> Result<(), Refusal> {
        let outcome = self.learn(io, running, budget, now);
        self.fault = outcome.as_ref().err().cloned();
        outcome
    }

    fn learn(
        &mut self,
        io: &impl Managed,
        running: &[BoxRecord],
        budget: CapacityResources,
        now: Instant,
    ) -> Result<(), Refusal> {
        let scope_of = |record: &BoxRecord| record.native_scope.as_ref().map(|s| *s.bytes());
        let gone: Vec<Scope> = self
            .vms
            .keys()
            .filter(|scope| !running.iter().any(|r| scope_of(r) == Some(**scope)))
            .copied()
            .collect();
        for scope in gone {
            self.node.end(&scope).map_err(refused)?;
            self.vms.remove(&scope);
        }
        // What each VM used of the cap it ran under in this window, for its next cap.
        let mut used = BTreeMap::new();
        for record in running {
            let Some(scope) = scope_of(record) else {
                continue;
            };
            let Some(allocation) = record.spec.allocation else {
                continue;
            };
            // A VM ending between the listing and its read is read no more; the next listing ends it.
            let Ok(usage) = io.usage(record) else {
                continue;
            };
            if let Some(vm) = self.vms.get_mut(&scope) {
                let (before, at) = vm.last;
                let cpu = Duration::from_micros(usage.cpu_usec.saturating_sub(before));
                let span = now.saturating_duration_since(at);
                self.node
                    .observe(
                        scope,
                        &Sample {
                            cpu: Cpu::Whole(cpu),
                            span,
                            mem_bytes: usage.memory_bytes,
                            disk_bytes: None,
                        },
                    )
                    .map_err(refused)?;
                let milli = cpu
                    .as_micros()
                    .checked_mul(u128::from(CPU_MILLI))
                    .and_then(|n| n.checked_div(span.as_micros().max(1)))
                    .and_then(|n| u64::try_from(n).ok())
                    .unwrap_or(u64::MAX);
                used.insert(scope, (milli, vm.rate));
                vm.last = (usage.cpu_usec, now);
                continue;
            }
            let class = match record.spec.class.class() {
                hex_api::Class::Shards => Class::Shards,
                hex_api::Class::Firecracker => Class::Firecracker,
                hex_api::Class::Process | hex_api::Class::Oci => continue,
            };
            let receipt = allocation.booking();
            self.node
                .track(
                    scope,
                    ClassKey::new(class, allocation.cost.id, allocation.resources),
                    record.spec.band,
                    vcpus(allocation)?,
                    receipt,
                )
                .map_err(refused)?;
            self.vms.insert(
                scope,
                Vm {
                    id: record.spec.id.try_clone()?,
                    class,
                    receipt,
                    last: (usage.cpu_usec, now),
                    rate: allocation.resources.cpu_milli,
                },
            );
        }
        self.node.set_budget(budget);
        self.node.settle().map_err(refused)?;
        let rates = self
            .node
            .rates(budget.cpu_milli, &used, u64::from(hex_api::MIN_CPU_MILLI))
            .map_err(refused)?;
        for (scope, rate) in rates {
            let Some(vm) = self.vms.get_mut(&scope) else {
                continue;
            };
            if vm.rate == rate {
                continue;
            }
            let record = running
                .iter()
                .find(|record| scope_of(record) == Some(scope))
                .ok_or_else(|| Refusal::Broken("a learned VM without its record".into()))?;
            io.set_rate(record, rate)?;
            vm.rate = rate;
        }
        let vms = &self.vms;
        self.last.window(
            &self.node,
            |scope| vms.get(scope).map(|vm| (vm.class, &vm.id)),
            |scope| {
                let record = running
                    .iter()
                    .find(|record| scope_of(record) == Some(*scope))
                    .ok_or_else(|| Refusal::Broken("a learned VM without its record".into()))?;
                io.stop(record)
            },
        )
    }

    /// Takes the ends not yet in their boxes' records, oldest first (C5).
    pub(crate) fn unrecorded(&mut self) -> VecDeque<(BoxId, Scope, LearnedEnd)> {
        self.last.unrecorded()
    }

    /// Keeps `left`, ends whose record writes failed, for the next window (C5).
    pub(crate) fn keep_unrecorded(&mut self, left: VecDeque<(BoxId, Scope, LearnedEnd)>) {
        self.last.keep_unrecorded(left);
    }

    /// The window's failure is `why`, which no read reached.
    pub(crate) fn failed(&mut self, why: Refusal) {
        self.fault = Some(why);
    }

    /// Why no VM's memory budget is held on this platform (C5).
    pub(crate) fn unheld(&self) -> &Refusal {
        &self.unheld
    }

    /// The cap each VM runs under now, by its lifetime.
    #[cfg(test)]
    fn rate(&self, scope: &Scope) -> Option<u64> {
        self.vms.get(scope).map(|vm| vm.rate)
    }

    /// What `class`'s capacity answer carries (C2, C8): the priors of `costs`, the class's learned use, and each of
    /// its running VMs' holdings, by their records' ids, those most past their receipts first. None after a failed
    /// window, so its VMs are booked whole.
    pub(crate) fn learned(
        &self,
        class: Class,
        costs: &[RuntimeCost],
        running: &[BoxRecord],
    ) -> Result<Option<Learned>, Refusal> {
        if self.fault.is_some() {
            return Ok(None);
        }
        let mut vms = Vec::new();
        for record in running {
            let Some(scope) = record.native_scope.as_ref().map(|s| *s.bytes()) else {
                continue;
            };
            if let Some(vm) = self.vms.get(&scope)
                && vm.class == class
            {
                vms.push((scope, &record.spec.id, vm.receipt));
            }
        }
        learned(&self.node, class, costs, vms, self.last.ended(class)?).map(Some)
    }
}

/// What a capacity answer for `class` carries of `node` (C2, C5, C8), shared by every OS's learner: the priors of
/// `costs` (at most one per profile and ceiling, none booked whole), the class's learned use and each of its `vms`'
/// holdings (by scope, box and receipt), those most past their receipts first (memory, then CPU, then disk), and the
/// class's `ended` VMs.
pub(crate) fn learned(
    node: &charge::Node<Scope>,
    class: Class,
    costs: &[RuntimeCost],
    vms: Vec<(Scope, &hex_api::BoxId, CapacityResources)>,
    ended: Vec<hex_api::LearnedEnd>,
) -> Result<Learned, Refusal> {
    let mut priors: Vec<hex_api::LearnedPrior> = Vec::new();
    for cost in costs {
        let ceiling = cost.allocate(CapacityResources::default())?.resources;
        let booking = node.prior(&ClassKey::new(class, cost.id, ceiling));
        let known = priors
            .iter()
            .any(|prior| prior.profile == cost.id && prior.ceiling == ceiling);
        if booking != ceiling && !known && priors.len() < hex_api::MAX_LEARNED_PRIORS {
            priors.push(hex_api::LearnedPrior {
                profile: cost.id,
                ceiling,
                booking,
            });
        }
    }
    let mut used = LearnedUse::default();
    let mut holdings = Vec::new();
    holdings
        .try_reserve_exact(vms.len())
        .map_err(|_| Refusal::Exhausted {
            what: "a class's learned holdings",
            bound: vms.len(),
        })?;
    for (scope, id, receipt) in vms {
        let holding = node
            .holding(&scope)
            .ok_or_else(|| Refusal::Broken("a learned VM without a holding".into()))?;
        used = used
            .checked_add(LearnedUse {
                booked: receipt,
                reservation: holding,
            })
            .ok_or_else(|| Refusal::Invalid {
                what: "a microVM's learned charge",
                why: "a class's learned use overflows".into(),
            })?;
        let past = (
            holding.mem_bytes.saturating_sub(receipt.mem_bytes),
            holding.cpu_milli.saturating_sub(receipt.cpu_milli),
            holding.disk_bytes.saturating_sub(receipt.disk_bytes),
        );
        holdings.push((
            past,
            hex_api::LearnedHolding {
                id: id.try_clone()?,
                holding,
            },
        ));
    }
    holdings.sort_by_key(|(past, _)| std::cmp::Reverse(*past));
    Ok(Learned {
        priors,
        used,
        holdings: holdings.into_iter().map(|(_, holding)| holding).collect(),
        ended,
    })
}

#[cfg(test)]
mod tests;
