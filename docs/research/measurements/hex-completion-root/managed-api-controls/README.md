# Managed API controls

This L4 assignment exercises the actual vendored `shards_api` managed API.
Its original ROOT c23 cohort failed in the API library before test compilation.
The current API pin is `0277c13a654290fb94429aaa3d53aa0c7643dc43`, including the
reviewed fallible `ManagedReceipt` intake and preserved Hex patches.
The assignment is AC-T7.4.1 in
`scheduler-completion-20261110/managed-api-controls.md`, with ROOT's subsequent
refinement assigning clone/scope/inline-stop controls to L2's signed 5d8 source.

The owned source is `crates/hex-cli/tests/completion_managed_api.rs`. Its four cases
call `prepare_boot`, `boot_managed`, `attach_managed` and `Guest` methods directly.
The native boundary is an in-memory receipt implementing `ManagedProcess` and
`ManagedLauncher`. The actual API creates the pipes, drains the bounded tail,
writes its end marker, runs its observer and presents the resulting `End` or
typed refusal. No production source or dependency is changed by this assignment.

## Controls and observations

| Case | Oracle |
| --- | --- |
| `launch_identity_refusal_stops_the_returned_native_owner` | A missing controller identity and a wrong vsock each refuse, invoke stop exactly once on the receipt returned by launch, close the supplied output copies and return the receipt references. Neither starts a native observer. |
| `attach_identity_mismatch_never_stops_the_foreign_owner` | An attach callback returns a receipt with another start identity. The API refuses it, invokes no stop, starts no observer and returns the receipt references. |
| `observer_uncertainty_remains_typed_and_stops_the_same_owner` | An explicitly published `Scope(Os { QueryScope, code })` remains typed through `process_exited`, `terminal_scope` and the first API wait. A subsequent wait still refuses. Last-drop stop addresses the same receipt and leaves its observation uncertain. |
| `terminal_output_is_bounded_drained_and_releases_native_receipt_refs` | Sequential stdout/stderr writes exceed `PreparedBoot::output_bytes`. A positive Released receipt produces exactly the final retained raw-byte tail after UTF-8 replacement, while both native writer copies remain open. Cleanup closes them and observes receipt destruction without a stop request. |

The receipt keys are fixture identities independent of the claimed PID/start.
Explicit event records prove callback entry, stop targets, native observer
subscription/return, writer closure and receipt destruction. The fixture's
control `Arc` has no receipt ownership. The receipt is allocated with
`triomphe::Arc::try_new` and converted by the actual `shards_api::managed_receipt`
helper. Its destructor event occurs after all receipt references return.
Cleanup requires exactly one such event. Triomphe has no weak receipt handle.

`Guest::wait_for` returns after the API's marker drain and terminal publication.
That return and final receipt destruction are protocol termination observations
for the hidden API workers. The API exposes no join handles. The tests create no
additional actor threads. Output bytes, error codes, status codes and claimed
identities are explicit fixture inputs.

## Bounds and cleanup

Event waits and API waits use the existing
`shards_api::conformance::PATIENCE.get()` guard. The exact c23 source derives it
from recorded native caller waits in M463. These controls never raise that guard
or teach it new observations. All event waits use one absolute deadline.

The output case derives its input length from the actual prepared output bound,
writes a bounded prefix and suffix, and performs both writes before publishing
Released. Raw output is capped at the prepared byte count. UTF-8 replacement may
expand its rendered byte count, which the expected string explicitly accounts for.

Every harness has an unwind cleanup guard. If a test exits while its native
observation is pending, cleanup publishes an explicit forced-release event to
wake the API subscriber. Every successful case rejects that forced event.
Cleanup drops the API handle, closes the fixture's stdout/stderr copies and waits
for the receipt-destruction event before final assertions. Observation errors
remain uncertain during cleanup. Rejected foreign attach remains pending.

The source checkpoint preceded execution. RESULTS.md records the actual four
passing controls, the remaining ABI Clippy failure, original failures, commands
and archive/binary/log hashes. Physical native scope, guest, GPU and VM effects
remain ROOT validation work.

## Exact private build closure

Use the four-member API workspace from the exact ROOT pin, including API/ABI/afunix/testdir,
workspace configuration and referenced upstream fixture sources. Install the
owned leaf byte-for-byte at `crates/api/tests/completion_managed_api.rs` in that
private copy. Keep the complete vendored source inventory. Select only this test
target, so native Node/Recorded code is outside the dependency closure.

For a ROOT-authorized SDK successor, `prepare.py --api-revision <literal-sha>`
exports that exact vendored workspace and records its API source separately from
the signed owned leaf revision. The original c23 closure and compile failure
remain attributable to their own archive. ROOT supplied exact 0277 after sealing
the fallible `ManagedReceipt` successor. Only fake receipt ownership changes in
the adapted leaf. All four control behaviors remain the same.

The upstream lock seed is `bb763aa3b175b755c8b85c2115722d6ad749088b:Cargo.lock`,
SHA-256 `1ecb41e738c4c4133a1968873e424efce5b2a65899dd8f40f37ac6aaffec8133`.
The private generated lock must retain each used registry package's exact
name/version/source/checksum from that seed. Record both complete lock hashes
and the registry subset comparison. Generated lock changes belong only to the
private closure and evidence.

For exact 0277, ROOT supplied
`docs/research/measurements/hex-completion-root/managed-api-source/managed-receipt-Cargo.lock`,
SHA-256 `5bc1b78708a541fda67263a3af6ed4690b93ab05882532cffa6fbfb0a996b66b`.
It adds the reviewed `unsize` 1.1.0 edge/entry to the original seed. Pass that
literal source pin, path and hash through the preparer's `--lock-revision`,
`--lock-path` and `--lock-sha256` options. The API workspace already has a direct
Triomphe dependency. Whole Hex integration requires ROOT to provide the same
test dependency to the CLI package when its full fixture cohort is compiled.

The original archive preparer copied reachable package tables directly. Cargo
refused that generated 0277 lock before Rust compilation. Its raw refusal is
retained in `original-0277-lock-refusal/`. The corrected preparer copies ROOT's
complete reviewed seed. A private `cargo metadata --format-version=1` resolves
the canonical lock before any compilation. Every selected registry tuple must
match the reviewed seed. Every static source hash must remain unchanged. Only
after those checks does the driver compile and run the four cases with
`--locked`. It records the input, compiled and final source manifests and the
complete generated lock. All following compile, execution and Clippy steps must
leave that generated lock unchanged.

Run only the four new cases and direct test-target Clippy on admitted d132 with
`cpus=1,mem=4g,disk=12g`, nice 10, jobs 1, test threads 1 and a new private
source/target/cache/temp. Transfer the exact archive with `--no-sync`. Copy the
compiled target to a private immutable execution path and verify both copies'
hashes before and after execution. Record every owned PID and actual exit code.
On timeout, child exit alone leaves descendant/worker cleanup uncertain.

## Policy and evidence obligations

ROOT is coordinator `01a1aa77778471339bea3beff90d6bbd`. L4 is implementer
`01a1c7676cfc7297a41236a8e71ee188`. Full policy ca3 was read before source edits,
with rules hash
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
ROOT acknowledged the exact c23 ownership, then narrowed the test selection.
The local Kiln restart interrupted a queued message before its dependent patch.
Recovery verified a clean c23 branch and no new leaf or dispatched check before
reapplying the draft. Earlier 05c/e29 branches and evidence remain preserved.

| Rules | Evidence and owed work |
| --- | --- |
| R01 | Changes are test/evidence paths. Test assertions follow explicit cleanup. Direct test Clippy is pending. Production panic review belongs to ROOT. |
| R02, R05, R12 | Primary sources are exact c23 API `lib.rs` managed traits and `client.rs` managed launch/attach/watch/drop/drain methods. The fixture uses their existing public seam and one fake receipt authority. |
| R03 | Production literals are unchanged. Fixture inputs derive output length from the prepared bound and guards from the existing caller measurements. |
| R04 | Pending, Released and Uncertain are explicit observation states with ordered event records. Stop requests cannot establish Released. |
| R06, R07, R13 | Four focused controls share one receipt harness. Comments explain worker termination, uncertainty and cleanup obligations. |
| R08, R14 | Comparative performance and allocation/fault profiling remain ROOT work. This fixture makes no performance claim. |
| R09, R10 | New contract controls are source-only until executed. Preserve any actual failure and report a production symptom before any proposed outside-scope fix. |
| R11 | The admitted host light run covers macOS arm64. Other supported targets and native VM environments remain unverified. |
| R15 | L2 owns the existing clone/scope/inline-stop tests. ROOT owns integration, native/campaign work, audits, gates and main merges. |
| R16 | Unique private paths preserve source/logs and avoid active cache mutation. Check laptop headroom and runner admission before dispatch. |

The complete development-policy script and all integration/full gates remain
ROOT work. This scoped API-only closure cannot establish the policy of the full
upstream native workspace or a compile-complete Hex backend cohort.
