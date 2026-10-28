//! The container state the OCI spec defines (`state`'s output), kept in the container's state
//! directory and replaced whole on each write. The status follows the spec's lifecycle: `creating`, `created`, `running`,
//! `stopped`. Only `creating` and `created` are written; `running` and `stopped` are derived, as crun derives
//! them (libcrun's `libcrun_get_container_state_string`): a live init whose exec FIFO is gone has been started,
//! and an init no longer live has stopped ([`derive`]). So `start` writes nothing (decision 3, goal 3 plan step 3).

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
    /// Whether the bundle names poststart hooks, recorded at create: Hex's own field, so `start` reads
    /// `config.json` only when there are hooks to run. Absent in state written before, which `start`
    /// reads as "maybe" and so reads the config, as it always did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poststart: Option<bool>,
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
            poststart: None,
        }
    }

    /// Whether the container's init still runs: a live process at its pid with the start time create
    /// recorded. State written before the class recorded one is judged by the pid alone.
    pub fn init_alive(&self) -> bool {
        alive(self.start_time, start_time(self.pid))
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

    /// Whether the exec FIFO is still there: `start` removes it once the init has its byte. A failure to
    /// look other than its absence is taken as present, so a container is never reported started that
    /// may not be.
    pub fn fifo_present(&self) -> bool {
        match fs::symlink_metadata(self.fifo()) {
            Ok(_) => true,
            Err(e) => e.kind() != std::io::ErrorKind::NotFound,
        }
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

/// A container's status as the runtime-spec defines it, from what create recorded and what is seen now: whether
/// the exec FIFO is still there (`start` removes it) and whether the init is alive ([`alive`]). As crun derives
/// it. A recorded `running` is a start from before this derivation, which rewrote the state and left the FIFO, so
/// it reads as running for as long as its init lives.
pub fn derive(recorded: Status, fifo_present: bool, init_alive: bool) -> Status {
    match recorded {
        Status::Creating => Status::Creating,
        Status::Stopped => Status::Stopped,
        Status::Created | Status::Running if !init_alive => Status::Stopped,
        Status::Created if fifo_present => Status::Created,
        Status::Created | Status::Running => Status::Running,
    }
}

/// Whether a container's init is alive: a live process at its pid (`now`, its start time, `None` for no process
/// or a zombie) with the start time create recorded, so a later process given the pid isn't taken for it. State
/// recorded before the class kept a start time is judged by the pid alone.
pub fn alive(recorded_start: Option<u64>, now: Option<u64>) -> bool {
    match (recorded_start, now) {
        (_, None) => false,
        (Some(recorded), Some(now)) => recorded == now,
        (None, Some(_)) => true,
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

    /// Goal 3 plan step 3, the oracle (AC-T7.1.4): every combination of what create recorded, whether the exec
    /// FIFO is there, whether a process holds the init's pid (live, a zombie, or none) and whether its start time
    /// matches the recorded one, against the status the runtime-spec gives: `created` before start, `running`
    /// after it while the init lives, `stopped` once it doesn't.
    #[test]
    fn the_derived_status_is_the_runtime_specs_for_every_combination() {
        // The process at the init's pid: its start time when live, `None` when a zombie or gone.
        let seen = [
            ("live, same start", Some(100)),
            ("live, recycled", Some(200)),
            ("zombie or gone", None),
        ];
        for recorded_start in [Some(100), None] {
            for (what, now) in seen {
                let live = match (recorded_start, now) {
                    (_, None) => false,
                    (Some(r), Some(n)) => r == n,
                    (None, Some(_)) => true,
                };
                assert_eq!(
                    alive(recorded_start, now),
                    live,
                    "{what}, recorded {recorded_start:?}"
                );
                for fifo in [true, false] {
                    let want = |recorded: Status| match (recorded, live, fifo) {
                        (Status::Creating, _, _) => Status::Creating,
                        (Status::Stopped, _, _) => Status::Stopped,
                        (_, false, _) => Status::Stopped,
                        (Status::Created, true, true) => Status::Created,
                        (_, true, _) => Status::Running,
                    };
                    for recorded in [
                        Status::Creating,
                        Status::Created,
                        Status::Running,
                        Status::Stopped,
                    ] {
                        assert_eq!(
                            derive(recorded, fifo, live),
                            want(recorded),
                            "recorded {recorded:?}, fifo {fifo}, init {what}, start time recorded {recorded_start:?}"
                        );
                    }
                }
            }
        }
        // The named cases, as the spec reads them.
        assert_eq!(
            derive(Status::Created, true, true),
            Status::Created,
            "before start"
        );
        assert_eq!(
            derive(Status::Created, false, true),
            Status::Running,
            "after start"
        );
        assert_eq!(
            derive(Status::Created, false, false),
            Status::Stopped,
            "after the init ends"
        );
        assert_eq!(
            derive(Status::Created, true, false),
            Status::Stopped,
            "an init that died before start"
        );
        assert_eq!(
            derive(Status::Running, true, true),
            Status::Running,
            "a start from before the derivation"
        );
        assert!(
            !alive(Some(100), Some(200)),
            "a recycled pid isn't the init"
        );
    }

    /// Append-only: state written before the poststart field reads as "maybe" (`None`), which `start` takes as
    /// a reason to read the config, as it always did; once set, the field round-trips.
    #[test]
    fn state_without_a_poststart_field_reads_as_maybe_and_one_with_it_round_trips() {
        let old = br#"{"ociVersion":"1.3.0","id":"c1","status":"created","pid":7,"bundle":"/b"}"#;
        let st: State = serde_json::from_slice(old).unwrap();
        assert_eq!(st.poststart, None);
        let mut with = st;
        with.poststart = Some(false);
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
