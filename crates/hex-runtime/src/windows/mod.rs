//! The Windows runtime: [`hex_api::Runtime`] over the `process` class (docs/design/runtime.md). A box
//! is a job object; its bounds are the kernel's and hard (`process`, `sys`, [`hex_core::job`]), and a
//! member is started suspended inside the job and resumed, so nothing runs outside it. The `oci` class
//! is refused by name here: it names each Windows feature Hex's installer turns on that is off
//! ([`features`]), and with both on, that a Shards guest carries Linux images (the native plan's step
//! 5) and HCS Windows images (its step 4). This is the node's runtime seam on Windows; the records, recovery and socket that wrap it on
//! Linux (`hex-node`) are not yet cross-OS, so this holds its boxes in memory and is driven in process.

pub mod features;
pub mod proc;
pub mod process;
pub mod sys;

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;
use std::os::windows::io::OwnedHandle;
use std::sync::{Arc, Mutex, OnceLock};

use hex_api::{
    BoxId, BoxInfo, BoxSpec, BoxState, Class, ClassSpec, Proc, Program, Refusal, Runtime, Signal,
    Stdio, Usage,
};

use hex_core::setup::{self, WindowsFeature};
use process::JobBox;
use sys::{Spawn, Started};

/// The macOS node's runtime over the `process` class, held in memory, keyed by id.
pub struct WinRuntime {
    /// The machine's logical processors, the CPU hard cap's denominator (read once at node start).
    processors: NonZeroU32,
    boxes: Mutex<BTreeMap<BoxId, Arc<JobBox>>>,
    /// The installer's features that are off, read at the first `oci` create: a feature turns on or
    /// off only across a reboot, which starts the node again.
    features_off: OnceLock<BTreeSet<WindowsFeature>>,
}

/// What the Windows `oci` class waits on once the installer's features are on.
const OCI_NOT_BUILT: &str = "a Linux image needs a Linux kernel, which a Shards guest carries (native plan step 5), and Windows images run through HCS (native plan step 4); neither is built yet";

impl WinRuntime {
    /// A runtime for this machine, its processor count read from Windows.
    pub fn new() -> Self {
        Self::with_processors(sys::active_processors())
    }

    /// A runtime told the machine has `processors` logical processors (at least one).
    pub fn with_processors(processors: u32) -> Self {
        Self {
            processors: NonZeroU32::new(processors).unwrap_or(NonZeroU32::MIN),
            boxes: Mutex::new(BTreeMap::new()),
            features_off: OnceLock::new(),
        }
    }

    /// The installer's features that are off, read once ([`features::off`]); a failed read is refused
    /// and tried again at the next ask.
    fn features_off(&self) -> Result<&BTreeSet<WindowsFeature>, Refusal> {
        if let Some(off) = self.features_off.get() {
            return Ok(off);
        }
        let off = features::off(self.processors)?;
        Ok(self.features_off.get_or_init(|| off))
    }

    fn boxes(&self) -> Result<std::sync::MutexGuard<'_, BTreeMap<BoxId, Arc<JobBox>>>, Refusal> {
        self.boxes
            .lock()
            .map_err(|_| Refusal::Broken("the node's box map lock is poisoned".to_owned()))
    }

    fn get(&self, id: &BoxId) -> Result<Arc<JobBox>, Refusal> {
        self.boxes()?
            .get(id)
            .cloned()
            .ok_or_else(|| Refusal::NoSuchBox(id.clone()))
    }

    fn info(&self, b: &JobBox) -> Result<BoxInfo, Refusal> {
        Ok(BoxInfo {
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
        })
    }
}

impl Default for WinRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl Runtime for WinRuntime {
    fn classes(&self) -> Vec<Class> {
        vec![Class::Process]
    }

    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        spec.bounds.check()?;
        let ClassSpec::Process = spec.class else {
            return Err(setup::windows_oci(self.features_off()?, OCI_NOT_BUILT));
        };
        let mut boxes = self.boxes()?;
        if boxes.contains_key(&spec.id) {
            return Err(Refusal::Exists(spec.id));
        }
        let b = Arc::new(JobBox::create(
            spec.id.clone(),
            &spec.bounds,
            self.processors,
        )?);
        let info = self.info(&b)?;
        boxes.insert(spec.id, b);
        Ok(info)
    }

    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        // A process box has no init to start: exec runs its programs, refused as the simulator and the
        // other OSes refuse it (HEX-RT-10). An absent box is NoSuchBox.
        let _ = self.get(id)?;
        Err(Refusal::Invalid {
            what: "start",
            why: format!("box {id} is a process box, which has no init to start"),
        })
    }

    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        let b = self.get(id)?;
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
        let boxes: Vec<Arc<JobBox>> = self.boxes()?.values().cloned().collect();
        boxes.iter().map(|b| self.info(b)).collect()
    }

    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        let b = self
            .boxes()?
            .remove(id)
            .ok_or_else(|| Refusal::NoSuchBox(id.clone()))?;
        // Ends the box (terminate its job); dropping the last Arc closes the job handle (kill-on-close)
        // as a backstop, so deleting a box a caller still holds through `list` is safe.
        b.delete()
    }
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
        command_line: command_line(program),
        environment: environment(program),
        cwd,
        stdio: [
            handle(stdio.stdin.map(OwnedHandle::from))?,
            handle(stdio.stdout.map(OwnedHandle::from))?,
            handle(stdio.stderr.map(OwnedHandle::from))?,
        ],
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

/// The command line for `program`, UTF-16 and NUL-terminated: each argument quoted as the Microsoft C
/// runtime parses `argv` (`docs/research/runtime/windows-job-objects.md`, cpp-docs
/// main-function-command-line-args.md), so a program reads back exactly the arguments it was given.
/// `program.check` has already refused a NUL in any argument.
fn command_line(program: &Program) -> Vec<u16> {
    let mut s = String::new();
    for (i, arg) in program.args.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        push_quoted(arg, &mut s);
    }
    s.encode_utf16().chain([0]).collect()
}

/// Appends `arg` to a command line, quoted as the C runtime parses it: a run of backslashes is doubled
/// only before a quote or the closing quote, and an embedded quote is escaped with a backslash.
fn push_quoted(arg: &str, out: &mut String) {
    let bare = !arg.is_empty()
        && !arg
            .chars()
            .any(|c| matches!(c, ' ' | '\t' | '\n' | '\u{0b}' | '"'));
    if bare {
        out.push_str(arg);
        return;
    }
    out.push('"');
    let mut backslashes: usize = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes = backslashes.saturating_add(1),
            '"' => {
                // The run of backslashes is literal (double it) and the quote is escaped (one more).
                for _ in 0..backslashes.saturating_mul(2).saturating_add(1) {
                    out.push('\\');
                }
                out.push('"');
                backslashes = 0;
            }
            other => {
                for _ in 0..backslashes {
                    out.push('\\');
                }
                backslashes = 0;
                out.push(other);
            }
        }
    }
    // Trailing backslashes precede the closing quote, so they are doubled to stay literal.
    for _ in 0..backslashes.saturating_mul(2) {
        out.push('\\');
    }
    out.push('"');
}

/// The environment block for `program`, UTF-16: each `name=value` NUL-terminated, the strings sorted
/// case-insensitively (Windows reads a sorted block, `changing-environment-variables.md:15`), the whole
/// double-NUL-terminated. Nothing is inherited; an empty environment is an empty block (two NULs).
/// `program.check` has already refused a NUL or an `=` in a name and a NUL in a value.
fn environment(program: &Program) -> Vec<u16> {
    let mut vars: Vec<(&String, &String)> = program.env.iter().map(|(k, v)| (k, v)).collect();
    vars.sort_by(|a, b| {
        a.0.to_uppercase()
            .cmp(&b.0.to_uppercase())
            .then_with(|| a.0.cmp(b.0))
    });
    let mut block: Vec<u16> = Vec::new();
    for (k, v) in vars {
        block.extend(k.encode_utf16());
        block.push(u16::from(b'='));
        block.extend(v.encode_utf16());
        block.push(0);
    }
    // An empty block is still a valid double-NUL-terminated block.
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}
