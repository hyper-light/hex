# Hex's changes to Shards' renderer build

Shards is vendored here (vendor/MANIFEST.toml `[projects.shards-gpu]`) at `2365d7844f7b4ecdfbba9aa94579b8fc007f3e63`
for the `shards` binary a node's Shards GPU guests boot with (SHARDS_BIN), whose GPU VM process renders their Venus
on the host's driver. Hex compiles none of it. Only the files Hex's change touches are taken, with LICENSE (MIT);
the recorded change (`vendor/patches/shards-gpu/*.patch`) is a row here, and `scripts/vendor.py check shards-gpu`
proves this tree is that upstream plus it, byte for byte. `scripts/shards-gpu/build.sh` rebuilds `shards` from the
same upstream commit plus this change.

Every change Hex carried at 0342b85d (patches 0001-0008, the hex/physical-gpu-0342 chain) is upstream at this
revision: Shards' hex-upstream-v1 maps each to its commit (docs/research/measurements/hex-upstream-v1/README.md),
D232 and D233 record them. Upstream ported GPU 0002 without its Windows arm64 packaging, which its ROOT's GPU lane
holds (D232, "Held"), and without its gpu-libs scratch move; the first is the one change here, and the second is
`scripts/shards-gpu/build.sh`'s TMPDIR under ~/.cache instead.

| Files | Change | Why | Patch |
| --- | --- | --- | --- |
| `crates/shards/build.rs`, `scripts/gpu-libs` | The GPU's VM process built and carried for Windows on arm64: `gpu_target` takes `aarch64-pc-windows-msvc`; gpu-libs names a Windows-aarch64 host from `SHARDS_GPU_TARGET` (else the shell's architecture), builds the renderer and identity bridge through an explicit COFF cross file, and pins ninja's and pkgconf's arm64 Windows wheels by hash. | Upstream builds and carries the GPU VM process for x86_64 Windows alone; its arm64 build is held by Shards' GPU lane (D232). Hex's GPU 0002 carried it; rebased here, the one change upstream does not hold. At 2365d784 its Windows branches take upstream's two x86_64 fixes for arm64 too: the identity bridge copied as `lib*.a`, meson's name for it on Windows (1cd72867), built for rustc's C runtime, `/MD` (1f86d092). A host running the tools under x64 emulation on ARM must not build an x64 renderer, hence the explicit machine (AC-T7.4.1). | `0001-gpu-build-and-carry-the-gpu-vm-process-for-windows-on-arm64.patch` |
