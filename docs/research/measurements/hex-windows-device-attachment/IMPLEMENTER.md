# Windows exact device attachment — source checkpoint

Implementer01a1c643e475709b8a829553d1100be1, coordinator01a1aa77778471339bea3beff90d6bbd.
Branch cp/t7-windows-device-attachment starts at exact signed dependency composition
`a03657d18add6343b9b5d9da55c7aefdc71a10c9`, parent API evidence8c4777d6, whose
source0429d084 was actually tested. API/HCS/bounds refs and original raw evidence
are preserved. Full current-policy readback cbd39284 / rulesdf2acf47 and exact owned
paths are in policy-read.json. This checkpoint is SOURCE ONLY: zero tests executed
in this scope. No local builds/tests or active remote jobs.

## D-Windows-binding: one native path and one holder owner

The real Windows CLI constructor opens the shared administrator catalog at
`args.state.join("devices")` before creating a keeper. ROOT owns Catalog/Entry/
Binding and registration; this branch neither edits nor duplicates their parser.
ROOT separately reported its5 catalog units/runtimeClippy0 on exacta036, not my run.

The native adapter maps DeviceId to configured Binding::Hcs LocationPath, appends
DeviceInstance values in the existing container(spec), and calls the same OciBox
create/HCS document operation. No class-wide, GpuMirror or all-device fallback is
added. Unknown/CDI selections and process-device claims refuse before native
creation. Existing empty-claim constructors/JSON remain compatible. Windows process
and OCI reports share one actual CPU/memory/disk pool identity; only OCI offers the
configured HCS devices. Entry::report supplies current holders fallibly.

Primary source: [HCS Device/DeviceType schema](https://learn.microsoft.com/en-us/virtualization/api/hcs/schemareference#device)
distinguishes DeviceInstance/LocationPath from ClassGuid/GpuMirror and introduces
the exact form in2.2; the existing e273/d71 renderer selects that minimum. The
[terminate reference](https://learn.microsoft.com/en-us/virtualization/api/hcs/reference/hcsterminatecomputesystem)
distinguishes operation start from successful operation result. Existing bounded
open_system/terminate/end calls are reused; no sys/unsafe/WMI/hardware probe or
wait-limit change. Source observations of old failure handling are not reproduced
native defects or reasons for a generic syscall rewrite.

Existing Holding.boxes remains the sole native ownership table. Its explicit states
are Creating, Cancelled, Finishing, CancelFinishing, Ready, Removing,
CleanupPending, Uncertain and RemovingUncertain. Same-ID and physical overlap
refuse before OS create. A delete while creating requests cancellation and returns
pending, never fabricated completion or NoSuchBox. Pending/uncertain/deleting IDs
remain reported as held. Successful physical cleanup precedes slot removal. OciBox
keeps the exact native binding metadata; report checks it agrees with retained IDs.
Keeper receipts are kept before their file write, and a close error becomes explicit
uncertainty rather than a blind repeat close of a potentially recycled value.

To make new pre-effect reservation fallible, the existing table uses a single
sorted Vec bounded by existing MAX_BOXES, with binary-search lookup, checked insert
preconditions and try_reserve before insertion. No second manager/index/cache was
added. Insert/remove shifts are O(boxes); overlap is O(retained IDs * log(selected))
after temporary selection sorting. Catalog selection uses a fallibly reserved
sorted reference vector, O(catalog log catalog + selected log catalog). Holder
report builds a bounded temporary sorted ID/reference vector and preserves catalog
order. These are source complexity statements, not measured performance claims;
ROOT still owns profiling/benchmarks and comparison evidence.

## Exact native marker and recovery

The existing HCS BoxRecord scratch marker appends default/skip-empty devices,
containing each selected DeviceId and the exact opaque LocationPath. Its known
compute-system ID is written with those bindings BEFORE HCS create, including an
assigned box otherwise ending with its node. No side marker/store is introduced.
OciBox create failure does not infer cancellation/absence from Err or handle drop;
unknown scratch/claims stay retained for explicit existing marker/end cleanup.
System and Mounted metadata are cleared only after successful operations, so a
failed termination/detach/removal remains retryable ownership rather than empty.

Adopt/take-back compares actual BoxSpec.devices, exact recorded bindings and current
catalog mapping before native adoption. Missing/changed mapping refuses with bytes
preserved; no re-pick or replacement grant. A rejected recovered marker retains
its known physical IDs in the holding table. Runtime pending/unknown entries never
report NoSuchBox. Missing-marker cleanup requires an actual existing HCS open result
of absence; it does not treat a missing file as physical absence. ROOT owns Recorded
Creating retention and integration, not this branch.

Legacy MAX_RECORD remains the existing path/JSON-derived bound. New record_bound()
checked-adds existing MAX_CONFIG_BYTES. The COMPLETE new JSON devices field shares
that configuration budget; the old path portion retains its own old budget. A
fallible bounded JSON writer prevents encoding beyond the combined ceiling; reads
use the checked ceiling and validate the same field budgets. Empty legacy output
is unchanged. New pure fixtures pin exact combined/individual boundaries, malformed
identity/location/duplicates and redacted diagnostics. No arbitrary new limit or
location-path grammar is introduced.

Compatibility limit: device-bearing Scratch::Volume is explicitly refused. Its
borrowed-volume ownership is not represented by the existing scratch marker, so
this slice does not invent an implicit cleanup rule or detach an unrelated volume.
Ordinary empty-claim volume behavior remains supported. Exact assignment currently
uses owned scratch folders. Configured HCS bindings are GPU-only by ROOT's catalog
contract; other native combinations remain typed unsupported. Actual HCS semantic
and physical location validation is ROOT-owned and unverified here.

## Planned light evidence and R01–R16

Five new portable marker tests and six Windows tests cover the actual data/planning/
holding seams. Five Windows cases are pure/data/private-file fixtures with a
constructor that issues no WMI or HCS calls; the sixth is one ordinary empty native
job create/delete, with no guest/container/GPU. All actual selection/counts will
be recorded from raw output. Expected focused d132 checks: HCS units, core native
Clippy and Windows compile/Clippy only if installed target/toolchain support exists;
Jupiter may run only the six named Windows cases under an admitted private envelope.
No HCS/GPU/VM/E2E/heavy/matrix/mutation/audit/profile/benchmark campaign is authorized.
No ROOT catalog test is rerun solely to collect its existing result.

R01/R03/R04/R06/R07/R13: checked count/bound conversions, fallible new buffers/copies,
explicit ownership/uncertainty and no panic allowances; existing broader allocation
paths are not certified panic-free. R02/R05/R12: one existing native seam, shared
catalog, primary HCS contracts, no new stored cache/wait/picker; existing feature
query cache/keeper waits are unchanged. R08/R14: no comparative/latency/allocation-
free/leak-free claim; ROOT measurements owed. R09/R10: protocol feature addition,
source observations separate from native reproduction; source/raw revisions stay
explicit. R11: only focused compile/native ordinary checks are planned, all other
supported targets/real HCS/hardware/guest matrix unverified. R15/R16: implementation
and light checks only, no children/goals/plans, fresh private runner source/target/
cache and precise no-sync transfer, local >=30GiB checked before bulk transfer;
all prior originals preserved. ROOT owns actual attachment/fault/native cleanup,
meaningful mutations, combined review/gates/matrix, benchmarks/profiling and merges.

## First light run and source successor

Exact147312f7 on private d132 executed HCS48pass/0fail/0ignored/282filtered,
including all5 new marker fixtures; core all-target native Clippy0. Windows target
std was already installed. Windows runtime all-target Clippy reached the owned
code then failed101 on two lints: an unused WinRuntime::keeping forwarding method
and collapsible-if. CLI compile and Windows native units did not run. Raw originals
are preserved; no lint suppression or larger resource/wait limit was added.
HCSlog f72e30ce291ce67ad2fe2a4c810f1bae24e56b001fc298647a42f52d95e91e14;
coreClippy fc8c7dfb3905282ccb05a3591a9d8f616683de754339910423fa42b83ba5efbb;
WindowsClippy c30f95794d0c93f61593bde130157dd571e6aca2447e158e72c685baeed7917c.
Recorded cargo31270/31579/31619 and localCLI40833 returned/reaped, temp0.

The successor removes that unused wrapper, collapses the conditional, and makes
rejected recovery retain the union of declared and native-recorded IDs (temporary
count at most the sum of the two existing bounded lists), so absent/contradictory
native metadata cannot turn a declared claim into free capacity. The same recovery
fixture adds that assertion without new native effects. Marker/core sources remain
byte-identical to the48/0 run; next d132 check targets only changed Windows compile/
Clippy, followed by six selected Jupiter Windows cases if compile succeeds.
A read-only Jupiter preflight observed WindowsX64 and existing1.99.1MSVC installed;
no native test has run yet, and the default outside-worktree1.99.0 is not the pinned
check toolchain. No installer/upgrade/service or paused VM was touched.
