//! The rule a Windows `oci` box is made by (docs/design/runtime.md, decision "the Windows `oci` class is an
//! HCS silo holding a Hex job"): which images a host runs process-isolated, the container a bundle's
//! `windows` section or an [`OciSpec`] asks for (every field honored or refused by name, CLAUDE.md §3), the
//! HCS v2 document that makes it, what an HCS error means, and the wire between the node and the launcher
//! it starts in each box. Sans-IO (CLAUDE.md §9): `hex_runtime::windows::oci` calls HCS and reads the host.
//! Sources: docs/research/runtime/windows-hcs.md (cited as note §n).

use std::fmt::Write as _;

use hex_api::{Mount, Network, OciSpec, Program, Refusal, Rootfs, drive_absolute};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// The platform a refusal here names.
pub const PLATFORM: &str = "windows";

/// Windows Server 2022's build, the image Windows 11 runs across builds (note §7).
pub const WS2022: u32 = 20_348;
/// Windows Server 2025's build, and Windows 11 24H2's, from which Windows 11 runs Server 2025 images too.
pub const WS2025: u32 = 26_100;
/// Windows 11's first build: a client host below it is Windows 10, which runs no image process-isolated.
pub const WINDOWS_11: u32 = 22_000;

/// The owner HCS records for Hex's containers, by which the node finds its own after a crash
/// (`HcsEnumerateComputeSystems`, note §2).
pub const OWNER: &str = "hex";

/// Where every box sees the folder holding Hex's own launcher, mapped read-only (the decision's launcher).
pub const LAUNCHER_DIR: &str = r"C:\hex";

/// The longest command line a member may have, in UTF-16 units: CreateProcessW's lpCommandLine "is
/// 32,767 characters, including the Unicode terminating null character" (Microsoft's CreateProcessW
/// reference). Longer is refused by name.
pub const COMMAND_LINE_UNITS: usize = 32_766;

/// A launch request's fixed part, in bytes: its JSON with every handle `u64::MAX`, an empty command line
/// and environment, a null folder and the newline (measured from the message's shape by the test that
/// pins it).
pub const WIRE_FIXED: usize = 140;

/// The longest request or reply on the launcher's wire, in bytes, derived (Ada's rule on constants,
/// 2026-11-06): the fixed part; the command line at JSON's worst six bytes a UTF-16 unit (a `\u00XX`
/// escape); and the environment and folder, which come from a `config.json` of at most
/// [`crate::bundle::MAX_CONFIG_BYTES`], at most three bytes out per byte in (an entry's text costs no more
/// escaped here than in valid JSON there, and its five bytes of brackets and quotes against the three of
/// `"",` keep each entry under three). Windows sets no limit on an environment block (Vista on), so the
/// config's bound is the one that holds. Longer is refused, not read on (CLAUDE.md §2).
pub const MAX_WIRE: usize =
    WIRE_FIXED + 6 * COMMAND_LINE_UNITS + 3 * crate::bundle::MAX_CONFIG_BYTES;

/// The longest folder a member may start in, in UTF-16 units: Windows' longest path, 32,767 with its NUL
/// (Microsoft's "Maximum Path Length Limitation"). Longer is refused by name.
pub const FOLDER_UNITS: usize = 32_766;

/// The longest environment variable, `KEY=VALUE` with its NUL, in UTF-16 units: "The maximum size of a
/// user-defined environment variable is 32,767 characters" (Microsoft's "Environment Variables").
pub const ENV_VAR_UNITS: usize = 32_767;

/// The most a Windows program's environment may hold, in UTF-16 units (each `KEY=VALUE` and its NUL),
/// derived (the lead's decision, 2026-11-06): Windows sets no limit on the block (Vista on), and an `oci`
/// box's program crosses the launcher's wire in one message, so the block is what [`MAX_WIRE`] leaves
/// once the fixed part, the worst command line and the worst folder are paid. That leftover is
/// `3 × MAX_CONFIG_BYTES − (6 × FOLDER_UNITS + 2 − 4)` bytes (a folder's quotes and escapes in place of the
/// fixed part's `null`), and an entry costs at most six JSON bytes per counted unit (its text at the
/// `\u00XX` escape's six, its eight bytes of brackets, quotes and comma against its `=` and NUL's two), so
/// the block is the leftover over six and always fits; `encode`'s refusal by name stays the backstop. The
/// `process` class takes the same block: one Windows rule for both classes, so a program moves between
/// them unchanged (a stated policy; nothing in CreateProcessW bounds it).
pub const ENV_BLOCK_UNITS: usize = (3 * crate::bundle::MAX_CONFIG_BYTES - 6 * FOLDER_UNITS + 2) / 6;

/// A Windows program's exec bound (HEX-RT-14), the same for the `process` and `oci` classes: each
/// NAME=VALUE with its NUL at most [`ENV_VAR_UNITS`] UTF-16 units, and the whole at most
/// [`ENV_BLOCK_UNITS`] in [`Program::exec_size`]'s count, which is sound because UTF-8 never takes fewer
/// bytes than UTF-16 takes units, so a program within it fits the wire (and its arguments and pointers
/// counted too only make it stricter). The command line is bounded apart ([`COMMAND_LINE_UNITS`]).
pub fn exec_limit() -> hex_api::ExecLimit {
    hex_api::ExecLimit {
        block: ENV_BLOCK_UNITS,
        string: Some(ENV_VAR_UNITS),
        unit: hex_api::StringUnit::Utf16,
        source: "Windows (a variable 32,767 UTF-16 units with its NUL; the environment what the launcher's wire leaves)",
    }
}

/// Refuses a command line past [`COMMAND_LINE_UNITS`], naming the limit.
fn command_line_fits(field: &str, line: &str) -> Result<(), Refusal> {
    let units = line.encode_utf16().count();
    if units > COMMAND_LINE_UNITS {
        return Err(invalid(format!(
            "{field}'s command line is {units} UTF-16 units, past CreateProcessW's 32,766 (32,767 with its NUL)"
        )));
    }
    Ok(())
}

/// What kind of Windows the host is: a client SKU (Windows 10, 11) or a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKind {
    Client,
    Server,
}

/// The host's Windows build and kind (`RtlGetVersion`'s build number and product type).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Host {
    pub build: u32,
    pub kind: HostKind,
}

/// Whether `host` runs an image of build `image` process-isolated, by Microsoft's table
/// (`version-compatibility.md:24-82`, note §7); a pair the table doesn't allow is refused naming both builds.
pub fn image_runs(host: Host, image: u32) -> Result<(), Refusal> {
    let allowed = match host.kind {
        HostKind::Server => image == host.build || (host.build >= WS2025 && image == WS2022),
        HostKind::Client => {
            host.build >= WINDOWS_11
                && (image == WS2022 || (image == WS2025 && host.build >= WS2025))
        }
    };
    if allowed {
        return Ok(());
    }
    Err(Refusal::Platform {
        what: "the oci class's image",
        platform: PLATFORM,
        why: mismatch(host, Some(image)),
    })
}

/// The refusal text for an image the host doesn't run: both builds, and what the host does run.
fn mismatch(host: Host, image: Option<u32>) -> String {
    let image = image.map_or_else(
        || "this image".to_owned(),
        |b| format!("an image of build {b}"),
    );
    let runs = match host.kind {
        HostKind::Server if host.build >= WS2025 => {
            format!(
                "images of its own build {} or Server 2022's {WS2022}",
                host.build
            )
        }
        HostKind::Server => format!("images of its own build {}", host.build),
        HostKind::Client if host.build < WINDOWS_11 => {
            "no image process-isolated (Windows 10)".to_owned()
        }
        HostKind::Client if host.build >= WS2025 => {
            format!("Server 2022 ({WS2022}) and Server 2025 ({WS2025}) images")
        }
        HostKind::Client => format!("Server 2022 ({WS2022}) images"),
    };
    format!(
        "{image} doesn't run process-isolated on this {} host of build {}, which runs {runs}",
        match host.kind {
            HostKind::Client => "Windows client",
            HostKind::Server => "Windows Server",
        },
        host.build
    )
}

/// What an HCS error means for the box (note §2): the codes the class acts on, in either of the two forms
/// HCS returns them (HRESULT 0x8037xxxx, or NTSTATUS-shaped 0xC037xxxx).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HcsError {
    /// The image's build doesn't match the host's (`HCS_E_IMAGE_MISMATCH`).
    ImageMismatch,
    /// The compute system has stopped already (`HCS_E_SYSTEM_ALREADY_STOPPED`, `..._PROCESS_ALREADY_STOPPED`).
    AlreadyStopped,
    /// No such compute system (`HCS_E_SYSTEM_NOT_FOUND`).
    NotFound,
    /// The operation's wait ran out (`HCS_E_OPERATION_TIMEOUT`).
    Timeout,
    /// The compute service isn't there (`HCS_E_SERVICE_NOT_AVAILABLE`).
    ServiceUnavailable,
    /// The layer's filter won't be detached (`ERROR_FLT_DO_NOT_DETACH`): what `UnprepareLayer` answers
    /// once a container has run on the layer, whose end took the filter with it (jupiter, 2026-11-06;
    /// `DeactivateLayer` then succeeds and the volume is gone, note §3).
    FilterHeld,
    /// Any other failure, its code kept for the refusal.
    Other,
}

/// The kind of HCS error `code` is.
pub fn hcs_error(code: u32) -> HcsError {
    // The facility and code, with the severity bits that tell the two forms apart masked off.
    match code & 0x0FFF_FFFF {
        0x0037_0101 => HcsError::ImageMismatch,
        0x0037_0110 | 0x0037_011F => HcsError::AlreadyStopped,
        0x0037_010E => HcsError::NotFound,
        0x0037_0118 => HcsError::Timeout,
        0x0037_0114 => HcsError::ServiceUnavailable,
        0x001F_0010 => HcsError::FilterHeld,
        _ => HcsError::Other,
    }
}

/// The refusal for HCS failing `op` with `code` and its result document's message, if it gave one:
/// an image mismatch refused as the version check refuses it, anything else as the OS failure it is.
pub fn refusal(op: &'static str, code: u32, host: Host, message: Option<&str>) -> Refusal {
    match hcs_error(code) {
        HcsError::ImageMismatch => Refusal::Platform {
            what: "the oci class's image",
            platform: PLATFORM,
            why: format!("HCS refused it ({code:#010x}): {}", mismatch(host, None)),
        },
        HcsError::ServiceUnavailable => containers_off(&format!(
            "the Host Compute Service isn't available ({code:#010x})"
        )),
        _ => Refusal::Os {
            op,
            why: match message {
                Some(m) => format!("HCS {code:#010x}: {m}"),
                None => format!("HCS {code:#010x}"),
            },
        },
    }
}

/// The class refused because the Containers feature is off, saying how it comes on (Ada's call,
/// 2026-10-31: the installer turns it on).
pub fn containers_off(what_is_missing: &str) -> Refusal {
    Refusal::Platform {
        what: "the oci class",
        platform: PLATFORM,
        why: format!(
            "{what_is_missing}: Windows' Containers feature is off on this machine; Hex's installer turns it on"
        ),
    }
}

/// A Windows container, every value checked: what its HCS document and its init need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Container {
    /// The image's read-only layer folders, topmost first (the spec's order, `config-windows.md:8`).
    pub layers: Vec<String>,
    /// Its scratch.
    pub scratch: Scratch,
    /// Host folders mapped in, in order.
    pub mounts: Vec<MappedDir>,
    /// Device interface classes granted, each a GUID in upper case without braces.
    pub devices: Vec<String>,
    pub hostname: Option<String>,
    /// Lines to add to the container's `hosts` file, a name and its address.
    pub hosts: Vec<(String, String)>,
    /// What the whole container is bounded by, through HCS (the spec's `windows.resources`).
    pub resources: Resources,
    pub init: Process,
}

/// A container's scratch: a folder the node prepares (from the base layer's `blank.vhdx`), or a volume
/// the caller mounted already (the spec's `root.path`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scratch {
    Folder(String),
    Volume(String),
}

/// A host folder mapped into the container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedDir {
    pub host: String,
    /// Where the container sees it: a drive-absolute path.
    pub container: String,
    pub read_only: bool,
}

/// What HCS bounds the whole container by (`Container.Memory`, `Container.Processor`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Resources {
    pub memory_mb: Option<u64>,
    pub processor: Option<Processor>,
}

/// One of HCS's mutually exclusive processor controls (`SchemaReference.md:1464`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Processor {
    Count(u32),
    /// Cycles per 10,000 (`config-windows.md:84`).
    Maximum(u32),
    /// 0 to 10,000 (`config-windows.md:83`).
    Weight(u32),
}

/// A program to run in the container.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Process {
    pub command_line: String,
    pub env: Vec<(String, String)>,
    pub cwd: Option<String>,
    /// The account it runs as (`process.user.username`), the image's default where `None`.
    pub user: Option<String>,
}

fn invalid(why: String) -> Refusal {
    Refusal::Invalid {
        what: "oci spec",
        why,
    }
}

fn unsupported(field: &str, why: &str) -> Refusal {
    invalid(format!(
        "{field} isn't supported by the Windows oci class: {why}"
    ))
}

/// `path` with `/` as `\` and no trailing separator past the root, for comparing container paths.
fn normal(path: &str) -> String {
    let p = path.replace('/', "\\");
    let trimmed = p.trim_end_matches('\\');
    if trimmed.len() <= 2 {
        format!("{trimmed}\\")
    } else {
        trimmed.to_owned()
    }
}

/// A host path a Windows container takes: drive-absolute, without a NUL or a `..` component
/// (`config.md:82`: "UNC paths and mapped drives are not supported").
fn host_path(field: &str, path: &str) -> Result<String, Refusal> {
    if path.contains('\0') || !drive_absolute(path) {
        return Err(invalid(format!(
            "{field} {path:?} isn't a drive-absolute host path (C:\\...), which a Windows container takes"
        )));
    }
    if path.split(['\\', '/']).any(|p| p == "..") {
        return Err(invalid(format!("{field} {path:?} has a '..' component")));
    }
    Ok(normal(path))
}

/// A path in the container: drive-absolute as given, or a POSIX-form absolute path read on the
/// container's system drive (`/data` is `C:\data`, a stated rule), without a NUL or a `..` component.
fn container_path(field: &str, path: &str) -> Result<String, Refusal> {
    if path.contains('\0') {
        return Err(invalid(format!("{field} {path:?} holds a NUL")));
    }
    let windows = if drive_absolute(path) {
        path.to_owned()
    } else if path.starts_with('/') {
        format!("C:{path}")
    } else {
        return Err(invalid(format!(
            "{field} {path:?} isn't an absolute path in the container"
        )));
    };
    if windows.split(['\\', '/']).any(|p| p == "..") {
        return Err(invalid(format!("{field} {path:?} has a '..' component")));
    }
    let units = windows.encode_utf16().count();
    if units > FOLDER_UNITS {
        return Err(invalid(format!(
            "{field} is {units} UTF-16 units, past Windows' longest path of 32,766 (32,767 with its NUL)"
        )));
    }
    Ok(normal(&windows))
}

/// Whether container path `inner` is `outer` or under it (case-insensitive, as Windows paths are).
fn within(inner: &str, outer: &str) -> bool {
    let (i, o) = (inner.to_ascii_lowercase(), outer.to_ascii_lowercase());
    i == o || i.starts_with(&format!("{}\\", o.trim_end_matches('\\')))
}

/// Refuses any mount whose destination is nested in another's, or in the launcher's folder
/// (`config.md:75`: "One mount destination MUST NOT be nested within another mount").
fn check_nesting(mounts: &[MappedDir]) -> Result<(), Refusal> {
    for (i, m) in mounts.iter().enumerate() {
        if within(&m.container, LAUNCHER_DIR) || within(LAUNCHER_DIR, &m.container) {
            return Err(invalid(format!(
                "mounts[{i}].destination {:?} overlaps {LAUNCHER_DIR}, where every box sees Hex's launcher",
                m.container
            )));
        }
        if normal(&m.container).len() <= 3 {
            return Err(invalid(format!(
                "mounts[{i}].destination {:?} is a drive's root: a mount over it would hide the rootfs",
                m.container
            )));
        }
        for (j, n) in mounts.iter().enumerate().skip(i.saturating_add(1)) {
            if within(&m.container, &n.container) || within(&n.container, &m.container) {
                return Err(invalid(format!(
                    "mounts[{i}].destination {:?} and mounts[{j}].destination {:?} are nested, which a \
                     Windows container refuses",
                    m.container, n.container
                )));
            }
        }
    }
    Ok(())
}

/// A device interface class GUID, checked and written upper case without braces.
pub fn class_guid(field: &str, id: &str) -> Result<String, Refusal> {
    let bare = id.trim_start_matches('{').trim_end_matches('}');
    let groups: Vec<&str> = bare.split('-').collect();
    let shape = [8, 4, 4, 4, 12];
    let ok = groups.len() == shape.len()
        && groups
            .iter()
            .zip(shape)
            .all(|(g, n)| g.len() == n && g.chars().all(|c| c.is_ascii_hexdigit()));
    if !ok {
        return Err(invalid(format!(
            "{field} {id:?} isn't a device interface class GUID"
        )));
    }
    Ok(bare.to_ascii_uppercase())
}

/// The container a [`Program`] runs as in a Windows container: its command line quoted as the C runtime
/// reads it, its whole environment, its working folder in the container. A uid and gid are refused: a
/// Windows process runs as a named account (`config.md:474`).
pub fn process(field: &str, program: &Program) -> Result<Process, Refusal> {
    program.check().map_err(|e| match e {
        Refusal::Invalid { why, .. } => invalid(format!("{field}: {why}")),
        other => other,
    })?;
    if program.user.is_some() {
        return Err(unsupported(
            &format!("{field}.user"),
            "a Windows process runs as a named account, not a uid and gid",
        ));
    }
    let cwd = match &program.cwd {
        Some(c) => Some(container_path(&format!("{field}.cwd"), c)?),
        None => None,
    };
    let command_line = crate::job::command_line(&program.args);
    // The command line's own limit first: it is what bounds a Windows program's arguments.
    command_line_fits(field, &command_line)?;
    // Then the exec bound, refused as the program it is, as every class refuses it (HEX-RT-14).
    program.fits(&exec_limit())?;
    Ok(Process {
        command_line,
        env: program.env.clone(),
        cwd,
        user: None,
    })
}

/// The container an [`OciSpec`] asks for, its scratch the folder `scratch` the node made for the box:
/// `Rootfs::Layers` lowest first becomes the spec's topmost-first order; a `Rootfs::Dir`, the host's
/// network and a uid/gid user are refused by name. [`OciSpec::check`] runs first, so a value no runtime
/// can render is refused as it is everywhere.
pub fn from_spec(spec: &OciSpec, scratch: &str) -> Result<Container, Refusal> {
    spec.check()?;
    let layers = match &spec.rootfs {
        Rootfs::Dir(_) => {
            return Err(unsupported(
                "rootfs.dir",
                "a Windows container runs from image layers over a scratch, not a folder",
            ));
        }
        Rootfs::Layers(layers) => layers
            .iter()
            .enumerate()
            .rev()
            .map(|(i, l)| host_path(&format!("rootfs.layers[{i}]"), &l.to_string_lossy()))
            .collect::<Result<Vec<_>, _>>()?,
    };
    if spec.network == Network::Host {
        return Err(unsupported(
            "network",
            "Hex asks HCS for no network for a Windows container (it holds no HNS endpoints), so it can't give one the host's",
        ));
    }
    let mounts = spec
        .mounts
        .iter()
        .enumerate()
        .map(|(i, m): (usize, &Mount)| {
            Ok(MappedDir {
                host: host_path(&format!("mounts[{i}].source"), &m.source.to_string_lossy())?,
                container: container_path(&format!("mounts[{i}].destination"), &m.destination)?,
                read_only: m.read_only,
            })
        })
        .collect::<Result<Vec<_>, Refusal>>()?;
    check_nesting(&mounts)?;
    Ok(Container {
        layers,
        scratch: Scratch::Folder(host_path("scratch", scratch)?),
        mounts,
        devices: Vec::new(),
        hostname: spec.hostname.clone(),
        hosts: spec.hosts.clone(),
        resources: Resources::default(),
        init: process("init", &spec.init)?,
    })
}

/// The fields of `object` (named `at`) that aren't in `known`, refused by the first one's name: a field
/// Hex doesn't render is never dropped (CLAUDE.md §3).
fn only(at: &str, object: &Map<String, Value>, known: &[&str]) -> Result<(), Refusal> {
    match object.keys().find(|k| !known.contains(&k.as_str())) {
        Some(k) => Err(unsupported(
            &format!("{at}{k}"),
            "Hex doesn't render it, and won't run a spec it would half-apply",
        )),
        None => Ok(()),
    }
}

fn object<'a>(field: &str, v: &'a Value) -> Result<&'a Map<String, Value>, Refusal> {
    v.as_object()
        .ok_or_else(|| invalid(format!("{field} isn't an object")))
}

fn string<'a>(field: &str, v: &'a Value) -> Result<&'a str, Refusal> {
    v.as_str()
        .ok_or_else(|| invalid(format!("{field} isn't a string")))
}

fn array<'a>(field: &str, v: &'a Value) -> Result<&'a Vec<Value>, Refusal> {
    v.as_array()
        .ok_or_else(|| invalid(format!("{field} isn't an array")))
}

fn uint(field: &str, v: &Value) -> Result<u64, Refusal> {
    v.as_u64()
        .ok_or_else(|| invalid(format!("{field} isn't a whole number")))
}

/// The container a runtime-spec bundle's `config.json` asks for, by the spec's Windows section
/// (`config-windows.md`, `config.md`'s Windows lines; note §1). Every field is honored or refused by name.
pub fn from_bundle(config: &Value) -> Result<Container, Refusal> {
    let top = object("config.json", config)?;
    only(
        "",
        top,
        &[
            "ociVersion",
            "process",
            "root",
            "hostname",
            "mounts",
            "windows",
            "annotations",
        ],
    )?;
    if let Some(a) = top.get("annotations")
        && !object("annotations", a)?.is_empty()
    {
        return Err(unsupported(
            "annotations",
            "they configure other runtimes; Hex reads none",
        ));
    }
    let windows = object(
        "windows",
        top.get("windows").ok_or_else(|| {
            invalid(
                "windows is missing: a Windows container's spec MUST set it (config.md:530)"
                    .to_owned(),
            )
        })?,
    )?;
    only(
        "windows.",
        windows,
        &[
            "layerFolders",
            "devices",
            "resources",
            "network",
            "credentialSpec",
            "servicing",
            "ignoreFlushesDuringBoot",
            "hyperv",
        ],
    )?;
    if windows.contains_key("hyperv") {
        return Err(unsupported(
            "windows.hyperv",
            "it asks for a Hyper-V container; this class runs process-isolated ones",
        ));
    }
    if windows.contains_key("credentialSpec") {
        return Err(unsupported(
            "windows.credentialSpec",
            "Hex holds no group Managed Service Account",
        ));
    }
    for flag in ["servicing", "ignoreFlushesDuringBoot"] {
        if windows.get(flag).is_some_and(|v| v != &Value::Bool(false)) {
            return Err(unsupported(
                &format!("windows.{flag}"),
                "Hex runs no servicing container and keeps the scratch's flushes",
            ));
        }
    }
    if let Some(n) = windows.get("network")
        && !object("windows.network", n)?.is_empty()
    {
        return Err(unsupported(
            "windows.network",
            "Hex holds no HNS endpoints yet; a container without them has no network",
        ));
    }
    let folders = array(
        "windows.layerFolders",
        windows
            .get("layerFolders")
            .ok_or_else(|| invalid("windows.layerFolders is missing".to_owned()))?,
    )?;
    let mut layers = folders
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let field = format!("windows.layerFolders[{i}]");
            host_path(&field, string(&field, f)?)
        })
        .collect::<Result<Vec<_>, _>>()?;
    // The last folder is the scratch (`config-windows.md:8`); at least one read-only layer is under it.
    let scratch_folder = layers.pop();
    if layers.is_empty() {
        return Err(invalid(
            "windows.layerFolders holds no image layer under its scratch".to_owned(),
        ));
    }
    let scratch = match top.get("root") {
        Some(root) => {
            let root = object("root", root)?;
            only("root.", root, &["path", "readonly"])?;
            if root
                .get("readonly")
                .is_some_and(|v| v != &Value::Bool(false))
            {
                return Err(unsupported(
                    "root.readonly",
                    "a Windows container's root MUST NOT be read-only (config.md:44)",
                ));
            }
            match root.get("path") {
                Some(p) => Scratch::Volume(volume_path(string("root.path", p)?)?),
                None => Scratch::Folder(scratch_folder.unwrap_or_default()),
            }
        }
        None => Scratch::Folder(scratch_folder.unwrap_or_default()),
    };
    let devices = match windows.get("devices") {
        Some(d) => array("windows.devices", d)?
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let field = format!("windows.devices[{i}]");
                let d = object(&field, d)?;
                only(&format!("{field}."), d, &["id", "idType"])?;
                let id_type = string(
                    &format!("{field}.idType"),
                    d.get("idType").unwrap_or(&Value::Null),
                )?;
                if id_type != "class" && id_type != "vpci-class-guid" {
                    return Err(unsupported(
                        &format!("{field}.idType"),
                        "a device is granted only by its interface class (config-windows.md:31)",
                    ));
                }
                class_guid(
                    &format!("{field}.id"),
                    string(&format!("{field}.id"), d.get("id").unwrap_or(&Value::Null))?,
                )
            })
            .collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };
    let resources = match windows.get("resources") {
        Some(r) => resources(object("windows.resources", r)?)?,
        None => Resources::default(),
    };
    let mounts = match top.get("mounts") {
        Some(m) => array("mounts", m)?
            .iter()
            .enumerate()
            .map(|(i, m)| mapped_dir(i, m))
            .collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };
    check_nesting(&mounts)?;
    let hostname = match top.get("hostname") {
        Some(h) => Some(string("hostname", h)?.to_owned()),
        None => None,
    };
    Ok(Container {
        layers,
        scratch,
        mounts,
        devices,
        hostname,
        hosts: Vec::new(),
        resources,
        init: bundle_process(
            top.get("process")
                .ok_or_else(|| invalid("process is missing".to_owned()))?,
        )?,
    })
}

/// A volume GUID path, `\\?\Volume{GUID}\` (`config.md:36`), written with its trailing backslash.
fn volume_path(p: &str) -> Result<String, Refusal> {
    let guid = p
        .strip_prefix(r"\\?\Volume{")
        .and_then(|r| r.trim_end_matches('\\').strip_suffix('}'));
    match guid {
        Some(g) if class_guid("root.path", g).is_ok() => Ok(format!(r"\\?\Volume{{{g}}}\")),
        _ => Err(invalid(format!(
            "root.path {p:?} isn't a volume GUID path (\\\\?\\Volume{{GUID}}\\, config.md:36)"
        ))),
    }
}

fn resources(r: &Map<String, Value>) -> Result<Resources, Refusal> {
    only("windows.resources.", r, &["memory", "cpu"])?;
    let memory_mb = match r.get("memory") {
        Some(m) => {
            let m = object("windows.resources.memory", m)?;
            only("windows.resources.memory.", m, &["limit"])?;
            match m.get("limit") {
                // HCS takes whole MiB; a limit is rounded down, so the container is never held looser
                // than asked, and one under a MiB is refused rather than becoming no limit.
                Some(l) => match uint("windows.resources.memory.limit", l)?
                    .checked_shr(20)
                    .unwrap_or(0)
                {
                    0 => {
                        return Err(invalid(
                            "windows.resources.memory.limit is under 1 MiB, the least HCS holds"
                                .to_owned(),
                        ));
                    }
                    mb => Some(mb),
                },
                None => None,
            }
        }
        None => None,
    };
    let processor = match r.get("cpu") {
        Some(c) => {
            let c = object("windows.resources.cpu", c)?;
            only("windows.resources.cpu.", c, &["count", "shares", "maximum"])?;
            let set: Vec<(&str, u64)> = ["count", "shares", "maximum"]
                .iter()
                .filter_map(|k| c.get(*k).map(|v| (*k, v)))
                .map(|(k, v)| Ok((k, uint(&format!("windows.resources.cpu.{k}"), v)?)))
                .collect::<Result<_, Refusal>>()?;
            match set.as_slice() {
                [] => None,
                [(k, v)] => {
                    let v32 = u32::try_from(*v).ok();
                    let ranged = |lo: u32, hi: u32| v32.filter(|n| (lo..=hi).contains(n));
                    Some(match *k {
                        "count" => Processor::Count(ranged(1, u32::MAX).ok_or_else(|| {
                            invalid("windows.resources.cpu.count is 0".to_owned())
                        })?),
                        "shares" => Processor::Weight(ranged(0, 10_000).ok_or_else(|| {
                            invalid(format!(
                                "windows.resources.cpu.shares {v} is outside 0..=10000"
                            ))
                        })?),
                        _ => Processor::Maximum(ranged(1, 10_000).ok_or_else(|| {
                            invalid(format!(
                                "windows.resources.cpu.maximum {v} is outside 1..=10000"
                            ))
                        })?),
                    })
                }
                _ => {
                    return Err(invalid(
                        "windows.resources.cpu sets more than one of count, shares and maximum, which \
                         are mutually exclusive (config-windows.md:80)"
                            .to_owned(),
                    ));
                }
            }
        }
        None => None,
    };
    Ok(Resources {
        memory_mb,
        processor,
    })
}

fn mapped_dir(i: usize, m: &Value) -> Result<MappedDir, Refusal> {
    let field = format!("mounts[{i}]");
    let m = object(&field, m)?;
    only(
        &format!("{field}."),
        m,
        &["destination", "source", "options", "type"],
    )?;
    if let Some(t) = m.get("type")
        && !string(&format!("{field}.type"), t)?.is_empty()
    {
        return Err(unsupported(
            &format!("{field}.type"),
            "a Windows mount maps a host folder and has no filesystem type",
        ));
    }
    let mut read_only = false;
    if let Some(opts) = m.get("options") {
        for (j, o) in array(&format!("{field}.options"), opts)?.iter().enumerate() {
            match string(&format!("{field}.options[{j}]"), o)?
                .to_ascii_lowercase()
                .as_str()
            {
                "ro" => read_only = true,
                "rw" | "bind" | "rbind" => {}
                other => {
                    return Err(unsupported(
                        &format!("{field}.options[{j}] {other:?}"),
                        "a Windows mount takes ro or rw (config.md:87)",
                    ));
                }
            }
        }
    }
    let source = m
        .get("source")
        .ok_or_else(|| invalid(format!("{field}.source is missing")))?;
    let destination = m
        .get("destination")
        .ok_or_else(|| invalid(format!("{field}.destination is missing")))?;
    Ok(MappedDir {
        host: host_path(
            &format!("{field}.source"),
            string(&format!("{field}.source"), source)?,
        )?,
        container: container_path(
            &format!("{field}.destination"),
            string(&format!("{field}.destination"), destination)?,
        )?,
        read_only,
    })
}

fn bundle_process(p: &Value) -> Result<Process, Refusal> {
    let p = object("process", p)?;
    only(
        "process.",
        p,
        &["args", "commandLine", "env", "cwd", "user", "terminal"],
    )?;
    if p.get("terminal").is_some_and(|t| t != &Value::Bool(false)) {
        return Err(unsupported(
            "process.terminal",
            "Hex gives a box's programs pipes, not a console",
        ));
    }
    let command_line = match (p.get("commandLine"), p.get("args")) {
        (Some(c), _) => string("process.commandLine", c)?.to_owned(),
        (None, Some(a)) => {
            let args = array("process.args", a)?
                .iter()
                .enumerate()
                .map(|(i, a)| string(&format!("process.args[{i}]"), a).map(str::to_owned))
                .collect::<Result<Vec<_>, _>>()?;
            if args.is_empty() {
                return Err(invalid("process.args is empty".to_owned()));
            }
            crate::job::command_line(&args)
        }
        (None, None) => {
            return Err(invalid(
                "process has neither args nor commandLine (config.md:252)".to_owned(),
            ));
        }
    };
    if command_line.is_empty() || command_line.contains('\0') {
        return Err(invalid(
            "process's command line is empty or holds a NUL".to_owned(),
        ));
    }
    command_line_fits("process", &command_line)?;
    let env = match p.get("env") {
        Some(e) => array("process.env", e)?
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let field = format!("process.env[{i}]");
                let s = string(&field, v)?;
                match s.split_once('=') {
                    Some((k, v)) if !k.is_empty() && !s.contains('\0') => {
                        Ok((k.to_owned(), v.to_owned()))
                    }
                    _ => Err(invalid(format!("{field} {s:?} isn't NAME=value"))),
                }
            })
            .collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };
    // The init is held to the exec bound when its config is read (HEX-RT-14), counted as a program is.
    Program {
        args: vec![command_line.clone()],
        env: env.clone(),
        ..Program::default()
    }
    .fits(&exec_limit())
    .map_err(|e| invalid(format!("process: {e}")))?;
    let cwd = match p.get("cwd") {
        Some(c) => Some(container_path("process.cwd", string("process.cwd", c)?)?),
        None => None,
    };
    let user = match p.get("user") {
        Some(u) => {
            let u = object("process.user", u)?;
            only("process.user.", u, &["username"])?;
            match u.get("username") {
                Some(n) => {
                    let n = string("process.user.username", n)?;
                    if n.is_empty() || n.contains('\0') {
                        return Err(invalid(format!(
                            "process.user.username {n:?} isn't an account name"
                        )));
                    }
                    Some(n.to_owned())
                }
                None => None,
            }
        }
        None => None,
    };
    Ok(Process {
        command_line,
        env,
        cwd,
        user,
    })
}

/// A mounted scratch: its volume and the read-only layers under it, each with the id HCS knows it by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Storage {
    pub volume: String,
    /// `(id, folder)`, topmost first.
    pub layers: Vec<(String, String)>,
}

/// The id HCS knows a read-only layer by, in the container's document and in `PrepareLayer`'s
/// descriptors: a GUID derived from its folder's path, the same for the same folder in every box and
/// different for different folders. hcsshim derives it from the folder's name with vmcompute's
/// `NameToGuid`, and its CIM path uses any unique GUID (note §3); a container started on jupiter with this
/// one (note §9, probe 1), so Hex needs no `NameToGuid`. The path is compared case-insensitively,
/// as Windows compares it; the hash is FNV-1a over 128 bits (a fixed, documented function, so the id is
/// stable across Rust releases, unlike std's hasher), shaped as an RFC 9562 version 8 GUID.
pub fn layer_id(folder: &str) -> String {
    let hex = format!("{:032x}", layer_guid(folder));
    format!(
        "{}-{}-{}-{}-{}",
        hex.get(0..8).unwrap_or_default(),
        hex.get(8..12).unwrap_or_default(),
        hex.get(12..16).unwrap_or_default(),
        hex.get(16..20).unwrap_or_default(),
        hex.get(20..32).unwrap_or_default()
    )
}

/// [`layer_id`]'s GUID as the 128-bit value a `GUID` is made from (`GUID::from_u128`).
pub fn layer_guid(folder: &str) -> u128 {
    const OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
    let mut h = OFFSET;
    for b in normal(folder).to_ascii_lowercase().bytes() {
        h = (h ^ u128::from(b)).wrapping_mul(PRIME);
    }
    // Version 8 in bits 76-79, variant 0b10 in bits 62-63 (RFC 9562 §5.8).
    const VERSION_BITS: u128 = 0x0000_0000_0000_f000_0000_0000_0000_0000;
    const VERSION_8: u128 = 0x0000_0000_0000_8000_0000_0000_0000_0000;
    const VARIANT_BITS: u128 = 0x0000_0000_0000_0000_c000_0000_0000_0000;
    const VARIANT_10: u128 = 0x0000_0000_0000_0000_8000_0000_0000_0000;
    (h & !VERSION_BITS & !VARIANT_BITS) | VERSION_8 | VARIANT_10
}

/// A volume's root, `\\?\Volume{GUID}\`, with its trailing backslash whether `volume` had one or not:
/// `HcsGetLayerVhdMountPath` gives the volume without it (jupiter, 2026-11-06), runhcs passes HCS the root
/// with it, and a path joined under the root needs it.
pub fn volume_root(volume: &str) -> String {
    format!("{}\\", volume.trim_end_matches('\\'))
}

/// The HCS v2 document for `container` on `storage`, Hex's launcher folder `launcher` mapped read-only at
/// [`LAUNCHER_DIR`] (note §2): schema 2.1, no hosting system, terminated when its last handle closes.
pub fn document(container: &Container, storage: &Storage, launcher: &str) -> Value {
    let mut mapped: Vec<Value> = container
        .mounts
        .iter()
        .map(|m| json!({"HostPath": m.host, "ContainerPath": m.container, "ReadOnly": m.read_only}))
        .collect();
    // A host path HCS maps is a Windows path: `/` (a Git Bash or Cargo path's) is `\`.
    let launcher = launcher.replace('/', "\\");
    mapped.push(json!({"HostPath": launcher, "ContainerPath": LAUNCHER_DIR, "ReadOnly": true}));
    let mut c = json!({
        "Storage": {
            "Path": volume_root(&storage.volume),
            "Layers": storage.layers.iter().map(|(id, path)| json!({"Id": id, "Path": path})).collect::<Vec<_>>(),
        },
        "MappedDirectories": mapped,
    });
    if let Some(obj) = c.as_object_mut() {
        if let Some(h) = &container.hostname {
            obj.insert("GuestOs".to_owned(), json!({"HostName": h}));
        }
        if !container.devices.is_empty() {
            obj.insert(
                "AssignedDevices".to_owned(),
                container
                    .devices
                    .iter()
                    .map(|g| json!({"Type": "ClassGuid", "InterfaceClassGuid": g}))
                    .collect::<Vec<_>>()
                    .into(),
            );
        }
        if let Some(mb) = container.resources.memory_mb {
            obj.insert("Memory".to_owned(), json!({"SizeInMB": mb}));
        }
        if let Some(p) = container.resources.processor {
            let (k, v) = match p {
                Processor::Count(n) => ("Count", n),
                Processor::Maximum(n) => ("Maximum", n),
                Processor::Weight(n) => ("Weight", n),
            };
            obj.insert("Processor".to_owned(), json!({ k: v }));
        }
    }
    json!({
        "Owner": OWNER,
        "SchemaVersion": {"Major": 2, "Minor": 1},
        "ShouldTerminateOnLastHandleClosed": true,
        "Container": c,
    })
}

/// The `ProcessParameters` HCS starts the launcher with (`SchemaReference.md:2780-2793`): Hex's own binary
/// from [`LAUNCHER_DIR`], a stdin pipe for the request and a stdout pipe for the reply, as `user` (the
/// image's default account where `None`). The member's own environment and folder travel in the request.
pub fn launcher_parameters(user: Option<&str>) -> Value {
    let mut p = json!({
        "CommandLine": format!(r"{LAUNCHER_DIR}\hex.exe oci-launch"),
        "WorkingDirectory": r"C:\",
        "CreateStdInPipe": true,
        "CreateStdOutPipe": true,
        "CreateStdErrPipe": false,
    });
    if let (Some(u), Some(obj)) = (user, p.as_object_mut()) {
        obj.insert("User".to_owned(), json!(u));
    }
    p
}

/// What the node asks the launcher to start: the member's command line, environment and folder, and the
/// handle values, valid in the launcher, of the box's job and the member's three standard streams.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launch {
    pub job: u64,
    pub stdio: [u64; 3],
    pub command_line: String,
    pub env: Vec<(String, String)>,
    pub cwd: Option<String>,
}

/// The launcher's one reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Launched {
    /// The member runs in the box's job, with this pid; the launcher holds its handle until the node's
    /// stdin closes, so the pid isn't reused before the node has opened it.
    Started { pid: u32 },
    /// The box's job is at its process bound (`ERROR_NOT_ENOUGH_QUOTA`).
    Full,
    /// `op` failed with Windows error `code`.
    Failed { op: String, code: u32 },
}

/// `message` as one line of the wire: JSON and a newline, refused past [`MAX_WIRE`].
pub fn encode<T: Serialize>(message: &T) -> Result<Vec<u8>, Refusal> {
    let mut line =
        serde_json::to_vec(message).map_err(|e| Refusal::os("encoding a launcher message", e))?;
    line.push(b'\n');
    if line.len() > MAX_WIRE {
        return Err(Refusal::Invalid {
            what: "program",
            why: format!(
                "its command line and environment take {} bytes on the launcher's wire, past its {MAX_WIRE}",
                line.len()
            ),
        });
    }
    Ok(line)
}

/// One message read from the wire's bytes `line` (without or with its newline), refused if it is longer
/// than [`MAX_WIRE`], not one line, or not the message.
pub fn decode<T: for<'de> Deserialize<'de>>(line: &[u8]) -> Result<T, Refusal> {
    let bad = |why: String| Refusal::Invalid {
        what: "launcher message",
        why,
    };
    if line.len() > MAX_WIRE {
        return Err(bad(format!("{} bytes is past {MAX_WIRE}", line.len())));
    }
    let body = line.strip_suffix(b"\n").unwrap_or(line);
    if body.contains(&b'\n') {
        return Err(bad("it holds more than one line".to_owned()));
    }
    serde_json::from_slice(body).map_err(|e| bad(e.to_string()))
}

/// Checks a request the launcher read before it acts on it: a command line and environment it can hand
/// to `CreateProcessW` (no NUL; a name without `=`).
pub fn check_launch(l: &Launch) -> Result<(), Refusal> {
    let bad = |why: String| Refusal::Invalid {
        what: "launcher message",
        why,
    };
    if l.command_line.is_empty() || l.command_line.contains('\0') {
        return Err(bad("its command line is empty or holds a NUL".to_owned()));
    }
    if let Some((k, _)) = l
        .env
        .iter()
        .find(|(k, v)| k.is_empty() || k.contains(['=', '\0']) || v.contains('\0'))
    {
        return Err(bad(format!("environment variable {k:?} can't be set")));
    }
    if l.cwd.as_ref().is_some_and(|c| c.contains('\0')) {
        return Err(bad("its working folder holds a NUL".to_owned()));
    }
    Ok(())
}

/// The lines to append to the container's `hosts` file for `hosts`, each `address name` (hosts(5)'s
/// form, which Windows' `drivers\etc\hosts` shares). [`OciSpec::check`] has refused a name or address that
/// could start another line.
pub fn hosts_lines(hosts: &[(String, String)]) -> String {
    let mut s = String::new();
    for (name, address) in hosts {
        // Writing to a String can't fail; the result is dropped by `let _`.
        let _ = writeln!(s, "{address} {name}");
    }
    s
}

#[cfg(test)]
mod tests {
    use hex_api::{Mount, Network, Program};
    use proptest::prelude::*;

    use super::*;

    fn client(build: u32) -> Host {
        Host {
            build,
            kind: HostKind::Client,
        }
    }

    fn server(build: u32) -> Host {
        Host {
            build,
            kind: HostKind::Server,
        }
    }

    /// HEX-HCS-1 (AC-T7.1.1, the step's "a mismatch is refused by name"): Microsoft's table, row by row
    /// (`version-compatibility.md:24-82`): servers run their own build, Server 2025 Server 2022's too;
    /// Windows 11 runs Server 2022, and Server 2025 from 24H2; Windows 10 runs none.
    #[test]
    fn a_host_runs_the_images_microsofts_table_allows() {
        let runs = |h, i| image_runs(h, i).is_ok();
        assert!(runs(server(WS2025), WS2025));
        assert!(runs(server(WS2025), WS2022));
        assert!(runs(server(WS2022), WS2022));
        assert!(!runs(server(WS2022), WS2025));
        assert!(runs(server(17_763), 17_763));
        assert!(!runs(server(17_763), WS2022));
        assert!(!runs(server(WS2025), 17_763));
        // jupiter: Windows 11 25H2, build 26200.
        assert!(runs(client(26_200), WS2025));
        assert!(runs(client(26_200), WS2022));
        assert!(!runs(client(26_200), 17_763));
        assert!(runs(client(22_621), WS2022));
        assert!(!runs(client(22_621), WS2025));
        assert!(!runs(client(19_045), WS2022));
        assert!(!runs(client(19_045), 19_045));
    }

    /// HEX-HCS-1: a mismatch is refused by name — the image's build and the host's, and what the host runs.
    #[test]
    fn a_mismatch_names_both_builds() {
        match image_runs(client(22_621), WS2025) {
            Err(Refusal::Platform { what, why, .. }) => {
                assert_eq!(what, "the oci class's image");
                assert!(why.contains("26100") && why.contains("22621"), "{why}");
                assert!(why.contains("Server 2022 (20348)"), "{why}");
            }
            other => panic!("{other:?}"),
        }
        match image_runs(client(19_045), WS2022) {
            Err(Refusal::Platform { why, .. }) => assert!(why.contains("Windows 10"), "{why}"),
            other => panic!("{other:?}"),
        }
    }

    proptest! {
        /// HEX-HCS-1 (model-based): a server never runs another build than its own but Server 2022's on
        /// Server 2025 and later; a client never runs one Microsoft's table doesn't list.
        #[test]
        fn the_table_holds_for_any_pair(host in 10_000u32..40_000, image in 10_000u32..40_000, is_server: bool) {
            let h = if is_server { server(host) } else { client(host) };
            let want = if is_server {
                image == host || (host >= WS2025 && image == WS2022)
            } else {
                host >= WINDOWS_11 && (image == WS2022 || (image == WS2025 && host >= WS2025))
            };
            prop_assert_eq!(image_runs(h, image).is_ok(), want);
        }
    }

    /// HEX-HCS-2: HCS's codes are read in both forms it returns them, and the image mismatch becomes the
    /// version refusal while the compute service missing becomes the Containers-feature refusal.
    #[test]
    fn an_hcs_error_reads_the_same_in_either_form() {
        assert_eq!(hcs_error(0x8037_0101), HcsError::ImageMismatch);
        assert_eq!(hcs_error(0xC037_0101), HcsError::ImageMismatch);
        assert_eq!(hcs_error(0x8037_0110), HcsError::AlreadyStopped);
        assert_eq!(hcs_error(0xC037_0110), HcsError::AlreadyStopped);
        assert_eq!(hcs_error(0x8037_011F), HcsError::AlreadyStopped);
        assert_eq!(hcs_error(0xC037_010E), HcsError::NotFound);
        assert_eq!(hcs_error(0x8037_0118), HcsError::Timeout);
        assert_eq!(hcs_error(0x8007_0005), HcsError::Other);
        // UnprepareLayer after the container ran (jupiter, 2026-11-06): ERROR_FLT_DO_NOT_DETACH alone.
        assert_eq!(hcs_error(0x801F_0010), HcsError::FilterHeld);
        assert_eq!(hcs_error(0x801F_0015), HcsError::Other);
        let host = client(26_200);
        assert!(matches!(
            refusal("creating a container", 0xC037_0101, host, None),
            Refusal::Platform {
                what: "the oci class's image",
                ..
            }
        ));
        match refusal("creating a container", 0x8037_0114, host, None) {
            Refusal::Platform { what, why, .. } => {
                assert_eq!(what, "the oci class");
                assert!(
                    why.contains("Containers feature") && why.contains("installer"),
                    "{why}"
                );
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            refusal("starting a container", 0x8037_0105, host, Some("bad state")),
            Refusal::Os {
                op: "starting a container",
                why: "HCS 0x80370105: bad state".to_owned()
            }
        );
    }

    fn bundle() -> Value {
        json!({
            "ociVersion": "1.2.0",
            "process": {
                "args": ["C:\\hex\\hex.exe", "selftest-box", "wait"],
                "env": ["PATH=C:\\Windows\\System32", "EMPTY="],
                "cwd": "C:\\",
                "user": {"username": "ContainerUser"}
            },
            "hostname": "box",
            "mounts": [
                {"destination": "C:\\data", "source": "D:\\srv\\data", "options": ["ro"]},
                {"destination": "/work", "source": "D:\\srv\\work"}
            ],
            "windows": {
                "layerFolders": ["D:\\layers\\app", "D:\\layers\\base", "D:\\boxes\\b1"],
                "devices": [{"id": "{5b45201d-f2f2-4f3b-85bb-30ff1f953599}", "idType": "class"}],
                "resources": {"memory": {"limit": 268_435_456u64}, "cpu": {"maximum": 5000}}
            }
        })
    }

    /// HEX-HCS-3 (the step: layer folders, class grants, read-only mounts, memory and CPU limits): a
    /// bundle's Windows section becomes the container it asks for, the scratch its last layer folder, an
    /// empty environment value kept (hcsshim drops one, a silent drop Hex doesn't copy).
    #[test]
    fn a_bundle_becomes_the_container_its_windows_section_asks_for() {
        let c = from_bundle(&bundle()).unwrap();
        assert_eq!(c.layers, vec![r"D:\layers\app", r"D:\layers\base"]);
        assert_eq!(c.scratch, Scratch::Folder(r"D:\boxes\b1".to_owned()));
        assert_eq!(c.devices, vec!["5B45201D-F2F2-4F3B-85BB-30FF1F953599"]);
        assert_eq!(
            c.mounts,
            vec![
                MappedDir {
                    host: r"D:\srv\data".to_owned(),
                    container: r"C:\data".to_owned(),
                    read_only: true
                },
                MappedDir {
                    host: r"D:\srv\work".to_owned(),
                    container: r"C:\work".to_owned(),
                    read_only: false
                },
            ]
        );
        assert_eq!(
            c.resources,
            Resources {
                memory_mb: Some(256),
                processor: Some(Processor::Maximum(5000))
            }
        );
        assert_eq!(c.init.command_line, r"C:\hex\hex.exe selftest-box wait");
        assert!(c.init.env.contains(&("EMPTY".to_owned(), String::new())));
        assert_eq!(c.init.user.as_deref(), Some("ContainerUser"));
        assert_eq!(c.hostname.as_deref(), Some("box"));
    }

    /// HEX-HCS-3: a `root.path` the caller mounted is the scratch, as a volume GUID path.
    #[test]
    fn a_mounted_root_is_the_scratch_volume() {
        let mut b = bundle();
        b["root"] = json!({"path": "\\\\?\\Volume{ec84d99e-3f02-11e7-ac6c-00155d7682cf}"});
        let c = from_bundle(&b).unwrap();
        assert_eq!(
            c.scratch,
            Scratch::Volume(r"\\?\Volume{ec84d99e-3f02-11e7-ac6c-00155d7682cf}\".to_owned())
        );
    }

    /// HEX-HCS-4 (CLAUDE.md §3): each field Hex doesn't render, and each value the spec forbids, is refused
    /// naming the field; nothing is half-applied.
    #[test]
    fn each_field_hex_doesnt_render_is_refused_by_its_name() {
        type Break = fn(&mut Value);
        let cases: [(&str, Break); 22] = [
            ("windows.hyperv", |b| b["windows"]["hyperv"] = json!({})),
            ("windows.credentialSpec", |b| {
                b["windows"]["credentialSpec"] = json!("x")
            }),
            ("windows.servicing", |b| {
                b["windows"]["servicing"] = json!(true)
            }),
            ("windows.network", |b| {
                b["windows"]["network"] = json!({"endpointList": ["e"]})
            }),
            ("windows.resources.storage", |b| {
                b["windows"]["resources"]["storage"] = json!({"iops": 5})
            }),
            ("windows.resources.cpu", |b| {
                b["windows"]["resources"]["cpu"] = json!({"maximum": 1, "shares": 2})
            }),
            ("windows.resources.cpu.maximum", |b| {
                b["windows"]["resources"]["cpu"] = json!({"maximum": 10_001})
            }),
            ("windows.resources.memory.limit", |b| {
                b["windows"]["resources"]["memory"]["limit"] = json!(4096)
            }),
            ("windows.devices[0].idType", |b| {
                b["windows"]["devices"][0]["idType"] = json!("vpci-location-path")
            }),
            ("windows.devices[0].id", |b| {
                b["windows"]["devices"][0]["id"] = json!("COM1")
            }),
            ("windows.layerFolders holds no image layer", |b| {
                b["windows"]["layerFolders"] = json!(["D:\\boxes\\b1"])
            }),
            ("windows.layerFolders[0]", |b| {
                b["windows"]["layerFolders"][0] = json!("\\\\server\\share\\l")
            }),
            ("windows.fancy", |b| b["windows"]["fancy"] = json!(1)),
            ("linux", |b| b["linux"] = json!({})),
            ("root.readonly", |b| b["root"] = json!({"readonly": true})),
            ("root.path", |b| b["root"] = json!({"path": "C:\\rootfs"})),
            ("mounts[0].options[1] \"nosuid\"", |b| {
                b["mounts"][0]["options"] = json!(["ro", "nosuid"])
            }),
            ("mounts[1].destination", |b| {
                b["mounts"][1]["destination"] = json!("C:\\data\\inner")
            }),
            ("mounts[0].destination", |b| {
                b["mounts"][0]["destination"] = json!("C:\\hex\\x")
            }),
            ("mounts[0].source", |b| {
                b["mounts"][0]["source"] = json!("relative")
            }),
            ("process.terminal", |b| {
                b["process"]["terminal"] = json!(true)
            }),
            ("process.env[0]", |b| {
                b["process"]["env"][0] = json!("NOEQUALS")
            }),
        ];
        for (field, break_it) in cases {
            let mut b = bundle();
            break_it(&mut b);
            match from_bundle(&b) {
                Err(Refusal::Invalid {
                    what: "oci spec",
                    why,
                }) => {
                    assert!(why.contains(field), "{field}: refused as {why:?}");
                }
                other => panic!("{field}: {other:?}"),
            }
        }
    }

    fn spec() -> OciSpec {
        OciSpec {
            rootfs: Rootfs::Layers(vec![r"D:\layers\base".into(), r"D:\layers\app".into()]),
            init: Program {
                cwd: Some("/work".to_owned()),
                ..Program::of(&[r"C:\hex\hex.exe", "selftest-box", "wait"])
            },
            mounts: vec![Mount {
                source: r"D:\srv\data".into(),
                destination: "/data".to_owned(),
                read_only: true,
            }],
            hostname: Some("box".to_owned()),
            network: Network::Private,
            hosts: vec![("db".to_owned(), "10.0.0.2".to_owned())],
        }
    }

    /// HEX-HCS-5 (one code path, CLAUDE.md §8): an OciSpec renders to the same container a bundle would:
    /// its layers lowest first become the spec's topmost first, a POSIX container path lands on C:.
    #[test]
    fn an_oci_spec_becomes_the_container_it_asks_for() {
        let c = from_spec(&spec(), r"D:\hex\boxes\b1").unwrap();
        assert_eq!(c.layers, vec![r"D:\layers\app", r"D:\layers\base"]);
        assert_eq!(c.scratch, Scratch::Folder(r"D:\hex\boxes\b1".to_owned()));
        assert_eq!(c.mounts[0].container, r"C:\data");
        assert!(c.mounts[0].read_only);
        assert_eq!(c.init.cwd.as_deref(), Some(r"C:\work"));
        assert_eq!(hosts_lines(&c.hosts), "10.0.0.2 db\n");
    }

    /// HEX-HCS-5: what an OciSpec asks that a Windows container can't be is refused by name.
    #[test]
    fn an_oci_spec_a_windows_container_cant_be_is_refused_by_name() {
        type Break = fn(&mut OciSpec);
        let cases: [(&str, Break); 5] = [
            ("rootfs.dir", |s| s.rootfs = Rootfs::Dir("/rootfs".into())),
            ("network", |s| s.network = Network::Host),
            ("init.user", |s| {
                s.init.user = Some(hex_api::User::default())
            }),
            ("rootfs.layers[0]", |s| {
                s.rootfs = Rootfs::Layers(vec!["/layers/base".into()])
            }),
            ("mounts[0].source", |s| {
                s.mounts[0].source = "/srv/data".into()
            }),
        ];
        for (field, break_it) in cases {
            let mut s = spec();
            break_it(&mut s);
            match from_spec(&s, r"D:\hex\boxes\b1") {
                Err(Refusal::Invalid {
                    what: "oci spec",
                    why,
                }) => {
                    assert!(why.contains(field), "{field}: refused as {why:?}");
                }
                other => panic!("{field}: {other:?}"),
            }
        }
    }

    /// HEX-HCS-6 (note §2, jupiter 2026-11-06): do: render a document on the volume path
    /// `HcsGetLayerVhdMountPath` gave (no trailing backslash) with a launcher folder holding `/` (as a
    /// Git Bash CARGO_TARGET_DIR leaves it). Expect: `Storage.Path` is the volume's root, with its trailing
    /// backslash as runhcs passes it, and the launcher's host path uses `\` only; a root given with its
    /// backslash keeps exactly one.
    #[test]
    fn the_document_names_the_volume_root_and_a_windows_launcher_path() {
        let c = from_bundle(&bundle()).unwrap();
        let s = Storage {
            volume: r"\\?\Volume{7b88d036-aeb7-4880-9530-fdfaff8337a0}".to_owned(),
            layers: vec![("L1".to_owned(), r"D:\layers\app".to_owned())],
        };
        let d = document(&c, &s, r"C:\Users\corph/.cache/hex-native-s4/target\debug");
        let k = &d["Container"];
        assert_eq!(
            k["Storage"]["Path"],
            r"\\?\Volume{7b88d036-aeb7-4880-9530-fdfaff8337a0}\"
        );
        assert_eq!(
            k["MappedDirectories"][2]["HostPath"],
            r"C:\Users\corph\.cache\hex-native-s4\target\debug"
        );
        assert_eq!(
            volume_root(r"\\?\Volume{7b88d036-aeb7-4880-9530-fdfaff8337a0}\"),
            r"\\?\Volume{7b88d036-aeb7-4880-9530-fdfaff8337a0}\"
        );
    }

    /// HEX-HCS-6 (note §2): the document is HCS v2's for a process-isolated container: schema 2.1, Hex's
    /// owner, terminated with its last handle, the scratch volume and layers, mounts read-only where asked
    /// and the launcher's folder read-only, devices by class, and the container's memory and CPU.
    #[test]
    fn the_document_is_hcs_v2s_process_isolated_container() {
        let c = from_bundle(&bundle()).unwrap();
        let s = Storage {
            volume: r"\\?\Volume{ec84d99e-3f02-11e7-ac6c-00155d7682cf}\".to_owned(),
            layers: vec![("L1".to_owned(), r"D:\layers\app".to_owned())],
        };
        let d = document(&c, &s, r"C:\Program Files\hex");
        assert_eq!(d["Owner"], "hex");
        assert_eq!(d["SchemaVersion"], json!({"Major": 2, "Minor": 1}));
        assert_eq!(d["ShouldTerminateOnLastHandleClosed"], true);
        assert!(d.get("HostingSystemId").is_none());
        let k = &d["Container"];
        assert_eq!(k["Storage"]["Path"], s.volume);
        assert_eq!(
            k["Storage"]["Layers"],
            json!([{"Id": "L1", "Path": "D:\\layers\\app"}])
        );
        assert_eq!(k["MappedDirectories"][0]["ReadOnly"], true);
        assert_eq!(k["MappedDirectories"][1]["ReadOnly"], false);
        assert_eq!(
            k["MappedDirectories"][2],
            json!({"HostPath": "C:\\Program Files\\hex", "ContainerPath": "C:\\hex", "ReadOnly": true})
        );
        assert_eq!(
            k["AssignedDevices"],
            json!([{"Type": "ClassGuid", "InterfaceClassGuid": "5B45201D-F2F2-4F3B-85BB-30FF1F953599"}])
        );
        assert_eq!(k["Memory"], json!({"SizeInMB": 256}));
        assert_eq!(k["Processor"], json!({"Maximum": 5000}));
        assert_eq!(k["GuestOs"], json!({"HostName": "box"}));
        // No grant, no AssignedDevices: a container sees no device it wasn't given.
        let mut bare = c.clone();
        bare.devices.clear();
        assert!(
            document(&bare, &s, "C:\\h")["Container"]
                .get("AssignedDevices")
                .is_none()
        );
    }

    /// HEX-HCS-6: a layer's id is a well-formed version 8 GUID, the same for one folder however it is
    /// written (case, separators, a trailing backslash) and different for different folders.
    #[test]
    fn a_layer_id_is_one_stable_guid_per_folder() {
        let a = layer_id(r"D:\layers\base");
        assert_eq!(a, layer_id("d:/LAYERS/base/"));
        assert_ne!(a, layer_id(r"D:\layers\app"));
        assert!(class_guid("id", &a).is_ok(), "{a}");
        assert_eq!(a.get(14..15), Some("8"), "{a}");
        assert!(matches!(a.get(19..20), Some("8" | "9" | "a" | "b")), "{a}");
        assert_eq!(
            a.replace('-', ""),
            format!("{:032x}", layer_guid(r"D:\layers\base"))
        );
    }

    /// HEX-HCS-7 (Ada's rule on constants, 2026-11-06): MAX_WIRE is derived, not picked. Do: encode the
    /// largest fixed fields (u64::MAX handles, a null folder). Expect: MAX_WIRE is exactly that, plus the
    /// command line's limit at JSON's worst six bytes a unit, plus three times the config.json bound the
    /// environment and folder come from.
    #[test]
    fn max_wire_is_derived_from_the_command_line_limit_and_the_config_bound() {
        let fixed = encode(&Launch {
            job: u64::MAX,
            stdio: [u64::MAX; 3],
            command_line: String::new(),
            env: Vec::new(),
            cwd: None,
        })
        .unwrap()
        .len();
        assert_eq!(fixed, WIRE_FIXED);
        assert_eq!(
            MAX_WIRE,
            WIRE_FIXED + 6 * COMMAND_LINE_UNITS + 3 * crate::bundle::MAX_CONFIG_BYTES
        );
    }

    /// HEX-HCS-7: the wire holds the worst a bundle can bring. Do: encode a launch whose command line is
    /// CreateProcessW's limit of control characters (each a six-byte escape) and whose environment is a
    /// config.json's worth of empty entries (`"",` each). Expect: both fit MAX_WIRE.
    #[test]
    fn the_wire_holds_the_worst_command_line_and_environment() {
        let worst = Launch {
            job: u64::MAX,
            stdio: [u64::MAX; 3],
            command_line: "\u{1}".repeat(COMMAND_LINE_UNITS),
            env: vec![(String::new(), String::new()); crate::bundle::MAX_CONFIG_BYTES / 3],
            cwd: None,
        };
        let line = encode(&worst).unwrap();
        assert!(line.len() <= MAX_WIRE, "{} > {MAX_WIRE}", line.len());
    }

    /// HEX-HCS-4 (CreateProcessW: lpCommandLine at most 32,767 characters with its NUL): do: a program and
    /// a bundle whose command line is one unit past the limit. Expect: each refused naming the limit; one
    /// at the limit is taken.
    #[test]
    fn a_command_line_past_createprocessw_s_limit_is_refused_by_name() {
        let at = "a".repeat(COMMAND_LINE_UNITS);
        let past = "a".repeat(COMMAND_LINE_UNITS + 1);
        assert!(process("program", &Program::of(&[at.as_str()])).is_ok());
        match process("program", &Program::of(&[past.as_str()])) {
            Err(Refusal::Invalid { why, .. }) => assert!(why.contains("32,766"), "{why}"),
            other => panic!("{other:?}"),
        }
        let mut b = bundle();
        b["process"] = json!({"commandLine": past, "cwd": "C:\\"});
        match from_bundle(&b) {
            Err(Refusal::Invalid { why, .. }) => assert!(why.contains("32,766"), "{why}"),
            other => panic!("{other:?}"),
        }
    }

    /// HEX-RT-14 on Windows (the lead's decision, 2026-11-06): the environment block's bound is what the
    /// wire leaves. Do: encode a launch at every worst at once: CreateProcessW's command line, the longest
    /// folder, and ENV_BLOCK_UNITS of environment (KEY=VALUE and its NUL counted in UTF-16 units), all in
    /// control characters, JSON's six-byte escape. Expect: it fits MAX_WIRE; and the block is the
    /// leftover over six, nothing picked.
    #[test]
    fn the_environment_block_is_what_the_wire_leaves() {
        assert_eq!(
            ENV_BLOCK_UNITS,
            (3 * crate::bundle::MAX_CONFIG_BYTES - 6 * FOLDER_UNITS + 2) / 6
        );
        // One-unit keys and values: KEY=VALUE\0 is four units, its JSON `["k","v"],` 10 bytes plus
        // the escapes, the worst case per unit.
        let entries = ENV_BLOCK_UNITS / 4;
        let worst = Launch {
            job: u64::MAX,
            stdio: [u64::MAX; 3],
            command_line: "\u{1}".repeat(COMMAND_LINE_UNITS),
            env: vec![("\u{1}".to_owned(), "\u{1}".to_owned()); entries],
            cwd: Some("\u{1}".repeat(FOLDER_UNITS)),
        };
        let line = encode(&worst).unwrap();
        assert!(line.len() <= MAX_WIRE, "{} > {MAX_WIRE}", line.len());
    }

    /// HEX-RT-14 on Windows (the lead's decisions, 2026-11-06): the class's exec bound is Microsoft's
    /// 32,767 units a variable, counted over NAME=VALUE and its NUL in UTF-16, and the environment block the
    /// launcher's wire leaves. Do: read it; give `process` a variable one unit past it, and a block past it.
    /// Expect: the values, and each program refused as an invalid program.
    #[test]
    fn a_windows_program_is_held_to_its_exec_bound() {
        let limit = exec_limit();
        assert_eq!(limit.block, ENV_BLOCK_UNITS);
        assert_eq!(limit.string, Some(ENV_VAR_UNITS));
        assert_eq!(limit.unit, hex_api::StringUnit::Utf16);
        // NAME=VALUE with its NUL at 32,768 units: "K" "=" and 32,765 of "é" (two UTF-8 bytes, one unit).
        let long = Program {
            env: vec![("K".to_owned(), "é".repeat(ENV_VAR_UNITS - 2))],
            ..Program::of(&["a"])
        };
        let block = Program {
            env: (0..ENV_BLOCK_UNITS / 1000 + 1)
                .map(|i| (format!("V{i}"), "x".repeat(1000)))
                .collect(),
            ..Program::of(&["a"])
        };
        for p in [long, block] {
            match process("program", &p) {
                Err(Refusal::Invalid {
                    what: "program", ..
                }) => {}
                other => panic!("{other:?}"),
            }
        }
        let at = Program {
            env: vec![("K".to_owned(), "é".repeat(ENV_VAR_UNITS - 3))],
            ..Program::of(&["a"])
        };
        assert!(process("program", &at).is_ok());
    }

    /// HEX-RT-14 on Windows, a bundle's init: do: a config.json whose environment holds a variable past
    /// the bound. Expect: refused when the config is read, naming the program.
    #[test]
    fn a_bundles_init_past_the_exec_bound_is_refused_at_read() {
        let mut b = bundle();
        b["process"]["env"] = json!([format!("K={}", "é".repeat(ENV_VAR_UNITS - 2))]);
        match from_bundle(&b) {
            Err(Refusal::Invalid { why, .. }) => assert!(why.contains("program"), "{why}"),
            other => panic!("{other:?}"),
        }
    }

    /// HEX-HCS-4 (Windows' longest path, 32,767 units with its NUL): do: a program whose folder is one unit
    /// past it. Expect: refused naming the limit.
    #[test]
    fn a_folder_past_the_longest_path_is_refused_by_name() {
        let past = format!("C:\\{}", "a".repeat(FOLDER_UNITS));
        let p = Program {
            cwd: Some(past),
            ..Program::of(&["a"])
        };
        match process("program", &p) {
            Err(Refusal::Invalid { why, .. }) => assert!(why.contains("32,766"), "{why}"),
            other => panic!("{other:?}"),
        }
    }

    /// HEX-HCS-7: the launcher's wire round-trips, and the launcher starts from Hex's folder with a stdin
    /// for the request and a stdout for the reply.
    #[test]
    fn the_launcher_wire_round_trips() {
        let l = Launch {
            job: 0x1a4,
            stdio: [0x1a8, 0x1ac, 0x1b0],
            command_line: r#"C:\hex\hex.exe selftest-box say "hi there""#.to_owned(),
            env: vec![("PATH".to_owned(), r"C:\Windows".to_owned())],
            cwd: Some(r"C:\".to_owned()),
        };
        let line = encode(&l).unwrap();
        assert_eq!(line.last(), Some(&b'\n'));
        assert_eq!(decode::<Launch>(&line).unwrap(), l);
        for r in [
            Launched::Started { pid: 4242 },
            Launched::Full,
            Launched::Failed {
                op: "CreateProcessW".to_owned(),
                code: 2,
            },
        ] {
            assert_eq!(decode::<Launched>(&encode(&r).unwrap()).unwrap(), r);
        }
        let p = launcher_parameters(Some("ContainerUser"));
        assert_eq!(p["CommandLine"], r"C:\hex\hex.exe oci-launch");
        assert_eq!(p["CreateStdInPipe"], true);
        assert_eq!(p["CreateStdOutPipe"], true);
        assert_eq!(p["User"], "ContainerUser");
        assert!(launcher_parameters(None).get("User").is_none());
    }

    /// HEX-HCS-7 (hostile input to every parser, CLAUDE.md §6): an oversized, multi-line or malformed
    /// message is refused, typed, never read on; a request that can't become a process is refused.
    #[test]
    fn a_hostile_launcher_message_is_refused() {
        // A well-formed message past the bound, so the bound (not the parser) is what refuses it.
        let big = serde_json::to_vec(&Launch {
            job: 1,
            stdio: [2, 3, 4],
            command_line: "y".repeat(MAX_WIRE),
            env: vec![],
            cwd: None,
        })
        .unwrap();
        match decode::<Launch>(&big) {
            Err(Refusal::Invalid {
                what: "launcher message",
                why,
            }) => assert!(why.contains(&format!("past {MAX_WIRE}")), "{why}"),
            other => panic!("an oversized message: {other:?}"),
        }
        assert!(decode::<Launched>(b"{\"full\":null}\n{}\n").is_err());
        // Two lines, the second empty: JSON would read the whitespace as trailing, so the one-line rule
        // is what refuses it.
        match decode::<Launched>(b"\"full\"\n\n") {
            Err(Refusal::Invalid { why, .. }) => {
                assert!(why.contains("more than one line"), "{why}")
            }
            other => panic!("a two-line message: {other:?}"),
        }
        assert!(decode::<Launched>(b"not json").is_err());
        let mut l = Launch {
            job: 1,
            stdio: [2, 3, 4],
            command_line: "a".to_owned(),
            env: vec![],
            cwd: None,
        };
        assert_eq!(check_launch(&l), Ok(()));
        l.env.push(("A=B".to_owned(), "c".to_owned()));
        assert!(check_launch(&l).is_err());
        l.env.clear();
        l.command_line = "a\0b".to_owned();
        assert!(check_launch(&l).is_err());
        let huge = Launch {
            command_line: "y".repeat(MAX_WIRE),
            ..l
        };
        assert!(matches!(
            encode(&huge),
            Err(Refusal::Invalid {
                what: "program",
                ..
            })
        ));
    }

    proptest! {
        /// HEX-HCS-7 (hostile input): no bytes make decode panic; whatever it accepts re-encodes to a
        /// message it accepts again.
        #[test]
        fn decode_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
            if let Ok(l) = decode::<Launch>(&bytes) {
                prop_assert_eq!(decode::<Launch>(&encode(&l).unwrap()).unwrap(), l);
            }
            let _ = decode::<Launched>(&bytes);
        }

        /// HEX-HCS-4 (hostile input): no JSON value makes the bundle reader panic.
        #[test]
        fn from_bundle_never_panics(s in "\\PC{0,200}") {
            if let Ok(v) = serde_json::from_str::<Value>(&s) {
                let _ = from_bundle(&v);
            }
        }
    }
}
