# Hex's changes to all-smi

all-smi (https://github.com/lablup/all-smi) is vendored (vendor/MANIFEST.toml) at
`fe479ae675e39475a2eab1cfbb3f5432f705e12b` (2026-11-09), only the files its `take` list names: `LICENSE`
and `crates/all-smi-amd-plugin/src/reader.rs`. Each recorded change (`vendor/patches/all-smi/*.patch`,
applied in name order) is a row here. `scripts/vendor.py check all-smi` proves the tree is that upstream
plus these, byte for byte.

**What hex-gpu takes, and why only this.** hex-gpu reads AMD GPUs through amdgpu's sysfs counters
(`docs/design/gpu-telemetry.md` G2). all-smi's Linux AMD reader is a companion library around
libamdgpu_top (`crates/all-smi-amd-plugin`), which reads through libdrm's ioctls on the device node and
hands all-smi JSON; its one direct read of an amdgpu counter is `read_gpu_busy_percent`, the fallback when
`gpu_metrics` carries no activity. That reader is taken, and the VRAM counters join it, read the same
way: `mem_info_vram_used` and `mem_info_vram_total`, which all-smi reads by those names for Intel's
discrete GPUs (`src/device/readers/intel_gpu_sysfs.rs`, not taken: its other paths are i915's and xe's)
and amdgpu prints the same way. all-smi's AMD names come from libdrm's `amdgpu.ids` through
libamdgpu_top; hex-gpu reads that table itself (`crates/hex-gpu/src/amd.rs`). On Windows all-smi reads
AMD GPUs through ADL (`src/device/readers/amd_adl/`) and WDDM's PDH and DXGI
(`src/device/readers/windows_gpu_perf/`); none of it is taken yet (decision G3 says what it would take).

**License.** Apache-2.0 (`LICENSE`, taken unchanged; upstream has no NOTICE file). It allows copying and
changing the code inside Hex's MIT-licensed tree, given that the license travels with it (`LICENSE`), the
copyright and license lines stay in the file (they do), and a changed file says it was changed (its
header names Hex's change and this file).

| Files | Change | Why | Patch |
| --- | --- | --- | --- |
| `crates/all-smi-amd-plugin/src/reader.rs` | Keeps `read_gpu_busy_percent` and `parse_gpu_busy_percent`, reading from a file opened once with one `pread` at offset 0 into a stack buffer and parsing the line in place; a percent past 100 is refused rather than clamped; `EPERM` (a runtime-suspended GPU) reads as asleep and `ENODEV` (a removed device) as gone. Adds `read_vram_bytes` for `mem_info_vram_used` and `mem_info_vram_total`, read the same way. Removes the rest: libamdgpu_top's reader, its sensor, power and process paths, and the plugin glue. | A sample is the scheduler's hot path: no path joined, file opened or string built per read, and sysfs runs the attribute's `show` again for every read from offset 0 (fs/seq_file.c). CLAUDE.md §3: a value no counter can hold is refused, never altered. amdgpu answers a suspended GPU's busy counter `EPERM` rather than wake it (drivers/gpu/drm/amd/pm/amdgpu_pm.c), and kernfs answers an open file of a removed device `ENODEV` (fs/kernfs/file.c). Hex reads the counters directly and links neither libamdgpu_top nor libdrm. | `0001-reader-keep-the-gpu-busy-percent-reader-add-the-vram.patch` |
