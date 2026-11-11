# Hex adversarial bounds: scoped fixture checkpoints

Implementer: Kiln `01a1c643e475709b8a829553d1100be1`.
Coordinator: Hex `01a1aa77778471339bea3beff90d6bbd`.
Base: `8767f0d9c03f37c64c9c86301bceaa878f7eb495`.
Owned source: `crates/hex-node/src/placement/adversarial_bounds_tests.rs` only;
this evidence directory is lane-owned. No shared/production files are changed.

## Policy and scope

Read full Hex CLAUDE.md, `/Users/adalundhe/.cache/hex-lead/HANDOFF-codex.md`,
`/Users/adalundhe/g121-handoff.md`, and fetched Shards `origin/main`
`eb5654d6f5356cacaca536ff51f14a632fa7e801`: AGENTS.md, development-rules.md
R01–R16, development-evidence.md, CLAUDE.md and managed-runtime-plan.md.
Rules SHA256: `d634fcb944959ce1e40a9277d12b0905fcfcebf82d9da9832e826f790a349242`.
POLICY_READ sent to coordinator with stable key
`hex-adversarial-bounds-policy-eb5654-8767-v1` (receipt seq6011).
Coordinator ACK received before edits: ownership/resource scope approved, no
further permission wait within this fixture/light-test queue.

No laptop build/test/benchmark, VM, child agent, autonomous goal or plan.
Any light execution uses only approved d1323898b255a3ed435eb83fa0f7b0cf,
cpus=1/mem=4g/disk=12g, nice10/jobs1/threads1, new private source/target/cache
and precise --no-sync transfer. ROOT owns registration, production changes,
mutations, aggregate review, full gates and all heavy/native campaigns.

## Checkpoint 1: ignored-entry boundary (source only)

Requirement: AC-T7.4.1/LB03. Test:
`excess_ignored_entry_refuses_before_crash_cleanup_and_recovers_at_zero_admission`.
Source SHA256:
`56187f46d943b10b2b47ee5ce0b6373d85787e950693816f64d5feba86524da3`.

Existing coverage read first: all placement tests, records.rs and its tests,
rows.rs, Driver recovery/validation and peer::Node::boot. Existing tests cover
malformed count JSON, arithmetic overflow, Files record/temp counts and small
recovery ceilings. This fixture instead crosses the Driver's total directory
entry bound with a private ignored extension while valid durable authority and
the exact permitted population of crash temporaries coexist.

One held box supplies three records (epoch, owner, runner). Its persisted 1/1
read budget allows three records and three temporaries, six total entries. One
extra private `.note` must return exactly Core(Full { directory entries, 6 })
before removing temporaries, rewriting records or calling Runtime inspect,
create, start or delete. Removing only that entry must permit recovery at active
0/0 admission, remove the three temporaries, preserve namespace/physical ID,
refuse new admission and retain deletion of the held box. An independent private
state directory shares the simulator and supplies a sibling box that must remain
intact until its own explicit cleanup. Both boxes are deleted before the final
refusal/effect assertions; TempDir owns all filesystem cleanup. No sockets,+pipes, OS child processes or native runtime boxes are used.

Observed execution: none yet. Selected tests: 0 executed, 0 known red, 0 known
green. The new module is deliberately unregistered in this worktree; a successful
zero-filter invocation would establish nothing. Scoped rustfmt and git diff
whitespace check exited 0 locally; these are source checks only.

ROOT/light invocation after explicit registration under placement::tests:
`cargo test -p hex-node --lib --locked placement::tests::adversarial_bounds_tests::excess_ignored_entry_refuses_before_crash_cleanup_and_recovers_at_zero_admission -- --exact --nocapture`.
Registration in a private light-check copy is permitted and must be recorded.

## Checkpoint 2: contradictory recovered start intent (executed red)

Fixture source SHA256:
`073ccb09cbdbc4fc922de351c15cf216e7ad3ca0b7914afd76f636f6a59bc3c3`.
The first fixture is unchanged. The new case is
`contradictory_recovered_start_intent_refuses_without_executing_init`.
Source checkpoint 1 is signed `5ea1d3654260cab3a50e97ad5d93d925eba52b0d`.

A valid create-only OCI payload leaves an actual SimRuntime box in Created, with
zero Runtime.start calls. After dropping the driver, change only the runner's
persisted payload.start to true; its same-key owner payload still says false.
Reboot with active 0/0 admission, within the persisted retained read allowance.
Expected: typed Core(Damaged), no Runtime.start, original box still Created,
unchanged durable rows and retained authority after restoring the contradictory
fixture input. Observed: boot succeeded, Runtime.start ran once, and the original
SimRuntime box became Running. Creates remained two (target and independent
sibling), deletes before cleanup were zero. The test then deleted the target,
verified the sibling unchanged, deleted that sibling, checked zero boxes and
epoch-only records, and only then failed its forbidden-start assertion.

This is a reproduced Runtime callback and simulator state effect through the
real Driver/Rows/peer recovery code. It is not native OCI process execution,
native backend cleanup, a VM run, or evidence of an external-input exploit.
Source hypothesis explaining the observed effect: boot validates each runner
payload independently and stages its start; peer recovery compares same-key
owner/runner demands, but does not compare their payloads. No production fix was
made or authorized to this lane. ROOT assigns/reviews a minimal fix separately.

Exact command, toolchain, parent registration and selected names are retained in
`cp2-result.json`; `light_check.py` is the create-new private-copy runner harness.
`cp2-transfer.json` records the precise no-sync stdin source transfer. The actual
native macOS arm64 libtest ran on approved d132 with Rust/Cargo 1.99.1,
nice10/jobs1/threads1, cpus1/mem4g/disk12g. It selected exactly two tests:
**1 passed, 1 failed, 0 ignored, 127 filtered out**, Cargo exit 101.
The ignored-entry boundary passed; the contradictory-start test failed at
starts=1 versus expected zero. Module registration exists only in that private
copy, with before/after parent hashes recorded. No local build/test occurred.

Original runner root (retained, completed, no active job):
`/Users/adalundhe/.cache/hex-adversarial-bounds/01a1c643-cp2-v1`.
Its source archive, source/target/cache and all original logs remain untouched.
Owned local CLI PID27121 and remote Cargo PID98443 completed; no termination was
needed. Fixture temp directory was empty after both tests, including the red.
Local exact copies were transferred and verified by byte length and SHA256:

| Artifact | Bytes | SHA256 |
| --- | ---: | --- |
| cp2-test.log | 5741 | 4789df2b6e242a8bcba1531f5d9f37bab3ca06e1125098f04b4a035cd7494557 |
| cp2-result.json | 2754 | ea15c2860cea760a45dd3a3851b1a1abf74c1dcf896522dea11859bb78c06e36 |
| cp2-run.json | 1713 | e4bcce83b0516a322745d399b5adbf8a449d9964751383b581879219f04fc222 |
| cp2-source-hashes.json | 109907 | f7df3c6204d50fc00ab35ef4dd88f1adf56d3c7337f29609f3c21e07dd1f80d3 |

Source archive SHA256:
`553c2f2e6329700b6dcd1e1154ce59765c16a5d2ef4815ec2d93a313db35fb5b`.
This closes only checkpoint 1's owed selected light execution. R09/R10 now have
an original red with actual forbidden simulated effect/cleanup; fixed green,
guard-removal/restoration, production review and native validation remain owed
to ROOT. The failing regression is deliberately retained as red.

## Checkpoint 3: representable hostile read count (executed green)

Test: `representable_hostile_read_ceiling_compacts_without_new_physical_authority`.
Fixture source SHA256:
`ace384486b9d1fc16cf14a9444096d85f1ea8ee9132518837d6e610ab703ea55`.
The preceding two test bodies are unchanged. Source base for the private copy is
signed red checkpoint `807ab21eaeb250a94c2ee38aad7edba29663b8a3` plus this new
test and an explicit exact-selector argument to the light harness.

Set each persisted read count to usize::MAX / 4. With the epoch, twice the sum
of the counts is usize::MAX - 1: the largest representable structural entry
envelope, immediately below the arithmetic-overflow cases already covered.
The directory still holds exactly one legitimate box's epoch/owner/runner rows.
Recovery at active 0/0 must accept the read envelope, compact saved counts to
the actual 1/1 retained rows, keep physical identity and preserve non-epoch row
bytes. A new submit must return typed Full(bound=0), then the original box must
remain deletable. No allocator instrument, timing threshold or cost claim is
introduced; the test does not measure allocations, RSS or kernel charges.

Actual approved d132 check selected only this new exact name:
**1 passed, 0 failed, 0 ignored, 129 filtered out**, Cargo exit 0.
Observed: Core(Full { what: owned records, bound: 0 }), creates=1, starts=0,
retained=1 before cleanup, boxes=0 after cleanup, empty fixture temp directory.
The first two tests were compiled but were not re-executed in this run. Their
checkpoint 2 results remain 1 green/1 red; no all-green final-tree claim is made.

Fresh private runner root, retained and inactive:
`/Users/adalundhe/.cache/hex-adversarial-bounds/01a1c643-cp3-v1`.
Its source/target/cache/temp paths are separate from checkpoint 2 and all ROOT or
other-lane paths. Same approved cpus1/mem4g/disk12g, nice10/jobs1/threads1,
no-sync stdin transfer, native macOS arm64 1.99.1 toolchain. No native runtime,
OS child fixture or VM effects were exercised. Local CLI PID52689 and remote
Cargo PID99425 completed normally; no stop action or owned live handle remains.
Raw originals and byte/hash-verified local copies are preserved:

| Artifact | Bytes | SHA256 |
| --- | ---: | --- |
| cp3-test.log | 5042 | 8c7445fa859d6a2aaf483b187c260b997d690a086aab60ad31d0d37d02417a16 |
| cp3-result.json | 2763 | 78b60eb5fa5f6a359136e43ca476609bb9a6a24fee184625e14b178dcee81904 |
| cp3-run.json | 1802 | c1bf770f361d6a7a3a35e974c584efe19d86ecd66a50eb2f82a06ecbd50e3e69 |
| cp3-source-hashes.json | 110884 | 06b7fa5ad62a284e5bb9899dd3057edd25efd0f210301e9029887d7e7f82fe98 |

Source archive SHA256:
`6a7dbb784fe06cf6a80b7cce5dcd74215df3116b9f3f6444eaca255d46b97533`.
All original red evidence remains unchanged. These are scoped fixture
checkpoints, not completion of the lane or broader scheduler acceptance work.

## R01–R16 accounting and owed work

- R01/R04/R06/R07/R13: test-only straight-line fixture at existing seams; typed
  Full refusal, explicit refusal/recovery/cleanup phases, no production change.
- R02/R03/R05/R12: no architecture, runtime constants, cache, wait or platform
  adapter added. Fixture counts derive from the actual three durable rows and
  existing one-temp-per-record rule (records.rs). No allocation or cost claim.
- R08/R14: no performance work or claim; all profiling/benchmarks remain ROOT's.
- R09/R10: this checkpoint is a source hypothesis, not reproduced behavior.
  Preserve original light red/green logs when executed; ROOT owns guard removal.
- R11: portable std-filesystem plus SimRuntime fixture; actual supported seven
  target compilation and native filesystem/runtime behavior remain unverified.
- R15: one assigned writer, no delegation, signed named-path checkpoint; no
  claim that this single checkpoint completes the lane or broader Hex goal.
- R16: local free space checked above 30GiB before edits; no cache reclamation,
  other-lane mutation, remote job or process handle exists at this checkpoint.

Owed: ROOT review/registration on combined tree and separate fix assignment;
meaningful guard-removal/restoration evidence; applicable combined formatting,
clippy, production/vendor and seven-target gates; actual native recovery effects
and any heavier campaigns ROOT selects. Shards' check-development-policy script
is absent from Hex; no such gate has been claimed passed here.
