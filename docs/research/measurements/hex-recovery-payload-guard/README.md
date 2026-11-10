# Immutable recovery payload consistency

Task AC-T7.4.1/LB03; coordinator Hex `01a1aa77778471339bea3beff90d6bbd`,
implementer `01a1c640e6ad7127bcf34212f474d37c`. Branch
`cp/t7-recovery-payload-guard`, fixed base
`06e6dbeddebd1b910eed5bfdc7c04179989b46a5`. The prior lifetime branch remains
at `ea30bc00387769ed9f0f5727bbf66931c9ed4000`; its source/artifacts are preserved.
Owned production path: placement/rows.rs only. New positive leaf:
placement/recovery_payload_guard_tests.rs. This measurement directory is lane-owned.
Shared parent registration and bounds-owned fixture are not edited in this tree.

POLICY_READ sent before edits with stable key
`recovery-payload-policy-read-f903-06e6-v1`: full current Shards AGENTS.md,
docs/development-rules.md R01–R16, docs/development-evidence.md and CLAUDE.md at
fetched origin/main `f9030566bc227d50770bba879bd1387ebffe410f`. Those four files
are byte-identical to previously read eb5654d6. Rules SHA256 remains
`d634fcb944959ce1e40a9277d12b0905fcfcebf82d9da9832e826f790a349242`.
Full Hex CLAUDE.md, current HANDOFF-codex.md and g121-handoff.md reread; the managed
plan's new terminal-proof entry read against its already loaded unchanged remainder.
Explicit coordinator task/previous acknowledgment authorize this scoped remedy.

## Accepted red and narrow correction

ROOT accepted bounds fixture `807ab21eaeb250a94c2ee38aad7edba29663b8a3`:
same-key owner requested create-only (`start=false`), runner row was modified to
`start=true`. Boot under Budget0/0 called Runtime.start and changed the actual
SimRuntime OCI init from Created to Running. The original selected bounds run was
**1 passed / 1 failed / 0 ignored / 127 filtered**, exit 101. Original log SHA256:
`4789df2b6e242a8bcba1531f5d9f37bab3ca06e1125098f04b4a035cd7494557`.
The source is a real Runtime callback reproduction with simulated box effects,
not native OCI or VM proof. This is accepted prior evidence, not a new before run.

`Driver::boot_scoped` evaluates the complete `Rows::running()?` collection before
entering its Runtime inspection loop. That existing collection already parses each
runner Key. It now looks up the matching owner row and compares the full Payload
(BoxSpec plus start). Any contradiction returns existing typed Stop::Damaged.
The guard does not print either payload, touch files, modify boxes, grant admission
or require an owner row for a remote runner. The complete collection fails before
any recovery Runtime callback, including when a contradictory row sorts later.

This uses the existing Key, Payload equality and pre-effect collection. A second
record scan or new state owner is unnecessary. Live `Driver::hear` already checks
immutable payload equality; boot previously bypassed that check. No wire type,
cache, manager, tuning value, unsafe code or unrelated parser behavior changes.
Production source at87 and06 is byte-identical for placement.rs and rows.rs.

The unchanged bounds regression verifies Damaged, starts0/Created, unchanged
durable record bytes/boxes, surviving sibling and owned cleanup. After refusal it
repairs the runner payload and successfully recovers the matching create-only box.
The new positive control fills the missing authorized-resume branch: existing
fail_start/fail_delete seams retain an OCI init in Created with both payloads
start=true. After removing those faults, Budget0/0 recovery must resume that exact
init, preserve both full Payloads, create no replacement, refuse new admission and
delete/release the owned box/rows/runner charge. All effects remain simulated.

## First guard checkpoint f0887076: focused execution

Actual d132 run `r6` is terminal 0: **28 passed / 0 failed / 0 ignored / 104
filtered out**, selected placement unit tests only, followed by focused node
lib/tests Clippy exit 0. Both the unchanged contradiction fixture and new matching
payload control have named successful results. The original before result above
is retained, not rerun. Scoped rustfmt --check and git diff --check passed.

The regression prints the actual expected/observed witness:
`Some(Core(Damaged("owned and runner rows disagree on payload")))/starts0/Created`,
creates=2, deletes_before_cleanup=0, cleanup_boxes=0. It asserts byte-identical
durable records across refusal, unchanged sibling box, matching create-only repair
and recovery, and final owned cleanup. The positive prints Running/starts2/creates1
(one initial failed start plus the authorized recovery start), admission_refused=true,
cleanup_boxes=0; it also asserts full owner/runner payload equality and final zero
runner charge/own+ran rows. These are native arm64 libtest executions with
Counting/SimRuntime effects; no native OCI/process box or VM was launched.

Exact commands in the private source:

```text
cargo test -p hex-node --lib --locked placement::tests:: -- --test-threads=1 --nocapture
cargo clippy -p hex-node --lib --tests --locked -- -D warnings
```

All source, target, CARGO_HOME and TMPDIR paths are NEW beneath
`~/.cache/hex-lead/lanes/recovery-payload-guard-01a1c640-06e6-v1/` on d132.
Installed toolchain supplied by existing `~/.cache/hex-lead/rustup-scheduler-copy`.
The bounds leaf was copied byte-for-byte from807 ONLY into that private source;
temporary parent registration of bounds+positive leaves exists ONLY there.
All source hashes before/after compilation match. The local parent is unchanged.
Exact source inputs and retained proof:

| Input/artifact | SHA256 |
| --- | --- |
| Literal06 archive with guard/positive/private fixture registration, 5,842,076 bytes | `55418e6d601550e419d0edb5e5077b1d2c6a9292d66c51704a2720c887b43b3f` |
| Changed production rows.rs | `5519a2d59114815d548286f578ab12ab88c771a4c51f72223575e776e273814f` |
| New positive leaf | `f495e2f687087c6c0b05768590534b90c947d7f23d873463c99c6b152476d7d9` |
| Unchanged bounds807 fixture | `073ccb09cbdbc4fc922de351c15cf216e7ad3ca0b7914afd76f636f6a59bc3c3` |
| Original/local parent | `00fb45393f5543e27ee1286e10a8a21657da45dea31f91055bf8443c154b2ea4` |
| Private registered parent | `4e29acfb3032d7ba6a51b4d92d647b6648d08af8e9792e2815622c7675b46580` |
| selected-placement-tests.log, 8,153 bytes | `b0ce6c04f093a0f41e03a8a0dbfd185702fe2a20193cfcdb2b4539ace943981e` |
| focused-clippy.log, 3,038 bytes | `7b884e2bcf276c2f414d9e2aef7446ddd049e6ffd6051fe19431aca05484786b` |
| hex_node-1028f62b4e94535b libtest, 16,670,472 bytes | `4a7f36cc319d887760cceeb7dc204217f75f4ef6572bed8f4a8cf2c10c28fb16` |

Full original proof is the private d132 `proof/` directory. Local frozen input,
original807 red log and exact driver are in
`/Users/adalundhe/.cache/hex-lead/transfers/recovery-payload-guard-01a1c640-06e6-v1/`.
Source and logs/artifacts remain retained. Driver PID1489 recorded/reaped exact
Cargo PID1598/start `Tue Nov 10 08:37:50 2026` and Clippy PID2693/start
`Tue Nov 10 08:39:07 2026`, both exit0. No process was killed. Simulated cleanup
belongs to the unit fixtures; no active owned job or real box/VM remains.

ROOT still owns registration, aggregate review, meaningful guard mutation/
restoration, native forbidden-effect validation, supported-target/full combined
gates and merge. The raw native callback-before-refusal boundary remains ROOT
validation work; no native proof is inferred from SimRuntime success.

## Final key-only diagnostic follow-up

ROOT reviewed f0887076 and accepted this follow-up. The same Damaged message now
adds ONLY the parsed Key Debug identity, preserving R04 affected identity without
rendering either payload/spec. Equality, guard location and lifecycle behavior
are unchanged. Bounds fixture457f832e tests equal start=false with a differing
valid OCI BoxSpec, callback deltas, exact durable bytes/boxes, matching repair, and
content-free key/cause diagnostics. That complete leaf was read/copied unchanged
ONLY into another NEW private light source; bounds-owned source and shared parent
remain untouched locally.

The first green source/target/binary, original807 red and all r6 logs are retained
unchanged. Its verified local 14-file proof bundle is 102,213 bytes, SHA256
`0c6b97b408c1cc682ed5f3362556b9d92397b307df6b25bc204ac8aa41d6dc28`, in the v1
transfer directory above. Adjacent `proof-readback.json` verifies every bundled
member, source equivalence, selected counts and original807 red hash.

Final diagnostic run `r7` is terminal0: **30 passed / 0 failed / 0 ignored /
104 filtered out**, then focused node Clippy0. Same commands and resource envelope;
distinct NEW source/target/CARGO_HOME/TMPDIR under
`~/.cache/hex-lead/lanes/recovery-payload-guard-01a1c640-06e6-v2/`.
Source before/after hashes match. No new baseline, mutation, native campaign or
full gate. The two extra units are bounds457's spec/read-ceiling cases, added only
in the private composition. Final identities:

| Input/artifact | SHA256 |
| --- | --- |
| Final production rows.rs | `72c113b9f160efc8d82d0c3dd4aa5b74dc9c81e0aebd58deee7d8f9fb5531160` |
| Unchanged bounds457 fixture | `b02e662366dcbb44360db1c87a39f0c7410d3d7512e25ed2b3b32f68f3a7a54e` |
| Final source archive, 5,843,491 bytes | `bfabdaaef439134d1cbc4a785ba538b07e88076a02ba7d5f88824455b62cbd82` |
| selected-placement-tests.log, 8,868 bytes | `76eaf56d0038f71b73f3802761a4bb43ae6fd21fb66036feb051de25543b05b9` |
| focused-clippy.log, 3,038 bytes | `76de43738dffa820c5397b3a51f729163396bd09fb2d10eae7c88626866eda23` |
| Final libtest binary, 16,804,664 bytes | `30f25e783a0db6754ffa78384699f87e50a84dc2fa7d8b7d05cdd06c936ccc73` |

Original807 now prints Damaged with Key0/1/0, starts0/Created, creates2,
deletes_before_cleanup0 and cleanup_boxes0. The spec fixture prints
observed_damaged=true, key=true, cause=true, content_hidden=true; callback counts
before/after are identically `(inspect=2,start=0,create=2,delete=0)`, target/sibling
unchanged=true, matching_recovery=true, cleanup_boxes0. Callback deltas and durable
bytes are asserted. Authorized resume still prints Running/starts2/creates1,
admission_refused=true, cleanup_boxes0 and asserts exact full Payloads/zero charge.
These remain native arm64 unit executions with simulated Runtime box effects.

Exact owned r7 processes: driver4122; Cargo4128/start `Tue Nov 10 08:42:24 2026`;
Clippy4278/start `Tue Nov 10 08:43:34 2026`. Both Cargo children exited0 and were
reaped; enclosing run terminal0. No process killed, VM/native runtime box launched
or other lane/root cache modified. Remote raw proof is v2/proof. Local frozen
driver/source/receipt are at
`/Users/adalundhe/.cache/hex-lead/transfers/recovery-payload-guard-01a1c640-06e6-v2/`.

Per ROOT steering after this checkpoint/raw collection: HOLD new source and jobs
pending next concrete scope. ROOT owns combined registration, meaningful mutation/
restoration, native forbidden-effect proof, aggregate review, full gates and merge.
No whole-lane completion is claimed.

## R01–R16 record and owed ROOT work

- R01: production guard uses checked map/Option operations and typed error return;
  no panic, indexing, arithmetic or unsafe additions. Existing lint policy unchanged.
- R02/R05/R12: narrow accepted defect, existing repository primary seams above;
  no broader architecture/platform decision. Existing full collection provides the
  validation barrier; no new scan/manager or wait/cache is needed or added.
- R03: no production constant or admission/deadline change. Test bounds and fault
  inputs exercise existing helpers and the exact Budget0/0 regression.
- R04: preserve immutable payload ownership, typed Damaged and existing lifecycle;
  matching authorized resume remains legal, contradictory recovery is refused.
- R06/R07/R13: one equality guard inside the existing bounded collection; comment
  explains its ordering obligation. No parser refactor or extra interface.
- R08/R14: no performance/competitive/allocation claim; all profiling and benchmarks
  remain ROOT-owned and unrun here.
- R09/R10: accepted exact red/log retained; actual after logs/source identities,
  cleanup and focused lint results recorded above. ROOT owes meaningful mutation
  and native forbidden-effect proof, not inferred from SimRuntime state alone.
- R11: scoped Mac arm64 light execution only; all other targets, native OCI/VM
  effects and full seven-target gates remain unverified for this guard.
- R15: no children/goals/plans, main push, audit, heavy campaign, full gates or merge.
  ROOT registers both leaves and reviews/validates the final combined revision.
- R16: local free checked at 34,608,607,232 bytes (>30 GiB); no data/cache reclaimed.
  New private d132 source/target/cache only, precise --no-sync transfer, admitted
  1 CPU/4 GiB/12 GiB; nice10 command/jobs1/threads1. Prior lane/root snapshots and
  paused Windows ARM VM remain untouched. Active handles/outcomes follow below.
