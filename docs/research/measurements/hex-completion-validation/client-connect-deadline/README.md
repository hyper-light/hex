# Client connection absolute deadline source (AC-T7.4.1)

Source parent: `e852c1b5ba9e1e2c8eb122dfb5817cd4dc339748` (ROOT, fixed).

The shared guest client adds `connect_before(LinkOwner, Instant)` and
`connect_evented_before(LinkOwner, Arc<dyn Fn() + Send + Sync>, Instant)`.
They validate the caller's original instant, choose a smaller configured reply
cap when present, and pass the same effective `ReplyDeadline` through the
existing control dial, partial relay-token reads, and Core hello send/answer.
The event form installs the existing backlog and hands the same Shared owner to
its existing Pump. The caller deadline is not retained as a connection lifetime.
A smaller learned hello wait retains its existing calibration and answer precedence.
No relative fallback is introduced when `Link::reply_budget()` is `None`.

Production changes are only `guest.rs` constructor seams and crate visibility on
`ReplyDeadline::caller_before` / `limited_by` in `calls.rs`. Guest constructors
serve Linux, macOS and Windows. Native socket-path constructors in `linux.rs`,
`macos.rs` and `windows.rs`, all protocol/readiness algorithms, and the original
6753 Drop regression are unchanged. A deadline-aware Link override remains
required; the default rejects a finite dial rather than claiming a bound.
L2 ports its SAME-pool consumer with the original `budget.deadline`.

Policy: implementer; fully read Shards
`7d7e0855a5073ed5163b4fe7ed3355c9ed7c2506`, four policy blobs unchanged,
rules SHA256 `df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
Exact parent Hex CLAUDE read; unchanged from the fully read 6ed9 source.

This checkpoint is source only. Builds, checks, tests, guest processes, native
and GPU executions: **0**. Three appended constructor controls are defined, with no prior guest tests rerun.
ROOT owns joined compilation and dispatch. No native readiness is claimed.

The exact three selected names are in `controls-source.json`. They cover both
ordinary and evented constructors: expired/zero/overflow refusal before dial,
exact original-at forwarding and smaller configuration, actual one-byte token
progress versus an unanswered real Hello, and positive complete Hello/link/Pump
handoff. Peers are preconnected owned TCP stand-ins, never a VM or native guest.
Actual token reads and successful Hello writes are observed independently of the
constructor result; the one complete request is decoded with the existing codec.

Every actor observation is captured without asserting. The parent shuts down and
**closes its owned peer, then joins its actor before assertions**, including the
observation-timeout branch. Shutdown errors stay in the diagnostic; closing the
owned descriptor and joining are the cleanup witnesses. A missed token-read
window is explicitly a harness miss. The negative cap (25 ms) and external
observation guard (100 ms) are caller inputs for these new stand-ins, equal to
the earlier client fixture policies; no old bound is changed. These are not
performance targets or production defaults. On macOS the already measured Hello
wait may win first; it is not reset or widened to manufacture an unarmed case.
Linux's measuring table can show caller-cap behavior; result attribution must
retain the actual refusal. Prequeued positive Hello uses the fresh request id 1,
which is independently checked against the actual emitted request.

This source defines no ongoing connection timeout and performs no workload.
The evented positive checks the same Link owner, token and stored configuration;
it is not a real native event-loop or workload lifetime proof.
