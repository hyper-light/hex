Trusted-entry shared typed fixture builder — source only

ROOT released L3 b467's measurement-only source partition. This is the ONE L2
builder for its parent and SAME Hex private entry, not a new runtime, manifest,
process owner, native protocol or permission to resume the foreign image.

The exact shared signature is:

```
pub(crate) struct Input<'a> {
    pub config: &'a GuestConfig,
    pub cost_file: &'a Path,
    pub cost_sha256: [u8; 32],
    pub evidence: &'a Path,
    pub paths: &'a RuntimePaths,
    pub socket: &'a Path,
    pub console_kept: usize,
}
pub(crate) fn prepare(Input<'_>) -> Result<(PreparedBoot, Allocation), Refusal>;
```

guest/native_fixture.rs is gated by cfg(any(test,
feature="native-trusted-entry-measurement")). Each freshly executed image
independently calls the SAME builder with ROOT's fixed owned inputs. No Rust
pointer, borrowed NativeStartup or attribute pointer survives a process exec.
L3 owns the reviewed fixed input carriage, native adapter and measurement leaf.

The builder calls the real load_runtime_inputs, requires the optional native
image declarations and matches their evidence identity with the original cost.
It uses the SAME factored CPU/memory floor/ceiling/configuration and allocation
check as GuestRuntime::with_runtime_cost. There is no second cost formula or
Index grant; the returned allocation is planned data, not admission authority.
It builds GuestSpec fallibly and calls real prepare_native_boot/native_startup.
No Runtime, Prober, Node/Recorded, VM, process or connection is initialized.

The command-line segment traversal is shared by the existing formatter and new
fallible construction: exact `console=ttyS0 panic=-1`, then ordered ro/rw share
modes only when roots exist. New spec storage is counted/reserved before copying;
paths/command line/ports/ordered shares have no infallible clone or formatting
fallback. Shares retain the existing virtio-slot bound. Allocation refusal
reports the actual requested representation/count bound, not a guessed new cap.
Ordinary spec() and its public signature remain available; no optimization,
allocation-free or performance result is claimed for the refactor.

ROOT registration (no manifest edited by L2):

```
// guest/mod.rs
#[cfg(any(test, feature = "native-trusted-entry-measurement"))]
pub(crate) mod native_fixture;

# hex-runtime/Cargo.toml, existing feature section (or create it)
[features]
native-trusted-entry-measurement = []
```

ROOT owns the corresponding CLI feature/entry forwarding; early dispatch must
still precede ordinary argv collection and preserve ordinary routes. This source
does not make Rust/dyld pre-main initialization single-thread safe or prove which
image a later posix_spawn(path) executed. Caller verification retains the SAME
open CLI image via NativeStartup::verify_cli with the original absolute budget.
That file-content witness is not an exec seal. Kernel/VMM declarations do not
authorize their execution in the held-CLI-only measurement.

Two new controls are UNRUN: fixed_native_fixture_uses_real_evidence_and_refuses_
legacy_or_wrong_cost and fallible_fixture_spec_preserves_existing_kernel_and_
ordered_share_grammar. Model artifact bytes/profile are explicit, not actual
native cost/image evidence. The control creates no socket, provider or process.
It checks the real typed accessor/allocation/evidence/kept0/ordered rights, rejects
legacy and wrong configured cost, and checks exact kernel grammar for zero/mixed
roots plus the existing share bound. ROOT owns compilation and any actual probe;
no old passing producer controls were replayed or new job dispatched by L2.

Actual ROOT80e3's separate producer SDK3+Hex2 were5/0/0, standalone SDK strict0
and runtime/node/CLI all-target strict0. Its remaining temp entry is exactly one
EMPTY temp/shards-tests directory, files0; this is not literal temp0 or leaked
payload. Original786 workspace/reader setup failures remain separate. Root's
Windows strict expression correction511f is source-only until its matched result;
no new Mac phase/Windows native custody is inferred here.

Policy7d7e0855a5073ed5163b4fe7ed3355c9ed7c2506 / rules
df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3,
implementer; ACK and file ownership persist. Local scoped rustfmt/parser,
diff/source checks only. No native/FFI/worker/codec/tag/check dispatch, shared
manifest, provider/cache/ledger or ROOT main merge change. Actual held-image,
pre-main, native cleanup/custody, fixed workers and whole-scope/final-helper proof
remain ROOT/L3 obligations.
