//! The macOS runtime: [`hex_api::Runtime`] over the `process` class (docs/design/runtime.md). A box is a
//! process group under a QoS clamp, its bounds held by a watcher thread reading each member's footprint
//! (`process`, `sys`, `watch`); the `oci` class is a Linux kernel's, refused by name here until a Shards
//! guest carries it (the native plan's step 5). This is the node's runtime seam on macOS; the records,
//! recovery and socket that wrap it on Linux (`hex-node`) are not yet cross-OS, so this holds its boxes
//! in memory and is driven in process.

pub mod proc;
pub mod process;
pub mod sys;

use std::collections::BTreeMap;
use std::ffi::CString;
use std::num::NonZeroU64;
use std::os::fd::OwnedFd;
use std::sync::{Arc, Mutex};

use hex_api::{
    BoxId, BoxInfo, BoxSpec, BoxState, Class, ClassSpec, ExecLimit, Proc, Program, Refusal,
    Runtime, Signal, Stdio, Usage,
};
use rustix::fs::{Mode, OFlags};

use process::{ProcessBox, Shared};

/// The bound macOS's execve puts on a program's arguments and environment, read from the kernel now
/// (`kern.argmax`, kind (a); docs/research/runtime/exec-size-bounds.md). XNU has no bound on one string apart.
pub fn exec_limit() -> Result<ExecLimit, Refusal> {
    Ok(ExecLimit {
        block: sys::arg_max().map_err(|e| Refusal::os("reading kern.argmax", e))?,
        counts: hex_api::BlockCount::Execve,
        string: None,
        unit: hex_api::StringUnit::Bytes,
        source: "macOS's execve (kern.argmax)",
    })
}

/// The macOS node's runtime over the `process` class. Holds its boxes in memory, keyed by id.
pub struct MacRuntime {
    shared: Arc<Shared>,
    boxes: Mutex<BTreeMap<BoxId, Arc<ProcessBox>>>,
    /// The kernel's bound on a program's arguments and environment, read at start (`kern.argmax`).
    exec_limit: ExecLimit,
}

impl MacRuntime {
    /// A runtime whose boxes are reckoned to grow at `rate_bytes` bytes and `rate_processes` processes a
    /// second (the bound's pacing, CLAUDE.md §5: measured at node start or cited, the caller says which).
    pub fn new(rate_bytes: NonZeroU64, rate_processes: NonZeroU64) -> Result<Self, Refusal> {
        let shared = Shared::new(rate_bytes, rate_processes)
            .map_err(|e| Refusal::os("reading the machine's timebase", e))?;
        Ok(Self {
            shared: Arc::new(shared),
            boxes: Mutex::new(BTreeMap::new()),
            exec_limit: exec_limit()?,
        })
    }

    fn boxes(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, BTreeMap<BoxId, Arc<ProcessBox>>>, Refusal> {
        self.boxes
            .lock()
            .map_err(|_| Refusal::Broken("the node's box map lock is poisoned".to_owned()))
    }

    fn get(&self, id: &BoxId) -> Result<Arc<ProcessBox>, Refusal> {
        self.boxes()?
            .get(id)
            .cloned()
            .ok_or_else(|| Refusal::NoSuchBox(id.clone()))
    }

    fn info(&self, b: &ProcessBox) -> Result<BoxInfo, Refusal> {
        Ok(BoxInfo {
            id: b.id().clone(),
            class: Class::Process,
            state: if b.running_now()? {
                BoxState::Running
            } else {
                BoxState::Empty
            },
            bounds: b.bounds(),
            adopted: false,
            // A process box has no init, so no container exit to report (HEX-RT-11).
            exit: None,
            abandoned: 0,
        })
    }
}

impl Runtime for MacRuntime {
    fn classes(&self) -> Vec<Class> {
        vec![Class::Process]
    }

    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        spec.bounds.check()?;
        let ClassSpec::Process = spec.class else {
            return Err(Refusal::Platform {
                what: "the oci class",
                platform: "macos",
                why: "a Linux image needs a Linux kernel; a Shards guest carries it (native plan step 5)"
                    .to_owned(),
            });
        };
        let mut boxes = self.boxes()?;
        if boxes.contains_key(&spec.id) {
            return Err(Refusal::Exists(spec.id));
        }
        let b = Arc::new(ProcessBox::create(
            spec.id.clone(),
            &spec.bounds,
            self.shared.as_ref(),
        )?);
        let info = self.info(&b)?;
        boxes.insert(spec.id, b);
        Ok(info)
    }

    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        // A process box has no init to start: exec runs its programs. Refused as the simulator and the
        // Linux node refuse it (HEX-RT-10). An absent box is NoSuchBox.
        let _ = self.get(id)?;
        Err(Refusal::Invalid {
            what: "start",
            why: format!("box {id} is a process box, which has no init to start"),
        })
    }

    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        program.fits(&self.exec_limit)?;
        let b = self.get(id)?;
        // The box's whole memory bound arms each member with a fatal per-process cap (native plan step 1's
        // `_ext` backstop); the box tracks which members carry it.
        let spawn = build_spawn(&program, stdio, b.bounds().memory_max)?;
        let pid = b.exec(spawn)?;
        Ok(Box::new(proc::BoxProc::watch(pid, b.members())?))
    }

    fn signal(&self, id: &BoxId, signal: Signal) -> Result<(), Refusal> {
        self.get(id)?.signal(signal)
    }

    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
        let b = self.get(id)?;
        self.info(&b)
    }

    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        self.get(id)?.usage()
    }

    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        let boxes: Vec<Arc<ProcessBox>> = self.boxes()?.values().cloned().collect();
        boxes.iter().map(|b| self.info(b)).collect()
    }

    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        let b = self
            .boxes()?
            .remove(id)
            .ok_or_else(|| Refusal::NoSuchBox(id.clone()))?;
        // Ends the box (kill, drain, stop its watcher); dropping the last Arc does the same, so deleting a
        // box a caller still holds through `list` is safe.
        b.delete()
    }
}

/// Builds the [`sys::Spawn`] for `program` with `stdio`: its exec path search (as the Linux process class
/// does it), its argv and environment, its working folder, its three streams (each the null device where
/// the caller gave none), and, when the box has a memory bound (`memory_max`), the fatal per-process
/// jetsam cap in MiB that arms each member at the box's whole bound (native plan step 1's `_ext`
/// backstop). The cap rounds down to MiB, so it never exceeds the bound ([`sys::jetsam_cap_mib`]).
fn build_spawn(
    program: &Program,
    stdio: Stdio,
    memory_max: Option<u64>,
) -> Result<sys::Spawn, Refusal> {
    program.check()?;
    if program.user.is_some() {
        return Err(Refusal::Invalid {
            what: "program",
            why: "a process box runs its programs as the node agent's user; a user is the oci class's"
                .to_owned(),
        });
    }
    let stdio = [
        stdio
            .stdin
            .map_or_else(|| null(OFlags::RDONLY), |p| Ok(OwnedFd::from(p)))?,
        stdio
            .stdout
            .map_or_else(|| null(OFlags::WRONLY), |p| Ok(OwnedFd::from(p)))?,
        stdio
            .stderr
            .map_or_else(|| null(OFlags::WRONLY), |p| Ok(OwnedFd::from(p)))?,
    ];
    Ok(sys::Spawn {
        paths: paths(program)?,
        argv: program
            .args
            .iter()
            .map(|a| c_string(a))
            .collect::<Result<_, _>>()?,
        envp: program
            .env
            .iter()
            .map(|(k, v)| c_string(&format!("{k}={v}")))
            .collect::<Result<_, _>>()?,
        cwd: program.cwd.as_deref().map(c_string).transpose()?,
        stdio,
        jetsam_mib: memory_max.and_then(sys::jetsam_cap_mib),
    })
}

/// The paths a program is executed from, in order, as execvp(3) tries them: its name when it holds a `/`,
/// else that name in each folder of its own `PATH`, or of the default `/usr/bin:/bin` when it sets none.
fn paths(program: &Program) -> Result<Vec<CString>, Refusal> {
    let first = program.args.first().ok_or(Refusal::Invalid {
        what: "program",
        why: "no arguments".to_owned(),
    })?;
    if first.contains('/') {
        return Ok(vec![c_string(first)?]);
    }
    let path = program
        .env
        .iter()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.as_str())
        .unwrap_or("/usr/bin:/bin");
    path.split(':')
        .map(|dir| {
            c_string(&format!(
                "{}/{first}",
                if dir.is_empty() { "." } else { dir }
            ))
        })
        .collect()
}

fn c_string(s: &str) -> Result<CString, Refusal> {
    CString::new(s).map_err(|_| Refusal::Invalid {
        what: "program",
        why: format!("{s:?} holds a NUL byte"),
    })
}

/// The null device, for a stream the caller gave none.
fn null(flags: OFlags) -> Result<OwnedFd, Refusal> {
    rustix::fs::open("/dev/null", flags | OFlags::CLOEXEC, Mode::empty())
        .map_err(|e| Refusal::os("opening /dev/null", e))
}
