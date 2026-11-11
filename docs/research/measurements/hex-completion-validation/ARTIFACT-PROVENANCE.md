# Fixture artifact provenance: source checkpoint

This is an implementation-only L4 checkpoint for T-T7.3.1's evidence machinery.
No Cargo invocation, Python test, native workload, mutation campaign, build-cache
reproduction or performance measurement was executed for this checkpoint.

## Observation and attribution

Native Firecracker validation owner `01a1c8aa92c87129a300ab8f333113bd`
reported an actual preparation failure: a successful target directory was copied
into a new private target, then changed fixture source was extracted with archive
epoch timestamps. Cargo returned zero with `fresh:true`, while the ELF hash stayed
at its older value. The owner caught and rejected this before invocation and
reported rebuilding into an empty target with public registry source cache only.
The owner's immutable full-hash/raw receipt has been requested; the queue message
alone is not independent verification. No shared-tool failure or GPU result is
claimed from that report. Native-lane source, artifacts and original logs remain
owned by that lane and untouched here.

Read-only inspection of `build.py` found that its output, target, Cargo cache and
temporary directories are already created exclusively. It offers no compiled
target import. Its artifact intake nevertheless accepted a matching target name
and executable without checking Cargo freshness or the source/target paths. That
is a source observation, not a reproduced failure of this driver.

## Decision and primary source

[Cargo's documented artifact protocol](https://doc.rust-lang.org/cargo/reference/external-tools.html#artifact-messages)
defines `fresh:true` as cached artifacts for which rustc did not execute. It also
provides the manifest path, root source path, target kind, compiler test profile,
generated file paths and executable path. This existing message is the narrow
acceptance seam; no second builder, inventory or artifact cache is introduced.

The driver keeps its exclusive empty-target requirement and now records that
observation before dispatch. It accepts exactly one intended integration-test
artifact only when all of the following hold:

- `fresh` is the exact boolean false, target kind is test, crate type is binary,
  and the compiler profile used the test harness.
- Manifest and fixture root paths resolve to the fixed files in the current
  private checkout and their bytes match its pinned source manifest.
- The executable is a regular file within the current private target after
  resolving symlinks, and Cargo lists it among the generated files.
- The retained executable copy has the same SHA-256 as the accepted artifact.

The build receipt retains relative compiler-artifact paths, freshness and artifact
hash, plus the new target's absolute path and empty/non-reuse observation. A
successful Cargo command gets its source-after snapshot persisted before artifact
intake, so a refusal retains both source manifests and the original Cargo logs.
Relative output arguments are normalized before the Cargo target is configured.
These are additive producer receipt fields; older build receipts do not acquire
this evidence retroactively. An independently produced/external receipt still
requires coordinator review of its own build method and artifact provenance.

Trusting Cargo's exit plus unchanged source hashes is insufficient for the owner's
reported case. Touching extracted files would depend on timestamp freshness and
would leave old source roots unbound. Requiring every source change to produce a
different executable hash is also invalid: comments or unused code can leave an
executable unchanged. Empty compiled targets plus current artifact/source joins
are chosen instead. Registry source reuse is distinct from compiled target reuse;
this driver adds no reuse path for either. No build-time or cache-benefit claim is
made, and existing caller time/byte/resource bounds are unchanged.

## New data controls, not yet executed

Only `test_artifact_provenance.ArtifactProvenanceControls` is new (six cases):

1. Accept a current compiler-artifact and retain its source/target join.
2. Refuse a cached or wrongly typed freshness value despite matching current
   source hashes, preserved source mtime and unchanged binary bytes; do not infer
   stale output from equal binary hashes when compilation is current.
3. Refuse another checkout's source or manifest even when bytes match.
4. Refuse current input bytes that no longer match the pinned source manifest.
5. Refuse wrong test identity/profile/kind or an unlisted generated executable.
6. Refuse an executable outside the private target or a symlink escaping it.

They use bounded literal Cargo-JSON shapes and disposable data files only. They
do not call Cargo, Git, the build entry point, an executable, native probes or a
runner. Their eventual focused command, after admission and exact source intake,
is `python3 -m unittest -v test_artifact_provenance.ArtifactProvenanceControls`
from the scripts directory. No previous tooling, route, SDK or native batch is
requested or repeated. No green result is asserted at this checkpoint.

## Policy, ownership and remaining work

Role: implementer. Full current Shards policy was reread at origin/main
`0a6d91b3de0f10a8397868aa3794adab1e09afdc`; AGENTS, R01–R16,
development-evidence and CLAUDE are byte-identical to the fully read ca3 policy.
Rules SHA-256 remains
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
Full current charter/validation brief and Hex 7bcc CLAUDE were reread. Its expanded
native rationale does not authorize L4 native execution. Source parent and exact
owned file hashes are in `artifact-provenance-source.json`.

R01/R04/R06/R07/R13: existing typed `InvalidEvidence` refusals; one focused artifact
validator, no effects before acceptance beyond the already owned build; no new
unbounded wait or retry. R02/R03/R05/R12: documented Cargo protocol at the existing
seam, no tuning literals or new cache. R09/R10: retain peer attribution and source
observation separately; owner raw verification and new data controls remain owed.
R08/R14: no performance/allocator claim; profiling and comparison remain ROOT.
R11: portable pathlib/JSON checks, actual seven-target producer compatibility is
unverified. R15/R16: source-only named-path signed checkpoint, no new jobs, compiled
target reuse, native actors, deletions or shared-source edits. ROOT owns review,
admission, integration and all full gates. The current source is not reported ready.
