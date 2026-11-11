# Linux whole-owner cold native metadata barrier

L1 implementer, current policy7d7e0855a5073ed5163b4fe7ed3355c9ed7c2506,
byte-equal to fully read0a6d91b3de0f10a8397868aa3794adab1e09afdc/rules
df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.
New required cold recovery behavior at the existing Node/Store/Supervisor seam;
SOURCE ONLY, no compiler/test/native job dispatched. No ordinary Linux lifecycle,
process, vfork, sys, cgroup parser or Stat::of behavior is changed here.

## State and ordering

Node::preflight_configured loads the SAME retained Box and Pool cohort, calls L3's
common structural barrier, checks EVERY initialized backend configuration, then
checks EVERY managed native owner and scope name. Only after success may the
existing install path initialize the native root lease/cgroups, KVM, prepare a Pool,
query backend capacity or enter recovery cleanup. Weak Books publication remains
pure owner publication. Original spec/class/route/allocation are retained.

The temporary managed slice borrows canonical NativeRecord values and uses the
SAME Node::managed_bounds allocation formula as native launch. CPU-only shared
inner workloads do not acquire another native scope; the actual Pool is included.
Unallocated Prepared/Released backings can have no native directory. No synthetic
Box, record, holder, index, second native owner or cleanup manager is introduced.

Supervisor briefly snapshots SAME managed TriArc receipts under its existing init
table mutex. No Store/native I/O, watch, reservation, adoption, reap, process spawn,
termination or cleanup occurs under that mutex. Existing owner/scope/immutable
commitment and persisted PID/birth must agree. Temporary scopes and owner references
sort once; duplicate scopes refuse and owner lookup is binary, not an all-pairs
scan. These temporary vectors are bounded/fallible, not persistent caches.

A readonly NOFOLLOW native-root listing must contain only canonical scope names
and normal interface files. Unmapped directory/symlink/type/read/overflow refuses
the complete barrier before effects. Each expected scope is opened through L3's
fallible readonly observe_managed and stays pinned. Its actual finite CPU/memory/
swap limits and complete positive PID membership use L3's SAME strict readers.
An unexpected delegated child cgroup refuses rather than allowing an incomplete
root-only member list. Controller/remaining helpers must be fair-class tasks covered
by cpu.max. The exact original PID birth and member placement are observed through
bounded /proc stat reads; an active controller outside the scope refuses. A stopped
or exited controller with remaining helpers stays uncertain. Empty/missing scope
metadata permits later terminal observation, never releases any claim here.

The path must still name the same pinned device/inode. Missing pathname cannot
hide a scope still present under the pinned root. Current native names/limits are
observations, not an atomic cross-file snapshot or kernel immutability proof.
Later native operations must retain exact scope/birth/receipt guards; this source
must not be reported as full fault/escape/terminal proof merely from pure controls.

## Primary sources and bounds

Linux v6.19 exact05f7e89ab9731565d8a62e3b5d1ec206485eeb0b:

- fs/proc/array.c do_task_stat emits52fields; each integer fits signed/u64 decimal
  plus separator, proc_task_name buffer64 and fixed punctuation. A stack frame
  derives the stat byte bound from those fields, plus one overflow detection byte.
  PID/u32 uses at most10 decimal digits in a fallibly reserved path. Native numeric
  suffix encoding, truncated/extended layout or incomplete newline refuses, never defaults.
  Task command bytes need not be UTF8 and cannot change the numeric identity suffix.
- fs/readdir.c verify_dirent_name rejects names >=PATH_MAX4096. A fixed getdents64
  frame includes that path bound, two u64s/u16/u8 fields and alignment headroom.
  Listing budget permits MAX_BOXES scopes, independently MAX_BOXES interface files
  and two dot entries, then refuses; it never grows a filename buffer without bound.
  A new open file description reads each complete directory from offset0 without
  altering a shared pin's offset or introducing a retained directory cache.
- include/uapi/linux/limits.h and include/linux/sched.h supply the physical limits;
  Linux UAPI scheduler policy0/3/5 are NORMAL/BATCH/IDLE fair-class values.

Exact raw files/hashes live in
~/.cache/hex-complete-bindings/linux-managed-preflight-primary-v1/manifest.json.
Initial web-tool primary retrieval returned a token_revoked401; that failed call
was not bypassed with credentials/runner changes. Direct public GitHub raw source
was read anonymously and its exact pins/hashes retained.
L3 LINUX-MANAGED-METADATA.md pins cgroupv2 limits, membership and uncertainty.

Dependencies: signedfa3535 strict managed_procs/check_managed_bounds;
signedc90a41cdf07fbabfa51cfd73966dd491ea037fbe fallible readonly pin;
signedf4394fd1f66b1304aa52ee7b90d70cfbf03bce16 strict managed_populated.
No source stub, cgroup DTO/parser duplication or dependency version is acceptable.

## Five focused controls

All SOURCE ONLY/UNRUN. Private regular-file fixtures or injected process facts,
no real /proc PID query, cgroupfs mutation, child, KVM/VM/GPU/HCS operation:

- later_unmapped_native_scope_stops_the_readonly_whole_owner_barrier:
  valid earlier scope plus an unmapped later native name refuses before callback;
  original CPU metadata/owned directory remains byte-identical/present.
- retained_native_limits_members_and_leaf_scope_are_required_before_adoption:
  exact native limit succeeds, enlarged memory/missing swap/unowned membership or
  delegated child scope refuses. Fake membership with no canonical init refuses
  before any process lookup.
- controller_exit_and_reused_birth_do_not_authorize_live_helper_cleanup:
  exact controller+helper accepted; controller gone with helpers, RT policy,
  wrong birth or an escaped live controller refuses. Empty scope/exited zombie is
  distinguishable but grants no physical release; Released cannot be populated.
- prepared_and_duplicate_incarnations_cannot_absorb_native_metadata:
  Prepared scope absence succeeds, existing scope or duplicate canonical generation
  refuses. No fresh generation or stale workload label can replace the scope.
- native_stat_command_bytes_do_not_replace_the_exact_birth_and_policy:
  arbitrary non-UTF8/parenthesized native command bytes preserve exact PID, birth
  and scheduler policy fields; wrong PID/truncated output refuses.

## Rule/evidence accounting

| Rules | Current source and owed validation |
| --- | --- |
| R01 | Temporary vectors/path/string reserves fallible, checked access/math, same TriArc owner; no unchecked new owner after effects. Inherited generic OS error rendering is not certified by this source. |
| R02/R03 | Exact Linux/cgroup contracts above, derived physical/API limits; no guessed timer, memory allowance or tuning constant. |
| R04 | Prepared/Creating/Ready/Deleting/Released native projection, exact PID/birth/scope/commitment; uncertainty never frees claims. |
| R05 | No wait/timer/retry/cache/provider. Synchronous finite listings/proc reads precede effects, no lock spans I/O. |
| R06/R07 | One focused existing Supervisor child and explicit source/safety rationale, no new unsafe. |
| R08/R14 | No performance or allocation-free claim. Root profiles per-record membership/proc cost, allocations, page faults and contention. |
| R09/R10 | Required new behavior, not claimed reproduced ordinary native bug. Five controls unrun; actual baseline recovery reds stay attributed to L3's retained fixtures. |
| R11/R12 | Linux cgroup-specific shell, canonical shared NativeRecord/Store authority; Mac/Win remain L3 adapters. |
| R13 | Same Store, Tables/Turns, native receipts, cgroup readers; no parallel manager or fabricated Box. |
| R15/R16 | Implementer source-only, signed named paths, no jobs or foreign effects; raw/dirty drafts preserved. |

ROOT owes coherent compilation/Clippy, new targeted controls, actual cold same-scope
recovery, partial/escaped helpers, PID reuse/native-name mutation, forbidden-effect
witnesses, membership/limits races, real VM termination/return, supported-platform
and full integration gates. None are implied passed by formatting or this receipt.
