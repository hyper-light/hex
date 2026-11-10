# Real Runtime pool census fixtures

CP-T7.4 / AC-T7.4.1. These are byte-preserved copies of the reviewed cache harnesses. This packaging ran no syntax checks, tests, builds, VMs or measurements. The coordinator adds current source/build/policy evidence and validates the final combined source; this directory supplies no passing result or installed wait seed.

`scheduler_census.py` runs ordinary `hex node` and the actual scheduler over private forced-key OpenSSH, with local and remote physical lifecycle controls. `scheduler_capacity_e2e.py` uses the same flags and first submits an oversized guest reservation, checks no assignment/physical creation, then runs the existing 64MiB positive control. For that boundary fixture, `--mode guest` and `--box-memory-mib` greater than `--guest-memory-mib` are compulsory. The fixed positive control is inherited fixture behavior, not a newly measured limit.

Requirements: matching already-built native Hex, static Linux Hex of the host/guest architecture, and the existing separately built Runtime probe (`--runtime-client`). The probe helper is intentionally not packaged here. OpenSSH `ssh`, `ssh-keygen` and native `sshd` must already be installed and usable by the coordinator. Native mode requires Linux and an explicitly delegated cgroup parent; guest mode requires macOS arm64/HVF and explicitly supplied signed/prepared Shards binary, kernel, Hex initrd, guest CPU and memory. No Windows or Intel Mac validation is implied. Re-read current policy and keep the Mac's required 30GiB free floor; no laptop execution.

Run the synthetic report regressions with ordinary module imports, **without `-I`**:

```sh
cd "$HEX_SRC/bench/sched/runtime-pools/census"
nice -n 10 python3 -B -m unittest -v test_scheduler_census.py
```

Those fixtures only read synthetic report data; their nanosecond words are never runtime measurements or seed inputs. Actual harness execution uses Python isolation instead. All uppercase inputs below are coordinator-supplied, with a matching binary/profile build receipt and current policy receipt JSON:

```sh
env -u HEX_GUEST_SHARES nice -n 10 python3 -I \
  "$HEX_SRC/bench/sched/runtime-pools/census/scheduler_census.py" \
  --source "${HEX_SRC:?}" --hex "${HEX_BIN:?}" \
  --runtime-client "${RUNTIME_PROBE_BIN:?}" --linux-hex "${STATIC_LINUX_HEX:?}" \
  --mode guest --profile "${PROFILE:?debug or release}" \
  --n "${N:?successful trials per local and remote phase}" \
  --max-census-lines "${ROWS_PER_PROCESS:?}" \
  --cpu-milli "${BOX_CPU_MILLI:?}" --box-memory-mib "${BOX_MEMORY_MIB:?}" \
  --shards-bin "${SHARDS_BIN:?}" --kernel "${HEX_GUEST_KERNEL:?}" \
  --initrd "${HEX_GUEST_INITRD:?}" --guest-cpus "${HEX_GUEST_CPUS:?}" \
  --guest-memory-mib "${HEX_GUEST_MEMORY_MIB:?}" \
  --deadline-seconds "${CANCELLATION_SECONDS:?}" \
  --deadline-evidence "${DEADLINE_RECEIPT:?}" \
  --policy-receipt "${POLICY_RECEIPT:?JSON object}" --build-receipt "${BUILD_RECEIPT:?}" \
  --sshd "${SSHD_BIN:?}" --run-parent "$HOME/.cache/hx"
```

For the actual capacity boundary, replace only the script name with `scheduler_capacity_e2e.py` and supply the oversized box memory as above. For native Linux census, use `--mode native`, replace the five guest-input flags with `--cgroup-parent "${DELEGATED_CGROUP_PARENT:?}"`, and retain every common argument. `--sshd` is optional if an installed daemon is discoverable. Optional `--guest-shares` accepts explicit operator roots; this campaign uses only a fresh private empty read-only share, never user folders. Unsetting inherited `HEX_GUEST_SHARES` and omitting that flag lets the harness create its own empty share. The guest backend also receives private empty Docker configuration and a private Shards home. HOME, user SSH files and runner services are unchanged.

**Use the short `~/.cache/hx` run parent.** Unix socket paths include the guest API's directory suffix. A long path can force its `/tmp` fallback outside the private run, correctly fail ownership checks and leave retirement owed. Do not weaken containment or discover/kill VMs by name to make a run pass.

N, workload resources and per-process row caps are explicit inputs. Source supplies protocol/API minima and WaitKinds. `--deadline-seconds` is the operator's outer cancellation budget, with its receipt; it is never a runtime timing seed. Each Hex/probe process and forced tunnel child gets a unique census filename. `call_outcome` and `successful_single_call_rows` join only schedule-call exits by that filename; `process_outcome` and `successful_process_rows` independently join command/probe exits. PIDs remain diagnostics and never join outcomes. Old records lacking filename identity remain unclassified. Raw rows remain separate and multiplexed rows can include typed refusals. `execution_policy` records the supplied current JSON receipt alongside unchanged historical `preparation_policy`.

By default the harness installs the existing measuring census, leaving runtime waits unseeded while collecting actual answers. A captured duration is not automatically a usable seed or a performance result. Coordinator review must establish profile/source/binary identity, success provenance, row completeness, no truncation and the physical controls before interpreting distributions or seeding anything.

To exercise matching already-built binaries with their installed waits armed, add `--validate-seeds` to the same invocation. Keep the same explicit N, resources, receipts and operator cancellation budget (including the existing 120s budget where used); this flag does not install or alter any seed or runtime deadline. The private runtime.json carries `wait_mode: armed`, so local Hex/probe children and forced SSH tunnel children both remove `HEX_WAITS_CENSUS` and `HEX_WAITS_CENSUS_MAX`, even if inherited. Default and older runtime fixtures remain `measuring`. The existing required `--max-census-lines` argument is retained for invocation compatibility but produces no rows in armed mode.

Armed runs report `kind: actual-scheduler-seeded-validation`, a distinct scope, and `census: {enabled: false, files: 0, ...}`. Any entry in the private census directory fails validation. They retain the same 2×N actual physical create/delete checks, baseline restoration and exact owned guest retirement; they report no timing distributions, coverage or seed evidence. `verify_census.py` accepts measuring captures only and refuses this distinct report kind. A passing lifecycle run alone does not establish complete seed coverage or seed eligibility. The coordinator owes actual armed native/guest and local/SSH controls, matching binary/source/policy evidence, cleanup, cancellation and meaningful mutation checks; synthetic process seams do not supply those results.

All state, generated keys/configuration, receipts and results belong under the run's private cache directory; none are source inputs to copy here. Cleanup stops scheduler clients, verifies/deletes only physical boxes from the fresh owned Runtime endpoint and restores its baseline before stopping the backend. Guest retirement uses the private `@guest` record and create-new ownership receipt, exact PID/start/vsock/configuration checks and the existing API attach/stop/wait path. If identity or cleanup cannot be proved, the run fails and retains recovery evidence; hard cancellation may leave exact owned retirement owed. No global daemon stop, broad process scan/kill or cross-tenant cgroup reuse is permitted.

Owed coordinator checks: syntax, report RED/GREEN regressions, current policy/build evidence, actual native/guest debug and release captures, local/SSH capacity boundaries, all WaitKind and capacity success classifications, physical/VM cleanup, cancellation/truncation behavior and later distribution/seed decisions. No result is invented by this packaging.

## Runtime observer source

The included standalone runtime-probe package preserves the exact observer Rust
source used in root935f captures (SHA2560327221bdcff90bdb2428805730b71ca90d114df1780fa22b3d4826cbba822c6).
It is benchmark tooling, never installed with Hex. Its only VM retirement authority
is the fresh private run's exact retained PID/start/vsock/config receipt; mismatched
identity/path/ownership is refused. It uses the existing Runtime and Shards API
seams, not a private socket protocol. No Windows/WHP run is implied by this Unix
fixture. Build it with the profile matching the supplied host/guest binaries and
an owned target directory:

```sh
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="${TARGET:?}" nice -n 10 cargo build --release --locked --manifest-path bench/sched/runtime-pools/census/runtime-probe/Cargo.toml
```

Omit --release for the debug observer. Source-preserved cache copies were compiled
and clippied by root; the packaged layout is separately checked before commit.

## Acknowledgement diagnostics

`status_pressure_e2e.py` imports these unchanged fixtures and adds an explicitly
supplied positive `--status-readers` count. Root r423 and r439 used4readers, N1,
the same633MiB/632MiB boundary,64MiB positive control and original120s outer
cancellation budget. Each reader shares the primary operation's absolute deadline;
all tracked Status children must be reaped and their threads joined. Only a valid
reply or the exact publication-Busy result is accepted. No other control requests,
new grace period, wait floor or readiness extension are introduced. Diagnostic
census rows are never runtime seeds. Supply all the boundary invocation flags
above, replacing the script with status_pressure_e2e.py and adding
`--status-readers 4`.

`pace_trace_report.py` reads an explicit result and explicit trace log paths and
writes a new report only. Example:

```sh
python3 -I pace_trace_report.py --result "$RUN/result.json" --owner-log "$RUN/o.log" --owner-log "$RUN/r.log" --output "$RUN/new-pace-report.json"
```

The parser preserves raw tagged rows, exact nanoseconds and input hashes. It
groups only at explicit queue-enter boundaries per log/node/peer/kind, labels
ordinals inferred, and leaves absent real queue/attempt IDs null. Exact ordered
comparisons are observations; remote Describe is legitimate outer work, and the
parser does not decide causality, seed values or whether an old stall is fixed.
Root reviewed this source and ran its syntax and both actual before/after reports;
the durable results are in ../retry-reset-a98824f7.json. Production contains none
of the temporary tagged trace instrumentation. The two new scripts are benchmark
diagnostics, not installed helpers or a new scheduler code path.


Census completion outcomes now use `name nanos` for an answer, `name !nanos` for a completed typed wire refusal, and the unchanged `name >nanos` for a run-out. Refusals remain online learning samples with exactly the existing maximum/deadline rule, but parsers retain them separately (`refused`, `refused_rows_by_name`) and exclude them from successful-answer counts/distributions. Historical unmarked rows retain their original meaning: old mixed data cannot be retrospectively separated. No refusal text is matched, no runtime seed/deadline is changed, and root still validates actual native/VM outcomes before interpreting a capture.

The marker identifies only wire `Answer::Refused`, not workload/application success. An unmarked `Answer::Scheduled` can describe pending work or contain typed refusal notices; the timing core does not reinterpret that payload. Actual physical lifecycle controls and per-call outcome provenance still determine seed eligibility. This commit makes no new seed decision and cannot retrospectively classify old unmarked rows.

Root actual marker, mutation, complete capture and verifier evidence is recorded
in [wire-refusals.md](../wire-refusals.md). The root verifier opens leaf inputs
nonblocking before checking regular-file type: an owned FIFO without a writer is
rejected, rather than waiting at open. Source/fixed controls and exact raw-file
verification are separate from an eligibility decision or seed installation.

## Bounded reconstruction (coordinator execution only)

`verify_census.py` reads an explicit completed run; it never starts a process/VM, queries a Runtime, installs a seed or decides eligibility. Example, with operator-selected storage budgets and the fixed code source supplying actual protocol/WaitKind bounds:

```sh
python3 -I verify_census.py --run "$RUN" --result "$RUN/result.json" --source "$HEX_SRC" \
  --max-result-bytes "${RESULT_JSON_BYTE_BUDGET:?}" --max-ledger-rows "${LEDGER_ROW_BUDGET:?}" \
  --output "${NEW_VERIFICATION_JSON:?}"
```

Result JSON and ledger bootstrap budgets are explicit because the existing schema supplies neither. Raw file byte ceilings derive from reported rows × actual MAX_MESSAGE; optional reported byte counts are checked too. Every raw/ledger/runtime/owned-record input uses directory-relative no-follow reads within the explicit run. Report-controlled source/binary paths are not followed; `--source` is separate explicit code authority. Inputs changing while open, symlinks, escapes, extra raw files, duplicate filename attribution, inconsistent counts/hash/flags, missing N targets or physical-control evidence are refused. Output is create-new mode0600; existing artifacts remain untouched. Interrupted writes can leave an incomplete new output, which the coordinator must treat as incomplete and retain for cancellation analysis.

The output preserves nearest-rank nanosecond distributions for unmarked raw wire answers, marked wire refusals, run-outs and uniquely filename-attributed call/process exit success. These categories do not establish workload/application success. Legacy unmarked wire outcomes remain explicitly unclassified; missing filename attribution never falls back to PID. Owned guest records/receipts/retirement identities are compared exactly, without live PID/API checks. The coordinator retains responsibility for actual source/binary hashes, Runtime/VM cleanup evidence, cancellation checks and interpretation; packaging/synthetic tests are not an audit of any real capture.

Synthetic tests only (ordinary imports, not `-I`): `python3 -B -m unittest -v test_verify_census.py`. Their scratch is `~/.cache/hex-lead/verifier-synthetic-tests`; no actual capture directory is read by the tests.

## Fresh-node first SDK Hello cohort (root execution only)

`cold_boot_census.py --n N --output NEW_JSON` composes the existing `scheduler_census.py` Harness.
Pass its original explicit guest/profile/source/binary/resource/policy/build/deadline inputs,
`--deadline-seconds 120` and an owned short `--run-parent` cache directory. `--validate-seeds` is
refused. N is positive and explicit; each iteration constructs a separate Harness/private run root,
starts one ordinary node/guest backend, verifies ready API/classes/live guest capacity and an empty
physical baseline, then uses the original finish_backend/cleanup paths. It runs no scheduler,
boxes or workload. Every iteration must retain a matching Stopped receipt and reaped backend before
the next starts. Output is a create-new aggregate plus each original run's raw census/ledger/receipts/
result. Existing capture directories/VMs and r515/r530 are not reused. No seed/timer changes occur.

The selected producer is the exact new backend Popen's unique census filename, not observer files,
PID fallback or warm-borrower data. Only its first completed unmarked `client.guest.answer.hello`
row is the first-SDK candidate; subsequent Hellos, refusals and run-outs remain separate. Source075fd:
`WithGuest::routing` boots through `GuestRuntime::guest`'s ordinary `Client::connect` before node
`Server::with_os` calls `Recorded::attach`/`join_loop`/`join` for its evented client. The two named
call phases derive the normal count; they are not a guessed sample threshold. Unexpected counts or
refusal/run-out evidence make the first-VM cohort ambiguous/excluded. Even normal counts do not prove
per-row VM identity or absence of pre-Hello boot retries: the final recorded guest and SDK row order
are distinct facts. Root reviews that attribution and decides any eligibility. SDK timing begins
after dial/token setup; this is cold-node-boot cohort collection, not full-boot latency.

Pre-readiness failure is an explicit ownership gap in the original Harness: its normal finish path
first queries a possibly unavailable socket. The new driver stops/reaps only its exact saved backend
child, requires an existing bounded regular private `backend_state/@guest`, and uses the existing
remember/stop-owned-guest helpers. A create-new recovery receipt must match any existing receipt's
entire identity/configuration before stop is requested; an existing receipt is never replaced.
Missing/invalid/mismatched records or receipts, or unproved retirement, retain cleanup-unproven and
halt the campaign without another guest. No PID discovery, scan or guessed stop is added. Root must
review this path before any live campaign and still owns all actual failure/cancel/retirement proof.
The same exact-record fallback runs if the original post-readiness snapshot/finish path raises,
including after it already cleared/reaped its backend. Successful fallback retains and re-raises
the original failure, so the iteration stays failed and no next guest starts. Closure reporting uses
the established ready identity or the separately verified recovery identity; proving Stopped through
recovery never supplies readiness, class verification or measurement success. Census lines stream
directly into the bounded classifier; exact rows/full markers remain preserved.

Synthetic only, on approved d132 with 1CPU/1GiB/2GiB, nice10/private Python:
`COLD_BOOT_SYNTHETIC_ROOT="$OWNED_SYNTHETIC_CACHE" nice -n 10 "$PRIVATE_PYTHON" -I -B test_cold_boot_census.py`.
Tests use fake Harness operations and synthetic files; no real CLI/process/VM or actual captures.
POLICY_READ implementer: Shards origin/main `f5cec365d2e435afae5e6f613c92d9d3759ef60f`, full R01–R16
rules SHA256 `d634fcb944959ce1e40a9277d12b0905fcfcebf82d9da9832e826f790a349242`, current AGENTS/evidence/
CLAUDE/handoff and Hex CLAUDE read before edits. Root owns all actual data, guest/E2E/resource/timeout/
provenance/gate review. No FD/task/allocator, competitive-performance or original-failure-cause claim.

Initial eccfc5e9 implementer evidence: approved d132/private Python3.13.18, 1CPU/1GiB/2GiB, nice10; the command above
passed all10 synthetic tests (exit0). The new saved-Popen/pruned-live-list regression first failed
against the initial driver, then passed after preserving that exact Popen authority; no process
lookup was added. Own logs are `~/.cache/hex-lead/cold-boot-census-synthetic/{pruned-red,tests-final}.log`.
Tested source SHA256 `b2c6c9031e39a2b9f0554fc618e4dc39bafcdd048e6eae48db23952fab0ede4d`, tests
`e96ebf649c7c9c7f9ef3f9707823788cf5c31377bbabb4908050980e8074d55f`; local/remote hashes match.
No actual Harness construction, CLI, guest, capture/hash review, VM/E2E/benchmark/profile or seeds
were executed by the implementer. Root still owes review before any live run, actual fresh-guest
cohort/normal-count/identity attribution, malformed/no-ready/cancellation/receipt/retirement cases,
all child/VM closure and resource-return observations, immutable provenance reconstruction and any
later eligibility decision. The existing base Harness and production sources are unchanged.

Cleanup/report follow-up: three new synthetic regressions failed against eccfc5e9 (post-ready
snapshot failure, failure after the original cleared the backend, and recovered closure with no
ready identity). All13 synthetic tests then passed on the same approved runner/envelope/private
Python, exit0. Own logs: `~/.cache/hex-lead/cold-boot-census-synthetic/followup-{red,green}.log`.
Final driver SHA256 `38cfb6a2148e3a05b37773e6523eaa3b9a25a4ec48f9666a09d6c1cb77e1ee2c`, tests
`8fdad8b9b3710f983e065c977caf5256f193b6b1c4e643b7e202dcd2d7f7e0bd`; local/remote match.
These are synthetic-only results; every actual lifecycle/failure/mutation/provenance check remains
root-owned and unrun by this implementer. No additional source scope or seed change is included.

## Cold-campaign raw reconstruction (root execution only)

`verify_cold_census.py` reconstructs a completed cold campaign using the shared verifier's
nonblocking, no-follow, regular-file reader, duplicate-key JSON decoder and integer nearest ranks.
Supply the explicit common parent containing the campaign JSON and its unique direct-child run
directories, plus operator storage budgets. It opens no reported external artifact/source paths,
starts no subprocess/VM/API call and makes no eligibility decision:

```sh
python3 -I "$HEX_SRC/bench/sched/runtime-pools/census/verify_cold_census.py" \
  --run "${COLD_CAMPAIGN_PARENT:?}" --result "${COLD_CAMPAIGN_JSON:?}" --source "${HEX_SRC:?}" \
  --max-result-bytes "${RESULT_JSON_BYTE_BUDGET:?}" --max-ledger-rows "${LEDGER_ROW_BUDGET:?}" \
  --expected-source-revision "${CAPTURE_SOURCE_REVISION:?}" --output "${NEW_VERIFICATION_JSON:?}"
```

Positive N, exact ordinals/counts/unique roots, one consistent declared debug/release profile,
each original result/runtime/ledger, the exact backend
filename producer and ordered recorded-Popen reap, ready OCI/empty baseline, guest PID/start/vsock,
receipt/configuration and exact Stopped closure are checked. The supported configuration is this
campaign's default private read-only `run/guest-shares`; explicit alternative shares are refused.
Guest binary/kernel/initrd paths and vCPU/memory declarations are compared with the recorded artifact
and environment authorities. Root still independently verifies artifact bytes, build/policy receipts
and live retirement; this reader does not follow those external paths or query a process/socket.

Every opened authority input receives SHA256/byte counts. Ledger and raw files stream within explicit
ledger-row budgets and each run's source-checked max_frame/max_rows bounds. All ledger-attributed
census files are reconstructed, but directory extras are not enumerated; unrelated observer files
never enter the backend first-Hello distribution. SDK first-completed, normal-source-order,
subsequent, wire-refused and run-out cohorts remain separate. Passed captures with missing, malformed
or truncated backend rows are refused, and aggregate cohort summaries must match raw reconstruction.
Normal counts retain `per_row_VM_identity_proven: false` and
`unobserved_pre_hello_boot_retries_possible: true`. Recorded no-workload/empty-baseline facts are not
an independent Runtime.create effect counter. No whole-boot latency, workload, seed or performance
claim follows from this output. Existing results are never replaced; interrupted create-new output
remains incomplete evidence for root's cancellation review.

Synthetic-only focused check, with ordinary imports and private cache scratch:
`nice -n 10 "$PRIVATE_PYTHON" -B -m unittest -v test_verify_cold_census.py` from this directory.
Scratch is `~/.cache/hex-lead/cold-verifier-synthetic`; fixtures use invented rows/identities only,
never measurements or seeds. Root owns all actual reconstruction/hash/provenance/cleanup/cancellation
checks, mutation interpretation and combined validation. No actual capture was read by this scoped
implementation.
