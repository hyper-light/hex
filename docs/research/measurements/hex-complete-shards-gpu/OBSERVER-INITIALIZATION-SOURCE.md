Optional observer initialization — owned source, 2026-11-11

SOURCE ONLY / UNCOMPILED / THREE NEW CONTROLS UNRUN. No native, build, old-test
replay, CLI or manifest edit. This closes the owned input producer required by
ROOT's configured_guest; registration remains ROOT's separate source integration.
Hashing configuration/artifacts is not a GPU result or measurement attestation.

Public input contract

GpuObserverInput::from_env() -> Result<Option<GpuObserverInput>,Refusal> reads
only HEX_GUEST_GPU_OBSERVER_FILE and HEX_GUEST_GPU_OBSERVER_SHA256. Neither
provided returns None. A partial pair, non-UTF8/nonhex hash or invalid input
refuses. from_vars has the identical injectable two-variable contract; it changes
no process environment. GpuObserverInput::load(&Path,[u8;32]) reads the explicit
absolute regular file pinned by that exact SHA-256, then decodes existing typed
input. It neither discovers hardware nor starts any observer/guest process.

JSON is one object with exactly these required fields, in any order:

| Field | Value |
| --- | --- |
| evidence | 64 ASCII hex digits identifying ROOT's reviewed complete execution/build/ABI/loader/context evidence |
| artifacts | ordered array of artifact objects below |
| launch | ordered array of explicit UTF8 executable-prefix strings; no PATH lookup |
| max_queue_families | unsigned u32 |
| max_allocation_bytes | unsigned native usize |
| stderr_bytes | unsigned native usize |
| bounds | explicit existing Bounds object below |

Each artifact has exactly role, host, guest, bytes, sha256. Role is program,
interpreter, loader, icd, driver or dependency. Host and guest are UTF8 path
strings. bytes is unsigned u64; sha256 is exactly64 ASCII hex digits. Both hex
cases are accepted using fixed stack normalization and the existing checked
observer hex-array decoder; plus signs/non-ASCII/width changes refuse. No UUID,
model, ordinal, native selector, runtime default or alternate provider is input.

Bounds uses the existing eight names cpu_weight, cpu_max_milli, memory_max,
memory_high, io_weight, io_latency_us, io_max, pids_max. All eight must appear
explicitly. Each scalar is its existing unsigned integer width or null. io_max
is null or the existing four-field IoRates object: read_bps, write_bps, read_iops,
write_iops, each explicitly unsigned u64 or null. The parser constructs only this
existing type; existing Bounds::check and real GpuObserver::new decide validity,
including mandatory positive CPU/memory/process limits. Null never supplies a
plan, profile or measured allowance. Float/exponent/negative values refuse for
these unsigned fields, as do duplicates, missing or unknown fields at every level.

JSON syntax/ownership and bounds

The existing bundle MAX_CONFIG_BYTES is1MiB. The file must be regular before and
after opening. Unix uses existing safe rustix NOFOLLOW|NONBLOCK|CLOEXEC read
ownership; Windows opens the reparse point itself and refuses that type. A
fallibly allocated size+1 buffer detects truncation/growth, checks the opened
file's metadata identity before/after, and verifies its complete exact SHA before
parsing. No read_to_end, unbounded buffer, executable or manifest dependency is
added. All handles remain lexical file owners. Existing artifact verification
remains the existing constructor's separate same-file identity/size/hash check.

The private decoder reads only this schema's objects/arrays, strings, unsigned
integers and null. It has no generic JSON Value, callback walker or recursive
value/skip routine. Maximum structural nesting is three: root/artifacts/one
artifact, or root/bounds/io_max. Unknown nested input refuses rather than walking
it. Cursor loops consume finite input bytes; each array has at most N logical
elements for the N-byte verified file, with checked count*sizeof(element) and
fallible Vec growth before push. The real constructor additionally applies its
existing artifact-count/hash-encoding and output/allocation bounds.

A string reserves at most its raw quoted byte length before decoding. JSON
escapes and real Unicode, including checked UTF16 surrogate pairs, are supported.
Decoded UTF8 cannot exceed that raw length. Paths use fallibly reserved native
PathBuf storage; no fixed ASCII-only filename constraint or unsafe conversion is
introduced. Control/escape/surrogate/UTF8 errors refuse. Only RFC8259 whitespace
is ignored; trailing bytes/commas or unrelated value types refuse. Unicode escape
widths/surrogate ranges come from RFC8259 sections7–8; SHA widths are its native
256-bit encoding. Bounds masks are the eight/four existing schema fields, not
resource or tuning constants. Allocator rounding/total resident cost is not a
profiled or allocation-free claim.

The existing schema-specific constructor is unchanged. It checks evidence and
positive explicit inputs, duplicate/missing artifact roles, exact read-only host
to guest mappings, real file size/digest/identity and the same paid allocation
payload room, then builds its original immutable observer identity. A hash alone
does not prove the declared artifact closure is complete or the supplied bounds
were measured; ROOT/L4 still supply/review those actual inputs. Execution still
requires the already-charged canonical Pool/selected VM scope and private renderer
report/readiness path. No external unreserved GPU query is introduced.

Exact ROOT CLI port, outside this source

Add the two exported observer ENV constants to guest_configuration's existing
requested-variable list, so observer-only partial configuration cannot be ignored.
After the existing .with_runtime_cost(cost)? and before canonical adapter
registration/preparation/recovery, use the same GuestRuntime value:

```rust
let mut guest = /* existing configured_guest construction through cost */;
if let Some(input) = hex_runtime::guest::GpuObserverInput::from_env().map_err(failed)? {
    guest = guest.with_gpu_observer(input).map_err(failed)?;
}
Ok(Some(guest))
```

The existing configured probe/boot path pair, SharedCatalog, SAME Books owner,
whole-owner retained preflight, metadata-only prepare, Index grant-before-boot
and native recovery order stay intact. Do not sample capacity or boot/query a
GPU merely to load this input. No cost profile, UUID map or selected set is
rewritten. Optional None retains existing CPU behavior.

Three new source controls

- optional_observer_manifest_reaches_real_constructor_with_escaped_unicode_paths:
  actual model artifact files under Unicode directories, escaped slashes and
  surrogate pairs load through both env variables and the unchanged constructor;
  immutable identity/Program argv+env match; absent/partial pairs and all standard
  string escapes are covered. These files are hash/mapping models, not binaries.
- observer_manifest_rejects_hostile_schema_lengths_escapes_and_partial_inputs:
  missing/duplicate/unknown/nested fields, numeric overflow, malformed Unicode,
  invalid UTF8, wrong digest/path/type, overbound file and Unix symlink refuse.
- loaded_observer_manifest_keeps_actual_artifact_and_paid_room_checks: valid
  pinned JSON cannot bypass writable-map, absent limits, unpaid room or changed
  actual artifact bytes. No VM, renderer, provider or native process is invoked.

Primary specification: https://www.rfc-editor.org/rfc/rfc8259.html sections2,4,6–9.
Existing config.rs is the sole artifact/bounds/Program implementation; this
loader reuses its types, parent fallible refusal and checked hex helpers. Generic
serde Value ownership was rejected for this new R01 path; serde is test-only here.
No claim about a reproduced serde defect or broad library audit is made.

R01–R16: new owned strings/Vec/paths reserve fallibly, numeric widths/arithmetic
are checked, lifecycle/authority and waits are unchanged; primary grammar and
derived bounds are recorded above. Scoped rustfmt/parser+diff checks only. All
compiler/strict/new control execution, meaningful mutation, native closure, actual
loader/ICD/driver/guest GPU compute, measured resources, supported VM effects and
performance remain ROOT-owned/unverified. Source reuses the pre-existing Refusal
OS factory and constructor/Bounds checks; it is not an audit claim that all those
existing allocation/error seams are panic-free. Current separate family/layout
checks and earlier green sources are not this new loader's test result.
