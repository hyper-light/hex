# Settle original fixture stall diagnosis (AC-T7.4.1)

Exact source: ROOT `9b44967ee57b0fc22ea18b6844e6780c4639b632`, whose budget
leaf is byte-identical to signed L4 2212. Full original raw and owned cleanup
receipt were read, hashed and copied unchanged here. The actual test compiled
and entered its sole selector, then did not finish within ROOT's unchanged900s
Cargo guard. There is NO completed count, no assertion/counter evidence, no
production red and no client Clippy result. Treat the original as INCOMPLETE.

ROOT recorded Cargo2011513 -15/reaped. The remaining owned test2059947 had the
recorded birth30607552, exact private executable hash5e106b84..., and the exact
owned unit. ROOT's pidfd SIGTERM observed a terminal event; the unit is now
not-found/inactive/dead with empty cgroup. This lane performed no PID, unit,
runner, cgroup, service or process action; it only verified the copied receipt.
Source0diff and temp0 remain the original collected facts. Reported two futex
and one ppoll threads are a ROOT observation, not an independently captured stack.

Concrete SOURCE hole: SettleOriginal::recv delegates Offers::recv (budget leaf
1879–1895 / 979ff). Offers drains the wake datagram and, with no queued message,
returns Ok(false). It never checks the Stalled.closed flag. By contrast the
existing Stalled::recv checks that flag and reports a typed terminal error.
Shared::end (calls.rs965ff) calls transport.shutdown BEFORE close. If the reader
drains the one-shot wake and polls again before closing mail is observed, the
mock has no persistent EOF/readable condition to end that unbounded poll. This
is a fixture transport-contract hole consistent with a cleanup join/futex plus
reader ppoll. It is NOT proof that the actual stalled process reached this
interleaving; the current log has no phase counters and no live stack trace.

The source peer queues only actual Started. Taken is produced for Given; an
accepting StreamsReady is produced after Offered. Neither corresponding answer
is in this fixture. Thus a rejected Taken/StreamsReady is a useful hypothesis
to observe, but not an established cause. Silently accepting new protocol words
would change the experiment without evidence. Unexpected words should be
recorded and remain explicit until that observation is reviewed.

Proposed fixture-only successor: after draining its queue, SettleOriginal must
observe its SAME closed flag as a typed transport error, matching existing
Stalled closure semantics. Add bounded, phase-specific diagnostics for setup,
Started, actual request kinds, observation result, cleanup start/return, pipe
close and join entry/completion. Preserve the original cap/guards, malformed
end, Started/Kill/no-Exit/withheld-answer behavior and failing deadline oracle.
No production settle/kill remedy is proposed by this diagnostic checkpoint.
There is no relaunch, budget expansion, retry, new case or check dispatch.

The bf13 diagnostic checkpoint kept source unchanged; original2212 and raw remain immutable.
Policy implementer7d7/rulesdf2/full current Hex CLAUDE receipt retained. New
builds/tests/native effects by L4:0. ROOT decides any next fixture-only intake
and reproduction after this source diagnosis; no result is invented.

ROOT-authorized fixture-only successor (source, not execution): SettleOriginal
still drains real queued answers first, then observes the SAME Stalled.closed
flag as a typed persistent transport error. It no longer treats consumption of
one wake packet as an open empty connection. Only this test transport changes;
production Shared/Caller/Proc and settle/kill remain byte-identical. Static
phase messages identify setup return, actual send kind/id, Started, Kill,
worker return, cleanup entry/return, pipe close and join entry/completion. No
periodic logging, new timer, new case or synthesized protocol acceptance exists.
The original regression assertion tail and all guards are byte-identical (hash
in fixture-source.json). Unknown Taken/StreamsReady, if ever seen, are printed
and still explicitly refused; no guessed protocol success masks the cause.
There is no next run by L4 and no production red/remedy claim. ROOT owns any
matched intake and reproduction after reviewing this tiny successor.
