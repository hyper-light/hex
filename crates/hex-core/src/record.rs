//! The node agent's durable record of each box, and the steps that make and remove a box in an
//! order a crash at any step leaves nothing unknown after (T-T7.1.2). A box's parts lie at paths its
//! id fixes (its cgroup, made first, its rendered bundle, its container's state directory), so
//! recovery finds what a box caught mid-create left by looking there, and its record is written once,
//! when the box is whole (goal 5 extended, step 5). A delete marks the record before the machine
//! changes, so a box caught mid-delete is known to be going.

use std::path::PathBuf;

use hex_api::{BoxSpec, ClassSpec, Exit, Refusal};
use serde::{Deserialize, Serialize};

use crate::cgroup::Baseline;

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
    /// How the container's init ended, once the node agent reaped it (`oci`). Added within version 1,
    /// so a record without it reads as `None` (append-only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<Exit>,
    /// Programs exec'd in the box that the node agent ended because their caller's connection to its
    /// socket closed while they ran (decision 3), which inspect reports. Added within version 1, so a
    /// record without it reads as 0 (append-only).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub abandoned: u64,
    /// The slot its cgroup is named by, for a box given a cgroup that may be handed on to its owner's
    /// next box (goal 5 extended, step 6; [`crate::reuse`]): a cgroup can't be renamed, so one that
    /// outlives its box is named by its slot, never by a box's id (research f35, f64). Added within
    /// version 1, so a record without it names a cgroup by its box's id (append-only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<Slot>,
    /// The program a run made this `process` box for ([`hex_api::Runtime::run`]): the box goes once
    /// it has ended, by its node agent or, after a restart, by recovery (decision 7). Added within
    /// version 1, so a record without it is a box made by create (append-only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<ProcId>,
}

/// A cgroup that may be handed from box to box of one owner ([`crate::reuse`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slot {
    /// Its number, which names its cgroup ([`crate::reuse::name`]).
    pub n: u64,
    /// Its cumulative counters when this box was given it, which its usage is reported from.
    pub baseline: Baseline,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// Where a box is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// Being made: its cgroup, directories and init may each exist or not. Written by node agents
    /// before goal 5 extended step 5, which write no record until the box is made; recovery removes a
    /// box whose record still says it.
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
            exit: None,
            abandoned: 0,
            slot: None,
            run: None,
        }
    }

    /// The record's file: refused past [`MAX_RECORD_BYTES`], which [`BoxRecord::decode`] would refuse
    /// to read back, so a box whose spec is that large is never made.
    pub fn encode(&self) -> Result<Vec<u8>, Refusal> {
        let mut out = serde_json::to_vec(self)
            .map_err(|e| Refusal::Broken(format!("encoding a record: {e}")))?;
        out.push(b'\n');
        if out.len() > MAX_RECORD_BYTES {
            return Err(Refusal::Invalid {
                what: "box spec",
                why: format!(
                    "its record is {} bytes, over the {MAX_RECORD_BYTES} the node agent reads back",
                    out.len()
                ),
            });
        }
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
    /// Persist exact backend intent before dispatching a guest create whose native effects may
    /// survive the caller. Its runtime owns the guest, not an OCI cgroup/state directory here.
    MarkCreating,
    /// Write the bundle rendered from the box's [`hex_api::OciSpec`] into a directory of its own.
    RenderBundle,
    /// Make the box's cgroup and write its bounds.
    MakeCgroup,
    /// Make the container's state directory (`oci`).
    MakeStateDir,
    /// Start the container's init in the box (`oci`), or a run's program (`process`).
    StartInit,
    /// Write the record, phase `Ready`, with the init it started: the box's first record.
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

/// The steps that make a box of `class`: no record before the last, each part made before it at a path
/// the box's id fixes, the cgroup first, whose mkdir claims the id.
pub fn create_steps(class: &ClassSpec) -> &'static [Step] {
    match class {
        ClassSpec::Process => &[Step::MakeCgroup, Step::MarkReady],
        ClassSpec::Bundle(_) => &[
            Step::MakeCgroup,
            Step::MakeStateDir,
            Step::StartInit,
            Step::MarkReady,
        ],
        ClassSpec::Oci(_) => &[
            Step::MakeCgroup,
            Step::RenderBundle,
            Step::MakeStateDir,
            Step::StartInit,
            Step::MarkReady,
        ],
        ClassSpec::Firecracker(_) | ClassSpec::Shards(_) => {
            &[Step::MarkCreating, Step::StartInit, Step::MarkReady]
        }
    }
}

/// The steps of a run ([`hex_api::Runtime::run`]): a `process` box's cgroup, its program cloned into it,
/// then the record naming the program, so a box no record names yet is found by its cgroup and removed.
pub const RUN_STEPS: &[Step] = &[Step::MakeCgroup, Step::StartInit, Step::MarkReady];

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
    use hex_api::{Bounds, BoxId, ClassSpec, Network, OciSpec, Program, Rootfs};
    use proptest::prelude::*;

    fn record() -> BoxRecord {
        let spec = BoxSpec {
            devices: Vec::new(),
            id: BoxId::new("r1").unwrap(),
            bounds: Bounds::default(),
            class: ClassSpec::Process,
            owner: None,
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

    /// Decision 7, append-only: a record written before `run` existed reads as a box made by create,
    /// one with no run is written without the field, and a run's program round-trips.
    #[test]
    fn a_record_without_a_run_reads_and_writes_as_before() {
        let r = record();
        let text = String::from_utf8(r.encode().unwrap()).unwrap();
        assert!(!text.contains("\"run\""), "{text}");
        assert_eq!(BoxRecord::decode(text.as_bytes()).unwrap().run, None);
        let mut ran = r;
        ran.run = Some(ProcId {
            pid: 7,
            start_ticks: 99,
        });
        assert_eq!(BoxRecord::decode(&ran.encode().unwrap()).unwrap(), ran);
    }

    /// HEX-SOCK-6, append-only: a record written before `abandoned` existed reads as 0, and a record
    /// with none is written as before, so an older node agent reads it.
    #[test]
    fn a_record_without_abandoned_programs_reads_and_writes_as_before() {
        let r = record();
        let text = String::from_utf8(r.encode().unwrap()).unwrap();
        assert!(!text.contains("abandoned"), "{text}");
        assert_eq!(BoxRecord::decode(text.as_bytes()).unwrap().abandoned, 0);
        let mut left = r;
        left.abandoned = 3;
        assert_eq!(
            BoxRecord::decode(&left.encode().unwrap())
                .unwrap()
                .abandoned,
            3
        );
    }

    /// Goal 5 extended, step 6, append-only: a record of a box whose cgroup is its own reads and writes
    /// as before, with no slot; one whose cgroup is a slot keeps the slot and its baseline.
    #[test]
    fn a_records_slot_is_kept_and_a_record_without_one_reads_as_before() {
        let r = record();
        let text = String::from_utf8(r.encode().unwrap()).unwrap();
        assert!(!text.contains("slot"), "{text}");
        assert_eq!(BoxRecord::decode(text.as_bytes()).unwrap().slot, None);
        let mut slotted = r;
        slotted.slot = Some(Slot {
            n: 7,
            baseline: Baseline {
                cpu_usec: 1,
                io_read_bytes: 2,
                io_write_bytes: 3,
                oom_kills: 4,
                pids_refused: 5,
            },
        });
        assert_eq!(
            BoxRecord::decode(&slotted.encode().unwrap()).unwrap(),
            slotted
        );
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

    /// CLAUDE.md §2: a record too large to read back is refused when it's written, typed, rather than
    /// written for recovery to find unreadable.
    #[test]
    fn a_record_too_large_to_read_back_is_refused_when_written() {
        let mut init = Program::of(&["sleep", "infinity"]);
        init.env
            .push(("BIG".to_owned(), "x".repeat(MAX_RECORD_BYTES)));
        let mut r = record();
        r.spec.class = ClassSpec::Oci(Box::new(OciSpec {
            rootfs: Rootfs::Dir(PathBuf::from("/srv/rootfs")),
            init,
            mounts: Vec::new(),
            hostname: None,
            network: Network::Private,
            hosts: Vec::new(),
        }));
        assert!(matches!(
            r.encode(),
            Err(Refusal::Invalid {
                what: "box spec",
                ..
            })
        ));
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
