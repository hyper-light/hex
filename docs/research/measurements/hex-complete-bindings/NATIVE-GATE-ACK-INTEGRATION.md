# Development-only native gate acknowledgement integration

Source-only L1 implementer, fixed ROOT `a6b20f34d815d41d5ae36233352e576f5c56b2c3`; current Shards7d7e/df2 policy. ROOT owns manifests/registration/checks. No new dependency version, crate, lock-package edge, production DTO or global FD.

Add to `crates/hex-runtime/Cargo.toml`:

```toml
[features]
# Read-only gate phase witness for Node's development/native fixtures. No execution authority.
native-gate-test = []
```

Add to `crates/hex-node/Cargo.toml`:

```toml
[target.'cfg(target_os = "linux")'.dev-dependencies]
hex-runtime = { workspace = true, features = ["native-gate-test"] }
```

The normal dependency stays unchanged. Cargo test feature unification supplies the observational method to Node's normal runtime dependency in its Linux libtests; runtime `cfg(test)` alone would not. ROOT must preserve any existing table/feature fields and record the actual resulting metadata/lock diff, including no diff if appropriate.

L4's existing `supervise::completion_native_gate_tests` fixture receives the SAME `ManagedVm` from `reserve_managed`. Before spawning its bind actor, call `let isolation = native.prepare_isolation_ack()?`. `bind_managed` consumes that one private notification writer and, after actual held ownership/watcher plus real byte2, emits `[2]` before publication. The reader carries no execution/release authority; there is no raw held-gate FD or caller-supplied producer writer.

Read one bounded notification event under the fixture's original absolute D/G rules. EOF/error/wrong/extra-byte facts remain explicit. Do **not** require pipe EOF before considering actual byte2 established: the missing-isolation mutant may deliberately retain the probe writer in a child. Preserve actual parent terminal `<D`, first Code127 `<G`, held publication refusal, identity/birth/pidfd, sibling STOP, canaries and full cleanup. See L4 signed `26b7820f22d5bc98eb2c443cbe3bf587f0e5ae55` `native-gate-r05/GATE-ACK-CAUSAL-PROOF.md` for exact unchanged oracles and meaningful mutation conditions.

No native fixture code/oracle/manifest/check was changed or run by this producer checkpoint. Three new bounded protocol/pipe controls are source-only; ROOT owns their matched light run and native mutation/campaign. Source hashes and exact unchanged production function readbacks are adjacent JSON files.
