//! AC-T7.4.1: the native Linux consumer of the scheduler's exact, durable device claims.

use super::*;
use hex_api::{DeviceId, DeviceKind};
use hex_core::cdi::{DeviceType, Edits};
use hex_runtime::devices::{Binding, Catalog};

impl Node {
    pub(super) fn hold_attachments(&self, devices: &[DeviceId]) -> Result<Vec<Turn<'_>>, Refusal> {
        let mut ordered = Vec::new();
        ordered
            .try_reserve_exact(devices.len())
            .map_err(|_| full(devices.len()))?;
        ordered.extend_from_slice(devices);
        ordered.sort_unstable();
        let mut held = Vec::new();
        held.try_reserve_exact(ordered.len())
            .map_err(|_| full(ordered.len()))?;
        for device in ordered {
            let id = BoxId::new(&format!("device-{}-{}", device.kind.name(), device.key))?;
            held.push(self.attachment_turns.take(&id)?);
        }
        Ok(held)
    }

    pub(super) fn attachment_capacity(&self) -> Result<Vec<hex_api::DeviceInfo>, Refusal> {
        if self.attachments.entries().is_empty() {
            return Ok(Vec::new());
        }
        let records = self.store.all()?;
        let mut reports = Vec::new();
        reports
            .try_reserve_exact(self.attachments.entries().len())
            .map_err(|_| full(self.attachments.entries().len()))?;
        for entry in self.attachments.entries() {
            // The native Linux runtime implements CDI nodes/mounts/hooks, not an HCS mapping.
            if !matches!(entry.binding, Binding::Cdi { .. })
                || entry.info.id.kind == DeviceKind::Net
            {
                return Err(Refusal::Platform {
                    what: "device attachment",
                    platform: "Linux OCI",
                    why: "configured native binding is unsupported".into(),
                });
            }
            let holder = self.attachments.holder(entry.info.id, &records)?;
            reports.push(entry.report(holder)?);
        }
        Ok(reports)
    }

    pub(super) fn render_attachments(
        &self,
        spec: &OciSpec,
        place: &Place<'_>,
        devices: &[DeviceId],
    ) -> Result<Bundle, Refusal> {
        let plan = self.attachments.cdi(devices)?;
        let edits = plan.edits()?;
        let resolved = resolve(&edits)?;
        bundle::render_with_devices(spec, place, &edits, &resolved)
    }
}

fn full(bound: usize) -> Refusal {
    Refusal::Exhausted {
        what: "native device attachment items",
        bound,
    }
}

fn resolve<'a>(edits: &Edits<'a>) -> Result<Vec<bundle::ResolvedNode<'a>>, Refusal> {
    let mut nodes = Vec::new();
    nodes
        .try_reserve_exact(edits.device_nodes.len())
        .map_err(|_| full(edits.device_nodes.len()))?;
    for node in &edits.device_nodes {
        let path = node
            .host_path
            .as_deref()
            .filter(|path| !path.is_empty())
            .unwrap_or(&node.path);
        let stat = rustix::fs::stat(path)
            .map_err(|error| Refusal::os("reading a selected CDI device", error))?;
        let kind = match rustix::fs::FileType::from_raw_mode(stat.st_mode) {
            rustix::fs::FileType::CharacterDevice => DeviceType::Char,
            rustix::fs::FileType::BlockDevice => DeviceType::Block,
            rustix::fs::FileType::Fifo => DeviceType::Fifo,
            _ => {
                return Err(Refusal::Invalid {
                    what: "CDI device node",
                    why: "selected host object is not a character, block or FIFO device".into(),
                });
            }
        };
        nodes.push(bundle::ResolvedNode {
            node,
            kind,
            major: rustix::fs::major(stat.st_rdev),
            minor: rustix::fs::minor(stat.st_rdev),
            // POSIX permission/setid/sticky bits; file type is represented separately above.
            file_mode: stat.st_mode & 0o7777,
        });
    }
    Ok(nodes)
}

pub(super) fn catalog(path: &Path) -> Result<(Catalog, Turns), Refusal> {
    let catalog = Catalog::open(path)?;
    let turns = Turns::new(
        "assigning or removing a physical device",
        "physical device changes under way",
        catalog.entries().len(),
        Patience::measuring("node.turns"),
    );
    Ok((catalog, turns))
}
