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
use std::os::windows::io::OwnedHandle;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use hex_api::{Bounds, BoxId, BoxState, Exit, Proc, Refusal, Signal, Usage};
use hex_core::hcs::{self, Container, HcsError, Process, Scratch, Storage};

use super::proc::BoxProc;
use super::process::JobBox;
use super::sys::Handle;
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
    /// The box's record as the marker holds it, rewritten as an outliving box's start goes ([`OciBox::start`]).
    record: Option<hcs::BoxRecord>,
    /// Set once the box's container exists and outlives its node: dropped without a delete (the node going
    /// down), its scratch stays the container's, for the next node to take back. Until then a failed create
    /// undoes it as any box's, so no create leaves a scratch attached without a container.
    owner: ScratchOwner,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScratchOwner {
    Node,
    Container,
    Uncertain,
}

/// The scratch markers under `root`, one per box, `<id>.scratch.json`.
const MARKER_SUFFIX: &str = ".scratch.json";

/// A marker's next record while it is written, `<id>.scratch.json.next`, moved over the marker once whole.
const NEXT_SUFFIX: &str = ".next";

/// Where `marker`'s next record is written before it is moved over it.
fn next_of(marker: &Path) -> PathBuf {
    let mut next = marker.as_os_str().to_owned();
    next.push(NEXT_SUFFIX);
    PathBuf::from(next)
}

/// Writes `record` as `marker` whole: to its next file, then moved over it, so a node killed while writing
/// leaves the record before or the record after, never a torn one.
fn write_marker(marker: &Path, record: &hcs::BoxRecord) -> Result<(), Refusal> {
    let bytes = hcs::write_record(record)?;
    let next = next_of(marker);
    std::fs::write(&next, bytes).map_err(|e| Refusal::os("writing a box's record", e))?;
    std::fs::rename(&next, marker).map_err(|e| Refusal::os("writing a box's record", e))
}

/// Reads the box's record at `marker`, at most [`hcs::MAX_RECORD`] bytes: what a node taking the box back
/// reopens ([`OciBox::take_back`]).
pub fn read_marker(marker: &Path) -> Result<hcs::BoxRecord, Refusal> {
    let bound = hcs::record_bound()?;
    let mut bytes = Vec::new();
    let room = bound
        .checked_add(1)
        .ok_or_else(|| super::allocation(bound))?;
    bytes
        .try_reserve_exact(room)
        .map_err(|_| super::allocation(room))?;
    std::fs::File::open(marker)
        .and_then(|f| {
            f.take(
                u64::try_from(bound)
                    .map_err(std::io::Error::other)?
                    .checked_add(1)
                    .ok_or_else(|| std::io::Error::other("native marker read bound overflows"))?,
            )
            .read_to_end(&mut bytes)
        })
        .map_err(|e| Refusal::os("reading a scratch marker", e))?;
    hcs::read_record(&bytes).map_err(|r| match r {
        Refusal::Invalid { what, why } => Refusal::Invalid {
            what,
            why: format!("{}: {why}", marker.display()),
        },
        other => other,
    })
}

impl Mounted {
    /// Takes the filter off, detaches the volume and removes what Hex made; the first failure is returned,
    /// every step is still tried. `UnprepareLayer` answering `ERROR_FLT_DO_NOT_DETACH` is the container's
    /// end having taken the filter (note §3), matched alone; `DeactivateLayer` then detaches the volume.
    fn undo(&mut self) -> Result<(), Refusal> {
        if let Some(scratch) = self.activated.as_ref() {
            if self.prepared {
                match sys::unprepare_layer(scratch) {
                    Ok(()) => {}
                    Err(f) if hcs::hcs_error(f.code) == HcsError::FilterHeld => {}
                    Err(f) => {
                        return Err(failure(
                            "taking the storage filter off a container's scratch",
                            f,
                        ));
                    }
                }
                self.prepared = false;
            }
            sys::deactivate_layer(scratch)
                .map_err(|f| failure("detaching a container's scratch", f))?;
            self.activated = None;
        }
        for _ in 0..self.made.len() {
            let Some(path) = self.made.last() else {
                break;
            };
            let gone = if path.is_dir() {
                std::fs::remove_dir(path)
            } else {
                std::fs::remove_file(path)
            };
            match gone {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Refusal::os("removing a container's scratch", e)),
            }
            self.made.pop();
        }
        if let Some(marker) = self.marker.as_ref() {
            match std::fs::remove_file(marker) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Refusal::os("removing a container's scratch marker", e)),
            }
            self.marker = None;
        }
        Ok(())
    }
}

/// Reclaims the scratch a killed node left under `root` (each `<id>.scratch.json` marker's folder):
/// detaches its volume where it is still activated (UnprepareLayer, then DeactivateLayer, which detaches it
/// whether or not the unprepare took: jupiter, 2026-11-06) and removes what Hex made, then the marker. The
/// containers of boxes that end with their node ended with it (ShouldTerminateOnLastHandleClosed, the
/// job's kill-on-close), so the scratch is all a crash leaves; an outliving box whose container HCS still
/// runs is left for the node's recovery to take back or end (`hcs::at_start`). Bounded by `root`'s entries, read once; each failure is returned, typed,
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
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        // A next record whose marker is gone is a first write a killed node didn't finish, made before
        // anything else was; one beside its marker goes with the marker.
        if let Some(marker) = name.strip_suffix(NEXT_SUFFIX)
            && marker.ends_with(MARKER_SUFFIX)
            && !root.join(marker).exists()
            && let Err(e) = std::fs::remove_file(&path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            failures.push(Refusal::os(
                "removing a scratch marker's unfinished record",
                e,
            ));
        }
        if !name.ends_with(MARKER_SUFFIX) {
            continue;
        }
        if let Err(r) = reclaim_one(&path) {
            failures.push(r);
        }
    }
    failures
}

fn reclaim_one(marker: &Path) -> Result<(), Refusal> {
    let m = read_marker(marker)?;
    // An outliving box whose container HCS still runs is the next node's recovery's to take back or end.
    let system_runs = match &m.system {
        Some(id) => sys::open_system(id.as_str())
            .map_err(|f| failure("reopening a killed node's container", f))?
            .is_some(),
        None => false,
    };
    if hcs::at_start(&m, system_runs) == hcs::AtStart::Leave {
        return Ok(());
    }
    remove_scratch(&m, marker)
}

/// What a node finds of the box a record names (native plan step 8, T-T7.1.2): its compute system, reopened
/// by id where HCS still holds it, and the process now at its init's pid where one is there, with
/// [`hcs::Found`], what [`hcs::take_back`] decides on.
struct Reopened {
    system: Option<System>,
    init: Option<Handle>,
    found: hcs::Found,
}

/// Reopens what `record` names of its box, for [`OciBox::take_back`] and [`alive`]: its compute system by
/// id, and its init by pid with that process's creation time and whether it still runs.
fn reopen(record: &hcs::BoxRecord) -> Result<Reopened, Refusal> {
    let system = match &record.system {
        Some(s) => {
            sys::open_system(s.as_str()).map_err(|f| failure("reopening a box's container", f))?
        }
        None => None,
    };
    let init = match record.init {
        Some(r) => {
            super::sys::open_process(r.pid).map_err(|e| Refusal::os("reopening a box's init", e))?
        }
        None => None,
    };
    let seen = match &init {
        Some(h) => Some(hcs::SeenProcess {
            created: super::sys::created(h)
                .map_err(|e| Refusal::os("reading when a box's init was created", e))?,
            running: !super::sys::wait(h, 0)
                .map_err(|e| Refusal::os("reading whether a box's init runs", e))?,
        }),
        None => None,
    };
    Ok(Reopened {
        found: hcs::Found {
            system: system.is_some(),
            init: seen,
        },
        system,
        init,
    })
}

/// Whether the box whose record is at `marker` is one its next node takes back (native plan step 8,
/// T-T7.1.2; the node's recovery asks it before it takes a box back or removes it): what
/// [`OciBox::take_back`] finds of it, decided as [`hcs::take_back`] decides, so recovery takes back only a
/// box its take-back takes and removes the rest: one made to end with its node, one whose container HCS no
/// longer holds, and one whose init ended though its container runs on (an outliving container doesn't end
/// with its init). A record already gone names nothing to take back.
pub fn alive(marker: &Path) -> Result<bool, Refusal> {
    if !marker.exists() {
        return Ok(false);
    }
    let record = read_marker(marker)?;
    let found = reopen(&record)?.found;
    Ok(hcs::take_back(&record, found).is_ok())
}

/// Ends what is left of the box the record at `marker` names, one a node before this one made that the
/// node's recovery removes rather than takes back (native plan step 8, T-T7.1.2): its container where HCS
/// still holds it, then its scratch and its record, as [`reclaim`] removes a killed node's. A record
/// already gone has nothing left to end.
pub fn end(marker: &Path) -> Result<(), Refusal> {
    if !marker.exists() {
        return Ok(());
    }
    let m = read_marker(marker)?;
    if let Some(id) = &m.system
        && let Some(system) = sys::open_system(id.as_str())
            .map_err(|f| failure("reopening a box's container to end it", f))?
    {
        match system.terminate(launch::PATIENCE) {
            Ok(()) => {}
            Err(f)
                if matches!(
                    hcs::hcs_error(f.code),
                    HcsError::AlreadyStopped | HcsError::NotFound
                ) => {}
            Err(f) => return Err(failure("ending a box's container", f)),
        }
    }
    remove_scratch(&m, marker)
}

/// A missing marker is not absence. Only the existing HCS open result can establish that outcome.
pub(crate) fn require_absent(id: &BoxId) -> Result<(), Refusal> {
    match sys::open_system(id.as_str())
        .map_err(|f| failure("establishing an uncertain container's absence", f))?
    {
        None => Ok(()),
        Some(_) => Err(Refusal::Broken(
            "native create outcome has no marker but its system still exists; claims remain held"
                .to_owned(),
        )),
    }
}

fn copy_bindings(bindings: &[hcs::DeviceBinding]) -> Result<Vec<hcs::DeviceBinding>, Refusal> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(bindings.len())
        .map_err(|_| super::allocation(bindings.len()))?;
    for binding in bindings {
        copy.push(hcs::DeviceBinding {
            id: binding.id,
            location: super::copy_text(&binding.location)?,
        });
    }
    Ok(copy)
}

/// Removes the scratch record `m` names and then its marker: detaches the volume where it is still
/// attached (UnprepareLayer, then DeactivateLayer, which detaches it whether or not the unprepare took:
/// jupiter, 2026-11-06), removes what Hex made, the marker's unfinished next record, and the marker.
fn remove_scratch(m: &hcs::BoxRecord, marker: &Path) -> Result<(), Refusal> {
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
    for p in m.made.iter().rev().map(Path::new) {
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
    if let Err(e) = std::fs::remove_file(next_of(marker))
        && e.kind() != std::io::ErrorKind::NotFound
    {
        return Err(Refusal::os(
            "removing a scratch marker's unfinished record",
            e,
        ));
    }
    std::fs::remove_file(marker).map_err(|e| Refusal::os("removing a scratch marker", e))
}

impl Drop for Mounted {
    fn drop(&mut self) {
        // The backstop for a box dropped without its delete; delete reports these failures. An outliving
        // box's scratch is its container's while that runs on.
        if self.owner == ScratchOwner::Node {
            let _ = self.undo();
        }
    }
}

fn failure(op: &'static str, f: HcsFailure) -> Refusal {
    hcs::refusal(op, f.code, sys::host(), f.message.as_deref())
}

/// Why box `id` isn't taken back, by name, for the node's recovery to end what is left of it.
fn lost_refusal(id: &BoxId, lost: hcs::Lost) -> Refusal {
    let why = match lost {
        hcs::Lost::EndsWithNode => "it ended with the node that made it",
        hcs::Lost::NoSystem => "HCS holds no container of it",
        hcs::Lost::InitEnded => "its init has ended, or its start wasn't finished",
    };
    Refusal::Invalid {
        what: "adopt",
        why: format!("box {id}: {why}"),
    }
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
    /// Dropped before the scratch: closing its last handle ends the container where it ends with its node
    /// (it was created to), and leaves it running where it outlives it.
    system: Mutex<Option<System>>,
    mounted: Mutex<Option<Mounted>>,
    user: Option<String>,
    init: Process,
    life: Mutex<Life>,
    devices: Vec<hcs::DeviceBinding>,
}

/// Whether HCS is here; `Err` is the class refused by name, saying the installer turns it on.
pub fn available() -> Result<(), Refusal> {
    sys::available().map_err(|missing| hcs::containers_off(&missing))
}

/// A bundle's `config.json` at `dir`, read at most [`hex_core::bundle::MAX_CONFIG_BYTES`] bytes (the
/// Linux bundle reader's bound: one rule for both) and parsed.
pub fn bundle_config(dir: &Path) -> Result<serde_json::Value, Refusal> {
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
    serde_json::from_str(&text).map_err(|e| Refusal::Invalid {
        what: "oci spec",
        why: format!("config.json isn't JSON: {e}"),
    })
}

impl OciBox {
    /// Makes the box `job` is for, for `container`, Hex's launcher mapped in from the folder `launcher`:
    /// checks the image runs on this host, prepares the scratch, and creates the compute system, which
    /// waits for [`OciBox::start`]. The node makes the box's job (named and held by its keeper where the box
    /// outlives it), so the class never names one. Its record (`marker`, an [`hcs::BoxRecord`]) and its
    /// container outlive its node where `lifetime` says so.
    pub fn create(
        job: JobBox,
        container: &Container,
        launcher: &Path,
        marker: Option<PathBuf>,
        lifetime: hcs::Lifetime,
    ) -> Result<Self, Refusal> {
        Self::create_assigned(job, container, launcher, marker, lifetime, Vec::new())
    }

    pub(crate) fn create_assigned(
        job: JobBox,
        container: &Container,
        launcher: &Path,
        marker: Option<PathBuf>,
        lifetime: hcs::Lifetime,
        devices: Vec<hcs::DeviceBinding>,
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
        let id = job.id().clone();
        let mut mounted = scratch(container, base, marker, &id, lifetime, &devices)?;
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
        let document = hcs::document(container, &storage, launcher, lifetime).to_string();
        // The marker already owns the exact system ID and bindings. A returned failure or a closed
        // handle does not prove cancellation completed; retain the scratch for explicit cleanup.
        if !devices.is_empty() {
            mounted.owner = ScratchOwner::Uncertain;
        }
        let system = match sys::create_system(id.as_str(), &document, launch::PATIENCE) {
            Ok(s) => s,
            Err(f) => {
                let refused = if devices.is_empty() {
                    failure("creating a container", f)
                } else {
                    hcs::refusal("creating an assigned container", f.code, host, None)
                };
                if devices.is_empty() {
                    mounted.undo()?;
                }
                return Err(refused);
            }
        };
        // From here the scratch is the container's: where it outlives its node, a node going down leaves both.
        mounted.owner = if lifetime == hcs::Lifetime::Outlives {
            ScratchOwner::Container
        } else {
            ScratchOwner::Node
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
            devices,
        })
    }

    pub(crate) fn device_bindings(&self) -> &[hcs::DeviceBinding] {
        &self.devices
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

    /// Starts `process` in the box through Hex's launcher: its process handle and pid.
    fn launch_member(
        &self,
        process: &Process,
        stdio: [OwnedHandle; 3],
    ) -> Result<(Handle, u32), Refusal> {
        let system = self
            .system
            .lock()
            .map_err(|_| Refusal::Broken("an oci box's system lock is poisoned".to_owned()))?;
        let system = system
            .as_ref()
            .ok_or_else(|| Refusal::NotRunning(self.id().clone(), "exec"))?;
        launch::start(system, &self.job, self.user.as_deref(), process, stdio)
    }

    fn launch(&self, process: &Process, stdio: [OwnedHandle; 3]) -> Result<BoxProc, Refusal> {
        let (member, pid) = self.launch_member(process, stdio)?;
        Ok(BoxProc::new(member, pid, self.job.shared()))
    }

    /// Whether the box's container and scratch outlive its node (set once its container exists).
    fn outlives(&self) -> Result<bool, Refusal> {
        Ok(self
            .mounted
            .lock()
            .map_err(|_| Refusal::Broken("an oci box's scratch lock is poisoned".to_owned()))?
            .as_ref()
            .is_some_and(|m| m.owner == ScratchOwner::Container))
    }

    /// Rewrites an outliving box's record with `change`; a box that ends with its node keeps its first.
    fn record(&self, change: impl FnOnce(&mut hcs::BoxRecord)) -> Result<(), Refusal> {
        let mut mounted = self
            .mounted
            .lock()
            .map_err(|_| Refusal::Broken("an oci box's scratch lock is poisoned".to_owned()))?;
        if let Some(m) = mounted.as_mut()
            && m.owner == ScratchOwner::Container
            && let (Some(marker), Some(record)) = (m.marker.as_ref(), m.record.as_mut())
        {
            change(record);
            write_marker(marker, record)?;
        }
        Ok(())
    }

    /// The box's job handle value, for the node's keeper to hold a second handle to it (`super::keeper`).
    pub fn job_token(&self) -> usize {
        self.job.job_token()
    }

    /// Takes back the box a node before this one made and left running (native plan step 8, T-T7.1.2):
    /// `job` its job, taken back by its name, `container` what its spec asks, `record` what its marker at
    /// `marker` says. Reopens its compute system by id, its scratch (still attached: its container holds
    /// it), and its init where it started, told from a process given that pid since by its creation time
    /// ([`hcs::take_back`]). Refused by name where any of them is gone, for recovery to end what is left.
    pub fn take_back(
        job: JobBox,
        container: &Container,
        marker: PathBuf,
        record: hcs::BoxRecord,
    ) -> Result<Self, Refusal> {
        Self::take_back_assigned(job, container, marker, record, Vec::new())
    }

    pub(crate) fn take_back_assigned(
        job: JobBox,
        container: &Container,
        marker: PathBuf,
        record: hcs::BoxRecord,
        devices: Vec<hcs::DeviceBinding>,
    ) -> Result<Self, Refusal> {
        if record.devices != devices
            || (!devices.is_empty() && record.system.as_ref() != Some(job.id()))
        {
            return Err(Refusal::Invalid {
                what: "native device recovery",
                why: "recorded exact bindings or system identity disagree".to_owned(),
            });
        }
        let id = job.id().clone();
        let Reopened {
            system,
            init,
            found,
        } = reopen(&record)?;
        let (system, started, init) = match (hcs::take_back(&record, found), system, init) {
            (Ok(hcs::TakeBack::Created), Some(s), _) => (s, false, None),
            (Ok(hcs::TakeBack::Running(r)), Some(s), Some(h)) => {
                (s, true, Some(BoxProc::new(h, r.pid, job.shared())))
            }
            (Err(lost), _, _) => return Err(lost_refusal(&id, lost)),
            (Ok(_), _, _) => {
                return Err(Refusal::Broken(format!(
                    "box {id} was taken back without the container or init its record names"
                )));
            }
        };
        let volume = sys::layer_mount_path(&record.folder)
            .map_err(|f| failure("reading a taken-back box's scratch volume", f))?;
        let mounted = Mounted {
            volume,
            activated: Some(record.folder.clone()),
            prepared: true,
            made: record.made.iter().map(PathBuf::from).collect(),
            marker: Some(marker),
            record: Some(record),
            owner: ScratchOwner::Container,
        };
        Ok(Self {
            job,
            system: Mutex::new(Some(system)),
            mounted: Mutex::new(Some(mounted)),
            user: container.init.user.clone(),
            init: container.init.clone(),
            life: Mutex::new(Life {
                started,
                init,
                exit: None,
            }),
            devices,
        })
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
        // An outliving box's record says a start was asked before HCS is, and names its init once it runs,
        // so a node killed in between leaves a start no node takes back unfinished (hcs::take_back).
        let outlives = self.outlives()?;
        self.record(|r| r.started = true)?;
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
        let (member, pid) = self.launch_member(&self.init, super::null_stdio()?)?;
        let created = outlives.then(|| super::sys::created(&member));
        // The init runs from here, so the box is started whatever its record's write says: one that fails
        // leaves a record naming no init, which no node takes back.
        life.started = true;
        life.init = Some(BoxProc::new(member, pid, self.job.shared()));
        if let Some(created) = created {
            let created =
                created.map_err(|e| Refusal::os("reading when a box's init was created", e))?;
            self.record(|r| r.init = Some(hcs::InitRecord { pid, created }))?;
        }
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
        let mut system = self
            .system
            .lock()
            .map_err(|_| Refusal::Broken("an oci box's system lock is poisoned".to_owned()))?;
        if let Some(held) = system.as_ref() {
            match held.terminate(launch::PATIENCE) {
                Ok(()) => {}
                Err(f)
                    if matches!(
                        hcs::hcs_error(f.code),
                        HcsError::AlreadyStopped | HcsError::NotFound
                    ) => {}
                Err(f) => return Err(failure("ending a container", f)),
            }
            system.take();
        }
        let mut mounted = self
            .mounted
            .lock()
            .map_err(|_| Refusal::Broken("an oci box's scratch lock is poisoned".to_owned()))?;
        if let Some(held) = mounted.as_mut() {
            held.owner = ScratchOwner::Node;
            held.undo()?;
            mounted.take();
        }
        Ok(())
    }
}

/// Prepares `container`'s scratch over its layers, `base` the bottom-most: a scratch folder gets a copy of
/// the base layer's `blank.vhdx` (unless it holds a `sandbox.vhdx` already), which vmcompute activates as
/// a volume and puts the storage filter over, the layers its parents, as hcsshim and runhcs do; the
/// documented storage calls made a writable layer no container finished starting on (note §3, §9); a
/// volume the caller mounted is used as it is.
fn scratch(
    container: &Container,
    base: &str,
    marker: Option<PathBuf>,
    id: &BoxId,
    lifetime: hcs::Lifetime,
    devices: &[hcs::DeviceBinding],
) -> Result<Mounted, Refusal> {
    let outlives = lifetime == hcs::Lifetime::Outlives;
    let folder = match &container.scratch {
        Scratch::Volume(v) => {
            if !devices.is_empty() {
                return Err(Refusal::Platform {
                    what: "device attachment",
                    platform: "windows OCI",
                    why: "assigned devices require an owned scratch-folder marker".to_owned(),
                });
            }
            return Ok(Mounted {
                volume: v.clone(),
                activated: None,
                prepared: false,
                made: Vec::new(),
                marker: None,
                record: None,
                owner: ScratchOwner::Node,
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
        record: None,
        owner: ScratchOwner::Node,
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
        let path = |p: &Path| {
            p.to_str()
                .map(str::to_owned)
                .ok_or_else(|| Refusal::os("naming a container's scratch", "not UTF-8"))
        };
        let record = hcs::BoxRecord {
            folder: path(&folder)?,
            made: made
                .iter()
                .map(|p| path(p.as_path()))
                .collect::<Result<_, _>>()?,
            outlives,
            system: if outlives || !devices.is_empty() {
                Some(id.try_clone()?)
            } else {
                None
            },
            started: false,
            init: None,
            devices: copy_bindings(devices)?,
        };
        if let Some(dir) = m.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Refusal::os("writing a scratch marker", e))?;
        }
        write_marker(&m, &record)?;
        mounted.marker = Some(m);
        mounted.record = Some(record);
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
        membership: None,
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
