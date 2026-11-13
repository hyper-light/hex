//! The cgroup a Linux test process subdivides for its boxes: its own, delegated to it ([`delegated`]). It is
//! prepared by moving the process into a leaf of it ([`LEAF`]), so the cgroup holds no process of its own and the
//! kernel lets it hand its controllers down, then by the caller's probe that every controller its boxes use is
//! handed down (hex-cli's tests open a `BoxRoot` there).

use std::fs;
use std::path::{Path, PathBuf};

/// The leaf the test process is moved into, and with it every process it starts from then on.
pub const LEAF: &str = "hex-agent";

/// This process's cgroup directory under /sys/fs/cgroup, as /proc/self/cgroup names it now.
pub fn own() -> Result<PathBuf, String> {
    let own = fs::read_to_string("/proc/self/cgroup")
        .map_err(|e| format!("reading /proc/self/cgroup: {e}"))?;
    let rel = own
        .lines()
        .find_map(|l| l.strip_prefix("0::"))
        .ok_or_else(|| "no cgroup v2 line (0::) in /proc/self/cgroup".to_owned())?;
    Ok(Path::new("/sys/fs/cgroup").join(rel.trim_start_matches('/')))
}

/// This process's own cgroup, delegated to it: the process moved into its [`LEAF`], then `probe` run on it. Behavior
/// kept here: each call prepares, reading the cgroup the process is in then, and only this process moves.
pub fn delegated(probe: impl FnOnce(&Path) -> Result<(), String>) -> Result<PathBuf, String> {
    let own = own()?;
    let leaf = own.join(LEAF);
    fs::create_dir_all(&leaf).map_err(|e| format!("making a leaf for the agent: {e}"))?;
    fs::write(leaf.join("cgroup.procs"), std::process::id().to_string())
        .map_err(|e| format!("moving self into a leaf: {e}"))?;
    probe(&own)?;
    Ok(own)
}

#[cfg(test)]
mod tests;
