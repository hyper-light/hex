//! AC-T7.4.1: native attachment offers intersect measured hardware; telemetry alone grants nothing.

use super::*;
use hex_sim::model::{Cluster, Device, DeviceKind, GpuLoad, ModelId};

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
    let mut devices = Vec::new();
    let own_prefix = format!("hex-place-{namespace}-");
    for (class, report) in reports {
        let Some(report) = report else {
            continue;
        };
        report.check()?;
        if !report.devices.is_empty() && *class != Class::Oci {
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
                    .find(|gpu| {
                        gpu.pci
                            .as_deref()
                            .and_then(hex_gpu::PciAddress::parse)
                            .is_some_and(|pci| {
                                let key = (u64::from(pci.domain) << 16)
                                    | (u64::from(pci.bus) << 8)
                                    | (u64::from(pci.device) << 3)
                                    | u64::from(pci.function);
                                key == offer.id.key
                            })
                    })
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
            devices.push(Device {
                kind,
                key: Some(offer.id.key),
                available: offer
                    .holder
                    .as_ref()
                    .is_none_or(|holder| holder.as_str().starts_with(&own_prefix)),
                model,
                size,
                load,
            });
        }
    }
    Ok(devices)
}
