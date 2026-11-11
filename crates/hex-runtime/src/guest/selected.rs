//! AC-T7.4.1: a dedicated GPU guest's immutable launch receipt. Box records remain the
//! device-holder authority; this receipt names the VM which must end before that holder
//! can be released. An intent with no process identity is uncertainty, never absence.

use std::io::Read;
use std::path::{Path, PathBuf};

use hex_api::{BoxId, BoxSpec, CapacityResources, Class, DeviceId, DeviceKind, Refusal};

use super::{GuestConfig, held};

const HEADER: &str = "hex-selected-guest 1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Phase {
    Intent,
    Running(shards_api::Identity),
    Stopping(shards_api::Identity),
    Released {
        identity: Option<shards_api::Identity>,
        status: shards_api::ManagedStatus,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Record {
    pub id: BoxId,
    pub devices: Vec<DeviceId>,
    pub class: Class,
    pub physical: Vec<shards_api::PhysicalGpu>,
    pub vcpus: u32,
    pub memory_mib: u64,
    pub phase: Phase,
}

fn invalid(why: impl Into<String>) -> Refusal {
    Refusal::Invalid {
        what: "selected GPU guest",
        why: why.into(),
    }
}

fn allocation(bound: usize) -> Refusal {
    Refusal::Exhausted {
        what: "selected GPU guest record",
        bound,
    }
}

/// The VM's configured envelope, charged by the caller's existing admission owner before
/// boot. MiB and CPU milli are unit conversions, not an estimated overhead allowance.
pub fn resources(config: &GuestConfig) -> Result<CapacityResources, Refusal> {
    Ok(CapacityResources {
        cpu_milli: u64::from(config.vcpus)
            .checked_mul(1000)
            .ok_or_else(|| invalid("configured CPU milli overflows"))?,
        mem_bytes: config
            .memory_mib
            .checked_mul(1 << 20)
            .ok_or_else(|| invalid("configured memory bytes overflow"))?,
        ..CapacityResources::default()
    })
}

impl Record {
    pub fn intent(spec: &BoxSpec, config: &GuestConfig) -> Result<Self, Refusal> {
        let receipt = Self::retained(spec)?;
        let configured = resources(config)?;
        if u64::from(receipt.vcpus).checked_mul(1000) != Some(configured.cpu_milli)
            || receipt.memory_mib.checked_mul(1 << 20) != Some(configured.mem_bytes)
        {
            return Err(invalid("admitted VM disagrees with new configured VM"));
        }
        Ok(receipt)
    }

    pub fn retained(spec: &BoxSpec) -> Result<Self, Refusal> {
        spec.check()?;
        let reserved = spec
            .allocation
            .ok_or_else(|| invalid("dedicated VM has no original allocation"))?
            .guest()?;
        if reserved.cpu_milli.checked_rem(1000) != Some(0)
            || reserved.mem_bytes.checked_rem(1 << 20) != Some(0)
        {
            return Err(invalid(
                "admitted VM resources are not exact native CPU/MiB units",
            ));
        }
        let vcpus = u32::try_from(
            reserved
                .cpu_milli
                .checked_div(1000)
                .ok_or_else(|| invalid("CPU unit conversion failed"))?,
        )
        .map_err(|_| invalid("admitted vCPUs exceed their native representation"))?;
        let memory_mib = reserved
            .mem_bytes
            .checked_div(1 << 20)
            .ok_or_else(|| invalid("memory unit conversion failed"))?;
        if (!matches!(spec.class.class(), Class::Oci | Class::Shards))
            || (spec.devices.is_empty() && spec.class.class() != Class::Shards)
            || spec.devices.iter().any(|id| id.kind != DeviceKind::Gpu)
        {
            return Err(invalid("a dedicated guest requires exact GPU claims"));
        }
        if spec
            .bounds
            .cpu_max_milli
            .is_none_or(|cpu| u64::from(cpu) > reserved.cpu_milli)
            || spec
                .bounds
                .memory_max
                .is_none_or(|mem| mem > reserved.mem_bytes)
        {
            return Err(invalid("box bounds must fit the configured VM envelope"));
        }
        let mut devices = Vec::new();
        devices
            .try_reserve_exact(spec.devices.len())
            .map_err(|_| allocation(spec.devices.len()))?;
        devices.extend_from_slice(&spec.devices);
        devices.sort_unstable();
        Ok(Self {
            id: spec.id.try_clone()?,
            devices,
            class: spec.class.class(),
            physical: Vec::new(),
            vcpus,
            memory_mib,
            phase: Phase::Intent,
        })
    }

    pub fn matches(&self, spec: &BoxSpec) -> Result<(), Refusal> {
        spec.check()?;
        let allocation = spec
            .allocation
            .ok_or_else(|| invalid("dedicated record lost its admission receipt"))?;
        let guest = allocation.guest()?;
        let cpu = u64::from(self.vcpus)
            .checked_mul(1000)
            .ok_or_else(|| invalid("receipt CPU overflows"))?;
        let memory = self
            .memory_mib
            .checked_mul(1 << 20)
            .ok_or_else(|| invalid("receipt memory overflows"))?;
        if self.id != spec.id
            || self.class != spec.class.class()
            || self.devices.len() != spec.devices.len()
            || !self
                .devices
                .iter()
                .all(|device| spec.devices.contains(device))
            || guest.cpu_milli != cpu
            || guest.mem_bytes != memory
        {
            return Err(invalid(
                "recovery disagrees with its original admission and launch receipt",
            ));
        }
        Ok(())
    }

    pub fn identity(&self) -> Result<&shards_api::Identity, Refusal> {
        match &self.phase {
            Phase::Running(identity)
            | Phase::Stopping(identity)
            | Phase::Released {
                identity: Some(identity),
                ..
            } => Ok(identity),
            Phase::Intent | Phase::Released { identity: None, .. } => Err(invalid(
                "a launch intent has no recorded process identity; physical absence is unproved",
            )),
        }
    }

    fn encode(&self) -> Result<String, Refusal> {
        let bound = hex_core::bundle::MAX_CONFIG_BYTES;
        let mut length = Length { bytes: 0, bound };
        self.render(&mut length).map_err(|_| allocation(bound))?;
        let mut text = String::new();
        text.try_reserve_exact(length.bytes)
            .map_err(|_| allocation(bound))?;
        self.render(&mut text).map_err(|_| allocation(bound))?;
        if text.len() > bound || parse(&text)? != *self {
            return Err(invalid("receipt is oversized or cannot be encoded exactly"));
        }
        Ok(text)
    }

    fn render(&self, text: &mut impl std::fmt::Write) -> std::fmt::Result {
        writeln!(text, "{HEADER}")?;
        writeln!(text, "box {}", self.id)?;
        writeln!(text, "class {}", self.class.name())?;
        writeln!(text, "vcpus {}", self.vcpus)?;
        writeln!(text, "memory_mib {}", self.memory_mib)?;
        writeln!(text, "devices {}", self.devices.len())?;
        if self.devices.len() != self.physical.len() {
            return Err(std::fmt::Error);
        }
        for (id, physical) in self.devices.iter().zip(&self.physical) {
            writeln!(text, "gpu {}", id.key)?;
            writeln!(text, "native {physical}")?;
        }
        match &self.phase {
            Phase::Intent => writeln!(text, "intent"),
            Phase::Released {
                identity: None,
                status,
            } => {
                writeln!(text, "released-unpublished")?;
                render_status(text, *status)
            }
            Phase::Running(id)
            | Phase::Stopping(id)
            | Phase::Released {
                identity: Some(id), ..
            } => {
                let phase = match self.phase {
                    Phase::Running(_) => "running",
                    Phase::Released { .. } => "released",
                    _ => "stopping",
                };
                writeln!(text, "{phase}")?;
                writeln!(text, "pid {}", id.pid)?;
                writeln!(text, "started {}", id.started)?;
                writeln!(text, "vsock {}", id.vsock.display())?;
                if let Phase::Released { status, .. } = self.phase {
                    render_status(text, status)?;
                }
                Ok(())
            }
        }
    }
}

fn render_status(
    out: &mut impl std::fmt::Write,
    status: shards_api::ManagedStatus,
) -> std::fmt::Result {
    match status {
        shards_api::ManagedStatus::Code(code) => writeln!(out, "status code {code}"),
        shards_api::ManagedStatus::Signal(signal) => writeln!(out, "status signal {signal}"),
        shards_api::ManagedStatus::Unknown => writeln!(out, "status unknown"),
    }
}

fn parse_status(line: Option<&str>) -> Result<shards_api::ManagedStatus, Refusal> {
    let value = field(line, "status ")?;
    if value == "unknown" {
        return Ok(shards_api::ManagedStatus::Unknown);
    }
    let (kind, value) = value
        .split_once(' ')
        .ok_or_else(|| invalid("missing terminal status"))?;
    let number = value
        .parse::<i32>()
        .map_err(|_| invalid("invalid native terminal status"))?;
    match kind {
        "code" => Ok(shards_api::ManagedStatus::Code(number)),
        "signal" => Ok(shards_api::ManagedStatus::Signal(number)),
        _ => Err(invalid("unknown native terminal status kind")),
    }
}

struct Length {
    bytes: usize,
    bound: usize,
}

impl std::fmt::Write for Length {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        self.bytes = self
            .bytes
            .checked_add(value.len())
            .filter(|bytes| *bytes <= self.bound)
            .ok_or(std::fmt::Error)?;
        Ok(())
    }
}

pub(super) fn write(path: &Path, record: &Record) -> Result<(), Refusal> {
    let text = record.encode()?;
    held::replace(path, text.as_bytes())
}

pub(super) fn read(path: &Path) -> Result<Option<Record>, Refusal> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Refusal::os("opening selected guest receipt", error)),
    };
    let bound = hex_core::bundle::MAX_CONFIG_BYTES;
    let room = bound.checked_add(1).ok_or_else(|| allocation(bound))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(room)
        .map_err(|_| allocation(room))?;
    bytes.resize(room, 0);
    let mut read = 0usize;
    let mut file = file.take(u64::try_from(room).map_err(|_| allocation(room))?);
    while read < room {
        let tail = bytes.get_mut(read..).ok_or_else(|| allocation(room))?;
        let count = file
            .read(tail)
            .map_err(|e| Refusal::os("reading selected guest receipt", e))?;
        if count == 0 {
            break;
        }
        read = read.checked_add(count).ok_or_else(|| allocation(room))?;
    }
    if read > bound {
        return Err(invalid("receipt exceeds the native configuration bound"));
    }
    let text = std::str::from_utf8(bytes.get(..read).ok_or_else(|| allocation(room))?)
        .map_err(|_| invalid("receipt is not UTF-8"))?;
    parse(text).map(Some)
}

pub(super) fn remove(path: &Path) -> Result<(), Refusal> {
    held::remove(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| invalid("receipt has no parent"))?;
    match held::sync_folder(parent) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Refusal::os("syncing selected guest receipt removal", e)),
    }
}

fn parse(text: &str) -> Result<Record, Refusal> {
    if text.len() > hex_core::bundle::MAX_CONFIG_BYTES || !text.ends_with('\n') {
        return Err(invalid("oversized or truncated receipt"));
    }
    let mut lines = text.lines();
    if lines.next() != Some(HEADER) {
        return Err(invalid("unknown receipt version"));
    }
    let id = BoxId::new(field(lines.next(), "box ")?)?;
    let class = match field(lines.next(), "class ")? {
        "oci" => Class::Oci,
        "shards" => Class::Shards,
        _ => return Err(invalid("receipt names another backend")),
    };
    let vcpus = field(lines.next(), "vcpus ")?
        .parse::<u32>()
        .map_err(|_| invalid("invalid vCPU count"))?;
    let memory_mib = field(lines.next(), "memory_mib ")?
        .parse::<u64>()
        .map_err(|_| invalid("invalid memory size"))?;
    let count = field(lines.next(), "devices ")?
        .parse::<usize>()
        .map_err(|_| invalid("invalid device count"))?;
    if (count == 0 && class != Class::Shards)
        || count > hex_api::MAX_DEVICES
        || vcpus == 0
        || memory_mib == 0
    {
        return Err(invalid("empty or excessive selected guest envelope"));
    }
    let mut devices = Vec::new();
    let mut physical = Vec::new();
    physical
        .try_reserve_exact(count)
        .map_err(|_| allocation(count))?;
    devices
        .try_reserve_exact(count)
        .map_err(|_| allocation(count))?;
    for _ in 0..count {
        let key = field(lines.next(), "gpu ")?
            .parse::<u64>()
            .map_err(|_| invalid("invalid GPU identity"))?;
        let id = DeviceId {
            kind: DeviceKind::Gpu,
            key,
        };
        if devices.last().is_some_and(|previous| *previous >= id) {
            return Err(invalid("GPU identities are repeated or unordered"));
        }
        devices.push(id);
        let native = field(lines.next(), "native ")?
            .parse::<shards_api::PhysicalGpu>()
            .map_err(|_| invalid("invalid native GPU selector"))?;
        physical.push(native);
    }
    shards_api::physical::check_selection(&physical)
        .map_err(|_| invalid("native selectors repeat or exceed their representation"))?;
    let phase = match lines.next() {
        Some("intent") => Phase::Intent,
        Some("released-unpublished") => Phase::Released {
            identity: None,
            status: parse_status(lines.next())?,
        },
        Some(phase @ ("running" | "stopping" | "released")) => {
            let pid = field(lines.next(), "pid ")?
                .parse::<u32>()
                .map_err(|_| invalid("invalid process identity"))?;
            let started = field(lines.next(), "started ")?
                .parse::<u64>()
                .map_err(|_| invalid("invalid process start"))?;
            let vsock = field(lines.next(), "vsock ")?;
            if pid == 0
                || started == 0
                || vsock.is_empty()
                || vsock.contains('\0')
                || vsock.contains('\r')
            {
                return Err(invalid("empty process identity or socket"));
            }
            let identity = shards_api::Identity {
                pid,
                started,
                vsock: {
                    let mut path = PathBuf::new();
                    path.try_reserve_exact(vsock.len())
                        .map_err(|_| allocation(vsock.len()))?;
                    path.push(vsock);
                    path
                },
            };
            match phase {
                "running" => Phase::Running(identity),
                "released" => Phase::Released {
                    identity: Some(identity),
                    status: parse_status(lines.next())?,
                },
                _ => Phase::Stopping(identity),
            }
        }
        _ => return Err(invalid("unknown launch phase")),
    };
    if lines.next().is_some() {
        return Err(invalid("receipt has trailing data"));
    }
    Ok(Record {
        id,
        devices,
        class,
        physical,
        vcpus,
        memory_mib,
        phase,
    })
}

fn field<'a>(line: Option<&'a str>, name: &str) -> Result<&'a str, Refusal> {
    line.and_then(|line| line.strip_prefix(name))
        .ok_or_else(|| invalid("missing or reordered receipt field"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> Record {
        Record {
            id: BoxId::new("selected-box").unwrap(),
            devices: vec![DeviceId {
                kind: DeviceKind::Gpu,
                key: 256,
            }],
            class: Class::Shards,
            physical: vec![shards_api::PhysicalGpu::Pci {
                domain: 0,
                bus: 1,
                device: 0,
                function: 0,
            }],
            vcpus: 1,
            memory_mib: 256,
            phase: Phase::Intent,
        }
    }

    /// AC-T7.4.1: every lifecycle phase retains the same exact claims and VM identity.
    #[test]
    fn durable_phases_preserve_exact_claims() {
        let identity = shards_api::Identity {
            pid: 42,
            started: 7,
            vsock: "/private/selected.sock".into(),
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipt");
        for phase in [
            Phase::Intent,
            Phase::Running(identity.clone()),
            Phase::Stopping(identity.clone()),
            Phase::Released {
                identity: Some(identity),
                status: shards_api::ManagedStatus::Signal(9),
            },
            Phase::Released {
                identity: None,
                status: shards_api::ManagedStatus::Unknown,
            },
        ] {
            let record = Record { phase, ..record() };
            write(&path, &record).unwrap();
            assert_eq!(read(&path).unwrap(), Some(record));
        }
        remove(&path).unwrap();
        assert_eq!(read(&path).unwrap(), None);
        assert!(record().identity().is_err());
    }

    /// AC-T7.4.1: torn/unknown/duplicate receipts cannot authorize a replacement VM.
    #[test]
    fn hostile_receipts_never_become_absence() {
        let text = record().encode().unwrap();
        for end in 0..text.len() {
            assert!(parse(text.get(..end).unwrap()).is_err());
        }
        for bad in [
            text.replace("devices 1\ngpu 256", "devices 2\ngpu 256\ngpu 256"),
            text.replace("devices 1", "devices 65537"),
            text.replace("intent", "running\npid 0\nstarted 7\nvsock /a"),
            format!("{text}trailing\n"),
        ] {
            assert!(parse(&bad).is_err());
        }
    }
}
