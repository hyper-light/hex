//! Physical identities carried from a runtime's catalog to a box's exact claims (AC-T7.3.1).
//! These values describe and request devices; they confer no attachment authority by themselves.

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{BoxId, Refusal};

/// One entry for every `u16` device index, as in the scheduler's existing device address space.
pub const MAX_DEVICES: usize = 1 << u16::BITS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    Gpu,
    /// An IOMMU isolation group, not an independently assignable PCI function.
    Vfio,
    Block,
    Net,
}

impl DeviceKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Gpu => "gpu",
            Self::Vfio => "vfio",
            Self::Block => "block",
            Self::Net => "net",
        }
    }
}

impl<'de> Deserialize<'de> for DeviceKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Kind;
        impl de::Visitor<'_> for Kind {
            type Value = DeviceKind;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a lowercase device kind")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<DeviceKind, E> {
                match value {
                    "gpu" => Ok(DeviceKind::Gpu),
                    "vfio" => Ok(DeviceKind::Vfio),
                    "block" => Ok(DeviceKind::Block),
                    "net" => Ok(DeviceKind::Net),
                    _ => Err(E::custom("device.kind is unsupported")),
                }
            }
        }
        deserializer.deserialize_str(Kind)
    }
}

/// A physical identity within its runtime pool. A GPU key is the native unsigned 64-bit
/// identity: packed PCI domain:32/bus:8/device:5/function:3 on PCI hosts, Metal registry ID on
/// Apple silicon. Its runtime validates that native correspondence without narrowing. VFIO
/// keys are `u32` IOMMU group numbers; block keys pack
/// major:32/minor:32; network keys are nonzero `u32` interface indices. Pool identity remains
/// necessary: a key from one node or guest never names another pool's device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceId {
    pub kind: DeviceKind,
    pub key: u64,
}

impl DeviceId {
    pub fn check(&self) -> Result<(), Refusal> {
        let why = match self.kind {
            DeviceKind::Vfio if u32::try_from(self.key).is_err() => {
                Some("vfio key exceeds the 32-bit isolation group")
            }
            DeviceKind::Net if self.key == 0 || u32::try_from(self.key).is_err() => {
                Some("net key is not a nonzero 32-bit interface index")
            }
            _ => None,
        };
        match why {
            Some(why) => Err(Refusal::Invalid {
                what: "device id",
                why: why.to_owned(),
            }),
            None => Ok(()),
        }
    }
}

/// A catalog fact: opaque model metadata, size in bytes where applicable, and its current holder.
/// Neither free metadata nor a reported holder substitutes for runtime admission or attachment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceInfo {
    pub id: DeviceId,
    pub model: String,
    pub size: u64,
    pub holder: Option<BoxId>,
}

impl DeviceInfo {
    pub fn check(&self) -> Result<(), Refusal> {
        self.id.check()
    }
}

pub(crate) fn check_ids(ids: impl ExactSizeIterator<Item = DeviceId>) -> Result<(), Refusal> {
    let count = ids.len();
    if count > MAX_DEVICES {
        return Err(Refusal::Invalid {
            what: "devices",
            why: "count exceeds the u16 device index address space".to_owned(),
        });
    }
    // A fallible, count-bounded copy keeps duplicate validation O(n log n), without changing the
    // catalog order that the scheduler's indices refer to. sort_unstable needs no further buffer.
    let mut sorted = Vec::new();
    sorted
        .try_reserve_exact(count)
        .map_err(|_| Refusal::Exhausted {
            what: "device identities",
            bound: count,
        })?;
    for id in ids {
        id.check()?;
        sorted.push(id);
    }
    sorted.sort_unstable();
    let mut previous = None;
    for id in sorted {
        if previous == Some(id) {
            return Err(Refusal::Invalid {
                what: "devices",
                why: "duplicate device identity".to_owned(),
            });
        }
        previous = Some(id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bounds, BoxSpec, Capacity, CapacityResources, ClassSpec, Program, Runtime, Stdio};

    fn id(kind: DeviceKind, key: u64) -> DeviceId {
        DeviceId { kind, key }
    }

    fn spec(devices: Vec<DeviceId>) -> BoxSpec {
        BoxSpec {
            id: BoxId::new("device-contract").unwrap(),
            bounds: Bounds::default(),
            class: ClassSpec::Process,
            owner: None,
            devices,
            allocation: None,
        }
    }

    fn capacity(devices: Vec<DeviceInfo>) -> Capacity {
        Capacity {
            id: "device-pool".to_owned(),
            total: CapacityResources::default(),
            free: CapacityResources::default(),
            devices,
            cost: None,
        }
    }

    fn info(id: DeviceId) -> DeviceInfo {
        DeviceInfo {
            id,
            model: "synthetic-private-model".to_owned(),
            size: u64::MAX,
            holder: Some(BoxId::new("synthetic-private-holder").unwrap()),
        }
    }

    /// AC-T7.3.1: physical identity bounds, including all major/minor bits and VFIO group zero.
    #[test]
    fn physical_key_domains_are_checked_without_truncation() {
        for device in [
            id(DeviceKind::Gpu, 0),
            // Metal registry identities are u64; PCI-only narrowing loses valid native IDs.
            id(DeviceKind::Gpu, u64::MAX),
            id(DeviceKind::Gpu, (u64::from(u32::MAX) << 16) | 0xffff),
            id(DeviceKind::Vfio, 0),
            id(DeviceKind::Vfio, u64::from(u32::MAX)),
            id(DeviceKind::Block, 0),
            id(DeviceKind::Block, u64::MAX),
            id(DeviceKind::Net, 1),
            id(DeviceKind::Net, u64::from(u32::MAX)),
        ] {
            assert_eq!(device.check(), Ok(()));
            assert_eq!(info(device).check(), Ok(()));
            assert_eq!(spec(vec![device]).check(), Ok(()));
            assert_eq!(capacity(vec![info(device)]).check(), Ok(()));
        }
        for device in [
            id(DeviceKind::Vfio, 1 << 32),
            id(DeviceKind::Net, 0),
            id(DeviceKind::Net, 1 << 32),
        ] {
            for error in [
                device.check().unwrap_err(),
                info(device).check().unwrap_err(),
                spec(vec![device]).check().unwrap_err(),
                capacity(vec![info(device)]).check().unwrap_err(),
            ] {
                assert!(matches!(
                    error,
                    Refusal::Invalid {
                        what: "device id",
                        ..
                    }
                ));
                let text = error.to_string();
                assert!(text.contains(device.kind.name()));
                assert!(!text.contains(&device.key.to_string()));
                assert!(!text.contains("synthetic-private"));
            }
        }
    }

    /// AC-T7.3.1: every existing u16 index remains usable; duplicates cannot mint another offer.
    #[test]
    fn claims_and_offers_check_the_full_index_space_and_duplicate_identity() {
        let devices: Vec<_> = (0..MAX_DEVICES)
            .map(|key| id(DeviceKind::Vfio, u64::try_from(key).unwrap()))
            .collect();
        assert_eq!(spec(devices.clone()).check(), Ok(()));
        assert_eq!(
            capacity(devices.iter().copied().map(info).collect()).check(),
            Ok(())
        );
        let mut oversized = devices.clone();
        oversized.push(id(DeviceKind::Vfio, MAX_DEVICES as u64));
        assert!(matches!(
            spec(oversized.clone()).check(),
            Err(Refusal::Invalid {
                what: "devices",
                ..
            })
        ));
        assert!(matches!(
            capacity(oversized.into_iter().map(info).collect()).check(),
            Err(Refusal::Invalid {
                what: "devices",
                ..
            })
        ));
        let duplicate = vec![devices[0], devices[1], devices[0]];
        assert!(
            matches!(spec(duplicate.clone()).check(), Err(Refusal::Invalid { what: "devices", ref why }) if why.contains("duplicate"))
        );
        assert!(
            matches!(capacity(duplicate.into_iter().map(info).collect()).check(), Err(Refusal::Invalid { what: "devices", ref why }) if why.contains("duplicate"))
        );
        // Equal numeric keys of distinct physical kinds remain distinct.
        assert_eq!(
            spec(vec![id(DeviceKind::Gpu, 1), id(DeviceKind::Net, 1)]).check(),
            Ok(())
        );
    }

    /// AC-T7.3.1: empty claims/offers preserve old JSON; kind names and populated fields round-trip.
    #[test]
    fn device_json_preserves_legacy_empty_fields_and_lowercase_kinds() {
        let old_spec = r#"{"id":"device-contract","bounds":{"cpu_weight":null,"cpu_max_milli":null,"memory_max":null,"memory_high":null,"io_weight":null,"io_latency_us":null,"io_max":null,"pids_max":null},"class":"process"}"#;
        let old_capacity = r#"{"id":"device-pool","total":{"cpu_milli":0,"mem_bytes":0,"disk_bytes":0},"free":{"cpu_milli":0,"mem_bytes":0,"disk_bytes":0}}"#;
        let legacy: BoxSpec = serde_json::from_str(old_spec).unwrap();
        assert_eq!(legacy, spec(Vec::new()));
        assert_eq!(serde_json::to_string(&legacy).unwrap(), old_spec);
        let legacy: Capacity = serde_json::from_str(old_capacity).unwrap();
        assert_eq!(legacy, capacity(Vec::new()));
        assert_eq!(serde_json::to_string(&legacy).unwrap(), old_capacity);
        for kind in [
            DeviceKind::Gpu,
            DeviceKind::Vfio,
            DeviceKind::Block,
            DeviceKind::Net,
        ] {
            assert_eq!(serde_json::to_value(kind).unwrap(), kind.name());
            let device = info(id(kind, 1));
            let json = serde_json::to_string(&device).unwrap();
            assert_eq!(serde_json::from_str::<DeviceInfo>(&json).unwrap(), device);
            let claimed = spec(vec![device.id]);
            assert_eq!(
                serde_json::from_str::<BoxSpec>(&serde_json::to_string(&claimed).unwrap()).unwrap(),
                claimed
            );
            let offered = capacity(vec![device]);
            assert_eq!(
                serde_json::from_str::<Capacity>(&serde_json::to_string(&offered).unwrap())
                    .unwrap(),
                offered
            );
        }
        let error = serde_json::from_str::<DeviceKind>(r#""synthetic-private-unknown-kind""#)
            .unwrap_err()
            .to_string();
        assert!(error.contains("device.kind"));
        assert!(!error.contains("synthetic-private"));
    }

    /// AC-T7.3.1: a schema capability must not become simulated physical attachment or a box.
    #[test]
    fn an_unsupported_backend_refuses_claims_before_effects_and_still_cleans_up() {
        let runtime = crate::sim::SimRuntime::new();
        let claimed = spec(vec![id(DeviceKind::Gpu, 1)]);
        assert!(matches!(
            claimed.check_no_devices("test"),
            Err(Refusal::Platform {
                what: "devices",
                platform: "test",
                ..
            })
        ));
        assert!(matches!(
            runtime.create(claimed.clone()),
            Err(Refusal::Platform {
                what: "devices",
                platform: "sim",
                ..
            })
        ));
        assert!(matches!(
            runtime.run(
                claimed.clone(),
                Program::of(&["say", "never"]),
                Stdio::default()
            ),
            Err(Refusal::Platform {
                what: "devices",
                platform: "sim",
                ..
            })
        ));
        assert!(runtime.list().unwrap().is_empty());
        let invalid = spec(vec![id(DeviceKind::Net, 0)]);
        assert!(matches!(
            invalid.check_no_devices("test"),
            Err(Refusal::Invalid {
                what: "device id",
                ..
            })
        ));
        let legitimate = spec(Vec::new());
        runtime.create(legitimate.clone()).unwrap();
        assert_eq!(runtime.list().unwrap().len(), 1);
        runtime.delete(&legitimate.id).unwrap();
        assert!(runtime.list().unwrap().is_empty());
    }
}
