//! AC-T7.4.1: native attachment offers intersect measured hardware; telemetry alone grants nothing.

use super::*;
use hex_sim::model::{Classes, Cluster, Device, DeviceKind, GpuLoad, ModelId};

pub(crate) fn offers(
    reports: &[(Class, Option<Capacity>)],
    facts: &hex_sim::facts::Facts,
    cluster: &mut Cluster,
    namespace: &str,
) -> Result<Vec<Device>, Refusal> {
    // Cluster::model only appends. This exclusive call publishes either the entire offer's
    // interned names or none, without copying the cluster or maintaining another model table.
    let before = cluster.models.len();
    let result = collect_offers(reports, facts, cluster, namespace);
    if result.is_err() {
        cluster.models.truncate(before);
    }
    result
}

fn collect_offers(
    reports: &[(Class, Option<Capacity>)],
    facts: &hex_sim::facts::Facts,
    cluster: &mut Cluster,
    namespace: &str,
) -> Result<Vec<Device>, Refusal> {
    if reports.len() > Class::COUNT {
        return Err(invalid("more attachment reports than placement classes"));
    }
    let mut devices: Vec<Device> = Vec::new();
    // Sorted native identities index the same physical slots. The input class count and each
    // report's u16 address space bound this temporary; no persistent discovery cache is kept.
    let mut identities: Vec<(hex_api::DeviceId, usize, Option<&hex_api::BoxId>)> = Vec::new();
    let mut classes = Classes::NONE;
    let own_prefix = format!("hex-place-{namespace}-");
    for (class, report) in reports {
        if classes.has(*class) {
            return Err(invalid("duplicate attachment class report"));
        }
        classes = classes.with(*class);
        let Some(report) = report else {
            continue;
        };
        report.check()?;
        if !report.devices.is_empty() && !facts.classes.has(*class) {
            return Err(invalid(
                "device offers require an initialized native attachment adapter",
            ));
        }
        devices
            .try_reserve_exact(report.devices.len())
            .map_err(|_| Refusal::Exhausted {
                what: "runtime device facts",
                bound: report.devices.len(),
            })?;
        identities
            .try_reserve_exact(report.devices.len())
            .map_err(|_| Refusal::Exhausted {
                what: "runtime device identities",
                bound: report.devices.len(),
            })?;
        for offer in &report.devices {
            let kind = match offer.id.kind {
                hex_api::DeviceKind::Gpu => DeviceKind::Gpu,
                hex_api::DeviceKind::Vfio => DeviceKind::Vfio,
                hex_api::DeviceKind::Block => DeviceKind::Block,
                hex_api::DeviceKind::Net => DeviceKind::Net,
            };
            if !class.exposes(facts.os, kind) {
                return Err(invalid(
                    "runtime offers a device its native class cannot expose",
                ));
            }
            let (model, size, load) = if kind == DeviceKind::Gpu {
                let gpu = facts
                    .gpus
                    .iter()
                    .find(|gpu| native_key(gpu, facts.os) == Some(offer.id.key))
                    .ok_or_else(|| {
                        invalid("a native GPU offer has no matching hardware identity")
                    })?;
                if offer.size != gpu.memory
                    || hex_sim::model::model_name(&offer.model)
                        != hex_sim::model::model_name(&gpu.model)
                {
                    return Err(invalid(
                        "native GPU metadata disagrees with measured hardware",
                    ));
                }
                (
                    cluster
                        .model(&gpu.model)
                        .map_err(|error| invalid(error.to_string()))?,
                    gpu.memory,
                    gpu.load,
                )
            } else {
                let model = if offer.model.is_empty() {
                    ModelId::UNNAMED
                } else {
                    cluster
                        .model(&offer.model)
                        .map_err(|error| invalid(error.to_string()))?
                };
                (model, offer.size, GpuLoad::default())
            };
            match identities.binary_search_by_key(&offer.id, |entry| entry.0) {
                Ok(at) => {
                    let (_, index, holder) = identities
                        .get(at)
                        .ok_or_else(|| invalid("attachment identity index disagrees"))?;
                    let previous = devices
                        .get_mut(*index)
                        .ok_or_else(|| invalid("attachment device index disagrees"))?;
                    if previous.model != model
                        || previous.size != size
                        || *holder != offer.holder.as_ref()
                    {
                        return Err(invalid(
                            "backend aliases disagree on physical metadata or holder",
                        ));
                    }
                    previous.classes = previous.classes.with(*class);
                    continue;
                }
                Err(at) => {
                    if devices.len() >= hex_api::MAX_DEVICES {
                        return Err(invalid(
                            "combined physical devices exceed the scheduler index space",
                        ));
                    }
                    identities.insert(at, (offer.id, devices.len(), offer.holder.as_ref()));
                }
            }
            devices.push(Device {
                kind,
                key: Some(offer.id.key),
                available: offer
                    .holder
                    .as_ref()
                    .is_none_or(|holder| holder.as_str().starts_with(&own_prefix)),
                classes: Classes::NONE.with(*class),
                model,
                size,
                load,
            });
        }
    }
    Ok(devices)
}

fn native_key(gpu: &hex_sim::facts::Gpu, os: hex_sim::model::Os) -> Option<u64> {
    match gpu.pci.as_deref() {
        Some(text) => {
            let pci = hex_gpu::PciAddress::parse(text)?;
            let key = hex_gpu::Identity::Pci(pci).key();
            if gpu.key.is_some_and(|native| native != key) {
                None
            } else {
                Some(key)
            }
        }
        None if os == hex_sim::model::Os::Macos && gpu.vendor == "apple" => gpu.key,
        None => None,
    }
}
