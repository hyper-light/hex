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
            if path_parts(&m.destination).all(|part| part.is_empty() || part == ".") {
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

/// Whether `path` is drive-absolute, Windows' absolute form (`C:\…` or `C:/…`); `C:data` is relative to
/// the drive's current folder and `\\server\share` a UNC path, neither absolute here.
pub fn drive_absolute(path: &str) -> bool {
    let mut c = path.chars();
    matches!(
        (c.next(), c.next(), c.next()),
        (Some(d), Some(':'), Some('\\' | '/')) if d.is_ascii_alphabetic()
    )
}

/// The components of an absolute path in either form: after its drive for a drive-absolute one, split on
/// both separators; split on `/` alone for a POSIX one, as Linux reads it.
fn path_parts(path: &str) -> impl Iterator<Item = &str> {
    let (rest, windows) = if drive_absolute(path) {
        (path.get(2..).unwrap_or(""), true)
    } else {
        (path, false)
    };
    rest.split(move |c| c == '/' || (windows && c == '\\'))
}

/// A host path a spec names: absolute on the host that runs the container (POSIX-absolute on Linux and
/// macOS, drive-absolute on Windows; the runtime refuses the form its host can't use), and UTF-8 without a
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
    if !text.starts_with('/') && !drive_absolute(text) {
        return Err(invalid_spec(format!(
            "{field} {text:?} isn't an absolute path"
        )));
    }
    Ok(())
}

/// A path in the container: absolute, without a NUL, and with no `..`, which could reach above the
/// rootfs before the runtime has pivoted into it. A POSIX path splits on `/` alone, as a Linux container
/// does; a drive-absolute one (a Windows container's) on both separators.
fn container_path(field: &str, path: &str) -> Result<(), Refusal> {
    if path.contains('\0') {
        return Err(invalid_spec(format!("{field} {path:?} holds a NUL")));
    }
    if !path.starts_with('/') && !drive_absolute(path) {
        return Err(invalid_spec(format!(
            "{field} {path:?} isn't an absolute path"
        )));
    }
    if path_parts(path).any(|part| part == "..") {
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

    /// The bytes an exec takes for this program, counted as Linux's `execve` counts them
    /// (docs/research/runtime/exec-size-bounds.md), which is at least what XNU counts: each argument and
    /// each `KEY=VALUE` string with its NUL, the first argument once more for the filename, and a pointer
    /// for each string (one argument at least). Saturates at `usize::MAX`, which no bound admits.
    pub fn exec_size(&self) -> usize {
        let args = self
            .args
            .iter()
            .fold(0usize, |n, a| n.saturating_add(a.len()).saturating_add(1));
        let filename = self.args.first().map_or(1, |a| a.len().saturating_add(1));
        let env = self.env.iter().fold(0usize, |n, (k, v)| {
            // KEY=VALUE and its NUL.
            n.saturating_add(k.len())
                .saturating_add(v.len())
                .saturating_add(2)
        });
        let pointers = self
            .args
            .len()
            .max(1)
            .saturating_add(self.env.len())
            .saturating_mul(size_of::<usize>());
        args.saturating_add(filename)
            .saturating_add(env)
            .saturating_add(pointers)
    }

    /// Whether this OS's exec takes it: refused as `Invalid` naming the sizes and the bound's source when it
    /// doesn't, before any spawn (CLAUDE.md §2, §3).
    pub fn fits(&self, limit: &ExecLimit) -> Result<(), Refusal> {
        let size = limit.size(self);
        if size > limit.block {
            let (what, unit) = match limit.counts {
                BlockCount::Execve => ("its arguments and environment take", "bytes"),
                BlockCount::Environment => ("its environment takes", limit.unit.name()),
            };
            return Err(Refusal::Invalid {
                what: "program",
                why: format!(
                    "{what} {size} {unit}; {} allows {}",
                    limit.source, limit.block
                ),
            });
        }
        let Some(most) = limit.string else {
            return Ok(());
        };
        let unit = limit.unit;
        // Each string with its NUL, in the OS's own unit.
        let arg_size = |a: &str| unit.of(a).saturating_add(1);
        let var_size = |k: &str, v: &str| unit.of(k).saturating_add(unit.of(v)).saturating_add(2);
        if let Some(a) = self.args.iter().find(|a| arg_size(a) > most) {
            return Err(Refusal::Invalid {
                what: "program",
                why: format!(
                    "an argument of {} {}; {} allows {most} for one string",
                    arg_size(a),
                    unit.name(),
                    limit.source
                ),
            });
        }
        if let Some((k, v)) = self.env.iter().find(|(k, v)| var_size(k, v) > most) {
            return Err(Refusal::Invalid {
                what: "program",
                why: format!(
                    "environment variable {k:?} takes {} {}; {} allows {most} for one string",
                    var_size(k, v),
                    unit.name(),
                    limit.source
                ),
            });
        }
        Ok(())
    }
}

/// What an OS's exec bound counts of a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockCount {
    /// Every argument and variable, the filename and their pointers, in bytes: [`Program::exec_size`]
    /// (Linux's execve, which bounds at least what XNU's does).
    Execve,
    /// The environment alone, each NAME=VALUE and its NUL in [`ExecLimit::unit`] (Windows' environment block;
    /// its command line is bounded apart).
    Environment,
}

/// The unit an OS counts one string of a program in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringUnit {
    /// UTF-8 bytes (Linux's `MAX_ARG_STRLEN`).
    Bytes,
    /// UTF-16 code units (Windows' 32,767 characters a variable).
    Utf16,
}

impl ExecLimit {
    /// `program`'s size as this bound counts it.
    pub fn size(&self, program: &Program) -> usize {
        match self.counts {
            BlockCount::Execve => program.exec_size(),
            BlockCount::Environment => program.env.iter().fold(0usize, |n, (k, v)| {
                // NAME=VALUE and its NUL.
                n.saturating_add(self.unit.of(k))
                    .saturating_add(self.unit.of(v))
                    .saturating_add(2)
            }),
        }
    }
}

impl StringUnit {
    /// `s`'s length in this unit.
    pub fn of(self, s: &str) -> usize {
        match self {
            Self::Bytes => s.len(),
            Self::Utf16 => s.encode_utf16().count(),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Bytes => "bytes",
            Self::Utf16 => "UTF-16 units",
        }
    }
}

/// The most an OS's exec takes of a program's arguments and environment
/// (docs/research/runtime/exec-size-bounds.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecLimit {
    /// The most a program may take, counted as [`ExecLimit::counts`] says.
    pub block: usize,
    /// What [`ExecLimit::block`] counts.
    pub counts: BlockCount,
    /// Any one string with its NUL, in [`ExecLimit::unit`], where the OS bounds that too.
    pub string: Option<usize>,
    /// What [`ExecLimit::string`] counts: Linux and macOS bound a string in bytes, Windows in UTF-16 units.
    pub unit: StringUnit,
    /// Where the bound was read, for the refusal.
    pub source: &'static str,
}

/// A user and its groups in a container.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub uid: u32,
    pub gid: u32,
    pub groups: Vec<u32>,
}

/// A program's standard streams: each one the caller's own (a pipe's end, given here), one the runtime makes
/// and gives the caller its end of ([`Stdio::ask`], [`crate::Proc::streams`]), or the null device.
#[derive(Debug, Default)]
pub struct Stdio {
    pub stdin: Option<PipeReader>,
    pub stdout: Option<PipeWriter>,
    pub stderr: Option<PipeWriter>,
    /// The streams the runtime makes: their far ends are the program's, the near ends the caller's, so the
    /// host copies no byte between them (native plan step 8). A stream both given above and asked for here
    /// is refused, naming it.
    pub ask: Ask,
}

/// Which of a program's standard streams the caller asks its runtime to make ([`Stdio::ask`]).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Ask {
    pub stdin: bool,
    pub stdout: bool,
    pub stderr: bool,
}

/// The caller's ends of the streams it asked for ([`crate::Proc::streams`]): stdin's to write, stdout's and
/// stderr's to read, each to its end as a pipe's end is.
#[derive(Debug, Default)]
pub struct Given {
    pub stdin: Option<End>,
    pub stdout: Option<End>,
    pub stderr: Option<End>,
}

/// A stream's end the caller holds: a file descriptor on Unix (a pipe's or a socket's), a handle on Windows.
#[cfg(unix)]
pub type End = std::os::fd::OwnedFd;
#[cfg(windows)]
pub type End = std::os::windows::io::OwnedHandle;

impl Stdio {
    /// Every stream the null device.
    pub fn null() -> Self {
        Self::default()
    }

    /// Makes each stream asked for as a pipe: the program's end put here, the caller's given back. A
    /// runtime whose programs take pipes as their stdio makes its asked streams so; one whose streams are
    /// elsewhere (a guest's) gives that stream's own end instead. Refused, naming the stream, when one is
    /// both given and asked for.
    pub fn make_asked(&mut self) -> Result<Given, crate::Refusal> {
        self.check()?;
        let pipe =
            || std::io::pipe().map_err(|e| crate::Refusal::os("making a stream asked for", e));
        let mut given = Given::default();
        if self.ask.stdin {
            let (r, w) = pipe()?;
            self.stdin = Some(r);
            given.stdin = Some(w.into());
        }
        if self.ask.stdout {
            let (r, w) = pipe()?;
            self.stdout = Some(w);
            given.stdout = Some(r.into());
        }
        if self.ask.stderr {
            let (r, w) = pipe()?;
            self.stderr = Some(w);
            given.stderr = Some(r.into());
        }
        self.ask = Ask::default();
        Ok(given)
    }

    /// Refused, naming the stream, when one is both given and asked for.
    pub fn check(&self) -> Result<(), crate::Refusal> {
        let both = [
            ("stdin", self.stdin.is_some() && self.ask.stdin),
            ("stdout", self.stdout.is_some() && self.ask.stdout),
            ("stderr", self.stderr.is_some() && self.ask.stderr),
        ];
        match both.into_iter().find(|(_, both)| *both) {
            Some((name, _)) => Err(crate::Refusal::Invalid {
                what: "stdio",
                why: format!("{name} is both given and asked for"),
            }),
            None => Ok(()),
        }
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

    /// docs/research/runtime/exec-size-bounds.md: do count `sh -c true` with one variable, expect Linux's
    /// count (each string and its NUL, the filename again, a pointer per string); do fit it against a block one
    /// byte short, expect a refusal naming the program and the bound's source; do a variable past the
    /// per-string bound, expect it named.
    #[test]
    fn a_program_is_counted_as_execve_counts_it_and_refused_past_a_bound() {
        let p = Program {
            args: vec!["sh".into(), "-c".into(), "true".into()],
            env: vec![("A".into(), "bcdef".into())],
            ..Program::default()
        };
        // "sh\0" "-c\0" "true\0" = 3 + 3 + 5; the filename "sh\0" = 3; "A=bcdef\0" = 8; 4 pointers.
        let size = 11 + 3 + 8 + 4 * size_of::<usize>();
        assert_eq!(p.exec_size(), size);
        let fits = ExecLimit {
            block: size,
            counts: BlockCount::Execve,
            string: None,
            unit: StringUnit::Bytes,
            source: "a test",
        };
        assert_eq!(p.fits(&fits), Ok(()));
        let short = ExecLimit {
            block: size - 1,
            ..fits
        };
        assert!(matches!(
            p.fits(&short),
            Err(Refusal::Invalid { what: "program", ref why }) if why.contains("a test")
        ));
        // Every argument fits in 6 bytes ("true\0" is the longest, 5); the variable's 8 don't.
        let one_string = ExecLimit {
            string: Some(6),
            ..fits
        };
        assert!(matches!(
            p.fits(&one_string),
            Err(Refusal::Invalid { what: "program", ref why }) if why.contains("\"A\"")
        ));
    }

    /// HEX-RT-14: a per-string bound counts in its OS's own unit. Do: a variable of three two-byte
    /// characters, "A=ééé": 8 bytes with its NUL, 6 UTF-16 units. Expect a bound of 6 to take it in UTF-16
    /// units (Windows' count) and refuse it in bytes (Linux's), naming the variable.
    #[test]
    fn a_per_string_bound_counts_in_the_os_unit() {
        let p = Program {
            args: vec!["sh".into()],
            env: vec![("A".into(), "ééé".into())],
            ..Program::default()
        };
        let utf16 = ExecLimit {
            block: usize::MAX,
            counts: BlockCount::Execve,
            string: Some(6),
            unit: StringUnit::Utf16,
            source: "a test",
        };
        assert_eq!(p.fits(&utf16), Ok(()));
        let bytes = ExecLimit {
            unit: StringUnit::Bytes,
            ..utf16
        };
        assert!(matches!(
            p.fits(&bytes),
            Err(Refusal::Invalid { what: "program", ref why }) if why.contains("\"A\"")
        ));
    }

    /// HEX-RT-14: Windows' block is the environment alone, in UTF-16 units (each NAME=VALUE and its NUL;
    /// the command line is bounded apart). Do: a long argument and the variable "A=ééé" (6 units with its
    /// NUL). Expect a 6-unit environment block to take it whatever the argument's length, and a 5-unit one
    /// to refuse it naming the environment.
    #[test]
    fn a_windows_block_counts_the_environment_alone_in_its_unit() {
        let p = Program {
            args: vec!["cmd".into(), "x".repeat(1000)],
            env: vec![("A".into(), "ééé".into())],
            ..Program::default()
        };
        let six = ExecLimit {
            block: 6,
            counts: BlockCount::Environment,
            string: None,
            unit: StringUnit::Utf16,
            source: "a test",
        };
        assert_eq!(six.size(&p), 6);
        assert_eq!(p.fits(&six), Ok(()));
        let five = ExecLimit { block: 5, ..six };
        assert!(matches!(
            p.fits(&five),
            Err(Refusal::Invalid { what: "program", ref why }) if why.contains("environment")
        ));
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

    /// AC-T7.1.1 (one code path, CLAUDE.md §8): a spec for a Windows node names Windows paths — a host
    /// path and a container path drive-absolute — and they pass as POSIX ones do; a drive-relative path
    /// (`C:data`), a UNC path, and a `..` behind a backslash are refused by the field's name.
    #[test]
    fn a_windows_path_passes_where_its_form_is_absolute() {
        let mut s = spec();
        s.rootfs = Rootfs::Layers(vec![r"D:\layers\base".into(), "C:/layers/app".into()]);
        s.mounts[0].source = r"D:\srv\data".into();
        s.mounts[0].destination = r"C:\data".to_owned();
        s.init.cwd = Some(r"c:\work".to_owned());
        assert_eq!(s.check(), Ok(()));
        type Break = Box<dyn Fn(&mut OciSpec)>;
        let cases: Vec<(&str, Break)> = vec![
            (
                "rootfs.layers[0]",
                Box::new(|s| s.rootfs = Rootfs::Layers(vec![r"D:layers".into()])),
            ),
            (
                "rootfs.layers[0]",
                Box::new(|s| s.rootfs = Rootfs::Layers(vec![r"\\server\share".into()])),
            ),
            (
                "mounts[0].destination",
                Box::new(|s| s.mounts[0].destination = r"C:\data\..\Windows".to_owned()),
            ),
            (
                "mounts[0].destination is the root",
                Box::new(|s| s.mounts[0].destination = r"C:\".to_owned()),
            ),
            (
                "init.cwd",
                Box::new(|s| s.init.cwd = Some("C:work".to_owned())),
            ),
        ];
        for (field, break_it) in cases {
            let mut w = s.clone();
            break_it(&mut w);
            refused_naming(&w, field);
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
