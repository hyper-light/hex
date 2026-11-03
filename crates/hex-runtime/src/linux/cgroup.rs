//! Boxes as cgroup v2 directories under one root the node agent owns (docs/design/runtime.md, "The
//! process box"). The root's parent hands it the controllers boxes use, and the root hands them on
//! to every box; cgroup v2's no-internal-processes rule means neither may hold a process itself, so
//! the node agent runs beside the root, never in it.

use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::io::ErrorKind;
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hex_api::{Bounds, BoxId, Refusal, Signal, Usage};
use hex_core::cgroup::{self as core, Baseline, CONTROLLERS, Dev, UsageFiles};
use hex_core::reuse;
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
/// Most cgroups the kill sweep walks in one box's subtree before it refuses. A box is its own cgroup
/// and any the container nested under it — a handful; a tree with thousands is pathological, so this
/// bounds the walk (its pending queue included) well above any real box (CLAUDE.md §2). Past it the
/// sweep refuses ([`Refusal::Full`]) rather than stop silently (§3).
pub const MAX_SWEEP_CGROUPS: usize = 4_096;

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
        self.make(id.as_str(), bounds, || Refusal::Exists(id.clone()))
    }

    /// Makes slot `n`'s cgroup ([`reuse::name`]) with `bounds` in force: a cgroup that may be handed
    /// from box to box of one owner (goal 5 extended, step 6). Slots are numbered once each, so one that
    /// is there already is a fault, refused as one.
    pub fn create_slot(&self, n: u64, bounds: &Bounds) -> Result<BoxCgroup, Refusal> {
        let name = reuse::name(n);
        self.make(&name, bounds, || {
            Refusal::Broken(format!(
                "slot {name} is under {} already, but slots are numbered once each",
                self.dir.display()
            ))
        })
    }

    /// Makes the cgroup `name` with `bounds` in force; `exists` is the refusal for one already there.
    fn make(
        &self,
        name: &str,
        bounds: &Bounds,
        exists: impl FnOnce() -> Refusal,
    ) -> Result<BoxCgroup, Refusal> {
        self.controllers_for(bounds)?;
        let writes = core::writes(bounds, &self.disks)?;
        if self.count()? >= MAX_BOXES {
            return Err(Refusal::Full(MAX_BOXES));
        }
        let dir = self.dir.join(name);
        match fs::create_dir(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::AlreadyExists => return Err(exists()),
            Err(e) => return Err(os_err("making a box's cgroup", &dir, e)),
        }
        let cg = BoxCgroup { dir };
        for w in writes {
            if let Err(e) = cg.write_bound(&w) {
                // A box is made whole or not at all; its empty cgroup goes with the refusal.
                let _ = fs::remove_dir(&cg.dir);
                return Err(e);
            }
        }
        Ok(cg)
    }

    /// A bound whose controller the root doesn't have, refused by name.
    fn controllers_for(&self, bounds: &Bounds) -> Result<(), Refusal> {
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
        Ok(())
    }

    /// Writes every bound again into `cg`, a cgroup an earlier box left, so it holds `bounds` and
    /// nothing of the earlier box's ([`core::writes_all`]; goal 5 extended, step 6). None of these files'
    /// writes takes the kernel's `cgroup_mutex` (research f25). A default for a file the kernel doesn't
    /// have is skipped: no earlier box could have set it either. `cpu.max` alone is read first and
    /// written only where it holds another value: its write takes the host-wide `cfs_constraints_mutex`
    /// and walks every task group on the host (Linux v6.19 `tg_set_cfs_bandwidth`,
    /// kernel/sched/core.c:9461-9462, `__cfs_schedulable` :9624; research f26), 68.5 us p50 against 1.5 us
    /// for `pids.max` on adacachy's 3,348 cgroups (2026-11-03, 2,000 writes each), where its read takes
    /// no lock (`cpu_max_show`, :10104), so the bound is in force either way.
    pub fn rebound(&self, cg: &BoxCgroup, bounds: &Bounds) -> Result<(), Refusal> {
        self.controllers_for(bounds)?;
        for w in core::writes_all(bounds, &self.disks)? {
            let path = cg.dir.join(w.file);
            if core::is_default(&w) && !path.exists() {
                continue;
            }
            if w.file == "cpu.max" && read_text(&path).is_ok_and(|now| now.trim() == w.value) {
                continue;
            }
            cg.write_bound(&w)?;
        }
        Ok(())
    }

    /// Slot `n`'s cgroup, when it exists.
    pub fn slot(&self, n: u64) -> Option<BoxCgroup> {
        let dir = self.dir.join(reuse::name(n));
        if dir.is_dir() {
            Some(BoxCgroup { dir })
        } else {
            None
        }
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

    /// How many cgroups are under the root, box or not: the count [`BoxRoot::boxes`] would list, read from
    /// the root's link count instead. A cgroup v2 directory's link count is its child directories plus two
    /// (kernfs sets it so, `set_nlink(inode, kn->dir.subdirs + 2)` in `kernfs_refresh_inode`, Linux v6.19
    /// fs/kernfs/inode.c; measured on fedora: 0, 1, 7 and 32 children, links 2, 3, 9 and 34), and a `stat`
    /// of it takes only kernfs's attribute lock, shared (`kernfs_iop_getattr`). A listing takes the root
    /// directory's own lock shared, and every box's `mkdir` and `rmdir` beside it take that lock
    /// exclusive, so at concurrency 32 a create waited behind them all: 5.7 ms of each lifecycle off the
    /// CPU, the largest wait a create had (goal 5 plan step 4, bench/competitive/probes/create_serial.md).
    ///
    /// A directory whose links don't count its children (fewer than two: btrfs, for one, gives every
    /// directory one) is listed instead, so the bound holds on any filesystem a root is opened on.
    pub fn count(&self) -> Result<usize, Refusal> {
        use std::os::unix::fs::MetadataExt;
        let links = fs::metadata(&self.dir)
            .map_err(|e| os_err("counting boxes", &self.dir, e))?
            .nlink();
        if links < 2 {
            return Ok(self.boxes()?.len());
        }
        Ok(usize::try_from(links.saturating_sub(2)).unwrap_or(usize::MAX))
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

    /// Kills every process in the box. `cgroup.kill` (Linux 5.14) kills the whole subtree in one write,
    /// but signals thread-group *leaders*, so a leaderless group — its leader thread exited while a
    /// non-leader thread lives — survives it and holds the box populated (proven on Linux 7.2.9 and
    /// 6.19, `bench/competitive/probes/delete_flake.md`). So if the box is still populated after it,
    /// sweep its `cgroup.procs` under a bound ([`sweep_targets`]) and send each process a
    /// *process-directed* SIGKILL through a process pidfd ([`sigkill_box_process`]): the kernel delivers
    /// it to the whole thread group (`PIDTYPE_TGID`), reaching a leaderless group's live worker the
    /// leader-directed `cgroup.kill` missed (probe `delete_flake_point4.c`). [`BoxCgroup::wait_empty`]
    /// then sees it empty.
    pub fn kill(&self) -> Result<(), Refusal> {
        write_file(&self.dir.join("cgroup.kill"), "1")?;
        if !self.populated()? {
            return Ok(());
        }
        for (dir, pid) in sweep_targets(&self.dir, &FsTree, MAX_SWEEP_CGROUPS, MAX_SIGNALLED)? {
            sigkill_box_process(&dir, pid)?;
        }
        Ok(())
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

    /// Writes one bound. A kernel without swap accounting has no memory.swap.max: with no swap on, the
    /// memory bound holds without it; with swap on it cannot hold, so the box is refused by name.
    fn write_bound(&self, w: &core::Write) -> Result<(), Refusal> {
        if w.file == "memory.swap.max" && !self.dir.join(w.file).exists() {
            return match swap_active()? {
                false => Ok(()),
                true => Err(Refusal::Platform {
                    what: "a memory bound",
                    platform: "linux",
                    why: "swap is on but this kernel has no swap accounting (no memory.swap.max), so a box could swap past its memory bound".to_owned(),
                }),
            };
        }
        write_file(&self.dir.join(w.file), &w.value)
    }

    /// What [`reuse::check`] reads of this cgroup before it is handed on: files whose reads take no
    /// lock (research f61). The memory files read as `None` where the memory controller isn't handed
    /// down, so the kernel has none.
    pub fn idle_files(&self) -> Result<reuse::Files, Refusal> {
        let read = |file: &str| read_text(&self.dir.join(file));
        let maybe = |file: &str| match read_text(&self.dir.join(file)) {
            Ok(text) => Ok(Some(text)),
            Err(Refusal::Os { .. }) if !self.dir.join(file).exists() => Ok(None),
            Err(e) => Err(e),
        };
        Ok(reuse::Files {
            events: read("cgroup.events")?,
            stat: read("cgroup.stat")?,
            subtree_control: read("cgroup.subtree_control")?,
            kind: read("cgroup.type")?,
            freeze: read("cgroup.freeze")?,
            oom_group: maybe("memory.oom.group")?,
            memory_current: maybe("memory.current")?,
        })
    }

    /// Its cumulative counters now, which a box given this cgroup has its usage reported from
    /// ([`core::since`]); a file the kernel doesn't have reads as 0, as [`BoxCgroup::usage`] reads it.
    pub fn baseline(&self) -> Baseline {
        let read = |f: &str| read_text(&self.dir.join(f)).unwrap_or_default();
        Baseline::of(&core::usage(&UsageFiles {
            memory_events: read("memory.events"),
            pids_events: read("pids.events"),
            cpu_stat: read("cpu.stat"),
            io_stat: read("io.stat"),
            ..UsageFiles::default()
        }))
    }

    /// Its `memory.peak`, opened and written, so reads through the descriptor say the peak from now on
    /// alone, never the cgroup's lifetime peak (Linux 6.12 on: "A write of any non-empty string to this
    /// file resets it to the current memory usage for subsequent reads through the same file
    /// descriptor", Documentation/admin-guide/cgroup-v2.rst; mm/memcontrol.c `peak_write`, v6.19 :4283;
    /// research f31). `None` where the memory controller isn't handed down; refused where the kernel
    /// can't reset it, so the cgroup isn't handed on with its predecessors' peak.
    pub fn peak_from_now(&self) -> Result<Option<std::os::fd::OwnedFd>, Refusal> {
        let path = self.dir.join("memory.peak");
        if !path.exists() {
            return Ok(None);
        }
        let fd = rustix::fs::open(&path, OFlags::RDWR | OFlags::CLOEXEC, Mode::empty())
            .map_err(|e| os_err("opening a cgroup's memory.peak to reset it", &path, e))?;
        rustix::io::write(&fd, b"reset")
            .map_err(|e| os_err("resetting a cgroup's memory.peak", &path, e))?;
        Ok(Some(fd))
    }

    /// What usage the box given this cgroup at `base` has had: [`core::since`] over its files now, its
    /// peak read through `peak`, the descriptor [`BoxCgroup::peak_from_now`] wrote ([`BoxCgroup::usage`]'s
    /// lifetime peak where there is none).
    pub fn usage_since(&self, base: &Baseline, peak: Option<&std::os::fd::OwnedFd>) -> Usage {
        let now = self.usage();
        let peak = peak
            .and_then(|fd| {
                let mut buf = [0u8; 32];
                let n = rustix::io::pread(fd, &mut buf, 0).ok()?;
                std::str::from_utf8(buf.get(..n)?).ok()?.trim().parse().ok()
            })
            .unwrap_or(now.memory_peak_bytes);
        core::since(&now, base, peak)
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

/// Largest cgroup file Hex reads whole: `io.stat` and `cpu.stat` hold a few lines each; a file past
/// this is damage, refused by type rather than read on.
const MAX_CGROUP_FILE: usize = 64 << 10;

/// A cgroup file's text, read with one open and as few reads as it takes, no stat (`fs::read_to_string`
/// takes one per file to size its buffer): an idle cgroup's check and baseline read a dozen on create's
/// path. Refused past [`MAX_CGROUP_FILE`].
fn read_text(path: &Path) -> Result<String, Refusal> {
    let fd = rustix::fs::open(path, OFlags::RDONLY | OFlags::CLOEXEC, Mode::empty())
        .map_err(|e| os_err("reading a cgroup file", path, e))?;
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = rustix::io::read(&fd, &mut buf)
            .map_err(|e| os_err("reading a cgroup file", path, e))?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(buf.get(..n).unwrap_or_default());
        if out.len() > MAX_CGROUP_FILE {
            return Err(Refusal::Os {
                op: "reading a cgroup file",
                why: format!("{} is over {MAX_CGROUP_FILE} bytes", path.display()),
            });
        }
    }
    String::from_utf8(out).map_err(|e| Refusal::Os {
        op: "reading a cgroup file",
        why: format!("{} isn't UTF-8: {e}", path.display()),
    })
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

/// Sends process `raw`, found in the box's cgroup `dir`, a process-directed SIGKILL — but only through
/// a process pidfd that pins it, and only once `dir`'s `cgroup.procs` is re-read and still lists it, so
/// an id that named one of the box's processes when the sweep read it but has since exited (and may now
/// name an unrelated process) is not signalled. The pidfd pins the identity after acquisition; the
/// re-read is the membership-and-lifetime check that closes the exit/reuse race ([`BoxCgroup::kill`]).
/// A process gone since the sweep (`ESRCH`, or no longer listed) is already as dead as the signal would
/// make it. No freeze is taken, so a post-kill zombie (an exited init awaiting reap) is handled, not
/// waited on.
fn sigkill_box_process(dir: &Path, raw: u32) -> Result<(), Refusal> {
    let Some(pid) = i32::try_from(raw)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    else {
        return Ok(());
    };
    // A process pidfd (no PIDFD_THREAD), so pidfd_send_signal is process-directed (PIDTYPE_TGID) and
    // reaches a leaderless group's live worker the leader-directed cgroup.kill missed (point4 probe).
    let pidfd = match rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty()) {
        Ok(fd) => fd,
        Err(rustix::io::Errno::SRCH) => return Ok(()),
        Err(e) => {
            return Err(Refusal::Os {
                op: "opening a box process's pidfd",
                why: format!("pid {raw}: {e}"),
            });
        }
    };
    if !cgroup_lists_pid(dir, raw)? {
        return Ok(());
    }
    match rustix::process::pidfd_send_signal(&pidfd, rustix::process::Signal::KILL) {
        Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
        Err(e) => Err(Refusal::Os {
            op: "SIGKILL a box process",
            why: format!("pid {raw}: {e}"),
        }),
    }
}

/// Whether `dir`'s `cgroup.procs` still lists process `raw`; a cgroup gone reads as not listing it.
fn cgroup_lists_pid(dir: &Path, raw: u32) -> Result<bool, Refusal> {
    let path = dir.join("cgroup.procs");
    match fs::read_to_string(&path) {
        Ok(text) => Ok(text
            .lines()
            .filter_map(|l| l.trim().parse::<u32>().ok())
            .any(|p| p == raw)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(os_err("re-reading cgroup.procs", &path, e)),
    }
}

/// A box's cgroup subtree as [`sweep_targets`] reads it, behind a seam so the sweep's bounds and typed
/// refusals are provable without a cgroupfs (CLAUDE.md §9). A cgroup gone since the walk began is a
/// benign race — `Ok(None)`, not an error; a real read failure is returned.
trait CgroupTree {
    /// The child cgroup directories of `dir`; `None` if `dir` has vanished.
    fn children(&self, dir: &Path) -> Result<Option<Vec<PathBuf>>, Refusal>;
    /// The process ids in `dir`'s `cgroup.procs`; `None` if the cgroup has vanished.
    fn procs(&self, dir: &Path) -> Result<Option<Vec<u32>>, Refusal>;
}

/// Every process in the box's cgroup subtree rooted at `root`, each with the cgroup it sits in, so
/// [`BoxCgroup::kill`] can signal it. Breadth-first over `tree`, bounded: at most `max_cgroups` cgroups
/// visited — the pending queue never holds more than that either — and at most `max_procs` processes
/// collected; past either it refuses ([`Refusal::Full`]) rather than silently stop (CLAUDE.md §2, §3). A
/// cgroup gone mid-walk is skipped (a benign race); any other read failure is returned. Pure over
/// `tree`, so a mock proves its bounds and its typed refusals.
fn sweep_targets(
    root: &Path,
    tree: &impl CgroupTree,
    max_cgroups: usize,
    max_procs: usize,
) -> Result<Vec<(PathBuf, u32)>, Refusal> {
    let mut pending: VecDeque<PathBuf> = VecDeque::new();
    pending.push_back(root.to_path_buf());
    let mut visited = 0usize;
    let mut targets: Vec<(PathBuf, u32)> = Vec::new();
    while let Some(dir) = pending.pop_front() {
        visited = visited.checked_add(1).ok_or(Refusal::Full(max_cgroups))?;
        if visited > max_cgroups {
            return Err(Refusal::Full(max_cgroups));
        }
        if let Some(children) = tree.children(&dir)? {
            for child in children {
                if pending.len() >= max_cgroups {
                    return Err(Refusal::Full(max_cgroups));
                }
                pending.push_back(child);
            }
        }
        if let Some(procs) = tree.procs(&dir)? {
            for pid in procs {
                if targets.len() >= max_procs {
                    return Err(Refusal::Full(max_procs));
                }
                targets.push((dir.clone(), pid));
            }
        }
    }
    Ok(targets)
}

/// The real cgroupfs for [`sweep_targets`].
struct FsTree;

impl CgroupTree for FsTree {
    fn children(&self, dir: &Path) -> Result<Option<Vec<PathBuf>>, Refusal> {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(os_err("listing a box's cgroups", dir, e)),
        };
        let mut dirs = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) if e.kind() == ErrorKind::NotFound => continue,
                Err(e) => return Err(os_err("listing a box's cgroups", dir, e)),
            };
            match entry.file_type() {
                Ok(t) if t.is_dir() => dirs.push(entry.path()),
                Ok(_) => {}
                Err(e) if e.kind() == ErrorKind::NotFound => {}
                Err(e) => return Err(os_err("a box cgroup entry's type", &entry.path(), e)),
            }
        }
        Ok(Some(dirs))
    }

    fn procs(&self, dir: &Path) -> Result<Option<Vec<u32>>, Refusal> {
        let path = dir.join("cgroup.procs");
        match fs::read_to_string(&path) {
            Ok(text) => Ok(Some(
                text.lines().filter_map(|l| l.trim().parse().ok()).collect(),
            )),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(os_err("reading cgroup.procs", &path, e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    /// A cgroup tree from in-memory maps: a dir absent from both maps has vanished (its reads return
    /// `None`); a dir in `io_error_at` fails with a non-vanishing read error.
    struct Mock {
        children: BTreeMap<PathBuf, Vec<PathBuf>>,
        procs: BTreeMap<PathBuf, Vec<u32>>,
        io_error_at: Option<PathBuf>,
    }
    impl CgroupTree for Mock {
        fn children(&self, dir: &Path) -> Result<Option<Vec<PathBuf>>, Refusal> {
            if self.io_error_at.as_deref() == Some(dir) {
                return Err(Refusal::Os {
                    op: "listing a box's cgroups",
                    why: "a non-vanishing read failure".to_owned(),
                });
            }
            Ok(self.children.get(dir).cloned())
        }
        fn procs(&self, dir: &Path) -> Result<Option<Vec<u32>>, Refusal> {
            Ok(self.procs.get(dir).cloned())
        }
    }

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn sweep_collects_every_process_in_the_subtree() {
        let tree = Mock {
            children: BTreeMap::from([(p("/b"), vec![p("/b/a"), p("/b/c")])]),
            procs: BTreeMap::from([(p("/b"), vec![10]), (p("/b/a"), vec![20, 21])]),
            io_error_at: None,
        };
        let got = sweep_targets(&p("/b"), &tree, 16, 16).unwrap();
        let pids: BTreeSet<u32> = got.iter().map(|(_, pid)| *pid).collect();
        assert_eq!(pids, BTreeSet::from([10, 20, 21]));
        assert!(
            got.contains(&(p("/b/a"), 20)),
            "a child's process carries its cgroup"
        );
    }

    #[test]
    fn sweep_refuses_past_the_cgroup_budget() {
        let tree = Mock {
            children: BTreeMap::from([(p("/b"), vec![p("/b/1"), p("/b/2"), p("/b/3"), p("/b/4")])]),
            procs: BTreeMap::new(),
            io_error_at: None,
        };
        assert_eq!(sweep_targets(&p("/b"), &tree, 3, 16), Err(Refusal::Full(3)));
    }

    #[test]
    fn sweep_refuses_past_the_process_budget() {
        let tree = Mock {
            children: BTreeMap::new(),
            procs: BTreeMap::from([(p("/b"), vec![1, 2, 3, 4])]),
            io_error_at: None,
        };
        assert_eq!(sweep_targets(&p("/b"), &tree, 16, 2), Err(Refusal::Full(2)));
    }

    #[test]
    fn sweep_returns_a_non_vanishing_io_failure() {
        let tree = Mock {
            children: BTreeMap::new(),
            procs: BTreeMap::new(),
            io_error_at: Some(p("/b")),
        };
        let err = sweep_targets(&p("/b"), &tree, 16, 16).unwrap_err();
        assert!(
            matches!(
                err,
                Refusal::Os {
                    op: "listing a box's cgroups",
                    ..
                }
            ),
            "got {err:?}"
        );
    }

    #[test]
    fn sweep_skips_a_vanished_cgroup() {
        // /b names a child /b/gone that is absent from the maps (vanished): it is skipped, not an error.
        let tree = Mock {
            children: BTreeMap::from([(p("/b"), vec![p("/b/gone")])]),
            procs: BTreeMap::from([(p("/b"), vec![7])]),
            io_error_at: None,
        };
        let got = sweep_targets(&p("/b"), &tree, 16, 16).unwrap();
        assert_eq!(got, vec![(p("/b"), 7)]);
    }
}
