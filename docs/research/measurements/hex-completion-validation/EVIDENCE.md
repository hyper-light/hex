# R01–R16 record and remaining coordinator work

Identity and ownership are in README.md. Policy revision and rules hash are
unchanged from the acknowledged initial readback. These tools are fixtures, not
production authority, and every native test is explicitly ignored by default.

| Rule | Implemented source/evidence | Remaining ROOT evidence |
|---|---|---|
| R01 | Rust fixtures may assert; Python/C fixture errors return nonzero. Work sizes, output buffers, native identity and arithmetic domains are checked. | Actual panic/error/cancellation and allocation-failure effects in integrated production. |
| R02 | Primary Vulkan identity/device/fence API, MoltenVK native Metal API, Apple libproc headers, Python process semantics, Linux cgroup counters and NIST statistics; alternatives in README. | Exact linked driver/VMM versions and live compatibility/security/cost comparison. |
| R03 | All run/resource/work/byte/count/wait budgets are explicit ROOT inputs. Native protocol bounds come from Vulkan limits, OS identity layouts, runtime wire ceiling and PCI field widths. | Hardware-specific workload/resource envelopes and measured costs; no guessed pass allowance. |
| R04 | Ordered native stages and separate full/selected-only proof scopes; creating/ready/deleting are read from existing records; cleanup uncertainty is retained. | Actual crash-at-write/native terminal ownership transitions, no repick/no double holder. |
| R05 | Existing picker/capacity/record authority observed, no new inventory or holder ledger. Readiness/dispatch events drive waits. Exact process waits sit inside ROOT's outer deadline; timeout cannot prove descendant cleanup. No discovery cache or polling loop. | Review integrated production waits/caches and actual before/after cost and mutation for any changed guard. |
| R06 | Focused fixture operations and typed refusal decoding at the existing seam; bounded native inventory. | Coordinator complexity/integration audit. |
| R07 | Comments identify protocol/native lifetime/uncertainty and mutation obligations. | Final coordinator source review. |
| R08 | Equivalent pinned-baseline comparison and explicit 4× target/miss results. | Actual quiet-hardware comparative samples; no performance result currently. |
| R09 | New acceptance machinery is a feature. Existing failures are preserved at their old refs; no source observation is described as a live reproduction. | Any new suspected production bug requires the exact live red before a fix. |
| R10 | Source-first signed checkpoints; per-run source/binary/log/config hashes, guard-red/restored-green records and retained raw scopes. | Native original/fixed/meaningful mutation receipts. |
| R11 | Seven-target set, 39 native/routed backend/topology obligations, explicit Linux-only Firecracker host constraint; no Intel Mac. Missing/paused/offline hardware remains unverified. | Actual supported native host/VM/libc/distribution/kernel matrix; Windows ARM stays paused. |
| R12 | Same socket fixture and GPU workload across backends; smallest OS read-only exact-PID observer. Windows draft uses official SDK interfaces, Vulkan Metal join uses official MoltenVK borrowed-device API. | Actual KVM/HVF/WHP implementation compatibility; no platform Unsupported accepted as completion. |
| R13 | No alternative runtime, picker, installed broker or reservation map. | Final complete integrated production behavior and evidence. |
| R14 | Collector/comparator preserve allocation/reallocation/copy, caller/runtime/guest/kernel coverage, RSS/faults, CPU, throughput/latency and uncertainty; unknowns remain null. | ROOT performs all actual profiles and benchmarks and reviews native/parent/kernel effects. |
| R15 | ROOT acknowledged implementer scope, owns all actual native/E2E/fault/mutation/profile/gates/main merges. No extra agents/goals/plans created. Light tests restricted to d132 private nice10/1CPU/4GiB/12GiB/jobs1/threads1. | Integrated review, correctness/adversarial/native/full gates and main merge/push by ROOT. |
| R16 | Original dirty drafts/old refs/evidence preserved. Unique private source/target/cache/temp. Laptop read/edit/Git/rustfmt/hash only; >30GiB floor checked as metadata. | Own-job cleanup receipts and preserved unknown native scopes; never foreign PID/data/cache cleanup. |

The native Windows compute source was copied byte-identically from the dirty
draft recorded in windows-draft.json. ROOT integrated the requested dev features
at 9d9dccfbd82c8e7448d67166525114d2a41a8db5. The port itself is unrun until its
focused compile result is recorded; no old draft/native result is attributed to
the new integrated revision. Its first Windows cross-Clippy exposed the old
draft's implicit ManuallyDrop union dereference. The red is preserved and the
owned copy now uses the explicit dereference required by Rust; the original
dirty draft is unchanged. Rechecking that fix awaits ROOT's exact combined
cohort along with the integrated socket fixture compile.

Pure/light result: source 85ee0ce83fad8d7becdcfc2d97b7625a94ee68c9, admitted d132,
one CPU/4 GiB/12 GiB, nice10/jobs1/threads1, private no-sync source/target/cache/temp.
Selected 27/27 pure cases passed, all 14 Python files passed syntax checks, and
the current C helper passed syntax against official Vulkan-Headers SDK
1.4.321.0 archive SHA256
17f8ff30fd79fb7531efcb7c78c02c17a595208d482a150f06836b0ca97ef8f2.
Unit log SHA256 1f2fc35f42da1d893d4cefc8d6560625e509d6dde11e217bda731158a0ed526b;
source archive SHA256 593bcc782777763575ad916b4bee65598839f055304980a056a2c9add4dcf868.
Source unchanged, child processes reaped, temp entries zero. No GPU, VM, HCS,
fault/mutation campaign or profile was executed. These checks do not verify
the native ABI at runtime or the final platform matrix.

The combined fixture requires source-pinned actual native observer, host
inventory, cleanup observer and telemetry helpers. They must read the actual
adapter's durable PID/session receipt, not fabricate a process identity from a
model or expected string. Backend Config/CLI/record schemas remain owned by L1,
L2, L3 and ROOT; record their final integration in the run manifest.

The Shards API process-start contract uses Linux /proc stat starttime, macOS
proc_pid_rusage ri_proc_start_abstime, and Windows process creation FILETIME.
The macOS observer reads that same ABI from official
[XNU resource.h](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/resource.h)
and [libproc.h](https://github.com/apple-oss-distributions/xnu/blob/main/libsyscall/wrappers/libproc/libproc.h).
The independent Vulkan-to-Metal join calls
[vkGetMTLDeviceMVK](https://github.com/KhronosGroup/MoltenVK/blob/main/MoltenVK/MoltenVK/API/mvk_deprecated_api.h)
and reads [MTLDevice.registryID](https://developer.apple.com/documentation/metal/mtldevice/registryid).
Deprecated or missing driver APIs fail as unverified; they never select by model,
ordinal, pipeline cache UUID, environment filter or default GPU.
