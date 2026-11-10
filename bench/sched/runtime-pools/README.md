# Runtime-pool allocation profile (CP-T7.4)

The side-storage candidate was rejected and reverted incc7c2190 after the quiet
comparison below. The capacity fix remains; the effective production tree is
identical to63ec2758. These are coordinator measurements of the single FitIndex
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
