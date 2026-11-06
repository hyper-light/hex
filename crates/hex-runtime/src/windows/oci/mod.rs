//! The `oci` class on Windows (docs/design/runtime.md, decision "the Windows `oci` class is an HCS silo
//! holding a Hex job"): a Windows image run process-isolated through the Host Compute Service, as the OCI
//! runtime spec's Windows section defines it. A box is an HCS compute system on the image's layers over a
//! scratch of its own, with a job object of Hex's nested in its silo: every program Hex runs there is
//! started suspended inside that job by Hex's launcher ([`launch`]), so the box's bounds hold on its
//! members by the `process` class's own rule (`hex_core::job`), while HCS bounds the container as a whole.
//! What the spec asks is decided in `hex_core::hcs`; this does the calls.

pub mod launch;
pub mod sys;

use std::io::Read;
use std::num::NonZeroU32;
use std::os::windows::io::OwnedHandle;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use hex_api::{Bounds, BoxId, BoxState, Exit, Proc, Refusal, Signal, Usage};
use hex_core::hcs::{self, Container, HcsError, Process, Scratch, Storage};

use super::proc::BoxProc;
use super::process::JobBox;
use sys::{HcsFailure, System};

/// The scratch file a box's folder holds, a copy of the base layer's `blank.vhdx` (note §3).
const SANDBOX: &str = "sandbox.vhdx";

/// What the box made for its scratch, undone at delete: the scratch folder vmcompute activated (its
/// `sandbox.vhdx` attached as the volume), whether the storage filter was put over it, and the files Hex
/// created.
struct Mounted {
    volume: String,
    activated: Option<String>,
    prepared: bool,
    made: Vec<PathBuf>,
    /// The marker naming this scratch for a node that starts after this one was killed ([`reclaim`]),
    /// removed last, once everything else is.
    marker: Option<PathBuf>,
}

/// What a box's scratch marker records, written before Hex makes anything: the folder vmcompute activates,
/// and what of it Hex made (removed at delete, or by the next node's [`reclaim`]).
#[derive(serde::Serialize, serde::Deserialize)]
struct ScratchMarker {
    folder: String,
    made: Vec<PathBuf>,
}

/// The largest marker a reclaim reads: a folder and two paths, each at most Windows' longest path in
/// UTF-8 (32,767 units × 3 bytes), with the JSON around them (CLAUDE.md §2).
const MAX_MARKER: u64 = 3 * 3 * 32_767 + 4096;

/// The scratch markers under `root`, one per box, `<id>.scratch.json`.
const MARKER_SUFFIX: &str = ".scratch.json";

impl Mounted {
    /// Takes the filter off, detaches the volume and removes what Hex made; the first failure is returned,
    /// every step is still tried. `UnprepareLayer` answering `ERROR_FLT_DO_NOT_DETACH` is the container's
    /// end having taken the filter (note §3), matched alone; `DeactivateLayer` then detaches the volume.
    fn undo(&mut self) -> Result<(), Refusal> {
        let mut first = None;
        if let Some(scratch) = self.activated.take() {
            if std::mem::take(&mut self.prepared)
                && let Err(f) = sys::unprepare_layer(&scratch)
                && hcs::hcs_error(f.code) != HcsError::FilterHeld
            {
                first.get_or_insert(failure(
                    "taking the storage filter off a container's scratch",
                    f,
                ));
            }
            if let Err(f) = sys::deactivate_layer(&scratch) {
                first.get_or_insert(failure("detaching a container's scratch", f));
            }
        }
        for p in self.made.drain(..).rev() {
            let gone = if p.is_dir() {
                std::fs::remove_dir(&p)
            } else {
                std::fs::remove_file(&p)
            };
            if let Err(e) = gone
                && e.kind() != std::io::ErrorKind::NotFound
            {
                first.get_or_insert(Refusal::os("removing a container's scratch", e));
            }
        }
        if first.is_none()
            && let Some(m) = self.marker.take()
            && let Err(e) = std::fs::remove_file(&m)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            first.get_or_insert(Refusal::os("removing a container's scratch marker", e));
        }
        first.map_or(Ok(()), Err)
    }
}

/// Reclaims the scratch a killed node left under `root` (each `<id>.scratch.json` marker's folder):
/// detaches its volume where it is still activated (UnprepareLayer, then DeactivateLayer, which detaches it
/// whether or not the unprepare took: jupiter, 2026-11-06) and removes what Hex made, then the marker. The
/// containers ended with the node (ShouldTerminateOnLastHandleClosed, the job's kill-on-close), so the
/// scratch is all a crash leaves. Bounded by `root`'s entries, read once; each failure is returned, typed,
/// and its marker kept so the next start tries again.
pub fn reclaim(root: &Path) -> Vec<Refusal> {
    let mut failures = Vec::new();
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return failures,
        Err(e) => {
            failures.push(Refusal::os("listing the scratch markers", e));
            return failures;
        }
    };
    for entry in entries {
        let path = match entry {
            Ok(e) => e.path(),
            Err(e) => {
                failures.push(Refusal::os("listing the scratch markers", e));
                continue;
            }
        };
        if !path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(MARKER_SUFFIX))
        {
            continue;
        }
        if let Err(r) = reclaim_one(&path) {
            failures.push(r);
        }
    }
    failures
}

fn reclaim_one(marker: &Path) -> Result<(), Refusal> {
    let mut text = String::new();
    std::fs::File::open(marker)
        .and_then(|f| f.take(MAX_MARKER).read_to_string(&mut text))
        .map_err(|e| Refusal::os("reading a scratch marker", e))?;
    let m: ScratchMarker = serde_json::from_str(&text).map_err(|e| Refusal::Invalid {
        what: "scratch marker",
        why: format!("{}: {e}", marker.display()),
    })?;
    if sys::layer_mount_path(&m.folder).is_ok() {
        let unprepared = sys::unprepare_layer(&m.folder);
        sys::deactivate_layer(&m.folder).map_err(|f| {
            failure(
                "detaching a killed node's scratch",
                HcsFailure {
                    message: Some(format!(
                        "{} (unprepare: {unprepared:?})",
                        f.message.clone().unwrap_or_default()
                    )),
                    ..f
                },
            )
        })?;
    }
    for p in m.made.iter().rev() {
        let gone = if p.is_dir() {
            std::fs::remove_dir(p)
        } else {
            std::fs::remove_file(p)
        };
        if let Err(e) = gone
            && e.kind() != std::io::ErrorKind::NotFound
        {
            return Err(Refusal::os("removing a killed node's scratch", e));
        }
    }
    std::fs::remove_file(marker).map_err(|e| Refusal::os("removing a scratch marker", e))
}

impl Drop for Mounted {
    fn drop(&mut self) {
        // The backstop for a box dropped without its delete; delete reports these failures.
        let _ = self.undo();
    }
}

fn failure(op: &'static str, f: HcsFailure) -> Refusal {
    hcs::refusal(op, f.code, sys::host(), f.message.as_deref())
}

/// How the box's life stands.
struct Life {
    started: bool,
    init: Option<BoxProc>,
    exit: Option<Exit>,
}

/// A Windows `oci` box.
pub struct OciBox {
    job: JobBox,
    /// Dropped before the scratch: closing its last handle ends the container (it was created to).
    system: Mutex<Option<System>>,
    mounted: Mutex<Option<Mounted>>,
    user: Option<String>,
    init: Process,
    life: Mutex<Life>,
}

/// Whether HCS is here; `Err` is the class refused by name, saying the installer turns it on.
pub fn available() -> Result<(), Refusal> {
    sys::available().map_err(|missing| hcs::containers_off(&missing))
}

/// The container a bundle at `dir` asks for (its `config.json`, read at most
/// [`hex_core::bundle::MAX_CONFIG_BYTES`] bytes, the Linux bundle reader's bound: one rule for both).
pub fn bundle(dir: &Path) -> Result<Container, Refusal> {
    const MAX_CONFIG: usize = hex_core::bundle::MAX_CONFIG_BYTES;
    let path = dir.join("config.json");
    let mut text = String::new();
    std::fs::File::open(&path)
        .and_then(|f| {
            f.take(
                u64::try_from(MAX_CONFIG)
                    .unwrap_or(u64::MAX)
                    .saturating_add(1),
            )
            .read_to_string(&mut text)
        })
        .map_err(|e| Refusal::os("reading a bundle's config.json", e))?;
    if text.len() > MAX_CONFIG {
        return Err(Refusal::Invalid {
            what: "oci spec",
            why: format!("config.json is past {MAX_CONFIG} bytes"),
        });
    }
    let config: serde_json::Value = serde_json::from_str(&text).map_err(|e| Refusal::Invalid {
        what: "oci spec",
        why: format!("config.json isn't JSON: {e}"),
    })?;
    hcs::from_bundle(&config)
}

impl OciBox {
    /// Makes box `id` for `container`, bounded by `bounds` on a machine of `processors`, Hex's launcher
    /// mapped in from the folder `launcher`: checks the image runs on this host, makes the box's job, prepares the scratch, and creates the compute system,
    /// which waits for [`OciBox::start`].
    pub fn create(
        id: BoxId,
        bounds: &Bounds,
        processors: NonZeroU32,
        container: &Container,
        launcher: &Path,
        marker: Option<PathBuf>,
    ) -> Result<Self, Refusal> {
        available()?;
        let host = sys::host();
        let base = container.layers.last().ok_or_else(|| Refusal::Invalid {
            what: "oci spec",
            why: "the image has no layers".to_owned(),
        })?;
        let ntdll = format!(r"{base}\Files\Windows\System32\ntdll.dll");
        let image = sys::file_build(&ntdll)
            .map_err(|e| Refusal::os("reading the image's Windows build from its base layer", e))?;
        hcs::image_runs(host, image)?;
        let job = JobBox::create(id.clone(), bounds, processors)?;
        let mut mounted = scratch(container, base, marker)?;
        if !container.hosts.is_empty() {
            append_hosts(&mounted.volume, &hcs::hosts_lines(&container.hosts))?;
        }
        let storage = Storage {
            volume: mounted.volume.clone(),
            layers: container
                .layers
                .iter()
                .map(|l| (hcs::layer_id(l), l.clone()))
                .collect(),
        };
        let launcher = launcher
            .to_str()
            .ok_or_else(|| Refusal::os("naming the launcher's folder", "not UTF-8"))?;
        let document = hcs::document(container, &storage, launcher).to_string();
        let system = match sys::create_system(id.as_str(), &document, launch::PATIENCE) {
            Ok(s) => s,
            Err(f) => {
                let refused = failure("creating a container", f);
                let _ = mounted.undo();
                return Err(refused);
            }
        };
        Ok(Self {
            job,
            system: Mutex::new(Some(system)),
            mounted: Mutex::new(Some(mounted)),
            user: container.init.user.clone(),
            init: container.init.clone(),
            life: Mutex::new(Life {
                started: false,
                init: None,
                exit: None,
            }),
        })
    }

    pub fn id(&self) -> &BoxId {
        self.job.id()
    }

    pub fn bounds(&self) -> Bounds {
        self.job.bounds()
    }

    fn life(&self) -> Result<std::sync::MutexGuard<'_, Life>, Refusal> {
        self.life
            .lock()
            .map_err(|_| Refusal::Broken("an oci box's state lock is poisoned".to_owned()))
    }

    fn launch(&self, process: &Process, stdio: [OwnedHandle; 3]) -> Result<BoxProc, Refusal> {
        let system = self
            .system
            .lock()
            .map_err(|_| Refusal::Broken("an oci box's system lock is poisoned".to_owned()))?;
        let system = system
            .as_ref()
            .ok_or_else(|| Refusal::NotRunning(self.id().clone(), "exec"))?;
        let (member, pid) = launch::start(system, &self.job, self.user.as_deref(), process, stdio)?;
        Ok(BoxProc::new(member, pid, self.job.shared()))
    }

    /// Starts the container and its init (the runtime-spec's `start`); a second start is refused.
    pub fn start(&self) -> Result<(), Refusal> {
        let mut life = self.life()?;
        if life.started {
            return Err(Refusal::Invalid {
                what: "start",
                why: format!("box {} has started already", self.id()),
            });
        }
        {
            let system = self
                .system
                .lock()
                .map_err(|_| Refusal::Broken("an oci box's system lock is poisoned".to_owned()))?;
            system
                .as_ref()
                .ok_or_else(|| Refusal::NoSuchBox(self.id().clone()))?
                .start(launch::PATIENCE)
                .map_err(|f| failure("starting a container", f))?;
        }
        let init = self.launch(&self.init, super::null_stdio()?)?;
        life.started = true;
        life.init = Some(init);
        Ok(())
    }

    /// Runs `process` in the started box with `stdio`; refused before the start and once the init ended.
    pub fn exec(
        &self,
        process: &Process,
        stdio: [OwnedHandle; 3],
    ) -> Result<Box<dyn Proc>, Refusal> {
        if !matches!(self.state()?, BoxState::Running) {
            return Err(Refusal::NotRunning(self.id().clone(), "exec"));
        }
        Ok(Box::new(self.launch(process, stdio)?))
    }

    /// The box's state: `Created` until its start, `Running` while its init runs, `Empty` once it ended.
    pub fn state(&self) -> Result<BoxState, Refusal> {
        let exit = self.exit()?;
        Ok(if !self.life()?.started {
            BoxState::Created
        } else if exit.is_some() {
            BoxState::Empty
        } else {
            BoxState::Running
        })
    }

    /// How the init ended, once it has (read without blocking, kept once read).
    pub fn exit(&self) -> Result<Option<Exit>, Refusal> {
        let mut life = self.life()?;
        if life.exit.is_none()
            && let Some(init) = life.init.as_mut()
        {
            life.exit = init.try_wait()?;
        }
        Ok(life.exit)
    }

    pub fn usage(&self) -> Result<Usage, Refusal> {
        self.job.usage()
    }

    /// Ends every program in the box with `signal` (KILL or TERM, the `process` class's rule).
    pub fn signal(&self, signal: Signal) -> Result<(), Refusal> {
        self.job.signal(signal)
    }

    /// Ends the box's programs, terminates its container, and removes its scratch.
    pub fn delete(&self) -> Result<(), Refusal> {
        self.job.delete()?;
        let system = self
            .system
            .lock()
            .map_err(|_| Refusal::Broken("an oci box's system lock is poisoned".to_owned()))?
            .take();
        if let Some(system) = system {
            match system.terminate(launch::PATIENCE) {
                Ok(()) => {}
                Err(f)
                    if matches!(
                        hcs::hcs_error(f.code),
                        HcsError::AlreadyStopped | HcsError::NotFound
                    ) => {}
                Err(f) => return Err(failure("ending a container", f)),
            }
        }
        let mounted = self
            .mounted
            .lock()
            .map_err(|_| Refusal::Broken("an oci box's scratch lock is poisoned".to_owned()))?
            .take();
        mounted.map_or(Ok(()), |mut m| m.undo())
    }
}

/// Prepares `container`'s scratch over its layers, `base` the bottom-most: a scratch folder gets a copy of
/// the base layer's `blank.vhdx` (unless it holds a `sandbox.vhdx` already), which vmcompute activates as
/// a volume and puts the storage filter over, the layers its parents, as hcsshim and runhcs do; the
/// documented storage calls made a writable layer no container finished starting on (note §3, §9); a
/// volume the caller mounted is used as it is.
fn scratch(container: &Container, base: &str, marker: Option<PathBuf>) -> Result<Mounted, Refusal> {
    let folder = match &container.scratch {
        Scratch::Volume(v) => {
            return Ok(Mounted {
                volume: v.clone(),
                activated: None,
                prepared: false,
                made: Vec::new(),
                marker: None,
            });
        }
        Scratch::Folder(f) => PathBuf::from(f),
    };
    let mut mounted = Mounted {
        volume: String::new(),
        activated: None,
        prepared: false,
        made: Vec::new(),
        marker: None,
    };
    let sandbox = folder.join(SANDBOX);
    // The marker first, naming what Hex is about to make, so a node killed at any later point leaves
    // nothing the next start can't find (reclaim).
    if let Some(m) = marker {
        let mut made = Vec::new();
        if !folder.exists() {
            made.push(folder.clone());
        }
        if !sandbox.exists() {
            made.push(sandbox.clone());
        }
        let record = ScratchMarker {
            folder: folder
                .to_str()
                .ok_or_else(|| Refusal::os("naming a container's scratch", "not UTF-8"))?
                .to_owned(),
            made,
        };
        let text = serde_json::to_string(&record)
            .map_err(|e| Refusal::os("writing a scratch marker", e))?;
        if let Some(dir) = m.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Refusal::os("writing a scratch marker", e))?;
        }
        std::fs::write(&m, text).map_err(|e| Refusal::os("writing a scratch marker", e))?;
        mounted.marker = Some(m);
    }
    if !folder.exists() {
        std::fs::create_dir_all(&folder)
            .map_err(|e| Refusal::os("making a container's scratch", e))?;
        mounted.made.push(folder.clone());
    }
    if !sandbox.exists() {
        std::fs::copy(Path::new(base).join("blank.vhdx"), &sandbox)
            .map_err(|e| Refusal::os("copying the base layer's blank.vhdx as a scratch", e))?;
        mounted.made.push(sandbox.clone());
    }
    let scratch = folder
        .to_str()
        .ok_or_else(|| Refusal::os("naming a container's scratch", "not UTF-8"))?
        .to_owned();
    sys::activate_layer(&scratch).map_err(|f| failure("attaching a container's scratch", f))?;
    mounted.activated = Some(scratch.clone());
    sys::prepare_layer(&scratch, &container.layers)
        .map_err(|f| failure("putting the storage filter over a container's scratch", f))?;
    mounted.prepared = true;
    mounted.volume = sys::layer_mount_path(&scratch)
        .map_err(|f| failure("reading a container's scratch volume", f))?;
    Ok(mounted)
}

/// Appends `lines` to the container's `hosts` file on its scratch volume, before it starts.
fn append_hosts(volume: &str, lines: &str) -> Result<(), Refusal> {
    use std::io::Write;
    let path = format!(
        r"{}Windows\System32\drivers\etc\hosts",
        hcs::volume_root(volume)
    );
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(format!("\r\n{lines}").as_bytes()))
        .map_err(|e| Refusal::os("writing a container's hosts file", e))
}

/// A box's state for the runtime's `BoxInfo`.
pub fn info(b: &OciBox) -> Result<hex_api::BoxInfo, Refusal> {
    Ok(hex_api::BoxInfo {
        id: b.id().clone(),
        class: hex_api::Class::Oci,
        state: b.state()?,
        bounds: b.bounds(),
        adopted: false,
        exit: b.exit()?,
        abandoned: 0,
    })
}

/// The devices implementing interface class `class` (a GUID, braces optional) where this process runs,
/// listed and reachable: the probe that a class not granted to a box isn't reachable inside it.
pub fn devices(class: &str) -> Result<sys::DeviceView, Refusal> {
    let g = hcs::class_guid("device class", class)?;
    let field = |from: usize, to: usize| {
        g.get(from..to)
            .and_then(|h| u64::from_str_radix(h, 16).ok())
            .ok_or_else(|| Refusal::Broken(format!("a checked GUID {g} didn't parse")))
    };
    let narrow = |v: u64| u8::try_from(v).map_err(|e| Refusal::Broken(e.to_string()));
    let mut data4 = [0u8; 8];
    for (i, b) in data4.iter_mut().enumerate() {
        // The last two groups, 19..23 and 24..36, hold data4's eight bytes; skip the dash at 23.
        let at = if i < 2 {
            i.saturating_mul(2).saturating_add(19)
        } else {
            i.saturating_mul(2).saturating_add(20)
        };
        *b = narrow(field(at, at.saturating_add(2))?)?;
    }
    let guid = windows_sys::core::GUID {
        data1: u32::try_from(field(0, 8)?).map_err(|e| Refusal::Broken(e.to_string()))?,
        data2: u16::try_from(field(9, 13)?).map_err(|e| Refusal::Broken(e.to_string()))?,
        data3: u16::try_from(field(14, 18)?).map_err(|e| Refusal::Broken(e.to_string()))?,
        data4,
    };
    sys::device_interfaces(&guid).map_err(|e| Refusal::os("listing device interfaces", e))
}

/// The scratch folder for a box made from an `OciSpec` under `root`.
pub fn scratch_folder(root: &Path, id: &BoxId) -> Result<String, Refusal> {
    root.join(id.as_str())
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| Refusal::os("naming a box's scratch folder", "not UTF-8"))
}
