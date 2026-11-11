# Frozen non-profiling collect proposal: source only

This proposes collection of ROOT's completed measured export. It performs no
transfer, remote staging, host query, profiling or collection now. A NEW explicit
LB quiet window and ROOT source/data/admission review are required. The previous
window is closed. Materialize/preflight/build/GPU and RuntimeCost receive no release.

The sender is byte-identical frozen83586fc225fbe7209eb250a6c8d21b8fb70dff97f82dd7dccb1f4f47bde00ad3,
not the changed measurement copy. The existing receiver is byte-identical
8f5f5a9a0624c9b2815f8bbaff87a51448e2541b28f264ebf668288c5ec338f5.
Input14e35, controlsd5e8, IOe9a19 and the original four-file runtime seal
`02bab67d6d60b1371b205a27b9456b8fd3703a51d0b2bd7a2e0035d0676de5cb`
are retained exactly. No source body, algorithm, sync policy or guard changes.

## Exact existing artifact and receipts

Completed root:
`/home/adalundhe/.cache/hex-native-firecracker/held451-export-measured-v1-01`.
Only its verified frozen archive is the transfer payload.

| Actual ROOT input | Exact expected pin |
| --- | --- |
| `output/held1596.tar` |474,576,896B / `b5919c38358cbe1c57cd9b4968a04fe447c431356b4a7a4ca21ccb1c3bcd1507` |
| `output/export-receipt.json` |1,437B / `93a93fbb1a8f00f9e213a338248178a79d5cde3c746eb8f385c450af41ce6b6c` |
| `launcher-final.json` |2,852B / `547bcc96f6dfbd25ed147f00d08407406871164be29d9cb57ef75f256511b662` |
| Manifest metadata inside archive |592,303B / `d61ad9b2b3e360c064ce04a0680d455e7158255e5ad7d9dc3aedcc228ce4f314` |

Both native receipt files were copied byte-for-byte from ROOT's local verified
metadata directory, then rehashed locally against the supplied pins. Their data
agree on complete1,596 objects/472,712,455B and actual archive pin. The launcher
reports child2594269/birth54900180 reaped/absent, scope absent/members[], no errors.
This proposal makes no new native cleanup observation.

`export-result.client-input.json` is explicitly a derived receiver input: Python
integer parsing of that exact launcher plus its actual `launcher_final_pin`
presentation field, as required by frozen8f5. It is not a replacement native
receipt. Original bytes and the derivation are retained in `input-provenance.json`.
Historical export ACK/deadline values inside the completed receipt remain historical;
the new collect admission has false review/quiet fields and null fresh ACK/deadline.

## Transfer and guards

`proposal.json` gives complete remote, normal CLI, receiver and external guard
argv/shell text. The normal CLI carries raw stdout directly into the receiver's
exclusive `held1596.tar.partial`; no MCP/base64 encoding, header stripping,
alternate machine or transfer fallback is proposed. The receiver requires exactly
474,576,896B and the full expected SHA before publishing its own immutable archive.
Any transport prefix, byte alteration, short/long output or nonzero CLI exit refuses
success and preserves raw/partial state. Binary transparency remains unexecuted
and must be established by that actual full-hash result.

The frozen8f5 receiver fixes its remote source bundle to
`/home/adalundhe/.cache/hex-native-firecracker/held451-collect-bundle-2359-v1`.
This lane has never staged or run it. Its absence is not newly queried: existing
fresh-exclusive staging must refuse any collision. No alias or silent client
edit is introduced. Proposed new work root is
`.../held451-collect-root-measured-v1-01`, unit
`hex-native-fc-held451-collect-root-measured-v1-01`. Proposed fresh caller cache is
`/Users/adalundhe/.cache/hex-native-firecracker/held451-acquired-root-measured-v1-01`.
Caller input files would go under the separate proposed
`.../held451-collect-input-root-measured-v1` only after ROOT review/release.

Remote admission remains normal451/linux/amd64,1CPU/4GiB/12GiB, CPU100%,
swap0, pids256, nice10, jobs1/threads1;175s worker/180s whole scope/200s
outer+3s/210s normal CLI are unchanged. Frozen receiver has its existing230s
pipe deadline and exact-owned CLI PID/group TERM2s/KILL2s cleanup. Its existing
required240s external bound is made explicit with the installed, pinned
`/opt/homebrew/Cellar/coreutils/9.11/bin/gtimeout` at235s TERM+5s KILL,
with nice10 on the receiver. No install or fallback is requested. Existing local
Python/CLI/guard binary hashes are recorded for ROOT to verify before admission.

The external timeout supervises the receiver group; frozen8f5 starts its CLI in
a new session and owns that child's independent deadline and PID/group cleanup.
The remote whole scope remains separately bounded. A timeout alone proves no
escaped CLI or remote absence: retain `child.json`, raw logs and partials, and
use only exact owned receipts/probes before claiming cleanup. No broad stop or
timeout-inflation rule is added. The caller requires its existing30GiB free-space
floor plus archive headroom. This is admission/headroom, not a hard quota.

The sender's collect operation reads only the pinned prior receipt/launcher and
frozen archive. It never enters the1,596-origin export loop. Existing83586
bootstrap/mapped ELF/Python control verification remains intact; it reads pinned
runtime-control files, some of whose paths also occur in the object inventory.
Thus no new original-object export/source scan is proposed, and no claim of zero
runtime-control reads is made. No strict verification is skipped to avoid those
reads.

After complete archive hash/exit success, frozen8f5 validates regular member
names/modes/count and extracts only the bounded manifest metadata. It does not
install SDK objects or unpack executable payloads. Actual collect parent/reap/
scope evidence is emitted on stderr and retained in the receiver's `kiln.raw.log`;
local success alone is not remote cleanup proof.

The normal immutable flat staging method remains ROOT's existing method. Its
seven inputs are the four exact runtime members, runtime seal, NEW reviewed
collect admission and NEW exact opaque peer receipt. No staging artifact with a
fabricated ACK/deadline is built now. All old/current roots, partials and raw stay
preserved. Source-only syntax/byte/receipt-join/argv checks are not collection,
SDK readiness, native GPU execution or a performance/whole-cost result. Current
7d7/df2 policy and native evidence ownership persist; ROOT owns the next decision.

The existing flat method is retained in
`../held451-export-c2f6681e/staging-command.json`, `remote_argv[20]`, inline SHA
`77c2531a43191db5d5c939e8753d58d17a5af4a19b2a6753db161ec2ea86c706`.
Its old source/admission/ACK/archive/deadline are reference only. ROOT would
substitute only the reviewed collect destination/unit and actual new seven-member
bundle pin/deadline using that same exclusive method after review and a new ACK.
No new staging helper or executable staging bundle is supplied here.

The receiver's separate CLI session follows its existing
`start_new_session=True` call; [Python's Popen documentation](https://docs.python.org/3.9/library/subprocess.html#subprocess.Popen)
describes the POSIX session creation. The installed coreutils9.11
`share/man/man1/gtimeout.1` documents its foreground-child caveat and delayed
KILL. These are source/document observations, not executed guard proof.

## Scoped R01–R16 evidence and limits

| Rule | This checkpoint |
| --- | --- |
| R01 | No production change. Frozen bodies copied exactly; no new runtime panic/failure claim. |
| R02 | Existing sender/receiver/IO read in full; exact ROOT native metadata and original source seals checked. This reuses the accepted transfer design. |
| R03 | No new runtime allowance/tuning constant. Inherited byte counts, guards and admitted envelope retained; existing240s caller requirement made explicit. |
| R04 | Source-only proposal → ROOT review/new ACK → potential one collect; current review/quiet booleans false and deadline/receipt null. Native terminal state is historical input. |
| R05 | Existing one sender/receiver ownership and waits retained; no cache, retry or poll added. Separate-session cleanup uncertainty stated above. |
| R06 | No algorithm/interface expansion; complete argv and immutable input joins are explicit. |
| R07 | New prose records authority, provenance and non-obvious transport/cleanup limits. |
| R08 | No competing baseline, ratio, speedup or cost claim. |
| R09 | No production defect or fix claimed; no live reproduction dispatched. |
| R10 | Exact source/receipt pins, derivation, false admission, source checks and review seal retained. Actual transfer/forbidden effects/cleanup still owed. |
| R11 | Proposed sender is Linux amd64 on approved451; local macOS arm64 caller is data relay only. Other target/native coverage is unverified. |
| R12 | Same frozen sender, receiver and IO; no package, SDK, host or transfer fallback. |
| R13 | Narrow collection proposal; no second ledger, service or new helper. |
| R14 | ROOT alone owns the prior changed-source measurement and any further profiling. This checkpoint profiles nothing and infers no RuntimeCost. |
| R15 | Assigned native lane is source-only here. ROOT owns next review/admission/execution and final integration; no stage/run/window request now. |
| R16 | All prior/current source/raw/partials preserved. Proposed outputs are fresh-exclusive private caches;30GiB caller floor and remote12GiB headroom remain checked by existing code, not asserted here as current free space/quota. |

`source-validation.json` records local AST/JSON/hash/argv checks only, with exact
validation source and exit. No frozen script import or function execution, local
test/build, remote read or process launch occurs. Repository full gates and
Shards `scripts/check-development-policy` remain ROOT-owned/unrun for this
docs-only checkpoint; there is no Shards tree or production change to validate.
The first metadata checker exited1 on an old-seal filename-key assumption;
`source-validation1.failure.json` retains that raw failure. The corrected checker
uses each existing seal's explicit schema and exits0; no old seal was changed.
