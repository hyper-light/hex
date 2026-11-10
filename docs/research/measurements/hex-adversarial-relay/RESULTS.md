# Service relay/order/cancellation fixture evidence

Implementer: Kiln `01a1c640e6ed710d8a8c919b4aa1fc8c`; coordinator Hex
`01a1aa77778471339bea3beff90d6bbd`. Requirement AC-T7.4.1/LB03.
Base `8767f0d9c03f37c64c9c86301bceaa878f7eb495`; first signed source checkpoint
`dc29e7dadb5d820bfa83e115280dc79574f9f68a`. Readback/initial evidence is in
[IMPLEMENTER.md](IMPLEMENTER.md). Ada/ROOT subsequently explicitly ACKed full
POLICY_READ and ownership/resource scope; no further permission wait applies.

The final leaf SHA256 is
`667b734a44c1ac87389e3dd8ebc56b522c7c753a39b9bda662a79ede59a9c7fb`.
Only this new leaf and this evidence directory changed. The final main leaf has
three contract fixtures. ROOT still owns parent registration; the parent in the
supplied worktree is unchanged. Private remote registration appended only
`#[path = "adversarial_relay_tests.rs"] mod adversarial_relay_tests;` to the copied
scheduler/tests.rs (before SHA256 `697e9c098a4d8689e22c765fb1ad445451c2bc0a72aa29407b403aeca642f697`,
after `58a87fd5e8aae1dec14ddaef201e8c3fea8915911662df362fa98440f3649c20`).

## What was executed

All builds/checks ran ONLY on approved d1323898b255a3ed435eb83fa0f7b0cf, admitted
cpus=1,mem=4g,disk=12g, `nice -n 10`, Cargo jobs1, Rust test threads1, precise
`--no-sync` input. The runner already inherited niceness10, so observed native
niceness was20. Toolchain: rustc1.99.1 `58cf8f9c3`, Cargo1.99.1 `5f94df478`,
macOS26.4 arm64. No local builds/tests/benches occurred.

These execute the production Service/Order/Driver and worker functions, actual
record-file I/O and std channels/threads, with a counted **SimRuntime**. They are
not pure Order/model tests. Runtime box effects are simulated; no native runtime
box, guest, SSH connection or VM was launched. They establish neither real
transport/runtime effects nor crash/power-loss durability.

Private roots on that runner:

- `.cache/hex-adversarial-relay-01a1c640-dc29-v1/` (r2): first build/test negatives.
- `.cache/hex-adversarial-relay-01a1c640-v2/` (r4): second setup negatives.
- `.cache/hex-adversarial-relay-01a1c640-v3/` (r6): contract/candidate separation.
- `.cache/hex-adversarial-relay-01a1c640-v4/` (r8): three contract tests, no candidate appendix.
- `.cache/hex-adversarial-relay-01a1c640-v5/` (r11): final leaf with four test-lint corrections.

Each has NEW private source/target/cargo/proof paths; subsequent snapshots cloned
only this lane's completed inactive paths with APFS copy-on-write. Earlier
sources, logs and binaries remain frozen. All non-leaf source bytes were compared
to the preceding private snapshot; shared/production bytes stayed identical.
No root snapshot/target/job, other lane, runner/service, host security/trust,
paused Windows ARM VM or unrelated process was touched.

The `checked_in_leaf_sha256` field in historical manifests identifies working-leaf
bytes at transfer, not a claim that each intermediate overlay was committed.
The fixed commit plus overlay hash and private registration identify each run.
The initial full source archive was5842523B, SHA256
`1a09691abb13c7d6255f3adc15f1bb4353ee4db5cad587f86f97061055374103`.

## Outcomes and contract classification

| Run | Actual selected results | Classification |
| --- | --- | --- |
| v1/r2 | build0; 0pass/2fail101 | Fixture setup: default helper prober initialized no classes, so Start had no runner row. Failed before intended assertions; not a reproduced production defect. |
| v2/r4 | build0; 0pass/3fail101 | Fixture setup: Shards placement correctly rejected a Process payload because microVM placement requires OCI. Not a reproduced production defect. |
| v3/r6 | build0; 2pass/2fail101 | Delayed/restart and wrong-owner/other-peer contract tests pass. Original candidate reaches its spec-red assertion. Cancellation completes effects/cleanup but its final fixture assertion compares raw cause text with Refusal::Display. |
| v4/r8 | build0; **3pass/0fail/0ignored**; scoped rustfmt0; scoped Clippy101 | Each runs1/passes1,129filtered. Four unreachable! test macros violate Clippy; original failure retained. Replaced only those with permitted test panic assertions, without lint allowances. |
| v5/r11 | build0; **3pass/0fail/0ignored**; scoped Clippy0; scoped rustfmt0 | Exact final leaf; each runs1/passes1,129filtered. Owned PIDs3898/3899/3901 recorded before wait and reaped; no candidate appendix. |

The original dc29 assertion that a wrong-owner payload leaves its **own sender's**
transport stamp unchanged is withdrawn as a contract expectation. Order explicitly
commits transport fencing before Driver effects, and changed bytes at an accepted
stamp refuse. v3 retained the original candidate function byte-for-byte (function
SHA256 `836d8ed336c5b80e27ee0594e4522ccb8608f1063267da6653820529be4b8699`)
under corrected fixture setup, separately from final contract tests. It observed
Invalid/placement, order advance, Broken on changed same-stamp bytes, later valid
progress, zero simulated effects at refusal, and cleanup boxes0/runner-row absent/
permits0. This supports intentional fencing, not a production defect. No production
fix was made. Original candidate source remains in dc29 and private v3.

Final fixtures establish:

- Wrong-owner Start: typed Invalid/placement before any simulated create/delete;
  another configured peer creates its own key; the original peer's next stamp
  creates its legitimate key. Order rows are independent. Changed bytes at the
  consumed stamp return Broken. Both boxes/runner rows and permits clean up.
- Delayed Start: identical retry creates only once; changed same-sequence message
  returns Broken without changing order; old Start after Delete/Forget/restart
  creates nothing. Newer sender epoch fences even old-epoch seqMAX. Wrong realm
  and unconfigured transport sender return typed Invalid before order changes or
  row allocation. A later legitimate Start still creates. Exactly2 simulated
  creates/2deletes; final boxes0/runner row absent/permits0.
- Cancellation: real owner placements go to two peer queues. Start actually
  creates a simulated box while its response is held/lost. Both accepted words
  retain their permits. Queue saturation returns exact typed Exhausted/bound2
  while deletion intent remains durable. The worker reconnects, suppresses stale
  Start and sends exact Delete; permits are **2/2/2/1/0**. After owner drop/reboot,
  deletion and the other placement's byte-identical owner row remain. Both peers
  then finish Delete/Ended/Forget; the second still progresses. Worker joins;
  exact trace is connection1 Start then connection2 Delete. Final permits0/0/0,
  boxes0/0 and all corresponding owner/runner rows absent. The report expectation
  derives from typed Refusal::Display; no production behavior was changed.

All gates inside the worker fixture wait on owned channel events, not sleeps or
elapsed-time assertions. Its virtual retry clock is fixed and the mock accepts
only Describe/Hear; Describe has no offer, so advertisement integration is outside
this witness. Exact original deadlines/production bounds are unchanged. The driver
has a finite120s outer per-test process ceiling, solely to bound broken fixtures.
Each actual selected test finished below1s; no performance conclusion follows.

## Retained raw proof and hashes

[Raw archive](proofs-v1-v5.tar.gz): **105141 bytes / 81 regular members**, SHA256
`e31dc24be0b6847754246c7111977fe2fd5c10e62023c57e20b67585ba4e63e3`.
[Receipt/member hashes](proof-receipt.json) verifies every member and final leaf
byte equality. Each version includes original raw build/log/summary/source records;
private sources, caches, targets and exact binaries also remain on d132. Historical
[drivers](light-v5/driver.py) record exact commands/environment; replay requires a
new private prefix rather than overwriting frozen logs. [collect-proof.py](collect-proof.py)
is the collection command run on d132; it creates only this lane's deterministic
archive and checks selected counts/log identities. Transfer used1000-character
base64 lines captured by the local CLI caller; no archive data was printed to UI.

Final binary SHA256:
`e607a3501e6ddaac576b90827e439d0d5e89893e6f12bfc33f3130ddf939f817`.
Final source SHA256:
`667b734a44c1ac87389e3dd8ebc56b522c7c753a39b9bda662a79ede59a9c7fb`.

| Final raw log | SHA256 |
| --- | --- |
| [Cancellation](raw/v5/proof/test-0.log) | `eb702a5a6557138eb10727e4cb93dd7273596167190484d22750e4cd06a9b73e` |
| [Delayed Start](raw/v5/proof/test-1.log) | `3850ba88263a280779c2b87713e5bf2c3f631d164e80858d92d1568ad878accb` |
| [Wrong owner/other peer](raw/v5/proof/test-2.log) | `211618d6635516ebbb265f356287be1c6cf72f634f9421b9f0a2fb8e4ed9a255` |

[Original candidate red](raw/v3/proof/test-1.log) SHA256
`44f33105bc3223dfca1ad7030ef35496bba02bca6dbab07c855cabf871b959ab`.
It remains historical evidence only and must never be registered in the main test
suite. Earlier setup negatives, cancellation assertion negative and v4 Clippy
failure are retained byte-exact; they are not relabelled production failures.
Raw libtest logs retain their emitted trailing spaces. Source/docs/driver whitespace
checks are separate from those deliberately unchanged original bytes.

ROOT reviewed the current667b leaf and accepted all3 contract cases. This is the
requested implementer checkpoint, not a combined-tree/full-goal completion.
After its signed handoff, source and jobs are **held pending ROOT's next concrete
scope**; no further campaign is authorized by this checkpoint.

## R01–R16 and work owed ROOT

R01/R04: only test code changed; typed Invalid/Broken/Exhausted and full keyed
control transitions/counters/rows are checked. R02/R03/R05/R12: existing seams and
documented Order contract only; no architectural decision, production literal,
cache, manager, platform adapter, learning or grant change. Demand uses existing
minimum API reservations and a valid simulated OCI payload; the native prober's
initialized class is explicit (Shards on macOS, OCI elsewhere). Unsupported native
class availability is not fabricated as success. R06/R07/R13: finite scoped
fixtures, comments for non-obvious contract/cleanup, scoped rustfmt checked; ROOT
source review remains owed.

R08/R14: no benchmarking/profiling, comparative, leak-free, memory/kernel return
or latency claim. R09/R10: original negatives, candidate red, sources, commands,
hashes, typed outcomes and cleanup are retained; no production bug/fix claimed.
R11: only actual arm64 macOS harness execution established; seven-target gates
and actual supported native runtime/transport/VM combinations are unverified.
R15: isolated implementer-owned leaf/evidence only; no children, automatic goals,
plans or dispatch, merges, pushes or heavy campaigns. R16: all recorded local free-space checks were
above30GiB; no reclamation or deletion of existing data/caches/evidence.

ROOT owes final registration/review on its combined revision, any actual native
runtime/SSH effects, meaningful guard-removal/restored proof, all full gates and
supported-platform coverage, aggregate audits, profiling/benchmarks and all heavy
E2E/VM/chaos/stress/sweeps. These focused fixtures do not complete the lane or the
full scheduler goal. No lane-owned guest, SSH child or native runtime process
needs cleanup; completed private sources/targets/caches/binaries are intentionally
retained for review.
