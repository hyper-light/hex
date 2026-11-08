//! The Windows runtime: [`hex_api::Runtime`] over the `process` and `oci` classes
//! (docs/design/runtime.md). A `process` box is a job object; its bounds are the kernel's and hard
//! (`process`, `sys`, [`hex_core::job`]), and a member is started suspended inside the job and resumed, so
//! nothing runs outside it. An `oci` box is a Windows image run process-isolated through the Host Compute
//! Service with such a job nested in its silo (`oci`, [`hex_core::hcs`]). Where a Windows feature Hex's
//! installer turns on is off ([`features`]), the class is refused by name, naming it. This is the node's
//! runtime seam on Windows; the records, recovery and socket that wrap it on Linux (`hex-node`) are not
//! yet cross-OS, so this holds its boxes in memory and is driven in process.

pub mod features;
pub mod keeper;
pub mod oci;
pub mod proc;
pub mod process;
pub mod sys;

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;
use std::os::windows::io::OwnedHandle;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use hex_api::{
    BoxId, BoxInfo, BoxSpec, BoxState, Class, ClassSpec, Proc, Program, Refusal, Rootfs, Runtime,
    Signal, Stdio, Usage,
};

use crate::guest::Side;
use hex_core::setup::{self, WindowsFeature};
use oci::OciBox;
use process::JobBox;
use sys::{Spawn, Started};

/// A box of either class.
#[derive(Clone)]
enum Held {
    Process(Arc<JobBox>),
    Oci(Arc<OciBox>),
}

impl Held {
    /// The box's job handle value, for the node's keeper to hold a second handle to it.
    fn job_token(&self) -> usize {
        match self {
            Held::Process(b) => b.job_token(),
            Held::Oci(b) => b.job_token(),
        }
    }
}

/// A node's keeper and the state folder its boxes' jobs are recorded in. The keeper is started by
/// [`Keeping::start`] (or an existing one adopted from the state folder) and holds a second handle to each
/// box's job, so a box outlives the node (`keeper`).
struct Keeping {
    state: PathBuf,
    command: Vec<String>,
    keeper: Mutex<keeper::NodeKeeper>,
}

impl Keeping {
    /// The keeper for a node on `state`: the one the state folder records if it is still alive, else a new
    /// one started from `command` (`hex keep ...`), recorded for the next node.
    fn start(state: &Path, command: &[String]) -> Result<Self, Refusal> {
        let keeping = Self {
            state: state.to_path_buf(),
            command: command.to_vec(),
            keeper: Mutex::new(Self::adopt_or_spawn(state, command)?),
        };
        Ok(keeping)
    }

    fn adopt_or_spawn(state: &Path, command: &[String]) -> Result<keeper::NodeKeeper, Refusal> {
        if let Some((pid, created)) = read_keeper(state)
            && let Ok(k) = keeper::NodeKeeper::open(pid, created, state)
            && k.alive()
        {
            return Ok(k);
        }
        Self::spawn_at(state, command)
    }

    /// Starts a fresh keeper and records it, for [`WinRuntime::ensure_keeper`] when the old one is gone.
    fn spawn(&self) -> Result<keeper::NodeKeeper, Refusal> {
        Self::spawn_at(&self.state, &self.command)
    }

    fn spawn_at(state: &Path, command: &[String]) -> Result<keeper::NodeKeeper, Refusal> {
        let (program, args) = command.split_first().ok_or_else(|| Refusal::Invalid {
            what: "keeper",
            why: "an empty keeper command".to_owned(),
        })?;
        let ready = state.join(format!("keeper-ready-{}", std::process::id()));
        let _ = std::fs::remove_file(&ready);
        // The keeper takes its state folder and ready path from the environment, so the command names only
        // how to launch it (`hex keep`, or the test's own keeper body); `keeper::Args` reads flags or these.
        let mut child = std::process::Command::new(program)
            .args(args)
            .env("HEX_KEEP_STATE", state)
            .env("HEX_KEEP_READY", &ready)
            .spawn()
            .map_err(|e| Refusal::os("starting the keeper", e))?;
        // Wait for the keeper to write its identity, waiting on the child itself between polls (no blocking
        // sleep): a bound of the node's EMPTY_PATIENCE, 60 s, at ticks of 10 ms.
        let watch = sys::open_process(child.id()).ok().flatten();
        let mut ticks: u32 = 6_000;
        let (pid, created) = loop {
            if let Some((pid, created)) = read_ready(&ready) {
                break (pid, created);
            }
            if let Ok(Some(status)) = child.try_wait() {
                return Err(Refusal::Broken(format!(
                    "the keeper exited before it was ready: {status}"
                )));
            }
            ticks = match ticks.checked_sub(1) {
                Some(n) => n,
                None => {
                    let _ = child.kill();
                    return Err(Refusal::Os {
                        op: "starting the keeper",
                        why: "it didn't say it was ready in 60 s".to_owned(),
                    });
                }
            };
            // Wait up to 10 ms on the child; its ending wakes us, else the next poll reads the ready file.
            if let Some(w) = watch.as_ref() {
                let _ = sys::wait(w, 10);
            }
        };
        let k = keeper::NodeKeeper::open(pid, created, state)?;
        write_keeper(state, pid, created).map_err(|e| Refusal::os("recording the keeper", e))?;
        Ok(k)
    }
}

fn read_ready(path: &Path) -> Option<(u32, u64)> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut p = text.split_whitespace();
    Some((p.next()?.parse().ok()?, p.next()?.parse().ok()?))
}

fn read_keeper(state: &Path) -> Option<(u32, u64)> {
    read_ready(&keeper::keeper_file(state))
}

fn write_keeper(state: &Path, pid: u32, created: u64) -> std::io::Result<()> {
    std::fs::write(keeper::keeper_file(state), format!("{pid} {created}"))
}

/// Why this runtime alone refuses a Linux image once the Windows Hypervisor Platform is on: a Linux image
/// runs in the node's Shards guest, where a node built with [`crate::guest::WithGuest::routing`] by
/// [`WinRuntime::oci_side`] sends it (native plan step 5; docs/design/shards-guest.md).
const LINUX_IN_GUEST: &str = "a Linux image runs in the node's Shards guest through the Windows Hypervisor Platform, which this runtime alone doesn't boot (hex_runtime::guest::WithGuest routes it there)";

/// The Windows node's runtime, its boxes held in memory, keyed by id.
pub struct WinRuntime {
    /// The machine's logical processors, the CPU hard cap's denominator (read once at node start).
    processors: NonZeroU32,
    /// Where an `oci` box made from an `OciSpec` keeps its scratch, one folder per box.
    scratch_root: PathBuf,
    /// The folder holding the `hex` binary every `oci` box maps read-only as its launcher: the node's
    /// own folder unless told otherwise.
    launcher: Option<PathBuf>,
    boxes: Mutex<BTreeMap<BoxId, Held>>,
    /// The node's name for its boxes' jobs, where its `process` boxes outlive it ([`WinRuntime::outliving`]):
    /// a box is the job `Local\hex-<node>-<box>`, which a node on the same name takes back. `None`: a
    /// runtime driven in process, whose boxes end with it (kill-on-close).
    node: Option<String>,
    /// Where its boxes outlive it: the keeper holding a second handle to each box's job, and the state
    /// folder their handle values are kept in. `None` for a runtime driven in process.
    keeper: Option<Keeping>,
    /// The installer's features that are off, read when the runtime is made (node start), so no box's
    /// create waits on the query: a feature turns on or off only across a reboot, which starts the
    /// node again.
    features_off: OnceLock<BTreeSet<WindowsFeature>>,
    /// How many times [`features::off`] ran, for the test that no create runs it.
    #[cfg(test)]
    feature_reads: std::sync::atomic::AtomicU32,
    /// What reclaiming a killed node's scratch at start failed to do, each typed (its markers kept for
    /// the next start).
    reclaim_failures: Vec<Refusal>,
}

impl WinRuntime {
    /// A runtime for this machine, its processor count read from Windows, its `oci` scratch under
    /// `%ProgramData%\hex\boxes`.
    pub fn new() -> Self {
        Self::with(sys::active_processors(), default_scratch_root())
    }

    /// A runtime told the machine has `processors` logical processors (at least one), keeping `oci`
    /// scratch under `scratch_root`, first reclaiming what a killed node left there ([`oci::reclaim`]).
    /// The installer's features are read here, before any box, so the first `oci` create doesn't wait
    /// on PowerShell's query (p50 1,183 ms on jupiter, bench/install/records/windows-jupiter-20261105.txt
    /// entry 19). A read that fails here isn't lost: the first box that needs the features reads them
    /// again, and is refused by name if that fails too.
    pub fn with(processors: u32, scratch_root: PathBuf) -> Self {
        let reclaim_failures = oci::reclaim(&scratch_root);
        let runtime = Self {
            processors: NonZeroU32::new(processors).unwrap_or(NonZeroU32::MIN),
            scratch_root,
            launcher: None,
            boxes: Mutex::new(BTreeMap::new()),
            node: None,
            keeper: None,
            features_off: OnceLock::new(),
            #[cfg(test)]
            feature_reads: std::sync::atomic::AtomicU32::new(0),
            reclaim_failures,
        };
        // Read now so no create waits on it; a failure is read again, and refused, at the first ask.
        let _ = runtime.features_off();
        runtime
    }

    /// A node's runtime on this machine whose boxes outlive it: each box's job is the one named for `node`
    /// and the box, kill-on-close, held by the node and by a keeper (`keeper`, a command line started apart
    /// from the node, its state in `dir`), and an `oci` box's container outlives the node with it (HCS
    /// doesn't end it with its last handle), so a node killed with boxes running leaves them to the keeper,
    /// and the next node on the same name takes them back ([`WinRuntime::adopt`]) or ends them
    /// ([`WinRuntime::end`]). With node and keeper both gone, kill-on-close ends them. `node` names one
    /// node's boxes apart from another's in the session. Its `oci` boxes' records and spec-made scratch are
    /// its own, under `dir\boxes`, so its start-time reclaim sweeps no other runtime's boxes (the machine's
    /// shared `%ProgramData%\hex\boxes` holds those of runtimes driven in process, a bench's among them).
    pub fn outliving(node: &str, dir: &Path, keeper: &[String]) -> Result<Self, Refusal> {
        std::fs::create_dir_all(dir)
            .map_err(|e| Refusal::os("making the node's state folder", e))?;
        let keeping = Keeping::start(dir, keeper)?;
        let mut runtime = Self::with(sys::active_processors(), dir.join("boxes"));
        runtime.node = Some(node.to_owned());
        runtime.keeper = Some(keeping);
        Ok(runtime)
    }

    /// The pid of the keeper holding this node's boxes' jobs, where its boxes outlive it; a keeper found
    /// dead is replaced first (the node's own watch does this on its port; here, for a runtime driven in
    /// process, it is done when asked).
    pub fn keeper_pid(&self) -> Option<u32> {
        let keeping = self.keeper.as_ref()?;
        self.ensure_keeper(keeping).ok()?;
        Some(keeping.keeper.lock().ok()?.pid())
    }

    /// Ensures the keeper is alive: one found dead is started again and every live box's job duplicated
    /// into it, its handle value rewritten. The node does this when it sees the keeper's end.
    fn ensure_keeper(&self, keeping: &Keeping) -> Result<(), Refusal> {
        let mut held = keeping
            .keeper
            .lock()
            .map_err(|_| Refusal::Broken("the node's keeper lock is poisoned".to_owned()))?;
        if held.alive() {
            return Ok(());
        }
        // The old keeper is gone (that is why it is replaced); its stop event is shared by the state folder,
        // so signalling it would stop the new keeper too. Just replace it.
        *held = keeping.spawn()?;
        // Re-duplicate every live box's job into the new keeper.
        for (id, h) in self.boxes()?.iter() {
            let value = held.hold(h.job_token())?;
            keeper::write_held(&keeping.state, id.as_str(), value)
                .map_err(|e| Refusal::os("recording a box's keeper handle", e))?;
        }
        Ok(())
    }

    /// Gives the keeper's handle after making sure it is alive.
    fn keeping(&self) -> Result<Option<&Keeping>, Refusal> {
        let Some(keeping) = self.keeper.as_ref() else {
            return Ok(None);
        };
        self.ensure_keeper(keeping)?;
        Ok(Some(keeping))
    }

    /// The job name a node's box `id` has (`Local\hex-<node>-<box>`), where its boxes outlive it.
    fn job_name(&self, id: &BoxId) -> Option<String> {
        self.node
            .as_ref()
            .map(|node| format!("Local\\hex-{node}-{id}"))
    }

    /// Takes back box `spec` a node before this one made: a `process` box with whatever runs in it (made
    /// again, empty, where nothing does); an `oci` box with its container, scratch and init, refused by name
    /// where any of them is gone ([`OciBox::take_back`]), for recovery to end it. Refused for a runtime whose
    /// boxes don't outlive it.
    pub fn adopt(&self, spec: &BoxSpec) -> Result<(), Refusal> {
        let Some(name) = self.job_name(&spec.id) else {
            return Err(Refusal::Invalid {
                what: "adopt",
                why: format!(
                    "box {}: this runtime's boxes end with it, so none outlives a node to take back",
                    spec.id
                ),
            });
        };
        spec.bounds.check()?;
        let b = match &spec.class {
            ClassSpec::Process => Held::Process(Arc::new(JobBox::take_back(
                spec.id.clone(),
                &spec.bounds,
                self.processors,
                &name,
            )?)),
            ClassSpec::Oci(_) | ClassSpec::Bundle(_) => {
                Held::Oci(Arc::new(self.take_back_oci(spec, &name)?))
            }
        };
        // The keeper already holds this box's job from the node that made it (the handle value is recorded
        // under the state folder), so taking it back keeps that handle; where none is recorded (a box this
        // runtime never kept), the current keeper holds it so a delete can close it.
        if let Some(keeping) = self.keeping()?
            && keeper::read_held(&keeping.state, spec.id.as_str()).is_none()
        {
            let value = keeping
                .keeper
                .lock()
                .map_err(|_| Refusal::Broken("the node's keeper lock is poisoned".to_owned()))?
                .hold(b.job_token())?;
            keeper::write_held(&keeping.state, spec.id.as_str(), value)
                .map_err(|e| Refusal::os("recording a box's keeper handle", e))?;
        }
        let mut boxes = self.boxes()?;
        if boxes.contains_key(&spec.id) {
            return Err(Refusal::Exists(spec.id.clone()));
        }
        boxes.insert(spec.id.clone(), b);
        Ok(())
    }

    /// Closes box `id`'s job in the keeper and forgets its value (its delete or its removal by recovery):
    /// after this the box's last handle is the node's, so kill-on-close ends it with the node.
    fn drop_from_keeper(&self, id: &BoxId) -> Result<(), Refusal> {
        if let Some(keeping) = self.keeping()?
            && let Some(value) = keeper::read_held(&keeping.state, id.as_str())
        {
            keeping
                .keeper
                .lock()
                .map_err(|_| Refusal::Broken("the node's keeper lock is poisoned".to_owned()))?
                .drop_held(value)?;
            keeper::forget_held(&keeping.state, id.as_str());
        }
        Ok(())
    }

    /// Ends what still runs of box `id` a node before this one made, for a box recovery removes: its job's
    /// members, and an `oci` box's container, scratch and record too ([`oci::end`]); one that left nothing
    /// has nothing to end. A runtime whose boxes end with it has none to end.
    pub fn end(&self, id: &BoxId) -> Result<(), Refusal> {
        match self.job_name(id) {
            Some(name) => {
                JobBox::end_named(&name)?;
                oci::end(&self.marker(id))?;
                // Close the keeper's handle too, so nothing holds the ended job; idempotent (a second end
                // finds no recorded value).
                self.drop_from_keeper(id)
            }
            None => Ok(()),
        }
    }

    /// The installer's features that are off, read once ([`features::off`]), at the runtime's making; a
    /// failed read is refused and tried again at the next ask.
    fn features_off(&self) -> Result<&BTreeSet<WindowsFeature>, Refusal> {
        if let Some(off) = self.features_off.get() {
            return Ok(off);
        }
        #[cfg(test)]
        self.feature_reads
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let off = features::off(self.processors)?;
        Ok(self.features_off.get_or_init(|| off))
    }

    /// The `oci` class refused for an image that needs `feature`: naming it while it is off, else saying
    /// what the image still waits on (`waits`).
    fn refused(&self, feature: WindowsFeature, waits: &str) -> Result<Refusal, Refusal> {
        let off: BTreeSet<WindowsFeature> = self
            .features_off()?
            .iter()
            .copied()
            .filter(|f| *f == feature)
            .collect();
        Ok(setup::windows_oci(&off, waits))
    }

    /// Where a node whose Linux images are its Shards guest's runs a box ([`crate::guest::WithGuest::routing`]):
    /// a `process` box and a Windows image here, a Linux image in the guest once the Windows Hypervisor
    /// Platform it needs is on, refused naming it before then. A bundle is a Windows image when its
    /// config has a `windows` section (config.md:530 makes it a Windows container's MUST); a spec, when its
    /// root is layers, as step 4's class reads one (a plain folder is a Linux root).
    pub fn oci_side(&self, class: &ClassSpec) -> Result<Side, Refusal> {
        let windows = match class {
            ClassSpec::Process => true,
            ClassSpec::Bundle(dir) => oci::bundle_config(dir)?.get("windows").is_some(),
            ClassSpec::Oci(o) => matches!(o.rootfs, Rootfs::Layers(_)),
        };
        if windows {
            return Ok(Side::Host);
        }
        self.refused_without(WindowsFeature::HypervisorPlatform)?;
        Ok(Side::Guest)
    }

    /// `Ok` while `feature` is on, else the class refused naming it.
    fn refused_without(&self, feature: WindowsFeature) -> Result<(), Refusal> {
        if self.features_off()?.contains(&feature) {
            return Err(self.refused(feature, "")?);
        }
        Ok(())
    }

    /// What this runtime's start-time reclaim of a killed node's scratch couldn't do.
    pub fn reclaim_failures(&self) -> &[Refusal] {
        &self.reclaim_failures
    }

    /// This runtime with `oci` boxes' launcher taken from `dir` (which holds `hex.exe`), not the node's
    /// own folder: for a node run from another binary, as a test is.
    pub fn with_launcher(self, dir: PathBuf) -> Self {
        Self {
            launcher: Some(dir),
            ..self
        }
    }

    /// A runtime told the machine has `processors` logical processors (at least one).
    pub fn with_processors(processors: u32) -> Self {
        Self::with(processors, default_scratch_root())
    }

    fn boxes(&self) -> Result<std::sync::MutexGuard<'_, BTreeMap<BoxId, Held>>, Refusal> {
        self.boxes
            .lock()
            .map_err(|_| Refusal::Broken("the node's box map lock is poisoned".to_owned()))
    }

    fn get(&self, id: &BoxId) -> Result<Held, Refusal> {
        self.boxes()?
            .get(id)
            .cloned()
            .ok_or_else(|| Refusal::NoSuchBox(id.clone()))
    }

    fn info(&self, b: &Held) -> Result<BoxInfo, Refusal> {
        match b {
            Held::Process(b) => Ok(BoxInfo {
                id: b.id().clone(),
                class: Class::Process,
                state: if b.running()? {
                    BoxState::Running
                } else {
                    BoxState::Empty
                },
                bounds: b.bounds(),
                adopted: false,
                membership: None,
                // A process box has no init, so no container exit to report (HEX-RT-11).
                exit: None,
                abandoned: 0,
            }),
            Held::Oci(b) => oci::info(b),
        }
    }

    /// The container `oci` box `spec` asks for, refused by name where a feature it needs is off.
    fn container(&self, spec: &BoxSpec) -> Result<hex_core::hcs::Container, Refusal> {
        Ok(match &spec.class {
            ClassSpec::Oci(o) => {
                self.refused_without(WindowsFeature::Containers)?;
                hex_core::hcs::from_spec(o, &oci::scratch_folder(&self.scratch_root, &spec.id)?)?
            }
            ClassSpec::Bundle(dir) => {
                let config = oci::bundle_config(dir)?;
                if config.get("windows").is_none() {
                    return Err(self.refused(WindowsFeature::HypervisorPlatform, LINUX_IN_GUEST)?);
                }
                self.refused_without(WindowsFeature::Containers)?;
                hex_core::hcs::from_bundle(&config)?
            }
            ClassSpec::Process => {
                return Err(Refusal::Broken(
                    "a process spec reached the oci class".to_owned(),
                ));
            }
        })
    }

    /// The folder holding the `hex` every `oci` box maps read-only as its launcher.
    fn launcher(&self) -> Result<PathBuf, Refusal> {
        match &self.launcher {
            Some(d) => Ok(d.clone()),
            None => std::env::current_exe()
                .ok()
                .and_then(|e| e.parent().map(std::path::Path::to_path_buf))
                .ok_or_else(|| Refusal::Broken("the node can't name its own folder".to_owned())),
        }
    }

    /// Where `oci` box `id`'s record is kept: its scratch marker under the runtime's scratch root.
    fn marker(&self, id: &BoxId) -> PathBuf {
        self.scratch_root.join(format!("{id}.scratch.json"))
    }

    /// The `oci` box `spec` asks for, made. Where this node's boxes outlive it, its job is the one named for
    /// the node and the box (the keeper holds it too: [`Runtime::create`]) and its container outlives the
    /// node with it; otherwise both end with the node.
    fn create_oci(&self, spec: &BoxSpec) -> Result<OciBox, Refusal> {
        let container = self.container(spec)?;
        let launcher = self.launcher()?;
        let (job, lifetime) = match self.job_name(&spec.id) {
            Some(name) => (
                JobBox::create_named(spec.id.clone(), &spec.bounds, self.processors, &name)?,
                hex_core::hcs::Lifetime::Outlives,
            ),
            None => (
                JobBox::create(spec.id.clone(), &spec.bounds, self.processors)?,
                hex_core::hcs::Lifetime::EndsWithNode,
            ),
        };
        OciBox::create(
            job,
            &container,
            &launcher,
            Some(self.marker(&spec.id)),
            lifetime,
        )
    }

    /// Takes back `oci` box `spec` a node before this one made on the same name: its record from its
    /// marker, its job by name, then its container, scratch and init ([`OciBox::take_back`]).
    fn take_back_oci(&self, spec: &BoxSpec, name: &str) -> Result<OciBox, Refusal> {
        let container = self.container(spec)?;
        let marker = self.marker(&spec.id);
        let record = oci::read_marker(&marker)?;
        let job = JobBox::take_back(spec.id.clone(), &spec.bounds, self.processors, name)?;
        OciBox::take_back(job, &container, marker, record)
    }
}

impl Default for WinRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl Runtime for WinRuntime {
    fn classes(&self) -> Vec<Class> {
        if oci::available().is_ok() {
            vec![Class::Process, Class::Oci]
        } else {
            vec![Class::Process]
        }
    }

    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        spec.bounds.check()?;
        if self.boxes()?.contains_key(&spec.id) {
            return Err(Refusal::Exists(spec.id));
        }
        let held = match spec.class {
            ClassSpec::Process => Held::Process(Arc::new(match self.job_name(&spec.id) {
                Some(name) => {
                    JobBox::create_named(spec.id.clone(), &spec.bounds, self.processors, &name)?
                }
                None => JobBox::create(spec.id.clone(), &spec.bounds, self.processors)?,
            })),
            ClassSpec::Oci(_) | ClassSpec::Bundle(_) => {
                Held::Oci(Arc::new(self.create_oci(&spec)?))
            }
        };
        let info = self.info(&held)?;
        // Hand the box's job to the keeper before the box is visible, holding no box-map lock across it: a
        // keeper found dead is replaced, which reads the box map ([`WinRuntime::ensure_keeper`]), so doing
        // this under the box lock would deadlock the runtime against itself (std's `Mutex` isn't reentrant).
        if let Some(keeping) = self.keeping()? {
            let value = keeping
                .keeper
                .lock()
                .map_err(|_| Refusal::Broken("the node's keeper lock is poisoned".to_owned()))?
                .hold(held.job_token())?;
            keeper::write_held(&keeping.state, spec.id.as_str(), value)
                .map_err(|e| Refusal::os("recording a box's keeper handle", e))?;
        }
        // Insert under the box lock alone. A create that lost a race to the same id undoes its keeper hold
        // and its box once the lock is released, so neither is done under it.
        let dup = {
            let mut boxes = self.boxes()?;
            match boxes.entry(spec.id.clone()) {
                std::collections::btree_map::Entry::Vacant(slot) => {
                    slot.insert(held.clone());
                    false
                }
                std::collections::btree_map::Entry::Occupied(_) => true,
            }
        };
        if dup {
            // Made beside another create of the same id: this one is ended, the first kept.
            self.drop_from_keeper(&spec.id)?;
            if let Held::Oci(b) = &held {
                let _ = b.delete();
            }
            return Err(Refusal::Exists(spec.id));
        }
        Ok(info)
    }

    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        match self.get(id)? {
            // A process box has no init to start: exec runs its programs.
            Held::Process(_) => Err(Refusal::Invalid {
                what: "start",
                why: format!("box {id} is a process box, which has no init to start"),
            }),
            Held::Oci(b) => b.start(),
        }
    }

    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        stdio.check()?;
        if stdio.ask != hex_api::Ask::default() {
            return Err(Refusal::Platform {
                what: "stdio streams made by the runtime",
                platform: "windows",
                why: "the Windows classes take the caller's own handles; the stream form is the Windows \
                      agent's to add, by its stdio record's winner"
                    .to_owned(),
            });
        }
        match self.get(id)? {
            Held::Process(b) => {
                let spawn = build_spawn(&program, stdio)?;
                let Started {
                    process,
                    thread,
                    pid,
                } = b.exec(spawn)?;
                // The initial thread is resumed; the node holds only the process handle to wait on.
                drop(thread);
                Ok(Box::new(proc::BoxProc::new(process, pid, b.shared())))
            }
            Held::Oci(b) => {
                let process = hex_core::hcs::process("program", &program)?;
                b.exec(&process, std_handles(stdio)?)
            }
        }
    }

    fn signal(&self, id: &BoxId, signal: Signal) -> Result<(), Refusal> {
        match self.get(id)? {
            Held::Process(b) => b.signal(signal),
            Held::Oci(b) => b.signal(signal),
        }
    }

    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
        let b = self.get(id)?;
        self.info(&b)
    }

    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        match self.get(id)? {
            Held::Process(b) => b.usage(),
            Held::Oci(b) => b.usage(),
        }
    }

    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        let boxes: Vec<Held> = self.boxes()?.values().cloned().collect();
        boxes.iter().map(|b| self.info(b)).collect()
    }

    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        let b = self
            .boxes()?
            .remove(id)
            .ok_or_else(|| Refusal::NoSuchBox(id.clone()))?;
        // Close the keeper's handle to this box's job first, so the node's TerminateJobObject and the
        // dropped Arc leave nothing holding it.
        self.drop_from_keeper(id)?;
        // Ends the box; dropping the last Arc closes its job (kill-on-close) and, for an oci box, its
        // container's last handle (terminate-on-close) as a backstop, so deleting a box a caller still
        // holds through `list` is safe.
        match b {
            Held::Process(b) => b.delete(),
            Held::Oci(b) => b.delete(),
        }
    }
}

/// `%ProgramData%\hex\boxes`, where `oci` boxes made from an `OciSpec` keep their scratch.
fn default_scratch_root() -> PathBuf {
    std::env::var_os("ProgramData")
        .map_or_else(|| PathBuf::from(r"C:\ProgramData"), PathBuf::from)
        .join("hex")
        .join("boxes")
}

/// Every standard stream the NUL device: an `oci` box's init has no caller to read it.
fn null_stdio() -> Result<[OwnedHandle; 3], Refusal> {
    Ok([handle(None)?, handle(None)?, handle(None)?])
}

/// The three standard handles for `stdio`, each the NUL device where the caller gave none.
fn std_handles(stdio: Stdio) -> Result<[OwnedHandle; 3], Refusal> {
    Ok([
        handle(stdio.stdin.map(OwnedHandle::from))?,
        handle(stdio.stdout.map(OwnedHandle::from))?,
        handle(stdio.stderr.map(OwnedHandle::from))?,
    ])
}

/// Builds the [`Spawn`] for `program` with `stdio`: its command line (argv quoted as the C runtime
/// parses it), its whole environment as a sorted block (nothing inherited), its working folder, and its
/// three standard handles (each the NUL device where the caller gave none).
fn build_spawn(program: &Program, stdio: Stdio) -> Result<Spawn, Refusal> {
    program.check()?;
    // One Windows exec bound for both classes, so a program moves between them unchanged (HEX-RT-14).
    program.fits(&hex_core::hcs::exec_limit())?;
    if program.user.is_some() {
        return Err(Refusal::Invalid {
            what: "program",
            why: "a process box runs its programs as the node agent's user; a user is the oci class's"
                .to_owned(),
        });
    }
    let cwd = match &program.cwd {
        Some(c) if c.contains('\0') => {
            return Err(Refusal::Invalid {
                what: "program",
                why: format!("working directory {c:?} holds a NUL"),
            });
        }
        Some(c) => Some(c.encode_utf16().chain([0]).collect()),
        None => None,
    };
    Ok(Spawn {
        command_line: hex_core::job::command_line(&program.args)
            .encode_utf16()
            .chain([0])
            .collect(),
        environment: hex_core::job::environment(&program.env),
        cwd,
        stdio: std_handles(stdio)?,
    })
}

/// The handle for a standard stream: the caller's where it gave one, else the NUL device (read and
/// write, so it serves stdin or stdout).
fn handle(given: Option<OwnedHandle>) -> Result<OwnedHandle, Refusal> {
    match given {
        Some(h) => Ok(h),
        None => {
            let nul = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("NUL")
                .map_err(|e| Refusal::os("opening NUL", e))?;
            Ok(OwnedHandle::from(nul))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    /// HEX-INST-9 (native plan step 6): the installer's features are read when the runtime is made, so
    /// a box's create doesn't wait on PowerShell's query. Do: make a runtime, then route a Linux image's
    /// bundle, the first ask that needs the features. Expect: one query, made with the runtime, and
    /// none by the routing.
    #[test]
    fn the_features_are_read_at_the_runtimes_making_not_at_a_create() {
        let scratch = tempfile::tempdir().unwrap();
        let rt = WinRuntime::with(1, scratch.path().to_path_buf());
        assert_eq!(rt.feature_reads.load(Ordering::SeqCst), 1);
        assert!(
            rt.features_off.get().is_some(),
            "the query at the runtime's making failed"
        );
        let bundle = tempfile::tempdir().unwrap();
        std::fs::write(
            bundle.path().join("config.json"),
            r#"{"ociVersion": "1.2.0", "process": {"args": ["/bin/true"], "cwd": "/"}, "root": {"path": "rootfs"}, "linux": {}}"#,
        )
        .unwrap();
        let _ = rt.oci_side(&ClassSpec::Bundle(bundle.path().to_path_buf()));
        assert_eq!(rt.feature_reads.load(Ordering::SeqCst), 1);
    }

    /// A program that runs until it is ended (PING's 600 one-second echoes to this machine).
    fn waiting() -> Program {
        let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
        let ping = format!("{system}\\System32\\PING.EXE");
        Program::of(&[ping.as_str(), "-n", "600", "127.0.0.1"])
    }

    /// Whether process `pid` runs, by `tasklist` (its output names the pid only while it runs).
    fn alive(pid: u32) -> bool {
        let out = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
    }

    fn wait_for(what: &str, mut fact: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !fact() {
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for {what}"
            );
            std::thread::park_timeout(std::time::Duration::from_millis(20));
        }
    }

    fn process_box(id: &str) -> BoxSpec {
        BoxSpec {
            id: BoxId::new(id).unwrap(),
            bounds: hex_api::Bounds::default(),
            class: ClassSpec::Process,
        }
    }

    /// A node name no other test run holds in this session.
    fn node(test: &str) -> String {
        format!("t{}-{test}", std::process::id())
    }

    /// The keeper for the tests: this test binary re-invoked to run only [`keeper_body`], the real keeper
    /// (`keeper::run`). The runtime hands it the state folder and ready path through the environment, as it
    /// hands `hex keep` them, so the command is just how to launch it.
    fn keeper(_test: &str) -> Vec<String> {
        vec![
            std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            "--exact".to_owned(),
            "windows::tests::keeper_body".to_owned(),
            "--ignored".to_owned(),
            "--nocapture".to_owned(),
            "--test-threads".to_owned(),
            "1".to_owned(),
        ]
    }

    /// The keeper, run as a process of its own by [`keeper`]: it reads its state folder and ready path from
    /// the environment and runs [`keeper::run`]. `#[ignore]`d so the harness runs it only when named.
    #[test]
    #[ignore]
    fn keeper_body() {
        let args = keeper::Args::parse(&[]).expect("keeper args from the environment");
        keeper::run(&args).expect("the keeper runs");
    }

    /// Ends process `pid` as a crash would (taskkill /F, TerminateProcess).
    fn crash(pid: u32) {
        std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output()
            .unwrap();
    }

    /// T-T7.1.2 (native plan step 8): do make a box on a node's runtime, run a program in it, and drop the
    /// runtime (the node's every handle closed, as when it is killed); expect the program still running,
    /// held by the keeper, the next runtime on the node's name taking the box back as running, and its
    /// delete ending the program.
    #[test]
    fn a_nodes_box_outlives_its_runtime_and_the_next_takes_it_back() {
        let (name, state) = (node("back"), tempfile::tempdir().unwrap());
        let spec = process_box("kept");
        let rt = WinRuntime::outliving(&name, state.path(), &keeper("back")).unwrap();
        rt.create(spec.clone()).unwrap();
        let pid = rt.exec(&spec.id, waiting(), Stdio::null()).unwrap().pid();
        drop(rt);
        assert!(alive(pid), "the box's program ended with its runtime");
        let rt = WinRuntime::outliving(&name, state.path(), &keeper("back")).unwrap();
        rt.adopt(&spec).unwrap();
        assert_eq!(rt.inspect(&spec.id).unwrap().state, BoxState::Running);
        rt.delete(&spec.id).unwrap();
        wait_for("the taken-back box's delete to end its program", || {
            !alive(pid)
        });
        if let Some(k) = rt.keeper_pid() {
            crash(k);
        }
    }

    /// T-T7.1.2: do leave a node's box running past its runtime, then end it by name from the next runtime
    /// (a box recovery removes); expect its program ended, and a second end, of nothing, accepted.
    #[test]
    fn a_box_no_node_takes_back_is_ended_by_name() {
        let (name, state) = (node("end"), tempfile::tempdir().unwrap());
        let spec = process_box("left");
        let rt = WinRuntime::outliving(&name, state.path(), &keeper("end")).unwrap();
        rt.create(spec.clone()).unwrap();
        let pid = rt.exec(&spec.id, waiting(), Stdio::null()).unwrap().pid();
        drop(rt);
        assert!(alive(pid), "the box's program ended with its runtime");
        let rt = WinRuntime::outliving(&name, state.path(), &keeper("end")).unwrap();
        rt.end(&spec.id).unwrap();
        wait_for("the end to end the box's program", || !alive(pid));
        rt.end(&spec.id).unwrap();
        if let Some(k) = rt.keeper_pid() {
            crash(k);
        }
    }

    /// T-T7.1.2: do run a program in a node's box, let the node go, then end its keeper too; expect
    /// kill-on-close to end the program with the job's last handle, so nothing is left running in a job no
    /// one can open.
    #[test]
    fn a_box_whose_node_and_keeper_are_both_gone_is_ended() {
        let (name, state) = (node("both"), tempfile::tempdir().unwrap());
        let spec = process_box("orphan");
        let rt = WinRuntime::outliving(&name, state.path(), &keeper("both")).unwrap();
        rt.create(spec.clone()).unwrap();
        let pid = rt.exec(&spec.id, waiting(), Stdio::null()).unwrap().pid();
        let kept = rt.keeper_pid().expect("a keeper holds the node's boxes");
        drop(rt);
        assert!(alive(pid), "the box's program ended with its runtime");
        crash(kept);
        wait_for("kill-on-close to end the program", || !alive(pid));
    }

    /// T-T7.1.2: do end a node's keeper while the node lives; expect the node to start another and give it
    /// the boxes' jobs, so a box still outlives the node afterwards and the next node takes it back.
    #[test]
    fn a_keeper_that_dies_while_its_node_lives_is_replaced() {
        let (name, state) = (node("again"), tempfile::tempdir().unwrap());
        let spec = process_box("rekept");
        let rt = WinRuntime::outliving(&name, state.path(), &keeper("again")).unwrap();
        rt.create(spec.clone()).unwrap();
        let pid = rt.exec(&spec.id, waiting(), Stdio::null()).unwrap().pid();
        let first = rt.keeper_pid().expect("a keeper holds the node's boxes");
        crash(first);
        wait_for("a new keeper", || {
            rt.keeper_pid().is_some_and(|k| k != first && alive(k))
        });
        drop(rt);
        assert!(alive(pid), "the box's program ended with its runtime");
        let rt = WinRuntime::outliving(&name, state.path(), &keeper("again")).unwrap();
        rt.adopt(&spec).unwrap();
        assert_eq!(rt.inspect(&spec.id).unwrap().state, BoxState::Running);
        rt.delete(&spec.id).unwrap();
        wait_for("the delete to end the program", || !alive(pid));
        if let Some(k) = rt.keeper_pid() {
            crash(k);
        }
    }

    /// T-T7.1.2 (native plan step 8): do end a node's keeper, then make a box on the node's runtime before
    /// anything replaces the keeper; expect the create to finish, the box held by a new keeper, rather than
    /// wait forever on its own runtime's box-map lock (found by s4), and the box to outlive the runtime.
    #[test]
    fn a_box_made_after_its_keeper_ended_is_made_and_held_by_a_new_keeper() {
        let (name, state) = (node("late"), tempfile::tempdir().unwrap());
        let spec = process_box("late");
        let rt = Arc::new(WinRuntime::outliving(&name, state.path(), &keeper("late")).unwrap());
        let first = rt.keeper_pid().expect("a keeper holds the node's boxes");
        crash(first);
        wait_for("the keeper to end", || !alive(first));
        let (made, making) = std::sync::mpsc::channel();
        let (runtime, asked) = (Arc::clone(&rt), spec.clone());
        let maker = std::thread::spawn(move || {
            let _ = made.send(runtime.create(asked).map(drop));
        });
        making
            .recv_timeout(std::time::Duration::from_secs(60))
            .expect("the create never finished: it waits on its own runtime's lock")
            .unwrap();
        maker.join().unwrap();
        let pid = rt.exec(&spec.id, waiting(), Stdio::null()).unwrap().pid();
        let second = rt.keeper_pid().expect("a new keeper holds the box");
        assert_ne!(second, first);
        drop(rt);
        assert!(alive(pid), "the box's program ended with its runtime");
        crash(second);
        wait_for("kill-on-close to end the program", || !alive(pid));
    }

    /// T-T7.1.2: do make a node's runtime on a state folder; expect its `oci` boxes' records and scratch
    /// kept inside that folder, so its start-time reclaim sweeps only its own boxes, not the machine's shared
    /// folder where a runtime driven in process (a bench's) keeps the records of boxes it is running.
    #[test]
    fn an_outliving_nodes_records_are_in_its_own_folder() {
        let (name, state) = (node("own"), tempfile::tempdir().unwrap());
        let rt = WinRuntime::outliving(&name, state.path(), &keeper("own")).unwrap();
        let own = rt.scratch_root.starts_with(state.path());
        if let Some(k) = rt.keeper_pid() {
            crash(k);
        }
        assert!(own, "the node keeps its records in {:?}", rt.scratch_root);
    }

    /// A runtime driven in process keeps kill-on-close: do make a box, run a program, drop the runtime;
    /// expect the program ended with it.
    #[test]
    fn an_in_process_runtimes_box_ends_with_it() {
        let spec = process_box("in-process");
        let rt = WinRuntime::new();
        rt.create(spec.clone()).unwrap();
        let pid = rt.exec(&spec.id, waiting(), Stdio::null()).unwrap().pid();
        drop(rt);
        wait_for("the program to end with its runtime", || !alive(pid));
    }
}
