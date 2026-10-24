use std::io::{PipeReader, PipeWriter};
use std::path::PathBuf;

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
}
