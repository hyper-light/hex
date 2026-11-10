# CDI codec implementation readback

Implementer Kiln `01a1c640e6ed710d8a8c919b4aa1fc8c`; coordinator Hex
`01a1aa77778471339bea3beff90d6bbd`. Branch `cp/t7-cdi-device-edits` starts at
`37021c35e64b2a8bab1d35f20f92a0042d874a44`. Prior relay branch/checkpoint
`cp/t7-adversarial-relay` / `20268f5cc695b074ee4e3e2fa7c36d896dcc4468`
and its original proof are preserved.

## POLICY_READ

Read FULL current Hex CLAUDE.md and HANDOFF-codex.md, current Shards origin/main
AGENTS.md, docs/development-rules.md R01–R16, docs/development-evidence.md,
CLAUDE.md, managed-runtime-plan.md and `/Users/adalundhe/g121-handoff.md` before
edits/tests. Shards revision, verified with origin ls-remote:
`cbd39284937d024640c56db6f8d8ef2bd9b0ee24`. Rules SHA256:
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
Hex CLAUDE SHA256 `8708b80967e5517ae9137eb642600668ac02d164a4c2145ad8462a37889d424e`;
Hex handoff SHA256 `9b780baaa9ff3dc1d491624d99a217ceb7a2b7cc913c1f56e0598f6d8f8694c1`.

Only NEW `crates/hex-core/src/cdi.rs`, `crates/hex-core/src/cdi/tests.rs`, and this
evidence directory are owned. ROOT owns lib.rs registration, shared APIs, native
adapters, integration/review/audits, all native/heavy/VM/benchmark/mutation/full
gates and merges. This assignment is implementation/light-only despite broader
current R15. No children, goals, plans or automatic dispatch; no laptop builds or
tests. Scoped pure tests/core Clippy only on d1323898b255a3ed435eb83fa0f7b0cf,
cpus1/mem4g/disk12g, nice10/jobs1/threads1, NEW private source/target/cache and
precise --no-sync transfer. Private-only lib.rs registration must be recorded.
User's scope message ACKs scope after full readback; no additional permission wait.
First readback sent stable key `hex-cdi-policy-read-cbd39284-37021c35-v1`.

## Exact interface and ownership

```rust
impl Spec {
    pub fn parse(bytes: &[u8], max_bytes: usize) -> Result<Spec, Error>;
    pub fn select<'s>(&'s self, qualified: &[&str], max_selected: usize)
        -> Result<Edits<'s>, Error>;
}
```

Spec owns validated data. Edits borrows complete selected data from Spec and owns
only bounded, fallibly reserved reference lists. Its fields represent env,
device_nodes, mounts, hooks, net_devices, intel_rdt and additional_gids. Every
node/mount/hook/network/RDT field is retained, including optional-value presence.
Spec/Edits/payload Debug redact content. Errors carry typed field/index/cause
and resource counts only, never serde messages, field values, paths or env data.

This is one-spec JSON parsing and qualified-name selection, not a file loader,
registry, cache, OCI injector, device admission manager or OS adapter. It creates
no mounts/devices/hooks, and does not infer host device identity or missing stats.
Unknown/duplicate members and unsupported values refuse by type; no partial
selection escapes on a duplicate/unresolved name. Caller byte/count bounds apply
before parsing/selection. Selected name length is bounded by the source spec's
byte length before scanning. There is no new production tuning constant.

## Pinned source contract

All source below is commit `98a7d735d9592c1ccca0130e06b0709f4b65ecbf` (CDI1.1.0).
ROOT chose CDI and records the broader vendor/native decision separately.

- [Go selector](https://github.com/cncf-tags/container-device-interface/blob/98a7d735d9592c1ccca0130e06b0709f4b65ecbf/pkg/cdi/cache.go): apply globals once, followed by requested devices in request order, and fail unresolved selection. This module rejects repeated requested names explicitly, as the assignment requires.
- [Go edits](https://github.com/cncf-tags/container-device-interface/blob/98a7d735d9592c1ccca0130e06b0709f4b65ecbf/pkg/cdi/container-edits.go): Append concatenates each list; last non-null intelRdt replaces the prior whole value. Preserve duplicate env/path operations in that order. Do not invent conflict rejection or prematurely coalesce them. OCI Apply replacement, mount ordering, stat/default filling and zero-GID handling remain native caller work; zero additionalGids remain represented.
- [Go names](https://github.com/cncf-tags/container-device-interface/blob/98a7d735d9592c1ccca0130e06b0709f4b65ecbf/pkg/parser/parser.go): use actual validator ASCII grammar, including internal device colon, first-letter vendor/class and alphanumeric endpoints. SPEC prose instead describes DNS-prefix/63-character-class constraints; do not invent a hybrid. One-letter components are handled without indexing/panic.
- [Go versions](https://github.com/cncf-tags/container-device-interface/blob/98a7d735d9592c1ccca0130e06b0709f4b65ecbf/specs-go/version.go): known released canonical0.3.0–0.8.0,1.0.0,1.1.0 with minimum-feature gates. Strict canonical JSON spelling rejects leading-v/prerelease/future versions. Mount type requires0.4; numeric device names/hostPath0.5; annotations/class dots0.6; RDT/additionalGids0.7; netDevices/RDT schemata/monitoring1.1.
- [Go types](https://github.com/cncf-tags/container-device-interface/blob/98a7d735d9592c1ccca0130e06b0709f4b65ecbf/specs-go/config.go): canonical `additionalGids` and `schemata`; keep all defined fields. Some SPEC examples have inconsistent spellings, which remain unknown-field refusals. Annotations are retained CDI metadata and do not become OCI metadata.
- [Go spec/device validators](https://github.com/cncf-tags/container-device-interface/blob/98a7d735d9592c1ccca0130e06b0709f4b65ecbf/pkg/cdi/spec.go) and [device validator](https://github.com/cncf-tags/container-device-interface/blob/98a7d735d9592c1ccca0130e06b0709f4b65ecbf/pkg/cdi/device.go): require a nonempty device set, unique names and nonempty per-device edit data; validate each edits object independently. No cross-object conflict rules are added.
- [SPEC](https://github.com/cncf-tags/container-device-interface/blob/98a7d735d9592c1ccca0130e06b0709f4b65ecbf/SPEC.md): positive hook timeout and absolute hook executable path; distinguish pure data validation from resource-existence/runtime failures. No hardware or native capability claim follows from a parsed plan.

Compatibility limits and exact source hashes, tests, negative results and owed
ROOT checks will accompany each signed checkpoint. The initial checkpoint will
say SOURCE ONLY until registered, nonzero selected tests actually execute.
