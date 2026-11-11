# Shared Venus API: focused result

Tested API source `3f2e9fb10e4caca0de5114e1dd5471540b17e217`, fixed Hex
base `0ba6a60f582d461e65673ad736b2ac658cee9b72`. Rust source unchanged after
that signed source-first checkpoint. ROOT reports importing it atedcc7280,
adapting outside GuestSpec/GuestConfig and recording0002 separately.

**r26: build0;4selected,4passed,0failed,0ignored.** Pure argument/bounds checks
and one simulator model receipt. No VM, native GPU, real start/attach or Vulkan
effect occurred. Shared Venus enable is not physical selection/exclusive binding.

| Exact selected case | Actual observation | Log SHA-256 |
| --- | --- | --- |
| client::tests::a_spec_is_the_flags_shards_run_reads | Existing exact false argument list retained, both init forms. | 7b02f16ed7b17a49e307513286946ff162f0fb3469a72fbefafc12308e22dba9 |
| client::tests::shared_venus_is_enabled_only_when_requested | True list equals the complete false list plus one --gpu, both init forms. | 4208e6456091cb5c4612301a134348718e27bf9c4f2bd11a12d7fbe4a09442ce |
| tests::bounds_are_refused_by_name | Both bool states accept legal bounds and refuse zero CPUs/odd/low memory. | 126440ff0ac4eca34ca7f220d7c9edf9a6925e662e0f7ee6699156542e3e09ba |
| the_simulator_retains_shared_gpu_enable | Both requested bool states survive full Spec equality; model power-off/stop/drop before final assertions. | 4b937a96dde58a15e73e6f3d8a2be4f8e9444394fef2482bb30116bcf629175f |

The real.rs harness was compiled, never invoked even for --list. No ignored real
test is counted as passed. Full conformance, allocation/descriptor/process
campaigns, native hardware, all-target gates, profiling and benchmarks were not run.

## Preserved failures and exact composition

| Job/private copy | Actual result | Retained cause |
| --- | --- | --- |
| r24 /v1 | Cargo101 before Rust compilation;0tests | Fresh offline registry lacked existing pinned triomphe0.1.17. |
| r25 /v2 | Cargo101 at API unit compilation;0tests | Normal upstream client.rs references missing managed-borrowed-nowait/focused.rs in the fixed vendor import. |
| r26 /v3 | API unit/sim/real harness compile0;4exact tests pass | Actual API files remain3f; exact canonical MAC include closure copied only into private snapshot. |

No production/API correction was needed between these jobs. All three source,
target, registry and raw results are preserved separately, with no test source
stubbed/disabled and no increased wait/resource limit. Public Cargo registry
access onv2 fetched the existing dependency; manifests/dependency requirements
did not change. v3 reused only this lane'sv2 registry clone and compiled offline.

The private input includes EXACT upstream33df focused.rs and contract.rs at their
declared paths. Their hashes are in [missing-fixtures](missing-fixtures.json);
both are byte-identical to ROOT's later authorized03f19b891417f2f87de331baf359c1fdb612c5e4
copies. No GuestSpec literals or additional include dependencies occur in them.
Their borrowed-connector test functions were compiled but never selected/executed.
No live snapshot was mutated. [Provenance](fixture-provenance.json) verifies all
five ROOT canonical copies; the three Linux-only files were not added or counted
as compiled in this Mac check. No actual worktree/vendor-doc file was changed.

ROOT separately reported reproducing/fixing the stale Windows ARM conformance
exclusion. This lane preserves0001 backend_of byte-for-byte and did not adopt or
count that separate ROOT fix. The full simulator conformance suite was not run.

The four-crate vendor scaffold has no checked-in Cargo.lock. Each private workspace
was seeded with EXACT upstream33df Cargo.lock, SHA-256
`1ecb41e738c4c4133a1968873e424efce5b2a65899dd8f40f37ac6aaffec8133`.
Cargo pruned only private scope; every resolved registry version/checksum was
verified against that seed before tests ran. Private lock SHA-256
`9e22d484d2c2d4d3d02a6461cb14ed24562bc2462dce6f02f6dc5874cdb93d61`.
This standalone dependency set is not a combined Hex-lockfile gate. The only
generated file under each private source tree was vendor/shards/Cargo.lock;
every original API/input source hash was unchanged after the check.

The firstv2 proof readback mistakenly looked for a Rust compiler message in stderr
instead of Cargo's JSON stream and stopped validation. The archive had already
been safely retained; the reader was corrected to examine both raw channels and
revalidated the same archive bytes. No raw data was changed and no build/test reran.

## Exact execution, raw hashes and cleanup

Runner d1323898b255a3ed435eb83fa0f7b0cf; actual macOS26.4 arm64; Rust/Cargo1.99.1.
cpus1/mem4g/disk12g,nice10(actual20),jobs1/threads1,no-sync. Python3.13 used for
the admitted driver. NEW roots ~/.cache/hex-shards-venus-api-01a1c640-3f2e9fb1-v1,
-v2,-v3, each with private source/target/cargo/proof. No laptop builds/tests,
other-lane caches, credentials, root snapshots or runner/service changes.

Commands/argv/PIDs/exits/listings/source hashes are retained in each raw proof:
API cargo test --manifest-path vendor/shards/Cargo.toml -p shards-api --lib --tests
--no-run --message-format=json; offline onv1/v3. Each of four compiled test names
was verified as present once, then invoked with --exact/--nocapture/--test-threads=1.
The scoped local rustfmt/diff checks were0 before the signed source checkpoint.
No additional API tests, Clippy or unchanged conformance case was run.

| Lossless raw proof | Bytes / safe regular members | SHA-256 |
| --- | --- | --- |
| [v1 failure](proof-v1.tar.gz) | 91316 /25 | ca469421169bcd2060a0b6a276797878b8e7b8a4b448e84c9a5e7a360db7a41c |
| [v2 compile failure](proof-v2.tar.gz) | 81905 /25 | fca50c629843c861b9503c8664bd630aef0a5d9e0f54d01843c97a265ef08888 |
| [v3 focused green](proof-v3.tar.gz) | 88737 /39 | cebc6d7b067525e51d164c81574034d0c4f0c9e4b1edbed19062ecc48eaaf7e6 |

[v1 receipt](proof-receipt-v1.json), [v2 receipt](proof-receipt-v2.json),
[v3 receipt](proof-receipt-v3.json) verify every member size/hash, safe regular path,
API bytes against signed3f and canonical include bytes against upstream33df.
Raw EOF whitespace stays compressed and unchanged. All member hashes and commands
are in source-and-logs-v1/v2/v3.json and light-v1/v2/v3/. Original input archives
remain remotely frozen. v3 input158764B/43members SHA-256
`abdebf1f27e9e21febaf9e463b18b9d7374e6d4b35f905fc8cb0aa1ab922acea`.
All transfer/fetch local free-space checks exceeded30GiB (exact values in receipts).

API test binary SHA-256 `3b89abfcc5ffd38a658ade9a92da2f3628df8867e23fe53e546190c0af9edc75`;
sim binary `00e5f103b83d096022d2312fce7104458c7b3eb93281e9be00af596e687b54be`;
compiled-only real binary `2d2832fe464f8143541fe4a7fb31cc510d96f480513d639acb9a23e7f3647935`.
v3 build stderr SHA-256 `9afd63b070342b4629cc38aff68444bc3fd9fd2ff2dc1b6f5123022b29278a13`.

Cargo44746/45144/46246 completed/waited; exact tests46332–46335 waited/reaped.
No timeout/kill was needed. Model guest scopes were stopped/dropped after observed
PowerOff; no native guest/box/device holder/socket/hook actor was created by these
selected cases. No active check job remains; source/cache/target/binaries/logs stay
preserved. Native resource-return/profiling claims are not inferred from these checks.

ROOT owes combined current-source API/literal/vendor-record/platform validation
and every actual supported-OS physical selection/attachment/recovery/cleanup proof.
The next assigned C-filter lane begins only after its own full policy readback/ACK;
it is not a completed consequence of this boolean API exposure.
