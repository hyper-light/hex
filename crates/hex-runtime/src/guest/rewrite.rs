//! A box's spec, as the host's caller wrote it, made one the guest's node can run (docs/design/
//! shards-guest.md §4a): its root (a bundle's `root.path`, a `Rootfs::Dir` or its layers) is copied into
//! the guest and named by its copy's path, and each bind mount's source is named by its path under the
//! share that holds it. A non-bind mount names no host path and passes as it is.

use std::path::{Path, PathBuf};

use hex_api::{BoxId, BoxSpec, ClassSpec, OciSpec, Refusal, Rootfs};
use hex_client::guest::stage::{Item, Source, staged_path};
use serde_json::Value;

use super::shares::{self, Share};

/// The most `config.json` a bundle's rewrite reads: the `oci` class's own (`hex_core::bundle`).
const MAX_CONFIG_BYTES: u64 = 1 << 20;

/// A spec for the guest, and what to stage for it first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewritten {
    pub spec: BoxSpec,
    pub items: Vec<Item>,
}

/// `spec` rewritten for a guest whose shares are `shared` and whose relay stages under `staged`.
pub fn rewrite(spec: BoxSpec, shared: &[Share], staged: &str) -> Result<Rewritten, Refusal> {
    let BoxSpec { id, bounds, class } = spec;
    let mut items = Vec::new();
    let class = match class {
        ClassSpec::Process => ClassSpec::Process,
        ClassSpec::Oci(mut oci) => {
            oci.rootfs = match oci.rootfs {
                Rootfs::Dir(dir) => {
                    Rootfs::Dir(stage(staged, &id, &mut items, "root".to_owned(), dir))
                }
                Rootfs::Layers(layers) => Rootfs::Layers(
                    layers
                        .into_iter()
                        .enumerate()
                        .map(|(i, layer)| {
                            stage(staged, &id, &mut items, format!("layer{i}"), layer)
                        })
                        .collect(),
                ),
            };
            for mount in &mut oci.mounts {
                let guest = shares::in_guest(shared, &mount.source)
                    .ok_or_else(|| shares::unshared(shared, &mount.destination, &mount.source))?;
                mount.source = PathBuf::from(guest);
            }
            ClassSpec::Oci(oci)
        }
        ClassSpec::Bundle(dir) => {
            let (root, config) = bundle(&dir, shared)?;
            items.push(Item {
                name: "root".to_owned(),
                source: Source::Path(root),
            });
            items.push(Item {
                name: "config.json".to_owned(),
                source: Source::Bytes(config),
            });
            ClassSpec::Bundle(PathBuf::from(format!("{staged}/{id}")))
        }
    };
    Ok(Rewritten {
        spec: BoxSpec { id, bounds, class },
        items,
    })
}

/// The spec's own refusals (HEX-RT-13), before its host paths are mapped or a guest is booted: every
/// value but the host paths as [`OciSpec::check`] reads it on any node, with each host path stood in by
/// `/`; and each host path as this host reads it (absolute here, UTF-8 and without a NUL, as the guest's
/// config carries it), since `check` reads them as a Linux host's and a Windows host's are not.
pub fn check(spec: &OciSpec) -> Result<(), Refusal> {
    let root = || PathBuf::from("/");
    let mut probe = spec.clone();
    probe.rootfs = match &spec.rootfs {
        Rootfs::Dir(_) => Rootfs::Dir(root()),
        Rootfs::Layers(layers) => Rootfs::Layers(layers.iter().map(|_| root()).collect()),
    };
    for mount in &mut probe.mounts {
        mount.source = root();
    }
    probe.check()?;
    let mut paths: Vec<(String, &Path)> = match &spec.rootfs {
        Rootfs::Dir(dir) => vec![("rootfs.dir".to_owned(), dir.as_path())],
        Rootfs::Layers(layers) => layers
            .iter()
            .enumerate()
            .map(|(i, layer)| (format!("rootfs.layers[{i}]"), layer.as_path()))
            .collect(),
    };
    paths.extend(
        spec.mounts
            .iter()
            .enumerate()
            .map(|(i, m)| (format!("mounts[{i}].source"), m.source.as_path())),
    );
    for (field, path) in paths {
        let invalid = |why: &str| Refusal::Invalid {
            what: "oci spec",
            why: format!("{field} {path:?} {why}"),
        };
        let text = path
            .to_str()
            .ok_or_else(|| invalid("isn't UTF-8, which config.json can't carry"))?;
        if text.contains('\0') {
            return Err(invalid("holds a NUL"));
        }
        if !path.is_absolute() {
            return Err(invalid("isn't an absolute path"));
        }
    }
    Ok(())
}

/// Adds `path` to stage as `name`, and returns its copy's path in the guest.
fn stage(staged: &str, id: &BoxId, items: &mut Vec<Item>, name: String, path: PathBuf) -> PathBuf {
    let guest = PathBuf::from(staged_path(staged, id, &name));
    items.push(Item {
        name,
        source: Source::Path(path),
    });
    guest
}

fn invalid(why: String) -> Refusal {
    Refusal::Invalid {
        what: "bundle",
        why,
    }
}

/// A bundle's root on the host, and its `config.json` rewritten: `root.path` the copy's (`root`, beside
/// the config in the staged bundle), each bind mount's source its path under its share.
fn bundle(dir: &Path, shared: &[Share]) -> Result<(PathBuf, Vec<u8>), Refusal> {
    use std::io::Read;
    let path = dir.join("config.json");
    let file = std::fs::File::open(&path)
        .map_err(|e| invalid(format!("reading {}: {e}", path.display())))?;
    let mut text = Vec::new();
    file.take(MAX_CONFIG_BYTES.saturating_add(1))
        .read_to_end(&mut text)
        .map_err(|e| invalid(format!("reading {}: {e}", path.display())))?;
    if u64::try_from(text.len()).unwrap_or(u64::MAX) > MAX_CONFIG_BYTES {
        return Err(invalid(format!(
            "{} is over the {MAX_CONFIG_BYTES} bytes the runtime reads",
            path.display()
        )));
    }
    let mut config: Value = serde_json::from_slice(&text)
        .map_err(|e| invalid(format!("{} isn't JSON: {e}", path.display())))?;
    let root = config
        .pointer_mut("/root/path")
        .ok_or_else(|| invalid("its config.json names no root.path".to_owned()))?;
    let host_root = host(
        dir,
        root.as_str()
            .ok_or_else(|| invalid("its config.json's root.path isn't a string".to_owned()))?,
    );
    *root = Value::String("root".to_owned());
    if let Some(mounts) = config.get_mut("mounts").and_then(Value::as_array_mut) {
        for mount in mounts {
            if !is_bind(mount) {
                continue;
            }
            let destination = mount
                .get("destination")
                .and_then(Value::as_str)
                .unwrap_or("a mount")
                .to_owned();
            let Some(source) = mount.get_mut("source") else {
                return Err(invalid(format!("bind mount {destination} names no source")));
            };
            let host_source = host(
                dir,
                source.as_str().ok_or_else(|| {
                    invalid(format!("bind mount {destination}'s source isn't a string"))
                })?,
            );
            let guest = shares::in_guest(shared, &host_source)
                .ok_or_else(|| shares::unshared(shared, &destination, &host_source))?;
            *source = Value::String(guest);
        }
    }
    let bytes = serde_json::to_vec(&config)
        .map_err(|e| invalid(format!("writing its rewritten config.json: {e}")))?;
    Ok((host_root, bytes))
}

/// A path in a bundle's config as the host reads it: relative ones are the bundle's (runtime-spec,
/// config.md "root": "a relative path ... to the bundle").
fn host(dir: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_owned()
    } else {
        dir.join(path)
    }
}

/// Whether a config's mount is a bind: its type `bind`, or `bind` or `rbind` among its options
/// (runtime-spec config.md, "Mounts"; runc's and crun's reading).
fn is_bind(mount: &Value) -> bool {
    let typed = mount.get("type").and_then(Value::as_str) == Some("bind");
    let optioned = mount
        .get("options")
        .and_then(Value::as_array)
        .is_some_and(|options| {
            options
                .iter()
                .any(|o| matches!(o.as_str(), Some("bind" | "rbind")))
        });
    typed || optioned
}
