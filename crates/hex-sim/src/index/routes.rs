//! AC-T7.4.1: initialized routes and real backing charges inside the existing index.
//! Provisioning IDs have no observation until the same native owner supplies one.

use super::*;
use crate::model::{Arch, Os, SharedRoutes, capacity_resources};
use hex_api::{
    Allocation, BackingKey, BackingPhase, CapacityResources, DeviceId, Execution, ImageArch,
    ImageOs, RouteOffer, RouteReceipt,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PhysicalSlot {
    pub id: Option<DeviceId>,
    pub observed: Option<Device>,
    pub routes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Holder {
    Box(WorkloadId),
    Backing(BackingKey),
    /// One physical claim during observer-to-workload handoff; never two independently free slots.
    Handoff {
        backing: BackingKey,
        workload: WorkloadId,
    },
}

impl Holder {
    pub fn workload(self) -> Option<WorkloadId> {
        match self {
            Self::Box(workload) | Self::Handoff { workload, .. } => Some(workload),
            Self::Backing(_) => None,
        }
    }
    pub fn backing(self) -> Option<BackingKey> {
        match self {
            Self::Backing(key) | Self::Handoff { backing: key, .. } => Some(key),
            Self::Box(_) => None,
        }
    }
}

pub(super) fn native_id(device: &Device) -> Option<DeviceId> {
    Some(DeviceId {
        kind: match device.kind {
            DeviceKind::Gpu => hex_api::DeviceKind::Gpu,
            DeviceKind::Vfio => hex_api::DeviceKind::Vfio,
            DeviceKind::Block => hex_api::DeviceKind::Block,
            DeviceKind::Net => hex_api::DeviceKind::Net,
        },
        key: device.key?,
    })
}

pub(super) fn physical(node: &NodeSpec) -> Result<Vec<PhysicalSlot>, SimError> {
    let mut out = Vec::new();
    out.try_reserve_exact(node.devices.len())
        .map_err(|_| SimError::Resources(node.id))?;
    for (at, device) in node.devices.iter().enumerate() {
        let mask = node
            .routes
            .as_ref()
            .and_then(|routes| routes.device_routes.get(at))
            .copied()
            .unwrap_or(0);
        out.push(PhysicalSlot {
            id: native_id(device),
            observed: Some(*device),
            routes: mask,
        });
    }
    let Some(routes) = &node.routes else {
        return Ok(out);
    };
    let mut known = Vec::new();
    known
        .try_reserve_exact(out.len())
        .map_err(|_| SimError::Resources(node.id))?;
    for slot in &out {
        known.push(slot.id.ok_or(SimError::Config(
            "initialized routes require stable physical IDs",
        ))?);
    }
    known.sort_unstable();
    let count = routes
        .backings
        .iter()
        .try_fold(0usize, |n, backing| n.checked_add(backing.devices.len()))
        .ok_or(SimError::Resources(node.id))?;
    let mut provisioned = Vec::new();
    provisioned
        .try_reserve_exact(count)
        .map_err(|_| SimError::Resources(node.id))?;
    for backing in &routes.backings {
        provisioned.extend_from_slice(&backing.devices);
    }
    provisioned.sort_unstable();
    provisioned.dedup();
    provisioned.retain(|id| known.binary_search(id).is_err());
    let total = out
        .len()
        .checked_add(provisioned.len())
        .ok_or(SimError::Resources(node.id))?;
    if total > crate::model::DEVICES {
        return Err(SimError::Full {
            what: "canonical physical slots",
            bound: crate::model::DEVICES,
        });
    }
    out.try_reserve_exact(provisioned.len())
        .map_err(|_| SimError::Resources(node.id))?;
    out.extend(provisioned.into_iter().map(|id| PhysicalSlot {
        id: Some(id),
        observed: None,
        routes: 0,
    }));
    let gpus = out
        .iter()
        .filter(|slot| {
            slot.id
                .is_some_and(|id| id.kind == hex_api::DeviceKind::Gpu)
        })
        .count();
    if u32::try_from(gpus) != Ok(node.capacity.gpus) {
        return Err(SimError::Config(
            "physical GPU claim budget differs from the canonical slot union",
        ));
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Choice {
    pub at: usize,
    pub offer: RouteOffer,
}
impl Choice {
    pub fn receipt(self) -> RouteReceipt {
        RouteReceipt {
            key: self.offer.key,
            backing: self.offer.backing,
        }
    }
}

#[derive(Debug)]
pub(super) struct Routes {
    pub catalog: SharedRoutes,
    pub used: Vec<CapacityResources>,
    pub phases: Vec<BackingPhase>,
}

pub(super) fn copy<T: Copy>(values: &[T], node: NodeId) -> Result<Vec<T>, SimError> {
    let mut out = Vec::new();
    out.try_reserve_exact(values.len())
        .map_err(|_| SimError::Resources(node))?;
    out.extend_from_slice(values);
    Ok(out)
}

pub(super) fn claim_indices(
    physical: &[PhysicalSlot],
    claims: &[DeviceId],
    node: NodeId,
) -> Result<Vec<usize>, SimError> {
    let mut indices = Vec::new();
    indices
        .try_reserve_exact(claims.len())
        .map_err(|_| SimError::Resources(node))?;
    if claims.is_empty() {
        return Ok(indices);
    }
    let mut by_id = Vec::new();
    by_id
        .try_reserve_exact(physical.len())
        .map_err(|_| SimError::Resources(node))?;
    by_id.extend(
        physical
            .iter()
            .enumerate()
            .filter_map(|(at, slot)| slot.id.map(|id| (id, at))),
    );
    by_id.sort_unstable_by_key(|(id, _)| *id);
    for id in claims {
        let at = by_id
            .binary_search_by_key(id, |(id, _)| *id)
            .ok()
            .and_then(|at| by_id.get(at))
            .map(|(_, at)| *at)
            .ok_or(SimError::Config(
                "backing provision claim has no canonical physical slot",
            ))?;
        indices.push(at);
    }
    Ok(indices)
}

pub(super) fn backing_charge(
    backing: &hex_api::BackingOffer,
    node: NodeId,
) -> Result<Resources, SimError> {
    let allocation = backing
        .allocation()
        .map_err(|_| SimError::Resources(node))?;
    Ok(Resources {
        cpu_milli: allocation.resources.cpu_milli,
        mem_bytes: allocation.resources.mem_bytes,
        disk_bytes: allocation.resources.disk_bytes,
        // Physical exclusivity is counted from the SAME holder slots, not a second pool GPU debit.
        gpus: 0,
    })
}

pub(super) fn restore_backings(
    node: NodeId,
    routes: &Routes,
    physical: &[PhysicalSlot],
    holders: &mut [Option<Holder>],
    free: &mut Resources,
) -> Result<(), SimError> {
    for (backing, phase) in routes.catalog.backings.iter().zip(&routes.phases) {
        if matches!(phase, BackingPhase::Prepared | BackingPhase::Released) {
            continue;
        }
        let indices = claim_indices(physical, &backing.devices, node)?;
        if indices
            .iter()
            .any(|&at| holders.get(at).is_none_or(Option::is_some))
        {
            return Err(SimError::Config(
                "backing physical grant overlaps another owner",
            ));
        }
        *free = free
            .checked_sub(&backing_charge(backing, node)?)
            .ok_or(SimError::Resources(node))?;
        for at in indices {
            *holders.get_mut(at).ok_or(SimError::Resources(node))? =
                Some(Holder::Backing(backing.key));
        }
    }
    Ok(())
}

impl Routes {
    pub fn rank_cpu(&self) -> u64 {
        self.catalog
            .backings
            .iter()
            .zip(&self.phases)
            .filter(|(_, phase)| **phase == BackingPhase::Live)
            .filter_map(|(backing, _)| {
                let mut shared = false;
                let used = self
                    .catalog
                    .offers
                    .iter()
                    .zip(&self.used)
                    .filter(|(offer, _)| {
                        offer.backing == Some(backing.key)
                            && offer.key.execution == Execution::ShardsShared
                    })
                    .try_fold(0u64, |sum, (_, used)| {
                        shared = true;
                        sum.checked_add(used.cpu_milli)
                    })?;
                shared
                    .then(|| {
                        backing
                            .cost
                            .floor
                            .cpu_milli
                            .checked_sub(backing.cost.guest_overhead.cpu_milli)?
                            .checked_sub(used)
                    })
                    .flatten()
            })
            .max()
            .unwrap_or(0)
    }

    pub fn refreshed(&self, catalog: &SharedRoutes, node: NodeId) -> Result<Self, SimError> {
        let mut next = Self::new(catalog, node)?;
        for (offer, used) in self.catalog.offers.iter().zip(&self.used) {
            if *used == CapacityResources::default() {
                continue;
            }
            let at = next
                .catalog
                .offers
                .iter()
                .position(|new| new.key == offer.key && new.backing == offer.backing)
                .ok_or(SimError::Config(
                    "refreshed routes remove a retained workload receipt",
                ))?;
            *next.used.get_mut(at).ok_or(SimError::Resources(node))? = *used;
        }
        for (backing, phase) in self.catalog.backings.iter().zip(&self.phases) {
            if matches!(phase, BackingPhase::Prepared | BackingPhase::Released) {
                continue;
            }
            let (new, new_phase) = next.backing(backing.key).ok_or(SimError::Config(
                "backing disappearance is not native release proof",
            ))?;
            if new.cost != backing.cost
                || new.devices != backing.devices
                || matches!(
                    (phase, new_phase),
                    (
                        BackingPhase::Live | BackingPhase::Draining,
                        BackingPhase::Prepared | BackingPhase::Granted
                    ) | (BackingPhase::Draining, BackingPhase::Live)
                )
            {
                return Err(SimError::Config(
                    "refreshed backing changes a retained grant or rolls back lifecycle",
                ));
            }
            if new_phase == BackingPhase::Released
                && next
                    .catalog
                    .offers
                    .iter()
                    .zip(&next.used)
                    .any(|(offer, used)| {
                        offer.backing == Some(backing.key)
                            && offer.key.execution == Execution::ShardsShared
                            && *used != CapacityResources::default()
                    })
            {
                return Err(SimError::Config(
                    "released backing still has retained inner workload reservations",
                ));
            }
        }
        for (backing, phase) in next.catalog.backings.iter().zip(&next.phases) {
            if !matches!(
                phase,
                BackingPhase::Granted | BackingPhase::Live | BackingPhase::Draining
            ) {
                continue;
            }
            let used = next
                .catalog
                .offers
                .iter()
                .zip(&next.used)
                .filter(|(offer, _)| {
                    offer.backing == Some(backing.key)
                        && offer.key.execution == Execution::ShardsShared
                })
                .try_fold(CapacityResources::default(), |sum, (_, used)| {
                    sum.checked_add(*used)
                })
                .ok_or(SimError::Resources(node))?;
            backing
                .check_inner(used)
                .map_err(|_| SimError::Resources(node))?;
        }
        Ok(next)
    }

    pub fn new(catalog: &SharedRoutes, node: NodeId) -> Result<Self, SimError> {
        let mut used = Vec::new();
        used.try_reserve_exact(catalog.offers.len())
            .map_err(|_| SimError::Resources(node))?;
        used.resize(catalog.offers.len(), CapacityResources::default());
        let mut phases = Vec::new();
        phases
            .try_reserve_exact(catalog.backings.len())
            .map_err(|_| SimError::Resources(node))?;
        phases.extend(catalog.backings.iter().map(|backing| backing.phase));
        Ok(Self {
            catalog: catalog.clone(),
            used,
            phases,
        })
    }
    pub fn try_clone(&self, node: NodeId) -> Result<Self, SimError> {
        Ok(Self {
            catalog: self.catalog.clone(),
            used: copy(&self.used, node)?,
            phases: copy(&self.phases, node)?,
        })
    }
    pub fn backing(&self, key: BackingKey) -> Option<(&hex_api::BackingOffer, BackingPhase)> {
        self.catalog
            .backings
            .iter()
            .enumerate()
            .find(|(_, b)| b.key == key)
            .and_then(|(at, b)| self.phases.get(at).copied().map(|phase| (b, phase)))
    }
    pub fn chosen(&self, demand: &Demand) -> Result<Choice, Verdict> {
        if demand
            .ask
            .runtime_class
            .is_some_and(|class| class != hex_api::Class::Process)
            && (demand.ask.os.is_none() || demand.ask.arch.is_none())
        {
            return Err(Verdict::NoPlatform);
        }
        let mut found = None;
        for (at, offer) in self.catalog.offers.iter().enumerate() {
            let form_matches = demand.ask.runtime_class.map_or_else(
                || match demand.class {
                    Class::Oci => matches!(
                        offer.key.class,
                        hex_api::Class::Process | hex_api::Class::Oci
                    ),
                    Class::Firecracker => offer.key.class == hex_api::Class::Firecracker,
                    Class::Shards => offer.key.class == hex_api::Class::Shards,
                },
                |class| offer.key.class == class,
            );
            if !form_matches
                || (demand.ask.runtime_class.is_none() && offer.key.execution != Execution::Native)
                || !offer.key.gpu.accepts(demand.need.gpus != 0)
                || (demand.ask.native_only && offer.key.execution != Execution::Native)
            {
                continue;
            }
            let platform = offer.key.image;
            let os = match platform.os {
                ImageOs::Linux => Os::Linux,
                ImageOs::Windows => Os::Windows,
                ImageOs::Macos => Os::Macos,
            };
            let arch = match platform.arch {
                ImageArch::Amd64 => Arch::X86_64,
                ImageArch::Arm64 => Arch::Aarch64,
            };
            if demand.ask.os.is_some_and(|asked| asked != os)
                || demand.ask.arch.is_some_and(|asked| asked != arch)
            {
                continue;
            }
            if found.is_some() {
                return Err(Verdict::NoPlatform);
            }
            found = Some(Choice { at, offer: *offer });
        }
        found.ok_or(Verdict::NoPlatform)
    }
    pub fn committed(&self, receipt: RouteReceipt) -> Result<Choice, SimError> {
        self.catalog
            .offers
            .iter()
            .enumerate()
            .find(|(_, offer)| offer.key == receipt.key && offer.backing == receipt.backing)
            .map(|(at, offer)| Choice { at, offer: *offer })
            .ok_or(SimError::Config(
                "committed route/backing differs from the initialized offer",
            ))
    }
    pub fn allocation(
        &self,
        choice: Choice,
        demand: &Demand,
    ) -> Result<Option<Allocation>, hex_api::Refusal> {
        choice
            .offer
            .cost
            .map(|cost| cost.allocate(capacity_resources(demand.need)))
            .transpose()
    }
    pub fn charged(
        &self,
        choice: Choice,
        demand: &Demand,
        allocation: Option<Allocation>,
    ) -> Result<Resources, hex_api::Refusal> {
        crate::model::reserved_resources(demand.need, allocation, Some(choice.receipt()))
    }

    pub fn holds_inner(&self, choice: Choice, demand: &Demand) -> bool {
        let Some(key) = choice.offer.backing else {
            return true;
        };
        let Some((backing, BackingPhase::Live)) = self.backing(key) else {
            return false;
        };
        if choice.offer.key.execution != Execution::ShardsShared {
            return true;
        }
        let used = self
            .catalog
            .offers
            .iter()
            .zip(&self.used)
            .filter(|(offer, _)| {
                offer.backing == Some(key) && offer.key.execution == Execution::ShardsShared
            })
            .try_fold(CapacityResources::default(), |sum, (_, used)| {
                sum.checked_add(*used)
            })
            .and_then(|used| used.checked_add(capacity_resources(demand.need)));
        used.is_some_and(|used| backing.check_inner(used).is_ok())
    }
    pub fn used_next(
        &self,
        receipt: RouteReceipt,
        demand: &Demand,
        adding: bool,
    ) -> Result<(usize, CapacityResources), SimError> {
        let choice = self.committed(receipt)?;
        let used = self
            .used
            .get(choice.at)
            .ok_or(SimError::Config("missing route usage"))?;
        let amount = capacity_resources(demand.need);
        let next = if adding {
            used.checked_add(amount)
        } else {
            used.checked_sub(amount)
        }
        .ok_or(SimError::Config(
            "route payload usage overflows or underflows",
        ))?;
        Ok((choice.at, next))
    }
}

impl FitIndex {
    /// The one index grants a prepared canonical incarnation BEFORE the native owner may boot it.
    pub fn grant_backing(
        &mut self,
        node: NodeId,
        key: BackingKey,
    ) -> Result<hex_api::BackingGrant, SimError> {
        let at = usize::try_from(node.0).map_err(|_| SimError::UnknownNode(node))?;
        let slot = self.slot(node)?;
        let routes = slot
            .routes
            .as_ref()
            .ok_or(SimError::Config("backing has no initialized route owner"))?;
        let (backing, phase) = routes
            .backing(key)
            .ok_or(SimError::Config("unknown canonical backing generation"))?;
        let offer = backing.try_clone().map_err(|_| SimError::Resources(node))?;
        let grant = hex_api::BackingGrant {
            key,
            allocation: offer.allocation().map_err(|_| SimError::Resources(node))?,
            devices: offer.devices,
        };
        if matches!(phase, BackingPhase::Granted | BackingPhase::Live) {
            return Ok(grant);
        }
        if phase != BackingPhase::Prepared {
            return Err(SimError::Config(
                "backing lifecycle cannot accept a new grant",
            ));
        }
        let physical = self.devices.get(at).ok_or(SimError::UnknownNode(node))?;
        let holders = self.held.get(at).ok_or(SimError::UnknownNode(node))?;
        let indices = claim_indices(physical, &grant.devices, node)?;
        if indices
            .iter()
            .any(|&index| holders.get(index).is_none_or(Option::is_some))
        {
            return Err(SimError::Config(
                "backing physical grant conflicts with another owner",
            ));
        }
        let charge = backing_charge(backing, node)?;
        let free = slot
            .free
            .checked_sub(&charge)
            .ok_or(SimError::Resources(node))?;
        let backing_at = routes
            .catalog
            .backings
            .iter()
            .position(|backing| backing.key == key)
            .ok_or(SimError::Resources(node))?;
        let from = bucket_slot(slot, self.last);
        let slot = self.slots.get_mut(at).ok_or(SimError::UnknownNode(node))?;
        *slot
            .routes
            .as_mut()
            .and_then(|routes| routes.phases.get_mut(backing_at))
            .ok_or(SimError::Resources(node))? = BackingPhase::Granted;
        slot.free = free;
        let to = bucket_slot(slot, self.last);
        let holders = self.held.get_mut(at).ok_or(SimError::UnknownNode(node))?;
        for index in indices {
            *holders.get_mut(index).ok_or(SimError::Resources(node))? = Some(Holder::Backing(key));
        }
        self.ix.relink(node.0, from, to);
        self.recount_gpus(node)?;
        Ok(grant)
    }

    /// A native result may advance only its exact grant. Disappearance, controller exit or a
    /// different generation cannot release any resource/physical claim.
    pub fn observe_backing(
        &mut self,
        node: NodeId,
        observed: &hex_api::BackingOffer,
    ) -> Result<(), SimError> {
        let at = usize::try_from(node.0).map_err(|_| SimError::UnknownNode(node))?;
        let slot = self.slot(node)?;
        let routes = slot
            .routes
            .as_ref()
            .ok_or(SimError::Config("backing has no initialized route owner"))?;
        let (backing, phase) = routes
            .backing(observed.key)
            .ok_or(SimError::Config("unknown canonical backing generation"))?;
        if backing.cost != observed.cost || backing.devices != observed.devices {
            return Err(SimError::Config(
                "native result changes the exact backing grant",
            ));
        }
        if phase == observed.phase {
            return Ok(());
        }
        let legal = matches!(
            (phase, observed.phase),
            (
                BackingPhase::Granted,
                BackingPhase::Live | BackingPhase::Draining | BackingPhase::Released
            ) | (BackingPhase::Live, BackingPhase::Draining)
                | (BackingPhase::Draining, BackingPhase::Released)
        );
        if !legal {
            return Err(SimError::Config(
                "native backing result rolls back or bypasses admission",
            ));
        }
        let releasing = observed.phase == BackingPhase::Released;
        if releasing
            && routes
                .catalog
                .offers
                .iter()
                .zip(&routes.used)
                .any(|(offer, used)| {
                    offer.backing == Some(observed.key)
                        && offer.key.execution == Execution::ShardsShared
                        && *used != CapacityResources::default()
                })
        {
            return Err(SimError::Config(
                "backing release still has retained inner reservations",
            ));
        }
        let next_free = if releasing {
            slot.free
                .checked_add(&backing_charge(backing, node)?)
                .filter(|free| slot.capacity.holds(free))
                .ok_or(SimError::Resources(node))?
        } else {
            slot.free
        };
        let backing_at = routes
            .catalog
            .backings
            .iter()
            .position(|backing| backing.key == observed.key)
            .ok_or(SimError::Resources(node))?;
        let from = bucket_slot(slot, self.last);
        let slot = self.slots.get_mut(at).ok_or(SimError::UnknownNode(node))?;
        *slot
            .routes
            .as_mut()
            .and_then(|routes| routes.phases.get_mut(backing_at))
            .ok_or(SimError::Resources(node))? = observed.phase;
        slot.free = next_free;
        let to = bucket_slot(slot, self.last);
        if releasing {
            for holder in self.held.get_mut(at).ok_or(SimError::UnknownNode(node))? {
                *holder = match *holder {
                    Some(Holder::Backing(key)) if key == observed.key => None,
                    Some(Holder::Handoff { backing, workload }) if backing == observed.key => {
                        Some(Holder::Box(workload))
                    }
                    other => other,
                };
            }
        }
        self.ix.relink(node.0, from, to);
        self.recount_gpus(node)
    }
}
