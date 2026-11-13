# Hex's `shards` build, with its recorded changes

Shards is vendored here (vendor/MANIFEST.toml `[projects.shards-gpu]`) at `f176b3e61cabf377f436bfe967b4df88c3085236`
for the `shards` binary a node boots its Shards guests and image VMs with (SHARDS_BIN): its GPU VM process renders a
Shards GPU guest's Venus on the host's driver, and its `shards api-run` hosts an image's boot (D231). Hex compiles
none of it. Only the files Hex's changes touch are taken, with LICENSE (MIT); each recorded change
(`vendor/patches/shards-gpu/*.patch`) is a row here, and `scripts/vendor.py check shards-gpu` proves this tree is that
upstream plus them, byte for byte. `scripts/shards-gpu/build.sh` rebuilds `shards` from the same upstream commit plus
vendor/patches/shards (the API the run host and Hex's node both speak; none at this revision) and these, on Linux,
macOS and Windows.

Every change Hex carried at 0342b85d (patches 0001-0008, the hex/physical-gpu-0342 chain) is upstream at this
revision: Shards' hex-upstream-v1 maps each to its commit (docs/research/measurements/hex-upstream-v1/README.md),
D232 and D233 record them. Upstream ported GPU 0002 without its Windows arm64 packaging, which its ROOT's GPU lane
holds (D232, "Held"), and without its gpu-libs scratch move; the first is change 0001 here, and the second is
`scripts/shards-gpu/build.sh`'s TMPDIR under ~/.cache instead. The run host's hold of each member to its reach of
the destinations kept, Hex's change 0002 at e82b9403 (joint plan step 15), is upstream at this revision as D246
(7a4ef5c02), so its files (`crates/shards/src/api_run.rs` and `agentfile.rs`, `crates/agentfile`'s) are no longer
taken.

| Files | Change | Why | Patch |
| --- | --- | --- | --- |
| `crates/shards/build.rs`, `scripts/gpu-libs` | The GPU's VM process built and carried for Windows on arm64: `gpu_target` takes `aarch64-pc-windows-msvc`; gpu-libs names a Windows-aarch64 host from `SHARDS_GPU_TARGET` (else the shell's architecture), builds the renderer and identity bridge through an explicit COFF cross file, and pins ninja's and pkgconf's arm64 Windows wheels by hash. | Upstream builds and carries the GPU VM process for x86_64 Windows alone; its arm64 build is held by Shards' GPU lane (D232). Hex's GPU 0002 carried it; rebased here, the one change upstream does not hold. Since 2365d784 its Windows branches take upstream's two x86_64 fixes for arm64 too: the identity bridge copied as `lib*.a`, meson's name for it on Windows (1cd72867), built for rustc's C runtime, `/MD` (1f86d092). A host running the tools under x64 emulation on ARM must not build an x64 renderer, hence the explicit machine (AC-T7.4.1). | `0001-gpu-build-and-carry-the-gpu-vm-process-for-windows-on-arm64.patch` |
