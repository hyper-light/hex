# CDI parser/selection light evidence

Role implementer Kiln01a1c640e6ed710d8a8c919b4aa1fc8c, coordinator Hex
01a1aa77778471339bea3beff90d6bbd. Full policy/API/source readback:
[READBACK.md](READBACK.md). Shards policy cbd39284937d024640c56db6f8d8ef2bd9b0ee24,
rules SHA256 df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.
Fixed Hex base37021c35e64b2a8bab1d35f20f92a0042d874a44; source-first signed
checkpoint9cb5953cd84ace541a4cead1269a71ef9d079c30. That checkpoint was accurately
reported SOURCE ONLY/0 executed tests before this separate private check.

## Exact interface and scope

```rust
Spec::parse(bytes: &[u8], max_bytes: usize) -> Result<Spec, Error>
Spec::select<'s>(&'s self, qualified: &[&str], max_selected: usize)
    -> Result<Edits<'s>, Error>
```

Spec owns validated data; Edits borrows complete env/device-node/mount/hook/network/
RDT data and owns its reference lists and copied additional GIDs. Returned errors
carry only Field, optional device/item indexes and Cause (including limits and
allocation refusal). Debug for exposed payload types/Spec/Edits is redacted.

This is pure data parsing and selection, not device authority or OCI application.
No file loading/registry/cache/host stat, hook execution, mounts, node creation,
network changes, shared BoxSpec/Runtime APIs, installed helpers, hardware or VM
operations were implemented. ROOT owns those decisions/adapters. Sources are the
pinned CDI1.1 spec and Go selector/validator; [sources.json](sources.json) records
exact revisions/hashes. No new dependencies or parent registration were committed.

## Actual v1/r15 result

**Build0; exactly8 selected,8 passed,0 failed,0 ignored.** Every command used the
listed exact test name, recorded running1/passed1 (not a zero-filter success),
and recorded/reaped its owned Popen child. PIDs18623–18630 all waited successfully;
none was killed. [Summary](raw-v1/proof/summary.json) contains all commands,
individual source/log/binary identities and ownership facts.

Production `cargo clippy -p hex-core --lib --locked --offline --no-deps -- -D warnings`
exit0; corresponding `--lib --tests` exit0; scoped Rust1.99.1 rustfmt check0.
No full workspace/seven-target/native-device campaign was run. The pure suite
exercises positive exact-data/non-vacuity cases and explicit negative cases; there
was no initial red at this source, production bug claim or guard mutation.

Runner ONLY d1323898b255a3ed435eb83fa0f7b0cf, admitted cpus=1,mem=4g,disk=12g,
`nice -n 10`, Cargo jobs1/testthreads1. Environment reports actual niceness20
(the runner inherited10), arm64 macOS26.4, rustc1.99.1 58cf8f9c3,
Cargo1.99.1 5f94df478. No laptop builds/tests/benches occurred.

Private root on runner:
`~/.cache/hex-cdi-01a1c640-9cb5953c-v1/{source,target,cargo,proof,retained}`.
Source came from the exact signed source checkpoint via precise CLI --no-sync
stdin. NEW private Cargo registry copy contains no credential/config copy; new
private target/source paths do not reuse any ROOT/relay source or target.
The source archive was6479055B SHA256
`932c4f358ccc7d0a436b242f89760356487d3e593cddf6a22e1e2b994a801c1d`.

Only private `hex-core/src/lib.rs` appended `pub mod cdi;`:
before SHA256 `040e51eb13da41c9d4a7bfb4bf94e18065ba3b6c491edd2ad58e39e2763035a1`,
after `bd43106b1bc3be61e84ff5ee37fa909cc2d8c2e8a3068d211768ca9a1214d4f9`.
Shared/parent files in the supplied worktree are untouched; ROOT must register the
module when composing. The checked source files remained byte-identical through
build, eight tests, both Clippy checks and formatting.

Source SHA256 `7ca46313f6c581c4194a9ba4a295cdda11435a285aacca8b304f41e77b6ce740`.
Tests SHA256 `ec66c85d14110a3c1aae9304f5ff6d095b92197552e70b6c4258cabd12ed5d30`.
Native test binary SHA256
`36180025962547a46e5176853409e6e2e7a8c3249af3bade879323f03ec74653`.

## Retained proof

[Raw archive](proof-v1.tar.gz):26132B,38 regular members, SHA256
`943060ae97ce3b3c20b8c67d6a861bfc0ede0a5791b09359f96dd48efbac9ee0`.
[Receipt](proof-receipt.json) verifies safe/unique members, every member hash and
source/test equality to this worktree. It retains original build output and exact
private sources, parent registration, commands, logs and summary. Human-readable
proof files live under raw-v1/proof. The exact [check driver](light-v1/driver.py)
and [collection code](collect-proof.py) are retained; replay requires NEW private
paths, not overwriting the frozen proof. Native binary/source/target/cache remain
on the runner. No archive/base64 contents were printed to UI.

| Executed test/log | SHA256 |
| --- | --- |
| [caller_bounds_are_exact_and_selection_failure_is_atomic](raw-v1/proof/test-0.log) | `cd1a07c8f3748b50ed4e992b980a8ebc10b701be891a5e5364c9a91a2ae26a48` |
| [edit_validation_does_not_invent_cross_object_conflicts](raw-v1/proof/test-1.log) | `c321b0aecb365765266b03097adacd1da1c102a4ea80a620c94e4194f356ae71` |
| [private_content_is_absent_from_error_and_debug_ergonomics](raw-v1/proof/test-2.log) | `f0b2b5cd3ee9c31db6044fc4d823576329e48044d1a8c6807aace71ea663b207` |
| [qualified_grammar_membership_and_duplicates_are_checked](raw-v1/proof/test-3.log) | `05610cae5d07ff907fa0bd6bcfd239d2d9c485eb2a5981253bc568dcf56875e4` |
| [request_order_globals_once_and_last_rdt_match_go_append](raw-v1/proof/test-4.log) | `49e7e4ea27a576ba8de0b59b73528ff8d562688e011831a1825f34b28bb56889` |
| [selection_retains_all_edits_and_only_requested_devices](raw-v1/proof/test-5.log) | `ba53fb899706f2a2ef4e2a9498d115563b3e867f370c3c44b1b8d7e986adcd78` |
| [unknown_duplicate_and_wrong_json_types_are_typed](raw-v1/proof/test-6.log) | `cfad071644886451ce28478952ffa6680d3ff71a00c2716733b57e5c2b45ee82` |
| [versions_and_minimum_feature_gates_match_the_pinned_source](raw-v1/proof/test-7.log) | `f5b4718f478c096697c67ac15e0dc90ffa8de4b8ee44895066aa061c4301f858` |

The eight cases establish complete selected field retention, globals once/request
order/whole last-RDT replacement; empty-selection no global edits; exact caller
bounds/atomic selection error; unknown/duplicate/malformed JSON refusal; qualified
name grammar/membership/duplicates; released-version feature gates; per-object
validation preserving Go Append conflicts; and private diagnostics redaction.
All source comparisons are against pinned Go code, not an executed Go oracle.
Runtime/OCI Apply semantics (last env/path wins, mount ordering, host stat/default
fill, zero-GID omission) remain deferred; selection retains their ordered input.

## R01–R16 / limits and owed ROOT

- R01/R04: no explicit production panic/index/unchecked arithmetic path, production
  and test Clippy0; typed decode/semantic/resource errors, atomic selection. Own
  retained strings, lists and result lists reserve fallibly. serde_json's internal
  decoding scratch/error allocations have not been fault-tested; no universal
  allocation-refusal/stack/panic-freedom proof is claimed. Fixed schema contains no
  self-recursive data types; every collection is input-byte bounded. ROOT owns any
  actual native allocation-refusal validation before stronger claims.
- R02/R03/R05/R12: the existing standard/Go implementation defines the data/merge
  contract. No architectural/device-exposure choice beyond ROOT's scope, no cache,
  waits, native adapter or new manager/dependency. Input and selected bounds are
  caller supplied; the only filename literal4096 is pinned CDI's RDT validator
  bound. Schema tags, supported versions and enum alternatives are protocol data.
- R06/R07/R13: one private fixed-schema decode bridge keeps serde member errors
  typed without inspecting/formatting message text and reserves owned strings
  fallibly. It exports no general JSON framework or shared manager. Two public
  operations hide validation/selection; scoped source formatting/whitespace0.
- R08/R14: no optimization/competitive/performance/leak-free claim and no profiling
  or benchmark. Borrowed selection avoids payload clones by source construction;
  this is not a measured allocation/cost result. ROOT owns all such evidence.
- R09/R10: new functionality, not a speculative production fix. Source-first status
  was explicit; exact executed unit/check logs, hashes, raw bytes and limitations
  are retained. Native effects/meaningful mutations were neither assigned nor run.
- R11: pure platform-neutral data only; native arm64 Mac harness execution proven,
  other supported targets and native hardware behavior unverified/ROOT owed.
- R15/R16: only assigned NEW files/evidence changed, no children/goals/plans,
  native campaign/VM/SSH/admin/other-tenant operations or merges/pushes. Prior20268
  relay branch/raw and all other snapshots/targets/jobs remain untouched. Last
  recorded local free33999504KiB exceeds30GiB. No deletion/reclamation occurred.

Cleanup: eight test children reaped. The codec created no device/guest/SSH/helper
process, socket, registry file or OS/device resource; parsed owned memory dropped. Completed private
build/source/cache/binary/proof paths are retained for review. The codec gives no
native device exposure/authority guarantee. ROOT owes source/interface review,
parent registration and combined checks, actual native device/OCI integration,
all platform/full gates, allocation/fault evidence, and any mutation/audit/profile/
benchmark work. One checkpoint does not complete the live device bridge or lane.
