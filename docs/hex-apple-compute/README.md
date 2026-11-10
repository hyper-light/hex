# Ignored native Metal compute witness

Status: **source-first; UNCOMPILED, UNRUN**. This lane will only compile the new test, never invoke
it. No native Metal operation, hardware test, campaign, benchmark or production change is authorized
here. ROOT owns actual effects, native cleanup, integration, audits, profiling and main.

Base `c2da38cc363e7d53f6df8fb82ab7bbda5e2f1efe`; branch `cp/t7-macos-gpu-witness`. Owned paths
ONLY new `crates/hex-gpu/tests/apple_compute.rs` and `docs/hex-apple-compute/`. Previous signed
source0e625a45/evidence8cc6e795 and all earlier branches/raw artifacts are preserved.

## Policy and acknowledgment

Implementer `01a1c640e6ad7127bcf34212f474d37c`, coordinator
`01a1aa77778471339bea3beff90d6bbd`. Full fixed Hex CLAUDE/current HANDOFF and current vendored
Shards AGENTS/R01–R16/development-evidence/CLAUDE reread before edits, plus g121 handoff. Vendor
MANIFEST pins Shards `33df95b08ad8f95bddeaa1c48acf0c64386e0eac`; all four vendored policy files
are byte-equal current Shards origin/main. Rules SHA256
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
POLICY_READ stable key `apple-compute-policy-read-33df-c2da-v1` delivered before branch/edits/checks;
ROOT's explicit new-scope ACK received before branch creation. Broader Shards R15 permission does
not enlarge this source/compile-only assignment. No children, goals/plans or autonomous campaigns.

No laptop builds/tests. Only reading/edit/Git/scoped rustfmt/data are local. Compile-only is admitted
on d1323898b255a3ed435eb83fa0f7b0cf, 1CPU/4GiB/12GiB, nice10/jobs1/threads1, new private source,
target, CARGO_HOME and TMPDIR, precise no-sync stream. Local floor before transfer/source signing:
218253216KiB (>30GiB). No deletion, protected target/snapshot/guest, service, install, host security
or foreign-process action. Original8cc evidence remains immutable on its branch.

## Exact ROOT input and invocation contract

One macOS arm64 integration test, explicitly ignored:
`root_selected_metal_device_executes_checked_kernel`.

All three environment inputs are **required**; there are no fallback/defaults:

| Input | Meaning / refusal |
| --- | --- |
| `HEX_METAL_REGISTRY_ID` | ROOT's actual measured native u64 key, decimal or lower-case `0x` prefix; malformed/missing or missing/duplicate inventory/native identity fails. No default-device API. |
| `HEX_METAL_ELEMENTS` | Positive u32 element count, converting to checked host byte length and fitting the selected device's actual maxBufferLength. |
| `HEX_METAL_ROUNDS` | Positive u32 rounds per element. Fixed finite shader loop; checked u64 element×round work count is reported. |

ROOT must derive these inputs from its own admitted campaign budget and actual inventory. They
are workload inputs, not production constants or tuning defaults. CPU oracle work is also
elements×rounds; two shared native buffers and the oracle each hold elements×4 bytes, plus native
compiler/pipeline/command overhead whose actual cost is unknown and ROOT-owned. Native group width
is min(elements, pipeline.maxTotalThreadsPerThreadgroup). Padding is checked against the shader's
uint index space and guarded before buffer access; group count/width/launched threads are reported.

ROOT-only command, **inside ROOT's existing externally guarded owned process/runner**:

```sh
# ROOT supplies all three required HEX_METAL_* environment values before invoking this.
cargo test -p hex-gpu --test apple_compute --locked -- \
  --ignored --exact root_selected_metal_device_executes_checked_kernel \
  --nocapture --test-threads=1
```

**The complete invocation REQUIRES ROOT's external deadline guard.** Metal waitUntilCompleted is
an unbounded native wait; CPU oracle setup and native shader/pipeline compilation can also take
time. There is no internal polling, guessed wait limit, worker, cancellation guarantee or timeout
framework here. ROOT records its actual process/PID/guard/deadline and cleanup outcome. Killing a
hung process is not command completion or proof of native GPU resource cleanup. This document does
not authorize this lane to execute the command or choose ROOT's deadline/work budget.

## Decisive behavior and resource ownership

1. Read required inputs before native calls. `Gpus::inventory` must contain exactly one Apple
   `Identity::Metal(required_id)`. Independently call MTLCopyAllDevices and enumerate every actual
   registryID, retaining exactly the uniquely matching device. No first/default/shape-based pick,
   absent-device skip, unsupported-as-pass, PCI placeholder or substitution.
2. Create all queue, pipeline and shared buffer objects from that exact device; read each object's
   device.registryID back. Compile one embedded MSL kernel through the native API (no installed
   compiler/sensor/new crate dependency). Shader/pipeline NSError or null results fail explicitly.
3. Generate deterministic 32-bit input; the CPU oracle independently applies rotate-left-one XOR
   (index+round modulo2^32), then flips all bits. Before dispatch write **expected XOR1** to every
   output word: every initial result is wrong. No CPU path writes the expected result to the GPU
   output, so an omitted/nonexecuted dispatch cannot pass by unchanged buffers.
4. Encode exactly one command with checked work bounds, endEncoding, commit and waitUntilCompleted.
   Require status **MTLCommandBufferStatusCompleted (4)** and no NSError, then compare **every**
   output word to the CPU oracle after GPU completion/shared-memory coherence. Missing completion,
   error (including status5), mismatched output or wrong device is a test failure, never a skip.
5. RAII guards release owned CF values, create/new Objective-C references, retained borrowed
   command/encoder/device references and this thread's autorelease pool (last). The outer test
   catches unwind, checks acquired/released wrapper-call counts and resumes the original failure.
   This validates fixture release calls, **not OS/driver leak freedom**. Gpus' own native references
   are released through its existing Drop and are outside the test wrapper-call counter. ROOT
   measures actual native cleanup and any timeout/abort uncertainty.

The test is entirely cfg-gated to supported macOS arm64. No production file/API/default or test
parent registration is changed: Cargo discovers this new integration-test leaf. Default test runs
leave it ignored; an ignored result is not native validation. No attachment, isolation, load-percent,
IOReport-counter correctness, host-reboot identity, performance or cross-platform GPU claim follows
from this compute witness alone.

## Primary sources and ABI record

Sources read 2026-11-10; new code is test-only and each unsafe operation has its own SAFETY reason.
The task explicitly authorizes minimal native test FFI in this leaf. Production sys.rs is untouched.

- Apple [registryID](https://developer.apple.com/documentation/metal/mtldevice/registryid): native
  global-across-tasks u64. Host-boot scope remains the earlier documented IOKit limit.
- Apple [performing calculations on a GPU](https://developer.apple.com/documentation/metal/performing-calculations-on-a-gpu):
  device-created resources, shared storage, command encoding/commit/completion and output checking.
  This witness deliberately selects the required enumerated ID rather than Apple's example default.
- Apple [command status](https://developer.apple.com/documentation/metal/mtlcommandbuffer/status)
  and [waitUntilCompleted](https://developer.apple.com/documentation/metal/mtlcommandbuffer/waituntilcompleted()):
  terminal success/error distinction and blocking completion semantics, with no timeout promise.
- Apple [setBytes](https://developer.apple.com/documentation/metal/mtlcomputecommandencoder/setbytes(_:length:index:)):
  copies the repr(C) two-u32 arguments (8 bytes, below the API's documented4KiB bound).
- Apple [Metal Shading Language specification](https://developer.apple.com/metal/Metal-Shading-Language-Specification.pdf):
  uint32 arithmetic, device/constant address spaces and thread_position_in_grid.
- Installed official SDK headers under
  `/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/System/Library/Frameworks/Metal.framework/Headers/`:
  `MTLDevice.h` SHA256 `d1ef1bb5bc85faeeec9bc85f49124a453a8cd33b1182b0a71a41ba0dc4ebbb89`,
  `MTLComputeCommandEncoder.h` `610bcf8f3e6cb6a7067622f4395d8aa292c56226afde457ac6cb902937872b7b`,
  `MTLCommandBuffer.h` `a5a1ead36de20b4c1f4dce8193b945fb981e67456ca4a3a1395d167c9df475e7`.
  These pin method signatures, MTLSize layout, shared-storage enum0, status4/5 and native limits.

No production literal introduced. Test fixture bit patterns target high bits and prove initial
wrongness; 1/31/32 are one-bit rotation of a u32, dimensions1 describe the requested one-dimensional
grid, 0/1/2 are explicit shader argument bindings, and 4 is the SDK terminal-completion enum.
All workload size/rounds come from required ROOT inputs and native limits, without an invented ceiling.

## Source-first checks and owed work

Local scoped rustfmt and git diff --check: exit0. No build/test/native call yet. Planned admitted
compile-only commands: `cargo test -p hex-gpu --test apple_compute --no-run --locked` and
`cargo clippy -p hex-gpu --test apple_compute --locked -- -D warnings`. No test execution, no
--ignored, no Gpus::open. Exact source/log hashes, compiler exits, owned PIDs/reaps and temporary
storage witness will be appended from actual results. No active job at this source checkpoint.

| R01–R16 | Applicable evidence / remaining obligation |
| --- | --- |
| R01 | New test may assert/panic per policy; no production edit. Unsafe test FFI is isolated, typed signatures/ownership and bounds explained; native validity ROOT owed. |
| R02/R03 | Official Apple APIs/SDK/spec and alternatives above; required caller work, native limits, no tuning default. Actual native data/cost remain ROOT owed. |
| R04 | Required inputs→identity match→owned setup→encode→commit→terminal check→oracle→release, with original failure resumed after release-call count check. Timeout/abort uncertain. |
| R05/R06/R07 | No cache/picker/worker; linear native enumeration and finite caller work; one dispatch and direct event wait. Safety/lifetime comments explain native obligations. |
| R08/R14 | No performance/allocation/leak claim. ROOT owns comparative benchmarks, profiles, process/kernel cost. |
| R09/R10 | New witness source, no defect reproduction or actual Metal pass claimed. Exact source-first/compile evidence separated from ROOT's native effects and meaningful mutations. |
| R11/R12 | macOS arm64-only Metal witness, no installed dependency/binary or alternate production path. Other platforms' GPU proof remains ROOT-owned. |
| R13 | Only inventory identity and deterministic compute/result/error/cleanup-call witness, no broader metrics or framework. |
| R15 | Scoped implementer after explicit ACK; only source and compile-only. ROOT actual effects/audits/integration/gates/main; no children or native campaign here. |
| R16 | Floor checked, earlier refs/evidence retained, new private compile paths only; no deletion or laptop build. |
