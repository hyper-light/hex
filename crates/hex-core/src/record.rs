//! The node agent's durable record of each box, and the steps that make and remove a box in the
//! order the records make safe: a record says what a box is about to become before the machine
//! changes, so after a crash at any step recovery knows what every leftover is (T-T7.1.2).

use std::path::PathBuf;

use hex_api::{BoxSpec, Class, Refusal};
use serde::{Deserialize, Serialize};

/// The record format's version; a record of another is refused, never guessed at.
pub const RECORD_VERSION: u32 = 1;
/// Largest record Hex reads: a spec with its mounts and program fits in a few kilobytes, so a
/// larger file is damage, refused before it is parsed.
pub const MAX_RECORD_BYTES: usize = 1 << 20;

/// What the node agent knows of one box.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoxRecord {
    pub version: u32,
    pub spec: BoxSpec,
    pub phase: Phase,
    /// The container's init once it started (`oci`).
    pub init: Option<ProcId>,
    /// Directories the box owns and removes with itself.
    pub dirs: Vec<PathBuf>,
}

/// Where a box is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// Being made: its cgroup, directories and init may each exist or not.
    Creating,
    /// Made: what it holds runs, or may.
    Ready,
    /// Being removed.
    Deleting,
}

/// A process, told apart from a later one given the same pid by its start time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcId {
    pub pid: u32,
    /// Its start time in clock ticks after boot (`/proc/<pid>/stat`, field 22).
    pub start_ticks: u64,
}

impl BoxRecord {
    pub fn new(spec: BoxSpec, phase: Phase, dirs: Vec<PathBuf>) -> Self {
        Self {
            version: RECORD_VERSION,
            spec,
            phase,
            init: None,
            dirs,
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>, Refusal> {
        let mut out = serde_json::to_vec(self)
            .map_err(|e| Refusal::Broken(format!("encoding a record: {e}")))?;
        out.push(b'\n');
        Ok(out)
    }

    /// A record from its file's bytes, or why they aren't one.
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(format!("{} bytes, over {MAX_RECORD_BYTES}", bytes.len()));
        }
        let r: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if r.version != RECORD_VERSION {
            return Err(format!(
                "version {}, this node agent reads {RECORD_VERSION}",
                r.version
            ));
        }
        Ok(r)
    }
}

/// One step that changes the machine or the records; the shell does them in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Write the record, phase `Creating`.
    MarkCreating,
    /// Make the box's cgroup and write its bounds.
    MakeCgroup,
    /// Make the directories the box owns.
    MakeDirs,
    /// Start the container's init in the box (`oci`).
    StartInit,
    /// Write the record, phase `Ready`, with the init it started.
    MarkReady,
    /// Write the record, phase `Deleting`.
    MarkDeleting,
    /// End every process in the box and wait until none is left.
    KillAll,
    /// Remove the box's cgroup.
    RemoveCgroup,
    /// Remove the directories the box owns.
    RemoveDirs,
    /// Remove the record.
    DropRecord,
}

/// The steps that make a box of `class`.
pub fn create_steps(class: Class) -> &'static [Step] {
    match class {
        Class::Process => &[Step::MarkCreating, Step::MakeCgroup, Step::MarkReady],
        Class::Oci => &[
            Step::MarkCreating,
            Step::MakeCgroup,
            Step::MakeDirs,
            Step::StartInit,
            Step::MarkReady,
        ],
    }
}

/// The steps that remove a box its caller asked to delete.
pub const DELETE_STEPS: &[Step] = &[
    Step::MarkDeleting,
    Step::KillAll,
    Step::RemoveCgroup,
    Step::RemoveDirs,
    Step::DropRecord,
];

/// The steps that remove a box recovery found unusable: each tolerates what it removes being
/// absent already, so a crash during recovery is recovered from by running it again.
pub const REMOVE_STEPS: &[Step] = &[
    Step::KillAll,
    Step::RemoveCgroup,
    Step::RemoveDirs,
    Step::DropRecord,
];

#[cfg(test)]
mod tests {
    use super::*;
    use hex_api::{Bounds, BoxId, ClassSpec};
    use proptest::prelude::*;

    fn record() -> BoxRecord {
        let spec = BoxSpec {
            id: BoxId::new("r1").unwrap(),
            bounds: Bounds::default(),
            class: ClassSpec::Process,
        };
        BoxRecord::new(
            spec,
            Phase::Ready,
            vec![PathBuf::from("/var/lib/hex/boxes/r1")],
        )
    }

    #[test]
    fn a_record_reads_back_as_written() {
        let mut r = record();
        r.init = Some(ProcId {
            pid: 42,
            start_ticks: 1234,
        });
        assert_eq!(BoxRecord::decode(&r.encode().unwrap()).unwrap(), r);
    }

    #[test]
    fn a_record_of_another_version_or_too_large_is_refused() {
        let mut r = record();
        r.version = 2;
        assert!(
            BoxRecord::decode(&r.encode().unwrap())
                .unwrap_err()
                .contains("version 2")
        );
        assert!(BoxRecord::decode(&vec![b' '; MAX_RECORD_BYTES + 1]).is_err());
    }

    proptest! {
        // Hostile input (CLAUDE.md §6): any bytes are a record or a reason, never a panic.
        #[test]
        fn any_bytes_decode_to_a_record_or_a_reason(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let _ = BoxRecord::decode(&bytes);
        }

        #[test]
        fn a_truncated_record_is_refused(cut in 0usize..200) {
            let full = record().encode().unwrap();
            let cut = cut.min(full.len().saturating_sub(2));
            prop_assert!(BoxRecord::decode(&full[..cut]).is_err());
        }
    }
}
