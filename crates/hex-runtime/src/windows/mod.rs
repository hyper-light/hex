//! The Windows runtime: [`hex_api::Runtime`] over the `process` and `oci` classes
//! (docs/design/runtime.md). A `process` box is a job object; its bounds are the kernel's and hard
//! (`process`, `sys`, [`hex_core::job`]), and a member is started suspended inside the job and resumed, so
//! nothing runs outside it. An `oci` box is a Windows image run process-isolated through the Host Compute
//! Service with such a job nested in its silo (`oci`, [`hex_core::hcs`]). Where a Windows feature Hex's
//! installer turns on is off ([`features`]), the class is refused by name, naming it. This is the node's
//! runtime seam on Windows; the records, recovery and socket that wrap it on Linux (`hex-node`) are not
//! yet cross-OS, so this holds its boxes in memory and is driven in process.

pub mod features;
pub mod oci;
pub mod proc;
pub mod process;
pub mod sys;

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;
use std::os::windows::io::OwnedHandle;
use std::path::PathBuf;
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
    /// The installer's features that are off, read when the runtime is made (node start), so no box's
    /// create waits on the query: a feature turns on or off only across a reboot, which starts the
    /// node again.
    features_off: OnceLock<BTreeSet<WindowsFeature>>,
    /// How many times [`features::off`] ran, for the test that no create runs it.
    #[cfg(test)]
    feature_reads: std::sync::atomic::AtomicU32,
}

impl WinRuntime {
    /// A runtime for this machine, its processor count read from Windows, its `oci` scratch under
    /// `%ProgramData%\hex\boxes`.
    pub fn new() -> Self {
        Self::with(sys::active_processors(), default_scratch_root())
    }

    /// A runtime told the machine has `processors` logical processors (at least one), keeping `oci`
    /// scratch under `scratch_root`. The installer's features are read here, before any box, so the
    /// first `oci` create doesn't wait on PowerShell's query (p50 1,183 ms on jupiter,
    /// bench/install/records/windows-jupiter-20261105.txt entry 19). A read that fails here isn't
    /// lost: the first box that needs the features reads them again, and is refused by name if that
    /// fails too.
    pub fn with(processors: u32, scratch_root: PathBuf) -> Self {
        let runtime = Self {
            processors: NonZeroU32::new(processors).unwrap_or(NonZeroU32::MIN),
            scratch_root,
            launcher: None,
            boxes: Mutex::new(BTreeMap::new()),
            node: None,
            features_off: OnceLock::new(),
            #[cfg(test)]
            feature_reads: std::sync::atomic::AtomicU32::new(0),
        };
        // Read now so no create waits on it; a failure is read again, and refused, at the first ask.
        let _ = runtime.features_off();
        runtime
    }

    /// A node's runtime on this machine whose `process` boxes outlive it: each is the job named for `node`
    /// and the box, without kill-on-close, so a node killed with boxes running leaves them running, and the
    /// next node on the same name takes them back ([`WinRuntime::adopt`]) or ends them
    /// ([`WinRuntime::end`]). `node` names one node's boxes apart from another's in the session.
    pub fn outliving(node: &str) -> Self {
        let mut runtime = Self::new();
        runtime.node = Some(node.to_owned());
        runtime
    }

    /// The job name a node's `process` box `id` has (`Local\hex-<node>-<box>`), where its boxes outlive it.
    fn job_name(&self, id: &BoxId) -> Option<String> {
        self.node
            .as_ref()
            .map(|node| format!("Local\\hex-{node}-{id}"))
    }

    /// Takes back `process` box `spec` a node before this one made, with whatever runs in it (made again,
    /// empty, where nothing does). Refused for a runtime whose boxes don't outlive it, and for an `oci` box,
    /// whose compute system went with its node.
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
        if !matches!(spec.class, ClassSpec::Process) {
            return Err(Refusal::Invalid {
                what: "adopt",
                why: format!(
                    "box {} is an oci box, whose compute system ends with its node",
                    spec.id
                ),
            });
        }
        spec.bounds.check()?;
        let b = JobBox::take_back(spec.id.clone(), &spec.bounds, self.processors, &name)?;
        let mut boxes = self.boxes()?;
        if boxes.contains_key(&spec.id) {
            return Err(Refusal::Exists(spec.id.clone()));
        }
        boxes.insert(spec.id.clone(), Held::Process(Arc::new(b)));
        Ok(())
    }

    /// Ends what still runs of `process` box `id` a node before this one made, for a box recovery removes;
    /// one that left nothing has nothing to end. A runtime whose boxes end with it has none to end.
    pub fn end(&self, id: &BoxId) -> Result<(), Refusal> {
        match self.job_name(id) {
            Some(name) => JobBox::end_named(&name),
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
                // A process box has no init, so no container exit to report (HEX-RT-11).
                exit: None,
                abandoned: 0,
            }),
            Held::Oci(b) => oci::info(b),
        }
    }

    /// The `oci` box `spec` asks for, made.
    fn create_oci(&self, spec: &BoxSpec) -> Result<OciBox, Refusal> {
        let container = match &spec.class {
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
        };
        let launcher = match &self.launcher {
            Some(d) => d.clone(),
            None => std::env::current_exe()
                .ok()
                .and_then(|e| e.parent().map(std::path::Path::to_path_buf))
                .ok_or_else(|| Refusal::Broken("the node can't name its own folder".to_owned()))?,
        };
        OciBox::create(
            spec.id.clone(),
            &spec.bounds,
            self.processors,
            &container,
            &launcher,
        )
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
        let mut boxes = self.boxes()?;
        if boxes.contains_key(&spec.id) {
            // Made beside another create of the same id: this one is ended, the first kept.
            if let Held::Oci(b) = &held {
                let _ = b.delete();
            }
            return Err(Refusal::Exists(spec.id));
        }
        boxes.insert(spec.id, held);
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
        // Ends the box; dropping the last Arc closes its job (kill-on-close, for a box that doesn't outlive
        // its node) and, for an oci box, its container's last handle (terminate-on-close) as a backstop, so
        // deleting a box a caller still holds through `list` is safe.
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

    /// T-T7.1.2 (native plan step 8): do make a box on a node's runtime, run a program in it, and drop the
    /// runtime (every handle the node held closed, as when it is killed); expect the program still
    /// running, the next runtime on the node's name taking the box back as running, and its delete ending
    /// the program.
    #[test]
    fn a_nodes_box_outlives_its_runtime_and_the_next_takes_it_back() {
        let name = node("back");
        let spec = process_box("kept");
        let rt = WinRuntime::outliving(&name);
        rt.create(spec.clone()).unwrap();
        let pid = rt.exec(&spec.id, waiting(), Stdio::null()).unwrap().pid();
        drop(rt);
        assert!(alive(pid), "the box's program ended with its runtime");
        let rt = WinRuntime::outliving(&name);
        rt.adopt(&spec).unwrap();
        assert_eq!(rt.inspect(&spec.id).unwrap().state, BoxState::Running);
        rt.delete(&spec.id).unwrap();
        wait_for("the taken-back box's delete to end its program", || {
            !alive(pid)
        });
    }

    /// T-T7.1.2: do leave a node's box running past its runtime, then end it by name from the next runtime
    /// (a box recovery removes); expect its program ended, and a second end, of nothing, accepted.
    #[test]
    fn a_box_no_node_takes_back_is_ended_by_name() {
        let name = node("end");
        let spec = process_box("left");
        let rt = WinRuntime::outliving(&name);
        rt.create(spec.clone()).unwrap();
        let pid = rt.exec(&spec.id, waiting(), Stdio::null()).unwrap().pid();
        drop(rt);
        assert!(alive(pid), "the box's program ended with its runtime");
        let rt = WinRuntime::outliving(&name);
        rt.end(&spec.id).unwrap();
        wait_for("the end to end the box's program", || !alive(pid));
        rt.end(&spec.id).unwrap();
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
