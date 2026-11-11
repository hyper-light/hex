# Native VM receipt ownership (AC-T7.4.1)

Hex ROOT coordinator 01a1aa77778471339bea3beff90d6bbd; Shards policy
ca3ab10c151aa5539e757f1cc76029746ccbb561, rules SHA256
df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.
This is the source decision for the active implementation, not a completed
native, allocation-fault, performance or platform result.

## One owner, allocated before effects

The canonical native slot owns one typed process/scope receipt. Allocate it with
the existing triomphe 0.1.17 `Arc::try_new` before creating a process, native Job,
cgroup or guest. Allocation refusal leaves those effects absent. The SDK receives
a reference to that same owner as `ManagedReceipt = triomphe::Arc<dyn
ManagedProcess>`; it does not allocate a wrapper, a second process record or an
independent lifecycle manager. A compiler-checked unsizing conversion consumes
the typed reference, preserving its allocation and ownership. Native finalization
still compares the canonical typed owner and its original identity.

Stable std `Arc::try_new` is unavailable on the pinned toolchain. An unchecked
std `Arc::new` after native launch can abort and lose the pre-publication cleanup
path. A separate std wrapper around an already fallible owner would retain that
extra allocation. A custom unsafe reference count/type-erasure implementation
would duplicate an established dependency's ownership machinery. The selected
alternative enables triomphe's existing `unsize` feature and uses the safe
`unsize::Coercion!` macro for the single dynamic-trait conversion.

Primary implementation sources are [triomphe 0.1.17 Arc](https://docs.rs/triomphe/0.1.17/src/triomphe/arc.rs.html)
(`try_new`, `CoerciblePtr`, `clone`) and [unsize 1.1.0 Coercion](https://docs.rs/unsize/1.1.0/unsize/struct.Coercion.html).
ROOT read the exact cached triomphe source and the published unsize crate source.
The unsize crate is 8,060 bytes, SHA256
4fa7a7a734c1a5664a662ddcea0b6c9472a21da8888c957c7f1eaa09dba7a939;
its `Coercion!` macro supplies the compiler's ordinary pointer unsizing. The
dependency has no additional runtime service or installed binary. Exact manifests,
lock tables and the reviewed API source belong in the vendor intake receipt.

## Release and recovery

Controller exit is distinct from complete native-scope release. A surviving
helper, failed observer or uncertain cancellation keeps the canonical record and
device reservation. On a positive whole-scope result, persist
`Released { identity: Option<Identity>, status: ManagedStatus }` in the same
selected-VM boot receipt before native finalization. Then finalize the matching
native owner, remove the canonical record and finally remove that boot receipt.
The optional identity represents a scope that failed before controller
publication; it is never replaced with pid/start zero. Recovery of durable
Released must not stop a name-reused foreign scope.

The API's worker references keep observation/output state alive; API lifetime
owners determine last-user stop. Readiness waits subscribe to the existing native
receipt. No repeated process probe, new holder ledger or receipt cache is added.
The existing cgroup/Job/native receipt remains the scope authority.

## Evidence still required

Source inspection establishes the intended allocation/unsizing path only.
Targeted API controls, allocation-fault injection before effects, identity and
owner guard mutations, native controller/helper death, durable-release crash
points, cleanup/recovery and all supported adapters remain coordinator validation.
Triomphe also has an abort guard above its `isize::MAX` reference-count limit;
the native/worker/API reference bounds and checked public cloning require their
own review. No general panic-free, allocation-free, leak-free or speed claim
follows from choosing a fallible initial allocation. R08/R14 comparative
profiles and benchmarks remain unrun for this implementation.

The upstream shared-manifest ownership request is pending. ROOT owns the minimal
dependency overlay in the isolated Hex vendor candidate; that overlay neither
edits the peer coordinator's checkout nor merges into upstream main.
