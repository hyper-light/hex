# Actual guest SDK Hello census helper

CP-T7.4 / decision 9: root's actual Guest census retains one client, leaving only two
`client.guest.answer.hello` samples per profile. This standalone **benchmark-only** tool repeatedly
opens/drops the existing `hex_client::guest::Client` through the actual borrowed Shards connector.
It is outside the production workspace, is not an installed product, creates no boxes, and never
acquires, attaches, adopts, stops or deletes a Guest. Root owns every real run and guest retirement.

The base is Hex `bba3d814cfa8373d589482dfbaa38b9bfa792d03`; it uses precisely that vendored API,
not the unmerged upstream borrowed candidate. Production/client/vendor sources and runtime-probe are
unchanged. `ShardsStream` has a private tuple constructor; `ShardsLink` requires an owning Guest.
The small benchmark-only `Duplex` wrapper therefore forwards the existing `shards_api::Stream`
without an extra reader, copier, unsafe code, socket conversion or production API expansion.

## Inputs and boundaries

All flags have explicit values; missing, duplicate and unknown flags are refused:

- `--owned-root`: fresh, root-controlled absolute private run directory (0700, Unix). This helper
  currently refuses non-Unix execution because it cannot prove private-directory ACLs with std.
  Windows compilation/actual ACL+guest coverage is not claimed.
- `--record`, `--receipt`, `--vsock`: absolute paths. Record and receipt must be within that private
  run; their PID/start/vsock and the explicit vsock must match. Use the existing runtime-probe receipt
  format including `declared_config`. The helper reads it; it does not create ownership authority.
- Optional `--owned-vsock-root`: explicit same-owner private 0700 directory already named by the
  receipt when the actual socket is outside the run root. Otherwise vsock must be inside the run.
  Leaf links and escapes through parent links are refused. Source/file/receipt binding supplements,
  and does not replace, root's proof that it owns this fresh VM and these paths.
- `--iterations`: positive u64; one new actual SDK client/Hello per iteration, then drop. No sleep,
  retry loop, default count, mock endpoint or workload is supplied.
- `--dial-nanos`: positive u64 nanoseconds. Zero, Duration::MAX/unrepresentable input, clock overflow
  and zero/MAX duration are refused before CONNECT. Revalidated before each borrowed dial. The
  existing API verifies PID+start before/after its one connection attempt.
- `--expected-class`: **Runtime API** `oci` or `process`, not a scheduler VM class. The successful
  Hello must advertise it. Cached `Client::classes()` validates the Hello; no extra RPC is invented.
- `--source-revision`: full lowercase 40-hex source revision; `--profile`: debug or release. These
  are operator declarations retained in JSON, not independently certified by the helper. Root must
  retain matching source/build receipts and review them against the current executable SHA256.

Set both existing `HEX_WAITS_CENSUS` (`hex_node::census::ENV_FILE`) and
`HEX_WAITS_CENSUS_MAX` (`ENV_MAX`) explicitly. The file must be create-new inside the private run;
the positive row bound must hold at least the requested Hello count. The existing census installer
and SDK timing points generate all samples. The fixture generates no timing values, quantiles or
seeds; root still verifies actual rows, full markers, wire success/refusal/runout attribution and
counts. A successful wire Hello and class advertisement are **not workload success**.

**The dial argument bounds borrowed CONNECT only.** The SDK's relay-token/Hello waits remain
measuring under the existing census policy. No per-RPC deadline enforcement is claimed. Root must
start a tracked subprocess with an explicit finite outer cancellation budget, retain its exact
Popen/PID and kill/reap it if any SDK call or drop hangs. Do not create an uncancellable timeout
thread or install a guessed RPC seed to get a result. Cancelled/incomplete observations are never
seeds. Root retains VM lifecycle ownership on both success and failure.

Normal completion writes one JSON summary to stdout with executable SHA, declared revision/profile,
record/receipt hashes, exact identity/paths/configuration, iterations, wire-Hello count, expected-class
validation count and existing census path/bound. A protocol/class failure returns nonzero with partial
counts. A hang/forced termination may leave no complete JSON: retain stderr/exit/PID and census as
incomplete, never substitute zeros. A setup error returns JSON failure before dialing.

## Root commands (actual guest runs unrun by implementer)

Build/light controls use this standalone manifest and an owned target only. Never build/test on the
laptop. Approved implementer runner is d132, 2 CPUs / 2GiB / 35GiB, jobs/testthreads2, nice10, actual
disk >=30GiB; private Rust toolchain1.99.1. This builds only the helper and existing API dependencies,
not Shards VMM/carried helpers/kernel. It does not install anything.

```sh
HELLO_MANIFEST="$HEX_SOURCE/bench/sched/runtime-pools/census/guest-hello/Cargo.toml"
export CARGO_TARGET_DIR="$OWNED_TARGET" CARGO_BUILD_JOBS=2
cargo generate-lockfile --offline --manifest-path "$HELLO_MANIFEST"
HELLO_CENSUS_TEST_ROOT="$OWNED_TEST_ROOT" nice -n 10 cargo test --locked --manifest-path "$HELLO_MANIFEST" -- --test-threads=2
nice -n 10 cargo clippy --locked --all-targets --manifest-path "$HELLO_MANIFEST" -- -D warnings
# Root only: use the explicitly selected profile, matching source/build receipts and recorded child.
nice -n 10 cargo build --locked --release --manifest-path "$HELLO_MANIFEST"
HEX_WAITS_CENSUS="$FRESH_RUN/census/guest-hello.tsv" HEX_WAITS_CENSUS_MAX="${ROW_BOUND:?}" \
  "$OWNED_TARGET/release/hex-guest-hello-census" --owned-root "$FRESH_RUN" \
  --record "$FRESH_RUN/node.state/@guest" --receipt "$FRESH_RUN/guest-receipt.json" \
  --vsock "${EXPLICIT_VSOCK:?}" --iterations "${ITERATIONS:?}" --dial-nanos "${DIAL_NANOS:?}" \
  --expected-class oci --source-revision "${HEX_SOURCE_REVISION:?}" --profile release
```

The final line is an invocation template **inside root's tracked outer-cancellation wrapper**, not
permission to run it without that wrapper. Supply `--owned-vsock-root` only when its exact value is
already bound in the receipt. Never point this tool at r468/r469 or any unrelated live guest/data.

## Root harness composition and reported actual controls

Root's separate [`guest_hello_e2e.py`](../guest_hello_e2e.py) composes this helper with the existing
`scheduler_capacity_e2e.py` boundary fixture through `importlib`. It accepts explicit
`--capacity-fixture`, `--hello-helper`, `--helper-revision`, `--hellos` and `--hello-dial-nanos` values;
the remaining arguments go to the original boundary/census harness.

After that harness starts its own fresh guest backend and records its ownership receipt, the
wrapper checks the live capacity identity `guest:<pid>:<started>`. It launches this helper as an
owned, recorded subprocess with the receipt's exact record/socket/optional owned-vsock-root, the
explicit count/dial budget, expected `oci` API class, declared helper revision/profile, and a
separate actual census file. The existing operator budget bounds the subprocess: expiration kills
and reaps that exact child using the existing cleanup budget. No SDK RPC timer is added.

The wrapper requires successful helper exit, matching requested/Hello/class-validation counts,
borrowed-only status, and an identity equal to the receipt. It then checks that the guest record is
unchanged, live capacity still identifies the same guest, and the physical box baseline is unchanged.
The original local and SSH boundary exercises follow, including oversized refusals and valid physical
create/delete controls. The original root-owned backend/guest cleanup remains responsible for final
retirement. These are separate protocol, physical-runtime and lifetime observations.

The coordinator reports completed debug and release runs **r489/r490**:

- Each profile's helper completed **1,000 actual SDK Hellos** with the expected class verified.
- The same guest identity remained live after borrowers dropped. The composed trial then completed
  the ordinary **two positive physical create/delete controls and two oversized refusals**.
- All guests were subsequently **Stopped** by their root-owned lifecycle cleanup.
- Root reconstructed the actual raw census. **No seeds were installed.**

This section records the coordinator's supplied results; this documentation implementer did not
read, hash or audit those captures or execute the harness. Root supplies the detailed data/provenance
JSON separately, including exact per-profile run mapping, source/build receipts, binary hashes,
artifact paths and row reconstruction. The earlier implementer checkpoint below covers only its
own synthetic checks.

`client.guest.answer.hello` timing begins at the existing SDK Hello wait, **after borrowed dial and
relay-token setup**. These samples do not measure full connection acquisition, VM boot, staging or
workload execution. Protocol/class counts establish Hello success; the separate boundary controls
establish their observed physical effects. Guest identity surviving borrower drop and subsequent
Stopped retirement establish those bounded lifetime controls. **FD, task, handle, allocator and kernel
resource return/leak freedom were not proved.** Timeout/cancellation/failure cases and other-platform
coverage remain unverified unless root records their separate evidence. No competitive performance,
whole-library panic-free or seed-eligibility claim follows from these runs.

## Policy/evidence record and owed validation

POLICY_READ implementer, Shards origin/main `f5cec365d2e435afae5e6f613c92d9d3759ef60f`, full
`docs/development-rules.md` SHA256 `d634fcb944959ce1e40a9277d12b0905fcfcebf82d9da9832e826f790a349242`.
AGENTS/development-evidence/CLAUDE, managed-runtime-plan and both current handoffs read before edits;
coordinator acknowledged exact ownership and light-check envelope. A receipt is not compliance proof.

| Rules | Applicable implementation / remaining proof |
| --- | --- |
| R01 | Fallible parsing/paths/counters/serialization; no new unsafe. Benchmark/library allocation and synchronization behavior still needs root failure review; no panic-free claim. |
| R02/R03/R05/R12 | Reuse pinned borrowed API + current Link/Duplex/SDK/census. Private constructor necessitates only a forwarding bench adapter. Budget/count explicit; no new cache, manager, algorithm, timer or upstream candidate. Root owns actual cost/compatibility evidence. |
| R04/R06/R07/R13 | Validate authority, install create-new census, repeat connect/class-check/drop, emit completed/failed counts. Borrowed streams own only sockets and are RAII-dropped on errors. Root's bounded lifetime controls are reported above; complete resource-return and cancellation proof remains owed. |
| R08/R14 | No comparative/fast/allocation-free/leak-free claim; implementer runs no benchmark/profile. Root owns counts, FD/handles/tasks/kernel costs, latency/cost comparisons and misses. |
| R09/R10 | This fills an observed sample-count gap, not a claimed SUT fix. Synthetic argument/containment/receipt tests are focused controls. Root's reported actual Hello and boundary results are separate; timeout/cancel/failure and resource-return evidence remains owed. |
| R11 | Existing portable stream API retained; no Intel Mac. Focused approved Mac compile only if recorded. Other targets/real KVM/HVF/WHP and non-Unix private-root proof remain unverified. |
| R15 | Implementer owns only this helper/manifest/lock/README; no children, heavy checks, live guest access, merges or pushes. Root audits evidence, runs actual guests and validates integration. Shards policy script/combined gates remain root-owned/unrun here. |
| R16 | No cache cleanup/deletion of experiments or active jobs. Private target/source, >=30GiB floor and headroom required; no runner/service actions. |

Before accepting sample provenance, root must independently prove: correct fresh VM PID/start/receipt;
wrong-start/dead-process/no-dial refusal; finite CONNECT timeout and withheld/partial Hello outer
cancellation; every client/drop closes all clones; repeated success/failure/drop FD/task/handle counts
return to baseline; VM stays alive after borrowers drop; root finally retires only its VM; all children
are reaped; requested/Hello/class counts match complete actual census rows without truncation or mixed
refusal promotion. No mock/synthetic row is eligible. Record exact helper commit, commands, toolchain,
profile, source/binary/artifact hashes, raw rows and unrun cases. The helper does not install seeds.

### Implementer focused checkpoint

Approved d132 (macOS arm64), rustc `1.99.1 (58cf8f9c3 2026-10-13)`, jobs2/testthreads2,
nice10, explicit 2CPU/2GiB/35GiB envelope. Actual disk available before checks was
2,947,156,080KiB, above the policy's 30GiB floor. Own source was copied from immutable
`snapshots/refusal-census-bba3d814` into `~/.cache/hex-lead/guest-hello-census/source`, then only this
new helper overlaid; no commands used shared active sources/targets or live capture data.
Target: `~/.cache/hex-lead/target-guest-hello-census`; synthetic filesystem root:
`~/.cache/hex-lead/guest-hello-census/synthetic`.

- r59: helper compiled, two synthetic tests passed. Clippy refused assertions in the test-only
  module under the repository's disallowed-macro rule. Added the standard test-only allowance;
  no behavior/lint allowance was added outside `#[cfg(test)]`.
- r61 final, exit0: `cargo test --locked --manifest-path
  bench/sched/runtime-pools/census/guest-hello/Cargo.toml -- --test-threads=2` — 2 passed, 0 failed,
  0 ignored; `cargo clippy --locked --all-targets --manifest-path` the same manifest `-- -D warnings`
  passed. Both ran with the envelope/toolchain above. No helper main, SDK dial, real VM, E2E,
  benchmark, profile or full suite was executed.
- Logs: own `~/.cache/hex-lead/guest-hello-census/{unit-final,clippy-final}.log`; first check's
  `unit.log`/`clippy.log` also retained. Exact tested helper SHA256
  `5d5f6f5512b7286d8ab7f111058fc21a06298406cb3a119ac5dd7eebff4ea64f`; lockfile
  `8cf3a77177cf5679113782b7823486827046a1c8d6b8a5a194577f2186b09f1a`, equal locally/remotely.

These focused checks prove neither actual borrowed-connection cleanup nor census validity. The
coordinator's later actual controls are recorded separately above; remaining resource, timeout,
failure and other-platform proof stays explicitly owed to root.

Documentation-only follow-up: POLICY_READ implementer renewed against current Shards origin/main
`f5cec365d2e435afae5e6f613c92d9d3759ef60f`, rules SHA256
`d634fcb944959ce1e40a9277d12b0905fcfcebf82d9da9832e826f790a349242`, with current AGENTS,
development evidence/CLAUDE/handoff and Hex CLAUDE read before edits. Only this README changed;
no capture access/hashing/audit, source changes, tests, builds, VM or benchmark execution were
performed for the prose update. Root owns detailed evidence, review and merge.
