//! Native prerequisites for exact prebound PCI functions and IOMMU groups.
//! No driver mutation, module loading or host configuration occurs here.

use super::native::{path, text};
use super::{config::Passthrough, exhausted, invalid};
use crate::devices::{Binding, Catalog, Entry, Route};
use hex_api::{DeviceId, DeviceKind, Refusal};
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::path::Path;

fn pci(sbdf: &str) -> Result<hex_gpu::PciAddress, Refusal> {
    let pci =
        hex_gpu::PciAddress::parse(sbdf).ok_or_else(|| invalid("invalid native PCI address"))?;
    if pci.domain > u32::from(u16::MAX) {
        return Err(invalid("PCI function must use the canonical native SBDF"));
    }
    // Linux SBDF is domain16/bus8/device5/function3 with these fixed separators.
    let mut canonical = String::new();
    canonical
        .try_reserve_exact("ffff:ff:1f.7".len())
        .map_err(|_| exhausted("native PCI address", "ffff:ff:1f.7".len()))?;
    write!(&mut canonical, "{pci}").map_err(|_| invalid("native PCI address cannot be encoded"))?;
    if canonical != sbdf {
        return Err(invalid("PCI function must use the canonical native SBDF"));
    }
    Ok(pci)
}

fn group_name(group: u32) -> Result<String, Refusal> {
    // IOMMU group numbers are u32; ten decimal digits cover every representable group.
    let mut name = String::new();
    name.try_reserve_exact(10)
        .map_err(|_| exhausted("IOMMU group name", 10))?;
    write!(&mut name, "{group}").map_err(|_| invalid("IOMMU group cannot be encoded"))?;
    Ok(name)
}

fn reserve<T>(items: &mut Vec<T>, more: usize) -> Result<(), Refusal> {
    if items
        .len()
        .checked_add(more)
        .is_none_or(|n| n > hex_api::MAX_DEVICES)
    {
        return Err(exhausted("VFIO function metadata", hex_api::MAX_DEVICES));
    }
    items
        .try_reserve(more)
        .map_err(|_| exhausted("VFIO function metadata", hex_api::MAX_DEVICES))
}

/// The RLIMIT_MEMLOCK a VMM that maps `needed` bytes of its guest's RAM for DMA runs under: vfio's type1 IOMMU charges
/// every page it pins to the VMM's RLIMIT_MEMLOCK unless the VMM holds CAP_IPC_LOCK
/// (drivers/vfio/vfio_iommu_type1.c, `vfio_pin_pages_remote`), and Firecracker maps all of a VM's RAM once a device is
/// given it whole. `Ok(None)`: the VMM runs under the node's own limit, which holds it, or holds the capability.
/// `Ok(Some(limit))`: its soft limit raised to its hard one, which holds the RAM where the soft doesn't. Refused by
/// type where neither holds it and the VMM won't hold CAP_IPC_LOCK (`ipc_lock`), naming the bytes, both limits and the
/// capability, so the create is refused before anything is launched rather than the VMM ending on its DMA map's ENOMEM
/// (the lead, 2026-11-11, from the GPU run r690's `IommuDmaMap(Error(12))`).
pub(super) fn dma_memlock(
    needed: u64,
    limit: rustix::process::Rlimit,
    ipc_lock: bool,
) -> Result<Option<rustix::process::Rlimit>, Refusal> {
    let holds = |bound: Option<u64>| bound.is_none_or(|bound| bound >= needed);
    if ipc_lock || holds(limit.current) {
        return Ok(None);
    }
    if holds(limit.maximum) {
        return Ok(Some(rustix::process::Rlimit {
            current: limit.maximum,
            maximum: limit.maximum,
        }));
    }
    let shown =
        |bound: Option<u64>| bound.map_or_else(|| "unlimited".to_owned(), |b| b.to_string());
    Err(Refusal::Platform {
        what: "a VM given a device whole (VFIO)",
        platform: "linux",
        why: format!(
            "its VMM pins all {needed} bytes of its guest's RAM for DMA, past RLIMIT_MEMLOCK: soft {}, hard {}, and \
             its VMM won't hold CAP_IPC_LOCK; give the node a memlock hard limit of {needed} bytes or more \
             (LimitMEMLOCK=), or CAP_IPC_LOCK",
            shown(limit.current),
            shown(limit.maximum)
        ),
    })
}

/// Whether a VMM this process starts holds CAP_IPC_LOCK where vfio looks for it, the initial user namespace's
/// (`capable(CAP_IPC_LOCK)`): a process of the initial user namespace whose child keeps it across its execve, a root
/// one's from its bounding set (capabilities(7), the root exemption), any other's from its ambient set alone.
pub(super) fn vmm_holds_ipc_lock() -> Result<bool, Refusal> {
    use rustix::thread::CapabilitySet;
    let map = fs::read_to_string("/proc/self/uid_map")
        .map_err(|e| Refusal::os("reading this process's user namespace map", e))?;
    let initial = map.split_whitespace().collect::<Vec<_>>() == ["0", "0", "4294967295"];
    if !initial {
        return Ok(false);
    }
    let held = if rustix::process::geteuid().is_root() {
        rustix::thread::capability_is_in_bounding_set(CapabilitySet::IPC_LOCK)
    } else {
        rustix::thread::capability_is_in_ambient_set(CapabilitySet::IPC_LOCK)
    };
    held.map_err(|e| Refusal::os("reading this process's CAP_IPC_LOCK", e))
}

pub(super) fn plan(catalog: &Catalog, ids: &[DeviceId]) -> Result<Vec<Passthrough>, Refusal> {
    let mut selected = Vec::new();
    reserve(&mut selected, ids.len())?;
    selected.extend_from_slice(ids);
    selected.sort_unstable();
    let mut indexed = Vec::new();
    reserve(&mut indexed, catalog.entries().len())?;
    indexed.extend(catalog.entries().iter());
    indexed.sort_unstable_by_key(|entry| entry.info.id);
    let mut groups = Vec::new();
    reserve(&mut groups, selected.len())?;
    for id in &selected {
        let at = indexed
            .binary_search_by_key(id, |entry| entry.info.id)
            .map_err(|_| invalid("selected physical identity has no VFIO binding"))?;
        let entry = indexed
            .get(at)
            .ok_or_else(|| invalid("physical identity index disagrees"))?;
        validate(entry)?;
        let Some(Binding::Vfio { group, .. }) = entry.binding(Route::Vfio) else {
            return Err(invalid("selected native binding is not VFIO"));
        };
        groups.push(*group);
    }
    groups.sort_unstable();
    groups.dedup();
    // Collect and sort transient administrator authority once. Companion declarations never
    // supply the independent GPU selection; that is checked against the actual function key.
    let mut functions = Vec::new();
    for entry in &indexed {
        let Some(Binding::Vfio {
            sbdf,
            group,
            companions,
        }) = entry.binding(Route::Vfio)
        else {
            continue;
        };
        if groups.binary_search(group).is_err() {
            continue;
        }
        for name in std::iter::once(sbdf).chain(companions) {
            reserve(&mut functions, 1)?;
            functions.push((
                *group,
                hex_gpu::Identity::Pci(pci(name)?).key(),
                name.as_str(),
            ));
        }
    }
    functions.sort_unstable_by_key(|function| (function.0, function.1));
    functions.dedup_by_key(|function| (function.0, function.1));
    let mut devices = Vec::new();
    for group in groups {
        let group_path = path(Path::new("/sys/kernel/iommu_groups"), &group_name(group)?)?;
        let group_path = path(&group_path, "devices")?;
        let mut count = 0usize;
        for member in
            fs::read_dir(&group_path).map_err(|e| Refusal::os("reading VFIO isolation group", e))?
        {
            if count >= hex_api::MAX_DEVICES {
                return Err(exhausted("VFIO group functions", hex_api::MAX_DEVICES));
            }
            count = count
                .checked_add(1)
                .ok_or_else(|| invalid("VFIO function count overflow"))?;
            let member = member.map_err(|e| Refusal::os("reading VFIO group member", e))?;
            let name = member.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| invalid("native VFIO function is not UTF-8"))?;
            let key = hex_gpu::Identity::Pci(pci(name)?).key();
            if functions
                .binary_search_by_key(&(group, key), |function| (function.0, function.1))
                .is_err()
            {
                return Err(invalid(
                    "IOMMU group includes an unauthorized physical function",
                ));
            }
            let class = validate_function(name, group)?;
            authorize_function(&indexed, &selected, group, key, class)?;
            reserve(&mut devices, 1)?;
            devices.push(Passthrough {
                id: function_id(name)?,
                sbdf: text(name)?,
                graphics: graphics(class),
            });
        }
        if count == 0 {
            return Err(invalid(
                "selected isolation group has no physical functions",
            ));
        }
    }
    devices.sort_unstable_by(|a, b| a.sbdf.cmp(&b.sbdf));
    // A hotplug/group change during the finite snapshot must not omit a selected primary while
    // leaving only one of its companions in an otherwise nonempty passthrough group.
    for id in &selected {
        let entry = indexed
            .binary_search_by_key(id, |entry| entry.info.id)
            .ok()
            .and_then(|index| indexed.get(index))
            .ok_or_else(|| invalid("selected identity disappeared from its index"))?;
        let Some(Binding::Vfio { sbdf, .. }) = entry.binding(Route::Vfio) else {
            return Err(invalid("selected identity lost its VFIO route"));
        };
        if devices
            .binary_search_by(|device| device.sbdf.as_str().cmp(sbdf))
            .is_err()
        {
            return Err(invalid(
                "selected physical function disappeared from its IOMMU group",
            ));
        }
    }
    Ok(devices)
}

fn graphics(class: u32) -> bool {
    // Linux PCI_BASE_CLASS_DISPLAY=0x03 and PROCESSING_ACCELERATOR=0x12; GPU compute functions
    // need independent authority even if an administrator lists them as another GPU's companion.
    matches!(class >> 16, 0x03 | 0x12)
}

fn authorize_function(
    indexed: &[&Entry],
    selected: &[DeviceId],
    group: u32,
    key: u64,
    class: u32,
) -> Result<(), Refusal> {
    if !graphics(class)
        || selected
            .binary_search(&DeviceId {
                kind: DeviceKind::Vfio,
                key: u64::from(group),
            })
            .is_ok()
    {
        return Ok(());
    }
    let id = DeviceId {
        kind: DeviceKind::Gpu,
        key,
    };
    let at = indexed
        .binary_search_by_key(&id, |entry| entry.info.id)
        .map_err(|_| invalid("actual GPU function is not independently catalogued"))?;
    let entry = indexed
        .get(at)
        .ok_or_else(|| invalid("GPU authority index disagrees"))?;
    if selected.binary_search(&id).is_err()
        || !matches!(entry.binding(Route::Vfio), Some(Binding::Vfio { group: configured, .. }) if *configured == group)
    {
        return Err(invalid(
            "IOMMU group includes an independently unselected GPU",
        ));
    }
    Ok(())
}

pub(super) fn validate(entry: &Entry) -> Result<(), Refusal> {
    let Some(Binding::Vfio { sbdf, group, .. }) = entry.binding(Route::Vfio) else {
        return Err(invalid("Firecracker requires VFIO native bindings"));
    };
    let key = hex_gpu::Identity::Pci(pci(sbdf)?).key();
    if (entry.info.id.kind == DeviceKind::Gpu && key != entry.info.id.key)
        || (entry.info.id.kind == DeviceKind::Vfio && entry.info.id.key != u64::from(*group))
    {
        return Err(invalid(
            "VFIO binding differs from its selected physical identity",
        ));
    }
    let class = validate_function(sbdf, *group)?;
    if entry.info.id.kind == DeviceKind::Gpu && !graphics(class) {
        return Err(invalid(
            "selected native function is not a GPU/compute accelerator",
        ));
    }
    let group_name = group_name(*group)?;
    for interface in ["vfio", group_name.as_str()] {
        let _access = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path(Path::new("/dev/vfio"), interface)?)
            .map_err(|e| Refusal::os("opening selected VFIO interface", e))?;
    }
    Ok(())
}

fn validate_function(sbdf: &str, group: u32) -> Result<u32, Refusal> {
    pci(sbdf)?;
    let device_path = path(Path::new("/sys/bus/pci/devices"), sbdf)?;
    let native_group = fs::canonicalize(path(&device_path, "iommu_group")?)
        .map_err(|e| Refusal::os("resolving native IOMMU group", e))?;
    if native_group
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.parse::<u32>().ok())
        != Some(group)
    {
        return Err(invalid(
            "physical function moved to a different isolation group",
        ));
    }
    let driver = fs::canonicalize(path(&device_path, "driver")?)
        .map_err(|e| Refusal::os("resolving native PCI driver", e))?;
    if driver.file_name().is_none_or(|name| name != "vfio-pci") {
        return Err(invalid("selected function is not prebound to vfio-pci"));
    }
    // PCI class is 24 bits, encoded by sysfs as 0x + six hex digits + newline. One excess
    // byte distinguishes an oversized/changed object; no unbounded string allocation occurs.
    let mut bytes = [0u8; b"0xffffff\n".len() + 1];
    let mut file = fs::File::open(path(&device_path, "class")?)
        .map_err(|e| Refusal::os("reading native PCI class", e))?;
    let mut used = 0usize;
    while used < bytes.len() {
        let count = file
            .read(
                bytes
                    .get_mut(used..)
                    .ok_or_else(|| invalid("PCI class byte count overflow"))?,
            )
            .map_err(|e| Refusal::os("reading native PCI class", e))?;
        if count == 0 {
            break;
        }
        used = used
            .checked_add(count)
            .ok_or_else(|| invalid("PCI class byte count overflow"))?;
    }
    if used >= bytes.len() {
        return Err(invalid("native PCI class exceeds its protocol width"));
    }
    let text = std::str::from_utf8(
        bytes
            .get(..used)
            .ok_or_else(|| invalid("PCI class byte count overflow"))?,
    )
    .map_err(|_| invalid("PCI class is not ASCII"))?;
    text.trim()
        .strip_prefix("0x")
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .filter(|value| *value <= 0xffffff)
        .ok_or_else(|| invalid("malformed native PCI class"))
}

pub(super) fn function_id(sbdf: &str) -> Result<String, Refusal> {
    let key = hex_gpu::Identity::Pci(pci(sbdf)?).key();
    // Prefix plus at most sixteen hexadecimal digits for the existing u64 physical key.
    let bound = "pci_"
        .len()
        .checked_add(16)
        .ok_or_else(|| invalid("VFIO identifier bound overflows"))?;
    let mut id = String::new();
    id.try_reserve_exact(bound)
        .map_err(|_| exhausted("VFIO function identifier", bound))?;
    write!(&mut id, "pci_{key:x}")
        .map_err(|_| invalid("VFIO function identifier cannot be encoded"))?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lead's ruling of 2026-11-11 (AC-T7.4.1; the GPU run r690's step a: RLIMIT_MEMLOCK 524,288 KiB against a VM of
    /// 1,536 MiB, `IommuDmaMap(Error(12))`): the RLIMIT_MEMLOCK a VMM that pins its guest's RAM for DMA runs under. Do:
    /// ask for r690's VM under its limit, with and without CAP_IPC_LOCK; under a soft limit short of the RAM and a hard
    /// one that holds it, or none; under a soft limit that holds it, or none. Expect r690's refused by type, naming
    /// the RAM in bytes, both limits and CAP_IPC_LOCK, and with the capability left as the node's; the short soft limit
    /// raised to its hard one; one that holds it left as the node's.
    #[test]
    fn a_vmm_pinning_its_ram_runs_under_a_memlock_that_holds_it_or_is_refused_naming_it() {
        use rustix::process::Rlimit;
        let ram = 1_536u64 << 20;
        let at = |soft: Option<u64>, hard: Option<u64>| Rlimit {
            current: soft,
            maximum: hard,
        };
        let r690 = at(Some(524_288 << 10), Some(524_288 << 10));
        let Err(Refusal::Platform { why, .. }) = dma_memlock(ram, r690, false) else {
            panic!("r690's VM refused before its launch")
        };
        for named in [
            "1610612736 bytes",
            "soft 536870912",
            "hard 536870912",
            "CAP_IPC_LOCK",
        ] {
            assert!(why.contains(named), "{named}: {why}");
        }
        assert_eq!(dma_memlock(ram, r690, true), Ok(None));
        let short = Some(8 << 20);
        for hard in [Some(ram), Some(ram + 1), None] {
            assert_eq!(
                dma_memlock(ram, at(short, hard), false),
                Ok(Some(at(hard, hard))),
                "{hard:?}"
            );
        }
        for soft in [Some(ram), None] {
            assert_eq!(dma_memlock(ram, at(soft, soft), false), Ok(None));
        }
    }

    /// AC-T7.4.1: native address and VMM witness identifiers retain exact canonical physical bits.
    #[test]
    fn native_function_names_keep_canonical_full_physical_identity() {
        assert_eq!(function_id("0000:01:00.0").unwrap(), "pci_100");
        assert_eq!(function_id("ffff:ff:1f.7").unwrap(), "pci_ffffffff");
        assert_eq!(group_name(u32::MAX).unwrap(), "4294967295");
        for invalid in [
            "0:01:00.0",
            "FFFF:ff:1f.7",
            "10000:00:00.0",
            "0000:00:20.0",
            "0000:00:00.8",
        ] {
            assert!(
                function_id(invalid).is_err(),
                "accepted noncanonical {invalid}"
            );
        }
    }

    #[test]
    fn companion_declaration_cannot_authorize_a_second_physical_gpu() {
        let catalog = Catalog::parse(
            b"hex-devices-1\ngpu\t256\tfixture\t4096\tvfio\t0000:01:00.0,0000:02:00.0\t7\n",
        )
        .unwrap();
        let indexed = catalog.entries().iter().collect::<Vec<_>>();
        let selected = [DeviceId {
            kind: DeviceKind::Gpu,
            key: 256,
        }];
        assert!(authorize_function(&indexed, &selected, 7, 512, 0x030000).is_err());
        assert!(authorize_function(&indexed, &selected, 7, 512, 0x120000).is_err());
        assert!(authorize_function(&indexed, &selected, 7, 512, 0x040300).is_ok());
        assert!(
            authorize_function(
                &indexed,
                &[DeviceId {
                    kind: DeviceKind::Vfio,
                    key: 7
                }],
                7,
                512,
                0x030000
            )
            .is_ok()
        );
    }
}
