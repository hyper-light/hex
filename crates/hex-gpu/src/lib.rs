//! hex-gpu: a node's NVIDIA and AMD GPUs and their load, for Hex's scheduler to balance on beside CPU
//! load (`AC-T7.3.1`, placement by CPU, memory, disk and devices; Ada, 2026-11-09: "make sure to
//! incorporate GPU load in the balancing"). NVIDIA's are read through NVML and AMD's through amdgpu's
//! sysfs counters, each vendored and adapted from upstream (`vendor/cuda-rust`, `vendor/all-smi`;
//! `docs/design/gpu-telemetry.md`).
//!
//! [`Gpus::open`] does every costly thing once: it loads and initialises NVML (once for the process),
//! lists the cards amdgpu drives, reads what doesn't change and opens every counter a sample reads.
//! Then:
//!
//! - [`Gpus::inventory`] gives each GPU's [`Gpu`]: its [`Vendor`], model, total memory in bytes,
//!   [`PciAddress`] and driver version. NVIDIA's come first, then AMD's, each vendor's in PCI order.
//! - [`Gpus::sample`] gives each GPU's [`Load`] now, in the inventory's order: the percent of the time
//!   it is busy and the bytes of its memory in use. A counter costs one NVML call or one read of a file
//!   kept open; a sample allocates nothing, spawns nothing and walks no path.
//!
//! Both refuse with [`NoGpus`] when the node has no GPU this process can read, saying for each vendor
//! why ([`Absent`]): on macOS that is [`Absent::Unsupported`] for both, on Windows for AMD (decision
//! G3 says what reading it there would take). A device a driver counts that can't be read is in
//! [`Gpus::skipped`] with why ([`Skipped`]), never dropped silently (CLAUDE.md §3). A counter that
//! can't be read is an [`Unread`] in its place: [`Unread::Gone`] once a GPU is unplugged or falls off
//! the bus, after which a new [`Gpus::open`] lists what is there now.
//!
//! Bounds (CLAUDE.md §2): one entry per device the drivers report, read at open, and nothing per sample;
//! NVML's count is refused past the NVIDIA driver's own 248 devices, and each AMD card holds two file
//! descriptors (`amd`).

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::disallowed_macros
    )
)]

#[cfg(target_os = "linux")]
mod amd;
#[cfg(any(target_os = "linux", target_os = "windows"))]
mod nvml;
#[cfg(test)]
mod tests;

use std::fmt;

/// Whose GPU it is, and so which driver interface reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Vendor {
    /// Read through NVML.
    Nvidia,
    /// Read through amdgpu's sysfs counters.
    Amd,
}

/// A PCI function's address, `domain:bus:device.function`: the GPU's identity on its node, the same
/// whichever driver names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PciAddress {
    pub domain: u32,
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl PciAddress {
    /// Parses an address in hex as the kernel names a PCI device (`0000:01:00.0`) and NVML its bus id
    /// (`00000000:01:00.0`): a domain of one to eight digits, a bus of two, a device of two below
    /// 0x20, a function of one below 8. `None` for anything else.
    pub fn parse(text: &str) -> Option<PciAddress> {
        let (domain, rest) = text.split_once(':')?;
        let (bus, rest) = rest.split_once(':')?;
        let (device, function) = rest.split_once('.')?;
        let hex = |s: &str, min: usize, max: usize| {
            (s.len() >= min && s.len() <= max && s.bytes().all(|b| b.is_ascii_hexdigit()))
                .then(|| u32::from_str_radix(s, 16).ok())
                .flatten()
        };
        Some(PciAddress {
            domain: hex(domain, 1, 8)?,
            bus: u8::try_from(hex(bus, 2, 2)?).ok()?,
            device: u8::try_from(hex(device, 2, 2)?)
                .ok()
                .filter(|d| *d < 0x20)?,
            function: u8::try_from(hex(function, 1, 1)?).ok().filter(|f| *f < 8)?,
        })
    }
}

/// As the kernel writes it: `0000:01:00.0`.
impl fmt::Display for PciAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04x}:{:02x}:{:02x}.{:x}",
            self.domain, self.bus, self.device, self.function
        )
    }
}

/// One GPU as [`Gpus::open`] found it: what doesn't change while it is plugged in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gpu {
    pub vendor: Vendor,
    /// NVIDIA's name for it (`NVIDIA GeForce RTX 5060 Ti`); for AMD's, libdrm's marketing name where
    /// its table has one, else the PCI ids as lspci prints them (`1002:13c0 rev e5`).
    pub model: String,
    /// Its memory: NVIDIA's framebuffer; AMD's VRAM, which on an APU is the carve-out the firmware
    /// gives it (the system memory it maps besides is not counted).
    pub memory_total_bytes: u64,
    pub pci: PciAddress,
    /// NVIDIA's driver's version (`620.70.06`); amdgpu's: a DKMS module's own, else the kernel's
    /// release, the in-tree driver's.
    pub driver_version: String,
}

/// One GPU's load at one sample. Each counter is read on its own, so one can fail while the other
/// reads (a MIG-enabled NVIDIA GPU has no utilisation, an asleep AMD one no busy percent; both have
/// their memory).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Load {
    /// The percent of the time the GPU is busy, 0 to 100: for NVIDIA, of NVML's last sample period
    /// (1/6 s to 1 s by product) that a kernel ran; for AMD, the SMU firmware's load over the GPU's IP
    /// cores (`gpu_busy_percent`).
    pub busy_percent: Result<u8, Unread>,
    /// The bytes of its memory in use: NVML's `used` (allocated and reserved), amdgpu's
    /// `mem_info_vram_used`.
    pub memory_used_bytes: Result<u64, Unread>,
}

/// Why a counter wasn't read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Unread {
    /// The GPU is gone: unplugged, fallen off the bus, or its driver unbound.
    #[error("the GPU is gone: unplugged, fallen off the bus or unbound from its driver")]
    Gone,
    /// The GPU is runtime-suspended: it runs nothing, and amdgpu answers its busy counter only awake.
    #[error("the GPU is asleep (runtime-suspended): it runs nothing")]
    Asleep,
    /// The driver doesn't offer this counter on this GPU.
    #[error("its driver doesn't offer this on this GPU")]
    Unsupported,
    /// What the driver answered isn't the value it documents (a percent past 100, a line that isn't a
    /// number, a name that isn't UTF-8).
    #[error("its driver answered with something that isn't the value it documents")]
    Malformed,
    /// NVML refused, with its `nvmlReturn_t`.
    #[error("NVML refused: {}", nvml_says(.code))]
    Nvml { code: i32 },
    /// The OS refused the read, with its error number (0: the error carried none).
    #[error("the OS refused the read: error {errno}")]
    Os { errno: i32 },
}

/// A device a driver counts that [`Gpus::open`] couldn't read, and why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skipped {
    /// NVML's device at this index (`nvmlDeviceGetHandleByIndex_v2`'s numbering).
    Nvml { index: u32, why: Unread },
    /// The DRM class's `card<N>` (`/sys/class/drm`).
    Drm { card: u32, why: Unread },
}

/// Why one vendor's GPUs aren't read on this node.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Absent {
    /// This platform has no interface Hex reads for them.
    #[error("this platform has no interface Hex reads for them")]
    Unsupported,
    /// Their library isn't installed: the names tried, and the loader's last error.
    #[error("their library isn't installed (tried {tried:?}: {error})")]
    NoLibrary {
        tried: &'static [&'static str],
        error: String,
    },
    /// Their library lacks an entry point Hex calls.
    #[error("their library has no {name}")]
    NoSymbol { name: &'static str },
    /// NVML refused to start or to list, with its `nvmlReturn_t`.
    #[error("NVML refused: {}", nvml_says(.code))]
    Nvml { code: i32 },
    /// Their devices couldn't be listed: the OS's error number.
    #[error("listing them failed: OS error {errno}")]
    Os { errno: i32 },
    /// The driver answered with something it doesn't document.
    #[error("their driver answered with something it doesn't document")]
    Malformed,
    /// The interface works and lists none this process can read ([`Gpus::skipped`] has any it saw).
    #[error("none is present that this process can read")]
    NoGpu,
}

/// The node has no GPU this process can read: why, for each vendor.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("no GPU to read: NVIDIA: {nvidia}; AMD: {amd}")]
pub struct NoGpus {
    pub nvidia: Absent,
    pub amd: Absent,
}

/// What nvml.h says each `nvmlReturn_t` means, for a person reading a refusal.
fn nvml_says(code: &i32) -> String {
    let says = match *code {
        1 => "NVML isn't initialised",
        2 => "an argument is invalid",
        3 => "the device doesn't support it",
        4 => "no permission",
        6 => "not found",
        9 => "NVIDIA's driver isn't loaded",
        12 => "NVML's library wasn't found",
        13 => "this NVML lacks the function",
        15 => "the GPU fell off the bus",
        17 => "the operating system or a cgroup blocks the GPU",
        18 => "the driver and NVML's library are different versions",
        _ => "",
    };
    if says.is_empty() {
        format!("nvmlReturn_t {code}")
    } else {
        format!("nvmlReturn_t {code} ({says})")
    }
}

/// A node's GPUs, found once, with every counter [`Gpus::sample`] reads kept open.
#[derive(Debug)]
pub struct Gpus {
    gpus: Vec<Gpu>,
    loads: Vec<Load>,
    skipped: Vec<Skipped>,
    /// Why there are none: set exactly when `gpus` is empty.
    none: Option<NoGpus>,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    nvidia: Vec<nvml::Device<&'static nvml::sys::Api>>,
    #[cfg(target_os = "linux")]
    amd: Vec<amd::Card<std::fs::File>>,
}

impl Gpus {
    /// Finds the node's GPUs: NVML loaded and initialised (once for the process; a failure is tried
    /// again at the next open), amdgpu's cards listed, each GPU's identity read and its counters opened.
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    pub fn open() -> Gpus {
        let mut gpus = Vec::new();
        let mut skipped = Vec::new();

        let (nvidia, nvidia_absent) = {
            let found = nvml::load::nvml()
                .map_err(nvml::absent)
                .and_then(nvml::open);
            match found {
                Ok(found) => {
                    skipped.extend(found.skipped);
                    let (found, devices): (Vec<_>, Vec<_>) = found.gpus.into_iter().unzip();
                    gpus.extend(found);
                    (devices, Absent::NoGpu)
                }
                Err(why) => (Vec::new(), why),
            }
        };

        #[cfg(target_os = "linux")]
        let (amd, amd_absent) = match amd::open(&amd::Paths::system()) {
            Ok(found) => {
                skipped.extend(found.skipped);
                let (found, cards): (Vec<_>, Vec<_>) = found.gpus.into_iter().unzip();
                gpus.extend(found);
                (cards, Absent::NoGpu)
            }
            Err(why) => (Vec::new(), why),
        };
        #[cfg(not(target_os = "linux"))]
        let amd_absent = Absent::Unsupported;

        let none = if gpus.is_empty() {
            Some(NoGpus {
                nvidia: nvidia_absent,
                amd: amd_absent,
            })
        } else {
            None
        };
        Gpus {
            loads: Vec::with_capacity(gpus.len()),
            gpus,
            skipped,
            none,
            nvidia,
            #[cfg(target_os = "linux")]
            amd,
        }
    }

    /// Finds the node's GPUs: none here, where neither vendor's interface exists (NVIDIA's driver and
    /// amdgpu are Linux's and Windows').
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    pub fn open() -> Gpus {
        Gpus {
            gpus: Vec::new(),
            loads: Vec::new(),
            skipped: Vec::new(),
            none: Some(NoGpus {
                nvidia: Absent::Unsupported,
                amd: Absent::Unsupported,
            }),
        }
    }

    /// The GPUs found at open: NVIDIA's, then AMD's, each vendor's in PCI order. Or why there are none.
    pub fn inventory(&self) -> Result<&[Gpu], &NoGpus> {
        match &self.none {
            Some(none) => Err(none),
            None => Ok(&self.gpus),
        }
    }

    /// Each GPU's load now, in [`Gpus::inventory`]'s order: one NVML call or one read of a kept-open
    /// counter per counter, into room reserved at open. Or why there are none.
    pub fn sample(&mut self) -> Result<&[Load], &NoGpus> {
        if let Some(none) = &self.none {
            return Err(none);
        }
        // Within the capacity reserved at open, one load per GPU: no push here reallocates.
        self.loads.clear();
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        for device in &self.nvidia {
            self.loads.push(device.sample());
        }
        #[cfg(target_os = "linux")]
        for card in &self.amd {
            self.loads.push(card.sample());
        }
        Ok(&self.loads)
    }

    /// The devices a driver counts that open couldn't read, each with why.
    pub fn skipped(&self) -> &[Skipped] {
        &self.skipped
    }
}
