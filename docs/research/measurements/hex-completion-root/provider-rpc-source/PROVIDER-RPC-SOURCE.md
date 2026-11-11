# One native provider and split scheduler RPC consumer

AC-T7.4.1; L1 implementer. SOURCE ONLY for this increment. ROOT31fe8ab8's
paired Mac/Linux runtime/node lib checks passed on their exact earlier source,
not this new consumer. The earlier actual original unread seam red0/1 and narrow
quality green1/0 have their own frozen696+2ce function-overlay attribution; they
do not prove this complete Service integration.

Public constructor for ROOT CLI:

```rust
facts::shared_prober(state: &Path, declared: Declared)
    -> Result<facts::SharedProber, Refusal>
```

SharedProber is triomphe::Arc<Mutex<Prober>>. The constructor fallibly prepares
the path and allocates the one typed owner before calling Gpus::open. Its
temporary uninitialized provider slot is uniquely owned and never published;
the finished owner is supplied to Node/Recorded, cloned with inferred .clone().
There are no Weak references or additional Prober/Gpus allocations in that
sharing path. Native initialization itself is still ROOT's unrun native effect
and R01/R14/vendor-call timing review. Standalone Prober::open remains for
shell/facts consumers; an initialized no-GPU provider still produces its stable
authority plus empty report and retains its source diagnostic.

Service::open remains the existing two-argument (config, report) API. Service
contains no Prober, SharedProber or Gpus. Both initialization and refresh obtain
CPU/memory/disk/declared-machine facts without GPU discovery, and query the actual
native Runtime::physical_report through the existing client/wire bridge.
Native RPCs finish before cluster/model locks. The one immutable native
Store/turn authority is pinned for the Service lifetime; changed or contradictory
authorities refuse rather than inheriting existing claims. This identity pin is
not an inventory or telemetry cache. Resource pool ids may name narrower guest
envelopes and are not equated with native physical authority.

reported_gpus checks the actual flat native DTO, preserves its stable opaque key,
model/memory and independent optional counters, and leaves unreported vendor,
driver and PCI presentation text absent. No ordinal, label or guessed PCI parser
creates an identity. The existing attachment-capacity report and sole Store
holder authority still gate every offer; telemetry alone grants no route.

Unknown counters mark fresh GPU room unavailable and the single model matcher
requires both readings. Missing native inventory retains configured physical
slots as explicitly unavailable placeholders, with unknown load; it cannot turn
catalog metadata into measured capacity. Exact existing bindings remain held and
CPU-only device admission progresses. The new bounded pure RPC-facts/offers
control exercises unknown and empty native reports; it is UNRUN at this source
checkpoint. Existing metadata-mismatch refusals remain when a current native
observation exists but disagrees with the native route report.

Exact six-source manifest is `provider-rpc-source.json`. ROOT must consume the
listed placement/physical.rs leaf too (it predates this commit on L1 but is not
in ROOT31f). That leaf coalesces overlapping routes, requires one native owner,
preserves independent unknowns and observed over-total pressure, and never
assigns a holder.

Metadata: hex-node needs its normal triomphe dependency, already present in
ROOT31f. This increment adds no version, build dependency or seed dependency.
hex-runtime's normal triomphe edge for SharedCatalog is also already in31f.
L3 NativeScope uses the separately ROOT-owned normal hex-node getrandom0.4.3 edge;
it is not a Service/provider dependency. Root's exact fallible BoxId constructor
410a910a is for the next native scope name. The old private retained-unit failure
was a missing locked dev crate autocfg1.5.1, not a production manifest gap; no
toolchain/runner installation or credential/service change was attempted.

Scoped rustfmt/diff checks only. Full source compile, real CLI reachability,
provider effects, fault/mutation, seven-target behavior, complete resource/CPU
progress and allocation/performance proof remain required ROOT gates. No static
Unsupported, empty report or source-only capability is completion.
