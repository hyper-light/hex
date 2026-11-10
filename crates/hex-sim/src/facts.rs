//! What a node reports about itself, and the node the scheduler places on from it (kiln
//! `crates/kiln-ci/src/facts.rs`'s `RunnerFacts` and `crates/kiln/src/runner/probe.rs` at `a63dd98ef`,
//! adapted to boxes): its OS and architecture, CPU, memory, the volume it keeps boxes on, its
//! virtualization, and the devices a box may be given, GPUs in hex-gpu's shape (`inventory()`'s
//! vendor, model, memory, PCI id and driver; `sample()`'s utilisation and memory in use).
//!
//! This is the pure half: the facts, the text each OS gives them in, read here, and the conversion.
//! A node's agent reads its OS's interfaces and hands the text to these readers (`hex-node`'s
//! `facts`), so every reader is tested on every OS. A fact a probe can't establish is left out, as
//! kiln's are: `None`, or no device.

use crate::SimError;
use crate::model::{
    Arch, Classes, Cluster, Device, DeviceKind, GpuLoad, ModelId, NodeId, NodeSpec, Os, Platform,
    Resources, Virt,
};

/// A node's facts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    pub os: Os,
    pub arch: Arch,
    pub cpu: Cpu,
    pub mem: Mem,
    /// The volume boxes' disks go on.
    pub disk: Disk,
    pub virt: Virt,
    /// Its GPUs, from hex-gpu.
    pub gpus: Vec<Gpu>,
    /// Why it reports no GPUs, where hex-gpu refused to read any (no driver, an OS it can't read).
    pub no_gpus: Option<String>,
    /// Devices a box may be given whole: PCI functions bound to `vfio-pci`, and the block and network
    /// devices its administrator gave Hex.
    pub devices: Vec<Found>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cpu {
    /// Physical cores, where the OS says.
    pub cores: Option<u32>,
    /// Hardware threads: what a node offers, in whole CPUs.
    pub threads: u32,
}

/// Memory: the machine's, no more than the node agent's cgroup allows (kiln's `mem`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mem {
    pub total: u64,
    pub free: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Disk {
    pub total: u64,
    /// What this user may still write.
    pub free: u64,
}

/// A GPU as hex-gpu's `inventory()` and `sample()` give it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gpu {
    pub vendor: String,
    pub model: String,
    /// Its memory in bytes; 0 where it shares the machine's (Apple silicon).
    pub memory: u64,
    /// Its PCI address (`0000:01:00.0`), where it has one.
    pub pci: Option<String>,
    pub driver: Option<String>,
    /// Its load from what isn't Hex's, as last sampled.
    pub load: GpuLoad,
}

/// A device found for boxes to be given whole: its kind, its name (a PCI address, a block device, a
/// network interface) and its size (a block device's bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub kind: DeviceKind,
    pub name: String,
    pub size: u64,
}

/// kiln's `mem`: the machine's memory, no more than a cgroup's limit, and free no more than that.
pub fn mem(total: u64, free: u64, limit: Option<u64>) -> Mem {
    let total = limit.map_or(total, |l| l.min(total));
    Mem {
        total,
        free: free.min(total),
    }
}

/// `/proc/meminfo`'s `MemTotal` and `MemAvailable`, in bytes (proc(5): each in kB, 1024 bytes).
pub fn meminfo(text: &str) -> Option<(u64, u64)> {
    let field = |name: &str| {
        text.lines().find_map(|l| {
            let rest = l.strip_prefix(name)?.strip_prefix(':')?;
            let kb: u64 = rest.trim().strip_suffix("kB")?.trim().parse().ok()?;
            kb.checked_mul(1024)
        })
    };
    Some((field("MemTotal")?, field("MemAvailable")?))
}

/// A cgroup v2 `memory.max`: a limit in bytes, or none for `max`.
pub fn cgroup_max(text: &str) -> Option<u64> {
    text.trim().parse().ok()
}

/// The node agent's own cgroup v2 path, from `/proc/self/cgroup`'s unified line (`0::/system.slice/…`).
pub fn own_cgroup(text: &str) -> Option<&str> {
    text.lines().find_map(|l| l.strip_prefix("0::"))
}

/// A kernel module's boolean parameter (`/sys/module/kvm_intel/parameters/nested`): `Y`, `y` or `1`.
pub fn yes(text: &str) -> bool {
    matches!(text.trim(), "Y" | "y" | "1")
}

/// Physical cores from each CPU's `topology/physical_package_id` and `topology/core_id`: the distinct
/// pairs.
pub fn cores<'a>(pairs: impl Iterator<Item = (&'a str, &'a str)>) -> Option<u32> {
    let mut seen: Vec<(u64, u64)> = pairs
        .filter_map(|(pkg, core)| Some((pkg.trim().parse().ok()?, core.trim().parse().ok()?)))
        .collect();
    seen.sort_unstable();
    seen.dedup();
    u32::try_from(seen.len()).ok().filter(|&n| n > 0)
}

/// A block device's size from `/sys/class/block/<name>/size`: 512-byte sectors, whatever the device's
/// own (the kernel's `include/linux/types.h` `sector_t`).
pub fn block_size(text: &str) -> Option<u64> {
    text.trim().parse::<u64>().ok()?.checked_mul(512)
}

/// A PCI address as sysfs names a device (`0000:01:00.0`): domain, bus, device and function in hex.
pub fn pci_address(name: &str) -> bool {
    let b = name.as_bytes();
    let hex = |r: std::ops::Range<usize>| {
        b.get(r)
            .is_some_and(|s| s.iter().all(u8::is_ascii_hexdigit))
    };
    b.len() == 12
        && hex(0..4)
        && b.get(4) == Some(&b':')
        && hex(5..7)
        && b.get(7) == Some(&b':')
        && hex(8..10)
        && b.get(10) == Some(&b'.')
        && b.get(11).is_some_and(|f| (b'0'..=b'7').contains(f))
}

/// The OS and architecture Rust names for this build's target (`std::env::consts`), as kiln reads them.
pub fn platform(os: &str, arch: &str) -> Option<(Os, Arch)> {
    Some((Os::named(os)?, Arch::named(arch)?))
}

impl Facts {
    /// The node the scheduler places on: its threads, memory and disk its capacity; the classes its
    /// OS and KVM run; its GPUs and devices those classes can be given (an Apple GPU none can, and
    /// no class can be given a device on a node that runs only microVMs), GPU models interned in
    /// `cluster`.
    pub fn node(&self, id: NodeId, cluster: &mut Cluster) -> Result<NodeSpec, SimError> {
        let classes = self.os.classes(self.virt);
        let given = |kind: DeviceKind| {
            [
                crate::model::Class::Oci,
                crate::model::Class::Firecracker,
                crate::model::Class::Shards,
            ]
            .into_iter()
            .any(|c| classes.has(c) && c.exposes(self.os, kind))
        };
        let mut devices = Vec::new();
        if given(DeviceKind::Gpu) {
            for g in &self.gpus {
                devices.push(Device {
                    load: g.load,
                    ..Device::gpu(cluster.model(&g.model)?, g.memory)
                });
            }
        }
        let gpus = u32::try_from(devices.len()).map_err(|_| SimError::Full {
            what: "a node's GPUs",
            bound: usize::try_from(u32::MAX).unwrap_or(usize::MAX),
        })?;
        for f in self.devices.iter().filter(|f| given(f.kind)) {
            devices.push(Device {
                kind: f.kind,
                model: ModelId::UNNAMED,
                size: f.size,
                load: GpuLoad::default(),
            });
        }
        let node = NodeSpec {
            id,
            capacity: Resources {
                cpu_milli: u64::from(self.cpu.threads).saturating_mul(crate::lb::CPU),
                mem_bytes: self.mem.total,
                gpus,
                disk_bytes: self.disk.total,
            },
            background_milli: 0,
            classes: if self.cpu.threads == 0 {
                Classes::NONE
            } else {
                classes
            },
            platform: Platform {
                os: self.os,
                arch: self.arch,
            },
            virt: self.virt,
            devices,
        };
        if !node.devices_agree() {
            return Err(SimError::Full {
                what: "a node's devices",
                bound: crate::model::DEVICES,
            });
        }
        Ok(node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CLAUDE.md §6, hostile input: do read real and broken `/proc/meminfo` text; expect bytes from the
    /// real, nothing from the broken.
    #[test]
    fn meminfo_reads_total_and_available_in_bytes() {
        let text =
            "MemTotal:       131986903 kB\nMemFree:         1000 kB\nMemAvailable:   65000000 kB\n";
        assert_eq!(meminfo(text), Some((131_986_903 * 1024, 65_000_000 * 1024)));
        for broken in [
            "",
            "MemTotal: lots kB\nMemAvailable: 1 kB",
            "MemTotal: 1\nMemAvailable: 1 kB",
            "MemTotal: 99999999999999999999 kB\nMemAvailable: 1 kB",
        ] {
            assert_eq!(meminfo(broken), None, "{broken:?}");
        }
    }

    #[test]
    fn a_cgroup_limit_caps_memory() {
        assert_eq!(cgroup_max("max\n"), None);
        assert_eq!(cgroup_max("17179869184\n"), Some(16 << 30));
        assert_eq!(
            mem(64 << 30, 40 << 30, Some(16 << 30)),
            Mem {
                total: 16 << 30,
                free: 16 << 30
            }
        );
        assert_eq!(
            mem(64 << 30, 40 << 30, None),
            Mem {
                total: 64 << 30,
                free: 40 << 30
            }
        );
        assert_eq!(
            own_cgroup("0::/system.slice/hex-node.service\n"),
            Some("/system.slice/hex-node.service")
        );
        assert_eq!(own_cgroup("1:name=systemd:/x\n"), None);
    }

    #[test]
    fn cores_are_distinct_package_and_core_pairs() {
        let pairs = [
            ("0", "0"),
            ("0", "0"),
            ("0", "1"),
            ("0", "1"),
            ("1", "0"),
            ("x", "1"),
        ];
        assert_eq!(cores(pairs.into_iter()), Some(3));
        assert_eq!(cores(std::iter::empty()), None);
    }

    #[test]
    fn sysfs_words_read_as_the_kernel_writes_them() {
        assert!(yes("Y\n") && yes("1") && !yes("N\n") && !yes(""));
        assert_eq!(block_size("1953525168\n"), Some(1_000_204_886_016));
        assert_eq!(block_size("18446744073709551615"), None);
        assert!(pci_address("0000:01:00.0"));
        for not in [
            "0000:01:00.8",
            "0000:01:00",
            "new_id",
            "0000:g1:00.0",
            "0000:01:00.0x",
        ] {
            assert!(!pci_address(not), "{not}");
        }
        assert_eq!(
            platform("macos", "aarch64"),
            Some((Os::Macos, Arch::Aarch64))
        );
        assert_eq!(platform("solaris", "x86_64"), None);
    }

    /// CP-T7.4: do turn each OS's facts into a node; expect its capacity from its threads, memory and
    /// disk, the classes its OS and KVM run, and only the devices one of those classes can be given:
    /// a Linux host's GPU and VFIO function, a Windows host's GPU and none of its devices whole, a
    /// Mac's GPU not at all.
    #[test]
    fn facts_make_the_node_each_os_can_be() {
        let gpu = Gpu {
            vendor: "nvidia".into(),
            model: "NVIDIA GeForce RTX 5060 Ti".into(),
            memory: 8_546_942_976,
            pci: Some("0000:01:00.0".into()),
            driver: Some("615.78.08".into()),
            load: GpuLoad {
                busy_per_mille: 120,
                memory_used: 1 << 30,
            },
        };
        let vfio = Found {
            kind: DeviceKind::Vfio,
            name: "0000:02:00.0".into(),
            size: 0,
        };
        let facts = |os, kvm| Facts {
            os,
            arch: Arch::X86_64,
            cpu: Cpu {
                cores: Some(16),
                threads: 32,
            },
            mem: Mem {
                total: 128 << 30,
                free: 64 << 30,
            },
            disk: Disk {
                total: 8 << 40,
                free: 4 << 40,
            },
            virt: Virt {
                kvm,
                nested: kvm,
                hvf: os == Os::Macos,
            },
            gpus: vec![gpu.clone()],
            no_gpus: None,
            devices: vec![vfio.clone()],
        };
        let mut c = Cluster::single_owner(Vec::new(), Vec::new());
        let linux = facts(Os::Linux, true).node(NodeId(0), &mut c).unwrap();
        assert_eq!(
            linux.capacity,
            Resources {
                cpu_milli: 32_000,
                mem_bytes: 128 << 30,
                gpus: 1,
                disk_bytes: 8 << 40
            }
        );
        assert_eq!(linux.classes, Classes::ALL);
        assert_eq!(linux.devices.len(), 2);
        assert_eq!(linux.devices[0].load, gpu.load);
        assert_eq!(c.models, vec!["nvidia-geforce-rtx-5060-ti".to_owned()]);
        let no_kvm = facts(Os::Linux, false).node(NodeId(1), &mut c).unwrap();
        assert_eq!(no_kvm.classes, Classes::NONE.with(crate::model::Class::Oci));
        let windows = facts(Os::Windows, false).node(NodeId(2), &mut c).unwrap();
        assert_eq!((windows.capacity.gpus, windows.devices.len()), (1, 1));
        let mac = facts(Os::Macos, false).node(NodeId(3), &mut c).unwrap();
        assert_eq!((mac.capacity.gpus, mac.devices.len()), (0, 0));
        assert_eq!(c.models.len(), 1);
    }
}
