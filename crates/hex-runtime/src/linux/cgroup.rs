//! Boxes as cgroup v2 directories under one root the node agent owns (docs/design/runtime.md, "The
//! process box"). The root's parent hands it the controllers boxes use, and the root hands them on
//! to every box; cgroup v2's no-internal-processes rule means neither may hold a process itself, so
//! the node agent runs beside the root, never in it.

use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hex_api::{Bounds, BoxId, Refusal, Signal, Usage};
use hex_core::cgroup::{self as core, CONTROLLERS, Dev, UsageFiles};
use rustix::fs::{Mode, OFlags};

use super::os_err;

/// Where cgroup v2 is mounted.
pub const CGROUP_ROOT: &str = "/sys/fs/cgroup";
/// Most boxes one root holds; past it `create` is refused with [`Refusal::Full`]. The kernel's own
/// default bound on descendants is unlimited; this is Hex's, well above a node's useful load.
pub const MAX_BOXES: usize = 65_536;
/// Most processes a signal is sent to one by one in a box (signals other than KILL, which the
/// kernel delivers to the whole box at once through `cgroup.kill`).
pub const MAX_SIGNALLED: usize = 1 << 20;

/// The cgroup this process is in, as a path under [`CGROUP_ROOT`].
pub fn own_cgroup() -> Result<PathBuf, Refusal> {
    let text = fs::read_to_string("/proc/self/cgroup")
        .map_err(|e| Refusal::os("reading /proc/self/cgroup", e))?;
    let rel = text
        .lines()
        .find_map(|l| l.strip_prefix("0::"))
        .ok_or_else(|| Refusal::Platform {
            what: "the process class",
            platform: "linux",
            why: "this process isn't in a cgroup v2 hierarchy (no 0:: line in /proc/self/cgroup)"
                .to_owned(),
        })?;
    Ok(Path::new(CGROUP_ROOT).join(rel.trim_start_matches('/')))
}

/// The directory that holds a node's boxes, each a cgroup below it.
#[derive(Debug, Clone)]
pub struct BoxRoot {
    dir: PathBuf,
    controllers: BTreeSet<String>,
    disks: Vec<Dev>,
}

impl BoxRoot {
    /// Opens the box root at `dir`, making it when it's missing: its parent is asked to hand it each
    /// of [`CONTROLLERS`] it has, and it hands them on to boxes. `disks` are the devices
    /// `io.latency` and `io.max` apply to. Refused when the parent holds processes of its own (the
    /// kernel then can't hand controllers down) or isn't writable (not delegated).
    pub fn open(dir: &Path, disks: Vec<Dev>) -> Result<Self, Refusal> {
        let parent = dir.parent().ok_or_else(|| Refusal::Invalid {
            what: "box root",
            why: format!("{} has no parent cgroup", dir.display()),
        })?;
        match fs::create_dir(dir) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => return Err(os_err("making the box root", dir, e)),
        }
        let available = read_set(&parent.join("cgroup.controllers"))?;
        let wanted: Vec<&str> = CONTROLLERS
            .iter()
            .copied()
            .filter(|c| available.contains(*c))
            .collect();
        let enable = wanted
            .iter()
            .map(|c| format!("+{c}"))
            .collect::<Vec<_>>()
            .join(" ");
        if !enable.is_empty() {
            if let Err(e) = write_file(&parent.join("cgroup.subtree_control"), &enable) {
                let procs = fs::read_to_string(parent.join("cgroup.procs")).unwrap_or_default();
                let why = if procs.trim().is_empty() {
                    format!("{e}")
                } else {
                    format!(
                        "{e}: {} holds processes of its own, so cgroup v2 can't hand its controllers to the box root; run the node agent in a leaf cgroup beside it (systemd: Delegate=yes, DelegateSubgroup=)",
                        parent.display()
                    )
                };
                return Err(Refusal::Os {
                    op: "enabling controllers for the box root",
                    why,
                });
            }
            write_file(&dir.join("cgroup.subtree_control"), &enable)?;
        }
        let controllers = read_set(&dir.join("cgroup.subtree_control"))?;
        Ok(Self {
            dir: dir.to_owned(),
            controllers,
            disks,
        })
    }

    /// The box root beside this process's own cgroup: `<parent of own cgroup>/hex`. The node agent
    /// runs in a leaf (`.../runner`) and its boxes in `.../hex/<id>`.
    pub fn beside_self(disks: Vec<Dev>) -> Result<Self, Refusal> {
        let own = own_cgroup()?;
        let parent = own.parent().ok_or_else(|| Refusal::Invalid {
            what: "box root",
            why: format!(
                "{} is the root cgroup; the node agent needs a delegated subtree",
                own.display()
            ),
        })?;
        Self::open(&parent.join("hex"), disks)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The controllers boxes get.
    pub fn controllers(&self) -> &BTreeSet<String> {
        &self.controllers
    }

    /// The disks `io.latency` and `io.max` apply to.
    pub fn disks(&self) -> &[Dev] {
        &self.disks
    }

    /// Makes box `id`'s cgroup with `bounds` in force. A bound whose controller the root doesn't
    /// have is refused by name, before anything is made.
    pub fn create(&self, id: &BoxId, bounds: &Bounds) -> Result<BoxCgroup, Refusal> {
        for (needed, set) in [
            (
                "cpu",
                bounds.cpu_weight.is_some() || bounds.cpu_max_milli.is_some(),
            ),
            (
                "memory",
                bounds.memory_max.is_some() || bounds.memory_high.is_some(),
            ),
            (
                "io",
                bounds.io_weight.is_some()
                    || bounds.io_latency_us.is_some()
                    || bounds.io_max.is_some(),
            ),
            ("pids", bounds.pids_max.is_some()),
        ] {
            if set && !self.controllers.contains(needed) {
                return Err(Refusal::Platform {
                    what: "a bound",
                    platform: "linux",
                    why: format!(
                        "the {needed} controller isn't delegated to {} (it has {:?}): delegate it to the node agent's cgroup",
                        self.dir.display(),
                        self.controllers
                    ),
                });
            }
        }
        let writes = core::writes(bounds, &self.disks)?;
        if self.boxes()?.len() >= MAX_BOXES {
            return Err(Refusal::Full(MAX_BOXES));
        }
        let dir = self.dir.join(id.as_str());
        match fs::create_dir(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                return Err(Refusal::Exists(id.clone()));
            }
            Err(e) => return Err(os_err("making a box's cgroup", &dir, e)),
        }
        let cg = BoxCgroup { dir };
        for w in writes {
            // A kernel without swap accounting has no memory.swap.max. With no swap on, the memory
            // bound holds without it; with swap on it cannot hold, so the box is refused by name.
            if w.file == "memory.swap.max" && !cg.dir.join(w.file).exists() {
                match swap_active() {
                    Ok(false) => continue,
                    Ok(true) => {
                        let _ = fs::remove_dir(&cg.dir);
                        return Err(Refusal::Platform {
                            what: "a memory bound",
                            platform: "linux",
                            why: "swap is on but this kernel has no swap accounting (no memory.swap.max), so a box could swap past its memory bound".to_owned(),
                        });
                    }
                    Err(e) => {
                        let _ = fs::remove_dir(&cg.dir);
                        return Err(e);
                    }
                }
            }
            if let Err(e) = write_file(&cg.dir.join(w.file), &w.value) {
                // A box is made whole or not at all; its empty cgroup goes with the refusal.
                let _ = fs::remove_dir(&cg.dir);
                return Err(e);
            }
        }
        Ok(cg)
    }

    /// Box `id`'s cgroup, when it exists.
    pub fn get(&self, id: &BoxId) -> Option<BoxCgroup> {
        let dir = self.dir.join(id.as_str());
        if dir.is_dir() {
            Some(BoxCgroup { dir })
        } else {
            None
        }
    }

    /// The name of every cgroup under the root, box or not, bounded by [`MAX_BOXES`].
    pub fn boxes(&self) -> Result<Vec<String>, Refusal> {
        let mut out = Vec::new();
        let entries = fs::read_dir(&self.dir).map_err(|e| os_err("listing boxes", &self.dir, e))?;
        for entry in entries.take(MAX_BOXES) {
            let entry = entry.map_err(|e| os_err("listing boxes", &self.dir, e))?;
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                out.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        Ok(out)
    }
}

/// One box's cgroup.
#[derive(Debug, Clone)]
pub struct BoxCgroup {
    dir: PathBuf,
}

impl BoxCgroup {
    /// The cgroup at `dir`, wherever it is: an `oci` container's cgroup goes where its spec puts it,
    /// not under a box root, and is killed, waited out and removed as a box's is.
    pub fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Its `cgroup.procs`, open for writing: a process that writes "0" to it joins the box.
    pub fn procs_for_joining(&self) -> Result<OwnedFd, Refusal> {
        let path = self.dir.join("cgroup.procs");
        rustix::fs::open(&path, OFlags::WRONLY | OFlags::CLOEXEC, Mode::empty())
            .map_err(|e| os_err("opening a box's cgroup.procs", &path, e))
    }

    /// The cgroup directory itself, open: what `clone3(CLONE_INTO_CGROUP)` takes.
    pub fn open_dir(&self) -> Result<OwnedFd, Refusal> {
        rustix::fs::open(
            &self.dir,
            OFlags::DIRECTORY | OFlags::RDONLY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| os_err("opening a box's cgroup", &self.dir, e))
    }

    /// The processes in the box now.
    pub fn procs(&self) -> Result<Vec<u32>, Refusal> {
        let path = self.dir.join("cgroup.procs");
        let text =
            fs::read_to_string(&path).map_err(|e| os_err("reading cgroup.procs", &path, e))?;
        Ok(text
            .lines()
            .filter_map(|l| l.trim().parse().ok())
            .take(MAX_SIGNALLED)
            .collect())
    }

    /// Whether any process runs in the box.
    pub fn populated(&self) -> Result<bool, Refusal> {
        let path = self.dir.join("cgroup.events");
        let text =
            fs::read_to_string(&path).map_err(|e| os_err("reading cgroup.events", &path, e))?;
        core::populated(&text).ok_or_else(|| Refusal::Os {
            op: "reading cgroup.events",
            why: format!("{}: no populated line", path.display()),
        })
    }

    /// Sends `signal` to every process in the box; refused as not running when there is none.
    pub fn signal(&self, id: &BoxId, signal: Signal) -> Result<(), Refusal> {
        if !self.populated()? {
            return Err(Refusal::NotRunning(id.clone(), "signal"));
        }
        if signal == Signal::KILL {
            return self.kill();
        }
        let sig =
            rustix::process::Signal::from_named_raw(signal.number()).ok_or(Refusal::Invalid {
                what: "signal",
                why: format!("{} isn't a signal this kernel names", signal.number()),
            })?;
        for pid in self.procs()? {
            // A process that ended since the list was read is no longer the box's to signal.
            if let Some(pid) = i32::try_from(pid)
                .ok()
                .and_then(rustix::process::Pid::from_raw)
            {
                let _ = rustix::process::kill_process(pid, sig);
            }
        }
        Ok(())
    }

    /// Kills every process in the box at once (`cgroup.kill`, Linux 5.14).
    pub fn kill(&self) -> Result<(), Refusal> {
        write_file(&self.dir.join("cgroup.kill"), "1")
    }

    /// Waits until no process is left in the box, at most `timeout`; whether none is.
    pub fn wait_empty(&self, timeout: Duration) -> Result<bool, Refusal> {
        let path = self.dir.join("cgroup.events");
        let fd = rustix::fs::open(&path, OFlags::RDONLY | OFlags::CLOEXEC, Mode::empty())
            .map_err(|e| os_err("opening cgroup.events", &path, e))?;
        let deadline = Instant::now().checked_add(timeout);
        let mut buf = [0u8; 256];
        loop {
            let n = rustix::io::pread(&fd, &mut buf, 0)
                .map_err(|e| os_err("reading cgroup.events", &path, e))?;
            let text = String::from_utf8_lossy(buf.get(..n).unwrap_or_default());
            if core::populated(&text) == Some(false) {
                return Ok(true);
            }
            let left = match deadline {
                Some(d) => d.saturating_duration_since(Instant::now()),
                None => timeout,
            };
            if left.is_zero() {
                return Ok(false);
            }
            // The kernel marks cgroup.events modified (POLLPRI) when `populated` changes, but on some
            // kernels that notification lags the actual depopulation by several ms (CachyOS 7.2;
            // probes/oci_breakdown.md run 3), which would leave `delete` waiting on a late wake. Cap the
            // poll so `populated` is re-read at least every POLL_CAP; 500 µs is well under crun's ~0.4 ms
            // delete there and far below the observed lag. Correctness is unchanged: it still returns only
            // when `populated` is false, or when the deadline passes.
            const POLL_CAP: Duration = Duration::from_micros(500);
            let poll_for = left.min(POLL_CAP);
            let ts = rustix::event::Timespec {
                tv_sec: i64::try_from(poll_for.as_secs()).unwrap_or(i64::MAX),
                tv_nsec: i64::from(poll_for.subsec_nanos()),
            };
            let borrowed = fd.as_fd();
            let mut fds = [rustix::event::PollFd::new(
                &borrowed,
                rustix::event::PollFlags::PRI,
            )];
            match rustix::event::poll(&mut fds, Some(&ts)) {
                Ok(_) | Err(rustix::io::Errno::INTR) => {}
                Err(e) => return Err(os_err("waiting on cgroup.events", &path, e)),
            }
        }
    }

    /// Removes the box's cgroup: it must be empty. Absent already is removed.
    pub fn remove(&self) -> Result<(), Refusal> {
        match fs::remove_dir(&self.dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(os_err("removing a box's cgroup", &self.dir, e)),
        }
    }

    /// What the box has used, from its cgroup files; a file the kernel doesn't have reads as 0.
    pub fn usage(&self) -> Usage {
        let read = |f: &str| fs::read_to_string(self.dir.join(f)).unwrap_or_default();
        core::usage(&UsageFiles {
            memory_current: read("memory.current"),
            memory_peak: read("memory.peak"),
            memory_events: read("memory.events"),
            pids_current: read("pids.current"),
            pids_events: read("pids.events"),
            cpu_stat: read("cpu.stat"),
            io_stat: read("io.stat"),
        })
    }
}

/// Writes `value` to a cgroup file: one write(2), as the kernel takes a cgroup file's value.
/// Whether any swap is on: `/proc/swaps` lists one device per line past its header.
fn swap_active() -> Result<bool, Refusal> {
    let path = Path::new("/proc/swaps");
    let text =
        fs::read_to_string(path).map_err(|e| os_err("reading the active swap list", path, e))?;
    Ok(text.lines().skip(1).any(|l| !l.trim().is_empty()))
}

pub fn write_file(path: &Path, value: &str) -> Result<(), Refusal> {
    let fd = rustix::fs::open(path, OFlags::WRONLY | OFlags::CLOEXEC, Mode::empty())
        .map_err(|e| os_err("opening a cgroup file", path, e))?;
    let n = rustix::io::write(&fd, value.as_bytes()).map_err(|e| Refusal::Os {
        op: "writing a cgroup file",
        why: format!("{} = {value:?}: {e}", path.display()),
    })?;
    if n == value.len() {
        Ok(())
    } else {
        Err(Refusal::Os {
            op: "writing a cgroup file",
            why: format!("{}: wrote {n} of {} bytes", path.display(), value.len()),
        })
    }
}

/// The words of a cgroup file that lists controllers (`cgroup.controllers`, `cgroup.subtree_control`).
pub(crate) fn read_set(path: &Path) -> Result<BTreeSet<String>, Refusal> {
    let text =
        fs::read_to_string(path).map_err(|e| os_err("reading cgroup controllers", path, e))?;
    Ok(text.split_ascii_whitespace().map(str::to_owned).collect())
}
