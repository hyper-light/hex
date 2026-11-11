# Whole retained-owner startup barrier

Source checkpoint only. The original actual two controls failed at
416458dd9917f35fa4d1040eb74cc7768e3bd0c1; this remedy has not been compiled or run.
The six added controls are also unrun. See recorded-preflight-remedy-source.json.
The unfinished Mac CPU/helper draft is excluded.

ROOT CLI ordering with the SAME Arc and already initialized immutable backend configuration:

```rust
node.bind_guest_books()?; // Weak registration only; no second Recorded/Store.
node.preflight_recovery()?; // All retained Box +Pool rows, structural +pure backend configuration.
node.prepare_guest_backing()?; // Prepared record only; no VM/probe/grant.
node.recover_guest_backing()?; // Original already-admitted parent generation.
node.recover()?; // Repeats the barrier before native observation/cleanup.
// Start serving new requests only after the above succeeds.
```

The two backing wrappers recheck the whole barrier themselves. No private runtime exposure,
GPU probe, native capacity or fresh placement is used to validate retained state. Unknown
configuration/generation, malformed record/identity or conflicting claims preserves all bytes
and charges. Existing native owners already running are not cancelled by this startup API.

Linux Node consumes exactly this existing-owner seam:

```rust
let records = self.store.retained_records()?;
let pools = self.store.all_pools()?;
crate::recorded::preflight_records(&records, &pools, Some(self.attachments.as_ref()))?;
// Validate EVERY retained record/pool against ALL actual initialized backend configurations,
// through their pure preflight_record/preflight_backing hooks. No native validate_binding here.
// Only then call recover_backends, remove_strays, native probes/adoption/cleanup or Store mutation.
```

Common helper signature:

```rust
pub(crate) fn preflight_records(
    records: &hex_runtime::devices::Records,
    pools: &[hex_core::record::PoolRecord],
    catalog: Option<&hex_runtime::devices::Catalog>,
) -> Result<(), hex_api::Refusal>;
```

ROOT retains its current canonical 5f lifecycle/authority fixture and schema ports. The original
4687 preflight leaf is a byte-identical prefix of the new leaf. Its two original test bodies are
not edited; six additional tests follow them. If ROOT's lifecycle fixture already registers it,
keep that registration. Otherwise append only once INSIDE that existing parent fixture:

```rust
#[path = "recorded_preflight_tests.rs"]
mod retained_preflight;
```

The child uses the real Recorded/Store and counted SimRuntime. No HCS/VM/native proof is implied.
ROOT joins the exact dependencies and owns the changed-cohort compiler/Clippy/8 focused controls,
mutation/fault/native/gate work. No owner duplicate dispatch was made.
