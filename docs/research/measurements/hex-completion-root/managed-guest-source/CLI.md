# Exact guest integration source

Guest code pin: efce10fabb9d7dd3c6a58c29b63c504c73c79ade. Metadata/source.json
hashes every one of its12 owned guest files and verifies nine original charge/route
blocks byte-identical to ROOT0277. This includes the full4c3 managed consumer,
9ec Released/finalization codec and a21 ManagedReceipt consumer; importing only the
Released delta is insufficient. ROOT applies the full owned subtree through its
fixed-base merge and preserves outside-lane/native/vendor differences.

SDK Rust: ROOT0277 / exact upstream eee80; dependency-correct check source ROOT
2af7dd6adabe4b949def667802d679f9a3ebbe05. Correct seed SHA256
1e9ae2e5e95e103c27bce8e0f7294900984825d22c9926f770c63c872d547386
includes unsize->autocfg build dependency. Earlier5bc1 seed omits that edge and is
not a valid locked-check seed. All upstream manifests remain untouched. Required
other sources: Books48bacdff and L3native15203af1, full hashes in source.json.
No joined Hex compile/native result is claimed. Required hex-runtime triomphe
edge belongs to ROOT (also used by native Win/Mac owners), never a temporary stub.

Construct a GuestRuntime using the actual configured VM and the same initialized
RuntimeCost accepted by admission. `cost` is ROOT's measured/evidence-identified
profile, not a guessed zero allowance. The public configured constructor performs
no boot, allowing the existing owner to be created and weakly registered first:

```rust
fn configured_guest(
    guests: std::sync::Arc<dyn shards_api::Guests>,
    config: hex_runtime::guest::GuestConfig,
    catalog: std::sync::Arc<hex_runtime::devices::Catalog>,
    cost: hex_api::RuntimeCost,
    receipt: std::path::PathBuf,
) -> Result<hex_runtime::guest::GuestRuntime, hex_api::Refusal> {
    hex_runtime::guest::GuestRuntime::new(guests, config)
        .recorded_at(receipt)
        .with_gpu_catalog(catalog)
        .with_runtime_cost(cost)
}
```

Linux keeps the CLI's existing Arc<Node>. Supply its ONE SharedProber before
installing adapters. Pass Node::device_catalog() to configured_guest, then call
Node::install_backends(existing_firecracker, Some(Arc::new(guest))). That existing
method binds guest Books weakly to the SAME Node and checks the initialized cost
and native pool identity. Recover only after installing actual adapters, then serve
that same Arc<Node>. Do not wrap it in another Recorded or create a Process row.
The native launcher calls the canonical Tx publication callback while suspended,
before resume; the guest consumer verifies that actual PID/birth before Ready.

For macOS/Windows use the existing single Recorded owner after configuration. This
is a constructor snippet for ROOT to integrate into its native CLI, not a separate
runtime/store/holder implementation:

```rust
fn recorded_guest<P: hex_node::recorded::Recover + 'static>(
    process: P,
    guest: hex_runtime::guest::GuestRuntime,
    route: fn(&P, &hex_api::ClassSpec)
        -> Result<hex_runtime::guest::Side, hex_api::Refusal>,
    state: &std::path::Path,
    prober: hex_node::facts::SharedProber,
) -> Result<
    std::sync::Arc<hex_node::recorded::Recorded<hex_runtime::guest::WithGuest<P>>>,
    hex_api::Refusal,
> {
    let routed = hex_runtime::guest::WithGuest::configured(process, guest, route);
    let node = std::sync::Arc::new(
        hex_node::recorded::Recorded::open_with_prober(routed, state, prober)?
    );
    node.bind_guest_books()?;
    // This does not boot the shared CPU guest; it validates dedicated admission
    // and exact GPU correspondence using the sole provider/Store authority.
    hex_api::Runtime::capacity(node.as_ref(), hex_api::Class::Shards)?
        .ok_or_else(|| hex_api::Refusal::Broken("Shards cost/pool is not initialized".into()))?;
    node.recover()?;
    Ok(node)
}
```

macOS passes guest::by_class. Windows passes WinRuntime::oci_side, preserving its
native HCS OCI route; WithGuest::route always sends exact ClassSpec::Shards to the
guest. Both register the same existing Arc owner, then derive Weak<dyn Books> via
bind_guest_books; no new allocation is needed for that trait view. SAME Prober is
supplied to Service where it runs in process; a split service uses the actual native
physical_report RPC, never opens another Gpus or fabricates load/holder metadata.

Public guest APIs required by registration/forwarding are initialize_gpu(),
attachment_offers(), validate_binding(&BoxSpec), recover_managed(&BoxSpec),
evented_for(&BoxId) and program_client_for(&BoxId). Scoped client functions never
enter Books again because their forwarding caller owns that canonical method turn.
Sync methods attach/install a missing route within that same Tx; ready recovery's
alive()->inspect() reaches that path before adopt's read-only validation. Do not
add another reentrant Recorded hold around an owns_record=true guest operation.

Remaining outside-lane/source integration limits are explicit:

- Windows152 stores its administrator Catalog by VALUE, not an Arc. A prior L2
  getter suggestion assumed an Arc and was corrected. ROOT/L3 must share that SAME
  initialized immutable catalog/owner view with the guest (a getter cloning a
  nonexistent Arc is not an implementation). Do not load a second catalog/holder.
  Linux Node already exposes its existing shared catalog. Mac has no native catalog;
  guest-only configuration must be loaded once by its initialized CLI authority.
- Books::capacity must remain native facts only. In L3native152 its capacity calls
  runtime.attachment_offers(), which includes the guest route; the native-only
  capacity/provider splice needs L3 review before readiness. L2 attachment_offers
  itself calls ONLY Books.physical_devices/records, never capacity. No native call
  or reproduced behavior defect is asserted by this source observation.
- CPU-only explicit Shards boxes are dedicated and pay the full original allocation.
  Ordinary CPU OCI remains the shared guest path. Its fixed shared backing admission
  in the existing runtime-pool/Index entry is still open; no system BoxSpec/second
  ledger/per-inner-box VM charge is supplied here. Do not use legacy new/routing's
  eager boot as an unreserved workaround. Legacy GPU OCI also needs effective route
  cost known before candidate admission; late Invalid is not a resource-accounting
  completion claim.
- Managed helper inheritance (old Mac setsid paths), actual native scope empty and
  handle proof, Linux native launch, Mac native launch, Windows ARM64 full renderer,
  full VMM/link/guest/native effects and ROOT gates remain owed. Process exited or
  wait error never releases physical claims. Released identity+exact status is
  synced before same-native-owner finalization, canonical remove and final receipt
  unlink. Both uncertain/native finalization failures retain the authoritative row.
