//! The container state the OCI spec defines (`state`'s output), kept durably in the container's
//! state directory. The status follows the spec's lifecycle: `creating`, `created`, `running`,
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

    /// Writes the state durably: temp file, fsync, rename, directory fsync.
    pub fn put(&self, state: &State) -> Result<(), OciError> {
        let bytes = serde_json::to_vec(state)
            .map_err(|e| OciError::Broken(format!("encoding state: {e}")))?;
        let tmp = self.dir.join(".state.json.tmp");
        {
            let mut f = File::create(&tmp).map_err(|e| OciError::os("writing state", e))?;
            f.write_all(&bytes)
                .map_err(|e| OciError::os("writing state", e))?;
            f.sync_all()
                .map_err(|e| OciError::os("flushing state", e))?;
        }
        fs::rename(&tmp, self.dir.join("state.json"))
            .map_err(|e| OciError::os("renaming state", e))?;
        File::open(&self.dir)
            .and_then(|d| d.sync_all())
            .map_err(|e| OciError::os("flushing the container directory", e))
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

#[cfg(test)]
mod tests {
    use super::*;

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
