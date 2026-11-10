# Apple Silicon GPU adapter — scoped source and light-check record

Latest result: signed source `0e625a452827f4e09ca77eec7e16fb491582b45b` has **18 pure unit passes,
0 failures/ignored/filtered and native all-target Clippy exit 0** on admitted d132. The first
checkpoint was source only; actual results below supersede its explicitly unrun check status. No hardware,
Metal/IOReport effect, attachment, performance or complete-platform claim. `AC-T7.3.1` supplies the
existing inventory/sample seam. ROOT owns native effects, attachment/catalog integration, mutations,
combined gates, audits, profiling/benchmarks and merges. This lane implements and checks only the
assigned `crates/hex-gpu/**` and this evidence directory.

Base: `07c68e378b741e3ce6008e1cd612b5e9d9bc3ed2`, branch `cp/t7-macos-gpu`.
Preserved Linux helper: signed `c646df710c267ebaa57ca262e7db777310296eb0` on
`cp/t7-native-device-discovery`; ROOT/relay received its exact contract before this branch switch.
Earlier ea30/ee/d2/7875 branches and artifacts remain intact.

## Policy and ownership

POLICY_READ: Shards current `origin/main` `33df95b08ad8f95bddeaa1c48acf0c64386e0eac`, rules
SHA256 `df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
Implementer `01a1c640e6ad7127bcf34212f474d37c`; coordinator
`01a1aa77778471339bea3beff90d6bbd`. Full Hex CLAUDE/current HANDOFF, Shards AGENTS/R01–R16/
development-evidence/CLAUDE/managed-runtime-plan and current g121 handoff read before edits and again
after compaction. Readbacks: `macos-gpu-policy-read-33df-07c-v1` and
`macos-gpu-policy-resume-33df-07c-v2`. Explicit task authorizes only `apple/sys.rs` unsafe OS FFI;
ROOT owns its CLAUDE §12 registration and all outside-crate consumers. No children/goals/plans.

Only admitted d1323898b255a3ed435eb83fa0f7b0cf light checks: 1 CPU, 4 GiB RAM, 12 GiB disk,
nice10, jobs1/threads1, NEW private source/target/cache/TMPDIR, precise no-sync source transfer.
No laptop builds/tests, native GPU execution, VM, installed binary, security/runner change, protected
snapshot/target reuse or cotenant process action. Local source/rustfmt/Git are permitted. Initial
local floor read: 218720380 KiB available, above 30 GiB. No data deleted by this lane.

## D-MACGPU-1: native identity and inventory

`Gpu.identity` is `Identity::Pci(PciAddress) | Identity::Metal(u64)`. `Gpu` and `Identity` expose
`key() -> u64` and `pci() -> Option<PciAddress>`. PCI uses existing domain32/bus8/device5/function3
packing; Metal retains all 64 native bits. The kind remains explicit even for equal numeric keys.
No invented PCI, hash/ranking/tuning identity, parallel catalog or API dependency. Vendor gains
`Apple`; NoGpus gains `apple`; skipped Metal array entries retain their exact index and cause.

[`MTLDevice.registryID`](https://developer.apple.com/documentation/metal/mtldevice/registryid)
identifies the same GPU across tasks. The installed Apple SDK's MTLDevice.h states that it is the
IORegistry entry ID. Lookup uses `IORegistryEntryIDMatching` and reads the ID back before retaining
the exact service. Apple's [IORegistry entry-ID contract](https://developer.apple.com/documentation/iokit/1514719-ioregistryentrygetregistryentryi)
limits its lifetime to the current host boot: this adapter **does not promise stability across host
reboots**, nor attachment merely from a telemetry ID. ROOT owns durable backend compatibility.

`MTLCopyAllDevices` retains the native array during queries; only devices supporting SDK
`MTLGPUFamilyApple1` (1001) are classified Apple. Model is native `name`; driver release is actual
`uname.release`, naming the OS-bundled driver without an invented OS-to-Metal-version table.
No sensors or commands are launched. Inventory and skipped storage reserve fallibly from the actual
array count, with no invented device ceiling. Sort by native identity and adjacent duplicate checks
cost O(n log n)/O(n); output/native-handle storage is O(n). Individual unreadable devices are kept in
Skipped; an invalid array/duplicate ID/allocation fails the typed inventory. CF/IOKit references and
the thread-local autorelease pool have owned Drop paths; no pool crosses a thread boundary.

## D-MACGPU-2: memory provenance

Apple [`recommendedMaxWorkingSetSize`](https://developer.apple.com/documentation/metal/mtldevice/recommendedmaxworkingsetsize)
is a native working-set budget for GPU resources. It is **not physical VRAM**, a hard allocation
ceiling or total host RAM. Apple's [Metal compute explanation](https://developer.apple.com/videos/play/tech-talks/10580/)
also distinguishes the working-set bound from total resources. This is the meaning of Apple's
`memory_total_bytes`; neither sysinfo/hw.memsize nor all-smi's whole-host memory totals are copied.

`memory_used_bytes` reads an instantaneous CF snapshot of the **exact Metal registry entry's**
`PerformanceStatistics` / `Alloc system memory`. No first-shaped-accelerator substitution, summing
another device, process-local `MTLDevice.currentAllocatedSize`, fallback to host memory, guessed
zero or clamp to working-set budget. Negative/wrong-type/unrepresentable numbers refuse Malformed;
missing property refuses Unsupported; detached exact service returns Gone. The driver-reported
mapped allocation can exceed its performance budget. This counter's availability, exact-entry join
and semantics still need ROOT's actual Apple proof; no native observation was performed here.

Alternative implementation evidence inspected:

- all-smi's pinned Apple reader uses sysinfo whole-host total/used memory: rejected for this API.
- [Stats GPU reader at 4af16e9878108e77d4035e8e3a8ef668a6265024](https://github.com/exelban/stats/blob/4af16e9878108e77d4035e8e3a8ef668a6265024/Modules/GPU/reader.swift)
  reads per-accelerator PerformanceStatistics / In use system memory; this is an active subset,
  different from mapped allocation. SHA256 `8fdabe062e6d6124fd8b6cabeda95618c2c962c62e5caf46fe98541d22f620c4`.
- [Gpuer at a3cd655f5ccb274d3561e4cbfcc771b0bb7e256a](https://github.com/simonw/gpuer/blob/a3cd655f5ccb274d3561e4cbfcc771b0bb7e256a/GpuerApp.swift)
  distinguishes Alloc and In use system memory. SHA256
  `4df6b9ef66e6bdd187b2111e6089211e7771a05b3d6b09b8347f64b66bc28838`.
  Its author explicitly disclaims deep review; it is implementation evidence, **not an Apple ABI
  guarantee or native validation**. This adapter uses direct typed IOKit snapshots, not that
  project's command/text parser, memory estimates, pressure proxy, capping or zero defaults.

## D-MACGPU-3: load, bounds, lifetime and no sampling policy

Native declarations and formula adapted from lablup/all-smi **fe479ae675e39475a2eab1cfbb3f5432f705e12b**:

- [macos_native/ioreport.rs](https://github.com/lablup/all-smi/blob/fe479ae675e39475a2eab1cfbb3f5432f705e12b/src/device/macos_native/ioreport.rs),
  downloaded bytes SHA256 `5debab2d472f79cfb41f377eeb695e8f58e451d42d14dcb66198169f3f989ff2`.
- [ioreport/channel_filter.rs](https://github.com/lablup/all-smi/blob/fe479ae675e39475a2eab1cfbb3f5432f705e12b/src/device/macos_native/ioreport/channel_filter.rs),
  SHA256 `4f2b6d7981edb0db4ee791e63378fdd511198a6014251f36f92e6d6044eb2609`.
- [macmon sources.rs at 561de4d3f09c1b84f7a632df23c2612984e73d40](https://github.com/vladkens/macmon/blob/561de4d3f09c1b84f7a632df23c2612984e73d40/src/sources.rs)
  supplies the companion ownership check: its Drop releases subscription, channel set and previous
  sample. The all-smi Drop omits subscription release; Hex owns/releases it rather than copying
  that leak. The installed SDK libIOReport.tbd confirms the bound native entry points.

Only GPU Stats / GPU Performance States / exact GPUPH is subscribed. Exactly one selected channel
is retained; missing channel is Unsupported, duplicate is Ambiguous. GPU package residency is
attributed only for a sole **enumerated** Metal device, including unreadable entries in that count.
Multiple identities retain inventory/exact-entry memory but never receive copied/guessed package
load. This is a typed attribution limit, not a platform-wide unsupported result.

At open, retain one baseline raw sample. Each caller's sample creates one new raw sample and one
delta; active residency is every state except names containing IDLE/OFF/DOWN, matching the pinned
algorithm. Busy = floor(100 * sum(active ticks) / sum(all ticks)); native i32 state count and i64
residency fit u128, with checked sum/product/division anyway. Negative deltas/wrong shapes refuse
Malformed; zero total gives NoSample, while nonzero all-idle gives real 0%. Failure never returns a
previous busy value. No sleep, timestamp-based rate, arbitrary minimum period, global singleton,
throttle, process cache, worker or polling manager. The raw baseline is mathematical delta history,
replaced per sample and released on Drop, not a cache of inferred availability.

One inventory/native handle per enumerated device, one subscribed channel/previous raw sample,
bounded native channel/state loops, and one native memory snapshot per card per call. CFString UTF-8
conversion reserves from the native encoded length; all new Rust vectors reserve fallibly. Native
CF allocation internals are outside Rust's fallible allocator API; no zero-allocation/leak-free/cost
claim follows from this source. Linux/Windows retain their existing allocation assertions; the
hardware fixture explicitly reports Apple's native-snapshot allocation distinction. It is UNRUN.
No new tuning literal: 100 is percent, 1001/UTF8/SInt64 are cited SDK enums, 0/null are native default
allocator/query arguments, one GPUPH is the exact attribution contract rather than a count ceiling.

## Source-first controls and actual narrow checks

New pure controls: high-bit Metal identity/no fake PCI/unchanged maximal PCI packing; observed
residency scale invariance; idle vs zero-window; negative/reset and maximum signed counters;
flooring/full-busy; zero/one/multiple device attribution. Existing ADL/parser tests remain real
matching controls. All were unrun at the source-first checkpoint. Only scoped rustfmt and
`git diff --check` ran locally (exit 0); no local build, test or native probe at any point.

Actual chronology, preserving the failed setup rather than calling it a test red:

1. Exact signed source archive SHA256
   `fec3668fa3dfe435267d7df3069bafee0d034128e33ed52bfdce9ef01b1e9155`, 7626349 bytes, 1707 regular
   files streamed in memory with `--no-sync` to NEW private d132
   `~/.cache/hex-lead/lanes/macos-gpu-01a1c640-0e625a45-light-v1`. Local CLI PID77812 reaped, exit0.
   Run **r39 exit1 before Cargo**: version-query subprocess ran from runner home, outside the
   checkout's toolchain pin, and the private rustup copy has no default. **0 selected tests, no
   compilation/native GPU call.** `run_light-r39.py`, `light-r39-submission.json`, exact
   `r39-preflight-failure.txt` and compressed original tool output preserve it. No default/toolchain
   install, runner setting or source change was made to work around it.
2. Corrected only the harness: query tool versions in source cwd and explicitly retain the
   checkout's exact `RUSTUP_TOOLCHAIN=1.99.1`. Same byte-identical signed source archive streamed to
   separate NEW `macos-gpu-01a1c640-0e625a45-light-v2` source/target/cargo-home/tmp/proof paths.
   Local CLI PID84458 reaped, exit0. Original v1 source/partial proof remains intact.
3. **r40 terminal0**, d132 macOS26.4/arm64, Rust1.99.1 (`58cf8f9c35b355199727627d07ae34a4e2846d67`),
   Cargo1.99.1 (`5f94df478`). Admitted cpus1/mem4g/disk12g; `nice -n 10`, inherited measured nice20,
   jobs1/test-threads1; no priority raised. Commands in `run_light.py` and `light-r40-submission.json`:
   - `cargo test -p hex-gpu --lib --locked -- --test-threads=1`: **18 pass, 0 fail, 0 ignored,
     0 measured, 0 filtered**, Cargo PID42142 reaped. 4 new pure controls + 14 existing pure controls;
     no `Gpus::open` or hardware fixture. Raw log 2646 bytes, SHA256
     `e349e73ccb21816c8658c1847e8d15fe756a6255ee37c94b51a42d887966258b`.
   - `cargo clippy -p hex-gpu --all-targets --locked -- -D warnings`: **exit0**, Cargo PID42378
     reaped. Native type checking includes the hardware leaf; it is never executed. Raw log 513
     bytes, SHA256 `43acfb665e78d93d6e342d0de845b89a74194c8c36d40d1fc42a8e4c99c0b081`.

All 1707 source files before/after match one another AND the exact local signed Git archive (including
byte-identical Cargo.lock). Private TMPDIR contains **0 entries**. Pure test executable
`hex_gpu-30be2fb99e73668f`, 1486776 bytes, SHA256
`42dc4d1e9b8e672291a0baab6be4d327b6284800f429336d632ef1cce066de85`. Linking/running pure tests
proves the native adapter compiles/links; it does **not** prove any foreign GPU method was called.
No active source/check job, child process or guest remains from this lane. All private source,
targets, caches, transfer receipts and frozen proof are retained; no other lane path was touched.

Raw collection: [light-r39-r40-proof.tar.gz](light-r39-r40-proof.tar.gz), **382995 bytes / 12 files**,
SHA256 `5348f6dd62ed54abde20c741713ccfcac11b322781e1805cdd76ec7b3b207dd6`.
The bundle preserves each original byte with member size/SHA in bundle-members.json, both transfer
receipts, first partial proof, successful environment/commands/owner identities/results/raw logs and
full source manifests. Collection used bounded <=12000 raw-byte reads, admitted no-sync, and checked
each chunk/archive/member hash; no encoded data was printed. Verified raw extraction retained under
`~/.cache/hex-lead/transfers/macos-gpu-01a1c640-0e625a45-light-v2/`. Local collection receipt:
[light-r39-r40-collection.json](light-r39-r40-collection.json). Original tool-output bytes are stored
losslessly compressed, with raw/stored identities in [tool-output-storage.json](tool-output-storage.json).
No raw-log whitespace normalization or broad format exemption.

Explicitly owed ROOT: actual Metal inventory identity/model/native budget and exact IOKit service
join/mapped-memory semantics; IOReport missing/zero/idle/busy/ambiguous/read failure behavior and
native reference cleanup; actual attachment/access/recovery/catalog effects; guard removals and
restores; all seven-target and combined integration gates; launch/CF allocation/counter overhead,
kernel/parent cost, profiles/benchmarks/comparisons. A service identity lookup failure is currently
Skipped; a missing memory **counter** preserves inventory and returns the typed counter error. ROOT
received this exact distinction. No platform-wide refusal is claimed as completion on a valid Mac.

## R01–R16 disposition after the scoped light check

| Rule | Scoped evidence / explicit remaining work |
| --- | --- |
| R01 | New math checked; no indexing/unwrap/panic in new production; vectors reserve fallibly; FFI confined with SAFETY. 18 pure passes and native all-target Clippy0; ROOT native failure/ABI proof owed. |
| R02 | Pinned all-smi/macmon and official Apple identity/memory/SDK interfaces; alternative memory sources rejected as above. Actual Apple counter provenance still ROOT owed. |
| R03 | Enums/unit/default/count-derived bounds documented; no timer, interval or arbitrary device ceiling. |
| R04 | Inventory/skipped vs per-counter typed failure; zero window/idle/ambiguous/gone distinct; owned native refs and replace/drop baseline explicit. Native error paths unexecuted. |
| R05 | Existing Gpus seam; no new manager/cache/thread; only necessary cumulative-counter baseline. |
| R06 | Flat count-bounded iteration; O(n log n) inventory sort; single-channel/state sum. ROOT reviews aggregate complexity. |
| R07 | Native ownership/ABI/counter provenance comments retained; no telemetry narration layer. |
| R08 | No comparative performance claim. ROOT owns equivalent measurements and remaining 4–5x target. |
| R09 | New platform implementation from explicit requirement; no reproduced native defect claimed from source. No native red or fix campaign run. |
| R10 | Signed source-first record and exact r39 setup failure/r40 raw success, manifests and cleanup retained. Native effects/mutation evidence ROOT owed. |
| R11 | macOS arm64 pure tests/native compile passed; owned NVIDIA/AMD/ADL API migration is source-preserving. Seven-target and hardware matrix unrun/ROOT owed. Intel Mac excluded. |
| R12 | Necessary Metal/IOKit/IOReport OS adapter isolated; existing workspace libc only new target use. No sensor executable/API dependency. ROOT CLAUDE registration required. |
| R13 | Only GPU inventory/identity/memory/busy; no CPU/power/frequency/thermal metrics or broad framework. |
| R14 | No performance/allocation/leak claim; ROOT profiles native/process/kernel cost. |
| R15 | Explicit scoped implementer; signed0e source-first then admitted r40 light check after preserved r39 setup failure. No active job or delegated child. ROOT integration/native/gates/merge owed. |
| R16 | Floor checked; preserved all earlier work/artifacts; no deletions or laptop builds. Private check paths only when admitted. |
