# Shared runtime device contract — source checkpoint

Implementer `01a1c643e475709b8a829553d1100be1`; coordinator
`01a1aa77778471339bea3beff90d6bbd`. Branch `cp/t7-runtime-device-contract` starts at
exact `d71a87ff3dee68913a622988df4aac2ed6a26b67`. Prior bounds7d and HCS e273/d71
branches/raw evidence are preserved. This is the assigned AC-T7.3.1/AC-T7.4.1
protocol feature slice, not a claimed reproduced native bug or attachment feature.

Full policy/handoff readback and exact owned source paths are in `policy-read.json`:
Shards current origin/main `cbd39284937d024640c56db6f8d8ef2bd9b0ee24`, rules
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
The source-first checkpoint has **zero executed selected tests**. Scoped rustfmt and
source `git diff --check` passed. No laptop builds/tests, children, goals, VM/native
probes, mutations, benchmarks, profiling, audits, integration gates or merges.

## D-contract: facts, claims, refusal

The user fixed the physical identity contract. `DeviceKind` has lowercase serde
names gpu/vfio/block/net. Copy/hash/ordered `DeviceId { kind, key }` checks GPU
48-bit packed domain32/bus8/device5/function3, VFIO isolation group u32, block
major32/minor32 pair64 and nonzero net ifindex u32. There is no existence or
attachment probe in this pure API. `DeviceInfo { id, model, size, holder }` checks
its ID; model remains opaque metadata and no size floor is invented. The methods
`DeviceId::check`, `DeviceInfo::check` and `DeviceKind::name` are available to ROOT's
real catalog consumer, with no dependency on sim. The catalog's new node devices.rs
and sim model/index/peer files remain untouched.

Primary sources read for the representation's distinctions:

- [Linux v6.19 PCI UAPI](https://raw.githubusercontent.com/torvalds/linux/v6.19/include/uapi/linux/pci.h)
  defines the 5/3 slot/function bits; [PCI header](https://raw.githubusercontent.com/torvalds/linux/v6.19/include/linux/pci.h)
  defines bus/devfn composition. Domain32 is the requested transport contract,
  not a claim that every OS offers that domain range.
- [Kernel VFIO documentation](https://docs.kernel.org/driver-api/vfio.html#groups-devices-and-iommus)
  identifies the isolation group as ownership granularity. PCI function identity
  would falsely imply assignability within an inseparable group and is rejected
  as the VFIO representation.
- [Linux man-pages 6.19 major/minor](https://man7.org/linux/man-pages/man3/major.3.html)
  exposes unsigned major/minor components; our high32/low32 transport pair is not
  the host's opaque dev_t binary layout.
- [Linux man-pages 6.19 if_nametoindex](https://man7.org/linux/man-pages/man3/if_nametoindex.3.html)
  returns unsigned interface indices and uses zero for failure. Physical existence
  and namespace identity remain ROOT's real catalog/backend responsibility.
- The exact base's `hex-sim/src/model.rs` has `DEVICES = 1 << 16` and u16 device
  indices. The new API independently derives `MAX_DEVICES = 1 << u16::BITS`;
  65,536 entries are accepted, with no new tuning constant or sim dependency.

`BoxSpec.devices` and `Capacity.devices` default to empty and serialize without
that field when empty. Old JSON round-trips byte-for-byte in focused fixtures.
Nonempty data makes claims explicit; capacity metadata grants no physical work.
Duplicate validation fallibly reserves exactly n DeviceId slots for n <= 65,536,
checks IDs, sorts that temporary copy in place with sort_unstable and scans adjacent
identities. Time O(n log n), temporary memory O(n * size_of::<DeviceId>()), no stored
state/cache; original order remains unchanged. Same numeric keys of different
kinds are distinct. Invalid-key/count/duplicate reasons name the field and cause
without the key/model/holder/private content.

The requested feature-path `BoxId::try_clone` reserves its current validated
String length fallibly and constructs Self directly. It returns fixed typed
Exhausted("box id", length); new/TryFrom/Clone are unchanged. The success fixture
checks exact identity. This does not establish universal allocator-failure safety.

## Wire and compatibility

Existing shared request/answer kinds remain fixed. Appended device lists use a
u32 count, explicit kind tags 0/1/2/3 (gpu/vfio/block/net), LE u64 key; offers then
carry model string, u64 size and optional holder BoxId. This is canonical field
encoding while preserving catalog order, since scheduler indices refer to it.
A reader checks count <= 65,536 and checked count * minimum item bytes against
remaining bytes BEFORE visiting or reserving elements. Each item is decoded and
checked before fallible try_reserve(1) and push; no full caller-selected list is
reserved up front. Vec growth is its standard amortized growth, without a new
tuning constant. Claims need at least 9 bytes/item;
offers at least 22 (9 identity + 4 string length + 8 size + 1 holder tag).
Decoded element count is bounded by min(65,536, remaining/minimum). The temporary
API duplicate-validation copy reserves that many DeviceId slots at most. Model String
reservation is also fallible. Unknown tags, invalid IDs, duplicates and malformed
holder/model input refuse by Fault; new diagnostics redact private content.
Existing unrelated codecs and refusal-name interning are not rewritten.

IDENTITY now includes both devices.rs and runtime.rs, in addition to the prior
inputs. The existing digest test mutates each source independently. Wire bytes and
identity intentionally change; old peers must fail the existing identity handshake,
not receive an empty-device downgrade. JSON empty-field compatibility is separate.

## Guards and mechanical additions

`BoxSpec::check_no_devices(platform)` calls check, then typed Platform for nonempty
claims. Create/run entrypoints use it before effects in SimRuntime, Linux Node,
MacRuntime, WinRuntime, GuestRuntime and WithGuest. Recorded's entrypoints also
refuse before Creating-record writes. Existing run-class checks remain. Guest
rewrite carries the devices field unchanged rather than dropping claims. Current
backends therefore do not pretend schema support means attachment support.

`empty-literals.json` lists all 164 necessary pre-device literal additions and
52 actual paths. `add_empty_fields.py` records that mechanical operation from the
fixed pre-device source; it skips unrelated sim Capacity, definitions, function
return types and struct updates. No benchmark/test behavior changes beyond empty
fields in those files. Full modified source paths/hashes are in source-hashes.json.

## R01–R16 and owed coordinator work

R01/R03/R04/R06/R07/R13: new key/count checks, fallible validation/reader reservations,
explicit tags, typed refusal and simple bounded paths; source review is not an
allocator proof. R02/R05/R12: one existing API/wire seam, primary sources and fixed
user contract above, no new manager or dependencies. No new wait/cache/polling path;
existing name interning and backend waits are unchanged. R08/R14: no performance,
allocation-free or leak-free claim; coordinator owns measurements. R09/R10: feature
addition, no native-red claim; signed source and later exact light raw results
remain distinct. R11: portable pure data code, only native ARM64 Mac light checks
planned; seven-target/real supported Windows/Linux/guest/HCS/hardware matrix unrun.
R15: implementation/light only; ROOT owns combined integration, actual attachment,
mutation/native effects, gates/audits/profiling/benchmarks and merges. R16: own new
private runner source/target/cache only, no other lane/root data touched; local free
space checked before transfer, no active job at this checkpoint.

Planned focused checks: hex-api library units (including four device and one
try_clone fixture), hex-core wire units (including four device codec fixtures),
native hex-api/core all-target Clippy, nice10/jobs1/threads1 on approved
`d1323898b255a3ed435eb83fa0f7b0cf` only with 1CPU/4GiB/12GiB and precise no-sync
transfer. ROOT owes native unsupported-backend durable/physical effects, catalog and
scheduler binding/recovery, real attach/detach and cleanup, mutation guards, seven
targets/full gates, real Windows/HCS/hardware and broader final matrix. No schema or
unit result closes those requirements.

## First light run: retained fixture error, successor source

The exact source-first 441ceb27 private v1 compiled API tests and observed the new
legacy-JSON fixture fail: Bounds already serializes absent options as explicit
null fields, whereas my expectation used {}. The successor corrects that expected
legacy encoding only; production Bounds/serialization are unchanged. The original
run/log remains separate, and no green count is claimed until raw terminal output.
Its complete API unit selection includes existing differential suites; the next
check selects only devices:: and the exact try_clone test, plus the assigned wire
units/Clippy, to keep validation focused. It does not turn incomplete old-suite
results into passing results.

A source review also tightened the NEW device reader to decode each item before
fallible try_reserve(1), rather than reserving the entire validated count first.
Count/minimum-byte checks still precede all element visits; standard Vec growth
adds no tuning constant, cache or stored state. Invalid first tags/IDs therefore
refuse before a list allocation. This is source hardening of the new protocol
feature, not a reproduced allocator/native bug or a production-limit change.

### Actual v1 terminal record

Exact 441ceb27 API units finished **34 passed / 1 failed / 0 ignored / 0 filtered**,
exit101 (outer1). The single failure was the expected-encoding fixture described
above. Wire/API-Clippy/core-Clippy were not run. Raw API log SHA256
`3fb7bb7788b086719ee3eb28e4422ece38e4d23f8397f255f9ea77a9a5799440`; collection archive
`f6ae2de08c9f679f4e83fe8307902bd60eb38251e6a5afc5d76a8f45ba2ec2b4`.
All 62 source hashes match the exact signed 441ceb27 manifest. Local CLI15482 and
recorded cargo22548 returned/reaped; private temp entries0. The private source,
archive, target, cache and raw originals are retained inactive; no deletion.
This is a failed feature-fixture check, not a reproduced native device defect.

## Final exact focused light result

Source `0429d0849e6661a6f2c78d9d1ef68526edc3e115` was actually checked on approved
d132 macOS26.4/ARM64, rustc/cargo1.99.1, nice10/jobs1/threads1, 1CPU/4GiB/12GiB.
`light-v2-result.json` records the exact commands, selected names, toolchain and
source hashes; raw logs are retained byte-for-byte.

| Phase | Passed | Failed | Ignored | Filtered | Exit |
| --- | ---: | ---: | ---: | ---: | ---: |
| API devices:: | 4 | 0 | 0 | 31 | 0 |
| API exact try_clone | 1 | 0 | 0 | 34 | 0 |
| Core wire:: | 28 | 0 | 0 | 289 | 0 |
| API all-target native Clippy | — | — | — | — | 0 |
| Core all-target native Clippy | — | — | — | — | 0 |

All nine new feature tests were selected. These are 33 distinct selected unit
tests, not a final full API/core/workspace suite. No filtered test is claimed
passed. The first run's 34/1 result remains attributed only to 441ceb27. The four
new wire tests cover exact tags/order/round trips, invalid keys/duplicate IDs,
count/minimum-byte refusal before element visits, and redacted unknown-tag/holder/
UTF-8 diagnostics. The digest test passed with devices.rs AND runtime.rs.

The actual executed effect witness is SimRuntime: valid device claims refuse as
Platform before a simulated box appears, then legitimate empty-claim create/delete
returns the simulated list to empty. It is not a physical device or native Runtime
attachment witness. The native/guest/Recorded guard ordering and mechanical
literals outside API/core remain SOURCE ONLY until ROOT compiles/exercises them.
No device existence, native attach/detach, actual VM launch or hardware behavior
is established by these JSON/wire/data checks.

All 62 modified source hashes match the exact remote archive and local candidate.
Current source scoped rustfmt-check0; source-only whitespace check0. No source
changes followed the tested0429 commit; subsequent commits collect evidence only.
`final-readback.json` retains final source/log hashes and a source-derived wire
identity (not an extra native invocation). Collected raw archive SHA256
`61952d0f439e8fb8830f6a2183d9dd9e550880fddec8d57c4dbf5dc67dbabd7c`.

Local CLI42279 returned0. Recorded cargo PIDs23775,24114,24117,24207,24223 were
waited/reaped by the owned driver; temp entries0. Both private v1/v2 source, target,
cache, archives and original raw logs are retained inactive. No owned live test
job or guest remains. No cleanup deletion or other lane/root data touched.

ROOT still owes actual backend/guest durable and physical effects, catalog and
scheduler binding/recovery/integration, attach/detach and cleanup, meaningful guard
mutations, supported seven targets/full gates and real Windows/HCS/hardware/guest
matrix, plus any profiling/benchmarking/aggregate review/merge. Source schema and
focused tests do not complete the full live device feature. This implementation
batch returns its tested checkpoint; it does not declare the adversarial lane or
full scheduler goal complete.

Raw evidence whitespace result: staged git diff --check reports exit2 with six
exact warnings, retained in raw-whitespace-check.json. Two trailing spaces come
from the original failing test's interrupted libtest line in v1 API/CLI logs; four
raw logs end with an extra blank line. Original bytes and hashes remain unchanged,
including the initial whitespace-check diagnostic in the private transfer folder.
These are recorded warnings, not a passing check or a blanket ignore rule.
Authored evidence/source whitespace checks are separate and pass.
