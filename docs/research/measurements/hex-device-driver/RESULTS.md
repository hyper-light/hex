# Device Driver: three model/light fixtures

Signed tested source `6483c6f2b0cb109cd41d009aa3974962a238c1b8`, exact production
parent `5e2cfbe7e09256ff6a7d979564717cfc6d8041f6`, branch `cp/t7-device-driver`.
Only NEW placement/device_binding_tests.rs plus this NEW evidence directory are
changed. The later evidence checkpoint has no Rust/production source change.
Full POLICY_READ cbd/df2, scope/pinned source decisions and expected witnesses are
in [README.md](README.md). Parent registration remains UNAPPLIED locally;
[REGISTRATION.patch](REGISTRATION.patch) belongs to ROOT's integration step.

## Actual selected results

One private admitted job r20, terminal exit0. Build0, exact3selected tests,
3passed/0failed/0ignored; node production-lib Clippy0, node lib/tests Clippy0 and
scoped leaf rustfmt0. Every exact-name command ran1test and recorded its own PID,
wait/reap and raw log hash. No unregistered or zero-filter result is counted.
No test failed and no source correction or extra case/run was needed.
The initial unapplied registration diff contained a whitespace-only context line;
local diff-check caught it before the source checkpoint and its context was
trimmed. This was a preparation check, not a test/production failure.

| Case | Actual model/SimRuntime observations after real Driver operations | Cleanup before final assertions |
| --- | --- | --- |
| Reboot/reorder/load swap | Old key0/1/0 -> GPU256, same BoxId/claim/binding row after reboot; no second create/start. Fresh key0/2/0 -> GPU512. Catalog order reversed512/256; exact holders match. create/start/delete2/2/2. | Both modeled boxes/claim records/holders/owned rows gone; admission0/0/0/0; epoch.rec only. |
| Caller forgery | Typed Runtime(Invalid{what:placement}); all durable bytes/names and full Runtime call log unchanged. Legit GPU key0/1/0 -> GPU256; CPU key0/1/1 progresses with simulated Code0. Exact create receipts GPU claim/[empty]; counts2/1/2. | Both model boxes/claims gone; admission zero; owned rows empty and epoch.rec only. |
| Metadata disagreement/repair | Pending GPU key0/1/0 initially Created. Wrong modeled GPU512 metadata -> typed Core(Damaged), all durable bytes/names unchanged, create/start/delete counters unchanged1/1/1; still Created. Independent CPU namespace progresses Code0 while target remains Created. Restore GPU256 metadata -> same BoxId/claim, Running, no second GPU create; exact failed-start then successful-start outcomes, total2/2/3 including initial failed deletion. | Both Driver ledgers epoch-only; both admissions zero, CPU owned empty, all shared model boxes/claims/holders gone. |

The exact physical-looking BoxIds are model identifiers in raw test logs. Main
namespace00112233445566778899aabbccddeeff; independent CPU namespace
ffeeddccbbaa99887766554433221100. Keys are shown as owner/epoch/sequence above.
The leaf records typed ModelCall IDs/claims/start/delete outcomes, asserts exact
create receipts, matching-key start outcomes and positive counts; raw summaries
retain keys, IDs, claims, states, counts and cleanup, rather than the entire
in-memory call vector. Metadata refusal permits read-only inspect/capacity calls;
its no-effect counters cover create/start/delete. Forgery checks the entire call
log, including probes. All full file maps are read from disk and compared in the
leaf, including names/bytes, not only the core's cached row text.

## Boundary and remaining limits

ALL Runtime effects here are model/SimRuntime only. The real file-backed Driver
and shared production Catalog::prepare/holder/Entry::report execute, but the
fixture keeps claims in in-memory test BoxRecords and clears them ONLY before
unchanged SimRuntime.create. SimRuntime itself still refuses physical claims.
The synthetic CDI catalog path/rootfs is never opened; no CDI application, host
stat, real device/cgroup/mount/hook/VM/SSH/process launch occurs. The simulated CPU
program's Code0 is not a native CPU/hardware conformance result. No production
picker/manager/catalog/cache/API/default/dependency changes are part of this lane.

## Exact environment, commands and private registration

Runner d1323898b255a3ed435eb83fa0f7b0cf, actual macOS-26.4-arm64-arm-64bit / arm64,
Rust/Cargo1.99.1. Admission cpus1/mem4g/disk12g; nice -n10, jobs1/testthreads1,
--no-sync. Actual nice20 (inherited runner nice10 plus10). No laptop builds/tests.
Fresh private source/target/cargo/proof at
`~/.cache/hex-device-driver-01a1c640-6483c6f2-v1` on that runner; registry-only
APFS clone, no credential/config copy. Existing ROOT/other-lane snapshots,
targets/caches and jobs remain untouched.

[Executed driver](light-v1/driver.py), [transfer](light-v1/transfer.py),
[collector](light-v1/collect.py), [safe proof fetch/readback](fetch-proof.py).
Commands (toolchain's exact executable paths/argv retained in raw proof):

- `cargo test -p hex-node --lib --locked --offline --no-run --message-format=json`
- Compiled binary `--exact placement::tests::device_binding_tests::NAME --nocapture --test-threads=1`, exactly the three leaf names in tests.list.
- `cargo clippy -p hex-node --lib --locked --offline --no-deps -- -D warnings`
- Same Clippy with `--lib --tests` to compile/check the node test class path; no other tests executed.
- `rustfmt --edition 2024 --check crates/hex-node/src/placement/device_binding_tests.rs`

Only the PRIVATE placement/tests.rs appends the two-line module registration.
Original parent SHA `0cdf5d751719274f5c9e5c92709895e73231263d9aabc04cecf7a20ec0e47e87`; private parent SHA `f2b76cf344e88de2b85eb2dd5bcec119f0ee5aad508caff085b0a1dc904e7dd4`.
The raw before/private bytes verify original parent equals exact5e and the private
file differs only by that registration. Actual local parent has no change.
The tested leaf/source production copies exactly match the signed commits.

## Retained raw proof and hashes

Leaf SHA256 `b55850799992e69687adeed1e3fbe3414a4a75b2c4902aed8c7273d1b6436d0f`.
Binary SHA256 `78cbee6f077357f2b2ca4ee927a60c9646de4562bc11342c03b3738721a20e88`.
Full [source/log hashes](source-and-logs.json) and [production source pins](pinned-sources.json).
Input source archive6,766,127bytes,1202regular members, SHA256
`d948088dd48d7a50adbcd017950a0b6ca59dc58d413da4ece0e61ce6b68c15bb`.
Raw proof [archive](proof-v1.tar.gz)51,462bytes/41regular members, SHA256
`7575d90f8a1257f3c51bdfd4bd352b7520252546f54ac2fa4b08ecf4d49e9ee8`.
All paths/counts/member hashes, signed source, exact private registration,
nonzero selected log results/cleanup markers and child reaps verified by the
[receipt](proof-receipt.json). Original raw files are in raw-v1/. Libtest's raw
EOF whitespace is preserved exactly and separately recorded in raw-whitespace.json;
source/docs/driver whitespace remains checked without a global exception.

| Raw proof log | SHA256 |
| --- | --- |
| `build.jsonl` | `b2f85733c55e476437d70419f0bb61be4e3780b104657afed168770df0fc4e0e` |
| `build.stderr` | `1f0b3186613efd0283f9484e457b9ea1f0fc0a13b7d662ceb24409bb7a7b76d9` |
| `clippy-lib.stderr` | `7966e55f4e36a6d97173721c6124c92971766a99371b6932a889617352334d04` |
| `clippy-tests.stderr` | `edfb5c0babb2b909dc3a7019af352c42a5bc749b94320cb1f555b3df0dda94ab` |
| `rustfmt.log` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `summary.json` | `78493ccc8758593538a6e19d6c6b45efe762c9fc8c32f36bf84d62eae2d5db4d` |
| `private-registration.json` | `73a74b74ec11d2a031ee9ab16214848833f2a9b5496a24aba9d74eb6c203567c` |
| `test-0.log` | `66d9d761cac44c9af1e4d1df4ca7eeac7c3dd7fc54a5a5493ae12efa6659a796` |
| `test-1.log` | `3c9171db0b2fa5791b99c8dc08a727d4120446aa4a47ba2836230ce34100c45e` |
| `test-2.log` | `d03fb8bed323bbd4b464f9b9abef1766dc73b64e6033fa81d554ff28d98691bf` |

## Owned cleanup and owed ROOT runs

r20 and all selected test/build/check Popen children are terminal/waited; no timeout
kill was needed. Actual selected child PIDs: 30432, 30433, 30434. Each case explicitly removed
its modeled boxes/metadata/claims and proved empty admissions/owned rows before
final assertions; TempDir removed its own file ledgers when the test ended.
No hardware/VM/socket/native child/hook was created. Input/source/targets/caches/
binaries/proof stay frozen; no other data/process was stopped or deleted.
Pre-transfer local free248249300KiB; pre-fetch local free242236796KiB, both above
30GiB. The initial r20 progress message copied an incorrect free-space number;
a correction was sent immediately and transfer.json is the exact authority.
Previous CDI/renderer/relay source/raw refs and jobs are preserved. No force,
stash, main mutation, push or merge occurred.

ROOT owes parent registration, independent source/evidence/aggregate review,
actual native hardware/driver/catalog/claim persistence/effect/cleanup validation,
all supported OS/architecture/seven targets and full gates, and all heavy/E2E/
VM/fault/mutation/sweep/profile/benchmark work. No actual device isolation,
performance/leak/OOM claim or completion of the live-balancing requirement follows
from this batch. Source/jobs hold after checkpoint pending ROOT's next concrete scope.

## R01–R16 accounting

| Rule | Evidence / remaining gap |
| --- | --- |
| R01 | Test-only unwrap/assert permitted; production unchanged; node lib/test Clippy0. Universal production panic/OOM proof remains outside this fixture. |
| R02 | Existing pinned5e Driver/devices/Rows/Runtime/SimRuntime/shared Catalog and parent tests read; source hashes retained. The user selected this model seam; no new broader architecture choice. |
| R03 | No production constants. Two fixture boxes/devices and Budget2/2; four request-sized resource units, MIN_MEMORY, distinct legal PCI keys, documented per-mille0/1000 loads; existing MOST caps local delivery/receipt count. |
| R04 | Typed claims/ModelCall/refusals/terminal states; no String-matched production control. Wrong metadata has typed Core(Damaged), caller forgery typed Invalid. |
| R05 | Existing Driver picker and Catalog validation/reporting, no duplicate selection/cache/poll/wait manager. Model BoxRecords are explicit test data only. |
| R06 | One focused leaf, three cases and required Runtime delegation; combined audit remains ROOT-owned. |
| R07 | Comments state model/native boundary, source/receipt bounds and why claims are extracted. |
| R08 | No competitive/performance result; ROOT comparative work unrun here. |
| R09 | Adds missing cross-seam fixtures against reviewed production pin; no speculative production bug fix or native reproduction claim. Expected modeled start/delete refusals are not failed tests. |
| R10 | Signed source-first0execution status preserved; one actual green run, exact source/log/raw/private-registration/cleanup evidence. Native before/after and mutation proof remain ROOT-owned. |
| R11 | Actual macOS arm64 pure/model checks only. Other supported targets/native OS effects unverified; Intel Mac unsupported; paused Windows ARM VM untouched. |
| R12 | Shared existing Runtime/Catalog/Driver/SimRuntime; no OS adapter/installed binary/root/trust/crypto change. |
| R13 | Only owned NEW leaf and evidence; actual parent/shared/production files unchanged. |
| R14 | No profiling/benchmark/allocation/page-fault/physical resource/leak claim; ROOT owns measurement. |
| R15 | Full current cbd/df2 readback and fixed signed5e pin before mutation/checks; one admitted private1CPU job, signed named paths, no children/goals/dispatch/merge. ROOT aggregate/native/heavy ownership retained. |
| R16 | Old refs/raw/source/binaries/caches preserved; local floor checked before bulk transfers; no unrelated cleanup or runner changes. |
