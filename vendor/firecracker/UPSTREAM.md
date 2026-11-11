# Hex's changes to Firecracker

Firecracker is vendored (vendor/MANIFEST.toml) at `500efc6bb169705f5d9417cb411e3f6723e051fe`, upstream's
VFIO development chain (`feature/vfio`), in no release yet: the official preboot PCI passthrough Hex's
`firecracker` class builds on (crates/hex-runtime/src/firecracker, its `PREBOOT_REVISION`). Hex compiles
none of it. Only the files Hex's changes touch are taken, with LICENSE and NOTICE (Apache-2.0); each
recorded change (`vendor/patches/firecracker/*.patch`, applied in name order) is a row here, and
`scripts/vendor.py check firecracker` proves this tree is that upstream plus these, byte for byte.
`scripts/firecracker/build.sh` rebuilds the VMM from the same upstream commit plus these changes.

| Files | Change | Why | Patch |
| --- | --- | --- | --- |
| `src/vmm/src/vfio.rs`, `src/vmm/src/pci/{mod,msi}.rs`, `src/vmm/src/device_manager/pci_mngr.rs`, `src/vmm/src/rpc_interface.rs`, `src/firecracker/src/api_server/parsed_request.rs`, `src/firecracker/src/api_server/request/device_passthrough.rs` | A passthrough device keeps one exclusive, counted MSI/MSI-X source whose guest-programmed address routes through the existing KVM irqfds; `GET /device-passthrough/{id}` answers the device's id, host SBDF and the guest SBDF the VMM assigned, and refuses when its interrupt programming failed. | Hex joins a guest's native GPU facts to the exact host PCI function it selected only through the VMM's own assignment record, never an ordinal guess (transport.rs `attachment`, observations.rs `join`). Hex commit `2c62f12a0a4c90ea4d2a806c44797e12a582f594` on this pin. | `0001-vfio-emulate-msi-sources-and-expose-actual-passthrough.patch` |
| `Cargo.lock` | Adds aws-lc-rs 1.18.1's published crates.io source and checksum and its aws-lc-sys 0.45.0 dependency closure; every existing package version is kept. | Upstream's lock at this pin lists aws-lc-rs without them, so `cargo build --locked` refuses before compiling. No manifest, feature, source or compiler change. Hex commit `6b1a0431b389118785da07a5223bb4f0ec182fa7`. | `0002-build-complete-pinned-aws-lc-dependency-lock-metadata.patch` |
