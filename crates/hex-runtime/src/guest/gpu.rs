//! The dedicated VM's boot authority and virtio-gpu render nodes. Only host-created
//! kernel parameters grant this mapping; a box payload, environment or CDI label
//! cannot assert that an arbitrary character device is a selected physical GPU.

#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};

use hex_api::{BoxId, BoxSpec, DeviceId, DeviceKind, Refusal};

/// Linux COMMAND_LINE_SIZE on supported x86_64/arm64, including its terminating
/// byte (arch/{x86,arm64}/include/{asm,uapi/asm}/setup.h).
pub const CMDLINE_BYTES: usize = 2048;
pub const CDI_KIND: &str = "hex.shards/gpu";

#[derive(Debug, PartialEq, Eq)]
pub struct Authority {
    pub owner: BoxId,
    pub devices: Vec<DeviceId>,
}

fn invalid(why: &'static str) -> Refusal {
    Refusal::Invalid {
        what: "guest GPU boot authority",
        why: why.into(),
    }
}

impl Authority {
    pub fn parse(cmdline: &str) -> Result<Option<Self>, Refusal> {
        if cmdline.len() >= CMDLINE_BYTES || cmdline.contains('\0') {
            return Err(invalid("kernel command line is oversized or contains NUL"));
        }
        let (mut owner, mut list) = (None, None);
        for word in cmdline.split_whitespace().take_while(|word| *word != "--") {
            if let Some(value) = word.strip_prefix("hex.gpu.box=") {
                if owner.replace(value).is_some() {
                    return Err(invalid("repeated GPU box parameter"));
                }
            } else if let Some(value) = word.strip_prefix("hex.gpu=") {
                if list.replace(value).is_some() {
                    return Err(invalid("repeated GPU selection parameter"));
                }
            }
        }
        let (owner, list) = match (owner, list) {
            (None, None) => return Ok(None),
            (Some(owner), Some(list)) if !list.is_empty() => (BoxId::new(owner)?, list),
            _ => {
                return Err(invalid(
                    "GPU box and nonempty exact selection must appear together",
                ));
            }
        };
        let mut devices = Vec::new();
        for key in list.split(',') {
            if devices.len() >= hex_api::MAX_DEVICES {
                return Err(invalid("too many GPU identities"));
            }
            let key = key
                .parse::<u64>()
                .map_err(|_| invalid("GPU key is not an unsigned native identity"))?;
            let id = DeviceId {
                kind: DeviceKind::Gpu,
                key,
            };
            if devices.last().is_some_and(|previous| *previous >= id) {
                return Err(invalid(
                    "GPU identities are repeated or not in canonical order",
                ));
            }
            devices.try_reserve(1).map_err(|_| Refusal::Exhausted {
                what: "guest GPU identities",
                bound: hex_api::MAX_DEVICES,
            })?;
            devices.push(id);
        }
        Ok(Some(Self { owner, devices }))
    }

    pub fn check(&self, spec: &BoxSpec) -> Result<(), Refusal> {
        spec.check()?;
        if spec.class.class() != hex_api::Class::Oci
            || spec.id != self.owner
            || spec.devices.len() != self.devices.len()
            || spec.devices.iter().any(|id| !self.devices.contains(id))
        {
            return Err(invalid(
                "request disagrees with the dedicated VM's exact owner or physical set",
            ));
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
pub fn boot_authority() -> Result<Option<Authority>, Refusal> {
    use std::io::Read;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(CMDLINE_BYTES)
        .map_err(|_| Refusal::Exhausted {
            what: "guest kernel command line",
            bound: CMDLINE_BYTES,
        })?;
    bytes.resize(CMDLINE_BYTES, 0);
    let mut file = std::fs::File::open("/proc/cmdline")
        .map_err(|e| Refusal::os("opening guest kernel command line", e))?;
    let mut used = 0usize;
    while used < CMDLINE_BYTES {
        let tail = bytes
            .get_mut(used..)
            .ok_or_else(|| invalid("kernel command line length overflows"))?;
        let count = file
            .read(tail)
            .map_err(|e| Refusal::os("reading guest kernel command line", e))?;
        if count == 0 {
            break;
        }
        used = used
            .checked_add(count)
            .ok_or_else(|| invalid("kernel command line length overflows"))?;
    }
    let text = std::str::from_utf8(
        bytes
            .get(..used)
            .ok_or_else(|| invalid("invalid command line length"))?,
    )
    .map_err(|_| invalid("kernel command line is not UTF-8"))?;
    Authority::parse(text)
}

/// Revalidate a resolved CDI node through the kernel's device identity, not its
/// path name or a copied ordinal. Used only with a matching dedicated authority.
#[cfg(target_os = "linux")]
pub fn check_render_node(major: u32, minor: u32) -> Result<(), Refusal> {
    let base = PathBuf::from(format!("/sys/dev/char/{major}:{minor}"));
    let subsystem = std::fs::canonicalize(base.join("subsystem"))
        .map_err(|e| Refusal::os("resolving guest render node subsystem", e))?;
    let driver = std::fs::canonicalize(base.join("device/driver"))
        .map_err(|e| Refusal::os("resolving guest render node driver", e))?;
    if subsystem.file_name().is_none_or(|name| name != "drm")
        || driver.file_name().is_none_or(|name| name != "virtio_gpu")
    {
        return Err(invalid(
            "selected guest node is not the virtio GPU DRM renderer",
        ));
    }
    let device =
        std::fs::canonicalize(&base).map_err(|e| Refusal::os("resolving guest DRM node", e))?;
    let render = device
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("renderD"));
    if render.is_none_or(|suffix| suffix.is_empty() || !suffix.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(invalid("selected guest node is not a DRM render node"));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
struct Node {
    path: String,
    major: u32,
    minor: u32,
}

#[cfg(target_os = "linux")]
struct Text(String);

#[cfg(target_os = "linux")]
impl Text {
    fn new() -> Result<Self, Refusal> {
        let mut text = String::new();
        text.try_reserve_exact(hex_core::bundle::MAX_CONFIG_BYTES)
            .map_err(|_| Refusal::Exhausted {
                what: "guest GPU configuration",
                bound: hex_core::bundle::MAX_CONFIG_BYTES,
            })?;
        Ok(Self(text))
    }
}

#[cfg(target_os = "linux")]
impl std::fmt::Write for Text {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        if self
            .0
            .len()
            .checked_add(value.len())
            .is_none_or(|len| len > hex_core::bundle::MAX_CONFIG_BYTES)
        {
            return Err(std::fmt::Error);
        }
        self.0.push_str(value);
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn render_nodes() -> Result<Vec<Node>, Refusal> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let mut nodes = Vec::new();
    let entries = std::fs::read_dir("/sys/class/drm")
        .map_err(|e| Refusal::os("listing guest DRM devices", e))?;
    for (seen, entry) in entries.enumerate() {
        if seen >= hex_api::MAX_DEVICES {
            return Err(invalid("guest DRM listing exceeds the device index space"));
        }
        let entry = entry.map_err(|e| Refusal::os("reading guest DRM entry", e))?;
        let name = entry.file_name();
        let Some(suffix) = name.to_str().and_then(|name| name.strip_prefix("renderD")) else {
            continue;
        };
        if suffix.is_empty() || !suffix.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid("malformed DRM render node name"));
        }
        let path = format!("/dev/dri/renderD{suffix}");
        let metadata =
            std::fs::metadata(&path).map_err(|e| Refusal::os("reading guest render node", e))?;
        if !metadata.file_type().is_char_device() {
            return Err(invalid("guest render path is not a character device"));
        }
        let major = rustix::fs::major(metadata.rdev());
        let minor = rustix::fs::minor(metadata.rdev());
        check_render_node(major, minor)?;
        nodes.try_reserve(1).map_err(|_| Refusal::Exhausted {
            what: "guest render nodes",
            bound: hex_api::MAX_DEVICES,
        })?;
        nodes.push(Node { path, major, minor });
    }
    if nodes.is_empty() {
        return Err(invalid(
            "dedicated GPU guest has no initialized virtio render node",
        ));
    }
    nodes.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    Ok(nodes)
}

/// PID 1 calls this after mounting proc/sys/dev and before starting its node. The
/// returned catalog is internal to this VM; it advertises no physical identity
/// based on the virtio node's ordinal. CPU-only guests write no GPU catalog.
#[cfg(target_os = "linux")]
pub fn initialize(state: &Path) -> Result<Option<Authority>, Refusal> {
    use std::fmt::Write as _;
    let Some(authority) = boot_authority()? else {
        return Ok(None);
    };
    let nodes = render_nodes()?;
    let cdi = state.join("shards-gpu.cdi");
    let file = cdi
        .to_str()
        .filter(|path| path.starts_with('/') && !path.contains(['\t', '\n', '\r', '\0']))
        .ok_or_else(|| invalid("guest catalog path is not an absolute flat UTF-8 field"))?;
    let mut catalog = Text::new()?;
    catalog
        .write_str("hex-devices-1\n")
        .map_err(|_| invalid("guest GPU catalog exceeds native config bytes"))?;
    for id in &authority.devices {
        writeln!(
            catalog,
            "gpu\t{}\tshards-renderer\t0\tguest-gpu\t{file}\t{CDI_KIND}={}",
            id.key, id.key
        )
        .map_err(|_| invalid("cannot encode guest GPU catalog"))?;
    }
    let mut json = Text::new()?;
    json.write_str("{\"cdiVersion\":\"1.1.0\",\"kind\":\"hex.shards/gpu\",\"devices\":[")
        .map_err(|_| invalid("guest GPU CDI exceeds native config bytes"))?;
    for (index, id) in authority.devices.iter().enumerate() {
        if index != 0 {
            json.write_char(',')
                .map_err(|_| invalid("guest GPU CDI exceeds native config bytes"))?;
        }
        write!(
            json,
            "{{\"name\":\"{}\",\"containerEdits\":{{\"deviceNodes\":[",
            id.key
        )
        .map_err(|_| invalid("cannot encode GPU CDI"))?;
        for (index, node) in nodes.iter().enumerate() {
            if index != 0 {
                json.write_char(',')
                    .map_err(|_| invalid("guest GPU CDI exceeds native config bytes"))?;
            }
            write!(json, "{{\"path\":\"{}\",\"type\":\"c\",\"major\":{},\"minor\":{},\"permissions\":\"rw\"}}", node.path, node.major, node.minor)
                .map_err(|_| invalid("cannot encode GPU CDI node"))?;
        }
        json.write_str("]}}")
            .map_err(|_| invalid("guest GPU CDI exceeds native config bytes"))?;
    }
    json.write_str("]}")
        .map_err(|_| invalid("guest GPU CDI exceeds native config bytes"))?;
    std::fs::create_dir_all(state).map_err(|e| Refusal::os("creating guest node state", e))?;
    super::held::replace(&cdi, json.0.as_bytes())?;
    super::held::replace(&state.join("devices"), catalog.0.as_bytes())?;
    Ok(Some(authority))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC-T7.4.1: only canonical boot parameters before -- bind one exact owner/set.
    #[test]
    fn boot_authority_is_exact_and_not_an_init_argument() {
        let parsed = Authority::parse(
            "console=ttyS0 hex.gpu.box=box-a hex.gpu=256,18446744073709551615 -- init",
        )
        .unwrap()
        .unwrap();
        assert_eq!(parsed.owner, BoxId::new("box-a").unwrap());
        assert_eq!(
            parsed.devices.iter().map(|id| id.key).collect::<Vec<_>>(),
            [256, u64::MAX]
        );
        assert_eq!(
            Authority::parse("-- hex.gpu.box=foreign hex.gpu=1").unwrap(),
            None
        );
        for text in [
            "hex.gpu=1",
            "hex.gpu.box=a",
            "hex.gpu.box=a hex.gpu=",
            "hex.gpu.box=a hex.gpu=1,1",
            "hex.gpu.box=a hex.gpu=2,1",
            "hex.gpu.box=a hex.gpu=1 hex.gpu=2",
            "hex.gpu.box=a hex.gpu.box=b hex.gpu=1",
            "hex.gpu.box=../a hex.gpu=1",
        ] {
            assert!(Authority::parse(text).is_err(), "{text}");
        }
    }
}
