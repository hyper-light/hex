# Hex's changes to hyper-raft

hyper-raft is vendored (vendor/MANIFEST.toml) at `6a236ae4c73e10013b149699ca8ec6802d919476`, only the
files its `take` list names. Each recorded change (`vendor/patches/hyper-raft/*.patch`, applied in
name order) is a row here. `scripts/vendor.py check hyper-raft` proves the tree is that upstream plus
these, byte for byte.

| Files | Change | Why | Patch |
| --- | --- | --- | --- |

None. hex-sim depends on hyper-swim and hyper-timing unchanged, by name, patched to this copy
(`Cargo.toml` [patch.crates-io]), so its crash class runs the detector hyper-swim ships
(`docs/design/failure-isolation.md` F1); and with G3's branch it includes
`crates/hyper-swim/src/coordinates.rs` unchanged (`crates/hex-sim/src/lib.rs`, `#[path]`), so its
scores use the engine hyper-swim ships (`docs/design/hex-sim.md` H12).
