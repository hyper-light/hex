//! The node's facts, read from its OS (kiln `crates/kiln/src/runner/probe.rs` at `a63dd98ef`: `machine`
//! :267, `kvm` :228, `volume` :236, `mem` :314, ported): its OS and architecture as this build's target
//! names them, its hardware threads, its memory no more than its cgroup's limit, the volume boxes' disks
//! go on, KVM and nested virtualization, Hypervisor.framework, and the devices boxes may be given whole.
//! Each OS's interface is read here and its text handed to `hex_sim::facts`' readers. kiln reads
//! through `sysinfo`; Hex reads each OS's own interface: Linux's `/proc` and `/sys`, macOS's sysctls,
//! Windows' `GlobalMemoryStatusEx` and `GetDiskFreeSpaceExW` (`hex_runtime`'s OS files).
//!
//! GPUs come from hex-gpu: a [`Prober`] opens `hex_gpu::Gpus` once (NVML's initialisation) and keeps
//! it, and each probe reads its `inventory()` and `sample()`. Where hex-gpu reads none, the node reports
//! no GPUs and why, as kiln leaves out a fact its probe can't establish. A platform Hex doesn't place on
//! is refused, typed, naming it.

use std::path::Path;

use hex_api::Refusal;
use hex_sim::facts::{self, Cpu, Disk, Facts, Found, Mem};
use hex_sim::model::{Class, Classes, Virt};

/// What an administrator gives Hex beside what the probe finds (kiln's `Declared`, W13-R02): block
/// devices and network interfaces boxes may be given whole, by their kernel names (`nvme1n1`,
/// `enp5s0f1v0`). A node's own disk and its uplink are never found for boxes: only what is declared is
/// theirs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Declared {
    pub block: Vec<String>,
    pub net: Vec<String>,
}

/// What reads a node's facts: its GPUs opened once and kept, the folder boxes' disks go on, and what
/// its administrator declares.
#[derive(Debug)]
pub struct Prober {
    gpus: hex_gpu::Gpus,
    dir: std::path::PathBuf,
    declared: Declared,
    classes: Classes,
    available: Option<Classes>,
}

impl Prober {
    /// Opens the node's GPUs (hex-gpu's `Gpus::open`: NVML loaded and initialised, amdgpu's counters
    /// opened), once for the node's life.
    pub fn open(dir: &Path, declared: Declared) -> Self {
        Self {
            gpus: hex_gpu::Gpus::open(),
            dir: dir.to_path_buf(),
            declared,
            classes: Classes::NONE,
            available: None,
        }
    }

    /// CP-T7.4: classes the caller's runtime registry initialized successfully. Platform probes further
    /// restrict them; an optional feature or hypervisor interface alone never configures a backend.
    pub fn with_classes(mut self, classes: Classes) -> Self {
        self.classes = classes;
        self
    }

    /// Every fact of this node, read now: its GPUs' load sampled.
    pub fn probe(&mut self) -> Result<Facts, Refusal> {
        let available = match self.available {
            Some(available) => available,
            None => {
                let available = here::classes()?;
                self.available = Some(available);
                available
            }
        };
        let classes = [Class::Oci, Class::Firecracker, Class::Shards]
            .into_iter()
            .filter(|&c| self.classes.has(c) && available.has(c))
            .fold(Classes::NONE, Classes::with);
        let mut f = machine(&self.dir, &self.declared, classes)?;
        (f.gpus, f.no_gpus) = gpus(&mut self.gpus);
        Ok(f)
    }
}

/// The GPUs hex-gpu reads, each with its load now; or none, and why.
pub fn gpus(found: &mut hex_gpu::Gpus) -> (Vec<facts::Gpu>, Option<String>) {
    let inventory = match found.inventory() {
        Ok(inventory) => inventory.to_vec(),
        Err(none) => return (Vec::new(), Some(none.to_string())),
    };
    match found.sample() {
        Ok(loads) => (gpu_facts(&inventory, loads), None),
        Err(none) => (Vec::new(), Some(none.to_string())),
    }
}

/// hex-gpu's inventory and loads as the node's facts. A counter that can't be read leaves its part of
/// the load out (a MIG-enabled NVIDIA GPU has no utilisation; its memory in use still counts), rather
/// than guessing it busy or idle.
pub fn gpu_facts(inventory: &[hex_gpu::Gpu], loads: &[hex_gpu::Load]) -> Vec<facts::Gpu> {
    inventory
        .iter()
        .zip(loads.iter().map(Some).chain(std::iter::repeat(None)))
        .map(|(g, load)| facts::Gpu {
            vendor: match g.vendor {
                hex_gpu::Vendor::Nvidia => "nvidia",
                hex_gpu::Vendor::Amd => "amd",
            }
            .to_owned(),
            model: g.model.clone(),
            memory: g.memory_total_bytes,
            pci: Some(g.pci.to_string()),
            driver: Some(g.driver_version.clone()),
            load: hex_sim::model::GpuLoad {
                busy_per_mille: load
                    .and_then(|l| l.busy_percent.ok())
                    .map_or(0, |p| u16::from(p.min(100)).saturating_mul(10)),
                memory_used: load.and_then(|l| l.memory_used_bytes.ok()).unwrap_or(0),
            },
        })
        .collect()
}

/// AC-T7.4.1: basic runtime room without device discovery or GPU driver initialization. The caller
/// supplies its stable pool identity and actual writable-bundles volume, including inside a guest.
pub fn capacity(dir: &Path, id: String) -> Result<hex_api::Capacity, Refusal> {
    let memory = here::memory();
    let disk = here::volume(dir);
    let cpu = std::thread::available_parallelism()
        .ok()
        .and_then(|n| u64::try_from(n.get()).ok())
        .and_then(|n| n.checked_mul(1000))
        .unwrap_or(0);
    #[cfg(target_os = "linux")]
    let cpu = here::cpu_milli(cpu);
    let total = hex_api::CapacityResources {
        cpu_milli: cpu,
        mem_bytes: memory.total,
        disk_bytes: disk.total,
    };
    let free = hex_api::CapacityResources {
        cpu_milli: cpu,
        mem_bytes: memory.free.min(memory.total),
        disk_bytes: disk.free.min(disk.total),
    };
    let capacity = hex_api::Capacity { id, total, free };
    capacity.check()?;
    Ok(capacity)
}

/// Every fact of this node but its GPUs, read now. `dir` is the folder boxes' disks go on.
fn machine(dir: &Path, declared: &Declared, initialized: Classes) -> Result<Facts, Refusal> {
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
    let virt = here::virt();
    Ok(Facts {
        os,
        arch,
        cpu: Cpu {
            cores: here::cores(),
            threads,
        },
        mem: here::memory(),
        disk: here::volume(dir),
        virt,
        classes: facts::classes(os, arch, virt, initialized),
        gpus: Vec::new(),
        no_gpus: None,
        devices: here::devices(declared),
    })
}

#[cfg(target_os = "linux")]
mod here {
    use std::fs;

    use super::{Classes, Declared, Disk, Found, Mem, Virt, facts};
    use hex_sim::model::DeviceKind;

    /// The most entries a probe reads of one directory of the kernel's: a PCI segment's functions
    /// (bus, device and function: 16 bits), which also bounds a node's CPUs and interfaces.
    const ENTRIES: usize = hex_sim::model::DEVICES;

    fn read(path: &str) -> Option<String> {
        fs::read_to_string(path).ok()
    }

    /// `/proc/meminfo`, constrained by each cgroup ancestor's finite limit and current charge.
    pub fn memory() -> Mem {
        let (total, free) = read("/proc/meminfo")
            .as_deref()
            .and_then(facts::meminfo)
            .unwrap_or((0, 0));
        let own = read("/proc/self/cgroup");
        let Some(own) = own.as_deref().and_then(facts::own_cgroup) else {
            return facts::mem(total, free, None);
        };
        let root = std::path::Path::new("/sys/fs/cgroup");
        let mut group = root.join(own.trim_start_matches('/'));
        let mut memory = facts::mem(total, free, None);
        // The path's own ancestor count bounds this walk; the root has no memory.max of its own.
        while group.starts_with(root) {
            let limit = match fs::read_to_string(group.join("memory.max")) {
                Ok(text) if text.trim() == "max" => None,
                Ok(text) => match facts::cgroup_max(&text) {
                    Some(limit) => Some(limit),
                    None => return Mem { free: 0, ..memory },
                },
                // An ancestor without this controller adds no bound. An unread bound establishes no room.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(_) => return Mem { free: 0, ..memory },
            };
            let current = fs::read_to_string(group.join("memory.current"))
                .ok()
                .and_then(|s| s.trim().parse().ok());
            memory = facts::within_cgroup(memory, limit, current);
            if group == root || !group.pop() {
                break;
            }
        }
        memory
    }

    /// cgroup v2 cpu.max is quota/period, in microseconds; keep fractional CPUs as milli,
    /// intersecting every ancestor. Unread/malformed finite authority grants no CPU room.
    pub fn cpu_milli(mut offered: u64) -> u64 {
        let Some(own) = read("/proc/self/cgroup")
            .as_deref()
            .and_then(facts::own_cgroup)
            .map(str::to_owned)
        else {
            return 0;
        };
        let root = std::path::Path::new("/sys/fs/cgroup");
        let mut group = root.join(own.trim_start_matches('/'));
        while group.starts_with(root) {
            match fs::read_to_string(group.join("cpu.max")) {
                Ok(text) => match quota_milli(&text) {
                    Ok(Some(quota)) => offered = offered.min(quota),
                    Ok(None) => {}
                    Err(()) => return 0,
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return 0,
            }
            if group == root || !group.pop() {
                break;
            }
        }
        offered
    }

    pub(super) fn quota_milli(text: &str) -> Result<Option<u64>, ()> {
        let mut words = text.split_whitespace();
        let quota = words.next().ok_or(())?;
        let period: u64 = words.next().ok_or(())?.parse().map_err(|_| ())?;
        if period == 0 || words.next().is_some() {
            return Err(());
        }
        if quota == "max" {
            return Ok(None);
        }
        let quota: u64 = quota.parse().map_err(|_| ())?;
        let milli = u128::from(quota)
            .checked_mul(1000)
            .and_then(|n| n.checked_div(u128::from(period)))
            .ok_or(())?;
        u64::try_from(milli).map(Some).map_err(|_| ())
    }

    pub fn classes() -> Result<Classes, hex_api::Refusal> {
        Ok(Classes::ALL)
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
    use super::{Classes, Declared, Disk, Found, Mem, Virt};
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

    pub fn classes() -> Result<Classes, hex_api::Refusal> {
        Ok(Classes::ALL)
    }

    /// No device is given whole on macOS: its one class, Shards through shards-api, names none.
    pub fn devices(_: &Declared) -> Vec<Found> {
        Vec::new()
    }
}

#[cfg(windows)]
mod here {
    use super::{Classes, Declared, Disk, Found, Mem, Virt};
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

    /// In-process WMI uses the installer's existing bounded feature query. An unread prerequisite is
    /// refused by type; caller-verified initialization is still needed when the features are enabled.
    pub fn classes() -> Result<Classes, hex_api::Refusal> {
        use hex_core::setup::WindowsFeature;
        hex_runtime::windows::features::off().map(|off| {
            hex_sim::facts::windows_classes(
                !off.contains(&WindowsFeature::Containers),
                !off.contains(&WindowsFeature::HypervisorPlatform),
            )
        })
    }

    /// No device is given whole on Windows: a process-isolated container is given a GPU by its
    /// device interface class (from hex-gpu), and nothing whole.
    pub fn devices(_: &Declared) -> Vec<Found> {
        Vec::new()
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod here {
    use super::{Classes, Declared, Disk, Found, Mem, Virt};

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
    pub fn classes() -> Result<Classes, hex_api::Refusal> {
        Ok(Classes::NONE)
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
        let f = Prober::open(dir.path(), Declared::default())
            .probe()
            .unwrap();
        assert_eq!(
            Some((f.os, f.arch)),
            facts::platform(std::env::consts::OS, std::env::consts::ARCH)
        );
        assert!(f.cpu.threads > 0, "{f:?}");
        assert!(f.mem.total > 0 && f.mem.free <= f.mem.total, "{f:?}");
        assert!(f.disk.total > 0 && f.disk.free <= f.disk.total, "{f:?}");
        // CP-T7.4: physical cores describe the host; offered threads respect this process's
        // affinity and CPU quota. A four-CPU node on a sixteen-core host keeps both facts.
        assert!(f.cpu.cores.is_none_or(|c| c > 0), "{f:?}");
        if cfg!(target_os = "macos") {
            assert!(f.cpu.cores.is_some() && f.virt.hvf, "{f:?}");
        }
        let mut cluster = hex_sim::model::Cluster::single_owner(Vec::new(), Vec::new());
        let node = f.node(hex_sim::model::NodeId(0), &mut cluster).unwrap();
        assert_eq!(node.capacity.cpu_milli, u64::from(f.cpu.threads) * 1000);
        assert_eq!(
            node.classes,
            Classes::NONE,
            "an unconfigured probe advertises no runtime"
        );
    }

    /// CP-T7.4: CPU quota can offer fewer threads than the host has physical cores. Keep the
    /// topology fact while charging placement only against the node's available parallelism.
    #[test]
    fn a_cpu_quota_bounds_capacity_without_rewriting_physical_cores() {
        let f = Facts {
            cpu: Cpu {
                cores: Some(16),
                threads: 4,
            },
            ..Facts::default()
        };
        let mut cluster = hex_sim::model::Cluster::single_owner(Vec::new(), Vec::new());
        let node = f.node(hex_sim::model::NodeId(0), &mut cluster).unwrap();
        assert_eq!(f.cpu.cores, Some(16));
        assert_eq!(node.capacity.cpu_milli, 4_000);
    }

    /// CP-T7.4: initialize only one fake backend, then probe. Advertisements retain only that
    /// backend's platform prerequisites; a probe without a registry never configures one implicitly.
    #[test]
    fn a_probe_advertises_only_the_initialized_runtime_registry() {
        let dir = tempfile::tempdir().unwrap();
        let requested = Classes::NONE.with(Class::Shards);
        let mut prober = Prober::open(dir.path(), Declared::default()).with_classes(requested);
        let found = prober.probe().unwrap();
        assert!(!found.classes.has(Class::Oci));
        assert!(!found.classes.has(Class::Firecracker));
        assert_eq!(
            found.classes,
            facts::classes(
                found.os,
                found.arch,
                found.virt,
                if prober.available.unwrap().has(Class::Shards) {
                    requested
                } else {
                    Classes::NONE
                }
            )
        );
    }

    /// CP-T7.4, GPU load from hex-gpu: do probe this machine's GPUs through the kept `Gpus`; expect
    /// either each GPU hex-gpu lists, with its load, or none and hex-gpu's reason (macOS, a host
    /// without a driver), never both and never neither.
    #[test]
    fn gpus_come_from_hex_gpu_or_say_why_not() {
        let dir = tempfile::tempdir().unwrap();
        let mut prober = Prober::open(dir.path(), Declared::default());
        let f = prober.probe().unwrap();
        match prober.gpus.inventory() {
            Ok(listed) => {
                assert_eq!((f.gpus.len(), &f.no_gpus), (listed.len(), &None), "{f:?}");
            }
            Err(none) => {
                assert!(f.gpus.is_empty(), "{f:?}");
                assert_eq!(f.no_gpus, Some(none.to_string()));
            }
        }
        if cfg!(target_os = "macos") {
            assert!(f.no_gpus.is_some(), "{f:?}");
        }
    }

    /// CP-T7.4: do turn hex-gpu's inventory and loads into the node's facts; expect its vendor,
    /// model, memory, PCI address and driver kept, its busy percent in per mille, its memory in use,
    /// and a counter that can't be read left out of the load rather than guessed.
    #[test]
    fn hex_gpus_inventory_and_loads_become_the_nodes_gpus() {
        let gpu = |model: &str, bus: u8| hex_gpu::Gpu {
            vendor: hex_gpu::Vendor::Nvidia,
            model: model.to_owned(),
            memory_total_bytes: 8_546_942_976,
            pci: hex_gpu::PciAddress {
                domain: 0,
                bus,
                device: 0,
                function: 0,
            },
            driver_version: "620.70.06".to_owned(),
        };
        let inventory = [gpu("NVIDIA GeForce RTX 5060 Ti", 1), gpu("NVIDIA L4", 2)];
        let loads = [
            hex_gpu::Load {
                busy_percent: Ok(37),
                memory_used_bytes: Ok(1 << 30),
            },
            hex_gpu::Load {
                busy_percent: Err(hex_gpu::Unread::Unsupported),
                memory_used_bytes: Ok(2 << 30),
            },
        ];
        let got = gpu_facts(&inventory, &loads);
        assert_eq!(got.len(), 2);
        assert_eq!(
            (got[0].vendor.as_str(), got[0].model.as_str()),
            ("nvidia", "NVIDIA GeForce RTX 5060 Ti")
        );
        assert_eq!(
            (
                got[0].memory,
                got[0].pci.as_deref(),
                got[0].driver.as_deref()
            ),
            (8_546_942_976, Some("0000:01:00.0"), Some("620.70.06"))
        );
        assert_eq!(
            got[0].load,
            hex_sim::model::GpuLoad {
                busy_per_mille: 370,
                memory_used: 1 << 30
            }
        );
        assert_eq!(
            got[1].load,
            hex_sim::model::GpuLoad {
                busy_per_mille: 0,
                memory_used: 2 << 30
            }
        );
        // A node made from them holds both, their load the pick reads.
        let f = Facts {
            gpus: got,
            cpu: facts::Cpu {
                cores: None,
                threads: 8,
            },
            virt: Virt {
                kvm: true,
                ..Virt::default()
            },
            classes: Classes::ALL,
            ..Facts::default()
        };
        let mut cluster = hex_sim::model::Cluster::single_owner(Vec::new(), Vec::new());
        let node = f.node(hex_sim::model::NodeId(0), &mut cluster).unwrap();
        assert_eq!(
            (node.capacity.gpus, node.devices[1].load.memory_used),
            (2, 2 << 30)
        );
    }

    /// A declared block device or interface that the kernel hasn't is left out, never guessed.
    #[test]
    fn a_declared_device_the_kernel_hasnt_is_left_out() {
        let declared = Declared {
            block: vec!["hex-no-such-disk".into()],
            net: vec!["hex-no-such-if".into()],
        };
        let dir = tempfile::tempdir().unwrap();
        let f = Prober::open(dir.path(), declared).probe().unwrap();
        assert!(
            f.devices
                .iter()
                .all(|d| d.name != "hex-no-such-disk" && d.name != "hex-no-such-if"),
            "{f:?}"
        );
    }
}

#[cfg(all(test, target_os = "linux"))]
mod capacity_tests {
    #[test]
    fn runtime_capacity_cpu_quota_preserves_fractional_milli_and_refuses_bad_authority() {
        assert_eq!(super::here::quota_milli("50000 100000"), Ok(Some(500)));
        assert_eq!(super::here::quota_milli("max 100000"), Ok(None));
        for bad in ["", "max", "1 0", "wat 100", "1 100 extra"] {
            assert!(super::here::quota_milli(bad).is_err(), "{bad}");
        }
    }
}
