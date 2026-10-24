//! The container state the OCI spec defines (`state`'s output), kept in the container's state
//! directory and replaced whole on each write. The status follows the spec's lifecycle: `creating`, `created`, `running`,
//! `stopped`.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::linux::oci::OciError;

/// The OCI runtime-state version Hex reports.
pub const OCI_VERSION: &str = "1.3.0";

/// A container's status (OCI runtime-spec §State).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Creating,
    Created,
    Running,
    Stopped,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Self::Creating => "creating",
            Self::Created => "created",
            Self::Running => "running",
            Self::Stopped => "stopped",
        }
    }
}

/// The container state, serialized as the OCI `state` output and kept on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub oci_version: String,
    pub id: String,
    pub status: Status,
    /// The container init's pid in the runtime's pid namespace; 0 before `created`.
    pub pid: i32,
    pub bundle: String,
    #[serde(default)]
    pub annotations: std::collections::BTreeMap<String, String>,
    /// The container's cgroup directory: Hex's own field beside the spec's, so `delete` finds the
    /// cgroup whatever has become of the bundle. Absent in state written before the class made one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cgroup: Option<String>,
    /// The init's start time ([`start_time`]), recorded at create: Hex's own field, as crun's
    /// `process-start-time`, so a later process given the init's pid isn't taken for it. Nothing
    /// writes `stopped`; it's whatever this no longer matches. Absent in state written before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_time: Option<u64>,
}

impl State {
    pub fn new(id: &str, bundle: &str) -> Self {
        Self {
            oci_version: OCI_VERSION.to_owned(),
            id: id.to_owned(),
            status: Status::Creating,
            pid: 0,
            bundle: bundle.to_owned(),
            annotations: std::collections::BTreeMap::new(),
            cgroup: None,
            start_time: None,
        }
    }

    /// Whether the container's init still runs: a live process at its pid with the start time create
    /// recorded. State written before the class recorded one is judged by the pid alone.
    pub fn init_alive(&self) -> bool {
        match (self.start_time, start_time(self.pid)) {
            (_, None) => false,
            (Some(recorded), Some(now)) => recorded == now,
            (None, Some(_)) => true,
        }
    }

    /// The JSON the `state` command prints, newline-terminated.
    pub fn to_json(&self) -> Result<Vec<u8>, OciError> {
        let mut out = serde_json::to_vec(self)
            .map_err(|e| OciError::Broken(format!("encoding state: {e}")))?;
        out.push(b'\n');
        Ok(out)
    }
}

/// A container's directory: its `state.json` and its `exec.fifo`.
#[derive(Debug, Clone)]
pub struct Dir {
    dir: PathBuf,
}

impl Dir {
    /// The directory for container `id` under `root` (the runtime's per-user state root).
    pub fn new(root: &Path, id: &str) -> Self {
        Self { dir: root.join(id) }
    }

    /// The container directory at `path` directly (it already points at `<root>/<id>`).
    pub fn at(path: &Path) -> Self {
        Self {
            dir: path.to_path_buf(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }

    pub fn fifo(&self) -> PathBuf {
        self.dir.join("exec.fifo")
    }

    pub fn exists(&self) -> bool {
        self.dir.join("state.json").exists()
    }

    /// Makes the container directory; refuses when one exists already.
    pub fn create(&self, id: &str) -> Result<(), OciError> {
        match fs::create_dir_all(&self.dir) {
            Ok(()) => {}
            Err(e) => return Err(OciError::os("making the container directory", e)),
        }
        if self.exists() {
            return Err(OciError::Exists(id.to_owned()));
        }
        Ok(())
    }

    /// Writes the state atomically, as runc and crun do: a temp file renamed into place, never
    /// fsynced. rename(2) leaves no moment without a whole state.json, and the page cache outlives a
    /// crash of this process. A host crash ends every container the state describes, and the state
    /// root is under /run, emptied at boot (docs/design/runtime-architecture.md, decision 1).
    pub fn put(&self, state: &State) -> Result<(), OciError> {
        let bytes = serde_json::to_vec(state)
            .map_err(|e| OciError::Broken(format!("encoding state: {e}")))?;
        let tmp = self.dir.join(".state.json.tmp");
        {
            let mut f = File::create(&tmp).map_err(|e| OciError::os("writing state", e))?;
            f.write_all(&bytes)
                .map_err(|e| OciError::os("writing state", e))?;
        }
        fs::rename(&tmp, self.dir.join("state.json")).map_err(|e| OciError::os("renaming state", e))
    }

    /// The container's state, or [`OciError::NoSuchContainer`].
    pub fn get(&self, id: &str) -> Result<State, OciError> {
        match fs::read(self.dir.join("state.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| OciError::Broken(format!("container {id}'s state: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(OciError::NoSuchContainer(id.to_owned()))
            }
            Err(e) => Err(OciError::os("reading state", e)),
        }
    }

    /// Removes the container directory and everything in it.
    pub fn remove(&self) -> Result<(), OciError> {
        match fs::remove_dir_all(&self.dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(OciError::os("removing the container directory", e)),
        }
    }
}

/// The start time of process `pid`, in clock ticks since boot (proc_pid_stat(5), field 22), or None
/// when no live process has that pid: none at all, or one that has exited and waits to be reaped.
pub fn start_time(pid: i32) -> Option<u64> {
    if pid <= 0 {
        return None;
    }
    parse_start_time(&fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

/// `/proc/<pid>/stat`'s start time, from the fields after the command name, which is parenthesized
/// and may itself hold spaces or parentheses: the state (field 3), then nineteen on, the start time.
fn parse_start_time(stat: &str) -> Option<u64> {
    let (_, rest) = stat.rsplit_once(')')?;
    let mut fields = rest.split_whitespace();
    match fields.next()? {
        // Z exited and unreaped, X dead: neither runs.
        "Z" | "X" => None,
        _ => fields.nth(18)?.parse().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decision 2: a process's start time comes from its stat line, a command with spaces and
    /// parentheses included, and an exited process has none.
    #[test]
    fn a_start_time_is_read_from_the_stat_line_and_an_exited_process_has_none() {
        let fields = "S 1 1 1 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 987654 1000 10";
        assert_eq!(
            parse_start_time(&format!("42 (sleep) {fields}")),
            Some(987_654)
        );
        assert_eq!(
            parse_start_time(&format!("42 (a (b) c) {fields}")),
            Some(987_654)
        );
        assert_eq!(
            parse_start_time(&format!("42 (sleep) {}", fields.replacen('S', "Z", 1))),
            None
        );
        assert_eq!(parse_start_time("42 (sleep) S 1"), None);
        assert_eq!(parse_start_time("garbage"), None);
        assert_eq!(start_time(0), None);
        assert!(start_time(i32::try_from(std::process::id()).unwrap()).is_some());
    }

    #[test]
    fn state_is_put_got_and_removed() {
        let root = tempfile::tempdir().unwrap();
        let dir = Dir::new(root.path(), "c1");
        assert!(dir.get("c1").is_err());
        dir.create("c1").unwrap();
        let mut st = State::new("c1", "/bundle");
        st.status = Status::Running;
        st.pid = 4321;
        dir.put(&st).unwrap();
        assert_eq!(dir.get("c1").unwrap(), st);
        assert!(dir.exists());
        dir.remove().unwrap();
        assert!(dir.get("c1").is_err());
    }

    // AC-T7.1.4, append-only wire format: state written before the cgroup field reads as having none,
    // and the field round-trips once set.
    #[test]
    fn state_without_a_cgroup_still_reads_and_one_with_it_round_trips() {
        let old = br#"{"ociVersion":"1.3.0","id":"c1","status":"created","pid":7,"bundle":"/b"}"#;
        let st: State = serde_json::from_slice(old).unwrap();
        assert_eq!(st.cgroup, None);
        let json: serde_json::Value = serde_json::from_slice(&st.to_json().unwrap()).unwrap();
        assert!(json.get("cgroup").is_none());
        let mut with = st;
        with.cgroup = Some("/sys/fs/cgroup/c1".to_owned());
        let back: State = serde_json::from_slice(&with.to_json().unwrap()).unwrap();
        assert_eq!(back, with);
    }

    #[test]
    fn the_state_json_matches_the_oci_shape() {
        let st = State::new("abc", "/b");
        let json: serde_json::Value = serde_json::from_slice(&st.to_json().unwrap()).unwrap();
        assert_eq!(json["ociVersion"], "1.3.0");
        assert_eq!(json["id"], "abc");
        assert_eq!(json["status"], "creating");
        assert_eq!(json["bundle"], "/b");
    }
}
