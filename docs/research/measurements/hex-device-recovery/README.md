# Device assignment recovery fixture

Implementer Kiln `01a1c640e6ad7127bcf34212f474d37c`; coordinator Hex
`01a1aa77778471339bea3beff90d6bbd`. Branch `cp/t7-device-recovery`, fixed base
`37021c35e64b2a8bab1d35f20f92a0042d874a44`. Own ONLY the NEW
`crates/hex-sim/src/peer/adversarial_device_recovery_tests.rs` and this NEW evidence
directory. Parent peer.rs registration and all production changes are ROOT-owned.
Lifetime ea30 and recovery guard ee64211c branches/artifacts remain preserved.

POLICY_READ before edits: fetched current Shards origin/main
`cbd39284937d024640c56db6f8d8ef2bd9b0ee24`; FULL AGENTS.md,
docs/development-rules.md R01–R16, docs/development-evidence.md, CLAUDE.md and
managed-runtime-plan.md, plus current Hex CLAUDE.md/HANDOFF-codex.md and
/Users/adalundhe/g121-handoff.md. Rules SHA256:
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
Readback stable key `device-recovery-policy-read-cbd39284-37021-v1`.
Current R15 permits assigned campaigns, while this explicit Hex assignment is
implementation-only: pure fixture plus ONE targeted private light test. ROOT
retains audits, mutations, native/VM/full gates, merges, profiling and benchmarks.
The task message acknowledges scope after full readback; no permission hold.

## Source hypothesis and exact invariant

Source inspected at fixed37021: peer::Ran/ran_text retain Demand but no selected
Held indices; Node::boot constructs fresh indices, then admission.take for a
Boxed::Running row binds again. index::FitIndex::take calls model::bind, which ranks
matching free GPUs by load. Existing full_demand_reboot_preserves_every_constraint_
and_device_hold checks continuing admission/exclusivity without changing GPU load
or comparing the retained slot. Existing index load-preference tests exercise
fresh binding, not surviving-box recovery. These are repository primary sources,
not an executed defect yet.

Required: a confirmed long-lived local OCI key initially assigned GPU0 retains
GPU0 after reboot when only the two matching GPUs' loads swap; a new GPU request
gets GPU1. No device is re-granted while its original box still holds it.

The fixture uses real pure Node/Memory/Records/FitIndex paths and typed peer
Start/Create/Started/Delete/Ended/Forget. It records the first chosen assignment
in a test observation map outside Node and preserves that map across Node drop/
reboot. This represents a simulated box's unchanged attachment; it is NOT a native
GPU attachment or a second production authority. Boot observes Boxed::Running for
the old GPU key and a CPU-only sibling. Kind/model/size/index/capacity/classes/
platform remain the same; only busy load swaps from [0,1000] to [1000,0].

Before checking a possible red, the test captures initial/recovered/new Held
indices, old/new/sibling/progress keys, durable own/ran rows, model runtime
observations, the independently retained assignment map, and slot collisions. A
new CPU-only key must confirm progress with the old sibling still present. All
four model boxes then follow exact delete/end/forget and the test verifies empty
assignments/owned/ran/charges, epoch-only Memory and available GPU admission. The
collision assertion occurs AFTER this cleanup. No sleep, retry or time value.

All capacities/models/load values are synthetic fixture inputs, not production
defaults, measured hardware or security claims. Two GPUs and four bounded keys
are exactly the actors needed for retained slot/new grant/sibling/progress.

## Source-first checkpoint

Status at signed `44a2df3bb8aeac1ed8ae405c3e933b88ee3f6abe`: SOURCE ONLY;
leaf unregistered locally, executed red/green counts UNRUN at that checkpoint.
Scoped rustfmt --check and git diff --check pass. Leaf SHA256:
`c98b6f1e9017e7d1152c4248c8006011389467d4139d455ea198201e95ff89d7`.
No source hypothesis is reported as a reproduced bug. ROOT will register the
leaf; temporary registration is permitted only in the NEW private light copy.

## Actual single selected red

ONE selected test was executed, once, on approved d132: **0 passed / 1 failed /
0 ignored / 158 filtered**, exit101. Owned Kiln run r8 is terminal101. This is the
intended final assignment-collision assertion, not a build, setup or zero-filter
failure. Tested source is signed44a2df3b; this evidence-only follow-up does not
change the fixture or production bytes. Local parent remains unregistered.

```text
cargo test -p hex-sim --lib --locked peer::adversarial_device_recovery_tests::reboot_with_swapped_gpu_load_keeps_surviving_assignment_and_never_grants_it_twice -- --exact --test-threads=1 --nocapture
```

Native Mac arm64 libtest execution, **pure modeled device/box effects only**:

| Key / phase | Required | Observed |
| --- | --- | --- |
| Old Key{owner:0,epoch:1,seq:0}, first confirmed assignment | GPU0 | `[0]` |
| Same old key after reboot, identical device identities with load swapped | GPU0 retained | recovered admission `[1]`; retained fixture observation `[0]` |
| Fresh Key{owner:0,epoch:2,seq:0} | GPU1 | actual new core Create/Started grant `[0]` |
| Observed assignment collision | none | old/fresh both `[0]`, collision indices `[0]` |
| CPU sibling Key{owner:0,epoch:1,seq:1} | survives | typed Started reconfirmation, `sibling_survived=true` |
| CPU progress Key{owner:0,epoch:2,seq:1} | new work admitted/confirmed | typed Create/Started completed after adverse sequence |

Boot's model runtime observations were exactly old=`[0]`, sibling=`[]`, both
Boxed::Running. The independent observation map keeps old=`[0]` while the new
grant is `[0]`; the logical index internally holds old=`[1]`, fresh=`[0]`. This is
an executed pure recovery ownership failure, not an observed native GPU collision.
The original ran row printed in the log is
`0 running d1 session oci 100 512 1 0 linux x86_64 fixture-gpu 1024 0 0 0 0 0`.

Before the failing assertion, all four exact keys completed Delete/End/Forget.
Cleanup assertions **passed before red**: assignment map0, owned0, runner rows0,
admitted charge `[0,0,0,0]`, epoch-only Memory, GPU admission available again in
both owner view and receiver index. Four Create and four Delete requests were
individually checked by the fixture helpers. The observed charge before cleanup
was `[400,2048,0,2]`. Held/durable-row equality assertions placed AFTER the final
collision assertion were not reached; no passing durable-row preservation check
is claimed from this red. Exact indices, sibling/progress and cleanup witnesses
are retained in the original output.

Environment: macOS26.4 arm64; Cargo1.99.1 (5f94df478 2026-08-27),
rustc1.99.1 (58cf8f9c3 2026-10-13). Admission1CPU/4GiB/12GiB, jobs1/threads1;
command nice-n10, observed nice20 inherited from runner clamp, never relaxed.
NEW private source/target/CARGO_HOME/TMPDIR:
`~/.cache/hex-lead/lanes/device-recovery-01a1c640-44a2df3b-v1/` on d132.
Existing installed toolchain read from `~/.cache/hex-lead/rustup-scheduler-copy`.
Temporary registration ONLY in private peer.rs:

```rust
#[cfg(test)]
#[path = "peer/adversarial_device_recovery_tests.rs"]
mod adversarial_device_recovery_tests;
```

Original peer.rs SHA256
`3bdec78b35be0c93c491ad65b5b268b312702c56a98202cae01bfe510903c120`;
private registered SHA256
`9de0915a302a0582d6683ee95528054c83d79300d829e172d52f17a265009d5c`.
All private source hashes before/after are byte-identical. Exact input receipt,
driver, raw log/result and collection readback are committed alongside this file.

| Artifact | SHA256 |
| --- | --- |
| Source archive, 6,470,388 bytes | `ee251bb9bb01edc8914b597ad7e7fbdb2c851da9c4b549b6ca8a3f0f7adf6105` |
| selected-device-recovery.log, 5,218 bytes | `6cbaa37df4769f9a3a4471561eaff2b9c52f5c0585161f3fa4f4b1db9c9b8a52` |
| libtest hex_sim-9ebd5bc0b4cd9747, 13,872,808 bytes | `b27e191583ee83b9c7f953aa8c8824410a0dbd1a804208319299e1e94d799e2c` |
| Verified 10-file proof bundle, 110,137 bytes | `b41259576db34206e56374e2a862ed7ab9dca8b0d9844ad4bc49c532532cda30` |
| Unchanged index.rs | `a4a59b18b0d45d94c799f0501ac761150d180b9f8797c56b18cca7bd3073c2b7` |
| Unchanged model.rs | `696cd6a34f5e0f7801955da1512ef942c1f84390f8f52a1f09a8e7dd7252f3ea` |

All bundle member hashes were verified after precise private transfer. Originals,
full source manifests, source archive, binary and private target/cache are retained
remotely and locally under
`/Users/adalundhe/.cache/hex-lead/transfers/device-recovery-01a1c640-44a2df3b-v1/`.
No binary is committed. Driver PID16056 recorded exact Cargo PID16062/start
`Tue Nov 10 09:24:08 2026`; Cargo exited101 and was reaped with Popen.wait, run
terminal101. No process was killed, actual box/VM/GPU attachment launched or other
lane/root source/target touched. No active owned job remains.

ROOT owes acceptance/review, any narrow production remedy under separately assigned
ownership, matching controls/meaningful mutations, native device bridge/effect
validation and combined supported-target/full gates/merge. No production fix,
extra selected test, Clippy or native campaign was run by this lane.

## R01–R16 and owed work

- R01/R04: test-only assertions and existing typed lifecycle/Boxed/Stop outcomes;
  no production panic/error/state change. Confirmed creation precedes reboot.
- R02/R03/R05/R12: existing repository seams, no architecture/interface/platform
  choice, production constants, extra manager, cache, polling or timing change.
- R06/R07/R13: one focused leaf with fixed actor/message sequences; comments
  distinguish external fixture observation from recovered bookkeeping.
- R08/R14: no competitive, performance, allocation or hardware claim; ROOT owns
  all profiling/benchmarks, none run here.
- R09/R10: the exact selected pure test reproduced the ownership failure above;
  original indices/model effect/cleanup and logs retained, no production fix.
- R11: pure portable source; native GPU/OCI, security, all target behavior and real
  device attachment are unverified, not inferred from model execution.
- R15: source-first signed checkpoint, then ONE selected unit test on approved
  d132, 1 CPU/4 GiB/12 GiB, nice10/jobs1/threads1, NEW private source/target/cache,
  precise no-sync transfer. No children/goals/plans. ROOT owes acceptance/source
  review, any production remedy, meaningful mutation/native/full combined gates.
- R16: local free checked at 35,157,925,888 bytes (>30 GiB); no data/cache removed.
  All old evidence, source, targets, snapshots and paused Windows ARM preserved.

This checkpoint returns the assigned single-test red and evidence. It does not
declare the whole lane, native GPU coverage or full scheduler goal complete.

## Authorized remedy: original transient-control source

ROOT accepted the exact model collision and expanded production ownership to
peer.rs/index.rs durable recovery only, this leaf and own evidence. Existing
index holder state remains the authority; no parallel Ran Held/cache is allowed.
Serialize exact indices in the same runner write before Create; restore instead
of picking; reject unprovable legacy device claims; preserve CPU-only legacy.
ROOT retains every audit/mutation/native/GPU/VM/full gate/profile/benchmark/merge.
Policy cbd39284/df2acf47 readback with this expanded ownership sent before edits.

Before any production change, appended a small original-source control for the
existing fresh-verdict checks: retained local owner+runner, and runner-only rows,
each under (a) transient GPU memory fully used and (b) session CPU background just
over the existing HOT_PER_MILLE boundary. The original collision test body stays
byte-identical. All four cases must keep assigned0 and cleanup before assertions;
fresh policy must still refuse NoGpu/Hot after successful retained recovery.
This control is SOURCE ONLY/unexecuted at this checkpoint. It justifies no broad
behavior change until the actual original-source outcome is retained.
