# ROOT-only Windows exact-device/HCS witness

Final source `888764365adf0d54788a51a7699aed396d962e73`: **scoped Windows cross Clippy exit0;
zero tests/native operations executed**. The initial source-first checkpoint was uncompiled and
UNRUN; its original receipt remains unchanged. No HCS/container/keeper was launched by this lane.
Only new `crates/hex-cli/tests/windows_assigned_device.rs` and this directory are owned. Base
`d3e936d12f89397715f8a3b34a4ca69160252f78`, branch `cp/t7-windows-device-witness`. ROOT integrates
and executes against its current combined source (new callback/borrowed-lifetime code at862ebab7
and later), **never native execution of this old fixed-parent cohort**. Public APIs are unchanged.
Prior fd4af845/18eccf40 Apple witness, 8cc/0e telemetry, c646 helper and all earlier refs/raw artifacts
remain preserved. No production/Cargo/shared parent edit, child agent, goal/plan or campaign.

## Policy and ACK

Full fixed Hex CLAUDE/current HANDOFF and vendor/shards AGENTS/R01–R16/development-evidence/CLAUDE
reread before edits. At that checkpoint, vendored/upstream Shards policy
`33df95b08ad8f95bddeaa1c48acf0c64386e0eac` was byte-equal in all four files; rules SHA256
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
Implementer01a1c640e6ad7127bcf34212f474d37c; coordinator01a1aa77778471339bea3beff90d6bbd.
POLICY_READ key `windows-device-witness-policy-d3e-33df-ffi-v1` includes test-only FFI rationale.
ROOT explicitly ACKed before branch/edits/checks. Its later single-GPU/foreign split refines the same
scope, with no redundant permission hold. Narrow Hex source/compile-only scope overrides broader
Shards adversarial permission. ROOT owns every actual effect/fault/mutation, native cleanup,
identity/LUID/location correlation, audit, profile/benchmark, cross-lane/gate/main operation.

After compaction, reread full current Shards origin/main
`a920c85b5c0c9d40aafc4f4cce32432c7c0aed79` AGENTS/rules/evidence/CLAUDE/managed-runtime-plan,
Hex CLAUDE/current HANDOFF and g121-handoff. AGENTS/rules/evidence/CLAUDE hashes are unchanged;
rules hash above.
The current plan and local handoff hashes are recorded in `policy-resume.json`, readback key
`windows-witness-resume-policy-a920-evidence-v1`. Same acknowledged scope; only evidence work
followed the resumed readback. No new permission, source expansion or campaign.

Allowed optional check ONLY Windows cross compilation on admitted d1323898b255a3ed435eb83fa0f7b0cf,
1CPU/4GiB/12GiB, nice10/jobs1/threads1, NEW private source/target/CARGO_HOME/TMPDIR, precise no-sync.
No laptop build/test, no Windows/Metal/HCS native invocation, actual VM, installed binary, host
settings/security/runner changes or foreign handle/process access. Local scoped rustfmt/Git/reading/
editing/data permitted. Local free215214108KiB before checkpoint (>30GiB); nothing deleted.

## Two explicit ignored ROOT parent cases, one ignored child

- `root_windows_selected_gpu_assignment_survives_restart`: one actual selected GPU suffices.
  Requires its host exact-interface open, baseline ungranted deny, granted open, holder reservation,
  overlap refusal with no physical container/record, CPU sibling progress, owner restart/adoption
  with same init/physical bindings and exact-interface result, then delete/free/cleanup.
- `root_windows_foreign_gpu_remains_unreachable`: same sequence plus a distinct actual **second GPU**
  identity/location/interface and successful host open. The foreign exact interface must fail in both
  ungranted and selected-only granted/recovered containers. Missing foreign data/hardware is an
  **UNVERIFIED fixture error**, never skip/pass. A foreign USB device is not a second GPU and does
  not establish this criterion. Single-GPU case never reads/requires these foreign inputs.
  Each denied foreign child result is followed by the same exact host query succeeding again;
  disappearance is explicitly UNVERIFIED rather than a passing isolation negative.
- `assigned_device_exact_path_probe`: parent-owned CHILD invocation of this same test executable,
  privately copied/mapped read-only at `C:\device-probe\assigned_device_probe.exe`. Direct invocation
  without the parent inputs fails. It writes one new exact-result file to a private read-write map.

All are `#[ignore]`, cfg Windows (amd64/arm64 source). Default ignored counts are not validation.
ROOT must select parent tests by exact name under its external whole-process native guard. No
source-only/cross-compilation pass is an actual HCS or GPU pass.

## Required ROOT input contract (no defaults/guessed devices, limits or layers)

| Environment input | Meaning |
| --- | --- |
| `HEX_WINDOWS_DEVICE_SCRATCH` | Existing absolute ROOT-owned parent for a NEW private lab; never a borrowed layer/volume to remove. |
| `HEX_WINDOWS_LAYERS` | Existing matching process-isolated Windows read-only image layers, topmost first, `;` separated, same existing windows_oci_box contract; empty elements fail. |
| `HEX_WINDOWS_DEVICE_BOUNDS` | Explicit JSON hex_api::Bounds, passed unchanged except cloning to every spec; validated, no Bounds::default fixture fallback. ROOT admits aggregate resources. |
| `HEX_WINDOWS_DEVICE_WAIT_MS` | Positive finite wait budget, below Win32 INFINITE(u32::MAX). ROOT chooses it; no learned/guessed/increased limit here. |
| `HEX_WINDOWS_DEVICE_CATALOG` | ROOT's authoritative native seven-field catalog file; bounded by existing MAX_CONFIG_BYTES and parsed with the existing Catalog API, copied into new private state/devices. |
| `HEX_WINDOWS_DEVICE_KEY` | Decimal actual u64 GPU physical key in that catalog with HCS binding. |
| `HEX_WINDOWS_DEVICE_INTERFACE` | Exact ROOT-measured Windows device-interface path (`\\?\...`), not a class/count or guessed NVIDIA/AMD ordinal. |
| `HEX_WINDOWS_DEVICE_CLASS` | ROOT-measured interface-class GUID, checked by existing hcs::class_guid and recorded as supplied metadata. |
| `HEX_WINDOWS_FOREIGN_KEY/INTERFACE/CLASS` | Same fields for a distinct actual second GPU, required ONLY by the foreign test. Different key/path/HCS location; host open mandatory. |

ROOT supplies/independently validates the join key↔PCI/LUID↔HCS location↔exact interface/class. This
fixture intentionally **does not infer that join from a GUID, vendor name, PCI ordinal, JSON grant
or count**, nor does it implement discovery/production attachment. Model/size come unchanged from
ROOT's catalog; no WMI AdapterRAM32 or invented capacity. Actual host telemetry supplied by ROOT
(proof432ce469/919): Radeon8060S, ADL25.20.32.06, PCI0000:c4:00.0/key50176,
4294967296 bytes; host PnP/location correlation is ROOT's independent native work. These observations
are context, **not fixture defaults**. Exact interface/class was still owed at assignment.

ROOT-only invocation after combining this source with the current tested production cohort:

```sh
# ROOT supplies the required inputs and applies its existing external native process guard.
cargo test -p hex-cli --test windows_assigned_device --locked -- \
  --ignored --exact root_windows_selected_gpu_assignment_survives_restart \
  --nocapture --test-threads=1
# Separately, ONLY with a verified actual second GPU (otherwise explicit UNVERIFIED failure):
cargo test -p hex-cli --test windows_assigned_device --locked -- \
  --ignored --exact root_windows_foreign_gpu_remains_unreachable \
  --nocapture --test-threads=1
```

The guard covers HCS/keeper operations, driver query opens, filesystem calls, setup and teardown.
Proc waits use ROOT's finite budget, but native driver/HCS calls are not universally cancellable.
Timeout/abort cleanup remains uncertain and ROOT-owned. No polling/sleep, worker framework,
installed executable or system/global cache is added. The probe copy exists only inside the lab.

## Exact effect and persistence witnesses

The same `query` function runs on the host and in the container child: CreateFileW desired access0,
FILE_SHARE_READ|FILE_SHARE_WRITE, null security/template, OPEN_EXISTING, flags0, then CloseHandle on
each successful handle. This matches existing oci::devices call semantics. Failed-open GetLastError
is captured immediately; close failure is explicit. Host and child never use different access modes.

Host positive result requires opened=true/closed=true/error=null for the exact selected path and
(foreign case only) the exact other path. In the child, required selected/foreign inputs and mode
are set by Program.env (no inherited credentials/environment). The child writes actual exact-path,
opened/error/closed facts with create_new+sync to a new result file. No stale-output reuse or
stdout/harness count scraping. Parent checks the exit and every exact field. The output read limit
is derived by serializing the same path/schema with longest booleans/u32 error numbers.

Baseline is an ungranted OCI/CPU box; selected interface must not open. It remains the independent
CPU sibling, producing actual files through hex selftest-box write after refused overlap, after
restart and after GPU release. Granted/recovered selected opens; second GPU remains denied only
where a real second-device host-positive control exists. **Query0 reachability is NOT GPU compute,
D3D/CUDA/ROCm functionality or whole-vendor isolation. Those remain ROOT owed.** Old win-devices
listed/reachable counts are deliberately not used as exact-device proof.

WinRuntime::outliving_with_devices loads the private catalog and existing keeper; Recorded owns
durable rows. Public capacity reports selected holder None→box→same box after restart→None after
delete, while any foreign entry stays free. Recorded overlap refusal is paired with unchanged
original BoxRecord/native marker bytes, no overlap row/marker and exact HCS open_system absence.
After dropping/reopening only this owner on the same private state, recovery must adopt CPU/GPU,
retain Running state, same native init pid+creation time and exact HCS DeviceBinding, and return the
same recorded spec/claims. No second create/ID substitution is hidden by a refusal string.

## Cleanup and uncertainty

Lab uses a NEW tempfile under ROOT's explicitly owned scratch parent and immediately keeps it so
unwind cannot erase control rows or detach anything uncertain. Image layers/borrowed volumes are
never removed. Every attempted box ID and its own scratch path are recorded before create.
Normal/failure cleanup operates ONLY those IDs via the public runtime, checks each physical HCS
system absent, box records absent, native marker/scratch absent and private store empty. A private
keeper is stopped only by its recorded PID+creation identity and verified ended under ROOT's
budget; reused PIDs are never stopped. No global hcsdiag/mountvol/process sweep or broad kill.

Cleanup errors/unknowns are printed beside the retained lab path; the original failure is resumed,
not replaced by a cleanup assertion. If cleanup itself panics, that uncertainty is recorded and the
lab remains. Only a successful body AND proven complete cleanup remove the new owned lab folder.
No manual record repair, guessed absence, force delete of scratch/volume, or metadata-only free
claim. Native abort/external kill can bypass Rust cleanup; ROOT must retain/reconcile that scope.

## Primary sources, compatibility and R01–R16

- Microsoft [CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew):
  query-only desired access0, sharing, OPEN_EXISTING, null non-inheritable security/template and
  invalid-handle failure contract. [CloseHandle](https://learn.microsoft.com/en-us/windows/win32/api/handleapi/nf-handleapi-closehandle)
  supplies owned-handle release and failure semantics. Minimal test-only unsafe blocks have SAFETY
  comments; no production sys.rs/Cargo/CLI change or new syscall binding.
- Existing fixed-d3 windows_oci_box::Lab bundles/layers/hex launcher, Recorded::recover, Runtime and
  Catalog are reused at their public seams. Its private helpers are not widened/copied wholesale;
  the new small Lab follows the same bundle contract and refuses rather than copying its SKIP path.
- Source requirement AC-T7.3.1/AC-T7.4.1/T-T7.1.2. ROOT's later862 callback/borrowed implementation
  must be composed for execution; this source never proves that pending cleanup implementation.

Local scoped rustfmt/git diff --check exit0. **No build/test/native invocation at source-first
checkpoint.** The later actual cross-check receipts follow. No source leaf change after green.

## Actual compile-only results and frozen raw collection

One scoped command per source, on admitted d1323898b255a3ed435eb83fa0f7b0cf,
`cpus=1,mem=4g,disk=12g`, `sync:false`, jobs1/threads1, controller invoked with nice10
(Cargo owner receipts observed inherited nice20). Rust1.99.1/LLVM23.1.1, macOS arm64 host,
**x86_64-pc-windows-msvc type/lint check only**:

```sh
cargo clippy -p hex-cli --test windows_assigned_device \
  --target x86_64-pc-windows-msvc --locked -- -D warnings
```

| Run / signed source | Actual result | Frozen log SHA256 |
| --- | --- | --- |
| r96 / `f07bc72bb152106322c844ef299e29e56eebba54` | Cargo101, run1; E0277: HcsFailure does not implement Display. Tests executed0; no native effect. | `c1789e127500676bfdba640d98cdb1fb0064b5353a32f04973ecde7719dd3864` (6686 bytes) |
| r97 / `888764365adf0d54788a51a7699aed396d962e73` | Cargo0, run0; scoped cross Clippy passes. Tests executed0; no native effect. | `58597e87b5fe39b39b3ee5394d99b0150a451a1f051f10efe0c6ec6fd1e8567a` (5935 bytes) |

The compiler failure is a fixture diagnostic formatting mistake, **not an HCS/device defect**.
The successor uses Debug for that existing typed failure, and ensures the foreign host-positive
query follows every denied foreign child result. Both changes were signed before r97. Final leaf
SHA256 `1813e39ea97a024e60470e1b9c8ed90d0bf25ccf7a4c8fc187f2d2bdb0e440e1`.
Earlier source-first `be46fa8ac2fe0aeb4b28d5455c9c59c458ef5a2e` and its historical
`source-first.json`, f07 red, and all original logs remain retained.

Each check used entirely NEW private source/target/CARGO_HOME/TMPDIR paths, respectively:

- `~/.cache/hex-lead/lanes/windows-device-01a1c640-f07bc72b-compile-v1`
- `~/.cache/hex-lead/lanes/windows-device-01a1c640-88876436-compile-v2`

Immutable source archives: f07 9390528 bytes/SHA256
`825c690cfd80363cf91f1a02a2402d12271698a2abfa2d55bb384973db90638c`;
888 9391144 bytes/SHA256
`1ffdb405e5c2766451f8d7e0880f6eb8c3a9ec680cf8599f99496ac2042442bf`.
All1794/1795 source files respectively match before, after, transferred member hashes and the
exact signed Git tree byte-for-byte. Targets/caches/source snapshots are preserved, no deletes.
Cargo53536/55078 reaped; both private tmp directories empty. Transfer CLI23691/52243 reaped0.
No active check/compiler/native job, no produced test executable, no MSVC link check. There is no
test-harness pass/fail/ignored count: **zero tests ran**, including all three ignored source cases.

`compile_only-r96.py` / `compile_only.py`, both `compile*-submission.json` files and the two
`transfer-*.txt` retain exact controllers, submissions and transfer observations.
`kiln-terminal.json.gz` retains actual terminal tool outputs. Frozen
`compile-r96-r97-proof.tar.gz` is 485710 bytes, SHA256
`0d6d7d175a325b9a73836030d36490a22efa4645db61ac6f701cb6e60c83151e`.
It contains both raw logs, environment, exact owner identities, results, source-before/after and
transfer manifests, with14 hashed members plus its member manifest. `collection.json` records
the verified41 chunk hashes, full source/archive/log identities, actual results and cleanup.
`verify_collected.py` checks only these own frozen compiler receipts and signed source bytes;
it invokes no Cargo or native fixture. Raw collection cache is retained at
`~/.cache/hex-lead/transfers/windows-device-01a1c640-88876436-compile-v2`.

Three collection-only diagnostic failures remain explicit in that receipt: initial reader lost
the first output fragment before JSON parsing (no appended bytes), then two diagnostic scripts
printed their checks before a stray here-document terminator caused NameError. Accumulating all
output fragments and using the named verifier produced exit0; original raw build results were
unchanged. No check was repeated for collection or communication. Local free219746185216 bytes
at evidence collection (>30GiB), no cleanup deletion.

ROOT still owes review/composition on current production, exact native interface/class/physical
identity correlation, actual selected-only HCS/holder/overlap/CPU/restart/cleanup execution and
guard/fault controls under an external native deadline. The actual-second-GPU case is currently
UNVERIFIED; one Jupiter Radeon does not establish it. Query0 is not GPU compute. GPU compute,
native resource/kernel accounting, Windows arm64/hardware coverage, aggregate seven-target gates,
policy/vendor gates, profiling/benchmarking and merge remain ROOT-owned and unrun by this lane.

| Rules | Scoped evidence / owed ROOT work |
| --- | --- |
| R01/R04 | Tests may assert/panic; no production change. Exact query/close outcomes, setup/create/held/recover/delete/unknown cleanup states, original-failure preservation. ABI/native paths unrun. |
| R02/R03 | Official query/close contracts, existing Windows fixtures/APIs, Root measured metadata. Work/waits/bounds/layers explicit ROOT inputs; schema-derived byte bounds, no tuning/default. |
| R05/R06/R07 | Existing catalog/Recorded/keeper authority, no cache/manager/picker/worker; one bounded probe result and explicit safety/ownership comments. |
| R08/R14 | No speed, zero-allocation, compute, kernel-cost or leak claim. All comparisons/profiles/benchmarks ROOT owed. |
| R09/R10 | New source witness, not a reproduced defect or native pass. Source-first/compile evidence separate from ROOT actual effects/guard-removal/fault outcomes. |
| R11/R12 | Windows amd64/arm64 source; cross compilation is not native HCS/hardware validation. Test-local existing FFI, no installed helper or production API. |
| R13 | Only assigned native assignment/overlap/recovery/cleanup behavior and actual path opens. |
| R15/R16 | Full policy/explicit ACK; exact new-file ownership, source/compile-only on d132, no children/native job/laptop build. Root owns effects/cleanup/audits/gates/main; earlier refs/data and floor preserved. |
