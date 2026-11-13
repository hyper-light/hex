# Hex's `shards` build, with its recorded changes

Shards is vendored here (vendor/MANIFEST.toml `[projects.shards-gpu]`) at `e82b9403b863718930fe9f224186d3ae862bfd61`
for the `shards` binary a node boots its Shards guests and image VMs with (SHARDS_BIN): its GPU VM process renders a
Shards GPU guest's Venus on the host's driver, and its `shards api-run` hosts an image's boot (D231). Hex compiles
none of it. Only the files Hex's changes touch are taken, with LICENSE (MIT); each recorded change
(`vendor/patches/shards-gpu/*.patch`) is a row here, and `scripts/vendor.py check shards-gpu` proves this tree is that
upstream plus them, byte for byte. `scripts/shards-gpu/build.sh` rebuilds `shards` from the same upstream commit plus
vendor/patches/shards (the API the run host and Hex's node both speak) and these, on Linux, macOS and Windows.

Every change Hex carried at 0342b85d (patches 0001-0008, the hex/physical-gpu-0342 chain) is upstream at this
revision: Shards' hex-upstream-v1 maps each to its commit (docs/research/measurements/hex-upstream-v1/README.md),
D232 and D233 record them. Upstream ported GPU 0002 without its Windows arm64 packaging, which its ROOT's GPU lane
holds (D232, "Held"), and without its gpu-libs scratch move; the first is change 0001 here, and the second is
`scripts/shards-gpu/build.sh`'s TMPDIR under ~/.cache instead. Change 0002 is joint plan step 15's, offered to
Shards' ROOT for upstreaming with its API half (vendor/shards change 0002).

| Files | Change | Why | Patch |
| --- | --- | --- | --- |
| `crates/shards/build.rs`, `scripts/gpu-libs` | The GPU's VM process built and carried for Windows on arm64: `gpu_target` takes `aarch64-pc-windows-msvc`; gpu-libs names a Windows-aarch64 host from `SHARDS_GPU_TARGET` (else the shell's architecture), builds the renderer and identity bridge through an explicit COFF cross file, and pins ninja's and pkgconf's arm64 Windows wheels by hash. | Upstream builds and carries the GPU VM process for x86_64 Windows alone; its arm64 build is held by Shards' GPU lane (D232). Hex's GPU 0002 carried it; rebased here, the one change upstream does not hold. Since 2365d784 its Windows branches take upstream's two x86_64 fixes for arm64 too: the identity bridge copied as `lib*.a`, meson's name for it on Windows (1cd72867), built for rustc's C runtime, `/MD` (1f86d092). A host running the tools under x64 emulation on ARM must not build an x64 renderer, hence the explicit machine (AC-T7.4.1). | `0001-gpu-build-and-carry-the-gpu-vm-process-for-windows-on-arm64.patch` |
| `crates/agentfile/src/agentfile.rs`, `crates/agentfile/tests/agentfile.rs`, `crates/shards/src/agentfile.rs`, `crates/shards/src/api_run.rs` | The run host of an image's boot holds each member to its reach of the destinations kept: `agentfile::named_reach` reads what each member reaches by name (its networks' destinations and its remote MCP servers'), the run host's `Agentfile` keeps it (`named`), and where a request names each member's reach (`Narrowing::members`, vendor/shards change 0002) `api_run`'s `held_apart` refuses the boot, a widening, before the VM starts, unless every member the image declares reaches exactly that of the destinations kept; a member the request names that the image declares not is the image's, one named twice the request's. Tests: each member's reach read, and every case of the hold. | Joint plan step 15: a harness reaches exactly the providers and the host its grants name. D236 keeps a destination image-wide; under its `held` a member reaches exactly its own names among those kept, so holding every member to its reach at the run host, which reads the Agentfile, makes the boot exact before anything runs; init and the network process are unchanged (Hex's lead, 2026-11-12; docs/design/network-grants.md N4). | `0002-image-run-host-each-member-held-to-its-reach-of-the.patch` |
