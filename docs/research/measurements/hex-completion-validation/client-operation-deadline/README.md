# Absolute run/delete operation deadlines

Source only, based on exact ROOT1a9a92bf1a657260b2684bb1982f5b3bfb46c00a.
No compiler, unit, native, VM or GPU invocation. Checks remain held until ROOT's
joined source pin. Existing 753 owner/barrier results and the separate C producer
329/QA47ab remain attributed to their own sources; none is repeated or promoted.

The new concrete methods are inherent on the existing Core and therefore on
the existing native Client aliases and `guest::Client`:

```rust
pub fn run_before(
    &self,
    spec: BoxSpec,
    program: Program,
    stdio: Stdio,
    deadline: Instant,
) -> Result<Box<dyn Proc>, Refusal>;

pub fn delete_before(&self, id: &BoxId, deadline: Instant) -> Result<(), Refusal>;
```

Runtime's original run/delete methods and trait contract remain. No new DTO,
wire tag, dependency, connection, owner, worker, timer, cache or manager is added.
L2 consumes these concrete methods for its actual paid SAME-Pool observer
Program; its parser/config/artifact binding/stream-drain/lifecycle remain its
owned source and ROOT's native acceptance obligations.

## Behavior and existing seam

The absolute constructor stores the supplied Instant unchanged with an internal
CallerAbsolute cause. It checks expiry before call/program reservations and
stream setup. New delete uses BoxId::try_clone; run shares the existing run
boxing/spawn ownership path. The previous relative Link/Transport reply budget
remains configuration, not a connection lifetime deadline. An earlier configured
deadline can shorten the caller cap, and the existing learned wait can shorten
that result. Later/None bounds cannot enlarge caller authority. Equal caller
and learned deadlines retain caller attribution, as before.

Both spawn branches enter the SAME Conn::call_before. The guest MAKES_ASKED
branch passes its effective cap through make/asks_before and carries_before.
Those existing adapters pass the exact Instant to Link::dial_before and use the
same ReplyDeadline for each partial/nonblocking named-stdio header write. The
native/non-MAKES_ASKED branch passes the cap to control send/answer and the
existing offered-stream protocol. Measuring/unarmed learned waits cannot turn
an explicit operation cap into an unbounded path. No Instant::now()+remaining
reanchoring or progress-based renewal occurs.

The existing whole-message sender wait, reader/queued complete-answer precedence,
offered-stream ownership cleanup and sibling-progress logic are unchanged. A
complete answer already queued retains its existing precedence at expiry; the
deadline bounds further waits rather than asserting hard real-time return from
every synchronous OS call. CallerAbsolute is excluded from learned ran_out
calibration, as relative caller caps already are. No deadline is stored in the
returned Proc or in workload stdio; Proc completion still uses its existing
wait_for argument. L2's one original outer deadline must also cover its actual
output drain, completion and cleanup.

Timeout/refusal is typed Refusal, never positive physical absence. A request may
have reached the agent before its answer was lost. Forgetting a local caller or
closing its local made ends does not prove remote workload/native termination,
release canonical claims, credit host capacity or authorize a substitute VM.
An expired delete_before does not acquire a new cleanup budget or automatically
retry; uncertainty remains with the existing canonical owner.

## Focused source controls, all UNRUN

The existing budget-test leaf gains exactly these five tests. Prior test bodies,
CAP25ms/OBSERVE100ms fixture bounds and registration remain unchanged.

| Exact name under calls::budget_tests | Control |
| --- | --- |
| `absolute_operations_reject_expired_before_effects` | Both new methods refuse an expired Instant with zero call/program/given reservations, writes and connection damage. |
| `absolute_operations_cap_unarmed_answers_without_link_defaults` | Actual Core run/delete requests are sent using a dedicated Measuring answer table and no transport cap; missing answers end by caller cause, with no learned timeout sample. Both owned actors are closed/joined before final assertions. |
| `absolute_delete_cap_covers_queued_sender` | An existing sender guard remains owned while new delete expires without writing, taking the guard, closing the connection or claiming remote absence. |
| `absolute_caller_limit_keeps_original_instant_and_smaller_bounds` | The actual deadline type keeps exact supplied Instant, accepts only earlier configured/learned bounds, preserves tie attribution and leaves None unarmed except for the caller cap. |
| `absolute_run_caps_named_stdio_and_preserves_headers` | Actual guest Stream/Link path with no Link reply default, preconnected owned loopback peers, all three actual named headers and an independently read complete Run request with a withheld answer. Every named dial receives the SAME cap no later than original; names join that exact request ID and made ends close on caller expiry. No process/VM executes. |

The named control provisions its owned sockets and token before starting the
operation. No server worker is needed: the one client actor always has a finite
observation cleanup, is joined before header assertions, and subsequent peer
reads have the unchanged observation guard. Any scheduling miss before the
required dial/header/send witnesses is retained as a harness miss/failure, not
accepted as the intended answer-wait symptom. There is no timer extension,
sleep or guard weakening. These test threads are not new production workers.

ROOT may run ONLY these five new exact cases and scoped client Clippy after
publishing the joined pin, with existing admitted d132 private1CPU/4GiB/12GiB,
nice10/jobs1/threads1/no-sync requirements. No invocation is authorized or claimed
by this source record. Preserve source/log/binary/archive hashes, selected
counts, failures, actual owned PID/actor cleanup and temp/source outcomes. No
old 11/wire/native batch is requested.

## Scope, policy and remaining evidence

L4 is implementer, current Shards0a6d91b3de0f10a8397868aa3794adab1e09afdc;
all four policy files remain equal to the fully read policy. Rules SHA256:
df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.
Full exact ROOT CLAUDE/current relevant client source read before proposal and
edits. Branch cp/t7-client-operation-deadlines is isolated from the frozen Link
owner, C/QA, SDK upstream and other validation branches. Actual writes are only
calls.rs, appended calls/budget_tests.rs and this new validation document/manifest.
No borrowed connector/upstream, runtime/backend, wire, Cargo, native transport
constructor, helper codec, factory or L2 observer file is edited.

| Rules | Source account and unresolved work |
| --- | --- |
| R01/R04 | New deadline state is Copy, expiration/invalid transport budget is typed and reservation-free, delete ID copy is fallible. Baseline run/Proc allocations and ownership stay on the same path. Compiler/lints and all five controls are unrun; no total panic-free claim. |
| R02/R03 | Uses exact existing ReplyDeadline/Conn/transport protocol sources and caller Instant, no tuning default. Relative/learned semantics and fixture bounds are retained. |
| R05/R06/R13 | Existing one connection/sender/reader/stream state, small shared spawn extension, flat bounded waits; no framework, cache or periodic probe. |
| R07 | Comments describe authority and uncertainty rather than narrated operations. |
| R08/R14 | No speed/allocation/leak/resource-return claim; ROOT owns actual profiles and comparisons. |
| R09/R10 | User-requested new optional API, source-observed absence of a public absolute method, no claimed native reproduction. Source/preserved-function hashes and owed control names are in source.json. |
| R11/R12 | Shared Core logic is used by existing OS/guest adapters; no platform-specific unsafe or new dependencies. ROOT owes joined platform compile/native use. |
| R15/R16 | Signed named-path source; no laptop tests/builds or foreign/runner/driver changes. ROOT owns joined checks, native acceptance, full gates/main. Old raw and all prior source remain untouched. |

Remaining: ROOT source review/joined compile, five exact client controls and
Clippy; L2 actual observer stream drain/cleanup integration; ROOT matched
artifacts, native deadline/cleanup/fault/resource proofs and full gates. This
source is not a completed observer or a native cleanup witness.
