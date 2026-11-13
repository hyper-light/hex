//! Recorded agent traces (joint plan step 14, item 3; `bench/charge/record_trace.py`): a real harness session's load as
//! `millis threads mib` steps, so many milliseconds of so many busy threads holding so many MiB, `#` comments and blank
//! lines aside, its header naming where its memory came from. Original (red): hex-sim's runs replay none, their VMs'
//! use drawn (`sim::Agent::draw`).

use super::ChargeError;

/// One step of a recorded trace: `millis` milliseconds of `threads` busy threads holding `mib` MiB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub millis: u64,
    pub threads: u64,
    pub mib: u64,
}

/// Where a trace's memory came from, as its header names it (`# memory: ...`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Memory {
    /// Its cgroup's memory controller's count (`memory.current`): its page cache and kernel memory too.
    Cgroup,
    /// Its processes' resident sets (`procs-rss`): their own pages alone.
    ProcsRss,
}

/// A recorded trace: its steps, in order, and where its memory came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trace {
    pub memory: Memory,
    pub steps: Vec<Step>,
}

/// A recorded trace, from its text. Original (red): refused, typed: the sim reads no trace.
pub fn parse(text: &str) -> Result<Trace, ChargeError> {
    let _ = text;
    Err(ChargeError::Invalid {
        what: "a recorded trace",
        why: "the sim replays no trace: its VMs' use is drawn",
    })
}
