# Hex's changes to hyper-raft

hyper-raft is vendored (vendor/MANIFEST.toml) at `6a236ae4c73e10013b149699ca8ec6802d919476`, only the
files its `take` list names. Each recorded change (`vendor/patches/hyper-raft/*.patch`, applied in
name order) is a row here. `scripts/vendor.py check hyper-raft` proves the tree is that upstream plus
these, byte for byte.

| Files | Change | Why | Patch |
| --- | --- | --- | --- |

None: hex-sim includes `crates/hyper-swim/src/coordinates.rs` unchanged (`crates/hex-sim/src/lib.rs`,
`#[path]`), so its scores use the engine hyper-swim ships (`docs/design/hex-sim.md` H12).
