//! The macOS runtime: [`hex_api::Runtime`] over the `process` class (docs/design/runtime.md). A box is a
//! process group under a QoS clamp, its bounds held by a watcher thread reading each member's footprint
//! (`process`, `sys`, `watch`); the `oci` class is a Linux kernel's, refused by name here until a Shards
//! guest carries it (the native plan's step 5). This is the node's runtime seam on macOS; the records,
//! recovery and socket that wrap it on Linux (`hex-node`) are not yet cross-OS, so this holds its boxes
//! in memory and is driven in process.

pub mod helper_client;
pub mod helper_daemon;
pub mod helper_sys;
pub mod proc;
pub mod process;
pub mod sys;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::ffi::CString;
use std::num::NonZeroU64;
use std::os::fd::{AsFd, OwnedFd};
use std::sync::{Arc, Mutex};

use hex_api::{
    Bounds, BoxId, BoxInfo, BoxSpec, BoxState, Class, ClassSpec, Membership, Proc, Program,
    Refusal, Runtime, Signal, Stdio, Usage,
};
use rustix::fs::{Mode, OFlags};

use process::{ProcessBox, Shared};

/// The headroom held back from a box's bound for the fork arming window, in bytes ([`hex_core::boxuser`]'s
/// reserve): a just-forked member is unarmed until the helper arms it, and this is what it may grow into
/// first. A constant until measured on the running machine (CLAUDE.md §5, `bench/native/macos/boxlab.sh`);
/// capped at a quarter of a box's bound so a small box still has most of its bound to apportion.
const RESERVE_BYTES: u64 = 16 << 20;

/// A box-user box as the node tracks it (the helper holds the live state): its bounds, to report on
/// inspect, and whether its leader has started — the first `exec` opens the box user's session.
struct UserBox {
    bounds: Bounds,
    leader_started: bool,
}

/// The node's box-user backend (native plan step 2): a connection to the privileged helper and the boxes
/// it manages.
struct BoxUser {
    /// Shared with each program it starts, which asks the helper for its end ([`proc::UserProc`]).
    helper: Arc<helper_client::Helper>,
    boxes: Mutex<BTreeMap<BoxId, UserBox>>,
}

/// The macOS node's runtime over the `process` class. Rootless (`helper` `None`), a box is a process group
/// under a watcher (step 1); with the privileged helper installed (`helper` `Some`), a box is a user of
/// its own the helper starts and bounds in the kernel (step 2). `new` is rootless; `detect` picks box-user
/// when the helper's socket answers. Holds its boxes in memory, keyed by id.
pub struct MacRuntime {
    shared: Arc<Shared>,
    boxes: Mutex<BTreeMap<BoxId, Arc<ProcessBox>>>,
    helper: Option<BoxUser>,
}

impl MacRuntime {
    /// A rootless runtime (no helper): boxes are process groups whose bounds a watcher holds, reckoned to
    /// grow at `rate_bytes` bytes and `rate_processes` processes a second (the bound's pacing, CLAUDE.md §5:
    /// measured at node start or cited, the caller says which).
    pub fn new(rate_bytes: NonZeroU64, rate_processes: NonZeroU64) -> Result<Self, Refusal> {
        let shared = Shared::new(rate_bytes, rate_processes)
            .map_err(|e| Refusal::os("reading the machine's timebase", e))?;
        Ok(Self {
            shared: Arc::new(shared),
            boxes: Mutex::new(BTreeMap::new()),
            helper: None,
        })
    }

    /// Picks box-user mode if the helper at `helper_socket` answers, else the rootless process-group mode
    /// of [`MacRuntime::new`]. The node's startup calls this; the conformance suite uses `new`.
    pub fn detect(
        rate_bytes: NonZeroU64,
        rate_processes: NonZeroU64,
        helper_socket: &std::path::Path,
    ) -> Result<Self, Refusal> {
        match helper_client::Helper::connect(helper_socket)? {
            Some(helper) => {
                let shared = Shared::new(rate_bytes, rate_processes)
                    .map_err(|e| Refusal::os("reading the machine's timebase", e))?;
                Ok(Self {
                    shared: Arc::new(shared),
                    boxes: Mutex::new(BTreeMap::new()),
                    helper: Some(BoxUser {
                        helper: Arc::new(helper),
                        boxes: Mutex::new(BTreeMap::new()),
                    }),
                })
            }
            None => Self::new(rate_bytes, rate_processes),
        }
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
            // No helper installed: this is the rootless process-group class (step 1), and inspect says so.
            membership: Some(Membership::ProcessGroup),
            // A process box has no init, so no container exit to report (HEX-RT-11).
            exit: None,
            abandoned: 0,
        })
    }
}

impl BoxUser {
    fn boxes(&self) -> Result<std::sync::MutexGuard<'_, BTreeMap<BoxId, UserBox>>, Refusal> {
        self.boxes
            .lock()
            .map_err(|_| Refusal::Broken("the node's box-user map lock is poisoned".to_owned()))
    }

    /// Acquires a pool user for the box and remembers its bounds.
    fn create(&self, spec: &BoxSpec) -> Result<BoxInfo, Refusal> {
        let mut boxes = self.boxes()?;
        if boxes.contains_key(&spec.id) {
            return Err(Refusal::Exists(spec.id.clone()));
        }
        let bound = spec.bounds.memory_max.unwrap_or(u64::MAX);
        let reserve = RESERVE_BYTES.min(bound / 4);
        self.helper.acquire(&spec.id, bound, reserve)?;
        boxes.insert(
            spec.id.clone(),
            UserBox {
                bounds: spec.bounds.clone(),
                leader_started: false,
            },
        );
        Ok(info(spec.id.clone(), spec.bounds.clone(), BoxState::Empty))
    }

    /// Starts `program` as the box's pool user through the helper (the first one opens the session).
    fn exec(&self, id: &BoxId, program: &Program, fds: [OwnedFd; 3]) -> Result<i32, Refusal> {
        let leader = {
            let mut boxes = self.boxes()?;
            let b = boxes
                .get_mut(id)
                .ok_or_else(|| Refusal::NoSuchBox(id.clone()))?;
            let leader = !b.leader_started;
            b.leader_started = true;
            leader
        };
        self.helper.start(
            id,
            program,
            [fds[0].as_fd(), fds[1].as_fd(), fds[2].as_fd()],
            leader,
        )
    }

    fn info(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
        let bounds = self
            .boxes()?
            .get(id)
            .map(|b| b.bounds.clone())
            .ok_or_else(|| Refusal::NoSuchBox(id.clone()))?;
        let state = self.helper.inspect(id)?;
        let box_state = if state.members > 0 {
            BoxState::Running
        } else {
            BoxState::Empty
        };
        Ok(info(id.clone(), bounds, box_state))
    }

    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        let s = self.helper.inspect(id)?;
        Ok(Usage {
            memory_bytes: s.footprint,
            memory_peak_bytes: s.peak,
            oom_kills: s.crossings,
            pids: u64::from(s.processes),
            pids_refused: 0,
            cpu_usec: 0,
            io_read_bytes: 0,
            io_write_bytes: 0,
        })
    }

    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        self.helper.end(id)?;
        self.boxes()?.remove(id);
        Ok(())
    }
}

/// A box-user box's [`BoxInfo`]: a `process` box, with box-user membership (the helper holds it), no init
/// exit, and no abandoned programs (the node's disconnect rule is the helper's here).
fn info(id: BoxId, bounds: Bounds, state: BoxState) -> BoxInfo {
    BoxInfo {
        id,
        class: Class::Process,
        state,
        bounds,
        adopted: false,
        membership: Some(Membership::BoxUser),
        exit: None,
        abandoned: 0,
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
        if let Some(bu) = &self.helper {
            return bu.create(&spec);
        }
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
        if let Some(bu) = &self.helper {
            if !bu.boxes()?.contains_key(id) {
                return Err(Refusal::NoSuchBox(id.clone()));
            }
        } else {
            let _ = self.get(id)?;
        }
        Err(Refusal::Invalid {
            what: "start",
            why: format!("box {id} is a process box, which has no init to start"),
        })
    }

    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        if let Some(bu) = &self.helper {
            program.check()?;
            if program.user.is_some() {
                return Err(Refusal::Invalid {
                    what: "program",
                    why: "a box-user box runs its programs as its pool user; a user is the oci class's"
                        .to_owned(),
                });
            }
            let pid = bu.exec(id, &program, stdio_fds(stdio)?)?;
            return Ok(Box::new(proc::UserProc::watch(
                pid,
                id.clone(),
                Arc::clone(&bu.helper),
            )?));
        }
        let b = self.get(id)?;
        // The box's whole memory bound arms each member with a fatal per-process cap (native plan step 1's
        // `_ext` backstop); the box tracks which members carry it.
        let spawn = build_spawn(&program, stdio, b.bounds().memory_max)?;
        let pid = b.exec(spawn)?;
        Ok(Box::new(proc::BoxProc::watch(pid, Some(b.members()))?))
    }

    fn signal(&self, id: &BoxId, signal: Signal) -> Result<(), Refusal> {
        if let Some(bu) = &self.helper {
            return bu.helper.signal(id, signal.number());
        }
        self.get(id)?.signal(signal)
    }

    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
        if let Some(bu) = &self.helper {
            return bu.info(id);
        }
        let b = self.get(id)?;
        self.info(&b)
    }

    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        if let Some(bu) = &self.helper {
            return bu.usage(id);
        }
        self.get(id)?.usage()
    }

    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        if let Some(bu) = &self.helper {
            let ids: Vec<BoxId> = bu.boxes()?.keys().cloned().collect();
            return ids.iter().map(|id| bu.info(id)).collect();
        }
        let boxes: Vec<Arc<ProcessBox>> = self.boxes()?.values().cloned().collect();
        boxes.iter().map(|b| self.info(b)).collect()
    }

    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        if let Some(bu) = &self.helper {
            return bu.delete(id);
        }
        let b = self
            .boxes()?
            .remove(id)
            .ok_or_else(|| Refusal::NoSuchBox(id.clone()))?;
        // Ends the box (kill, drain, stop its watcher); dropping the last Arc does the same, so deleting a
        // box a caller still holds through `list` is safe.
        b.delete()
    }
}

/// A program's three stdio descriptors for a box-user `exec`, each the null device where the caller gave
/// none (as the process class's spawn does), to ride to the helper by `SCM_RIGHTS`.
fn stdio_fds(stdio: Stdio) -> Result<[OwnedFd; 3], Refusal> {
    Ok([
        stdio
            .stdin
            .map_or_else(|| null(OFlags::RDONLY), |p| Ok(OwnedFd::from(p)))?,
        stdio
            .stdout
            .map_or_else(|| null(OFlags::WRONLY), |p| Ok(OwnedFd::from(p)))?,
        stdio
            .stderr
            .map_or_else(|| null(OFlags::WRONLY), |p| Ok(OwnedFd::from(p)))?,
    ])
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
    let parts = program_parts(program)?;
    Ok(sys::Spawn {
        paths: parts.paths,
        argv: parts.argv,
        envp: parts.envp,
        cwd: parts.cwd,
        stdio,
        jetsam_mib: memory_max.and_then(sys::jetsam_cap_mib),
        // No spec of this class names a descriptor limit, so each program gets the default (HEX-INST-6).
        nofile: Some(default_nofile()),
    })
}

/// The descriptor limit a box's program gets where its spec names none: Docker 29's, capped at this node's
/// own hard limit, which an unprivileged process can't raise ([`hex_core::limits`], native plan step 6).
pub(crate) fn default_nofile() -> hex_core::limits::Nofile {
    hex_core::limits::default_nofile(
        rustix::process::getrlimit(rustix::process::Resource::Nofile).maximum,
    )
}

/// A program's exec parts, apart from its stdio: the paths to try, its argv and environment, and its
/// working folder, as C strings. Shared by the `process` class's [`build_spawn`] and the box helper
/// daemon's privileged spawn ([`helper_sys`]), which supplies its own stdio and user.
pub(crate) struct Parts {
    pub paths: Vec<CString>,
    pub argv: Vec<CString>,
    pub envp: Vec<CString>,
    pub cwd: Option<CString>,
}

/// Builds a program's [`Parts`]: its path search, argv, environment and working folder.
pub(crate) fn program_parts(program: &Program) -> Result<Parts, Refusal> {
    program.check()?;
    Ok(Parts {
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
