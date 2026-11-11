//! Normalize actual initialized endpoints into the existing host/index resource plane.

use super::{amounts, least};
use hex_api::{Capacity, Refusal};
use hex_sim::model::{Class, Classes, Cluster, NodeSpec, Resources, RuntimeRoutes};

fn invalid(why: &'static str) -> Refusal {
    let mut text = String::new();
    if text.try_reserve_exact(why.len()).is_err() {
        return Refusal::Exhausted {
            what: "initialized capacity refusal bytes",
            bound: why.len(),
        };
    }
    text.push_str(why);
    Refusal::Invalid {
        what: "initialized runtime capacity",
        why: text,
    }
}

fn reserve<T>(values: &mut Vec<T>, count: usize, what: &'static str) -> Result<(), Refusal> {
    values
        .try_reserve_exact(count)
        .map_err(|_| Refusal::Exhausted { what, bound: count })
}

fn bit(at: usize) -> Result<u64, Refusal> {
    u32::try_from(at)
        .ok()
        .and_then(|at| 1u64.checked_shl(at))
        .ok_or_else(|| invalid("route eligibility exceeds its 64-bit address space"))
}

fn class(form: hex_api::Class) -> Class {
    match form {
        hex_api::Class::Process | hex_api::Class::Oci => Class::Oci,
        hex_api::Class::Shards => Class::Shards,
        hex_api::Class::Firecracker => Class::Firecracker,
    }
}

/// No native calls, boot or grants occur here. Reports are moved into one immutable view after
/// physical identity/metadata and the common resource-plane identity have been checked.
pub(super) fn apply(
    reports: Vec<(Class, Option<Capacity>)>,
    facts: &hex_sim::facts::Facts,
    cluster: &mut Cluster,
    namespace: &str,
    node: &mut NodeSpec,
    owned: Resources,
) -> Result<(), Refusal> {
    let devices = super::devices::offers(&reports, facts, cluster, namespace)?;
    let mut offers: Vec<hex_api::RouteOffer> = Vec::new();
    reserve(
        &mut offers,
        hex_api::MAX_RUNTIME_ROUTES,
        "initialized route catalog",
    )?;
    let mut plane = None;
    let mut room = None;
    let mut seen = Classes::NONE;
    for (backend, report) in &reports {
        if seen.has(*backend) {
            return Err(invalid("duplicate initialized endpoint class"));
        }
        seen = seen.with(*backend);
        let Some(report) = report else { continue };
        report.check()?;
        if report.routes.is_empty() {
            return Err(invalid(
                "legacy and initialized route reports cannot share admission",
            ));
        }
        if let Some((identity, total)) = plane {
            if identity != report.id.as_str() || total != report.total {
                return Err(invalid(
                    "initialized routes disagree about their physical host resource plane",
                ));
            }
        } else {
            plane = Some((report.id.as_str(), report.total));
        }
        let free = amounts(&report.free, 0);
        room = Some(room.map_or(free, |previous| least(previous, free)));
        for offer in &report.routes {
            if let Some(previous) = offers.iter().find(|previous| previous.key == offer.key) {
                if previous != offer {
                    return Err(invalid(
                        "route aliases disagree about cost or canonical backing",
                    ));
                }
            } else {
                if offers.len() >= hex_api::MAX_RUNTIME_ROUTES {
                    return Err(invalid("combined routes exceed their eligibility mask"));
                }
                offers.push(*offer);
            }
        }
    }
    let (_, total) = plane.ok_or_else(|| invalid("no initialized host resource plane"))?;
    let credited = room
        .ok_or_else(|| invalid("no initialized host free-room observation"))?
        .checked_add(&Resources {
            mem_bytes: owned.mem_bytes,
            disk_bytes: owned.disk_bytes,
            ..Resources::default()
        })
        .ok_or_else(|| invalid("measured owned resources plus host room overflow"))?;
    let host = least(amounts(&total, 0), credited);
    let mut by_id = Vec::new();
    reserve(
        &mut by_id,
        devices.len(),
        "normalized device identity index",
    )?;
    for (at, device) in devices.iter().enumerate() {
        by_id.push((
            device
                .native_id()
                .ok_or_else(|| invalid("initialized device lacks native identity"))?,
            at,
        ));
    }
    by_id.sort_unstable_by_key(|(id, _)| *id);
    let mut masks = Vec::new();
    reserve(&mut masks, devices.len(), "normalized device route masks")?;
    masks.resize(devices.len(), 0u64);
    let mut backings: Vec<hex_api::BackingOffer> = Vec::new();
    reserve(
        &mut backings,
        hex_api::MAX_RUNTIME_ROUTES,
        "initialized backing catalog",
    )?;
    for (_, report) in reports {
        let Some(report) = report else { continue };
        for (device, local_mask) in report.devices.iter().zip(&report.device_routes) {
            let at = by_id
                .binary_search_by_key(&device.id, |(id, _)| *id)
                .ok()
                .and_then(|at| by_id.get(at))
                .map(|(_, at)| *at)
                .ok_or_else(|| invalid("route mask has no canonical physical device"))?;
            let mask = masks
                .get_mut(at)
                .ok_or_else(|| invalid("canonical device index is absent"))?;
            for (local_at, offer) in report.routes.iter().enumerate() {
                if local_mask & bit(local_at)? == 0 {
                    continue;
                }
                let global_at = offers
                    .iter()
                    .position(|global| global.key == offer.key)
                    .ok_or_else(|| invalid("local route is absent from normalized catalog"))?;
                *mask |= bit(global_at)?;
            }
        }
        for backing in report.backings {
            if let Some(previous) = backings
                .iter()
                .find(|previous| previous.key.pool == backing.key.pool)
            {
                if previous != &backing {
                    return Err(invalid(
                        "backing aliases disagree about generation, profile, phase or complete claims",
                    ));
                }
            } else {
                if backings.len() >= hex_api::MAX_RUNTIME_ROUTES {
                    return Err(invalid(
                        "combined backing catalog exceeds route address space",
                    ));
                }
                backings.push(backing);
            }
        }
    }
    let routes = RuntimeRoutes {
        offers,
        device_routes: masks,
        backings,
    };
    let provisioned = routes
        .unobserved_claims(&devices, node.id)
        .map_err(|_| invalid("canonical physical claim union exceeds its address space"))?;
    let gpus = devices
        .iter()
        .filter(|device| device.kind == hex_sim::model::DeviceKind::Gpu)
        .count()
        .checked_add(
            provisioned
                .iter()
                .filter(|id| id.kind == hex_api::DeviceKind::Gpu)
                .count(),
        )
        .and_then(|count| u32::try_from(count).ok())
        .ok_or_else(|| invalid("canonical GPU claim count overflows"))?;
    // Every route remains constrained by the host plane. Native HCS/OCI and CPU-only process
    // routes never inherit another backend's fixed guest ceiling or per-box VM cost.
    node.capacity = Resources { gpus, ..host };
    node.classes = routes.offers.iter().fold(Classes::NONE, |classes, offer| {
        let payload = class(offer.key.class);
        if facts.classes.has(payload) {
            classes.with(payload)
        } else {
            classes
        }
    });
    node.devices = devices;
    node.runtime_pools.clear();
    node.costs = [None; Class::COUNT];
    let routes = routes.share().map_err(|_| Refusal::Exhausted {
        what: "immutable initialized runtime routes",
        bound: 1,
    })?;
    routes
        .check(node)
        .map_err(|_| invalid("normalized initialized route topology is invalid"))?;
    node.routes = Some(routes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_api::{
        BackingKey, BackingOffer, BackingPhase, CapacityResources, DeviceId, DeviceInfo,
        DeviceKind, Execution, GpuEligibility, ImageArch, ImageOs, ImagePlatform, RouteKey,
        RouteOffer, RuntimeCost,
    };
    use hex_sim::facts::{Cpu, Facts, Gpu};
    use hex_sim::model::{GpuLoad, NodeId, Os, Virt};

    fn room() -> CapacityResources {
        CapacityResources {
            cpu_milli: 8_000,
            mem_bytes: 8 << 30,
            disk_bytes: 8 << 30,
        }
    }
    fn profile() -> RuntimeCost {
        let guest = CapacityResources {
            cpu_milli: 1_000,
            mem_bytes: 512 << 20,
            disk_bytes: 0,
        };
        RuntimeCost {
            id: [0x72; 32],
            floor: guest,
            ceiling: guest,
            quantum: CapacityResources {
                cpu_milli: 1_000,
                mem_bytes: 1,
                disk_bytes: 1,
            },
            guest_overhead: CapacityResources::default(),
            overhead: CapacityResources {
                cpu_milli: 100,
                mem_bytes: 64 << 20,
                disk_bytes: 0,
            },
        }
    }
    fn route(class: hex_api::Class, execution: Execution, gpu: GpuEligibility) -> RouteOffer {
        RouteOffer {
            key: RouteKey {
                class,
                execution,
                gpu,
                image: ImagePlatform {
                    os: ImageOs::Linux,
                    arch: ImageArch::Amd64,
                },
            },
            cost: (execution == Execution::Firecracker).then(profile),
            backing: None,
        }
    }
    fn report(
        routes: Vec<RouteOffer>,
        devices: Vec<DeviceInfo>,
        masks: Vec<u64>,
        backings: Vec<BackingOffer>,
    ) -> Capacity {
        Capacity {
            id: "same-native-host".into(),
            total: room(),
            free: room(),
            cost: None,
            routes,
            devices,
            device_routes: masks,
            backings,
        }
    }
    fn facts() -> Facts {
        Facts {
            os: Os::Linux,
            cpu: Cpu {
                threads: 8,
                ..Cpu::default()
            },
            classes: Classes::ALL,
            virt: Virt {
                kvm: true,
                ..Virt::default()
            },
            gpus: vec![Gpu {
                key: Some(1),
                model: "actual fixture GPU".into(),
                memory: 2 << 30,
                load: GpuLoad {
                    busy_per_mille: Some(0),
                    memory_used: Some(0),
                },
                ..Gpu::default()
            }],
            ..Facts::default()
        }
    }
    fn device() -> DeviceInfo {
        DeviceInfo {
            id: DeviceId {
                kind: DeviceKind::Gpu,
                key: 1,
            },
            model: "actual fixture GPU".into(),
            size: 2 << 30,
            holder: None,
        }
    }
    fn normalize(reports: Vec<(Class, Option<Capacity>)>) -> Result<NodeSpec, Refusal> {
        let facts = facts();
        let mut node = NodeSpec::new(NodeId(0), Resources::default());
        let mut cluster = Cluster::single_owner(Vec::new(), Vec::new());
        apply(
            reports,
            &facts,
            &mut cluster,
            "00112233445566778899aabbccddeeff",
            &mut node,
            Resources::default(),
        )?;
        Ok(node)
    }

    /// AC-T7.4.1: aliases reorder local bits, but one physical device and one host plane survive.
    #[test]
    fn service_route_normalization_remaps_alias_bits_and_refuses_conflicting_plane() {
        let native = route(hex_api::Class::Oci, Execution::Native, GpuEligibility::Any);
        let process = route(
            hex_api::Class::Process,
            Execution::Native,
            GpuEligibility::None,
        );
        let input = || {
            vec![
                (
                    Class::Oci,
                    Some(report(
                        vec![native, process],
                        vec![device()],
                        vec![1],
                        Vec::new(),
                    )),
                ),
                (
                    Class::Shards,
                    Some(report(
                        vec![process, native],
                        vec![device()],
                        vec![2],
                        Vec::new(),
                    )),
                ),
            ]
        };
        let node = normalize(input()).unwrap();
        assert_eq!(node.capacity.cpu_milli, room().cpu_milli);
        assert_eq!(node.capacity.mem_bytes, room().mem_bytes);
        assert_eq!(node.devices.len(), 1);
        assert_eq!(node.routes.as_ref().unwrap().device_routes, vec![1]);
        assert_eq!(node.routes.as_ref().unwrap().offers.len(), 2);
        assert!(node.costs.iter().all(Option::is_none));
        assert!(node.runtime_pools.is_empty());
        let mut bad = input();
        bad.get_mut(1).unwrap().1.as_mut().unwrap().id = "unrelated-guest-budget".into();
        assert!(normalize(bad).is_err());
    }

    /// AC-T7.4.1: an unobserved cold GPU is a reservable physical claim, never invented telemetry.
    #[test]
    fn service_cold_claim_union_has_no_fabricated_device_or_classwide_vm_cap() {
        let key = BackingKey {
            pool: [0x24; 32],
            generation: [0x25; 16],
        };
        let mut vm = route(
            hex_api::Class::Firecracker,
            Execution::Firecracker,
            GpuEligibility::Required,
        );
        vm.backing = Some(key);
        let backing = BackingOffer {
            key,
            cost: profile(),
            phase: BackingPhase::Prepared,
            devices: vec![DeviceId {
                kind: DeviceKind::Gpu,
                key: 2,
            }],
        };
        let node = normalize(vec![
            (
                Class::Oci,
                Some(report(
                    vec![route(
                        hex_api::Class::Oci,
                        Execution::Native,
                        GpuEligibility::Any,
                    )],
                    vec![device()],
                    vec![1],
                    Vec::new(),
                )),
            ),
            (
                Class::Firecracker,
                Some(report(vec![vm], Vec::new(), Vec::new(), vec![backing])),
            ),
        ])
        .unwrap();
        assert_eq!(node.capacity.gpus, 2);
        assert_eq!(node.devices.len(), 1);
        assert_eq!(node.devices.first().unwrap().key, Some(1));
        assert_eq!(node.capacity.mem_bytes, room().mem_bytes);
        assert_eq!(
            node.routes
                .as_ref()
                .unwrap()
                .unobserved_claims(&node.devices, node.id)
                .unwrap(),
            vec![DeviceId {
                kind: DeviceKind::Gpu,
                key: 2
            }]
        );
        hex_sim::index::FitIndex::new(&[node]).unwrap();
    }
}
