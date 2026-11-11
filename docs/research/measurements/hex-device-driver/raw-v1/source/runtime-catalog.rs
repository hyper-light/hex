//! AC-T7.4.1: backend-owned physical bindings. Records remain the holder authority.
//! The catalog is immutable configuration, not a second admission ledger or discovery cache.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use hex_api::{BoxId, BoxSpec, DeviceId, DeviceInfo, DeviceKind, Refusal};
use hex_core::record::BoxRecord;

/// A trusted administrator binds one physical resource to an existing native adapter.
/// CDI hooks/paths and HCS location paths are backend configuration, never peer payloads.
pub enum Binding {
    Cdi { file: String, qualified: String },
    Hcs { location: String },
}

pub struct Entry {
    pub info: DeviceInfo,
    pub binding: Binding,
}

impl Entry {
    pub fn report(&self, holder: Option<&BoxId>) -> Result<DeviceInfo, Refusal> {
        Ok(DeviceInfo {
            id: self.info.id,
            model: copy(&self.info.model)?,
            size: self.info.size,
            holder: holder.map(BoxId::try_clone).transpose()?,
        })
    }
}

#[derive(Default)]
pub struct Catalog {
    entries: Vec<Entry>,
}

/// Parsed configuration for one create. No background registry, watcher or mutable copy is kept.
pub struct CdiPlan<'a> {
    specs: Vec<(hex_core::cdi::Spec, Vec<&'a str>)>,
}

pub type Records = BTreeMap<BoxId, Result<BoxRecord, String>>;

fn invalid(why: &'static str) -> Refusal {
    Refusal::Invalid {
        what: "runtime device catalog",
        why: why.into(),
    }
}

fn allocation(bound: usize) -> Refusal {
    Refusal::Exhausted {
        what: "runtime device catalog bytes",
        bound,
    }
}

fn copy(value: &str) -> Result<String, Refusal> {
    let mut owned = String::new();
    owned
        .try_reserve_exact(value.len())
        .map_err(|_| allocation(value.len()))?;
    owned.push_str(value);
    Ok(owned)
}

impl Catalog {
    /// An absent catalog advertises no attachment capability. Read at backend startup, so a
    /// configuration change takes effect on an ordinary backend restart, never halfway through create.
    pub fn open(path: &Path) -> Result<Self, Refusal> {
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(Refusal::os("opening runtime device catalog", error)),
        };
        // One excess byte distinguishes an exact-bound file from a truncated oversized file.
        // Reserve before reading; even a concurrently growing file cannot trigger an implicit growth.
        let bound = hex_core::bundle::MAX_CONFIG_BYTES;
        let room = bound.checked_add(1).ok_or_else(|| allocation(bound))?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(room)
            .map_err(|_| allocation(room))?;
        bytes.resize(room, 0);
        let mut used = 0usize;
        while used < room {
            let tail = bytes.get_mut(used..).ok_or_else(|| allocation(room))?;
            match file.read(tail) {
                Ok(0) => break,
                Ok(count) => used = used.checked_add(count).ok_or_else(|| allocation(room))?,
                Err(error) => return Err(Refusal::os("reading runtime device catalog", error)),
            }
        }
        if used > bound {
            return Err(invalid("configuration exceeds the OCI config byte bound"));
        }
        bytes.truncate(used);
        Self::parse(&bytes)
    }

    /// A flat, versioned administrator file; tabs delimit seven fields and cannot occur in values.
    /// This finite syntax avoids an arbitrary-depth JSON tree and fallible-string gaps in this path.
    pub fn parse(bytes: &[u8]) -> Result<Self, Refusal> {
        if bytes.len() > hex_core::bundle::MAX_CONFIG_BYTES {
            return Err(invalid("configuration exceeds the OCI config byte bound"));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| invalid("configuration is not UTF-8"))?;
        if text.contains('\0') {
            return Err(invalid("configuration contains NUL"));
        }
        let mut lines = text.lines();
        if lines.next() != Some("hex-devices-1") {
            return Err(invalid("unsupported or absent configuration version"));
        }
        let mut catalog = Self::default();
        for line in lines {
            // The existing scheduler uses u16 device indices; it cannot represent another entry.
            if catalog.entries.len() > usize::from(u16::MAX) {
                return Err(invalid(
                    "configuration exceeds the scheduler device index space",
                ));
            }
            let mut fields = line.split('\t');
            let kind = match fields.next() {
                Some("gpu") => DeviceKind::Gpu,
                Some("vfio") => DeviceKind::Vfio,
                Some("block") => DeviceKind::Block,
                Some("net") => DeviceKind::Net,
                _ => return Err(invalid("unknown device kind")),
            };
            let key = fields
                .next()
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or_else(|| invalid("invalid physical device key"))?;
            let model = fields
                .next()
                .ok_or_else(|| invalid("missing model field"))?;
            let size = fields
                .next()
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or_else(|| invalid("invalid device size"))?;
            let adapter = fields
                .next()
                .ok_or_else(|| invalid("missing adapter field"))?;
            let source = fields
                .next()
                .ok_or_else(|| invalid("missing binding source"))?;
            let selector = fields
                .next()
                .ok_or_else(|| invalid("missing selector field"))?;
            if fields.next().is_some() {
                return Err(invalid("excess configuration fields"));
            }
            let id = DeviceId { kind, key };
            id.check()?;
            let binding = match adapter {
                "cdi" if source.starts_with('/') && !selector.is_empty() => Binding::Cdi {
                    file: copy(source)?,
                    qualified: copy(selector)?,
                },
                "hcs" if !source.is_empty() && selector.is_empty() && kind == DeviceKind::Gpu => {
                    Binding::Hcs {
                        location: copy(source)?,
                    }
                }
                _ => return Err(invalid("unsupported or malformed native binding")),
            };
            let info = DeviceInfo {
                id,
                model: copy(model)?,
                size,
                holder: None,
            };
            info.check()?;
            catalog
                .entries
                .try_reserve(1)
                .map_err(|_| allocation(bytes.len()))?;
            catalog.entries.push(Entry { info, binding });
        }
        let mut ids = Vec::new();
        ids.try_reserve_exact(catalog.entries.len())
            .map_err(|_| allocation(catalog.entries.len()))?;
        ids.extend(catalog.entries.iter().map(|entry| entry.info.id));
        ids.sort_unstable();
        if ids.windows(2).any(|pair| pair.first() == pair.last()) {
            return Err(invalid("duplicate physical device identity"));
        }
        Ok(catalog)
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Each file is read once per create; all selected files share the native config byte budget.
    /// A spec's globals are selected once, with its requested devices in request order.
    pub fn cdi<'a>(&'a self, devices: &[DeviceId]) -> Result<CdiPlan<'a>, Refusal> {
        if devices.len() > hex_api::MAX_DEVICES {
            return Err(invalid(
                "selection exceeds the scheduler device index space",
            ));
        }
        let mut groups: Vec<(&str, Vec<&str>)> = Vec::new();
        groups
            .try_reserve_exact(devices.len())
            .map_err(|_| allocation(devices.len()))?;
        for id in devices {
            let entry = self
                .entries
                .iter()
                .find(|entry| entry.info.id == *id)
                .ok_or_else(|| invalid("requested physical device has no native binding"))?;
            let Binding::Cdi { file, qualified } = &entry.binding else {
                return Err(Refusal::Platform {
                    what: "device attachment",
                    platform: "Linux OCI",
                    why: "selected binding is not CDI".into(),
                });
            };
            if let Some((_, names)) = groups.iter_mut().find(|(path, _)| *path == file.as_str()) {
                names
                    .try_reserve(1)
                    .map_err(|_| allocation(devices.len()))?;
                names.push(qualified);
            } else {
                let mut names = Vec::new();
                names.try_reserve_exact(1).map_err(|_| allocation(1))?;
                names.push(qualified.as_str());
                groups.push((file, names));
            }
        }
        let mut specs = Vec::new();
        specs
            .try_reserve_exact(groups.len())
            .map_err(|_| allocation(groups.len()))?;
        let mut left = hex_core::bundle::MAX_CONFIG_BYTES;
        for (path, names) in groups {
            let bytes = read_bounded(Path::new(path), left)?;
            left = left
                .checked_sub(bytes.len())
                .ok_or_else(|| allocation(left))?;
            let parsed = hex_core::cdi::Spec::parse(&bytes, bytes.len()).map_err(|error| {
                Refusal::Invalid {
                    what: "CDI device configuration",
                    why: error.to_string(),
                }
            })?;
            specs.push((parsed, names));
        }
        Ok(CdiPlan { specs })
    }

    /// Creating, Ready and Deleting records all retain the resource. Only physical removal followed
    /// by record removal makes it free. Malformed records cannot be treated as empty capacity.
    pub fn holder<'a>(
        &self,
        id: DeviceId,
        records: &'a Records,
    ) -> Result<Option<&'a BoxId>, Refusal> {
        let mut holder = None;
        for (box_id, record) in records {
            let record = record.as_ref().map_err(|_| {
                Refusal::Broken(
                    "cannot establish device holders from a malformed box record".into(),
                )
            })?;
            if record.spec.id != *box_id || record.spec.check().is_err() {
                return Err(Refusal::Broken(
                    "cannot establish device holders from an inconsistent box record".into(),
                ));
            }
            if record.spec.devices.contains(&id) {
                if holder.is_some() {
                    return Err(Refusal::Broken(
                        "a physical device has multiple recorded holders".into(),
                    ));
                }
                holder = Some(box_id);
            }
        }
        Ok(holder)
    }

    /// Called while the existing per-device turns are held, before a Creating record or OS effects.
    /// It grants no new authority: an unknown identity or any retained holder refuses the create.
    pub fn prepare<'a>(
        &'a self,
        spec: &BoxSpec,
        records: &Records,
    ) -> Result<Vec<&'a Entry>, Refusal> {
        spec.check()?;
        let mut selected = Vec::new();
        selected
            .try_reserve_exact(spec.devices.len())
            .map_err(|_| allocation(spec.devices.len()))?;
        for id in &spec.devices {
            let entry = self
                .entries
                .iter()
                .find(|entry| entry.info.id == *id)
                .ok_or_else(|| invalid("requested physical device has no native binding"))?;
            if self.holder(*id, records)?.is_some() {
                return Err(invalid("requested physical device is already held"));
            }
            selected.push(entry);
        }
        Ok(selected)
    }
}

impl CdiPlan<'_> {
    pub fn edits(&self) -> Result<hex_core::cdi::Edits<'_>, Refusal> {
        let mut all = hex_core::cdi::Edits::default();
        for (spec, names) in &self.specs {
            let edits = spec
                .select(names, names.len())
                .map_err(|error| Refusal::Invalid {
                    what: "CDI device selection",
                    why: error.to_string(),
                })?;
            append(&mut all.env, edits.env)?;
            append(&mut all.device_nodes, edits.device_nodes)?;
            append(&mut all.mounts, edits.mounts)?;
            append(&mut all.hooks, edits.hooks)?;
            append(&mut all.net_devices, edits.net_devices)?;
            append(&mut all.additional_gids, edits.additional_gids)?;
            if edits.intel_rdt.is_some() {
                all.intel_rdt = edits.intel_rdt;
            }
        }
        Ok(all)
    }
}

fn append<T>(to: &mut Vec<T>, from: Vec<T>) -> Result<(), Refusal> {
    to.try_reserve_exact(from.len())
        .map_err(|_| allocation(from.len()))?;
    to.extend(from);
    Ok(())
}

fn read_bounded(path: &Path, bound: usize) -> Result<Vec<u8>, Refusal> {
    let mut file =
        File::open(path).map_err(|error| Refusal::os("opening CDI device configuration", error))?;
    let room = bound.checked_add(1).ok_or_else(|| allocation(bound))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(room)
        .map_err(|_| allocation(room))?;
    bytes.resize(room, 0);
    let mut used = 0usize;
    while used < room {
        let tail = bytes.get_mut(used..).ok_or_else(|| allocation(room))?;
        let count = file
            .read(tail)
            .map_err(|error| Refusal::os("reading CDI device configuration", error))?;
        if count == 0 {
            break;
        }
        used = used.checked_add(count).ok_or_else(|| allocation(room))?;
    }
    if used > bound {
        return Err(invalid(
            "selected CDI input exceeds the native config byte budget",
        ));
    }
    bytes.truncate(used);
    Ok(bytes)
}

#[cfg(test)]
mod tests;
