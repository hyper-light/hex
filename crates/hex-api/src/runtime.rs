use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{Bounds, BoxId, BoxSpec, Class, Program, Stdio};

/// A runtime: boxes of one or more classes on one node. Calls are synchronous and short (a create
/// takes milliseconds); waiting for a program is [`Proc`]'s, with a deadline. Every refusal is
/// typed and says what and why (CLAUDE.md §3).
pub trait Runtime: Send + Sync {
    /// The classes it runs.
    fn classes(&self) -> Vec<Class>;

    /// Makes a box with its bounds in force. A `process` box is empty when this returns; an `oci`
    /// box's init is made and waits for [`Runtime::start`], [`BoxState::Created`].
    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal>;

    /// Lets a created `oci` box's init run its program: the runtime-spec's `start`, which `create`
    /// leaves to a later call so a box can be made before it's needed. A `process` box has no init to
    /// start, and an `oci` box that has started already can't start again: each is refused.
    fn start(&self, id: &BoxId) -> Result<(), Refusal>;

    /// Runs `program` in box `id` with `stdio`: a host process in a `process` box, a process in the
    /// container's namespaces in an `oci` box.
    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal>;

    /// Sends `signal` to every process in the box.
    fn signal(&self, id: &BoxId, signal: Signal) -> Result<(), Refusal>;

    /// The box as it is now.
    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal>;

    /// What the box has used and holds now.
    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal>;

    /// Every box on the node.
    fn list(&self) -> Result<Vec<BoxInfo>, Refusal>;

    /// Ends every process in the box and removes it, with any writable layer it owns.
    fn delete(&self, id: &BoxId) -> Result<(), Refusal>;
}

/// A program started in a box.
pub trait Proc: Send {
    /// Its process id on the node (0 in a simulator).
    fn pid(&self) -> u32;

    /// Its end if it has ended; never blocks.
    fn try_wait(&mut self) -> Result<Option<Exit>, Refusal>;

    /// Waits at most `timeout` for its end; `Ok(None)` when it is still running.
    fn wait_for(&mut self, timeout: Duration) -> Result<Option<Exit>, Refusal>;
}

/// How a program ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Exit {
    /// It exited with this status.
    Code(i32),
    /// This signal ended it.
    Signal(i32),
    /// It ended where its node agent couldn't collect how: while the agent was down, or (an `oci`
    /// box's init) after a restart, when the init is no longer the agent's child.
    Unknown,
}

/// A signal to send. Its number is the Linux one; Windows holds only KILL and TERM, both ending
/// the box's job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Signal(u8);

impl Signal {
    pub const HUP: Self = Self(1);
    pub const INT: Self = Self(2);
    pub const QUIT: Self = Self(3);
    pub const KILL: Self = Self(9);
    pub const USR1: Self = Self(10);
    pub const USR2: Self = Self(12);
    pub const TERM: Self = Self(15);

    /// Signal number `n`, 1 to 64.
    pub fn new(n: i64) -> Result<Self, Refusal> {
        u8::try_from(n)
            .ok()
            .filter(|n| (1..=64).contains(n))
            .map(Self)
            .ok_or(Refusal::Invalid {
                what: "signal",
                why: format!("{n} is outside 1..=64"),
            })
    }

    pub fn number(self) -> i32 {
        i32::from(self.0)
    }
}

/// Whether anything runs in a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BoxState {
    /// Nothing runs in it.
    Empty,
    /// At least one process runs in it.
    Running,
    /// An `oci` box whose init waits for [`Runtime::start`].
    Created,
}

/// A box as its runtime holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoxInfo {
    pub id: BoxId,
    pub class: Class,
    pub state: BoxState,
    pub bounds: Bounds,
    /// Taken back after its node agent restarted, rather than made by this one.
    pub adopted: bool,
    /// How an `oci` box's init ended, once it has: the container's own exit. `None` while it runs,
    /// and for a `process` box, which has no init. Added after the first release, so absent on the
    /// wire means `None` (append-only, CLAUDE.md).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<Exit>,
}

/// What a box has used, and holds now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub memory_bytes: u64,
    pub memory_peak_bytes: u64,
    /// Processes the kernel killed in the box for its memory bound.
    pub oom_kills: u64,
    pub pids: u64,
    /// Forks the kernel refused the box for its process bound.
    pub pids_refused: u64,
    pub cpu_usec: u64,
    pub io_read_bytes: u64,
    pub io_write_bytes: u64,
}

/// Why a request was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("{what} isn't valid: {why}")]
    Invalid { what: &'static str, why: String },
    #[error("no box {0}")]
    NoSuchBox(BoxId),
    #[error("box {0} exists already")]
    Exists(BoxId),
    #[error("box {0} has nothing running to {1}")]
    NotRunning(BoxId, &'static str),
    #[error("{what} isn't available on {platform}: {why}")]
    Platform {
        what: &'static str,
        platform: &'static str,
        why: String,
    },
    #[error("the node holds its most boxes ({0})")]
    Full(usize),
    /// A per-node budget other than the box count is spent: `what` names it, `bound` is its size.
    #[error("the node holds its most {what} ({bound})")]
    Exhausted { what: &'static str, bound: usize },
    #[error("{op} failed: {why}")]
    Os { op: &'static str, why: String },
    #[error("the node agent's state is unusable: {0}")]
    Broken(String),
}

impl Refusal {
    /// An operating system call that failed, with what was being done.
    pub fn os(op: &'static str, err: impl std::fmt::Display) -> Self {
        Self::Os {
            op,
            why: err.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signal_number_outside_the_kernels_range_is_refused() {
        assert!(Signal::new(0).is_err());
        assert!(Signal::new(65).is_err());
        assert!(Signal::new(-9).is_err());
        assert_eq!(Signal::new(9).unwrap(), Signal::KILL);
        assert_eq!(Signal::new(64).unwrap().number(), 64);
    }

    /// Append-only wire (CLAUDE.md): a `BoxInfo` written before `exit` existed reads as `None`, and
    /// one with no exit is written without the field, so an older reader sees what it always did.
    #[test]
    fn a_box_info_without_an_exit_reads_and_writes_as_before() {
        let old = r#"{"id":"b","class":"oci","state":"empty","bounds":{},"adopted":false}"#;
        let info: BoxInfo = serde_json::from_str(old).unwrap();
        assert_eq!(info.exit, None);
        assert!(!serde_json::to_string(&info).unwrap().contains("exit"));
        let ended = BoxInfo {
            exit: Some(Exit::Signal(9)),
            ..info
        };
        let back: BoxInfo = serde_json::from_str(&serde_json::to_string(&ended).unwrap()).unwrap();
        assert_eq!(back.exit, Some(Exit::Signal(9)));
    }
}
