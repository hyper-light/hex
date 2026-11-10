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

The harness installs the existing measuring census, leaving runtime waits unseeded while collecting actual answers. A captured duration is not automatically a usable seed or a performance result. Coordinator review must establish profile/source/binary identity, success provenance, row completeness, no truncation and the physical controls before interpreting distributions or seeding anything.

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
