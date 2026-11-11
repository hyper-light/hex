# Lifetime fixture checkpoints

Implementer: Kiln `01a1c640e6ad7127bcf34212f474d37c`; coordinator Hex
`01a1aa77778471339bea3beff90d6bbd`. Fixed source base:
`8767f0d9c03f37c64c9c86301bceaa878f7eb495`.
Owned paths: this directory and the NEW
`crates/hex-node/src/placement/adversarial_lifetime_tests.rs` only.

POLICY_READ: fetched current Shards origin/main
`eb5654d6f5356cacaca536ff51f14a632fa7e801`; full AGENTS.md, R01–R16,
development-evidence.md, CLAUDE.md and managed-runtime-plan.md read.
Rules SHA256: `d634fcb944959ce1e40a9277d12b0905fcfcebf82d9da9832e826f790a349242`.
Full Hex CLAUDE.md and both specified handoffs read. Readback sent with stable key
`lifetime-policy-read-eb5654d6-8767f0d9-v1`; coordinator acknowledged the scoped
leaf/private-copy registration workflow. No shared or production file edits.

## Checkpoint 1: stale cancellation versus an equal-key sibling

Requirement AC-T7.4.1/LB03. Existing placement tests cover clean independent
namespace deletion/restart; this case captures a Delete, reboots that owner,
reuses seq=0 in a new epoch, replays the old Delete twice alongside duplicate
current Starts, then checks the sibling. Primary implementation seams at the
fixed base: `placement::Driver::{boot,hear,drive,delete,poll_status}`, durable
`placement::rows::Rows`, and `hex_sim::peer::Node::{hear,start,end}`.

Expected: exact Inspect/Create/Delete callback IDs stay in the acting namespace;
the sibling's durable bytes and actual simulated box survive; its program returns
typed Exit::Code(7), and a subsequent sibling submission reaches Runtime.create
and Notice::Started. All owned simulated boxes and own/ran rows are then removed.
The runtime wrapper records IDs and delegates unchanged to existing Counting and
SimRuntime. Counting's counters remain the independent effect counts.

Status at signed checkpoint `59685fe17a21f36d8476cb286425c205316f6b04`:
SOURCE ONLY, unregistered leaf. No compiled or executed test result yet;
red/green counts are **unrun**, not 0/0 passing. Scoped rustfmt --check and
git diff --check pass. No native backend, socket, process/VM isolation, resource
measurement or heavy adversarial campaign claim. ROOT registers the leaf under
placement::tests. Temporary registration, if needed for a light remote check,
is permitted only in the new private source copy and must be recorded.

## Checkpoint 2: failed physical cleanup and a smaller reboot budget

The co-located owner/runner uses the existing Counting fault seams: create makes
the simulated box and returns Refusal::Os, then delete refuses while it remains.
After a second box succeeds, cancellation of the uncertain box is durable but its
physical delete still fails. Reboot both roles with owned=1/ran=1 while two boxes
remain. Recovery must inspect the same IDs, retain both charges/own+ran rows,
replay Delete rather than Create, and preserve typed Unknown with no Ended.

Expected witnesses include the runner's actual admitted resource amounts,
deleting/running durable row grammar, byte-identical rows across another failed
delete/refused admission, and the two actual SimRuntime boxes. Once exact delete
succeeds, one surviving box still fills the current smaller ceiling. An erroneous
replacement admission is driven to Runtime.create and cleaned before the final
invariant assertion. After the survivor executes and is deleted, new work may
physically create again; cleanup leaves no boxes/own/ran rows or runner charges.

Status at signed checkpoint `a249cef1e6f52f0c3400b6ffea02ae4a03f6f8a4`:
SOURCE ONLY, both leaf tests unregistered/unexecuted. Red/green UNRUN;
scoped rustfmt --check and git diff --check pass. Co-located simulated restart is
not a separately crashed owner/runner process or actual native backend effect.
No new inspect fault seam, waits or production behavior were added.

## Actual focused execution of a249cef1

Both named tests executed on approved d132 macOS 26.4 arm64, with Cargo
`1.99.1 (5f94df478 2026-08-27)` and rustc
`1.99.1 (58cf8f9c3 2026-10-13)`. Kiln run `r4` ended 0.
Admission: `cpus=1,mem=4g,disk=12g`; CARGO_BUILD_JOBS=1 and
RUST_TEST_THREADS=1. The command used `nice -n 10`; the observed process nice was
20 under the inherited runner clamp. No priority/security setting was relaxed.
All source, target, CARGO_HOME and TMPDIR paths are newly created beneath
`~/.cache/hex-lead/lanes/adversarial-lifetime-01a1c640-a249cef1/` on d132.
Existing `~/.cache/hex-lead/rustup-scheduler-copy` supplied the installed toolchain.

Precise no-sync archive: 5,896,113 bytes, SHA256
`b223794f06cd142cec7a17b8f6ea8c4fae5b7b67976e874f039546b3f6cf9b65`.
Test leaf SHA256 before and after:
`dafcb6d6c108432f96de2bc436c5e53436c05c739223f5c59e41a7e7635b5006`.
All 1,170 archive members were validated before extraction. Temporary registration
was appended ONLY to the private source's placement/tests.rs:

```rust
#[path = "adversarial_lifetime_tests.rs"]
mod adversarial_lifetime_tests;
```

Shared parent original/local SHA256:
`00fb45393f5543e27ee1286e10a8a21657da45dea31f91055bf8443c154b2ea4`;
private registered parent SHA256:
`66c4b2ce90d910e6915591c365b09f570ed14a9b04e2c2cffc5521c1e47d2582`.
Complete private source hashes before/after compilation match. No local parent
or ROOT file changed. This evidence update changes documentation only; the tested
leaf and other compilation inputs remain byte-identical to a249cef1.

Exact selected command (in that private source, with the environment above):

```text
cargo test -p hex-node --lib --locked placement::tests::adversarial_lifetime_tests:: -- --test-threads=1 --nocapture
cargo clippy -p hex-node --lib --tests --locked -- -D warnings
```

Observed: **2 passed, 0 failed, 0 ignored, 127 filtered out**, test exit 0;
both `failed_cleanup_and_delete_intent_survive_smaller_budget_restart_until_exact_delete`
and `replayed_cancel_after_reboot_and_sequence_reuse_cannot_touch_sibling_namespace`
have individual `... ok` lines. Focused Clippy exit 0. No test red or production
defect was reproduced. Deliberate Refusal::Os fault inputs are expected behavior.

The successful tests checked these concrete observed effects against the stated
expectations; the original logs do not serialize per-transition raw snapshots:

| Case | Passed callback/state/cleanup assertions |
| --- | --- |
| Cancellation replay | Exact callback ID arrays; 4 creates and 6 delete callbacks in total; sibling durable bytes unchanged; current and sibling boxes both present after replay; sibling Exit::Code(7), subsequent create/Started; final box count 0 and only epoch.rec in each directory. |
| Failed cleanup/reboot | Actual uncertain and surviving SimRuntime boxes retained; deleting owner/running runner rows; retained runner charge `[200, 2097152, 0, 0]`; repeated failed Delete and refused admission preserve bytes; exact deletion leaves `[100, 1048576, 0, 0]` and one box; equality still refuses admission; survivor Exit::Code(7); fresh create succeeds after both old boxes are deleted; 3 creates/7 deletes; final box count/owner rows/runner rows/charge all 0, epoch.rec retained. |

These are **native arm64 libtest executions with simulated Runtime effects**.
They are not native process-box/OCI/VM effects or an aggregate adversarial campaign.

Retained original proof on d132 is the private `proof/` directory. Local copy:
`/Users/adalundhe/.cache/hex-lead/transfers/adversarial-lifetime-01a1c640-a249cef1/proof/`.
All 16 bundled files/member hashes were verified after transfer. Archive:
`proof-bundle.tar.gz`, 100,720 bytes, SHA256
`20282998829c65af1ae6fd02f966f74d0f6fc41401fa2e4c74d1e57194900fb7`.
The adjacent `proof-readback.json` identifies individual selected results, source
hash equivalence, toolchain, log/binary hashes and ownership. Important identities:

- selected-tests.log: 5,240 bytes, SHA256
  `397fdc8191d8703d829d92fee43ff656cfc8cc0596d983d695df7fb97b23f189`.
- focused-clippy.log: 3,025 bytes, SHA256
  `8ed1e7c0aaf6b5de606e10e525a2f4ae4dda247bd6f0e318821ef9ab20202b17`.
- libtest `hex_node-1028f62b4e94535b`: 16,618,680 bytes, SHA256
  `d41199b5446ddb0a466a2d8ee729083dfd0523e78c51ff79ce39a75628a7fd35`.
- frozen light-v2-command.sh SHA256:
  `b4fd24c701af978ce8eb342f8e08ef5abebf0d0fbc4d9c5a1201c3833e8f7883`.

The first driver failed BEFORE compiler/test spawn: `os.nice(0)` raised EPERM
while attempting a priority read. It ran 0 tests, is not a test red, and did not
change source. Original returned traceback, v1 command and failure receipt are
preserved in the bundle as `light-v1-tool-result.json`, `light-v1-command.sh` and
`driver-attempt-1.json`. Only that read was replaced by `ps` in driver v2.

Owned process records: driver PID 86487; test Cargo PID 86519/start
`Tue Nov 10 08:26:06 2026`; Clippy Cargo PID 98235/start
`Tue Nov 10 08:27:19 2026`. Both exact child processes reached exit 0 and were
reaped with Popen.wait; the enclosing run is terminal 0. No VM, socket server or
native runtime box was launched. No process was killed. Private source/targets,
artifacts, cache, drivers and original evidence are retained, not reclaimed.

## R01–R16 and owed coordinator work

- R01/R04: test-only typed errors, messages, statuses and terminal Exit variants;
  no production panic/lint/state-machine changes.
- R02/R03/R05/R12: existing repository Runtime/durable seams, no new architecture,
  production constants, waits, caches, managers or platform path. The two replays
  are the bounded adversarial input, and exit 7 identifies simulated progress.
- R06/R07/R13: one focused leaf, a callback recorder and unchanged Counting
  delegation; comments explain provenance and the missing interleaving.
- R08/R14: no speed/resource/competitive claim; profiling and comparison remain
  ROOT-owned and were not run by this implementer.
- R09/R10: no production defect/fix claim. Exact focused invocation, original
  negative driver record, green logs/source identities and expected/observed
  state/cleanup assertions are retained above. Guard mutations remain ROOT-owned.
- R11: native macOS arm64 selected libtests and focused Clippy passed; all other
  targets, seven-target combined gates and native backend behavior are unverified.
- R15: no children/goals/plans, no aggregate audit, merge, full gate, stress or
  VM execution. ROOT owes source review/module registration, combined execution,
  relevant guard mutation/restoration, combined gates, separately restarted peers,
  uncertain native inspect/delete and actual backend coverage.
- R16: no cache cleanup; local free space checked above 30 GiB. Remote light checks
  may use only d132, 1 CPU/4 GiB/12 GiB admission, nice10/jobs1/threads1, private
  source/target/cache and precise no-sync transfer. Own run r4 is terminal 0;
  no active owned job or unreaped owned child remains. Local free was last checked
  at 34,581,450,752 bytes, above 30 GiB; no cleanup/deletion was performed.

Checkpoint readiness is not completion of the lane or the full scheduler goal.
