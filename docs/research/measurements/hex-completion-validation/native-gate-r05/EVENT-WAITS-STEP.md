# Test-only STOP and Child-end event step

SOURCE ONLY/PARTIAL: the old gate /proc oracle and its until/sleep are deliberately unchanged. That final known R05 strict blocker awaits the ROOT/L1 test-only ACK contract and causal-oracle review. Do not dispatch a duplicate strict check against this incomplete closure. No green/ready/native claim is made.

Fixed parent26b7820f22d5bc98eb2c443cbe3bf587f0e5ae55/e028 code. Only completion_native_gate_tests.rs and these docs change. Exact protected/source hashes are in event-waits-source.json. Current full7d7/df2 policy receipt retained.

The acknowledged Child-end wait now checks an already-cached Child status once, otherwise awaits the one retained exact parent pidfd with existing poll_exit under the supplied absolute end, and finally lets that SAME Child consume its exit. There is no repeated try_wait/timer or second parent reaper. Parent pidfd is opened once after spawn while this Child owns the unreaped process; failures remain cleanup uncertainty.

Before the actual STOP wait, the existing owned same-binary controller-parent emits an identities phase. This is only availability of the existing identities, not a claim of the pending native prepared ACK. The outer controller validates/acquires exact pidfds first. The parent then uses blocking waitid(P_PIDFD, STOPPED|NOWAIT), validates stopped()+SIGSTOP, and emits stopped-ready with exactly the same identities. Its Supervisor remains EXIT collector while alive; the outer actor consumes adopted native exits only after parent terminal. No new waiter thread/signal framework is added.

The original readiness reader now carries a cursor and one explicit end across both phases, reusing the same fd/readiness and total byte bound. It handles complete lines, so coalesced phases are not dropped and partial UTF8 is not parsed prematurely. The first protocol line starts after a newline to separate any libtest prefix. Encountered duplicate/out-of-order phase lines refuse. No arbitrary polling/retry or refreshed phase budget is introduced.

The same configured guard is anchored to began BEFORE spawn; actual parent terminal must precede that SAME outer_end. Since callback guard timers start later, they cannot expire before this terminal proof. Overflow is checked before changing subreaper state. Cleanup still uses its existing finite cleanup guard, signals only owned parent/native pidfds, closes/reaps and restores subreaper state before final assertions. A missed window remains uncertainty.

Both publication-refusal/positive cases, the exact ppoll predicate, earliest-gate/first127/still-stopped-sibling/CONT+sibling127/canary oracles and three ignored registrations are unchanged. No producer method/stub, production DTO, global FD, default or allow has been added. L1 confirms the actual native ready2 ordering; ROOT's dev-only registration/feature decision remains necessary because dependency cfg(test) alone is insufficient.

No build/test/native/mutation/check dispatch occurred here. Scoped rustfmt and source-integrity inspection only. The next concrete boundary is the matched test-only producer plus reviewed gate phase replacement; ROOT owns joined compilation and eventual native/fault/mutation acceptance.
