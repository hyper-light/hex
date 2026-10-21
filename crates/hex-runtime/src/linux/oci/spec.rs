//! The OCI Runtime Specification v1.3.0 `config.json`, the fields Hex's `oci` class reads. Unknown
//! fields are ignored, not refused (the spec requires a runtime not error on an unknown property;
//! runtime-tools' `misc_props` checks it). Hostile input is a typed error, never a panic
//! (CLAUDE.md §6): the model parses, and [`Spec::check`] refuses what the runtime can't honour.

use serde::Deserialize;

use crate::linux::oci::OciError;

/// Longest `config.json` Hex reads: a spec is a few kilobytes; a larger file is refused unparsed.
pub const MAX_CONFIG_BYTES: usize = 1 << 20;

/// A parsed `config.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec {
    pub oci_version: String,
    #[serde(default)]
    pub hostname: Option<String>,
    pub root: Root,
    #[serde(default)]
    pub mounts: Vec<Mount>,
    pub process: Option<Process>,
    #[serde(default)]
    pub linux: Linux,
    #[serde(default)]
    pub hooks: Hooks,
    /// Arbitrary metadata; carried into the container state the `state` command and hooks see.
    #[serde(default)]
    pub annotations: std::collections::BTreeMap<String, String>,
}

/// Lifecycle hooks (OCI spec §POSIX-platform Hooks). Each list runs in order at its point; the
/// container state JSON is written to each hook's stdin.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hooks {
    /// Deprecated; runs in the runtime namespace after the container is created, before `start`.
    #[serde(default)]
    pub prestart: Vec<Hook>,
    #[serde(default)]
    pub create_runtime: Vec<Hook>,
    #[serde(default)]
    pub create_container: Vec<Hook>,
    #[serde(default)]
    pub start_container: Vec<Hook>,
    #[serde(default)]
    pub poststart: Vec<Hook>,
    #[serde(default)]
    pub poststop: Vec<Hook>,
}

/// One hook: a program, its arguments and environment, and how long it may run.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hook {
    pub path: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub timeout: Option<i64>,
}

/// The container's root filesystem.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Root {
    pub path: String,
    #[serde(default)]
    pub readonly: bool,
}

/// A mount into the container.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mount {
    pub destination: String,
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub options: Vec<String>,
}

/// The first process the container runs.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Process {
    #[serde(default)]
    pub terminal: bool,
    #[serde(default = "root_cwd")]
    pub cwd: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub user: User,
    #[serde(default)]
    pub capabilities: Option<Capabilities>,
    #[serde(default)]
    pub rlimits: Vec<Rlimit>,
    #[serde(default)]
    pub no_new_privileges: bool,
    #[serde(default)]
    pub oom_score_adj: Option<i32>,
}

fn root_cwd() -> String {
    "/".to_owned()
}

/// Who the first process runs as.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    #[serde(default)]
    pub uid: u32,
    #[serde(default)]
    pub gid: u32,
    #[serde(default)]
    pub additional_gids: Vec<u32>,
    #[serde(default)]
    pub umask: Option<u32>,
}

/// The capability sets, each a list of `CAP_*` names.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    #[serde(default)]
    pub bounding: Vec<String>,
    #[serde(default)]
    pub effective: Vec<String>,
    #[serde(default)]
    pub inheritable: Vec<String>,
    #[serde(default)]
    pub permitted: Vec<String>,
    #[serde(default)]
    pub ambient: Vec<String>,
}

/// A resource limit (`setrlimit`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rlimit {
    #[serde(rename = "type")]
    pub kind: String,
    pub hard: u64,
    pub soft: u64,
}

/// The Linux-specific configuration.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Linux {
    #[serde(default)]
    pub namespaces: Vec<Namespace>,
    #[serde(default)]
    pub uid_mappings: Vec<IdMapping>,
    #[serde(default)]
    pub gid_mappings: Vec<IdMapping>,
    #[serde(default)]
    pub devices: Vec<Device>,
    #[serde(default)]
    pub masked_paths: Vec<String>,
    #[serde(default)]
    pub readonly_paths: Vec<String>,
    #[serde(default)]
    pub sysctl: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub rootfs_propagation: Option<String>,
    #[serde(default)]
    pub cgroups_path: Option<String>,
}

/// A namespace the container runs in; `path` joins an existing one instead of making a new one.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Namespace {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub path: Option<String>,
}

/// A uid or gid mapping for a user namespace.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdMapping {
    #[serde(rename = "containerID")]
    pub container_id: u32,
    #[serde(rename = "hostID")]
    pub host_id: u32,
    pub size: u32,
}

/// A device node to make in the container.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    #[serde(rename = "type")]
    pub kind: String,
    pub path: String,
    #[serde(default)]
    pub major: Option<i64>,
    #[serde(default)]
    pub minor: Option<i64>,
    #[serde(default)]
    pub file_mode: Option<u32>,
    #[serde(default)]
    pub uid: Option<u32>,
    #[serde(default)]
    pub gid: Option<u32>,
}

impl Spec {
    /// A spec from `config.json`'s bytes, or why they aren't one.
    pub fn parse(bytes: &[u8]) -> Result<Self, OciError> {
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(OciError::Config(format!(
                "config.json is {} bytes, over {MAX_CONFIG_BYTES}",
                bytes.len()
            )));
        }
        let spec: Spec = serde_json::from_slice(bytes)
            .map_err(|e| OciError::Config(format!("config.json: {e}")))?;
        spec.check()?;
        Ok(spec)
    }

    /// Refuses a spec the runtime can't honour, by name.
    pub fn check(&self) -> Result<(), OciError> {
        if self.oci_version.is_empty() {
            return Err(OciError::Config("ociVersion is empty".to_owned()));
        }
        if self.root.path.is_empty() {
            return Err(OciError::Config("root.path is empty".to_owned()));
        }
        // A destination must be absolute, so a mount can't escape the container's tree.
        for m in &self.mounts {
            if !m.destination.starts_with('/') {
                return Err(OciError::Config(format!(
                    "mount destination {:?} isn't absolute",
                    m.destination
                )));
            }
        }
        for p in self
            .linux
            .masked_paths
            .iter()
            .chain(&self.linux.readonly_paths)
        {
            if !p.starts_with('/') {
                return Err(OciError::Config(format!("path {p:?} isn't absolute")));
            }
        }
        if let Some(process) = &self.process
            && process.args.first().is_none_or(String::is_empty)
        {
            return Err(OciError::Config("process.args is empty".to_owned()));
        }
        Ok(())
    }

    /// Whether the spec asks for a namespace of `kind` (made fresh, no `path`).
    pub fn makes_namespace(&self, kind: &str) -> bool {
        self.linux
            .namespaces
            .iter()
            .any(|n| n.kind == kind && n.path.is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"{
        "ociVersion": "1.3.0",
        "root": {"path": "rootfs", "readonly": true},
        "process": {"args": ["/bin/sh"], "cwd": "/"},
        "mounts": [{"destination": "/proc", "type": "proc", "source": "proc"}],
        "linux": {"namespaces": [{"type": "pid"}, {"type": "mount"}], "maskedPaths": ["/proc/kcore"]}
    }"#;

    #[test]
    fn a_minimal_config_parses_with_its_fields() {
        let spec = Spec::parse(MINIMAL.as_bytes()).unwrap();
        assert_eq!(spec.oci_version, "1.3.0");
        assert!(spec.root.readonly);
        assert_eq!(spec.process.as_ref().unwrap().args, ["/bin/sh"]);
        assert_eq!(spec.mounts[0].kind.as_deref(), Some("proc"));
        assert!(spec.makes_namespace("pid"));
        assert!(!spec.makes_namespace("network"));
        assert_eq!(spec.linux.masked_paths, ["/proc/kcore"]);
    }

    #[test]
    fn an_unknown_field_is_ignored_not_refused() {
        let with_extra =
            MINIMAL.replace("\"readonly\": true", "\"readonly\": true, \"madeUp\": 42");
        assert!(Spec::parse(with_extra.as_bytes()).is_ok());
    }

    #[test]
    fn a_relative_mount_destination_or_path_is_refused() {
        let bad = MINIMAL.replace("/proc", "proc");
        assert!(matches!(
            Spec::parse(bad.as_bytes()),
            Err(OciError::Config(_))
        ));
    }

    #[test]
    fn hostile_bytes_are_a_reason_not_a_panic() {
        for bad in [
            "",
            "{}",
            "null",
            "[]",
            "{\"ociVersion\":\"\"}",
            "{not json",
            &"{".repeat(10_000),
        ] {
            assert!(Spec::parse(bad.as_bytes()).is_err(), "{bad:?} parsed");
        }
        assert!(Spec::parse(&vec![b'{'; MAX_CONFIG_BYTES + 1]).is_err());
    }
}
