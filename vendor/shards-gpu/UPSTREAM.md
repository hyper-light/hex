# Hex's `shards` build

Shards is vendored here (vendor/MANIFEST.toml `[projects.shards-gpu]`) at `16451e19f172cfb29882efcab6502cc43b8bd061`
for the `shards` binary a node boots its Shards guests and image VMs with (SHARDS_BIN): its GPU VM process renders a
Shards GPU guest's Venus on the host's driver, and its `shards api-run` hosts an image's boot (D231). Hex compiles
none of it. The files Hex's changes touched are taken, with LICENSE (MIT), and `scripts/vendor.py check shards-gpu`
proves this tree is that upstream, byte for byte: Hex records no change of its own at this revision.
`scripts/shards-gpu/build.sh` rebuilds `shards` from the same upstream commit, on Linux, macOS and Windows.

Every change Hex carried at 0342b85d (patches 0001-0008, the hex/physical-gpu-0342 chain) is upstream at this
revision: Shards' hex-upstream-v1 maps each to its commit (docs/research/measurements/hex-upstream-v1/README.md),
D232 and D233 record them. So are Hex's later two: the GPU's VM process built and carried for Windows on arm64
(`gpu_target` taking `aarch64-pc-windows-msvc`, gpu-libs building every Windows renderer as a meson cross build), its
change 0001 at e82b9403, is upstream as 9e01ef94a, byte for byte the files it changed; and the run host's hold of each
member to its reach of the destinations kept, its change 0002 at e82b9403 (joint plan step 15), is upstream as D246
(7a4ef5c02). Hex carries no change here: the files 0001 took (`crates/shards/build.rs`, `scripts/gpu-libs`) stay
taken, upstream's, beside LICENSE. gpu-libs' scratch move upstream did not take stays
`scripts/shards-gpu/build.sh`'s TMPDIR under ~/.cache.

## Recorded changes

None.
