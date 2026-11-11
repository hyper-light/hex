# Original unfinished-Proc Drop deadline regression

SOURCE ONLY. The single new exact case is
`calls::budget_tests::absolute_run_error_return_does_not_renew_drop_sender_budget`.
Compiler invocations, tests executed, native/VM/GPU executions and remote jobs are
all zero. ROOT alone executes this one original control before considering any
production remedy. No result is inferred from source inspection or formatting.

Fixed parent: ROOT6ed9f7820d134f44778cc66da9af2ab789b1b27c. Its client producer
and previous budget leaf are byte-identical to910146edb088bd1d91a23bde257e9af7f57db618.
The new case is appended only to the existing budget test module. No production
file, prior test body, API/Drop semantics, timeout, helper registration, dependency,
runner or native code is changed. The earlier C/QA and client checkpoints stay
frozen, as do the reviewed L2 source and original failures.

## Actual symptom and non-vacuous control

The existing Offers/Stalled fixture supplies a complete actual wire Run request
and Started answer, with no Exit ever published. The transport has no relative
reply budget and a private Measuring table. Core::run_before creates the real
RemoteProc using an original absolute Instant. The case verifies its PID7 is the
fixture's response, try_wait is still None, and the existing program reservation
is present. No OS process is represented by that stand-in PID and no PID is opened,
signalled, waited on or killed.

One owned actor receives that actual Proc. It waits on an owned notification
channel before returning the observer's typed timeout from a scope that owns the
unfinished Proc. Returning that scope executes the actual destructor before the
result can reach the result channel. The parent holds the SAME existing sender
guard and uses the existing `until(original)` readiness/deadline helper to
establish expiry. It then releases the actor's gate and observes its entered
timestamp and result under the existing OBSERVE guard. No copied Drop algorithm,
fake clock, sleep, periodic timer, new manager or production worker exists.

At unchanged910, RemoteProc::drop calls Conn::send_now; that function starts a
fresh transport-derived budget. None becomes an unbounded acquisition of the
held sender. The expected original symptom is therefore a bounded observation
timeout before cleanup; this is currently a SOURCE prediction, not an actual red.
The test asserts the intended bounded final-return behavior and should fail on
that exact symptom after ROOT runs the original. A missing Run/Started/unfinished
state/held guard/expiry/entered witness fails as a harness/setup miss instead of
being accepted as the intended regression.

## Cleanup before every post-spawn assertion

Spawn happens BEFORE the parent holds the sender: even spawn failure cannot drop
its captured Proc behind a guard held by that same spawning thread. After a
successful spawn, the case captures every observation result, always drops the
held guard, ends ONLY its own fixture connection, then joins the owned actor
before asserting. The first bounded result and any later post-cleanup result are
logged separately. Cleanup cannot retroactively satisfy the pre-cleanup return
oracle. Run/Release/other request counts, original/actor/return/release times,
guard ownership, connection closure and reservation cleanup are recorded.

There is no assertion before that cleanup/join sequence after spawn. The fixture
closes no foreign PID, socket, directory or data. The existing CAP25ms and
OBSERVE100ms constants are unchanged caller/test inputs, not production defaults
or a guard expansion. This one control exercises sender ownership, not the
separate partial-frame backpressure branch, actual guest I/O, native cleanup,
GPU execution or whole-scope resource restoration. Missing those results remains
unverified.

ROOT's exact invocation after its matched private compile is:

```text
<matched-client-test-binary> --exact calls::budget_tests::absolute_run_error_return_does_not_renew_drop_sender_budget --nocapture --test-threads=1
```

Use the existing admitted private d1321CPU/4GiB/12GiB, nice10/jobs1/threads1,
empty private target and precise no-sync source/archive rules. ROOT retains raw
logs, selected1/0-executed-at-this-checkpoint, source/compiler/binary hashes,
owned process reaping and temp/source receipts. This document does not dispatch
that command or authorize a repeated batch. No production remedy is included.

## Policy and evidence

Implementer role. Full current Shards7d7e0855a5073ed5163b4fe7ed3355c9ed7c2506
AGENTS.md, R01–R16, development-evidence and CLAUDE read; all four blobs equal the
fully read0a6 policy. Rules SHA256:
df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.
Full exact ROOT6ed Hex CLAUDE and relevant source read. Named signed checkpoint,
isolated cp/t7-client-drop-original; no agents/goals/plans or native/check job.

R01 permits test assertions; production remains unchanged. R02/R03 use the exact
existing Drop/sender algorithm, original caller authority and existing fixture
bounds. R04/R05/R06/R13 reuse one connection/Proc/guard and explicit owned events.
R07 comments state cleanup/provenance obligations. R09/R10 require ROOT's actual
original red before a fix; source alone is not reproduction. R08/R14 comparative
performance/profile claims are not made and remain ROOT's role. R11/R12 use the
existing cross-platform memory transport; actual platform/guest results are
unrun. R15/R16 preserve source/raw, no laptop build/test or foreign cleanup and
return a source-only checkpoint to ROOT for exact execution and final gates.
