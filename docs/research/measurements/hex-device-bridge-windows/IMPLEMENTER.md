# HCS precise device-instance data adapter

Implementer: Kiln `01a1c643e475709b8a829553d1100be1`; coordinator Hex
`01a1aa77778471339bea3beff90d6bbd`. Branch cp/t7-hcs-device-instance from fixed
`37021c35e64b2a8bab1d35f20f92a0042d874a44`. Prior bounds branch remains at signed
`7d7307359befabec3d87a611fad45e6d19752762`, with all originals preserved.

## Policy and ownership

Read full current Hex CLAUDE.md/HANDOFF-codex and g121-handoff; fetched Shards
origin/main `cbd39284937d024640c56db6f8d8ef2bd9b0ee24` and read full AGENTS.md,
development-rules R01–R16, development-evidence, CLAUDE and managed-runtime plan.
Rules SHA256: `df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
POLICY_READ stable key hex-hcs-device-policy-cbd39284-df2acf47-v1, receipt seq6189.
Current user authorization acknowledges this scope after readback without another
permission hold. New R15 permits assigned adversarial campaigns, but this explicit
Hex assignment remains implementation and light checks only.

Owned: hcs.rs assigned-device representation/parse/render and associated tests;
new hcs/device_instance_tests.rs; this evidence directory. No CPU/memory, driver,
OS/syscall, BoxSpec/Runtime-method, dependency or other-path changes. ROOT owns
the broader runtime bridge, all native/HCS/hardware effects, audits, mutations,
profiles/benchmarks, integration gates and merge. No children/goals/plans,
laptop builds/tests, VM, host changes or all-device/GpuMirror fallback.

## Primary sources read before code

Pinned hcsshim revision: `7d4e89bc32c031455054426674d6bd7bc03b3e08`.
Read the actual pinned files, not an assumed extension spelling:

| Source | Read fact | SHA256 of pinned raw file |
| --- | --- | --- |
| [internal/uvm/virtual_device.go](https://github.com/microsoft/hcsshim/blob/7d4e89bc32c031455054426674d6bd7bc03b3e08/internal/uvm/virtual_device.go#L19-L25) | Exact extension is vpci-location-path; class/vpci-class-guid and vpci-instance-id are distinct. | 682ebd02d859126cdc35fe162fa21221cda46249d54d585227635a3abcce8b46 |
| [internal/hcsoci/hcsdoc_wcow.go](https://github.com/microsoft/hcsshim/blob/7d4e89bc32c031455054426674d6bd7bc03b3e08/internal/hcsoci/hcsdoc_wcow.go#L491-L513) | The location extension copies the id to LocationPath and selects DeviceInstance; class variants use InterfaceClassGuid. | 3787732253f46220ec9efb838be41b5aa37067c8072d859ad7fe3e1afd238b2f |
| [internal/hcs/schema2/device.go](https://github.com/microsoft/hcsshim/blob/7d4e89bc32c031455054426674d6bd7bc03b3e08/internal/hcs/schema2/device.go) | DeviceInstance, ClassGuid and GpuMirror are distinct forms/fields. | e88bbc1367a69ceb60ce5a4361b26b4c0d8706b9be8636fae355e0dedc7ee77f |

[Microsoft HCS Device](https://learn.microsoft.com/en-us/virtualization/api/hcs/schemareference#device)
and [DeviceType](https://learn.microsoft.com/en-us/virtualization/api/hcs/schemareference#devicetype)
were read on 2026-11-10. LocationPath and DeviceInstance require schema2.2.
The API table describes LocationPath as a string without a lexical grammar.
The hcsshim generated file's API-version header is not used to contradict the
official per-field minimum-version table. Its permissive logging/error examples
are not copied: this adapter's errors omit private id/idType values.

The alternatives are the existing class-wide GUID grant and GpuMirror. Neither
can represent the requested exact binding, and GpuMirror would widen authority.
No different mechanism, OS adapter or extra public runtime interface is needed.

## Initial source-first checkpoint (unexecuted at that point)

The only production source changed is assigned-device code in hcs.rs:
AssignedDevice::{ClassGuid,LocationPath}; typed Container.devices; exact extension
parsing; DeviceInstance/LocationPath rendering; schema minor2 iff an instance is
present. Class and vpci-class-guid still normalize GUIDs and render their existing
Type/InterfaceClassGuid objects with schema2.1; empty/missing devices stay schema2.1
and omit AssignedDevices. This explicitly preserves ordinary legacy output as
requested; it does not establish any new native compatibility claim.

Location values are opaque, nonempty and NUL-free. The duplicate copy is bounded
by existing bundle::MAX_CONFIG_BYTES (the whole config file reader's existing byte
budget), with fallible reservation before copying. This introduces no Windows
filesystem-path grammar, location normalization, new production cap, polling or
cache. A direct Value may reach the individual-string boundary while its complete
JSON exceeds the file reader's limit; the boundary test explicitly distinguishes
these layers. Native HCS still owns semantic/device validation.

Invalid location values, class ids and unsupported/malformed idTypes produce
typed field/cause refusals without echoing their values. Allocation refusal uses
the existing typed Exhausted variant for location-path bytes. Existing error
helpers/CPU/memory and OS callers are otherwise unchanged. The old negative
idType test now uses still-unsupported vpci-instance-id instead of the newly
implemented location extension; its refusal-by-field requirement remains.

Nine new pure tests cover exact instance/schema output, opaque text preservation,
legacy class whole-document golden, mixed forms/order, empty/missing input,
unsupported/malformed kinds, invalid identifiers, byte-boundary and private
diagnostics. Markers are synthetic constants, never real secrets. Existing HCS
tests were read first and remain registered; one expectation now uses the typed
enum. New leaf is registered directly under hcs for nonzero filter selection.

Initial source hashes:

- hcs.rs: `ee7113ef724784a2467f09a0f2af493a488972c8952574543f3e0e59823f5aa9`
- hcs/device_instance_tests.rs: `d62338f42a2e5c1cabc3494befd33ec5112ef259c789d0bb274ee5c740a56533`

Source checks: scoped rustfmt and diff whitespace check exit0. Executed tests0;
no green/red count or native bug reproduction is claimed at this checkpoint.
This is a requested protocol feature addition, not a fix for a reproduced native
failure. Next: only focused HCS units and core Clippy on approved d132 with
cpus1/mem4g/disk12g, nice10/jobs1/threads1, new private source/target/cache and
precise no-sync transfer. No old/root/other-lane snapshot or cache is reused.

## R01–R16 / owed ROOT work

R01: no panic/indexing/unchecked arithmetic introduced; new location copy reserves
fallibly, error literals do not contain input. Existing infallible renderer/JSON
allocation behavior is not claimed solved. R02/R03/R05/R12: pinned primary/API
data contract at existing seam, no broader architecture/default/cache/wait change;
schema2.2 is source-derived and copy bound reuses the caller's existing budget.
R04: two explicit assignment variants, unknown kinds refuse; no fallback grant.
R06/R07/R13: small match/validation and one schema selection, comments cite why.
R08/R14: no cost/performance profiling or claim. R09/R10: source-only status above,
retained command/log/source hashes will follow light execution; no native-bug claim.
R11: adapter is portable JSON/data code; seven-target/real Windows/HCS/hardware
behavior remains unverified. R15: one writer and source-first signed checkpoint,
ROOT owns integrated/native proof. R16: local free34,443,840KiB before work, no
cache deletion; previous source/evidence and branches preserved.

Owed: actual focused HCS units/core Clippy with exact source/log/counts/cleanup;
ROOT runtime-bridge integration, real Windows/HCS/hardware semantic validation,
appropriate owned-effect/mutation checks, supported full matrix/gates and merge.
JSON output alone does not prove device assignment, exposure, isolation or driver
compatibility. Shards policy-check script is not present in Hex and is not claimed
passed here. No full scheduler-goal or broader device-handoff completion claim.

## Executed scoped light result

Source-first signed revision actually checked:
`e273db87b40e2e020d74b8b3231ef75cbbea7e6f`.
Production hcs.rs and the new test leaf retain the exact initial hashes above.
No source change was needed after the check. The private source archive was
6,362,253 bytes, SHA256
`40bfbb4744b9df9a696fbfe8012350cd3efa06c4edf90d85369c1dbf800aece3`.
The exact remote driver is light_check.py, SHA256
`e9c31e5d9f3907717b2846e4ea3edc12c8032b240507bfa5da8332ad95c0c768`.
Its command and precise no-sync stdin transfer are recorded in light-v1-transfer.json;
all source member hashes are recorded in light-v1-source-hashes.json.

Actual runner d1323898b255a3ed435eb83fa0f7b0cf, macOS26.4 arm64;
rustc1.99.1 (58cf8f9c3), Cargo1.99.1 (5f94df478). Admission was cpus1/mem4g/disk12g,
nice10, CARGO_BUILD_JOBS=1/RUST_TEST_THREADS=1. Entirely new private paths:
`/Users/adalundhe/.cache/hex-device-bridge-windows/01a1c643-e273db87-light-v1/`
with source, target, cache/cargo and temp children. Other-lane and ROOT sources,
targets and jobs were not reused or changed. Toolchain binaries were read from
the existing approved pinned toolchain; no install/restart/upgrade occurred.

- `cargo test -p hex-core --lib --locked hcs:: -- --nocapture --test-threads=1`:
  **43 passed, 0 failed, 0 ignored, 270 filtered out**, exit0. All nine new test
  names were actually selected and passed; existing HCS tests also passed.
- `cargo clippy -p hex-core --all-targets --locked -- -D warnings`: exit0 for
  native core targets. This is not a seven-target or workspace gate.
- Both owned Cargo children were waited/reaped (unit PID18683, Clippy PID19012);
  local CLI PID88912 completed. Remaining fixture temp entries=0. No physical
  boxes, OS/HCS test actors, actual device probes or VMs were launched. Original
  archive/source/target/cache/logs remain retained and inactive, not reclaimed.

Exact raw originals were collected once, byte-length/SHA256 verified and preserved:

| Artifact | Bytes | SHA256 |
| --- | ---: | --- |
| light-v1-hcs-unit.log | 6570 | 65758f4ca1c5076443151b16bd88d6fb0d2cfbe1b5eb8978443a79819c0c1a3f |
| light-v1-core-clippy.log | 1249 | 5178e6ef139472e10d88f73c951b8a356b8201bae513738c1ae720518a0442ff |
| light-v1-result.json | 6907 | dee836b8d663d03ff49bd1a5b78f9dcf3bc5c328e538257ddf6d311ea1e81780 |
| light-v1-run.json | 6723 | 5ce86464c4bb24c7f20814750f9c62cad216350a340433609e7d6d60b7b56d12 |
| light-v1-source-hashes.json | 127723 | 110e0c4a9834c1c44f54082b94e291ffa6fa2ff1f1d1faef2ae2aa2480fb333d |

These results satisfy this checkpoint's focused unit/core-Clippy obligations.
R01–R16 limitations and owed ROOT work above remain: actual Windows/HCS/device
semantic acceptance/exposure, broader bridge ownership and cleanup/isolation,
real hardware/driver combinations, appropriate native/mutation checks, supported
full matrix/integration gates and merge. No native bug, hardware assignment,
performance or full live-scheduler completion claim follows from these JSON tests.

Raw evidence checkpoint is signed `1c4d1584ca883a86bfd744de5e37b71eb5fa0dc4`.
The staged evidence-wide `git diff --cached --check` returned2 for one retained
artifact: light-v1-hcs-unit.log:142, new blank line at EOF. This is the original
Cargo output (SHA65758f4c above), not a source-format result. Its exact bytes were
preserved rather than normalized; no global whitespace exception was added.
The earlier scoped source rustfmt/whitespace checks remain exit0. No test was
rerun to change evidence formatting, and no full-tree whitespace/gate pass is
claimed. ROOT receives this explicit retained-artifact check limitation.
