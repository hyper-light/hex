# Fallible client link and connection owners

Signed source `9b242628f9d026ba27beef39010b63c02a058889` has exact parent
`d1ed4e1cf54d8d0a22da931d55832e4b38b202c9`. It changes the nine admitted
`hex-client` source/test files only. The source manifest and protected-region
hashes are in `client-link-owner-source.json`. This checkpoint is uncompiled.

The full current Shards `0a6d91b3` policy and exact-parent Hex CLAUDE were read;
ROOT acknowledged the implementer readback and released this scope, including
the existing Conn allocation and admitted client test-constructor ports. ROOT
owns Cargo metadata, outside test ports, joined checks and native validation.

## Frozen public interface

```rust
pub type LinkOwner = triomphe::Arc<dyn Link>;
pub fn own_link<L: Link>(link: L) -> Result<LinkOwner, hex_api::Refusal>;

// On hex_client::guest::Client:
pub fn connect(link: LinkOwner) -> Result<Self, hex_api::Refusal>;
pub fn connect_evented(
    link: LinkOwner,
    wants_room: std::sync::Arc<dyn Fn() + Send + Sync>,
) -> Result<(Self, Pump), hex_api::Refusal>;
```

`Link` already requires `'static`. `own_link` uses `triomphe::Arc::try_new`, then
the same safe `CoerceUnsize`/`Coercion!` pattern as the current SDK's
`ManagedReceipt`. Coercion consumes the typed reference while preserving its
allocation; it does not add a forwarding Link or a second state/holder. The
constructor calls no Link method. Callers must construct the owner before their
actual endpoint/guest effects and propagate refusal. L1/L2 received this frozen
shape before consumer ports and then received the exact signed source pin.

The owner is retained directly by Stream and passed through connect, evented
connect and the test-only dial helper. The event callback remains a standard
Arc. Books/Weak, Guest/ManagedReceipt and physical GPU holder authority are
outside this change and remain unchanged.

## Connection ownership and failure

The existing Shared object is allocated by
`Shared::try_new(T) -> Result<SharedOwner<T>, Refusal>`, where SharedOwner is a
TriArc of the same Shared object. Conn retains that owner; Pump retains a clone
of it. Core, Deferred and RemoteProc share the existing Conn object through its
fallibly allocated TriArc. No connection state, reader, send authority or native
owner is copied. Conn's existing last-drop shutdown behavior is unchanged.

The three allocation failures use static `Refusal::Exhausted` labels:

| Allocation | `what` | `bound` derivation |
| --- | --- | --- |
| Link | `guest link owner` | `size_of::<L>()` |
| Shared | `client shared state owner` | `size_of::<Shared<T>>()` |
| Conn | `client connection owner` | `size_of::<Conn<T>>()` |

The diagnostic size is the requested value's inline payload size, not allocator
overhead or a host-resource credit. It introduces no cap or tuning constant.
Errors propagate before Hello is sent; owned values follow their existing drop
paths on failure. This source-only review does not prove allocation-fault cleanup
or make all other existing buffer/queue allocations fallible. Transport reading
initialization and unrelated standard Arc uses remain as before.

No reply budget, calibration/index table, protocol framing, named-stream setup,
send lock, cancellation, event-loop or workload timer behavior changes. Protected
section hashes establish exact source equality for the affected protocol paths;
they are not runtime or performance evidence.

## ROOT metadata and outside ports

The only dependency request is the already locked Triomphe `0.1.17` with its
existing unsize feature, plus already locked unsize `1.1.0`. For example:

```toml
# Workspace dependencies: existing triomphe pin is retained.
unsize = "=1.1.0"

# crates/hex-client/Cargo.toml [dependencies]
triomphe = { workspace = true, features = ["unsize"] }
unsize.workspace = true
```

ROOT owns those metadata/lock direct-edge changes. No dependency version,
checksum, framework, metadata file or private compile overlay was changed here.
The local primary Triomphe source and exact SDK coercion source are recorded in
the manifest. ROOT must join the dependency edges and matching L1/L2 production
consumers before compilation. Outside node relay and CLI guest-tree/node-socket
test Link constructors remain ROOT's ports; this patch does not edit them.

Inside the admitted client tests, Shared/Conn constructors now use the fallible
seams. Link test helpers use own_link. Where type erasure would hide concrete
test counters, those same dial/deadline observations are retained in standard
Arc counters outside the Link trait object. Existing budget values, observer
events, cleanup paths and assertions retain their prior meanings.

One new pure control is source-only:
`guest::tests::link_owner_retains_one_instance_without_dial_effects`. It observes
zero dial effects at construction, pointer identity across clones, dynamic
dispatch to the retained Link, and one drop after the last reference. It starts
no socket, thread, guest or native process. Allocation-failure injection and
actual endpoint/guest cleanup remain unverified and ROOT-owned.

Scoped formatting and whitespace review returned zero. Tests, Cargo/Clippy,
native/API/VM/GPU invocations: **zero**. No old deadline, EOF3, artifact6 or other
passed batch was repeated. No native acceptance or full-gate claim is made.
