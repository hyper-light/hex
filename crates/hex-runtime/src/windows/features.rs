//! Which of the Windows features Hex's installer turns on are off, read without elevation (native plan
//! step 6; docs/research/runtime/installer-per-os.md §2.4): PowerShell's `Get-CimInstance` reads
//! `Win32_OptionalFeature` ([`setup::FEATURE_QUERY`]), and [`setup::features_off`] parses its listing.
//! The query runs as a member of a box of its own, so its wait is the class's bounded one and past it
//! the box's end takes everything it started. The `oci` class's refusal names each feature that's off
//! ([`setup::windows_oci`]).

use std::collections::BTreeSet;
use std::io::Read;
use std::num::NonZeroU32;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use hex_api::{Bounds, BoxId, Proc, Program, Refusal, Stdio};
use hex_core::setup::{self, FEATURE_LISTING_MAX, WindowsFeature};

use super::process::JobBox;
use super::sys::Started;
use super::{build_spawn, proc};

/// The longest the query may take before its box is ended and the read refused by name: twice its slowest
/// recorded run ([`setup::FEATURE_QUERY_PATIENCE`]), a bound on a wait (CLAUDE.md §2).
const PATIENCE: Duration = setup::FEATURE_QUERY_PATIENCE;

/// The environment PowerShell and WMI read, taken from the node's own where it has them: the box gives
/// a program nothing it isn't given.
const ENVIRONMENT: [&str; 5] = ["SystemRoot", "windir", "TEMP", "TMP", "PATH"];

/// The installer's features that are off on this machine of `processors` logical processors.
pub fn off(processors: NonZeroU32) -> Result<BTreeSet<WindowsFeature>, Refusal> {
    setup::features_off(&listing(processors, &powershell(setup::FEATURE_QUERY)?)?)
}

/// Windows PowerShell running `command`, with the environment it and WMI read ([`ENVIRONMENT`]).
fn powershell(command: &str) -> Result<Program, Refusal> {
    let root = std::env::var("SystemRoot").map_err(|e| Refusal::Os {
        op: "finding PowerShell to read the Windows features",
        why: format!("SystemRoot: {e}"),
    })?;
    let powershell = format!(r"{root}\System32\WindowsPowerShell\v1.0\powershell.exe");
    let mut program = Program::of(&[
        &powershell,
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        command,
    ]);
    program.env = ENVIRONMENT
        .iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| ((*k).to_owned(), v)))
        .collect();
    Ok(program)
}

/// What `program` writes, run as the member of a job-object box of its own within [`PATIENCE`]: past it
/// the box ends and the read is refused by name.
fn listing(processors: NonZeroU32, program: &Program) -> Result<String, Refusal> {
    let deadline = Instant::now().checked_add(PATIENCE);
    let id = BoxId::new(&format!("hex-features-{}", std::process::id()))?;
    let b = JobBox::create(id, &Bounds::default(), processors)?;
    let (out, w) = std::io::pipe().map_err(|e| Refusal::os("making a pipe", e))?;
    let spawn = build_spawn(
        program,
        Stdio {
            stdout: Some(w),
            ..Stdio::null()
        },
    )?;
    let read = run(&b, spawn, out, deadline);
    // The box ends whatever the query left, read or not.
    b.delete()?;
    read
}

/// Runs the query in `b` and reads its listing from `out` by `deadline`, its exit a success.
fn run(
    b: &JobBox,
    spawn: super::sys::Spawn,
    out: std::io::PipeReader,
    deadline: Option<Instant>,
) -> Result<String, Refusal> {
    let Started {
        process,
        thread,
        pid,
    } = b.exec(spawn)?;
    drop(thread);
    let mut member = proc::BoxProc::new(process, pid, b.shared());
    // The listing is read on a thread of its own, so the wait for it has a deadline; one byte past its
    // bound is read, so an overlong listing is refused rather than cut.
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut text = String::new();
        let bound = u64::try_from(FEATURE_LISTING_MAX)
            .ok()
            .and_then(|n| n.checked_add(1))
            .unwrap_or(u64::MAX);
        let read = out.take(bound).read_to_string(&mut text).map(|_| text);
        let _ = tx.send(read);
    });
    let left = || deadline.map_or(PATIENCE, |d| d.saturating_duration_since(Instant::now()));
    let late = || Refusal::Os {
        op: "reading the Windows features",
        why: format!(
            "PowerShell's Win32_OptionalFeature query didn't finish within {} ms",
            PATIENCE.as_millis()
        ),
    };
    let listing = match rx.recv_timeout(left()) {
        Ok(read) => read.map_err(|e| Refusal::os("reading the Windows features", e))?,
        Err(_) => return Err(late()),
    };
    match member.wait_for(left())? {
        Some(hex_api::Exit::Code(0)) => Ok(listing),
        Some(exit) => Err(Refusal::Os {
            op: "reading the Windows features",
            why: format!("PowerShell's Win32_OptionalFeature query ended {exit:?}"),
        }),
        None => Err(late()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HEX-INST-9 (native plan step 6): a query that runs past the bound is ended and refused by name, not
    /// waited on, and the refusal states the bound as it is (in ms: whole seconds read 4,842 ms as 4 s).
    /// The fake query is PowerShell sleeping twice [`PATIENCE`], so the case derives from the bound itself.
    #[test]
    fn a_query_past_the_bound_is_refused_by_name() {
        let processors = NonZeroU32::new(super::super::sys::active_processors()).unwrap();
        let sleep = format!("Start-Sleep -Milliseconds {}", PATIENCE.as_millis() * 2);
        let refused = listing(processors, &powershell(&sleep).unwrap()).unwrap_err();
        assert!(
            matches!(&refused, Refusal::Os { why, .. } if why.contains("didn't finish within")),
            "{refused:?}"
        );
        let bound = format!("within {} ms", PATIENCE.as_millis());
        assert!(
            matches!(&refused, Refusal::Os { why, .. } if why.contains(&bound)),
            "{refused:?} doesn't name {bound}"
        );
    }
}
