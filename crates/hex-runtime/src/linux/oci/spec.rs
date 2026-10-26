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
    /// The AppArmor profile to switch to on exec. Hex does not yet apply it, so `Spec::check` refuses a
    /// spec that sets it rather than run the container without the profile it asked for (CLAUDE.md §3).
    #[serde(default)]
    pub apparmor_profile: Option<String>,
    /// The SELinux process label. Same: refused until Hex applies it, never silently dropped (§3).
    #[serde(default)]
    pub selinux_label: Option<String>,
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
    #[serde(default)]
    pub resources: Option<Resources>,
    /// `linux.seccomp`: the syscall filter. Hex compiles it to a BPF program and loads it in the
    /// container init before `execve` (see [`super::seccomp`]). A profile the compiler can't honour
    /// is refused there, by name (CLAUDE.md §3), never run unfiltered.
    #[serde(default)]
    pub seccomp: Option<Seccomp>,
    /// `linux.mountLabel`: the SELinux label for the container's mounts. Refused until applied (§3).
    #[serde(default)]
    pub mount_label: Option<String>,
}

/// `linux.seccomp` (config-linux.md, "Seccomp"): a syscall filter. `default_action` is taken when
/// no `syscalls` entry matches; each entry names syscalls, the action for them, and optional
/// argument conditions. [`super::seccomp`] compiles this to a BPF program.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Seccomp {
    /// The action when no rule matches, e.g. `SCMP_ACT_ERRNO`. Required; a missing one is malformed.
    pub default_action: String,
    /// The errno `default_action` returns when it is `SCMP_ACT_ERRNO`/`SCMP_ACT_TRACE`; else unset.
    #[serde(default)]
    pub default_errno_ret: Option<u32>,
    /// The `SCMP_ARCH_*` the profile applies to; empty means the native architecture only.
    #[serde(default)]
    pub architectures: Vec<String>,
    /// `SECCOMP_FILTER_FLAG_*` for the `seccomp(2)` call.
    #[serde(default)]
    pub flags: Vec<String>,
    /// The syscall groups, each an action for the syscalls it names under its argument conditions.
    #[serde(default)]
    pub syscalls: Vec<SeccompSyscall>,
}

/// One `linux.seccomp.syscalls` entry: an action for the named syscalls, under all of `args`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeccompSyscall {
    #[serde(default)]
    pub names: Vec<String>,
    /// This group's action, e.g. `SCMP_ACT_ALLOW`. Required; a missing one is malformed.
    pub action: String,
    /// The errno this group returns when its action is `SCMP_ACT_ERRNO`/`SCMP_ACT_TRACE`.
    #[serde(default)]
    pub errno_ret: Option<u32>,
    /// The argument conditions; all must hold for the group to match (AND). Empty matches always.
    #[serde(default)]
    pub args: Vec<SeccompArg>,
}

/// One `args` condition: compares syscall argument `index` against `value` (and `value_two` for a
/// masked compare) with `op` (an `SCMP_CMP_*`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeccompArg {
    pub index: u32,
    pub value: u64,
    #[serde(default)]
    pub value_two: Option<u64>,
    /// The comparison, e.g. `SCMP_CMP_EQ`. Required; a missing one is malformed.
    pub op: String,
}

/// `linux.resources` (config-linux.md, "Control groups"): cgroup v1-shaped limits, which
/// [`super::resources`] converts to cgroup v2 files, and `unified`, cgroup v2 files by name.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resources {
    /// The allowed device list, applied in order.
    #[serde(default)]
    pub devices: Vec<DeviceRule>,
    #[serde(default)]
    pub memory: Option<Memory>,
    #[serde(default)]
    pub cpu: Option<Cpu>,
    #[serde(default, rename = "blockIO")]
    pub block_io: Option<BlockIo>,
    #[serde(default)]
    pub hugepage_limits: Vec<HugepageLimit>,
    /// `net_cls` and `net_prio`, which cgroup v2 doesn't have: present is refused, whatever it holds.
    #[serde(default)]
    pub network: Option<serde::de::IgnoredAny>,
    #[serde(default)]
    pub pids: Option<Pids>,
    #[serde(default)]
    pub rdma: std::collections::BTreeMap<String, Rdma>,
    #[serde(default)]
    pub unified: std::collections::BTreeMap<String, String>,
}

/// One entry of the allowed device list.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRule {
    pub allow: bool,
    /// `a` (all), `c` (char) or `b` (block); unset is all.
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    /// Unset is every major (minor).
    #[serde(default)]
    pub major: Option<i64>,
    #[serde(default)]
    pub minor: Option<i64>,
    /// Some of `r`, `w` and `m`; unset is all three.
    #[serde(default)]
    pub access: Option<String>,
}

/// `linux.resources.memory`, in bytes, `-1` for unlimited.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub reservation: Option<i64>,
    /// Memory plus swap, as cgroup v1 counts it.
    #[serde(default)]
    pub swap: Option<i64>,
    #[serde(default)]
    pub kernel: Option<i64>,
    #[serde(default, rename = "kernelTCP")]
    pub kernel_tcp: Option<i64>,
    #[serde(default)]
    pub swappiness: Option<u64>,
    #[serde(default, rename = "disableOOMKiller")]
    pub disable_oom_killer: Option<bool>,
    #[serde(default)]
    pub use_hierarchy: Option<bool>,
    #[serde(default)]
    pub check_before_update: Option<bool>,
}

/// `linux.resources.cpu`: times in microseconds.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cpu {
    #[serde(default)]
    pub shares: Option<u64>,
    #[serde(default)]
    pub quota: Option<i64>,
    #[serde(default)]
    pub burst: Option<u64>,
    #[serde(default)]
    pub period: Option<u64>,
    #[serde(default)]
    pub realtime_runtime: Option<i64>,
    #[serde(default)]
    pub realtime_period: Option<u64>,
    #[serde(default)]
    pub cpus: Option<String>,
    #[serde(default)]
    pub mems: Option<String>,
    #[serde(default)]
    pub idle: Option<i64>,
}

/// `linux.resources.blockIO`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockIo {
    #[serde(default)]
    pub weight: Option<u16>,
    #[serde(default)]
    pub leaf_weight: Option<u16>,
    #[serde(default)]
    pub weight_device: Vec<WeightDevice>,
    #[serde(default)]
    pub throttle_read_bps_device: Vec<ThrottleDevice>,
    #[serde(default)]
    pub throttle_write_bps_device: Vec<ThrottleDevice>,
    #[serde(default, rename = "throttleReadIOPSDevice")]
    pub throttle_read_iops_device: Vec<ThrottleDevice>,
    #[serde(default, rename = "throttleWriteIOPSDevice")]
    pub throttle_write_iops_device: Vec<ThrottleDevice>,
}

/// One device's IO weight.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeightDevice {
    pub major: i64,
    pub minor: i64,
    #[serde(default)]
    pub weight: Option<u16>,
    #[serde(default)]
    pub leaf_weight: Option<u16>,
}

/// One device's IO rate limit, in bytes or operations per second.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThrottleDevice {
    pub major: i64,
    pub minor: i64,
    pub rate: u64,
}

/// A HugeTLB limit for one page size (`2MB`, `1GB`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HugepageLimit {
    pub page_size: String,
    pub limit: u64,
}

/// `linux.resources.pids`: `-1` is no limit, and `0` is a limit of zero.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pids {
    #[serde(default)]
    pub limit: Option<i64>,
}

/// One RDMA device's limits.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rdma {
    #[serde(default)]
    pub hca_handles: Option<u32>,
    #[serde(default)]
    pub hca_objects: Option<u32>,
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
        if let Some(process) = &self.process {
            if process.args.first().is_none_or(String::is_empty) {
                return Err(OciError::Config("process.args is empty".to_owned()));
            }
            // A spec that names a security control Hex can't yet apply is refused by name, never run
            // without it (CLAUDE.md §3: a dropped security control is a silent failure).
            if process
                .apparmor_profile
                .as_deref()
                .is_some_and(|s| !s.is_empty())
            {
                return Err(OciError::Config(
                    "process.apparmorProfile is set: Hex does not yet apply an AppArmor profile, and \
                     refuses rather than run the container without the one it asked for"
                        .to_owned(),
                ));
            }
            if process
                .selinux_label
                .as_deref()
                .is_some_and(|s| !s.is_empty())
            {
                return Err(OciError::Config(
                    "process.selinuxLabel is set: Hex does not yet apply a SELinux process label, and \
                     refuses rather than run without it"
                        .to_owned(),
                ));
            }
        }
        // linux.seccomp is honoured: the container init compiles it and loads the BPF filter before
        // execve (super::seccomp). A profile the compiler can't honour is refused there, by name, so
        // the refusal is no longer blanket here.
        if self
            .linux
            .mount_label
            .as_deref()
            .is_some_and(|s| !s.is_empty())
        {
            return Err(OciError::Config(
                "linux.mountLabel is set: Hex does not yet apply a SELinux mount label, and refuses \
                 rather than run without it"
                    .to_owned(),
            ));
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
    fn a_security_control_hex_cannot_yet_apply_is_refused_not_dropped() {
        // CLAUDE.md §3: a security control Hex can't honour is refused by name, never run without
        // (a dropped LSM control is a silent security failure). Parse runs check. linux.seccomp is
        // no longer here: Hex compiles and loads it (the compiler refuses an unsupported profile by
        // name, super::seccomp), so a settable seccomp profile is honoured, not refused wholesale.
        let cases = [
            (
                "process.apparmorProfile",
                r#"{"ociVersion":"1.0.0","root":{"path":"rootfs"},
                    "process":{"args":["/bin/sh"],"apparmorProfile":"docker-default"},"linux":{}}"#,
            ),
            (
                "process.selinuxLabel",
                r#"{"ociVersion":"1.0.0","root":{"path":"rootfs"},
                    "process":{"args":["/bin/sh"],"selinuxLabel":"system_u:system_r:container_t:s0"}}"#,
            ),
            (
                "linux.mountLabel",
                r#"{"ociVersion":"1.0.0","root":{"path":"rootfs"},"process":{"args":["/bin/sh"]},
                    "linux":{"mountLabel":"system_u:object_r:container_file_t:s0"}}"#,
            ),
        ];
        for (field, json) in cases {
            let r = Spec::parse(json.as_bytes());
            assert!(
                matches!(&r, Err(OciError::Config(why)) if why.contains(field)),
                "{field} should be refused with a Config error naming it, got {r:?}"
            );
        }
    }

    // AC-T7.1.4 / CLAUDE.md §6: linux.seccomp now parses into its fields (it is no longer refused at
    // the spec), so a wrong field name wouldn't silently drop part of the filter. The compile step
    // (super::seccomp) is what refuses an unsupported profile; here we only check the model reads.
    #[test]
    fn a_seccomp_profile_parses_into_its_fields() {
        let json = r#"{"ociVersion":"1.0.0","root":{"path":"rootfs"},"process":{"args":["/bin/sh"]},
            "linux":{"seccomp":{
                "defaultAction":"SCMP_ACT_ERRNO","defaultErrnoRet":38,
                "architectures":["SCMP_ARCH_X86_64","SCMP_ARCH_X86"],
                "flags":["SECCOMP_FILTER_FLAG_TSYNC"],
                "syscalls":[
                    {"names":["read","write"],"action":"SCMP_ACT_ALLOW"},
                    {"names":["socket"],"action":"SCMP_ACT_ERRNO","errnoRet":22,
                     "args":[{"index":0,"value":16,"op":"SCMP_CMP_EQ"},
                             {"index":2,"value":9,"valueTwo":9,"op":"SCMP_CMP_MASKED_EQ"}]}
                ]}}}"#;
        let spec = Spec::parse(json.as_bytes()).unwrap();
        let sc = spec.linux.seccomp.unwrap();
        assert_eq!(sc.default_action, "SCMP_ACT_ERRNO");
        assert_eq!(sc.default_errno_ret, Some(38));
        assert_eq!(sc.architectures, ["SCMP_ARCH_X86_64", "SCMP_ARCH_X86"]);
        assert_eq!(sc.flags, ["SECCOMP_FILTER_FLAG_TSYNC"]);
        assert_eq!(sc.syscalls.len(), 2);
        assert_eq!(sc.syscalls[0].names, ["read", "write"]);
        assert_eq!(sc.syscalls[0].action, "SCMP_ACT_ALLOW");
        let netlink = &sc.syscalls[1];
        assert_eq!(
            (netlink.action.as_str(), netlink.errno_ret),
            ("SCMP_ACT_ERRNO", Some(22))
        );
        assert_eq!(netlink.args[0].index, 0);
        assert_eq!(
            (netlink.args[1].value, netlink.args[1].value_two),
            (9, Some(9))
        );
        assert_eq!(netlink.args[1].op, "SCMP_CMP_MASKED_EQ");
    }

    // Hostile input to the seccomp model is a typed error, not a panic (CLAUDE.md §6): a group
    // missing its required `action`, or an arg missing `op`, is malformed and refused at parse.
    #[test]
    fn a_malformed_seccomp_profile_is_a_reason_not_a_panic() {
        for bad in [
            // syscalls entry with no action
            r#"{"ociVersion":"1.0.0","root":{"path":"rootfs"},"process":{"args":["/bin/sh"]},
                "linux":{"seccomp":{"defaultAction":"SCMP_ACT_ALLOW","syscalls":[{"names":["read"]}]}}}"#,
            // no defaultAction
            r#"{"ociVersion":"1.0.0","root":{"path":"rootfs"},"process":{"args":["/bin/sh"]},
                "linux":{"seccomp":{"syscalls":[]}}}"#,
            // arg with no op
            r#"{"ociVersion":"1.0.0","root":{"path":"rootfs"},"process":{"args":["/bin/sh"]},
                "linux":{"seccomp":{"defaultAction":"SCMP_ACT_ALLOW","syscalls":[
                    {"names":["read"],"action":"SCMP_ACT_ERRNO","args":[{"index":0,"value":1}]}]}}}"#,
        ] {
            assert!(
                matches!(Spec::parse(bad.as_bytes()), Err(OciError::Config(_))),
                "malformed seccomp should be a Config error: {bad}"
            );
        }
    }

    // AC-T7.1.4: linux.resources parses under the spec's own field names, several of which aren't
    // plain camelCase (blockIO, kernelTCP, disableOOMKiller, throttleReadIOPSDevice): a wrong name
    // would read as absent, and the limit it holds would silently not be applied.
    #[test]
    fn resources_parse_under_the_specs_field_names() {
        let json = MINIMAL.replace(
            "\"maskedPaths\": [\"/proc/kcore\"]",
            r#""maskedPaths": ["/proc/kcore"], "cgroupsPath": "/a/b",
               "resources": {"devices": [{"allow": false, "access": "rwm"}],
                 "memory": {"limit": 1024, "swap": 2048, "kernelTCP": 1, "disableOOMKiller": false},
                 "cpu": {"shares": 1024, "realtimeRuntime": 1, "cpus": "0-1"},
                 "blockIO": {"weight": 500, "throttleReadIOPSDevice": [{"major": 8, "minor": 0, "rate": 100}]},
                 "hugepageLimits": [{"pageSize": "2MB", "limit": 1}],
                 "network": {"classID": 1},
                 "pids": {"limit": -1},
                 "rdma": {"mlx5_0": {"hcaHandles": 1}},
                 "unified": {"memory.high": "1"}}"#,
        );
        let spec = Spec::parse(json.as_bytes()).unwrap();
        assert_eq!(spec.linux.cgroups_path.as_deref(), Some("/a/b"));
        let r = spec.linux.resources.unwrap();
        assert!(!r.devices[0].allow);
        let m = r.memory.unwrap();
        assert_eq!(
            (m.limit, m.swap, m.kernel_tcp, m.disable_oom_killer),
            (Some(1024), Some(2048), Some(1), Some(false))
        );
        let c = r.cpu.unwrap();
        assert_eq!((c.shares, c.realtime_runtime), (Some(1024), Some(1)));
        assert_eq!(c.cpus.as_deref(), Some("0-1"));
        let b = r.block_io.unwrap();
        assert_eq!(b.weight, Some(500));
        assert_eq!(b.throttle_read_iops_device[0].rate, 100);
        assert_eq!(r.hugepage_limits[0].page_size, "2MB");
        assert!(r.network.is_some());
        assert_eq!(r.pids.unwrap().limit, Some(-1));
        assert_eq!(r.rdma["mlx5_0"].hca_handles, Some(1));
        assert_eq!(r.unified["memory.high"], "1");
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
