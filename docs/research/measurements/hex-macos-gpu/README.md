# Apple Silicon GPU adapter — scoped source and light-check record

Status at first checkpoint: **source only, no compilation/test/native reading yet**. No hardware,
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

## Planned narrow checks and current outcomes

New pure controls: high-bit Metal identity/no fake PCI/unchanged maximal PCI packing; observed
residency scale invariance; idle vs zero-window; negative/reset and maximum signed counters;
flooring/full-busy; zero/one/multiple device attribution. Existing ADL/parser tests remain real
matching controls. **All tests are UNRUN at source-first checkpoint.** Only scoped rustfmt and
`git diff --check` ran locally (exit 0); no local build, test or native probe. No active light job.

Next admitted private check will execute `cargo test -p hex-gpu --lib --locked -- --test-threads=1`
(pure fixtures only; no Gpus::open), then native `cargo clippy -p hex-gpu --all-targets --locked -- -D
warnings` as compile-only. Exact source/log hashes, selected pass/fail/ignored counts, PIDs/reaps,
storage and cleanup will be appended from actual results. No hardware integration test execution.

## R01–R16 disposition at source-first checkpoint

| Rule | Scoped evidence / explicit remaining work |
| --- | --- |
| R01 | New math checked; no indexing/unwrap/panic in new production; vectors reserve fallibly; FFI confined with SAFETY. Compile/lint and ROOT native failure proof owed. |
| R02 | Pinned all-smi/macmon and official Apple identity/memory/SDK interfaces; alternative memory sources rejected as above. Actual Apple counter provenance still ROOT owed. |
| R03 | Enums/unit/default/count-derived bounds documented; no timer, interval or arbitrary device ceiling. |
| R04 | Inventory/skipped vs per-counter typed failure; zero window/idle/ambiguous/gone distinct; owned native refs and replace/drop baseline explicit. Native error paths unexecuted. |
| R05 | Existing Gpus seam; no new manager/cache/thread; only necessary cumulative-counter baseline. |
| R06 | Flat count-bounded iteration; O(n log n) inventory sort; single-channel/state sum. ROOT reviews aggregate complexity. |
| R07 | Native ownership/ABI/counter provenance comments retained; no telemetry narration layer. |
| R08 | No comparative performance claim. ROOT owns equivalent measurements and remaining 4–5x target. |
| R09 | New platform implementation from explicit requirement; no reproduced native defect claimed from source. No native red or fix campaign run. |
| R10 | This durable source/decision record; source-only vs planned checks explicit; native effects/mutation evidence ROOT owed. |
| R11 | macOS arm64 implementation; owned NVIDIA/AMD/ADL API migration preserves other platforms. Seven-target and hardware matrix unrun/ROOT owed. Intel Mac excluded. |
| R12 | Necessary Metal/IOKit/IOReport OS adapter isolated; existing workspace libc only new target use. No sensor executable/API dependency. ROOT CLAUDE registration required. |
| R13 | Only GPU inventory/identity/memory/busy; no CPU/power/frequency/thermal metrics or broad framework. |
| R14 | No performance/allocation/leak claim; ROOT profiles native/process/kernel cost. |
| R15 | Explicit scoped implementer; signed source-first then light private checks; ROOT integration/native/gates/merge. No delegated child. |
| R16 | Floor checked; preserved all earlier work/artifacts; no deletions or laptop builds. Private check paths only when admitted. |
