# ROOT same-fixture deadline comparison

Exact signed base9b68269c, exact prepared fixture1a0be87518194ba141359eedfd0c1f124cb436d8a064098af9fd04c335e486ec in all four fresh immutable composite sources/EMPTY targets. Linux1CPU4GiB12GiB, nice10/jobs1/threads1; scopeCPU100%/memory4GiB/swap0/pids256. The only comparison difference is declared/hash-checked calls.rs; every variant full before/post manifest equals exact base except that one declared file, independently rechecked after collection. No native VM/GPU/backend call.

| Composite | Actual selected result | Relevant witness | Log SHA-256 |
| --- | --- | --- | --- |
| before69a, calls38ae3313 |0pass1fail0ignored, Cargo101|actual Run/setup/Started/Kill before25ms; no Exit/answer/release; original error returned only after cleanup|84f15a1a1f1ccf64c387bee2a881a63b4573e242e68739c120deca1173573a01|
| after69a, calls2274b7d4 |1pass0fail0ignored, Cargo0|same phases/budgets; original error returned under original Kill authority|1bf328d6df7a849881c0b42e34beb71dbb27a0382bd3cdd5f369b6bed0dfef99|
| missingKillcap, calls8b6d7613 |0pass1fail0ignored, Cargo101|only Kill cap removed; same relevant delayed-error oracle fails again|32ba0a3375ccb8078c25dfe17c450183febeab686e126f524665d05d21c723e2|
| restoredafter, calls2274b7d4 |1pass0fail0ignored, Cargo0|fresh source/target contains exact restored bytes, phase and cleanup pass|dc6ff58a298ffe56432c163f070cf970c52651a5c4b4105d4f8cc16c0d0d0909|

All four report PHASE_WITNESSED/cleanOktrue, actual1 selected, every Cargo reaped; no guard timeout, source changes or remaining private temp. Test/binary/source hashes and exact commands/PIDs are in result.json. Expected-before/mutation red is a successful comparison result, not workspace-green. Overall driver0 means these four declared expectations were met. Raw1572428B SHA472f2d763752601f0ec478c9efd3cfc113d256e2003d359b380b8ef7c240a56b, allmembers/sourceverified.

This separately pinned successor is not a relabeling of original5a red or231d precondition miss. Their exact source/raw remain preserved. Caller25ms/observation100ms were not increased and all first diagnostics occur after close/join. No broader public-constructor phase, whole payload deadline, native portability, zero-allocation, resource/performance or full-gate claim follows.
