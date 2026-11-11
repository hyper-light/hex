# Deterministic Hello phase controls A/B (AC-T7.4.1)

Fixed ROOT parent: `9b44967ee57b0fc22ea18b6844e6780c4639b632`.
This adds exactly TWO source-only controls in the existing calls budget test
module. No production, strict public constructor fixture, timeout table, wire,
manager, SDK, dependency or existing test is changed. The entire prior leaf,
including the separate 2212 settle original, is retained as an exact prefix.
Builds/tests/actors/native effects executed by L4: **0**. ROOT owns their first
matched compilation, execution and coverage decision; this source is no pass.

A — `selected_smaller_learned_setup_expiry_prevents_send` — uses an actual
Conn::call_before and the already existing private Calibrated<false> test table
(1 ms recorded sample, hence a 2 ms learned bound; no new tuning input). The
existing per-call setup callback receives the actual selected ReplyDeadline,
reports it, and consumes that exact authority using the existing until helper.
It then returns normally so the real sender must refuse before try_send. The
test requires the selected source to be Learned and earlier than the original
caller instant, its learned send refusal, and zero write notifications. Stalled
commits the entire nonempty request on its first try_send, so an unconsumed
notification is a zero-send/zero-byte witness. This is not a guessed sleep or a
reconstruction of when selection happened. It demonstrates the allowed policy
branch; it does not reproduce the public Mac 590.582 us distribution or alter it.

B — `isolated_ordinary_stream_hello_withheld_answer_uses_original_cap` — owns
one actual guest::Stream and the ordinary Core::hello_before path. A test-only
value delegate forwards the SAME Stream's reading, handle, frame, recv, send
and shutdown, replacing only Transport::answers with a private Measuring array.
It adds no event pump, provider, native owner or copied Runtime implementation.
The ordinary stream is prepared through its existing test-only dial with a
complete prequeued token. That preparation is outside this isolated Hello's
caller cap; no bounded public dial/token/factory claim is made by B.

A real peer decodes the complete TCP length-prefix/body and exact Hello/build/id,
while a separate send trace counts successful actual bytes. It supplies NO Hello
answer. The actual Core must exhaust the original CAP as caller authority,
without a learned or renewed-relative wait. Frame observation uses the existing
protocol_ready helper and one original deadline for new waits; no dribble resets
it. Already available bytes may be collected as late observation evidence, never
as additional operation authority. The frame is bounded by the existing wire
MAX_FRAME and requires progress. Core/Stream owns its original connection and
shared owner; the peer and shared connection end and the sole actor joins BEFORE
any result assertions. A has the same owned end/join-before-assert ordering.

Only existing CAP25ms / OBSERVE100ms and the existing readiness/until helpers are
used. No global measure_all, calibration mutation, default timeout, inflation,
random rerun, ignore or weakened public branch is introduced. Missing full-frame
or selected-policy witnesses fail their controls; they are not inferred from a
return code. These are isolated seam controls, **B is not proof of the unseen
ordinary public-constructor withheld-answer phase**.

The full ROOT6548 negative log was read and its exact bytes are retained here:
SHA256 `527d6fa93f69e4d5a34d3bfe5e31a7952c62462120add64d32a9ab4e221ff5bd`.
That actual result is 0/1/0, with both partial-token absolute expiries, complete
Evented Hello/withheld answer, and Ordinary empty-byte learned-before-send.
Its strict missing-witness assertion and status UNVERIFIED remain unchanged.
The separate positive1/0 bothforms is not rerun or assigned to these controls.
ROOT decides contract-versus-phase gate coverage only after actual A/B results.

Policy: implementer; fully read Shards7d7/rulesdf2 retained. The FULL exact parent
Hex CLAUDE was reread, including its new same-clock Mac helper safety rationale.
Only the two new selectors in source.json are requested for ROOT's future review;
no native/check dispatch or broader acceptance claim is made by this lane.
