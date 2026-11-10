# Current borrowed connector contract (source checkpoint)

ROOT Hex coordinator01a1aa77778471339bea3beff90d6bbd. Full current Shards policy
at a920c85b5c0c9d40aafc4f4cce32432c7c0aed79 read; R01–R16 hash
df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.

The peer reviewed an older18,736-byte c55 patch. That artifact is retained as an
obsolete proposal, not a current merge request. Current vendor33df already uses
Ready for zero, rejects MAX/overflow before identity/dial, starts one absolute
deadline before identity, and shares the CONNECT/OK path with remaining-time
updates for each partial I/O. Public owned Guest::connect no-clock compatibility
remains separate. VendorAPI source is unchanged through upstream a920; Hex's
three prior patches concern WindowsARM recognition, sharedGPU enable and stale
backend conformance, not borrowed production behavior.

Existing canonical upstream tests cover zero-withheld on Unix, overflow/no-dial,
identity mismatch, withheld/dribbled answer, bytes/clone/half-close, typed replies
and raw readiness. They were previously compiled, not selected by Hex's GPU API
check. ROOT must execute them now; source inspection alone is not a passing gate.

The new ignored public API test file adds a real owned test-process stand-in:
stream+clone drops leave its PID/start and socket directory alive, a reconnect
works, then the explicitly ended child yields a Process refusal before dial.
The child is this same test binary, requires an explicit fixture environment,
and waits for stdin EOF; no Guest/adoption/watch/export or real VM is used.

Windows-only ignored repeated test requires HEX_BORROWED_CASE_ROUNDS, positive
and at most64 (the u64 identity bit width). ROOT supplies the workload, one
wrong-start bit per round. After initializing each path once, every complete
round must return exactly to GetProcessHandleCount baseline: wrongstart,
missing socket, MAX-before-identity, zero-withheld one attempt, and successful
connect+clone+drop+server join. Query/synchronize handles and sockets have no
permitted drift. A held reply is never answered before zero returns. A Windows
connect that is still pending may refuse during Dial; no forced Read-phase
assumption replaces readiness semantics. No absence, timeout or skip counts as
pass. Root external whole-invocation guard is mandatory for all blocking fixture
operations and child cleanup. Native OS/kernel leak freedom is a separate claim.

Microsoft GetProcessHandleCount/GetProcessTimes contracts were read from official
Win32 documentation. The pseudo-handle is borrowed; each Child handle remains
owned for its query. MacOS/libproc start-abstime and Linux proc/stat starttime
match the existing API identity formats, independently queried for the owned
fixture. Every minimal test-only unsafe block states its native ownership/ABI.
No new production constant, API, dependency, watcher, cache or native import.

This checkpoint is SOURCE ONLY; tests/builds/native calls0. Planned ROOT admitted
checks: canonical API borrowed cases and owned MAX/zero compatibility on Mac,
new public process-lifetime check, native Windows repeats and public lifetime,
then supported-target compile/Linux native when admission allows. Exact Linux
unavailability remains unverified. Required code/evidence belongs to ROOT, not
subagents. No laptop builds/tests; all targets/cache/temp private, no runner/VM
pause changes and no unrelated process stop. Preserve all original patch/logs.
