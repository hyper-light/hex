//! A program started in a box, watched for its end. The node owns the member's process handle (from
//! `CreateProcessW`), so it waits on it directly (`WaitForSingleObject`, the fact, never a wall-clock
//! guess, CLAUDE.md §4) and reads its exit code. A member Hex ended by signalling its box's job reads
//! as that signal ([`hex_core::job::exit`] against the box's recorded signals); any other code reads as
//! itself.

use std::sync::Arc;
use std::time::{Duration, Instant};

use hex_api::{Exit, Proc, Refusal};

use super::process::State;
use super::sys::{self, Handle};

/// A program the node started in a box, by its process handle. The handle is dropped once the program
/// has ended and its exit is read, so the terminated process is reaped and leaves the box's job (a
/// terminated process a handle is still open on stays in the job's live process list, which the box
/// reads for whether a member runs).
pub struct BoxProc {
    process: Option<Handle>,
    /// The box it runs in, so its exit reads against the signals Hex ended the box's job for.
    state: Arc<State>,
    pid: u32,
    exit: Option<Exit>,
}

impl BoxProc {
    pub fn new(process: Handle, pid: u32, state: Arc<State>) -> Self {
        Self {
            process: Some(process),
            state,
            pid,
            exit: None,
        }
    }

    /// Reads the program's exit if it has ended within `timeout_ms` (`u32::MAX` waits for it), caching
    /// it and closing its handle; `Ok(None)` while it still runs.
    fn poll(&mut self, timeout_ms: u32) -> Result<Option<Exit>, Refusal> {
        if let Some(exit) = self.exit {
            return Ok(Some(exit));
        }
        let Some(process) = self.process.as_ref() else {
            return Ok(self.exit);
        };
        if !sys::wait(process, timeout_ms).map_err(|e| Refusal::os("waiting on a program", e))? {
            return Ok(None);
        }
        let code =
            sys::exit_code(process).map_err(|e| Refusal::os("reading a program's exit code", e))?;
        let exit = hex_core::job::exit(code, self.state.sent()?);
        self.exit = Some(exit);
        // Close the handle: the ended process is reaped and leaves the box's job.
        self.process = None;
        Ok(Some(exit))
    }
}

/// Milliseconds of `d` for a Windows wait, capped at `0xFFFF_FFFE` (`u32::MAX` is `INFINITE`, which a
/// finite timeout must never become).
fn millis(d: Duration) -> u32 {
    u32::try_from(d.as_millis())
        .unwrap_or(0xFFFF_FFFE)
        .min(0xFFFF_FFFE)
}

impl Proc for BoxProc {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn try_wait(&mut self) -> Result<Option<Exit>, Refusal> {
        self.poll(0)
    }

    fn wait_for(&mut self, timeout: Duration) -> Result<Option<Exit>, Refusal> {
        // One bounded wait on the process handle for the whole timeout: it is signalled the instant the
        // process ends, so this returns then, not at a polled interval.
        let deadline = Instant::now().checked_add(timeout);
        if self.poll(millis(timeout))?.is_some() {
            return Ok(self.exit);
        }
        // A spurious early return (none yet) is retried for the time left, so the bound holds.
        loop {
            let left = deadline.map_or(timeout, |d| d.saturating_duration_since(Instant::now()));
            if left.is_zero() {
                return Ok(None);
            }
            if self.poll(millis(left))?.is_some() {
                return Ok(self.exit);
            }
        }
    }
}
