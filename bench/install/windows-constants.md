# The Windows line's constants (Ada's rule, 2026-11-06: learned or data-derived values only)

Every constant the Windows line adds or relies on: the `process` class on job objects (native step 3), the
installer's Windows part (step 6), the routing of `oci` boxes by image (steps 4 and 5) and the jupiter tooling.
Step 4's HCS class (`crates/hex-runtime/src/windows/oci/`, `crates/hex-core/src/hcs.rs`) and step 5's guest
(`crates/hex-runtime/src/guest/`, `crates/hex-client/src/guest*`) are audited by their own agents. Kinds:
(a) learned at runtime, (b) derived from measured data with its record, (c) a platform or spec limit cited at its
source, (d) picked or copied.

| Name | Value | file:line | Kind | Source, or why it stays (d) |
|---|---|---|---|---|
| `OVERSHOOT_MARGIN` | 2,326,528 bytes | crates/hex-core/src/job.rs | (b) | 2 × `LARGEST_RECORDED_OVERSHOOT` = 1,163,264 bytes, the measured max over a 64 KiB-256 MiB step sweep (`bench/native/windows/RESULT.md`, record `job-events-68e99688-*`); ×2 is Hex's false-alarm margin (3fc48ee). |
| `NOTIFY_MARGIN` | 4 MiB | crates/hex-core/src/job.rs:28 | (b) | Measured: a notification at the hard limit never fires, and one 4 MiB under fired in every pass (same record, experiment e17). Only 4 MiB was measured, not the smallest margin that fires. |
| `TERMINATED` | 0xE048_0000 | crates/hex-core/src/job.rs:33 | (c) | MS-ERREF §2.3: severity error, customer bit 29, facility 0x048, so no status of Windows' own collides. |
| `DEFAULT_WEIGHT` | 5 | crates/hex-core/src/job.rs:37 | (c) | `jobobject_cpu_rate_control_information.md:147`: "The default is 5". |
| `RATE_SCALE` | 10,000 | crates/hex-core/src/job.rs:40 | (c) | `jobobject_cpu_rate_control_information.md:132`: a hard cap is cycles per 10,000. |
| `weight()` ranges | 1..=10,000 → 1..=9, 100 ↦ 5 | crates/hex-core/src/job.rs:176, :192 | (c) | cgroup v2's `cpu.weight` range and default (1-10,000, 100) onto Windows' `Weight` range and default (1-9, 5). The piecewise-linear shape between the pinned ends is a design choice, with the share each weight gives measured in `bench/native/windows/RESULT.md`. |
| `LIVE_CAP` | 1,024 → removed | crates/hex-runtime/src/windows/sys.rs (`Job::live_processes`) | was (d), now (a) | Was a picked id-buffer size (a box past it read as 1,024). Now one read with room for one id returns the job's own `NumberOfAssignedProcesses`, filled in at any size (`ERROR_MORE_DATA`). The test `an_ended_member_whose_handle_is_held_leaves_the_id_list` shows on jupiter that this count agrees with the ids listed and keeps no ended member. `windows_process_box` passes 10/10 with it. |
| temp-path buffer | 261 u16 | crates/hex-runtime/src/windows/sys.rs:97 | (c) | `GetTempPath2W`'s documentation: "The maximum possible return value is MAX_PATH+1 (261)". Step 5's code. |
| `setup::FEATURE_QUERY_PATIENCE` (features.rs `PATIENCE` uses it) | 4,842 ms | crates/hex-core/src/setup.rs:149, :154 | (b) | 2 × `FEATURE_QUERY_SLOWEST` = 2,421 ms, the slowest across both records: this branch's 30 runs (windows-jupiter-20261105.txt entry 19) and step 6's 20 (windows-feature-query-jupiter-20261106.txt, max 1,683 ms). One definition, step 6's (8b8625d, merged at 1c00e5d). |
| features `ENVIRONMENT` | 5 names | crates/hex-runtime/src/windows/features.rs:28 | (c) | The variables PowerShell and WMI read to start; a list of names, not a tunable. |
| `FEATURE_LISTING_MAX` | 36 bytes | crates/hex-core/src/setup.rs:130 | (c) | Computed from the format: `WindowsFeature::ALL`'s names plus " N\r\n" each (step 6, 09b853a test, 1088b42 fix; merged at 6c1ca30). |
| routing test: simulated guest | 1 vCPU, 128 MiB | crates/hex-cli/tests/windows_process_box.rs:198-199 | (c) | Was a picked 1 and 256 (test only); now Shards' minimums (a0cced87 `runtime.rs:1061` "at least one vCPU is required", `x86_64.rs:28` `MIN_MEMORY_MIB = 128`). The simulated guest reads neither. |
| tests' box bound | 256 MiB | crates/hex-cli/tests/windows_process_box.rs:298, :336 | (c) | The conformance suite's own bound for the memory cases (`hex_api::conformance`), the same on every OS, so classes compare. |
| tests' hog | 2 GiB | crates/hex-cli/tests/windows_process_box.rs:309 | (d), test only | Any size past the 256 MiB bound shows the refusal. Convertible: derive it as a multiple of the bound, but the multiple is itself picked. |
| tests' waits | 30 s x 7 | crates/hex-cli/tests/windows_process_box.rs | (d) here, to become (b) | They become the one shared `hex_api::conformance::PATIENCE`, which step 7 has converted to 2 × SLOWEST_RECORDED_END (6,239 ms, a jupiter run) on cp/t7-native-s7; taken when that merges, not a copy of my own. |
| tests' read chunk | 256 bytes | crates/hex-cli/tests/windows_process_box.rs:520 | (d), test only | A read buffer for a program's output; any size reads it whole in a loop, so it bounds no behaviour. |
| lab `--test-threads=1` | removed | bench/install/windows_lab.ps1:35 | was moot | Step 6 dropped it (1cf3a19). |
| `jupiter.sh` status lines | 20 → removed | bench/install/jupiter.sh:69 | was (d) | Now records the number of paths that differ from the commit, uncapped (266df3a). |
| OCI version | "1.2.0" | windows_process_box.rs test bundles | (c) | The runtime-spec version the bundles are written to. |
| jupiter tooling (not in the tree) | `CARGO_BUILD_JOBS=8`; scheduled-task limits 8 h, 2 h | kiln runs and `~/.cache/hex-t7-windows/*.sh` on jupiter | (d) | 8 of jupiter's 32 CPUs, picked to leave room for its co-tenants (Amphora); task limits picked as hang bounds, far past the measured build (2 min 29 s) and test runs (1-106 s). |

The rule (the lead, 2026-11-06, for every Hex agent): a hang bound, or a tolerance past a measured effect, is twice the slowest or largest value recorded, the ×2 stated where it is defined as the false-alarm margin.
