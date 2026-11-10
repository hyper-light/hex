# CDI OCI renderer: scoped implementation and light evidence

Final tested production source: `2a059b5c56f9561a225c8172c808679c269e8ea1` on
`cp/t7-cdi-oci-render`, from exact `d4d96083c83f88ff3e9b9447598a3b95f461b680`.
Preserved tested CDI parser branch/source d4d/9cb and original relay20268/raw.
The later evidence checkpoint changes docs only. This is a pure renderer result,
not actual device attachment/enforcement or completion of ROOT's live-device work.

Full policy/readback, exact typed interface, pinned source decisions and limits:
[RENDER-READBACK.md](RENDER-READBACK.md). Scope authority remains ROOT's explicit
implementation/light-only assignment; all actual native/heavy/audits/mutations/
benchmark/profile/full gates and final merges remain ROOT-owned. No children,
goals, plans or automatic dispatch were used.

## Exact API and native boundary

```rust
pub fn render_with_devices(
    spec: &OciSpec,
    at: &Place<'_>,
    edits: &crate::cdi::Edits<'_>,
    resolved: &[ResolvedNode<'_>],
) -> Result<Bundle, Refusal>;

pub struct ResolvedNode<'a> {
    pub node: &'a crate::cdi::DeviceNode,
    pub kind: crate::cdi::DeviceType,
    pub major: u32,
    pub minor: u32,
    pub file_mode: u32,
}
```

`bundle::ResolvedNode` is re-exported by the owned bundle module. It identifies
the EXACT selected borrowed node in order; cloned/reordered objects refuse.
The shell owns actual host stat and its authority/race handling. This renderer
performs no lookup, mount, hook execution, registry, IO or OS adaptation. A receipt
is required even for fully specified nodes; Go's absent-stat fallback is not
represented by this interface. Receipt kind is Block/Char/Fifo, numbers are
unsigned stat-derived unix Major/Minor, and mode is Linux stat without file-type
bits. Debug is redacted. Explicit CDI fields, including zero IDs/mode, retain
precedence; missing UID/GID inherit nonzero process IDs per pinned Apply.

Ordinary render delegates to this same typed Config builder with empty edits.
There is no second production renderer/JSON Value tree/cache/registry. Added
nodes replace by path; b/c grants append exact major/minor/access and retain prior
grants. Mount removal deletes the first exact destination, then global stable
sorting uses Linux-cleaned depth on every host. All hook phases/arguments/env/
timeout are represented. Additional GIDs skip0 and deduplicate. The pinned
runtime-tools initial-env quirk is deliberately retained and tested, not silently
corrected; see readback/source links. Selected netDevices/intelRdt return typed
Platform before output because native enforcement is unavailable.

The existing Refusal boundary has String presentation. New failures bridge with
fixed field/index/cause only (Invalid/Platform/Exhausted), never private values
or raw serde error text; no string-matching control or shared API edits. Added
input operations/bytes and encoded length use existing MAX_CONFIG_BYTES, checked
counts and fallible vector growth. These are logical bounds, not an audited
physical allocator/kernel charge or a universal OOM/panic-free proof. Existing
base-builder allocations and Refusal String presentation remain limitations.

## Actual results and retained failures

| Run / exact source | Actual selected test processes | Clippy lib / tests | Scoped fmt | d4d byte comparison | Outer result |
| --- | --- | --- | --- | --- | --- |
| r17 /661cbd81 |27pass/0fail/0ignored:25focused + current/baseline captures |101 /101 |0 |12/12 equal |1 |
| r18 /74f889be |26pass/0fail/0ignored:25focused + current capture |101 /101 |0 |12/12 equal |1 |
| r19 /2a059b5c |26pass/0fail/0ignored:25focused + current capture |0 /0 |0 |12/12 equal |0 |

The25focused tests are9new renderer tests,8existing renderer tests and8CDI parser/
selector tests. Each exact-name invocation ran1test and recorded its own PID,
wait/reap and raw log hash. There are no zero-filter or unregistered passes.
Baseline1capture executed only in r17; r18/r19 compared its frozen d4d output
bytes read-only and did not re-execute/recount the baseline. Twelve compared files
are config/hosts/dirs for directory/layered roots on root/rootless owners. These
actual before/after data outputs are byte-equal; the existing golden tests also
passed. No before/after native device effect or defect is claimed.

r17's two bool::then_some calls were forbidden; r18's then closures triggered
unnecessary_lazy_evaluations. Both original101 compiler/lint logs and corresponding
source/binaries/caches remain preserved. Plain conditional Options fix that lint
conflict with no exceptions. The additional fallible initial deny-rule reservation
replaces the old fixed array's newly extensible representation. r17 source-first
checkpoint originally reported0executed; subsequent results are separately named.

Behavioral witnesses: selected globals once/requested devices only/request order;
exact environment replacement semantics; implicit host identity/mode/default IDs;
explicit fields/zero IDs/fifo/unbuffered variants; same-path replacement + precise
old/new grants; count/order/copied identity/type/mode refusals; first-destination
mount replacement, Linux clean depth/stable order; all six hook phases; GID0/
dedup; unsupported native edits before output; exact writer bound and escaping
expansion overflow; private-content redaction and immutable selected input.

## Commands, private changes and environment

Approved runner d1323898b255a3ed435eb83fa0f7b0cf, macOS26.4 arm64, Rust/Cargo1.99.1.
Admission `cpus=1,mem=4g,disk=12g`, `nice -n10`, jobs1/testthreads1, sync:false.
Actual nice20 inherited the runner's nice10 plus requested10. No laptop build/test.
Each snapshot has NEW private source/current-target/current-cargo; registry-only
APFS clone, no credentials/config copy. Original r17 also has fresh baseline/
baseline-target/baseline-cargo. Roots below `~/.cache/` on that runner:

- `hex-cdi-render-01a1c640-661cbd81-v1`
- `hex-cdi-render-01a1c640-74f889be-v2`
- `hex-cdi-render-01a1c640-2a059b5c-v3`

Each [render-light-v1](render-light-v1/driver.py),
[v2](render-light-v2/driver.py), [v3](render-light-v3/driver.py) driver records exact
commands/exit codes. Build: `cargo test -p hex-core --lib --locked --offline
--no-run --message-format=json`; execute compiled binary once per exact test with
`--exact NAME --nocapture --test-threads=1`. Core Clippy: `cargo clippy -p hex-core
--lib [--tests] --locked --offline --no-deps -- -D warnings`. Scoped rustfmt checks
only bundle.rs/device_edits.rs/child tests.

Only in private copies: append `pub mod cdi;` to lib.rs and insert the retained
`bundle::tests::render_legacy_capture` test. That proof-only test writes synthetic
Bundle bytes to owned proof files. All other bundle text was independently checked
against each signed source; original d4d baseline bundle also matches Git exactly.
This is not a second production renderer. Actual local lib.rs remains unchanged;
ROOT still owns its integration registration. Input archive hashes, commands,
manifest and local floor observations are in each transfer.json. Capture source,
full private modules and source-before bytes are retained in each raw proof.

## Source, logs and artifacts

Final native pure-harness binary SHA256:
`79e54e7bf21be9f33e29b7cb50fce51d26eb595433068302bf5432a9f0318c0e`. The frozen path is recorded in raw-render-v3/proof/summary.json.
Source hashes (also in render-source-and-logs.json):

| Path | SHA256 |
| --- | --- |
| `crates/hex-core/src/bundle.rs` | `e9f2eb4fc2cee296bb884e38c5de2e505814a6105627a4866ac609b2296cc022` |
| `crates/hex-core/src/bundle/device_edits.rs` | `ed9878f9eabd3678bbfc5bb78d63879785ef8ab5dd88dc9b9481a6f31a73bd3b` |
| `crates/hex-core/src/bundle/device_edits/tests.rs` | `854b0986c80e899a7772e588582e977a7a6772d011b33cfcfaee76a698185a5f` |
| `crates/hex-core/src/cdi.rs` | `7ca46313f6c581c4194a9ba4a295cdda11435a285aacca8b304f41e77b6ce740` |
| `crates/hex-core/src/cdi/tests.rs` | `ec66c85d14110a3c1aae9304f5ff6d095b92197552e70b6c4258cabd12ed5d30` |
| `crates/hex-core/src/lib.rs` | `040e51eb13da41c9d4a7bfb4bf94e18065ba3b6c491edd2ad58e39e2763035a1` |

Final raw log hashes, relative to raw-render-v3/proof:

| File | SHA256 |
| --- | --- |
| `source.json` | `96a01386e4a156281293b92f03e03ceda74c25bf106726dda820ffc94dac39c0` |
| `summary.json` | `b3e29c6e8c7456d5334676bb6d3f3dcd47510f53811bafcec74c337f433c793c` |
| `private-registration.json` | `e6d833e103d2ed30dba4809677c76e298d0fb8e3a66fb895a8b7d12869de87a2` |
| `legacy-comparison.json` | `abc06fdedd527c24d75ce681a05fb4ef1368083915b4d5eea67c995c7f94d3cf` |
| `current-build.jsonl` | `f679358e833767106f4039d57bfbb2c4de8dc1033e8e75fa850d0cbfe3660949` |
| `current-build.stderr` | `3692738f2084e2d509a23e8487cbdd80413354e731bde002884959d693302764` |
| `clippy-lib.stderr` | `0b96dd1c5eb5bd6ba90596712a91b5fed130258eb2517ffe6e6ed0bfd21755fd` |
| `clippy-tests.stderr` | `a59b0bb77c49c02704e521f17ee59874906c2159ff0cdeb32983ceae07ba33dc` |
| `rustfmt.log` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `environment.json` | `b0fba2d0dccffe9f98547b55cb155f2816a620109a336c8061306cc069c33927` |

Individual raw test log hashes/commands/PIDs are in each summary.json. Transport
receipts verify archive bytes, all safe regular member paths/counts/hashes, exact
signed source, each actual one-test result and all recorded reaps. Checks are
recorded honestly even for failed snapshots. Original archives:

| Archive | Bytes | Regular members | SHA256 |
| --- | --- | --- | --- |
| v1 | 93995 | 136 | `a47664cbbc6247d57c8035cb9bb3a4ab512e5ea73e795d5a6b77d9ae803dc05c` |
| v2 | 77420 | 109 | `5dc79163b7a30962780af3d241c8e2e0056ef17874a08d7db23a54a6996ab4bd` |
| v3 | 76968 | 109 | `d880916ca4e0c7f3bd2ca50ccae341d7460fd319a15a273a7e03e4f5ebfd8fa1` |

See `proof-render-v{1,2,3}.tar.gz`, `raw-render-v{1,2,3}/`, per-version
proof-receipt.json, and render-independent-readback.json. Raw libtest/compiler
whitespace is preserved byte-for-byte; all79 exact diagnostics/allowlist are in
render-raw-whitespace.json. Source/docs/driver whitespace is checked separately;
no global whitespace or lint exception was introduced.

## Owned cleanup, limits and owed ROOT work

r17/r18/r19 are terminal. All recorded direct test/build/check Popen children
were waited; every selected test's PID/reap/log witness is retained. No timeout
kill was needed. No sockets/native devices/VMs/SSH processes or hooks were started.
Proof/inputs/source/targets/caches/binaries stay frozen; no old root/other-lane data
or processes were deleted/touched. No main/force/stash/push/merge occurred.
Local free fell below30GiB during read/edit work; reported ROOT and held large
transfers. External recovery restored it; every bulk transfer rechecked >=30GiB,
and receipts retain exact observations. Small source/signing remained permitted.

ROOT owes combined source/API/lib.rs registration/caller review, actual stat/path
identity/TOCTOU and error propagation, native precise device and cgroup behavior,
mount/hooks/GID effects and cleanup/unsupported negatives, all supported target
and OS/arch gates, mutations/E2E/VM/heavy campaigns, physical resource/OOM evidence
and comparative profiling/benchmarks. NetDevices/intelRdt need supported native
variants before acceptance, not silent fields. This batch does not authorize those
runs in this lane and cannot establish the live balancing requirement. Holding
source/jobs after this signed checkpoint pending ROOT's next concrete scope.

## R01–R16 accounting

| Rule | Evidence / concrete remaining gap |
| --- | --- |
|R01|Checked new counts/fallible additions/bounded writer; core production/test Clippy0. Existing base allocations/error String and actual allocation-fault proof remain ROOT review; no universal panic claim.|
|R02|Pinned SPEC/Apply/Unix/OCI/go.mod and exact runtime-tools edf4cb3d21162a17b02e19637f7d7f8c6bd8523f; full source hashes/compatibility in readback + render-light-v1/sources.json. CDI choice/actual native decision ROOT-owned.|
|R03|Existing runtime MAX_CONFIG_BYTES; one existing deny rule; Linux07777 stat permission/special bits; protocol type/phase/access values. No new timing/limit default or measurement copied into policy.|
|R04|Immutable selected inputs/ordered receipts, typed Invalid/Platform/Exhausted and fixed causes; no partially returned Bundle. Shared error API/actual OS outcomes ROOT-owned.|
|R05|Existing Config builder, one render path; no IO/registry/cache/manager/wait/poll. Temporary sort ordinals are per-render metadata only.|
|R06|Focused check/apply/encode helpers and nine cases at the existing renderer seam. Aggregate/source audit ROOT-owned, not claimed here.|
|R07|Comments explain source/minimal-spec/env-quirk/stable sort/budget obligations. Existing unrelated comments/test logic preserved.|
|R08|No performance ratio or competitive claim; comparisons/targets remain ROOT-owned and unrun here.|
|R09|New scoped renderer integration, not a speculative existing production bug fix. Actual lint failures retained; pinned env quirk is a source compatibility observation, not a live Go/native defect claim.|
|R10|Source-first signed checkpoints, actual before/after Bundle bytes, exact commands/raw/hash/cleanup; native effects/mutations remain explicitly owed ROOT.|
|R11|Actual macOS arm64 pure harness only. Linux GNU/musl/Windows amd64/arm64 + other native effects unverified, no passing skips; Intel Mac unsupported and paused ARM Windows VM untouched.|
|R12|Shared pure renderer emits Linux path ordering on every host; no platform adapter, extra binary, host root/trust/crypto changes. Native OS adapters remain ROOT-owned.|
|R13|Only owned renderer device sections/new child/tests/evidence; no shared API/BoxSpec/catalog changes, dependencies or duplicate production renderer.|
|R14|No profiling, benchmark, allocation/page-fault/physical-charge/leak/cost claim; ROOT owns actual measurement.|
|R15|Implementer current cbd/df2 readback/authorized light-only scope, admitted private checks, signed named paths, no children/goals/dispatch/main merge. ROOT owns integration/review/actual heavy work.|
|R16|Original refs/raw errors/inputs/source/targets/binaries/caches preserved; floor hold reported and later verified before transfers; no unrelated cleanup or runner changes.|
