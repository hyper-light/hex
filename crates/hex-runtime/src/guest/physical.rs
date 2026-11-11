//! Exact renderer/native correspondence. Every read uses Books' same initialized
//! provider; this module stores neither inventory nor holder state.

use super::*;
use crate::devices::{Binding, Route};

fn invalid(why: &'static str) -> Refusal {
    Refusal::Invalid {
        what: "Shards physical GPU correspondence",
        why: why.into(),
    }
}

fn key(physical: shards_api::PhysicalGpu) -> Result<u64, Refusal> {
    match physical {
        shards_api::PhysicalGpu::Pci {
            domain,
            bus,
            device,
            function,
        } => Ok((u64::from(domain) << 16)
            | (u64::from(bus) << 8)
            | (u64::from(device) << 3)
            | u64::from(function)),
        shards_api::PhysicalGpu::MetalRegistry(id) => Ok(id),
        shards_api::PhysicalGpu::Luid(_) => Err(invalid(
            "a current-boot LUID is not a stable physical DeviceId",
        )),
    }
}

fn joined_key(device: &shards_api::ResolvedGpu) -> Result<u64, Refusal> {
    match device.physical {
        shards_api::PhysicalGpu::Luid(luid) => {
            // No integer equality between DeviceId and LUID. Without complete
            // Vulkan PCI properties the provider's separate PNP/KMT join is owed.
            if device.luid != Some(luid.to_le_bytes()) {
                return Err(invalid("native LUID bytes changed"));
            }
            let Some(pci @ shards_api::PhysicalGpu::Pci { .. }) = device.pci else {
                return Err(invalid(
                    "live LUID has no verified complete PCI correspondence",
                ));
            };
            key(pci)
        }
        physical => key(physical),
    }
}

impl GuestRuntime {
    pub(super) fn catalog_physical(
        &self,
        devices: &[hex_api::DeviceId],
    ) -> Result<Vec<shards_api::PhysicalGpu>, Refusal> {
        let catalog = self
            .gpu_catalog
            .as_ref()
            .ok_or_else(|| invalid("no exact physical catalog"))?;
        let mut physical = Vec::new();
        physical
            .try_reserve_exact(devices.len())
            .map_err(|_| Refusal::Full(hex_api::MAX_DEVICES))?;
        for id in devices {
            let entry = catalog
                .entries()
                .iter()
                .find(|entry| entry.info.id == *id)
                .ok_or_else(|| invalid("claim is absent from the configured catalog"))?;
            let Some(Binding::ShardsGpu { physical: native }) = entry.binding(Route::ShardsGpu)
            else {
                return Err(invalid("claim has no configured Shards route"));
            };
            physical.push(*native);
        }
        shards_api::physical::check_selection(&physical)
            .map_err(|_| invalid("physical selectors alias or exceed their representation"))?;
        Ok(physical)
    }

    pub(super) fn checked_physical(
        &self,
        ids: &[hex_api::DeviceId],
        physical: &[shards_api::PhysicalGpu],
    ) -> Result<Vec<hex_api::DeviceInfo>, Refusal> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        if ids.len() != physical.len() || ids.len() > hex_api::MAX_DEVICES {
            return Err(invalid("physical claim count changed"));
        }
        let authority = self.authority()?;
        let records = authority.records()?;
        let mut native = authority.physical_devices()?;
        if native.len() > hex_api::MAX_DEVICES {
            return Err(Refusal::Full(hex_api::MAX_DEVICES));
        }
        native.sort_unstable_by_key(|info| info.id);
        if native.windows(2).any(|pair| {
            pair.first()
                .zip(pair.get(1))
                .is_some_and(|(a, b)| a.id == b.id)
        }) {
            return Err(invalid("native provider repeated a physical DeviceId"));
        }
        let mut spec = self.config.spec();
        spec.gpu = true;
        spec.gpu_devices
            .try_reserve_exact(physical.len())
            .map_err(|_| Refusal::Full(hex_api::MAX_DEVICES))?;
        spec.gpu_devices.extend_from_slice(physical);
        let probe = self
            .guests
            .probe_gpu(&spec, self.first_dial_bound()?)
            .map_err(|why| refusal("initializing exact renderer GPU correspondence", &why))?;
        probe.check(physical).map_err(|why| {
            refusal(
                "checking exact renderer GPU correspondence",
                &shards_api::Refusal::GpuProbe(why),
            )
        })?;
        let catalog = self
            .gpu_catalog
            .as_ref()
            .ok_or_else(|| invalid("physical catalog disappeared"))?;
        let mut offers = Vec::new();
        offers
            .try_reserve_exact(ids.len())
            .map_err(|_| Refusal::Full(hex_api::MAX_DEVICES))?;
        for (id, resolved) in ids.iter().zip(&probe.devices) {
            if id.kind != hex_api::DeviceKind::Gpu || joined_key(resolved)? != id.key {
                return Err(invalid(
                    "complete native identity disagrees with the stable physical claim",
                ));
            }
            let index = native
                .binary_search_by_key(id, |info| info.id)
                .map_err(|_| invalid("the same native provider has no current selected device"))?;
            let info = native
                .get_mut(index)
                .ok_or_else(|| invalid("native device index changed"))?;
            info.check()?;
            if info.holder.is_some() {
                return Err(invalid("native facts cannot assert a holder"));
            }
            // Move actual model/size without an unchecked clone. Only the sole
            // durable Store supplies holders; catalog metadata is never capacity.
            let mut info = std::mem::replace(
                info,
                hex_api::DeviceInfo {
                    id: *id,
                    model: String::new(),
                    size: 0,
                    holder: None,
                },
            );
            info.holder = catalog
                .holder(*id, &records)?
                .map(BoxId::try_clone)
                .transpose()?;
            offers.push(info);
        }
        Ok(offers)
    }

    pub fn attachment_offers(&self) -> Result<Vec<hex_api::DeviceInfo>, Refusal> {
        let Some(catalog) = &self.gpu_catalog else {
            return Ok(Vec::new());
        };
        let mut ids = Vec::new();
        for entry in catalog.entries() {
            if entry.binding(Route::ShardsGpu).is_some() {
                if ids.len() >= hex_api::MAX_DEVICES {
                    return Err(Refusal::Full(hex_api::MAX_DEVICES));
                }
                ids.try_reserve(1)
                    .map_err(|_| Refusal::Full(hex_api::MAX_DEVICES))?;
                ids.push(entry.info.id);
            }
        }
        ids.sort_unstable();
        let physical = self.catalog_physical(&ids)?;
        self.checked_physical(&ids, &physical)
    }

    /// Explicit initialization uses the same production join as later admission.
    pub fn initialize_gpu(&self) -> Result<(), Refusal> {
        self.attachment_offers().map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC-T7.4.1: current-boot LUID bytes never become a stable integer identity;
    /// missing PCI domain or stale LUID correspondence remains unknown/refused.
    #[test]
    fn luid_requires_complete_current_native_correspondence() {
        let pci = shards_api::PhysicalGpu::Pci {
            domain: 7,
            bus: 2,
            device: 1,
            function: 3,
        };
        let mut resolved = shards_api::ResolvedGpu {
            physical: shards_api::PhysicalGpu::Luid(9),
            uuid: [1; 16],
            pci: None,
            luid: Some(9u64.to_le_bytes()),
        };
        assert!(joined_key(&resolved).is_err());
        resolved.pci = Some(pci);
        assert_eq!(
            joined_key(&resolved).unwrap(),
            (7 << 16) | (2 << 8) | (1 << 3) | 3
        );
        resolved.luid = Some(10u64.to_le_bytes());
        assert!(joined_key(&resolved).is_err());
        assert!(key(shards_api::PhysicalGpu::Luid(9)).is_err());
    }
}
