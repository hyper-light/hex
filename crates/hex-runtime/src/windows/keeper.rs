//! The keeper: a passive process a node starts to hold its boxes' job handles, so a box outlives the node
//! (native plan step 8, the decision "the Windows node, on one completion port", its recovery part; the
//! lead's (A), 2026-11-08). Each box's job stays kill-on-close; the node and the keeper each hold a handle
//! to it, so:
//! - the node crashes: the keeper's handle keeps the job and its name, and the next node takes the box
//!   back by name;
//! - the keeper dies while the node lives: the node sees its end and starts another, duplicating the
//!   handles again;
//! - both are gone: the last handle closes, and kill-on-close ends the boxes, so nothing is left running
//!   in a job no one can open.
//!
//! The keeper does nothing itself: the node duplicates job handles into it (`sys::duplicate_into`) and
//! closes them there on delete (`sys::close_in`). It waits until its node ends or it is told to stop, then
//! holds the handles for a bound time for the next node to take them, and exits. It is `hex keep`, started
//! by the node ([`super::WinRuntime`]); this module is the keeper's body and the node's handle on it.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hex_api::Refusal;

use super::sys::{self, Handle};

/// How long a keeper whose node has ended holds its boxes' jobs for the next node to adopt it, before it
/// exits and kill-on-close ends them: five times the node's own longest wait (a delete's 60 s), so only a
/// node that never restarts lets it lapse, as on Linux a crashed node's boxes wait on the service to
/// restart it.
const HOLD_AFTER_NODE: Duration = Duration::from_secs(300);

/// What `hex keep` is given: the folder its node keeps its state in (where the node writes which node pid
/// owns the name, [`node_file`]), and the path it writes its own identity to once ready.
pub struct Args {
    pub state: PathBuf,
    pub ready: PathBuf,
}

impl Args {
    /// Parses `hex keep`'s arguments: `--state <dir> --ready <path>`.
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let (mut state, mut ready) = (None, None);
        let mut rest = args.iter();
        while let Some(flag) = rest.next() {
            let value = rest.next().ok_or_else(|| format!("{flag} needs a value"))?;
            match flag.as_str() {
                "--state" => state = Some(PathBuf::from(value)),
                "--ready" => ready = Some(PathBuf::from(value)),
                other => return Err(format!("unknown argument {other}")),
            }
        }
        let env_path = |flag: &str, var: &str| {
            std::env::var_os(var)
                .map(PathBuf::from)
                .ok_or_else(|| format!("{flag} is needed"))
        };
        Ok(Self {
            state: state.map_or_else(|| env_path("--state", "HEX_KEEP_STATE"), Ok)?,
            ready: ready.map_or_else(|| env_path("--ready", "HEX_KEEP_READY"), Ok)?,
        })
    }
}

/// The file a node writes to say it owns the name now: its pid and creation time. The keeper polls it; a
/// node absent or ended for [`HOLD_AFTER_NODE`] lets the keeper exit, and kill-on-close ends the boxes.
pub fn node_file(state: &Path) -> PathBuf {
    state.join("node.owner")
}

/// How often the keeper polls for a live node: a second, finer than the hold, coarse enough to cost
/// nothing. A bounded loop (CLAUDE.md §2): each tick does one liveness check.
const POLL: Duration = Duration::from_secs(1);

/// Runs the keeper: it holds whatever the node duplicates into it, and polls [`node_file`]; while a live
/// node owns the name it waits, and once none has for [`HOLD_AFTER_NODE`] it exits (the stop is immediate).
/// It never touches the handles itself.
pub fn run(args: &Args) -> Result<(), Refusal> {
    let stop = sys::named_event(&stop_name(&args.state))
        .map_err(|e| Refusal::os("making the keeper's stop", e))?;
    let me = sys::open_process(std::process::id())
        .map_err(|e| Refusal::os("reading the keeper's own identity", e))?
        .ok_or_else(|| Refusal::Broken("the keeper can't open itself".to_owned()))?;
    let created =
        sys::created(&me).map_err(|e| Refusal::os("reading the keeper's start time", e))?;
    write_ready(&args.ready, std::process::id(), created)
        .map_err(|e| Refusal::os("saying the keeper is ready", e))?;
    let mut absent_since: Option<Instant> = None;
    loop {
        // Stop set: the node is going down cleanly or replacing this keeper.
        if sys::wait(&stop, 0).map_err(|e| Refusal::os("reading the keeper's stop", e))? {
            return Ok(());
        }
        if node_alive(&args.state) {
            absent_since = None;
        } else {
            let since = *absent_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= HOLD_AFTER_NODE {
                return Ok(());
            }
        }
        // Wait a tick, or wake at once if the stop is set meanwhile.
        let _ = sys::wait(&stop, millis(POLL));
    }
}

fn millis(d: Duration) -> u32 {
    u32::try_from(d.as_millis()).unwrap_or(u32::MAX)
}

/// The keeper's stop event's name, one per state folder (`Local\hex-keep-<hash of path>-stop`): the node
/// and its keeper reach the same event by the folder they share.
fn stop_name(state: &Path) -> Vec<u16> {
    let mut h: u64 = 1469598103934665603;
    for b in state.to_string_lossy().bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(1099511628211);
    }
    format!("Local\\hex-keep-{h:016x}-stop\0")
        .encode_utf16()
        .collect()
}

fn write_ready(path: &Path, pid: u32, created: u64) -> io::Result<()> {
    std::fs::write(path, format!("{pid} {created}"))
}

/// Whether a live node owns the name (its pid and creation time in [`node_file`] name a running process).
fn node_alive(state: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(node_file(state)) else {
        return false;
    };
    let mut parts = text.split_whitespace();
    let (Some(pid), Some(created)) = (
        parts.next().and_then(|p| p.parse::<u32>().ok()),
        parts.next().and_then(|c| c.parse::<u64>().ok()),
    ) else {
        return false;
    };
    matches!(sys::open_process(pid), Ok(Some(p)) if sys::created(&p).ok() == Some(created))
}

/// Writes which node owns the name now, for the keeper to see ([`node_alive`]).
pub fn write_node(state: &Path, pid: u32, created: u64) -> io::Result<()> {
    std::fs::write(node_file(state), format!("{pid} {created}"))
}

/// The node's handle on the keeper it started: its process, for watching and duplicating handles into, and
/// the stop it signals.
pub struct NodeKeeper {
    process: Handle,
    pid: u32,
    created: u64,
    stop: Handle,
}

impl NodeKeeper {
    /// The keeper process `pid` (its creation time `created`), opened for watching and duplicating into;
    /// its stop is the one named for `state`.
    pub fn open(pid: u32, created: u64, state: &Path) -> Result<Self, Refusal> {
        let process = sys::open_process(pid)
            .map_err(|e| Refusal::os("opening the node's keeper", e))?
            .filter(|p| sys::created(p).ok() == Some(created))
            .ok_or_else(|| Refusal::Invalid {
                what: "keeper",
                why: format!("keeper pid {pid} is gone or reused"),
            })?;
        let stop = sys::named_event(&stop_name(state))
            .map_err(|e| Refusal::os("opening the keeper's stop", e))?;
        Ok(Self {
            process,
            pid,
            created,
            stop,
        })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn created(&self) -> u64 {
        self.created
    }

    /// Whether the keeper still runs (its handle unsignalled).
    pub fn alive(&self) -> bool {
        sys::wait(&self.process, 0)
            .map(|ended| !ended)
            .unwrap_or(false)
    }

    /// The keeper's process handle, to watch on the node's port for its end.
    pub fn process(&self) -> &Handle {
        &self.process
    }

    /// Duplicates the handle value `job` (a box's job, this process's) into the keeper, returning its
    /// value there.
    pub fn hold(&self, job: usize) -> Result<usize, Refusal> {
        sys::duplicate_into(job, &self.process)
            .map_err(|e| Refusal::os("handing a box's job to the keeper", e))
    }

    /// Closes the handle `value` in the keeper (a deleted box's job).
    pub fn drop_held(&self, value: usize) -> Result<(), Refusal> {
        sys::close_in(&self.process, value)
            .map_err(|e| Refusal::os("closing a box's job in the keeper", e))
    }

    /// Tells the keeper to stop and end (the node is going down cleanly, or replacing it).
    pub fn stop(&self) {
        let _ = sys::set(&self.stop);
    }
}

/// The file naming the keeper holding a node's boxes, under the state folder: its pid and creation time, so
/// a node after a restart finds the same keeper (or starts a new one where it is gone).
pub fn keeper_file(state: &Path) -> PathBuf {
    state.join("keeper.owner")
}

/// The file holding the handle value of box `id`'s job inside the keeper, so a delete or a restart can
/// close or re-duplicate it.
pub fn held_file(state: &Path, id: &str) -> PathBuf {
    state.join(format!("held-{id}"))
}

/// Writes box `id`'s keeper handle value (`held_file`).
pub fn write_held(state: &Path, id: &str, value: usize) -> io::Result<()> {
    std::fs::write(held_file(state, id), value.to_string())
}

/// Box `id`'s keeper handle value, if recorded.
pub fn read_held(state: &Path, id: &str) -> Option<usize> {
    std::fs::read_to_string(held_file(state, id))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Forgets box `id`'s keeper handle value (its job closed in the keeper).
pub fn forget_held(state: &Path, id: &str) {
    let _ = std::fs::remove_file(held_file(state, id));
}
