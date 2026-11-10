# Relay/order/cancellation implementer checkpoints

Role: implementer, Kiln `01a1c640e6ed710d8a8c919b4aa1fc8c`; coordinator Hex
`01a1aa77778471339bea3beff90d6bbd`. Fixed base
`8767f0d9c03f37c64c9c86301bceaa878f7eb495`. Only the new
`crates/hex-node/src/scheduler/adversarial_relay_tests.rs` and this evidence
directory are owned. ROOT owns module registration and production/shared files.

## Policy and scope

Read full Hex CLAUDE.md and HANDOFF-codex.md; Shards current origin/main AGENTS.md,
development-rules.md R01–R16, development-evidence.md, CLAUDE.md,
managed-runtime-plan.md, and `/Users/adalundhe/g121-handoff.md` before edits.
Shards revision `eb5654d6f5356cacaca536ff51f14a632fa7e801`, verified against origin
main with ls-remote; rules SHA256
`d634fcb944959ce1e40a9277d12b0905fcfcebf82d9da9832e826f790a349242`.
POLICY_READ sent with stable key
`hex-adversarial-relay-policy-read-eb5654d6-8767f0d9-v1` (coordinator seq6000).
Coordinator's post-readback seq6001 acknowledges source checkpoint/private-copy
registration scope. No children or goal/plan/automatic dispatch calls.

## Checkpoint 1: wrong-owner Start and transport order

Requirement: AC-T7.4.1/LB03. Existing Order tests exercise stamps in isolation;
existing placement payload tests bypass Service's durable ordering. This fixture
joins the existing Service -> Order -> Driver -> counted SimRuntime seam.

Source hypothesis: Service::Hear calls Order.accept before Driver::hear rejects a
Start whose key.owner differs from its configured sender. A refused packet could
advance durable transport order and reject the legitimate correction at that stamp.
This is a hypothesis until the registered fixture executes.

Expected: typed Invalid/placement, unchanged peer-0.rec, zero simulated create and
delete calls, then a legitimate Start at the refused stamp succeeds. The fixture
also sends a later valid stamp, deletes/forgets the box, drops feedback, and checks
one create/one delete, no runtime boxes/runner row/queued permits before its RED
order assertion. Cleanup therefore also runs on the anticipated red path.

Initial source SHA256:
`60a9c51d6efec5dfb6810821351e4e03582f426917d58105aa03857b4f489d29`.
Local allowed checks: scoped `rustfmt --edition 2024` and `git diff --check`, both
exit0. No local builds/tests/benches. This initial signed checkpoint is SOURCE
ONLY: **0 executed tests, red/green counts unknown**. The parent module is
intentionally unchanged and the new leaf is not registered in this worktree.

Read-only runner preflight on approved d1323898b255a3ed435eb83fa0f7b0cf accepted
cpus=1,mem=4g,disk=12g, Darwin arm64. No source/target/cache writes or test runs
at this checkpoint. Local free space was >30GiB. No processes, sockets, children,
guests or targets were started/owned; no cleanup was needed.

## R01–R16 evidence and owed ROOT work

| Rules | Source/light evidence or explicit gap |
| --- | --- |
| R01/R04 | Test-only code; typed Refusal variants, durable order and runtime outcomes asserted. Production panic/state behavior unchanged. |
| R02/R03/R05/R12 | Existing Service/Order/Driver/channel seam only; no architecture, cache, production literals or platform adapter. Test stamps/counters are finite protocol examples, minimum demand derives from existing API bounds. |
| R06/R07/R13 | One focused fixture plus local helpers and counted simulator adapter; scoped formatting checked. ROOT source review owed. |
| R08/R14 | No performance claim, benchmark or profiling performed. All comparative/resource profiling belongs to ROOT. |
| R09/R10 | Source hypothesis and expected cleanup retained; executed reproduction, raw logs and guard mutation remain owed. No production fix authorized. |
| R11 | No OS-specific fixture behavior introduced; supported seven-target gates and actual native effects remain unverified/ROOT-owned. macOS arm64 only. |
| R15 | Isolated owned new file, source-only signed checkpoint, no children; ROOT registration, combined review, fix assignment and all heavy campaigns remain owed. |
| R16 | >30GiB local free; no reclamation or existing source/cache/target mutation. New private remote paths required for any subsequent light check. |

ROOT owes registration under scheduler::tests, exact-source focused reproduction,
review/assignment of any minimal production fix, guard-removal/restored proof,
native transport/runtime effects, all full gates/platform coverage and combined
audit/profiling/benchmarks/heavy E2E/VM/chaos/stress/sweeps. One checkpoint does not
complete the lane. Next independent queue: delayed/repeated Start across
Delete/Forget/new epoch, then in-flight worker cancellation with two peer queues.
