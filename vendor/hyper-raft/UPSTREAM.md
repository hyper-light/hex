# Hex's changes to hyper-raft

hyper-raft is vendored (vendor/MANIFEST.toml) at `0856e05df3d136f0870a98808113294a73399206`, the revision kiln
vendors, only the files its `take` list names. Each recorded change (`vendor/patches/hyper-raft/*.patch`,
applied in name order) is a row here. `scripts/vendor.py check hyper-raft` proves the tree is that upstream plus
these, byte for byte.

| Files | Change | Why | Patch |
| --- | --- | --- | --- |
| `crates/hyper-transport/Cargo.toml` | hyper-transport depends on hyper-quic with `default-features = false` | hyper-transport makes no qlog trace, and hyper-quic's default `qlog` feature turns on serde_json's `preserve_order` for every crate of the build it joins (hyper-quic `VENDORED.md` §20), which would change the key order of every JSON document Hex writes. Hex asks hyper-quic without it by name, as Shards does (its `crates/remote/Cargo.toml`); this keeps hyper-transport's own path dependency from turning it back on | `0001-hyper-transport-takes-hyper-quic-without-its-qlog-traces.patch` |
| `crates/hyper-transport/src/endpoint.rs` | `Endpoint::negotiated(peer)`: the TLS 1.3 cipher suite and key exchange group a connection negotiated, read from hyper-quic's handshake data | every link of the joint kiln, Hex and Shards plan takes `TLS13_AES_256_GCM_SHA384` alone, and a test at each end of a connection asserts the suite it negotiated (docs/design/hex-quic.md Q5); hyper-transport held each connection's TLS session and gave no way to read it | `0002-hyper-transport-says-which-suite-and-group-a-connection.patch` |
| `crates/hyper-transport/src/budget.rs`, `crates/hyper-transport/src/receive.rs` | `Fixed::new` and `Receive::new` start empty and grow to their bounds (`max_spare` buffers, `limit` chunks) as buffers come back and chunks are added | each laid out room for its whole bound at the start, so an endpoint paid its bounds as memory before any peer came: a Hex node's bounds are its own connections and programs (docs/design/hex-quic.md Q8), and a client's lanes its descriptors (2^23 on Windows), hundreds of megabytes or more than a process can allocate. The bounds themselves are unchanged | `0003-hyper-transport-grows-its-spare-buffers-and-receive-chunks-to-their-bounds.patch` |

All three are Hex's to offer upstream: the first as a choice hyper-transport's manifest can make itself, since it
names no qlog type; the second as an accessor beside `export_keying_material`; the third as bounds kept as bounds.
Once hyper-raft carries one, the re-vendor drops it.

hex-sim depends on hyper-swim and hyper-timing unchanged, by name, patched to this copy (`Cargo.toml`
[patch.crates-io]), so its crash class runs the detector hyper-swim ships
(`docs/design/failure-isolation.md` F1); and it includes `crates/hyper-swim/src/coordinates.rs` unchanged
(`crates/hex-sim/src/lib.rs`, `#[path]`), so its proximity scores use the engine hyper-swim ships
(`docs/design/hex-sim.md` H20). hex-quic depends on hyper-transport and hyper-quic by name, patched the same
way (`docs/design/hex-quic.md` Q1).
