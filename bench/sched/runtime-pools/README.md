# Runtime-pool allocation profile (CP-T7.4)

The side-storage candidate was rejected and reverted incc7c2190 after the quiet
comparison below. The capacity fix remains; the effective production tree is
identical to63ec2758 at cc7c2190. Later retry/census changes and their exact
revisions are recorded below. These are coordinator measurements of the single FitIndex
admission/placement path, not external competitive performance results. All runs used the arm64 Mac Studio, macOS26.4,
Rust1.99.1 release builds with the same18 resolved dependencies, thin LTO, one codegen
unit and overflow checks. Root ran the builds and measurements; implementers prepared
source and ran only their earlier focused implementation checks.

The optimization compares exact63ec2758d8f2ef89cde84d48bb566485eeb60e36 (inline pool
state) with935f6b21f98ebb11a58a605dc2447c0f6770910b (node-indexed side storage).
The older b9489761 constructor has no pool feature, so it is an explanatory baseline,
not an equivalent competitor. The side change preserves the host-plus-pool verdict
and atomic take/give constraints; it is not a new scheduling algorithm.

## Inputs and method

10,000 nodes are AC-T7.4.2's count. Each host has48,000mCPU and412,316,860,416bytes,
the committed Huawei m5d.metal trace shape. Five rounds follow the existing
fast_path record method; they do not establish a confidence interval.

The first constructor capture used default nodes with no finite runtime pool.
The second capture supplies one Shards pool per node:4,000mCPU and632MiB, matching
the actual VM census configuration. Its demand is the same1,000mCPU/64MiB valid
workload. Verdict mode rotates candidates; fit mode runs the actual global
FitIndex::fit with Scorer::HEX. Every selected node is charged and released, and
give must restore its exact key. Each round has1,000 measured cycles and explicit
zero warmup; cold samples are retained. This is an empty-index healthy path, not
the90%-loaded trace benchmark or a real VM timing measurement.

A forwarding System global allocator records successful allocations, reallocations,
deallocations and requested/live layout bytes. Inputs, Pick/Shapes and preallocated
sample storage remain live across cumulative snapshots. Each SUT constructor,
decision, take, give and drop is timed separately; JSON and statistics run after
index drop. Counters are never reset while allocations remain live. External
/usr/bin/time -l records whole-process CPU, RSS and faults separately. Copied bytes
are unmeasured; allocator counts do not measure copying, kernel memory or allocator
metadata. Raw samples and hashes remain in the linked JSON records.

## Observed counts

| Input | Inline retained bytes | Side retained bytes | Reduction |
| --- | ---: | ---: | ---: |
| 10,000 default nodes, no pool | 5,904,724 | 4,704,724 | 1,200,000 bytes |
| 10,000 nodes, one finite pool each | 5,904,724 | 5,104,724 | 800,000 bytes |

All five rounds in each case had the same retention. Default-node constructors
made10 allocations inline and11 with side storage. A finite pool adds one owned
pool declaration allocation per node in side storage: construction and hot-path
costs are separate. In both versions, all5,000 decisions,5,000 takes and5,000 gives
per decision mode made zero observed allocator calls or reallocations. This does
not establish allocation freedom for device binding, failures, refresh or other
inputs. The tracked live-byte delta returned to zero after each index drop; this
is not a process-wide leak-freedom claim.

The default-node whole-process maximumRSS was6,389,760bytes for the older b948,
9,043,968inline and7,864,320side. Page reclaims were762/924/852; major faults1each.
These whole-process observations include harness costs and are not index-retention
counts. Finite-pool process observations are retained in the second JSON record.

Root runs: r350 validates the initial constructor probe; r357 builds the side
variant; r377 compiles/clippies both extended probes; r390 validates and measures
actual pool operations. Root also validated constructor compatibility, nonvacuous
fit walks, exact healthy cycle/key restoration and oversized-input refusal for
both binaries. The source binaries and raw JSONL/time file hashes are recorded. Packaged-layout
validation first exposed Cargo automatically treating the module as a standalone
bench (r394). Explicit autobenches=false retains only the declared tool entrypoint;
root r396 then passed package clippy, release build and constructor smoke.

## Limits and repeatability

Concurrent root workspace gates and actual VM census runs loaded the host. The
operation capture's load was about5; it fails the repository's quiet benchmark
condition. All recorded durations are diagnostic, and no speed/throughput ratio
or AC-T7.4.2 latency pass follows. Tiny verdict intervals can be zero at this
clock's resolution; no invented overhead is subtracted. The initial loaded capture establishes no latency improvement. A separate quiet
comparison, below, caused the candidate to be rejected.

The standalone allocation-probe package contains the measured source, with its
entrypoint under benches/ to keep it out of production. Build with an owned target:

```sh
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="${TARGET:?}" nice -n 10 cargo build --release --locked --manifest-path bench/sched/runtime-pools/allocation-probe/Cargo.toml
```

For a version comparison, point all three manifest paths at one immutable source
per copy, preserve equivalent locks/toolchain/flags, and run the executable with
the exact arguments in allocation-loaded-935f6b21.json. Preserve stdout JSONL,
stderr/time output, source/binary hashes, exit status and concurrent load. Read the
probe README for the counting/failure boundaries. No helper is installed.

Data: constructor-loaded-935f6b21.json and allocation-loaded-935f6b21.json.
Raw remote paths are retained in those records on runner
 d1323898b255a3ed435eb83fa0f7b0cf under~/.cache/hex-lead.

## Quiet comparison and decision

Root r401 first ran5paired rounds of1,000global-fit cycles per version on the same
Mac16,9/Apple M4 Pro,14cores,36GiB, macOS26.2. Both immutable release binaries were
transferred unchanged; no build or VM run occurred there. Every before/after load
check was below2 and found no visible compiler/build process. The candidate p99 was
higher while paired directions varied, so root extended the capture rather than
calling this a speed improvement. The initial data remain in fit-quiet-935f6b21.json.

Root r403 used the same inputs/binaries,5paired rounds of10,000cycles per version
(10,000 is AC-T7.4.2's placements/s count, supplied to the probe, not a runtime
constant). AB/BA order alternated; no warmup or sample was discarded. Every fit
walked all10,000nodes and every take/give restored the exact key. All ten runs
passed the same quiet checks; load ranged about1.23-1.30. Raw files/hashes,
whole-process time/fault observations and per-round distributions are retained in
fit-quiet-n10000-935f6b21.json.

| Global fit,50,000samples/version | Inline63ec | Candidate935f |
| --- | ---: | ---: |
| p50 | 138,000ns | 138,208ns |
| p90 | 146,208ns | 155,209ns |
| p99 | 162,875ns | 189,292ns |
| max | 644,125ns | 953,375ns |

Four of five paired p99 values were worse for the candidate. Five paired rounds
do not establish a confidence interval or isolate the microarchitectural cause,
but the measured memory saving does not justify accepting this latency tradeoff
under the user's latency-first instruction. Root restored inline state exactly;
no new cache or replacement algorithm was introduced. The rejected signed commit
and both positive/negative measurement records remain in history. These results
are specific to this healthy empty-index workload and allocator-instrumented
binaries, not a claim about real VM start speed or a named external competitor.

## Actual Runtime census records

census-release-935f6b21.json and census-debug-935f6b21.json are root-verified
summaries of actual ordinary Hex nodes/real Shards guests locally and over forced
OpenSSH. Each contains2,000physical creates/deletes, restored empty baselines and
owned-guestStopped proofs. Root verified all107,546release+129,854debug census
files and reconstructed the reported counts/distributions with0hash mismatches
or wait-expiration rows. Each profile includes4,004capacity observer answers
joined to successful processes by unique census filename, with no PID fallback.

Raw multiplexed rows are labeled separately; not every raw row is a verified
healthy single call. No seeds are installed by these records. Their SUT source
is935f, including the rejected storage representation; never relabel them ascc
or the later retry fix. They ran under declared concurrent load and are not quiet
VM performance comparisons. The source-preserved actual fixture and observer are
in census/. Its ordinary synthetic report tests2/2 and observer package clippy/
release build passed in root r407; no helpers are installed.

The subsequent freshcc release boundary failed acknowledgement liveness while
both oversized admission checks passed; its guest was retired. The capacity
record in docs/research/scheduler/runtime-pools.md states this unresolved limit.
A root diagnostic repeat with4additional Status callers passed and proved all
Status-child joins/reaps; those diagnostic rows are explicitly forbidden seeds.

## Retry measurement correction

Signed a98824f7 excludes a local Advertise retry's own pause from its next work
sample. The regression was run red before the fix: virtual requested pauses grew
[1,2,4]ns for equal work; after the reset they remain[1,1,1]ns. The independent
root mutation check r452 reproduced that exact red after removing the
reset (source equal to96a86a73), then passed after restoring a988; see
retry-reset-root-mutation-a98824f7.json. Root r430's
uninstrumented debug boundary and five release boundaries all passed both
oversized refusals, both valid physical lifecycles, baseline restoration and
guest retirement. This is bounded replay evidence, not a competitive timing claim
or proof of the original intermittent stall's sole cause.

Root r423/r439 provide separate actual Status4 before/after traces. Before, one
refresh's requested pauses grew4,802,584/16,732,875/62,724,209ns despite Busy calls
of2,583/2,167/4,667ns. After, a refresh's first6,697,375ns actual outer sample
remained its requested pause across eight subsequent1,125-1,875ns samples.
Both traced runs passed their physical controls and joined/reaped every diagnostic
Status child; both owned guests stopped. Initial remote Describe time is retained
as real work. Source and ordered trace data are preserved in retry-reset-a98824f7.json
and census/{status_pressure_e2e.py,pace_trace_report.py}. Missing real queue IDs stay
null; group ordinals are derived only from explicit queue-enter boundaries.

The trace binary is explicitly temporary cc+the same minimal fix+DEBUG, separate
from exact uninstrumented a988 binaries. These pressure/trace rows are forbidden
seed inputs. No runtime constant, deadline or grace interval was raised. Root r427
passed the complete seven-target/workspace/vendor gate at exact a98824f7; see
gates-a98824f7.json for command/exit/log hashes and ignored cases. Native Linux
and remaining calibration checks are still owed. Later census-outcome source
changes require their own validation; this gate is not relabeled to that source.

## Current classified captures

Root completed fresh debug/release captures at bba3d814 after the retry timing
correction and typed refusal census marker. Each profile passed 1,000 local and
1,000 SSH physical create/delete cycles, restored the empty baseline and retired
its exact guest. Root reconstructed every SHA, marker/count and filename join in
194,912 files and 567,541 rows. The records are
census-{debug,release}-bba3d814.json; wire-refusals.md explains classification and
mutation checks. These loaded measuring runs do not certify installed seeds.

Separate fresh guests provided 1,000 class-validated SDK Hellos per profile. A
post-borrower-drop capacity RPC still named the same live guest, then the normal
boundary controls passed and each guest ended Stopped. See
guest-hello-{debug,release}-fa5c7e4e.json. This proves the stated identity/lifetime
checks, not FD/task/allocator leak freedom. The SDK sample begins after dial/token
setup and does not measure CONNECT or VM boot latency. Root's FIFO reproduction
and verifier correction are in verifier-nonregular-controls.json.

Root r462 passed seven-target lint, native macOS workspace checks and vendoring
at exact bba3d814; gates-bba3d814.json retains exits and ignored cases. Native
Windows core/client checks and the real classifier mutation passed separately.
Native Linux/root-lab, calibration and other unrun platform/resource checks
remain unverified. No current capture guest remains live and no seed was
installed by this evidence batch.

## Candidate wait seeds: broader validation failed

Signed bbb3ef21 selected exact macOS-specific per-profile maxima for 14 response
waits; calibration-macos-bba3d814.json retains the operation/sample provenance.
Root r500 passed51 client cases and seven-target client lint. Root r503 built
both profiles; source-equivalence checks compared441 crate/vendor/root build
inputs against integration e2fb4f17 with no mismatch. These results do not make
the candidate ready for main.

Both armed N1 capacity boundaries passed (r510 debug and r512 release): two
oversized refusals, two valid physical cycles and empty baseline per profile,
with each guest Stopped and no census installed. A private held-open Unix peer
control also distinguished the modes: actual debug/release CLI calls refused
without a reply when armed; re-enabling measuring census left the same debug
call waiting until the unchanged120s administrative cancellation reaped it.
This is timeout-mode sensitivity evidence, not a workload or speed claim.

The larger armed release run r516 failed local Delete after180 complete physical
cycles, at166.381084ms (twice the recorded83.190542ms). Its guest ended Stopped;
complete-run count/baseline fields are absent and are not invented. The full
e2fb4f17 gate r505 separately failed guest_loop's reboot-count assertion (three
boots instead of two). Isolated original and single-variable diagnostic repeats
did not reproduce that symptom, so neither a timing cause nor a fix is claimed.
No seed/deadline was increased. mac-seeds-validation-e2fb4f17.json preserves
positive and negative results and the scope of those diagnostics. Further root
diagnosis/calibration and a passing final combined gate remain required.

## Completed extension and native Linux quota guard

The debug armed N1000 run completed 1,000 local and 1,000 forced-SSH physical
create/delete cycles, restored its empty baseline and stopped its exact guest.
Root reconstructed all 272,257 ledger rows and confirmed census remained absent;
armed-debug-e2fb4f17.json preserves that bounded validation. It does not override
the separate failed release validation or combined gate above.

The fresh unarmed release extension also completed 2,000 physical cycles and
stopped its guest. Root reconstructed all 86,530 files and 262,641 census rows;
census-release-round2-bba3d814.json preserves the distributions. Completed Delete
answers reached 150.082083ms. The existing decision9 rule also retains the earlier
166.381084ms run-out as a censored lower bound; the fresh completed maximum does
not erase it. This batch changes no wait values or eligibility decision.

Root additionally cross-built the exact hex-node Linux ARM64 musl library test
and ran it as an OCI init through the production laptop scheduler in a real
Shards/HVF guest. The fractional CPU-quota test passed; changing only the quota
arithmetic to truncate whole CPUs before multiplying failed with exit101; the
byte-identical restored fixed binary passed again. Each exited init's box stayed
tracked until explicit deletion, the empty physical baseline returned, and guest
25719/start12412173209405 ended Stopped. The record is
native-linux-arm-quota-8f51140e.json; census/native_quota_e2e.py is the exact live
wrapper. Invoke it with the existing cold-census arguments plus --cold-driver
PATH/TO/cold_boot_census.py and --unit-artifacts BUILD-METADATA.json. That metadata
names the fixed and whole-cpu-truncation-mutant artifacts, their source/binary
SHA256s and the exact filtered test; build each source with an explicit manifest
and a separate private CARGO_TARGET_DIR before dispatch.

The first attempt retained an invalid control: Cargo reused the fixed artifact
for the mutated source in a shared target directory. Both saved hashes were the
same, and the mutant exited0. The failed result and its Stopped receipt are
preserved. The corrected dispatch required distinct artifact hashes first.
This is one native ARM64 parser/guard execution with real scheduler lifecycle
checks, not an AMD64 gate, cgroup-controller integration, root lab, timing seed,
resource-return benchmark or whole-goal completion.

Root also exercised cold-node failure closure at both parent boundaries: after
the real ready line but before its original receipt, and at the first finish
snapshot after verified readiness. Both requested two iterations and started
only one. Each original failure and its generic census error remained a failed
result; exact record/receipt reconstruction confirmed the guest ended Stopped
and the saved backend child was reaped. The post-ready original and recovery
receipts matched completely. cold-failure-closure-8f51140e.json retains those
errors and identities. census/cold_fault_e2e.py is the exact actual wrapper;
use --cold-driver PATH/TO/cold_boot_census.py, --fault pre-ready or post-ready,
then the existing cold-census arguments with --n 2 and a new output path. These
injected-failure rows are excluded from calibration and workload success.

## Cold-start and tail extension candidate

Root r571 independently reconstructed all 14,000 census files and 44,000 rows
from the two fresh-node N1000 campaigns. Each profile had 1,000 normal first
completed SDK Hellos and 1,000 subsequent Hellos, with no marked Hello refusal,
run-out, malformed/full row or ambiguous cohort. Root separately enumerated every
census directory entry and hashed all external artifacts and build/policy inputs.
The compact records are cold-sdk-hello-{debug,release}-8f51140e.json; they retain
the full reconstruction and authority-manifest hashes. All exact guest identities
and recorded Stopped results agree. Normal source-order counts still cannot prove
each row's VM identity or exclude an unobserved pre-Hello boot retry.

Fresh first-Hello maxima were 295,291ns debug and 3,022,291ns release, compared
with the earlier warm-borrower maxima of 197,042ns and 202,583ns. The SDK timer
begins after dialing/token setup. These are response records, not boot timings.

calibration-macos-cold-extension-1bb73f74.json applies the existing decision9
method across the eligible warm/fresh/subsequent cohorts and uncensored release
extension, retaining larger prior records. It also preserves the earlier Delete
166,381,084ns run-out as a distinct censored lower bound, rather than treating it
as a completed answer or dropping it below the new 150,082,083ns completed max.
The learner and TIMES_SLOWEST rule are unchanged; only these macOS records change.
This is a new candidate requiring actual armed fresh-node/local/SSH validation
and final combined gates. It does not claim a cause or fix for the unreproduced
guest_loop reboot-count failure. Other platforms and measuring waits retain
their recorded scope and remain unverified where no actual record exists.

The new candidate subsequently passed 1,000 fresh armed nodes per profile. Root
reconstructed all 10,002 authority inputs and 32,000 ledger rows, checked every
empty census directory and external artifact/receipt hashes, and matched all
2,000 unique guest identities and exact recorded Stopped closures. These results
are armed-cold-{debug,release}-265c1501.json. They establish the stated fresh-node
readiness/closure controls, with no timing or workload claim.

The release physical boundary also passed two oversized refusals and two valid
local/SSH cycles. Its larger physical run then FAILED on Submit at456.592ms after
106 complete local cycles. The final durable owner/runner rows for key0/1/106
were created/running while the caller had timed out without confirmation. Record
data mtimes alone cannot locate a delay or establish fsync completion. The guest
ended Stopped; final full-run baseline/count fields are absent and not invented.
armed-submit-negative-df534d2f.json retains exact input hashes and the failure.
The calibration manifest is explicitly failed again. No further record value or
deadline changed. The separate profiled run completed 1,000 local and 1,000
forced-SSH physical cycles. Root reconstructed 103,238 raw files, 296,104 rows
and 213,502 ledger rows. Submit's maximum observed call-exit time was
118,030,084ns; the timeout was not reproduced. The three retained native stack
samples include ordinary runtime creation and directory synchronization, which
do not establish the earlier failure's cause. These data have unquantified
profiling overhead and remain excluded from calibration and performance claims;
diagnostic-submit-profile-df534d2f.json records the exact sources and hashes.

The exact df534d2f combined gate passed all seven target lints, format,
production checks, native macOS workspace tests and vendoring. Its 957 passing
test executions include a separate focused 51-test client run; 13 were ignored.
Passing test stderr is hidden, so internally skipped hardware cases are not
claimed as live coverage. Native Linux AMD64/root-lab and Windows ARM VM proof
remain outside this result; see gates-df534d2f.json.

Diagnostic d4d91e3a's trace-off N1 control passed both physical cycles with no
markers. Trace-on failed during an unrelated CLI Hello wait after sending took
longer than its 1.659ms budget. Both exact guest identities ended Stopped with
the original backend reaped. This is distinct from the armed Submit failure;
socket readiness includes error/hangup and does not prove a complete queued
answer. The original failed trace is retained in diagnostic-trace-controls-d4d91e3a.json.
The next separate diagnostic revision limits client traces to typed Submit.
A passing gate or fresh-node campaign does not certify the failed physical
scheduler lifecycle or the whole goal.

The physical diagnostic adapter now shares the existing cold fault hooks rather
than owning another recovery path. Root injected both the pre-ready receipt
boundary fault and the post-ready finish-snapshot fault. Each remained failed,
retained its original error, reaped the saved original backend and joined its
record/receipt to the recorded Stopped identity. The refactored standalone cold
entrypoint also stopped after the first injected failure despite requesting two
iterations. Generic failed-iteration cleanup messages remain visible. Independent
hashes and identity joins are in diagnostic-physical-fault-controls-aa2f0bdd.json.
That record also retains Submit-only client trace8cc99eb3's unrelated LinkUp
289.25µs timeout; its Submit completed in110.665333ms. Both trace controls again
retired their exact guests. Daemon traces are now being restricted to Submit's
synchronous scope in separate diagnostic1467ca67. Its N1 controls both passed:
trace-off had no markers; trace-on had94, limited to Submit. Root independently
joined both local/remote physical events and each record/receipt/Stopped identity.
See diagnostic-submit-trace-n1-1467ca67.json. Larger reproduction remains separate;
no production change or new timing seed follows from these controls.
