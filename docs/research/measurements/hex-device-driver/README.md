# File-backed Driver device binding fixtures

POLICY_READ: implementer Kiln `01a1c640e6ed710d8a8c919b4aa1fc8c`, coordinator
`01a1aa77778471339bea3beff90d6bbd`. Current Shards origin/main
`cbd39284937d024640c56db6f8d8ef2bd9b0ee24` equals remote main; full current AGENTS,
R01–R16, development-evidence and CLAUDE re-read, plus current g121-handoff and
Hex CLAUDE/HANDOFF. Full managed-runtime-plan at the same cbd was read in the prior
batch and remains current; no Shards production/native dependent change occurs.
Rules SHA256 `df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
Hex policy/handoff SHA256 `8708b80967e5517ae9137eb642600668ac02d164a4c2145ad8462a37889d424e` /
`9b780baaa9ff3dc1d491624d99a217ceb7a2b7cc913c1f56e0598f6d8f8694c1`.
Receipt keys `hex-device-driver-policy-cbd-df2-v1` and
`hex-device-driver-fixed-5e2-policy-v1`; not proof of policy compliance by themselves.

Branch `cp/t7-device-driver` starts at EXACT signed production pin
`5e2cfbe7e09256ff6a7d979564717cfc6d8041f6`. Before receiving that pin, only
read-only policy/ROOT source inspection occurred; no floating-root branch mutation,
source checkpoint, transfer, build or test. Current pinned Driver/devices/shared
Runtime Capacity/DeviceInfo/SimRuntime/Catalog and existing parent Counting/local/
request and recovery cases were read. Source hashes are in pinned-sources.json.
Previous renderer checkpoint4849fe2b/tested source2a059b5c, parserd4d/9cb,
relay20268 and all raw/source/targets remain preserved on their original refs.

Own ONLY NEW `crates/hex-node/src/placement/device_binding_tests.rs` and this NEW
evidence directory. Parent `placement/tests.rs` registration remains ROOT-owned;
REGISTRATION.patch is UNAPPLIED in the actual worktree. ROOT owns production/shared
fixes, API/catalog/OS/native, audits/integration review/all heavy/mutations/VM/E2E/
stress/sweeps/full matrix/profile/benchmark and merge work. No children/goals/plans/
automatic dispatch. Current scope is these three unit/light cases only.

## Existing real seam and model boundary

The file-backed real Driver performs submit/hear/create/recovery/delete using its
existing picker and durable rows. A test-only DeviceRuntime wraps existing Counting
and its unchanged SimRuntime. It retains the EXACT incoming BoxSpec.devices in
in-memory test BoxRecords. It invokes shared Catalog::prepare and Catalog::holder /
Entry::report. Capacity returns only configured keyed IDs and their exact modeled
holders. Claims are extracted before ordinary SimRuntime.create because SimRuntime
correctly refuses physical device claims. All create/start/inspect/exec/delete
observations are model/SimRuntime effects only, never hardware/native attachment.
The trusted synthetic catalog's CDI file path is never opened; Catalog::cdi and
all OS adapters are outside these tests. No parallel picker/production registry is
added. Replacing fixture catalog order models a permitted backend-inventory change;
it does not restart an actual backend or establish hardware availability.

Reuse existing parent Counting/local/request. Local peer delivery uses its existing
finite records::MOST guard; the receipt log uses the same finite ceiling and never
truncates. Each case makes at most two modeled boxes; parent Budget2/2 is derived
from that concurrent pair. Synthetic resource capacity is four request-sized units;
GPU memory is MIN_MEMORY. GPU loads0/1000 represent the documented per-mille idle/
fully-busy extremes. GPU keys0x100/0x200 are distinct valid packed PCI identities.
No new production literal/default/dependency/API exists. Ordinary data operations
are bounded and synchronous; no sleeps, polling harness, native subprocess, socket
or new wait deadline occurs inside the leaf.

## Three cases and expected witnesses

1. Two keyed GPUs with unequal loads: the initial receiver chooses GPU_A. Reboot
   with reversed catalog/node inventory and swapped loads preserves exactly GPU_A,
   original physical BoxId and durable binding row with no second create/start.
   Fresh work gets GPU_B; shared Catalog holder reports match both exact claims.
   Delete both: model boxes/holder records/admission/owned rows empty, epoch only.
   Final expected create/start/delete counts2/2/2, exact keyed create receipts.
2. Individually valid caller-forged BoxSpec.devices is refused as typed Invalid
   before any durable file byte/name change or Runtime call (including probes).
   A legitimate GPU request still gets the intended ID and sequence0; a CPU sibling
   progresses via a simulated say program. Delete both before final assertions:
   boxes/claims/admission empty, epoch only, counts2/1/2 and exact create receipts.
3. Existing fail-start/fail-delete controls leave one modeled GPU init Created with
   its claims retained. Change ONLY its in-memory reported BoxSpec claims to the
   other GPU. Recovery must return typed Core(Damaged) before any durable file
   writes or init restart/create/delete; bytes/names and effect counters unchanged.
   An independent CPU Driver/namespace on the same model Runtime progresses while
   the target remains Created. Repair exact metadata, recover the same key/ID/claim
   with no second GPU create and one authorized start, then delete CPU/target before
   final assertions. Expected total2/2/3 includes the deliberately failed initial
   deletion; start outcomes are exact start refusal then success. Both ledgers end
   epoch-only, both admissions zero, model boxes/claims empty.

All meaningful final checks follow modeled cleanup; setup unwraps/panics still
fail the test normally. TempDir drops own file-backed ledgers; no native process
or device fixture exists to clean. The leaf logs explicit hardware_effect=false.
Existing standalone peer tests already cover pure held-index recovery; these three
add the actual Driver/file/Catalog/Runtime boundary and no duplicate selector.

## Verification status and authorized checks

SOURCE ONLY at this first checkpoint:0 executed/0 passing claims, leaf unregistered.
After source-first signing, one NEW private d1323898b255a3ed435eb83fa0f7b0cf job:
cpus1/mem4g/disk12g, nice10, jobs1/testthreads1, fresh source/target/cache, precise
--no-sync transfer. Apply only the retained parent registration in that PRIVATE copy;
record before/after hashes. Compile the whole node class path if required, execute
ONLY these three exact leaf tests (nonzero list/count check), run node lib/test
Clippy and scoped leaf rustfmt. No unregistered/zero-filter pass. A ROOT compile
failure is reported promptly with raw source/log hashes; no shared fix in this lane.
Actual result/log/source hashes and cleanup will follow in RESULTS.md.

ROOT still owes independent fixture review/parent registration, combined/native
hardware attachment/holders/effects/cleanup, seven targets and all heavy/full gates,
mutations/negative campaigns and performance/resource evidence. These model tests
cannot establish live device isolation or complete the live scheduling requirement.


Checkpoint update: actual r20 at signed6483c6f2 on exact5e passed all3selected
model/SimRuntime cases,0failed/0ignored, build/node lib+testClippy/scopedfmt0.
Actual raw/hash/source/private-registration/cleanup readback is in RESULTS.md.
The source-first0execution statement above describes its original checkpoint.
No additional case/campaign or production correction was needed.
