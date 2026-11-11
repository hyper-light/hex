# Continued joined checks

AC-T7.4.1; ROOT coordinator; ca3/df2 policy. These are exact archived results, not final gates.

| Source | Scope | Actual outcome |
| --- | --- | --- |
| 9c41481694ad5b2747ea47fa335fc14a5aaeb5d5 | Windows x64+arm64 Runtime/Node production Clippy | 101 at seven Runtime lints; Cargo48111 reaped, no tests/native. The first two compiler errors fromC3 were resolved; first target still stops before claiming both architectures passed. |
| e1cdcd683fd641650fe0117c4ef8dc646f6f0b9e | New cost-profile/evidence controls | 2 passed/0failed/0ignored, Cargo49022 reaped; log622588e197e0b98ec1e9a77a69f4968fb5df1aad26c709d3e414c332185ce41a. No native effects. |
| samee1 | Runtime all-target Clippy | 101 at production FC/GPU lints and three test-only lint issues, Cargo49397 reaped. Original dce49725 log retained; no repeated cost controls for correction. |
| b5b67b383e3cb2746efa330687fdd911275b5d6c | New sole-provider RPC/unknown-load Runtime+Node lib compilation | MacARM64 and Linuxx64 both0; Cargo50366/249028 reaped; source unchanged/temp0/native0. Logs0e6fdc776c601034a1bac23de496b3013efd65060c68bea42524d13b4b3e04d3 and97abc7955ef3102e3de43a8bfec41fd0521697d423b332d3155ebd7cc2cf7538. |

Every run used explicit needs1CPU/4GiB/12GiB, nice10/jobs1/testthreads1, Rust1.99.1 and new private source/target/cache/temp. Only verified public registry tarballs/index metadata were seeded. Each data-only collection verified gzip member bytes and exact Git-archive input/source-post manifests. Local collector33801/33800 returned0/reaped. Full commands, targets, source hashes, PID/results and provenance are retained in raw archives. Original sources, build caches and uncompressed logs remain intact.

7345ca2a imports the named owners' minimal lint/terminal-owner corrections. dacdc8a6 additionally preserves the original guest test module/oracles while meeting test lints. Checks on those later source bytes remain separate. No old success is promoted into final native/cold-recovery/GPU/VM/main readiness.
