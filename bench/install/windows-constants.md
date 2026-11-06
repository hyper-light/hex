# The Windows line's constants (Ada's rule, 2026-11-06: learned or data-derived values only)

Every constant the Windows line adds or relies on: the `process` class on job objects (native step 3), the
installer's Windows part (step 6), the routing of `oci` boxes by image (steps 4 and 5) and the jupiter tooling.
Step 4's HCS class (`crates/hex-runtime/src/windows/oci/`, `crates/hex-core/src/hcs.rs`) and step 5's guest
(`crates/hex-runtime/src/guest/`, `crates/hex-client/src/guest*`) are audited by their own agents. Kinds:
(a) learned at runtime, (b) derived from measured data with its record, (c) a platform or spec limit cited at its
source, (d) picked or copied.

| Name | Value | file:line | Kind | Source, or why it stays (d) |
|---|---|---|---|---|
| `OVERSHOOT_MARGIN` | 2 MiB | crates/hex-core/src/job.rs:21 | (b), rounded (d) | Measured: a job's peak overshoots its commit limit by at most 1,163,264 bytes (~1.11 MiB) over a 64 KiB-256 MiB step sweep, bounded (`bench/native/windows/RESULT.md`, record `job-events-68e99688-*`). The rounding from 1.11 up to 2 MiB is picked headroom. Convertible to the measured max itself, but a margin equal to the max seen is a flake; the rule for how far past a measured max a bound sits is a project-wide call (below). |
| `NOTIFY_MARGIN` | 4 MiB | crates/hex-core/src/job.rs:28 | (b) | Measured: a notification at the hard limit never fires, and one 4 MiB under fired in every pass (same record, experiment e17). Only 4 MiB was measured, not the smallest margin that fires. |
| `TERMINATED` | 0xE048_0000 | crates/hex-core/src/job.rs:33 | (c) | MS-ERREF §2.3: severity error, customer bit 29, facility 0x048, so no status of Windows' own collides. |
| `DEFAULT_WEIGHT` | 5 | crates/hex-core/src/job.rs:37 | (c) | `jobobject_cpu_rate_control_information.md:147`: "The default is 5". |
| `RATE_SCALE` | 10,000 | crates/hex-core/src/job.rs:40 | (c) | `jobobject_cpu_rate_control_information.md:132`: a hard cap is cycles per 10,000. |
| `weight()` ranges | 1..=10,000 → 1..=9, 100 ↦ 5 | crates/hex-core/src/job.rs:176, :192 | (c) | cgroup v2's `cpu.weight` range and default (1-10,000, 100) onto Windows' `Weight` range and default (1-9, 5). The piecewise-linear shape between the pinned ends is a design choice, with the share each weight gives measured in `bench/native/windows/RESULT.md`. |
| `LIVE_CAP` | 1,024 → removed | crates/hex-runtime/src/windows/sys.rs:329 | was (d), now (a) | Was a picked id-buffer size (a box past it read as 1,024). Now the list is sized from the job's own `NumberOfAssignedProcesses`, read first with room for one id, and the live count is `NumberOfProcessIdsInList`. Built; the native run on jupiter is pending (jupiter didn't answer at 14:40Z). |
| temp-path buffer | 261 u16 | crates/hex-runtime/src/windows/sys.rs:97 | (c) | `GetTempPath2W`'s documentation: "The maximum possible return value is MAX_PATH+1 (261)". Step 5's code. |
| features `PATIENCE` | 30 s | crates/hex-runtime/src/windows/features.rs:24 | (d), with data | Measured now on jupiter (2026-11-06T14:27Z, 30 runs of the exact query, PowerShell start plus one WMI read): min 1,048, p50 1,183, p90 1,398, max 2,421 ms. 30 s is about 12x the max seen. Step 6's constant; the same project-wide margin question as above. |
| features `ENVIRONMENT` | 5 names | crates/hex-runtime/src/windows/features.rs:28 | (c) | The variables PowerShell and WMI read to start; a list of names, not a tunable. |
| `FEATURE_LISTING_MAX` | 4,096 bytes | crates/hex-core/src/setup.rs:125 | (d), convertible to (c) | Picked. The listing's size is fixed by its format: one `<name> <state>` line per feature of `WindowsFeature::ALL`, 36 bytes for both with CRLFs ("HypervisorPlatform 1" 22, "Containers 1" 14, as jupiter printed it). Convertible to that derived length (plus PowerShell's own line ending), test-first. Step 6's code: proposed to its agent. |
| routing test: simulated guest | 1 vCPU, 128 MiB | crates/hex-cli/tests/windows_process_box.rs:198-199 | (c) | Was a picked 1 and 256 (test only); now Shards' minimums (a0cced87 `runtime.rs:1061` "at least one vCPU is required", `x86_64.rs:28` `MIN_MEMORY_MIB = 128`). The simulated guest reads neither. |
| tests' box bound | 256 MiB | crates/hex-cli/tests/windows_process_box.rs:298, :336 | (c) | The conformance suite's own bound for the memory cases (`hex_api::conformance`), the same on every OS, so classes compare. |
| tests' hog | 2 GiB | crates/hex-cli/tests/windows_process_box.rs:309 | (d), test only | Any size past the 256 MiB bound shows the refusal. Convertible: derive it as a multiple of the bound, but the multiple is itself picked. |
| tests' waits | 30 s x 7 | crates/hex-cli/tests/windows_process_box.rs:312, :346, :386, :416, :507, :518, :547 | (d), test only, with data | Each ends on a fact (a program's exit, a box emptying); the timeout only turns a hang into a failure. Measured on jupiter: hex's memory-bound end p50 187-259 ms, max 361 ms over 100 runs; delete p99.9 8 ms (records entries 7-8). Proposed: one shared bound (`hex_api::conformance::PATIENCE`) instead of seven literals, derived once by its owner. |
| tests' read chunk | 256 bytes | crates/hex-cli/tests/windows_process_box.rs:520 | (d), test only | A read buffer for a program's output; any size reads it whole in a loop, so it bounds no behaviour. |
| lab `--test-threads=1` | 1 | bench/install/windows_lab.ps1:35 | moot | Each call names one test with `--exact`, so only one test runs whatever the thread count. Step 6's script. |
| `jupiter.sh` status lines | 20 → removed | bench/install/jupiter.sh:69 | was (d) | Now records the number of paths that differ from the commit, uncapped (266df3a). |
| OCI version | "1.2.0" | windows_process_box.rs test bundles | (c) | The runtime-spec version the bundles are written to. |
| jupiter tooling (not in the tree) | `CARGO_BUILD_JOBS=8`; scheduled-task limits 8 h, 2 h | kiln runs and `~/.cache/hex-t7-windows/*.sh` on jupiter | (d) | 8 of jupiter's 32 CPUs, picked to leave room for its co-tenants (Amphora); task limits picked as hang bounds, far past the measured build (2 min 29 s) and test runs (1-106 s). |

Open for a project-wide rule (not mine to pick alone): how far past a measured maximum a bound on a wait or a
margin sits (OVERSHOOT_MARGIN, PATIENCE, the tests' waits). Every (b) above names its record; the margin over it
is the remaining (d).
