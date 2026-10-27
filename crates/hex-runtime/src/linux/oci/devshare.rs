//! One device program shared by the containers whose device policies are the same (decision 2 of
//! docs/design/runtime-architecture.md). A device program is stateless: it decides an access from the
//! device's type and numbers and the access asked, so one loaded copy attached to many cgroups
//! (`BPF_F_ALLOW_MULTI`) decides for each exactly as a copy of its own would. A copy per container held a
//! vmalloc'd page, 1.2 KiB of per-cpu stats and slab beside them: 6.3-7.4 KiB of kernel memory per idle
//! container, which a shared program pays once per node (bench/competitive/RESULT.md, "What closed it").
//!
//! `create` still loads its own copy, which tells it the program's kernel tag. The hints name, per tag,
//! a program loaded before; `create` attaches that one instead, once the kernel shows it's the same
//! program (the same type, tag and lengths, and the same translated instructions byte for byte), and
//! lets its own copy go. A hint is only a hint: a stale, missing or rewritten one costs the sharing,
//! never the policy, since nothing is attached that the kernel hasn't shown is the same program.
//!
//! This is the pure core: the hints' format and bound, and the match. `run.rs` drives it.

use serde::{Deserialize, Serialize};

use super::OciError;
use super::sys::ProgramIdentity;

/// The hints file, in the runtime's state root.
pub const HINTS_FILE: &str = ".device-programs.json";

/// The most programs the hints name (CLAUDE.md §2), one per distinct device policy in use; past it,
/// the one remembered longest ago is dropped. The default policy is one, and a node runs few others.
pub const MAX_PROGRAMS: usize = 64;

/// The most bytes of the hints file read: 64 programs take under 3 KiB, so a larger file isn't one
/// this module wrote, and is read as no hints.
pub const MAX_HINT_BYTES: u64 = 64 * 1024;

/// A program remembered: its kernel tag, as 16 hex digits, and its id when it was remembered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hint {
    pub tag: String,
    pub id: u32,
}

/// The programs remembered, the most recently remembered first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hints {
    pub programs: Vec<Hint>,
}

impl Hints {
    /// The hints `bytes` hold. Bytes that don't parse are no hints: a hint is never used unchecked,
    /// and the next program remembered rewrites the file whole.
    pub fn parse(bytes: &[u8]) -> Self {
        let mut hints = serde_json::from_slice::<Self>(bytes).unwrap_or_default();
        hints.programs.truncate(MAX_PROGRAMS);
        hints
    }

    /// The id remembered for a program with `tag`, if any.
    pub fn candidate(&self, tag: &[u8; 8]) -> Option<u32> {
        let tag = hex(tag);
        self.programs.iter().find(|h| h.tag == tag).map(|h| h.id)
    }

    /// Remembers program `id` for `tag` first, in place of any earlier one for that tag, keeping at
    /// most [`MAX_PROGRAMS`].
    pub fn remember(&mut self, tag: &[u8; 8], id: u32) {
        let tag = hex(tag);
        self.programs.retain(|h| h.tag != tag);
        self.programs.insert(0, Hint { tag, id });
        self.programs.truncate(MAX_PROGRAMS);
    }

    /// The hints as the file holds them.
    pub fn to_bytes(&self) -> Result<Vec<u8>, OciError> {
        serde_json::to_vec(self)
            .map_err(|e| OciError::Broken(format!("encoding the device program hints: {e}")))
    }
}

/// A loaded program as the kernel describes it: its identity, and its translated instructions as the
/// kernel dumps them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub identity: ProgramIdentity,
    pub insns: Vec<u8>,
}

/// Whether `theirs` may be attached in place of `mine`, the copy `create` just loaded: another program
/// (not `mine` itself) of the same type, tag and lengths, whose translated instructions are `mine`'s
/// byte for byte. The tag is 64 bits of a digest, so equal tags alone don't prove one program; equal
/// instructions do, and a device program decides from nothing else.
pub fn same_program(mine: &Loaded, theirs: &Loaded) -> bool {
    let (a, b) = (&mine.identity, &theirs.identity);
    b.id != a.id
        && b.prog_type == a.prog_type
        && b.tag == a.tag
        && b.jited_len == a.jited_len
        && b.xlated_len == a.xlated_len
        && !mine.insns.is_empty()
        && theirs.insns == mine.insns
}

fn hex(tag: &[u8; 8]) -> String {
    tag.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(id: u32, tag: u8) -> Loaded {
        Loaded {
            identity: ProgramIdentity {
                prog_type: 15,
                id,
                tag: [tag; 8],
                jited_len: 120,
                xlated_len: 16,
            },
            insns: vec![0x61, 0x12, 0, 0, 0, 0, 0, 0, 0x95, 0, 0, 0, 0, 0, 0, 0],
        }
    }

    // do X: remember programs for tags, then ask for each; expect Y: the latest id per tag, newest
    // first, one entry per tag (decision 2: one program per distinct device policy).
    #[test]
    fn a_tag_names_the_program_remembered_last_for_it() {
        let mut h = Hints::default();
        h.remember(&[1; 8], 10);
        h.remember(&[2; 8], 20);
        h.remember(&[1; 8], 11);
        assert_eq!(h.candidate(&[1; 8]), Some(11));
        assert_eq!(h.candidate(&[2; 8]), Some(20));
        assert_eq!(h.candidate(&[3; 8]), None);
        assert_eq!(h.programs.len(), 2);
        assert_eq!(h.programs[0].id, 11, "the last remembered comes first");
    }

    // do X: remember one more program than the bound; expect Y: the bound holds and the one
    // remembered longest ago is the one dropped (CLAUDE.md §2).
    #[test]
    fn past_the_bound_the_oldest_program_is_dropped() {
        let mut h = Hints::default();
        for n in 0..=MAX_PROGRAMS {
            let n = u8::try_from(n).unwrap();
            h.remember(&[n; 8], u32::from(n));
        }
        assert_eq!(h.programs.len(), MAX_PROGRAMS);
        assert_eq!(h.candidate(&[0; 8]), None, "the oldest is dropped");
        assert_eq!(h.candidate(&[1; 8]), Some(1));
    }

    // do X: parse what remember wrote, and hostile bytes; expect Y: the same hints back, and no hints
    // (never a panic) for anything that isn't a hints file (CLAUDE.md §6: hostile input to every
    // parser). A file past the bound is cut to it.
    #[test]
    fn hints_round_trip_and_hostile_bytes_are_no_hints() {
        let mut h = Hints::default();
        h.remember(&[0xab; 8], 7);
        assert_eq!(Hints::parse(&h.to_bytes().unwrap()), h);
        for bytes in [
            &b""[..],
            b"\xff\xfe",
            b"{",
            b"[]",
            b"{\"programs\":5}",
            b"{\"programs\":[{\"tag\":1,\"id\":\"x\"}]}",
        ] {
            assert_eq!(Hints::parse(bytes), Hints::default(), "{bytes:?}");
        }
        let deep = format!("{}{}", "[".repeat(10_000), "]".repeat(10_000));
        assert_eq!(Hints::parse(deep.as_bytes()), Hints::default());
        let long = Hints {
            programs: (0..200)
                .map(|n| Hint {
                    tag: format!("{n:016x}"),
                    id: n,
                })
                .collect(),
        };
        assert_eq!(
            Hints::parse(&long.to_bytes().unwrap()).programs.len(),
            MAX_PROGRAMS
        );
    }

    // do X: match a candidate against the copy just loaded; expect Y: only another program of the same
    // type, tag and lengths, with the same translated instructions, is taken, and never the copy itself
    // (decision 2: a shared program decides for each container as its own copy would).
    #[test]
    fn only_another_program_with_the_same_instructions_is_taken() {
        let mine = loaded(5, 0xaa);
        assert!(same_program(&mine, &loaded(3, 0xaa)));
        assert!(!same_program(&mine, &mine), "the copy itself");
        assert!(!same_program(&mine, &loaded(3, 0xbb)), "another tag");
        let mut other_type = loaded(3, 0xaa);
        other_type.identity.prog_type = 1;
        assert!(!same_program(&mine, &other_type), "another type");
        let mut other_len = loaded(3, 0xaa);
        other_len.identity.xlated_len = 24;
        assert!(!same_program(&mine, &other_len), "another length");
        // The tag collides, the instructions don't: one immediate differs.
        let mut collided = loaded(3, 0xaa);
        if let Some(b) = collided.insns.get_mut(4) {
            *b = 1;
        }
        assert!(!same_program(&mine, &collided), "a tag collision");
        let withheld = |mut l: Loaded| {
            l.insns.clear();
            l
        };
        assert!(
            !same_program(&withheld(loaded(5, 0xaa)), &withheld(loaded(3, 0xaa))),
            "no dump on either side is no proof"
        );
    }
}
