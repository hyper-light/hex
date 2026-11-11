# Original: typed Shards CONNECT cause loss

Status: **source only / UNRUN**. Parent Hex source is
`d92faeaf24e2c847b781aadb3fefdefeb2cfd451`. No compilation, test, native,
network, VM, build or mutation dispatch occurred in this lane.

The scoped requirement is AC-T7.4.1 and Shards R04: keep refusals typed through
the adapter and wire, with affected port, handshake phase and cause available
as data. This is an original control, not a production remedy. ROOT owns its
first execution, result classification and any later carrier design.

## Actual path and oracle

The new test-only child implements a bounded `shards_api::Guest` model and
calls the existing `ShardsLink::dial_before`, not the refusal mapper directly.
Its three immediate calls use the identical original `Instant` and existing
`hex_core::stream::CONTROL_PORT`. Every relative connect, identity, observation,
stop or wait method records an unexpected call and refuses. No stream, actor,
thread, socket, native owner or native process is created by the model.

The SDK inputs are:

1. `Connect { port: CONTROL_PORT, failure: Io { phase: Read,
   kind: ConnectionRefused, code: Some(111) } }`.
2. Legacy `Channel` with the same port and exactly the first cause's rendered
   presentation. The SDK variants differ, while their public displays match.
3. The first typed `Connect` with only its phase changed to `Write`.

`111` is an explicit synthetic raw-code input, not an observed host errno.
The one-second caller input follows the adjacent existing ShardsLink model
test. All fixture data is prepared before anchoring it; no wait, retry, actor
or relative fallback consumes it. No existing bound, deadline or guard changes.

Each actual adapter refusal passes through existing
`wire::snapshot_refusal`, `wire::read_answer`, and a canonical re-encoding.
The current wire identity is checked by the existing decoder; the test requires
the existing Refused tag, id zero, no descriptors, and exact decoded-adapter
equality. It does not implement a decoder, parse presentation, shadow the mapper,
name a future Hex variant or change any encoder/default.

Before the semantic oracle, all three exact SDK calls must be witnessed with
their original caller/port and no other SDK method. The phase-change positive
control must change both the decoded refusal and canonical bytes. Only then does
the original require the typed Connect and same-text legacy Channel to have
different canonical Hex bytes. At the pinned source the adapter wildcard maps
both to `Os` text: source inspection predicts this final equality will fail.
That prediction is **not an executed red**. A missing SDK call/deadline
precondition or failed positive control is not evidence of the target symptom.

Raw `--nocapture` output retains the exact caller Instant, SDK variant, port,
phase, kind, optional raw code, actual call trace, adapted/decoded refusals and
bounded frame bytes before the final assertion. This original proves the narrow
non-collapse property; it does not specify or validate all fields of an eventual
new Hex carrier. ROOT must assess that separate remedy contract and its controls.

## ROOT selector (not invoked here)

On ROOT's admitted private matched source/target/cache, nice 10, one build job and
one test thread:

```sh
cargo test --locked -p hex-client --lib guest::shards::connect_original_tests::shards_link_connect_cause_remains_distinct_from_legacy_channel_text_on_wire -- --exact --nocapture --test-threads=1
```

Intended selection: **one test**. Executed here: **zero**. Compile result,
semantic red/green, logs and binary hashes remain absent. Preserve the first raw
result, including a compile failure or precondition miss; neither is a semantic
typed-loss reproduction. No old v5, snapshot, settle, A/B, constructor, Drop or
native-gate test is selected by this command.

## Policy, scope and evidence limits

Implementer: L4 `01a1c7676cfc7297a41236a8e71ee188`.
Coordinator: ROOT `01a1aa77778471339bea3beff90d6bbd`.
Current Shards policy `bff3015182ad754274f1048ce393ac12cb6cd91f` was fully read
(AGENTS, R01–R16, development evidence, CLAUDE); all four blobs equal fully read
`7d7e0855a5073ed5163b4fe7ed3355c9ed7c2506`. Rules SHA256:
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
Fixed d92 Hex CLAUDE, current charter and validation brief were read before edits.

Ownership is only the new `guest/shards/connect_original_tests.rs`, an appended
`#[cfg(test)] mod connect_original_tests;` registration in `guest/shards.rs`,
and these new documents. The parent's complete pre-existing bytes are retained
as an exact prefix. Production/API/wire/defaults/Cargo and old tests are unchanged;
the manifest pins the primary source inputs and named source outputs.

R01/R04/R06/R07/R13: only test code and evidence are added; model states and the
last semantic assertion are explicit. R02/R03/R05/R12: reuse the pinned SDK Guest,
existing adapter and canonical codecs; no new architecture, provider, wait/cache
or runtime constant. R09/R10: original only, actual reproduction and any remedy
remain ROOT work. R11: no supported-target compilation or native coverage is
claimed. R08/R14: no performance/allocation/fault claim or measurement. R15/R16:
no delegation, campaign, runner transfer, shared-source writes or cleanup action;
all previous sources/raw remain intact. This source receipt is not readiness.
