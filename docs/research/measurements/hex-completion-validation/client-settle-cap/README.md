# Failed bounded setup retains its cleanup authority

L4 implementer source only, fixed ROOT `5a0a020b4f49073bc020818205739a893b059ac0`. Full policy7d7/rulesdf2 receipts retained; exact Hex CLAUDE matches the fully read a093 digest. `source.json` records source/protected hashes and exact original evidence.

ROOT's actual original failed 0/1 with full phase witnesses. Run/Started and the real cleanup Kill occurred before the original25ms deadline; no Exit or Release occurred. The worker did not return by the100ms observation and returned the original stream-settlement Broken error only after owned connection cleanup. The unchanged original leaf closed its owned pipe and joined before assertions; clean/join were true. This local copy matches the supplied3c63 log hash, and the collected archive matches cea019. The older9b44 incomplete/stall remains separately attributed.

The failed setup branch now dispatches Some(cap) to private RemoteProc::kill_before; None keeps ordinary kill. The cap is the SAME effective caller/configured bound already selected by spawn_before, not a new relative wait. The existing zero-duration wait uses that cap at each loop boundary so unrelated progress cannot renew authority between turns. Already recorded completion retains precedence. Kill then passes the same cap to existing Conn::call_before, covering queued send, frame progress and answer wait; existing learned policy may shorten it. No cap is reconstructed from transport policy.

The ordinary Proc trait kill/try_wait/wait_for implementation and Drop body are byte-identical. Their wait wrapper delegates with no cleanup cap. Failed bounded setup still returns its exact original refusal; failed cleanup is not positive evidence that the remote workload exited, stopped or was removed. The same RemoteProc/connection ownership drops through the already bounded Release path, without forget/leak/new owner. Expired authority cannot authorize an arbitrary new Kill.

No test source or guards change, and no new selector is added. ROOT should execute only the unchanged original after intake. The supplied mutation-hook.json removes ONLY the cleanup Kill call's retained cap, while keeping all phase preconditions, wait policy and guard values. ROOT owns actual mutation execution/restoration and source/binary/result attribution. Compile failure or a missed phase is not mutation rejection proof.

No builds, tests, mutations or native effects were executed by this lane. Prior HelloA/B and API/Index/merge/wire passes remain at their original cohorts and are not replayed here.
