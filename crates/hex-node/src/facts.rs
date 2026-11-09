//! The node's facts, read from its OS (kiln `crates/kiln/src/runner/probe.rs` at `a63dd98ef`: `machine`
//! :267, `kvm` :228, `volume` :236, `mem` :314, ported): its OS and architecture as this build's target
//! names them, its hardware threads, its memory no more than its cgroup's limit, the volume boxes' disks
//! go on, KVM and nested virtualization, Hypervisor.framework, and the devices boxes may be given whole.
//! Each OS's interface is read here and its text handed to `hex_sim::facts`' readers. kiln reads
//! through `sysinfo`; Hex reads each OS's own interface: Linux's `/proc` and `/sys`, macOS's sysctls,
//! Windows' `GlobalMemoryStatusEx` and `GetDiskFreeSpaceExW` (`hex_runtime`'s OS files).
//!
//! GPUs come from hex-gpu's `inventory()` and `sample()`; until it is wired here a node reports none,
//! as kiln leaves out a fact its probe can't establish. A platform Hex doesn't place on is refused,
//! typed, naming it.

use std::path::Path;

use hex_api::Refusal;
use hex_sim::facts::{self, Cpu, Disk, Facts, Found, Mem};
use hex_sim::model::Virt;

/// What an administrator gives Hex beside what the probe finds (kiln's `Declared`, W13-R02): block
/// devices and network interfaces boxes may be given whole, by their kernel names (`nvme1n1`,
/// `enp5s0f1v0`). A node's own disk and its uplink are never found for boxes: only what is declared is
/// theirs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Declared {
    pub block: Vec<String>,
    pub net: Vec<String>,
}

/// Every fact of this node, read now. `dir` is the folder boxes' disks go on.
pub fn probe(dir: &Path, declared: &Declared) -> Result<Facts, Refusal> {
    let (os, arch) =
        facts::platform(std::env::consts::OS, std::env::consts::ARCH).ok_or(Refusal::Platform {
            what: "a node's facts",
            platform: std::env::consts::OS,
            why: format!(
                "Hex places on Linux, macOS and Windows, on x86_64 and aarch64; this is {} on {}",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
        })?;
    let threads = std::thread::available_parallelism()
        .map_or(0, |n| u32::try_from(n.get()).unwrap_or(u32::MAX));
    Ok(Facts {
        os,
        arch,
        cpu: Cpu {
            cores: here::cores(),
            threads,
        },
        mem: here::memory(),
        disk: here::volume(dir),
        virt: here::virt(),
        gpus: Vec::new(),
        devices: here::devices(declared),
    })
}

#[cfg(target_os = "linux")]
mod here {
    use std::fs;

    use super::{Declared, Disk, Found, Mem, Virt, facts};
    use hex_sim::model::DeviceKind;

    /// The most entries a probe reads of one directory of the kernel's: a PCI segment's functions
    /// (bus, device and function: 16 bits), which also bounds a node's CPUs and interfaces.
    const ENTRIES: usize = hex_sim::model::DEVICES;

    fn read(path: &str) -> Option<String> {
        fs::read_to_string(path).ok()
    }

    /// `/proc/meminfo`, no more than the agent's cgroup v2 `memory.max`.
    pub fn memory() -> Mem {
        let (total, free) = read("/proc/meminfo")
            .as_deref()
            .and_then(facts::meminfo)
            .unwrap_or((0, 0));
        let limit = read("/proc/self/cgroup")
            .as_deref()
            .and_then(facts::own_cgroup)
            .and_then(|p| read(&format!("/sys/fs/cgroup{p}/memory.max")))
            .as_deref()
            .and_then(facts::cgroup_max);
        facts::mem(total, free, limit)
    }

    /// Each online CPU's package and core (`/sys/devices/system/cpu/cpuN/topology`).
    pub fn cores() -> Option<u32> {
        let entries = fs::read_dir("/sys/devices/system/cpu").ok()?;
        let mut pairs = Vec::new();
        for e in entries.filter_map(Result::ok).take(ENTRIES) {
            let name = e.file_name();
            let Some(n) = name.to_str().and_then(|n| n.strip_prefix("cpu")) else {
                continue;
            };
            if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let dir = e.path().join("topology");
            if let (Ok(pkg), Ok(core)) = (
                fs::read_to_string(dir.join("physical_package_id")),
                fs::read_to_string(dir.join("core_id")),
            ) {
                pairs.push((pkg, core));
            }
        }
        facts::cores(pairs.iter().map(|(p, c)| (p.as_str(), c.as_str())))
    }

    /// The volume's size and what this user may write (statvfs(3)).
    pub fn volume(dir: &std::path::Path) -> Disk {
        super::statvfs(dir)
    }

    /// `/dev/kvm` opened read-write, as a VMM opens it; nested from the KVM module's parameter.
    pub fn virt() -> Virt {
        let kvm = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/kvm")
            .is_ok();
        let nested = [
            "/sys/module/kvm_intel/parameters/nested",
            "/sys/module/kvm_amd/parameters/nested",
        ]
        .iter()
        .any(|p| read(p).is_some_and(|v| facts::yes(&v)));
        Virt {
            kvm,
            nested,
            hvf: false,
        }
    }

    /// PCI functions bound to `vfio-pci` (an administrator bound them for passthrough), and the block
    /// devices and interfaces declared, each as the kernel has it now.
    pub fn devices(declared: &Declared) -> Vec<Found> {
        let mut out = Vec::new();
        if let Ok(entries) = fs::read_dir("/sys/bus/pci/drivers/vfio-pci") {
            let mut names: Vec<String> = entries
                .filter_map(Result::ok)
                .take(ENTRIES)
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| facts::pci_address(n))
                .collect();
            names.sort_unstable();
            out.extend(names.into_iter().map(|name| Found {
                kind: DeviceKind::Vfio,
                name,
                size: 0,
            }));
        }
        for name in declared.block.iter().take(ENTRIES) {
            if let Some(size) = read(&format!("/sys/class/block/{name}/size"))
                .as_deref()
                .and_then(facts::block_size)
            {
                out.push(Found {
                    kind: DeviceKind::Block,
                    name: name.clone(),
                    size,
                });
            }
        }
        for name in declared.net.iter().take(ENTRIES) {
            if fs::metadata(format!("/sys/class/net/{name}")).is_ok() {
                out.push(Found {
                    kind: DeviceKind::Net,
                    name: name.clone(),
                    size: 0,
                });
            }
        }
        out
    }
}

#[cfg(target_os = "macos")]
mod here {
    use super::{Declared, Disk, Found, Mem, Virt};
    use hex_runtime::macos::sys::sysctl_number;

    /// `hw.memsize`; free the free and speculative pages (`vm.page_free_count`,
    /// `vm.page_speculative_count`) of `hw.pagesize` bytes.
    pub fn memory() -> Mem {
        let total = sysctl_number(c"hw.memsize").unwrap_or(0);
        let pages = sysctl_number(c"vm.page_free_count")
            .unwrap_or(0)
            .saturating_add(sysctl_number(c"vm.page_speculative_count").unwrap_or(0));
        let free = pages.saturating_mul(sysctl_number(c"hw.pagesize").unwrap_or(0));
        hex_sim::facts::mem(total, free, None)
    }

    pub fn cores() -> Option<u32> {
        sysctl_number(c"hw.physicalcpu")
            .ok()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|&n| n > 0)
    }

    pub fn volume(dir: &std::path::Path) -> Disk {
        super::statvfs(dir)
    }

    /// Hypervisor.framework, as the kernel says (`kern.hv_support`); macOS has no KVM.
    pub fn virt() -> Virt {
        Virt {
            hvf: sysctl_number(c"kern.hv_support").is_ok_and(|v| v == 1),
            ..Virt::default()
        }
    }

    /// No device is given whole on macOS: its one class, Shards through shards-api, names none.
    pub fn devices(_: &Declared) -> Vec<Found> {
        Vec::new()
    }
}

#[cfg(windows)]
mod here {
    use super::{Declared, Disk, Found, Mem, Virt};
    use hex_runtime::windows::sys;

    pub fn memory() -> Mem {
        let (total, free) = sys::memory().unwrap_or((0, 0));
        hex_sim::facts::mem(total, free, None)
    }

    /// Windows' cores are left out: its processor count is threads (`GetActiveProcessorCount`).
    pub fn cores() -> Option<u32> {
        None
    }

    pub fn volume(dir: &std::path::Path) -> Disk {
        let (total, free) = sys::disk_space(dir).unwrap_or((0, 0));
        Disk { total, free }
    }

    /// Neither KVM nor Hypervisor.framework; the Windows Hypervisor Platform isn't among kiln's facts.
    pub fn virt() -> Virt {
        Virt::default()
    }

    /// No device is given whole on Windows: a process-isolated container is given a GPU by its
    /// device interface class (from hex-gpu), and nothing whole.
    pub fn devices(_: &Declared) -> Vec<Found> {
        Vec::new()
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod here {
    use super::{Declared, Disk, Found, Mem, Virt};

    pub fn memory() -> Mem {
        Mem::default()
    }
    pub fn cores() -> Option<u32> {
        None
    }
    pub fn volume(_: &std::path::Path) -> Disk {
        Disk::default()
    }
    pub fn virt() -> Virt {
        Virt::default()
    }
    pub fn devices(_: &Declared) -> Vec<Found> {
        Vec::new()
    }
}

/// statvfs(3) of `dir`: its volume's blocks and those free to an unprivileged writer, each of
/// `f_frsize` bytes; nothing where it can't be read.
#[cfg(unix)]
fn statvfs(dir: &Path) -> Disk {
    let Ok(st) = rustix::fs::statvfs(dir) else {
        return Disk::default();
    };
    Disk {
        total: st.f_blocks.saturating_mul(st.f_frsize),
        free: st.f_bavail.saturating_mul(st.f_frsize),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CP-T7.4, the per-OS facts: do probe this machine; expect its platform the build's, threads
    /// and memory and the volume's size read (each OS's own interface: `/proc` and statvfs on Linux,
    /// sysctls and statvfs on macOS, `GlobalMemoryStatusEx` and `GetDiskFreeSpaceExW` on Windows), free
    /// no more than total, and a node the scheduler can place on made from them.
    #[test]
    fn this_machine_reports_its_facts() {
        let dir = tempfile::tempdir().unwrap();
        let f = probe(dir.path(), &Declared::default()).unwrap();
        assert_eq!(
            Some((f.os, f.arch)),
            facts::platform(std::env::consts::OS, std::env::consts::ARCH)
        );
        assert!(f.cpu.threads > 0, "{f:?}");
        assert!(f.mem.total > 0 && f.mem.free <= f.mem.total, "{f:?}");
        assert!(f.disk.total > 0 && f.disk.free <= f.disk.total, "{f:?}");
        assert!(
            f.cpu.cores.is_none_or(|c| c > 0 && c <= f.cpu.threads),
            "{f:?}"
        );
        if cfg!(target_os = "macos") {
            assert!(f.cpu.cores.is_some() && f.virt.hvf, "{f:?}");
        }
        let mut cluster = hex_sim::model::Cluster::single_owner(Vec::new(), Vec::new());
        let node = f.node(hex_sim::model::NodeId(0), &mut cluster).unwrap();
        assert_eq!(node.capacity.cpu_milli, u64::from(f.cpu.threads) * 1000);
        assert!(node.classes.count() > 0, "{node:?}");
    }

    /// A declared block device or interface that the kernel hasn't is left out, never guessed.
    #[test]
    fn a_declared_device_the_kernel_hasnt_is_left_out() {
        let declared = Declared {
            block: vec!["hex-no-such-disk".into()],
            net: vec!["hex-no-such-if".into()],
        };
        let dir = tempfile::tempdir().unwrap();
        let f = probe(dir.path(), &declared).unwrap();
        assert!(
            f.devices
                .iter()
                .all(|d| d.name != "hex-no-such-disk" && d.name != "hex-no-such-if"),
            "{f:?}"
        );
    }
}
