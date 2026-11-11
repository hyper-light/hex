# Fixed c576 baseline driver — SOURCE ONLY

L4 implementer. Full current Shards7d7e0855a5073ed5163b4fe7ed3355c9ed7c2506 AGENTS/R01–R16/evidence/CLAUDE readback retained; rules SHA256 df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3. Hex CLAUDE/charter and current ROOT scope loaded. No build, syntax invocation, dry run, pure check, native selector, mutation or campaign was executed by L4. `root_native_gate_baseline.py` is the ONE new baseline driver. Earlier mutation/command notes and all original evidence remain unchanged; this driver implements no mutation.

Driver source: 26668 bytes, SHA256 d1b42b03247b336c47a6724d6640c114c89c9ceca81d521db637220585759531. This is a source hash, not a compiled or executed result.

ROOT alone dispatches with normal Kiln admission needs1CPU/4g/12g, nice10/jobs1/testthreads1 and its finite outer owned-unit guard. No admission/SSH bypass, runner/service/ancestor/driver changes, foreign process migration, installed helper or production manager. Disk12g is required external admission/headroom, not an asserted filesystem quota. Keep ROOT's actual admission and outer cleanup receipts with this driver's output.

## Exact immutable input

- Signed source: c576d4c2a0a37d7722a81174b9ab1d7ff1434ad5; source directory `/home/adalundhe/.cache/hex-lead/cohort-c576d4c2-linux-loader-gate-c576/source`.
- Already-built Node libtest: `/home/adalundhe/.cache/hex-lead/cohort-c576d4c2-linux-loader-gate-c576/target/debug/deps/hex_node-d6283f30893b31f3`, 185675000 bytes, SHA256 f8b10118b68414c0d9dc3faed95664bc8b248f7b27bd76e285dd0fbd2b8372d9.
- Runtime sys.rs SHA256 f90284ac37e877201342f3c170f4ae091a82269f460fbc47815706dd73f055e4; b039 native test leaf da4ba641b6122a402007aa8939cddb6154dbe114ac437ce47073afdae7431241. The driver also checks c576 API spec6c209d58... and Bounds54876217... in full. All constants are in its `SOURCES` table.
- ROOT reports the b039/70fd composite compiled0. This is not a native gate result. The driver checks the exact source/binary before any child, again before each selector and after effects; records its own source and `/bin/sh` content/inode pins. Its self-exec actor hashes and executes the same opened ELF inode, preventing a replacement of the binary pathname from substituting different bytes. Source/target/cache are never modified or rebuilt.

The two required operator arguments are only an exact fresh `.scope` name and a new absolute lab directory directly inside that fixed cohort. Existing paths refuse, preserving prior attempts; no retry or alternative binary/cohort/default is selected.

## Fresh user scope and safe controller distribution

Within the admitted ROOT command, a concrete launch shape is below. ROOT supplies fresh exact unit name, immutable transferred driver path and fresh lab basename; the strings are artifact names, not production tuning inputs. Do not run it from this implementation lane.

```sh
systemd-run --user --scope --collect --unit=ROOT_FRESH_NATIVE_GATE.scope \
  --property=CPUQuota=100% --property=MemoryMax=4G \
  --property=MemorySwapMax=0 --property=TasksMax=256 --property=Delegate=yes \
  nice -n 10 python3 /ABSOLUTE/ROOT/IMMUTABLE/root_native_gate_baseline.py \
  --unit ROOT_FRESH_NATIVE_GATE.scope \
  --lab /home/adalundhe/.cache/hex-lead/cohort-c576d4c2-linux-loader-gate-c576/g576v1
```

The initial unified cgroup must be exactly that unit, domain type, no child directories, empty subtree_control and cgroup.procs **only the actual driver PID**. Exact cpu.max ratio100%, memory.max4GiB, memory.swap.max0 and pids.max256 are required. All four cpu/io/memory/pids must already be available from the parent. No ancestor write or optional/noop controller path exists.

The driver creates `control`, moves ONLY itself into it, proves the original scope empty and verifies the active unit's actual `Delegate=yes` and ControlGroup by read-only systemctl query. Only then does it enable `+cpu +io +memory +pids` on that SAME fresh scope. No-internal-process constraint is a kernel requirement: [kernel cgroup-v2 documentation](https://docs.kernel.org/admin-guide/cgroup-v2.html#no-internal-process-constraint), plus its [top-down constraint](https://docs.kernel.org/admin-guide/cgroup-v2.html#top-down-constraint) and [delegation model](https://docs.kernel.org/admin-guide/cgroup-v2.html#model-of-delegation). Missing controllers, a foreign process, preexisting child, wrong scope/limits or permission failure refuses before native selectors.

Each of the three fixed selectors gets an initially empty `native-N` parent and `actor` leaf inside this SAME private scope. The SAME Python driver is re-executed for one minimal pre-exec step: verify its original parent PID/birth and exact phase, move only its OWN PID from control into actor, then exec the pinned Node ELF. This creates no new helper binary, native owner or manager. It avoids a racy post-launch PID move and leaves the fixture's delegated_parent empty. The original180s phase expiry is passed unchanged through this preparation and checked immediately before Node exec. Only that phase's self-placed actor and its descendants can occupy its new subtree; timeout cleanup targets that subtree, never a service ancestor or a PID found by text matching.

## Exact disposable test inputs

The generated config uses the actual current API:

- `ClassSpec::Firecracker(OciSpec)`, serialized as `{"firecracker":{...}}`; owned absolute empty rootfs Dir; valid init Program `/bin/sh -c 'exit 0'`. The initializer is metadata for this legacy managed stand-in, never an actual Firecracker boot. devices[], ownerNone, allocationNone, routeNone; no synthetic admitted profile or backing receipt.
- All eight actual Bounds fields are present: cpu_max_milli1000, memory_max67108864, pids_max256; cpu_weight/memory_high/io_weight/io_latency_us/io_max null. This is the explicitly authorized disposable test payload within the enclosing4GiB/100%/256-task job, not a production allowance.
- Actual `/bin/sh` marker Program: `printf '%s' "$2" > "$1"`, with exact substituted marker/token, no newline, empty env, cwd/user null. Its actual bytes and inode are pinned before and after; no host binary is changed.
- pidfd_budget2; guard_ms60000; gate_budget_ms120000, preserving strict causal D<G with gate twice the guard; observation_ms1 provenance only, never polled; syscall271 x64 provenance only, never sampled; existing byte_limit1048576. No budget is learned/enlarged/defaulted by the driver.

Exactly the three existing ignored selectors run individually, once each, in this order, using `--ignored --exact ... --nocapture --test-threads=1`:

1. supervise::completion_native_gate_tests::completion_publication_refusal_never_executes_marker
2. supervise::completion_native_gate_tests::completion_durable_publication_releases_and_reaps
3. supervise::completion_native_gate_tests::completion_controller_parent_death_cancels_gate

Each has one original180s driver phase deadline including its source/config checks, actor setup, output and cleanup. Control calls use no renewed relative wait; stdout/stderr and pidfd are readiness-driven. Each stream has the existing1MiB bound with one retained overflow witness byte before refusal; no silent truncation/pass. A positive pidfd terminal event precedes the SAME direct child's sole blocking reap; Popen timeout busy-wait is not used. Native test source still owns its original ACK2/P<D<G/STOP/early127/canary/Store/native cleanup assertions.

A semantic failure may be followed by the other distinct selectors only after exact owned cleanup is positively complete; no selector retries. A cancelled driver, evidence I/O failure, missing phase or uncertain cleanup stops later actors. `attempted` and actual completed harness counts are separate; an entered but incomplete test is not a completed native result. PASS requires all three one-test positive results, both pipe EOFs, no fallback kill, exact reap, empty/removed phase subtrees, unchanged pins and final driver restoration. There is no VM/GPU/readiness or whole Shards-helper claim.

## Evidence and every cleanup/refusal branch

The fresh lab is deliberately retained. `journal.jsonl` is flushed/fsynced around actor creation, errors, results and cleanup. Per-case config/stdout/stderr/before/after/result, pins-before/after, unit query output, scope snapshots/restoration, result.json and final SHA256.json stay together. All first/sibling ACK/tail/Creating records, parent.stdout/stderr, original gate-result.json, original scope/commitment and available native PID/birth/reap/cleanup facts produced in kept gate-* labs remain byte-preserved and hashed. No missing raw native field is fabricated. A missed observer window remains unverified.

On output/deadline/observer error, raw partial data is preserved before the exact direct-child pidfd cleanup. The created phase subtree is observed, killed only if populated, then awaited by actual cgroup.events POLLPRI notification under the SAME phase deadline. Positive native empty observation precedes its direct-child reap/removal; unknown emptiness/reap or an expired guard stays explicit. Any fallback kill makes that case fail even if cleanup later succeeds. Evidence I/O errors are retained as failures without bypassing cleanup. Fatal external SIGKILL/storage failure cannot guarantee a final receipt; ROOT's exact owned-unit guard and partial journal remain necessary, never an absence claim.

After all owned phase subtrees are removed, the driver proves only its control leaf/self remains, disables only the four controllers it enabled at this private scope, moves ONLY itself back into that SAME scope and removes its empty control leaf. Original resource limit strings, sole PID and empty subtree state are checked and recorded. Foreign/unexpected children/processes refuse restoration rather than move/signal them or write an ancestor. The driver cannot observe its own later death: ROOT collects the exact systemd unit terminal/empty state externally after driver return.

All driver code, supplied commands, pure parsing and native outcomes are UNRUN here. The existing92cf source review and earlier R10 results remain separate, unchanged evidence. ROOT reviews this source, records the driver hash and dispatches with normal admission; L4 does not invoke even --help, a syntax compiler, native selector or mutation.
