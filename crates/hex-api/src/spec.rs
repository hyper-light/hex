use std::io::{PipeReader, PipeWriter};
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Bounds, BoxId, Refusal};

/// What a box holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    /// Host processes in a resource bound: a cgroup on Linux, QoS and limits on macOS, a job object
    /// on Windows.
    Process,
    /// An OCI container: namespaces, cgroups, seccomp, rootless through user namespaces (Linux).
    Oci,
}

impl Class {
    pub fn name(self) -> &'static str {
        match self {
            Self::Process => "process",
            Self::Oci => "oci",
        }
    }
}

/// A box to make.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoxSpec {
    pub id: BoxId,
    pub bounds: Bounds,
    pub class: ClassSpec,
}

/// The class a box is of, with what that class needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClassSpec {
    /// Host processes, started in the box by [`crate::Runtime::exec`].
    Process,
    /// An OCI container.
    Oci(Box<OciSpec>),
    /// An OCI container from a runtime-spec bundle: a directory holding `config.json` and the root
    /// filesystem it names, run as its config says, in the box's cgroup under its bounds. For callers
    /// that already speak the runtime-spec: kiln's executor, which unpacks images into bundles, and the
    /// bench, which runs one bundle under every runtime.
    Bundle(PathBuf),
}

impl ClassSpec {
    pub fn class(&self) -> Class {
        match self {
            Self::Process => Class::Process,
            Self::Oci(_) | Self::Bundle(_) => Class::Oci,
        }
    }
}

/// A container: its root filesystem, the program it runs first, and what it sees of the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OciSpec {
    pub rootfs: Rootfs,
    /// The container's first program. The container runs while it does; [`crate::Runtime::exec`]
    /// runs others beside it, as `docker exec` does.
    pub init: Program,
    /// Host folders and files bound in, in order.
    pub mounts: Vec<Mount>,
    pub hostname: Option<String>,
    pub network: Network,
    /// Lines added to the container's `/etc/hosts`: a name and the address it resolves to.
    pub hosts: Vec<(String, String)>,
}

impl OciSpec {
    /// Longest hostname: Linux's (`HOST_NAME_MAX`, gethostname(2)).
    pub const MAX_HOSTNAME: usize = 64;
    /// Longest name in an `/etc/hosts` line: a DNS name's longest written form (RFC 1035 §2.3.4's
    /// 255 octets on the wire).
    pub const MAX_HOSTS_NAME: usize = 253;

    /// Whether every value it names is well formed, before anything is made: a value that isn't is
    /// refused by its field's name, never dropped or half-applied (CLAUDE.md §3). A runtime refuses
    /// more as it renders the spec, what its host can't make.
    pub fn check(&self) -> Result<(), Refusal> {
        match &self.rootfs {
            Rootfs::Dir(dir) => host_path("rootfs.dir", dir)?,
            Rootfs::Layers(layers) => {
                if layers.is_empty() {
                    return Err(invalid_spec(
                        "rootfs.layers is empty: an overlay needs a layer under its writable one"
                            .to_owned(),
                    ));
                }
                for (i, layer) in layers.iter().enumerate() {
                    host_path(&format!("rootfs.layers[{i}]"), layer)?;
                }
            }
        }
        self.init.check().map_err(|e| match e {
            Refusal::Invalid { why, .. } => invalid_spec(format!("init: {why}")),
            other => other,
        })?;
        if let Some(cwd) = &self.init.cwd {
            container_path("init.cwd", cwd)?;
        }
        for (i, m) in self.mounts.iter().enumerate() {
            host_path(&format!("mounts[{i}].source"), &m.source)?;
            let field = format!("mounts[{i}].destination");
            container_path(&field, &m.destination)?;
            if m.destination
                .split('/')
                .all(|part| part.is_empty() || part == ".")
            {
                return Err(invalid_spec(format!(
                    "{field} is the root: a mount over it would hide the rootfs"
                )));
            }
        }
        if let Some(name) = &self.hostname {
            host_name("hostname", name, Self::MAX_HOSTNAME)?;
        }
        for (i, (name, address)) in self.hosts.iter().enumerate() {
            host_name(&format!("hosts[{i}].name"), name, Self::MAX_HOSTS_NAME)?;
            if address.parse::<IpAddr>().is_err() {
                return Err(invalid_spec(format!(
                    "hosts[{i}].address {address:?} isn't an IPv4 or IPv6 address"
                )));
            }
        }
        Ok(())
    }
}

fn invalid_spec(why: String) -> Refusal {
    Refusal::Invalid {
        what: "oci spec",
        why,
    }
}

/// A host path a spec names: absolute on the Linux host that runs the container, and UTF-8 without a
/// NUL, since `config.json` carries it as a JSON string and the kernel takes it as a C string.
fn host_path(field: &str, path: &Path) -> Result<(), Refusal> {
    let Some(text) = path.to_str() else {
        return Err(invalid_spec(format!(
            "{field} {path:?} isn't UTF-8, which config.json can't carry"
        )));
    };
    if text.contains('\0') {
        return Err(invalid_spec(format!("{field} {text:?} holds a NUL")));
    }
    if !text.starts_with('/') {
        return Err(invalid_spec(format!(
            "{field} {text:?} isn't an absolute path"
        )));
    }
    Ok(())
}

/// A path in the container: absolute, without a NUL, and with no `..`, which could reach above the
/// rootfs before the runtime has pivoted into it. Split on `/` alone, as the container's Linux does.
fn container_path(field: &str, path: &str) -> Result<(), Refusal> {
    if path.contains('\0') {
        return Err(invalid_spec(format!("{field} {path:?} holds a NUL")));
    }
    if !path.starts_with('/') {
        return Err(invalid_spec(format!(
            "{field} {path:?} isn't an absolute path"
        )));
    }
    if path.split('/').any(|part| part == "..") {
        return Err(invalid_spec(format!(
            "{field} {path:?} has a '..' component"
        )));
    }
    Ok(())
}

/// A hostname, or a name in an `/etc/hosts` line: 1 to `max` printable ASCII characters, none of them
/// a space or a `#`, so the line holds exactly the name it was given and starts no comment or other
/// line (hosts(5)).
fn host_name(field: &str, name: &str, max: usize) -> Result<(), Refusal> {
    if name.is_empty() || name.len() > max {
        return Err(invalid_spec(format!(
            "{field} {name:?} isn't 1 to {max} characters"
        )));
    }
    if let Some(c) = name.chars().find(|c| !c.is_ascii_graphic() || *c == '#') {
        return Err(invalid_spec(format!("{field} {name:?} holds {c:?}")));
    }
    Ok(())
}

/// A container's root filesystem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rootfs {
    /// A directory, used as it is and written in place.
    Dir(PathBuf),
    /// Read-only layers, lowest first (an image's, unpacked), under a writable layer the box owns
    /// and deletes with itself.
    Layers(Vec<PathBuf>),
}

/// A host path bound into a container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mount {
    pub source: PathBuf,
    /// Where it appears in the container: an absolute path.
    pub destination: String,
    pub read_only: bool,
}

/// What a container sees of the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    /// The host's network namespace.
    Host,
    /// A namespace of its own with loopback only.
    Private,
}

/// A program to run: `args[0]` is looked up in `env`'s `PATH` when it holds no `/`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Program {
    pub args: Vec<String>,
    /// Its whole environment: nothing is inherited.
    pub env: Vec<(String, String)>,
    /// Its working folder: a host path for `process`, a path in the container for `oci`.
    pub cwd: Option<String>,
    /// Who it runs as in a container; `process` runs everything as the node agent's user.
    pub user: Option<User>,
}

impl Program {
    /// `args` with `PATH` set to the usual system folders, nothing else in the environment.
    pub fn of(args: &[&str]) -> Self {
        Self {
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            env: vec![(
                "PATH".to_owned(),
                "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_owned(),
            )],
            cwd: None,
            user: None,
        }
    }

    /// Whether it can be started at all.
    pub fn check(&self) -> Result<(), Refusal> {
        let invalid = |why: String| {
            Err(Refusal::Invalid {
                what: "program",
                why,
            })
        };
        match self.args.first() {
            None => return invalid("no arguments: args[0] names the program".to_owned()),
            Some(a) if a.is_empty() => return invalid("args[0] is empty".to_owned()),
            Some(_) => {}
        }
        if let Some(a) = self.args.iter().find(|a| a.contains('\0')) {
            return invalid(format!("argument {a:?} holds a NUL"));
        }
        if let Some((k, _)) = self
            .env
            .iter()
            .find(|(k, v)| k.is_empty() || k.contains('=') || k.contains('\0') || v.contains('\0'))
        {
            return invalid(format!("environment variable {k:?} can't be set"));
        }
        Ok(())
    }
}

/// A user and its groups in a container.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub uid: u32,
    pub gid: u32,
    pub groups: Vec<u32>,
}

/// A program's standard streams; `None` is the null device.
#[derive(Debug, Default)]
pub struct Stdio {
    pub stdin: Option<PipeReader>,
    pub stdout: Option<PipeWriter>,
    pub stderr: Option<PipeWriter>,
}

impl Stdio {
    /// Every stream the null device.
    pub fn null() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_that_cant_be_started_is_refused_before_it_is_tried() {
        assert!(Program::default().check().is_err());
        assert!(Program::of(&[""]).check().is_err());
        assert!(Program::of(&["sh", "a\0b"]).check().is_err());
        let mut p = Program::of(&["sh"]);
        p.env.push(("A=B".to_owned(), "c".to_owned()));
        assert!(p.check().is_err());
        assert_eq!(Program::of(&["sh", "-c", "true"]).check(), Ok(()));
    }

    fn spec() -> OciSpec {
        OciSpec {
            rootfs: Rootfs::Layers(vec!["/images/base".into(), "/images/app".into()]),
            init: Program {
                cwd: Some("/work".to_owned()),
                ..Program::of(&["sleep", "infinity"])
            },
            mounts: vec![Mount {
                source: "/srv/data".into(),
                destination: "/data".to_owned(),
                read_only: true,
            }],
            hostname: Some("box-1.example".to_owned()),
            network: Network::Private,
            hosts: vec![
                ("db_main".to_owned(), "10.0.0.2".to_owned()),
                ("cache".to_owned(), "fd00::2".to_owned()),
            ],
        }
    }

    /// AC-T7.1.1, CLAUDE.md §3: a spec with a value no runtime can render is refused before anything
    /// is made, typed, with the field it names, and a well-formed one passes.
    #[test]
    fn an_oci_spec_with_a_value_that_cant_be_rendered_is_refused_by_its_name() {
        assert_eq!(spec().check(), Ok(()));
        type Break = Box<dyn Fn(&mut OciSpec)>;
        let cases: Vec<(&str, Break)> = vec![
            (
                "rootfs.dir",
                Box::new(|s| s.rootfs = Rootfs::Dir("images/base".into())),
            ),
            (
                "rootfs.layers is empty",
                Box::new(|s| s.rootfs = Rootfs::Layers(vec![])),
            ),
            (
                "rootfs.layers[1]",
                Box::new(|s| s.rootfs = Rootfs::Layers(vec!["/a".into(), "b".into()])),
            ),
            ("init: ", Box::new(|s| s.init.args.clear())),
            ("init: ", Box::new(|s| s.init.args.push("a\0b".to_owned()))),
            (
                "init.cwd",
                Box::new(|s| s.init.cwd = Some("work".to_owned())),
            ),
            (
                "mounts[0].source",
                Box::new(|s| s.mounts[0].source = "srv/data".into()),
            ),
            (
                "mounts[0].destination",
                Box::new(|s| s.mounts[0].destination = "data".to_owned()),
            ),
            (
                "mounts[0].destination",
                Box::new(|s| s.mounts[0].destination = "/data/../../etc".to_owned()),
            ),
            (
                "mounts[0].destination",
                Box::new(|s| s.mounts[0].destination = "/data\0".to_owned()),
            ),
            (
                "mounts[0].destination is the root",
                Box::new(|s| s.mounts[0].destination = "//./".to_owned()),
            ),
            ("hostname", Box::new(|s| s.hostname = Some(String::new()))),
            (
                "hostname",
                Box::new(|s| s.hostname = Some("a".repeat(OciSpec::MAX_HOSTNAME + 1))),
            ),
            (
                "hostname",
                Box::new(|s| s.hostname = Some("two words".to_owned())),
            ),
            (
                "hosts[1].name",
                Box::new(|s| s.hosts[1].0 = "cache\n10.6.6.6 evil".to_owned()),
            ),
            (
                "hosts[1].name",
                Box::new(|s| s.hosts[1].0 = "cache#".to_owned()),
            ),
            (
                "hosts[0].address",
                Box::new(|s| s.hosts[0].1 = "10.0.0.256".to_owned()),
            ),
            (
                "hosts[0].address",
                Box::new(|s| s.hosts[0].1 = "db.example".to_owned()),
            ),
        ];
        for (field, break_it) in cases {
            let mut s = spec();
            break_it(&mut s);
            refused_naming(&s, field);
        }
    }

    /// A host path config.json can't carry, bytes that aren't UTF-8, is refused by name rather than
    /// written lossily (a lossy path would name a different directory).
    #[cfg(unix)]
    #[test]
    fn a_host_path_that_isnt_utf8_is_refused_by_its_name() {
        use std::os::unix::ffi::OsStrExt;
        let mut s = spec();
        s.rootfs = Rootfs::Layers(vec![std::ffi::OsStr::from_bytes(b"/layers/\xff").into()]);
        refused_naming(&s, "rootfs.layers[0]");
    }

    fn refused_naming(s: &OciSpec, field: &str) {
        match s.check() {
            Err(Refusal::Invalid {
                what: "oci spec",
                why,
            }) => assert!(why.contains(field), "{field}: refused as {why:?}"),
            other => panic!("{field}: {other:?}"),
        }
    }
}
