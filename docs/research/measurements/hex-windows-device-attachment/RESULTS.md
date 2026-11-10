# Windows device attachment scoped checkpoint

Tested production source: **edffbd22e0d45e2237875949aa2fb47c23dfca2a**.
Exact base: a03657d18add6343b9b5d9da55c7aefdc71a10c9. Policy: current
Shards33df95b08ad8f95bddeaa1c48acf0c64386e0eac / rules SHA256
df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.
Implementer01a1c643; coordinator01a1aa777. This is implementation and focused
light evidence, not full device/hardware completion.

The real Windows CLI now loads state/devices. Exact selected DeviceIds map to
configured HCS LocationPaths at the existing document/create seam. The same native
marker persists exact bindings/system identity, typed storage ownership and native
Pending/Created phase. Borrowed-volume assignment is supported without layer
activation/deactivation or deletion of caller storage. Holding retains claims
through uncertain create, cancellation, keeper/write or cleanup failure. Keeper
handoff does its native/durable work outside the holding-map mutex. Its native
completion receipt preserves the same operation/System/input owner through Runtime
drop, then closes/deallocates once on terminal completion. No fallback class grant,
GpuMirror, side marker, global manager, thread or new OS import/call is added.

## Executed results

All checks used fresh private source/target/cache/temp, no sync, admitted1CPU,
4GiB memory/12GiB disk, nice10 or Windows BelowNormal, jobs1/test-threads1.
No laptop build/test ran. Rust/cargo1.99.1; d132 macOS26.4 arm64, Jupiter
WindowsNT10.0.26200 x64. Exact invocations, environments, PIDs and raw hashes are
in [verification.json](verification.json) and the archived driver/result records.

| Exact source | Actual selected result | Compile/Clippy and limit |
| --- | --- | --- |
|147312f7|HCS48 passed,0 failed/ignored,282 filtered|core Clippy0; Windows Clippy101 for unused wrapper/collapsible if; preserved|
|67f98335|Windows6 passed,0 failed/ignored,57 filtered|cross/native Runtime Clippy0, real CLI cross compile0|
|ce99413f|reclaim callback regression0 passed/1 failed,0 ignored,63 filtered|one forbidden cleanup callback; source/native-callback simulation, no HCS effect|
|e75121e4|Windows selection9 passed/0 failed/ignored,56 filtered|eight assigned plus one existing Catalog match; Clippy101 test-module placement, preserved|
|57dc6540|HCS50 passed/0 failed/ignored,282 filtered|core Clippy0; Windows compile101 new enum/FFI alias collision; preserved|
|8a29dfd3|Windows16 passed/0 failed/ignored,56 filtered|15 assigned plus one Catalog match; cross Clippy101 two mem_forget denials, preserved|
|4d215cce|new callback source type-checked; native units not run at this revision|cross Clippy101 only deprecated fetch_update, preserved|
|**edffbd22**|**Windows19 passed/0 failed/ignored,56 filtered**|**18 assigned plus one Catalog match; cross/native Runtime all-target Clippy0 and real CLI compile0**|

Core/HCS/API/vendor/Cargo dependency bytes are identical from57dc6540 to final
edffbd22; the50 HCS units/core Clippy are separately attributed to that exact core
source. They were not repeated or represented as another whole-tree suite. The
original PowerShell native-v1 wrapper failed before cargo:0 tests selected. Native
v2 and later used a short encoded bootstrap plus exact stdin archive. Light-v6's
original result label HEAD is preserved; its immutable transferred Git archive
embeds8a29dfd3, as the archived source-pin.json proves. Later submissions resolve
and record full commit IDs first.

Final Jupiter units include actual private marker files, exact Runtime/capacity
holder effects over synthetic HCS calls, overlap refusal, continued independent
empty-Job progress, terminal cleanup and context close/deallocation witnesses.
Four ordinary empty Windows Jobs are created/deleted across those cases. HCS
completion/query/cancel calls are fake controls; there is **no actual HCS, GPU,
container, keeper, WMI, guest or VM campaign**. Callback ordering controls are not
an assertion that all native races or hardware effects were validated.

Final exact log SHA256:

- HCS50: f6434928acb58b0843b6a681007ee4d99432848d6d129545d0057d9ea99051c7.
- Core Clippy: 1c6f93697b230329054de0fc0ab4b4d6e4ae54c89b65d2305d147ce9d2d12519.
- Windows19 stdout: 2e2cdc6f3fa8ff32de4bccd04cf1f7220a9deb1cbc72d8bfc8ad1b9f4537f4e2.
- Native Runtime Clippy stderr: 727028f025e9b4ac7bd60dd0e5fdce5033a8fc634bf1a53b64cd0723d9053860.
- Native CLI stderr: 8abbecfbc96132ab6ea920ef8ee5b47b70fc6b88cb08247c4af84bd3dc52dde9.
- Cross Runtime Clippy: 90e896279ca4129123dbcd2b51af8b6c7cc5455216dceb430d025bd52a64a3e2.
- Cross CLI: 5d6c5b31970a9bcbd51dad9819f0f995961deb3f5dae045c30bd76608b918f10.

## Retention, cleanup and owed ROOT work

[raw-evidence.tar.gz](raw-evidence.tar.gz) retains111 original files,424151 bytes,
SHA256478108935d542a95c12281ef970b0fbd72ce1dda832ad8a778264eb027d236b0.
[raw-index.json](raw-index.json) names every original size/hash. ANSI/CRLF/CLIXML,
trailing whitespace and blank libtest EOF are preserved compressed; no broad
whitespace waiver or normalization. Uncompressed originals were moved, not deleted,
to ~/.cache/hex-lead/transfers/windows-device-attachment-01a1c643-raw-originals-final;
each original private transfer/result/source archive is also preserved. All prior
bounds7d730, HCSd71/e273 and API8c/0429 branches/raw remain untouched.

All recorded Cargo children were waited/reaped. Final d13247117/47580 and Jupiter
324244/321956/324452 are terminal0; local CLI87019/87024 returned0. Final temp
directories contain0 entries. No owned build/test job remains active. The fixture
asserts owned marker/Job cleanup and one context deallocation; it makes no general
kernel-handle, leak-free or performance claim. No unrelated process/data was stopped
or removed. Local free space remained above30GiB.

ROOT still owns composition with its current guest/router/capacity and Linux/macOS
integration, all actual HCS late/synchronous completion and fault cleanup proof,
physical GPU/VFIO/block/net effects, meaningful native mutations, seven targets,
major Linux variants, Windows amd64/arm64, macOS arm64, audits, profiling,
benchmarks, full gates and merges. A Pending marker with no recoverable terminal
operation/system remains uncertain; absence at one instant never frees its claim.
Valid-device support across the required platforms remains required; no OS-based
Unsupported result is presented as completion. Intel Mac remains outside support.

The final evidence commit changes documentation/drivers/raw only; the six
[source hashes](source-hashes.json) remain the actually checked edffbd22 bytes.
Run `python3 docs/research/measurements/hex-windows-device-attachment/verify_evidence.py`
for a read-only byte/source/result readback, not a product build/test.
