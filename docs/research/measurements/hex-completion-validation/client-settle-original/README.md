# Settle-error Kill original control (AC-T7.4.1)

Fixed ROOT source: `25d98e88be3b8173605ed7e8e1d109d309a278b6`.
Own only an appended calls/budget_tests.rs fixture and this new evidence path.
The sole new selector is
`calls::budget_tests::absolute_run_settle_error_does_not_renew_kill_answer_budget`.
This is SOURCE ONLY: builds, tests, actors, native/GPU/VM effects executed by
this lane are all zero. ROOT alone reproduces this one new selector. No current
red, pass, receiver effect or cleanup result is invented.

The actual Core/RemoteProc path is driven by the existing private Offers/Stalled
memory transport, with its existing wire encoder/decoder. A deadline-aware
stream setup returns one real owned stdout pipe end when only stdin was asked
for. The memory peer sends actual Started(pid7), never Exit. The production
make/settle path sees that wrong end only after Started and produces its bounded
unasked-stdout refusal; no fake API or copy of settle implements this behavior.
The transport positively records Started delivery and real Kill request id/exec,
while withholding only the Kill answer under reply_budget=None and a private
Measuring answer table. No sender guard is artificially held here: that isolates
the new concern from the separate 6753 held-sender Drop original and 0e9 fix.

Source inspection shows RemoteProc::wait(None) uses a zero duration and polls;
it is not assumed to wait forever for Exit. The Kill send, when observed,
independently proves this poll returned and execution reached the later ordinary
Conn::call with an unarmed answer wait. If the regression result is missing but
Kill was never witnessed, the test calls that phase unverified rather than
attributing a block to wait(None).

The parent captures Started, Kill, actual original expiry and bounded result
observations. It then ends ONLY its owned Shared connection, drops its retained
pipe and joins the sole worker BEFORE asserting any phase or error result.
Poison recovery is allowed solely for closing this test-owned pipe guard; it is
recorded and fails the witness after join. No foreign PID or native owner is
created or changed. Returned timestamps, setup cap, outbound ids, zero Exit,
late result, pipe close, ended transport and empty call/program/given/waiting
state are journaled. The existing CAP25ms / OBSERVE100ms and existing until
helper are reused unchanged; no sleep, timer framework, timeout inflation,
random rerun, child process or new campaign is introduced. The external guard
is a fixture observation limit, not new authority for the production call.

The oracle still EXPECTS the original setup error result before owned cleanup
under the original caller authority. It does not turn the predicted original
wait into a passing expectation. Only if the result is missing does a real Kill
send become a required causal witness before the final failing assertion. A
future bounded remedy may return before sending a Kill; that is not forced to
reproduce the old behavior. The original typed unasked-stdout refusal must
remain the returned error. No production remedy is included.

The entire prior budget leaf is preserved byte-for-byte as a prefix, including
6753 and existing bootstrap controls. Production calls.rs (including current
0e9 effective cap and unchanged settle-error Kill), guest constructors/oracle,
Cargo and policy files are byte-identical. source.json records all exact hashes.
The 6548 ordinary constructor withheld phase stays separately UNVERIFIED; no
passed constructor or old unit is requested again.

Policy: implementer; fully read Shards7d7, rules SHA256
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`;
exact parent Hex CLAUDE compared unchanged to the fully read source. ROOT owns
joined compilation, reproduction, any subsequent remedy, audits and native gates.
