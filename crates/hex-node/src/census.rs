//! The waits census (`bench/competitive/probes/waits_census.md`; decision 9): with [`ENV_FILE`] naming a file, every
//! answer every named wait in this process records ([`hex_core::patience`]) is appended to it as a line `name nanos`,
//! from the waits' own timing points, so the probe reads each wait's distribution without re-implementing it. With
//! it unset nothing is installed and a wait's answer costs one atomic load more. A bench's switch: the probe sets it.
//!
//! Bounded (CLAUDE.md §2): at most [`ENV_MAX`] lines, which the probe sets from its N; past it one last line says
//! the census is full, and no more is written, so the record says it was cut rather than end silently (§3).

use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use hex_api::Refusal;

/// The file the census appends to.
pub const ENV_FILE: &str = "HEX_WAITS_CENSUS";
/// The most lines it writes.
pub const ENV_MAX: &str = "HEX_WAITS_CENSUS_MAX";

static OUT: OnceLock<Mutex<File>> = OnceLock::new();
static MAX: AtomicU64 = AtomicU64::new(0);
static LINES: AtomicU64 = AtomicU64::new(0);

/// Installs the census where [`ENV_FILE`] and [`ENV_MAX`] are set: says whether it did. One set without the other,
/// or a bound that isn't a number, is refused by type.
pub fn install_from_env() -> Result<bool, Refusal> {
    let (file, max) = match (std::env::var_os(ENV_FILE), std::env::var_os(ENV_MAX)) {
        (None, None) => return Ok(false),
        (Some(file), Some(max)) => (file, max),
        _ => {
            return Err(Refusal::Invalid {
                what: "waits census",
                why: format!("{ENV_FILE} and {ENV_MAX} are set together or not at all"),
            });
        }
    };
    let max: u64 = max
        .to_str()
        .and_then(|m| m.parse().ok())
        .ok_or_else(|| Refusal::Invalid {
            what: "waits census",
            why: format!("{ENV_MAX} isn't a count of lines"),
        })?;
    let out = File::options()
        .create(true)
        .append(true)
        .open(&file)
        .map_err(|e| Refusal::os("opening the waits census", e))?;
    MAX.store(max, Ordering::Relaxed);
    let _ = OUT.set(Mutex::new(out));
    Ok(hex_core::patience::census(record))
}

/// One answer, appended as `name nanos`; past the bound, one line saying the census is full, then nothing.
fn record(name: &'static str, took: Duration) {
    let Some(out) = OUT.get() else {
        return;
    };
    let n = LINES.fetch_add(1, Ordering::Relaxed);
    let max = MAX.load(Ordering::Relaxed);
    let line = if n < max {
        format!("{name} {}\n", took.as_nanos())
    } else if n == max {
        format!("census full at {max} lines\n")
    } else {
        return;
    };
    if let Ok(mut out) = out.lock() {
        let _ = out.write_all(line.as_bytes());
    }
}
