# Retain original authority through unfinished Proc Drop (AC-T7.4.1)

Fixed source parent: `e852c1b5ba9e1e2c8eb122dfb5817cd4dc339748`.

ROOT executed the exact original 6753 control on that source: **0 pass, 1 fail**.
Raw log SHA256: `e3d2818db4d03b8d4296495ef2c0bb86bcf815f5dbee1d6f3632d823e9dfe7c0`.
The same sending guard was held, Drop began after the original instant, the
observer result timed out before cleanup and returned only after guard release;
Release count 1, other count 0, actor joined and state cleaned. This is ROOT's
reported actual control, not a test execution by this lane.

The sole production source change is in `calls.rs`. The existing RemoteProc
retains the explicit `run_before` effective caller/configured deadline for its Drop Release. The
same Conn sends with that original authority through the existing whole-message
sender guard and the existing partial-frame send. It checks expiry before
framing and again after acquiring the guard. Explicit Drop never starts a new
relative budget, and never substitutes None. An incomplete frame retains the
existing connection shutdown behavior when its bounded send fails.

Ordinary run/exec and event release retain the old `send_now` path with no
explicit deadline. Proc wait/kill, answer precedence, stream ownership and
connection ownership do not change. There is no new owner, worker, timer,
forget/leak workaround or physical-absence claim. A Release not sent before
expiry does not prove that the workload or native scope has ended; the caller's
cleanup failure remains uncertain. This does not extend the original deadline
into a workload completion timer.

The complete budget_tests leaf, including the original 6753 regression, is
byte-identical to the parent. ROOT owns its exact successor execution and any
additional partial-frame behavior controls. No constructor source is included;
that producer is a separate 5c10 checkpoint from the same fixed parent.

Policy: implementer; fully read Shards `7d7e0855a5073ed5163b4fe7ed3355c9ed7c2506`,
rules SHA256 `df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`;
four policy blobs unchanged, exact parent Hex CLAUDE fully read.

This successor is **source only**: builds/tests/native executions by L4 = **0**.

Effective-bound source correction to 2a1b: that checkpoint retained only the
original caller instant, dropping a smaller configured reply cap selected for
creation. Now explicit spawn_before selects the minimum caller/configured
absolute deadline ONCE. Both transport branches pass that same effective value
directly to the existing Conn::call_before, and RemoteProc retains exactly it.
Drop starts no new relative budget and cannot enlarge the earlier configured
limit. Ordinary None run/exec paths retain their old budget selection. Learned
answer timing still bounds the call itself, not Proc completion or lifetime.
This is a source correction requested by ROOT, not a new execution result.

Separate source gap remains: malformed Given settlement still calls ordinary
proc.kill(), which may start a fresh/unbounded wait before Drop. That code and
the Proc wait/kill implementation are byte-identical; no remedy is included.
The separate proposed original control requires real Run/Given with an owned
unasked descriptor, a witnessed withheld Kill answer, then owned connection
cleanup/join before the regression assertion. No such control was implemented
or executed here. Receiver/native effects remain uncertain after caller expiry.
