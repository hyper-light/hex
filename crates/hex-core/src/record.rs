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

mod pool;
pub use pool::{NativeRecord, NativeStage, OwnerKey, OwnerRef, PoolRecord};

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
    /// One native VM lifetime, introduced by Store's fresh OS entropy fill before any native/boot
    /// intent effect. A legacy missing value supplies no authority to adopt or stop a native scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_scope: Option<NativeScope>,
}

/// An opaque 128-bit native incarnation. The core accepts injected bytes and performs no OS call;
/// the Store shell alone creates fresh production values. Neither PID, time nor path is identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NativeScope([u8; 16]);

impl NativeScope {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const fn bytes(&self) -> &[u8; 16] {
        &self.0
    }
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
    /// The native keeper commits to the immutable scope and original workload/admission receipt.
    /// Mutable lifecycle/accounting fields do not change that cleanup identity.
    pub fn native_commitment(&self) -> Result<[u8; 32], Refusal> {
        let scope = self
            .native_scope
            .ok_or_else(|| scope_refusal("native scope is absent"))?;
        commitment(&(scope, &self.spec))
    }

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
            native_scope: None,
        }
    }

    /// A fresh Store write adds its generated scope without cloning the original workload or
    /// allocating a parallel record. The flattened source has no scope field in this case.
    pub fn encode_fresh_scope(&self, scope: NativeScope) -> Result<Vec<u8>, Refusal> {
        if self.native_scope.is_some() || self.phase != Phase::Creating {
            let message = "only a fresh Creating record can introduce a native scope";
            let mut why = String::new();
            why.try_reserve_exact(message.len())
                .map_err(|_| Refusal::Exhausted {
                    what: "scope refusal bytes",
                    bound: message.len(),
                })?;
            why.push_str(message);
            return Err(Refusal::Invalid {
                what: "native scope introduction",
                why,
            });
        }
        #[derive(Serialize)]
        struct Fresh<'a> {
            #[serde(flatten)]
            original: &'a BoxRecord,
            native_scope: NativeScope,
        }
        encode_record(&Fresh {
            original: self,
            native_scope: scope,
        })
    }

    /// The record's file: refused past [`MAX_RECORD_BYTES`], which [`BoxRecord::decode`] would refuse
    /// to read back, so a box whose spec is that large is never made.
    pub fn encode(&self) -> Result<Vec<u8>, Refusal> {
        encode_record(self)
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

fn commitment(value: &impl Serialize) -> Result<[u8; 32], Refusal> {
    use sha2::Digest;
    struct Commitment(sha2::Sha256);
    impl std::io::Write for Commitment {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut commitment = Commitment(sha2::Sha256::new());
    serde_json::to_writer(&mut commitment, value)
        .map_err(|_| scope_refusal("native scope/spec cannot be committed exactly"))?;
    Ok(commitment.0.finalize().into())
}

fn scope_refusal(message: &'static str) -> Refusal {
    let mut why = String::new();
    if why.try_reserve_exact(message.len()).is_err() {
        return Refusal::Exhausted {
            what: "scope refusal bytes",
            bound: message.len(),
        };
    }
    why.push_str(message);
    Refusal::Invalid {
        what: "native scope",
        why,
    }
}

fn encode_record(record: &impl Serialize) -> Result<Vec<u8>, Refusal> {
    let mut out = serde_json::to_vec(record)
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
            route: None,
            devices: Vec::new(),
            id: BoxId::new("r1").unwrap(),
            bounds: Bounds::default(),
            class: ClassSpec::Process,
            owner: None,
            allocation: None,
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

    /// AC-T7.4.1: fresh scope encoding preserves the original workload and legacy None encoding.
    #[test]
    fn native_scope_is_an_append_only_pure_incarnation() {
        let mut original = record();
        original.phase = Phase::Creating;
        let legacy = original.encode().unwrap();
        assert!(
            !String::from_utf8(legacy.clone())
                .unwrap()
                .contains("native_scope")
        );
        assert_eq!(BoxRecord::decode(&legacy).unwrap().native_scope, None);
        let scope = NativeScope::from_bytes([0x31; 16]);
        let encoded = original.encode_fresh_scope(scope).unwrap();
        let decoded = BoxRecord::decode(&encoded).unwrap();
        assert_eq!(decoded.spec, original.spec);
        assert_eq!(decoded.phase, Phase::Creating);
        assert_eq!(decoded.native_scope, Some(scope));
        assert_eq!(decoded.native_scope.unwrap().bytes(), &[0x31; 16]);
        assert_eq!(original.native_scope, None);
        assert_eq!(original.encode().unwrap(), legacy);
        assert!(
            decoded
                .encode_fresh_scope(NativeScope::from_bytes([0x32; 16]))
                .is_err()
        );
        original.phase = Phase::Ready;
        assert!(original.encode_fresh_scope(scope).is_err());
    }

    /// AC-T7.4.1: malformed scope width cannot decode as a legacy record without scope.
    #[test]
    fn malformed_native_scope_never_falls_back_to_none() {
        let mut original = record();
        original.phase = Phase::Creating;
        let bytes = original
            .encode_fresh_scope(NativeScope::from_bytes([7; 16]))
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for width in [0, 1, 15, 17, 32] {
            let mut malformed = value.clone();
            malformed["native_scope"] = serde_json::json!(vec![7u8; width]);
            assert!(BoxRecord::decode(&serde_json::to_vec(&malformed).unwrap()).is_err());
        }
    }

    /// AC-T7.4.1: accounting/phase updates retain cleanup identity; another scope or workload cannot.
    #[test]
    fn native_scope_commitment_binds_scope_and_original_allocation() {
        let mut original = record();
        assert!(original.native_commitment().is_err());
        original.native_scope = Some(NativeScope::from_bytes([1; 16]));
        let commitment = original.native_commitment().unwrap();
        let mut next = original.clone();
        next.phase = Phase::Deleting;
        next.abandoned = 11;
        next.exit = Some(Exit::Code(0));
        assert_eq!(next.native_commitment().unwrap(), commitment);
        next.native_scope = Some(NativeScope::from_bytes([2; 16]));
        assert_ne!(next.native_commitment().unwrap(), commitment);
        next.native_scope = original.native_scope;
        next.spec.bounds.memory_max = Some(4096);
        assert_ne!(next.native_commitment().unwrap(), commitment);
    }

    proptest! {
        #[test]
        fn native_scope_every_injected_bitpattern_roundtrips_without_interpreting_its_bits(bytes in any::<[u8; 16]>()) {
            let mut original = record();
            original.phase = Phase::Creating;
            let scope = NativeScope::from_bytes(bytes);
            let decoded = BoxRecord::decode(&original.encode_fresh_scope(scope).unwrap()).unwrap();
            prop_assert_eq!(decoded.native_scope, Some(scope));
            prop_assert_eq!(scope.bytes(), &bytes);
        }

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
