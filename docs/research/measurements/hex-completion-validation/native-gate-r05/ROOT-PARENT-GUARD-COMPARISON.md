# ROOT-only parent guard comparison — SOURCE ONLY

L4 implementer; retained full7d7e0855a5073ed5163b4fe7ed3355c9ed7c2506/rules df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3 readback. No prepare/seal/helper import, syntax check, compile, native cell, mutation or restoration was executed by L4. This driver is a source/command handoff for ONE ROOT-dispatched comparison batch, not a readiness result. Old a128/c576 actual3/3 and archive83ef3375... remain unchanged and are not replayed or relabeled.

Comparison driver source: 24652 bytes, SHA256780e113dfed6063e6d1a67f23d8cdb4568e49f0ec0a9796c9303b1fe842ff6b2; source identity only, UNCOMPILED/UNRUN.

`root_native_gate_comparison.py` uses the immutable a128 `root_native_gate_baseline.py` (SHA d1b42b03247b336c47a6724d6640c114c89c9ceca81d521db637220585759531) beside it for existing config, stdout/stderr/pidfd, self-placement, cgroup observation and cleanup primitives. It never calls the old three-test main or old c576 pin validator. Per-cell compiled ELF values are supplied only from a sealed actual Cargo artifact. No installed helper, native manager, new picker/owner, production switch or FD exposure is added.

Required common leaf is signed5b033f7954bbe856c966bf2ca4335969a373c05a, SHA20e5512d525aae24cdfc58537875d37c48e4386b311d863fcb774d4a245359fe. ROOT must first integrate it into its coherent signed source cohort. Its sys.rs must remain originalf90284ac37e877201342f3c170f4ae091a82269f460fbc47815706dd73f055e4. A changed leaf/sys refuses; no private compatibility overlay or floated source is accepted. ROOT supplies the exact full regular-file source manifest and its separately reviewed digest from that signed archive; source pin alone is not proof of archive identity.

## Prepare three fresh copies, not three mutable states

All uppercase variables below are ROOT's existing absolute reviewed artifacts/normal-admission inputs, with no defaults. Use a short fresh private comparison root (for example `/home/adalundhe/.cache/hex-lead/gc576v1`) to keep the original fixture's socket metadata paths short. Source, targets and caches stay outside shared checkouts. No command here runs in this implementation lane.

```sh
python3 "$TOOLS/root_native_gate_comparison.py" prepare \
  --source "$ROOT_SOURCE" --source-pin "$ROOT_SIGNED_PIN" \
  --manifest "$ROOT_SOURCE_MANIFEST" --manifest-sha256 "$ROOT_SOURCE_MANIFEST_SHA256" \
  --output "$COMPARE"
```

Preparation checks every source byte against that reviewed manifest, then creates baseline/bypass/restored/source, initially EMPTY target/cargo/temp directories, and source-before.json per cell. New files get current mtimes; no old target/dep-info/binary is copied. Baseline and restored source manifests are equal in full. Bypass differs ONLY in this exact unique held_child call (before ready2/wait1):

```diff
-        && held_close_unexpected(&keep).is_ok()
+        && {
+            let _retained_guard_body = held_close_unexpected;
+            true
+        }
```

The reference retains compilation of the original helper body/type; it is not called. Join, actual ready2/peek2, held wait, gate authorization and all test code are identical. The restored cell is a new exact original copy, not an overwrite of the failed mutant; both remain inspectable. This is the meaningful **whole inherited-FD isolation guard** mutation even when own and foreign writers both remain. No foreign-only attribution/precondition is claimed.

## ROOT compiles only Node libtest, then seals real artifacts

ROOT uses normal1CPU/4g/12g admission, nice10/jobs1/threads1 and its existing finite build/whole-owned-process guards. Confirm retained copies/targets fit admitted disk; preserve failures, do not enlarge allowances or reclaim foreign data. Seed only reviewed public registry archives/index into each new cargo directory if needed; never copy target, incremental/dep-info, generated executables or credentials. Execute each Cargo command INSIDE ROOT's finite build guard and record the actual PID/birth/reap, toolchain/environment and exit. This shell shape does not provide or waive that outer guard:

```sh
for CELL in baseline bypass restored; do
  (
    cd "$COMPARE/$CELL/source" || exit 1
    env CARGO_TARGET_DIR="$COMPARE/$CELL/target" CARGO_HOME="$COMPARE/$CELL/cargo" \
      TMPDIR="$COMPARE/$CELL/temp" CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 RAYON_NUM_THREADS=1 \
      nice -n 10 "$CARGO_PROXY" test -p hex-node --lib --locked --no-run \
        --message-format=json --target x86_64-unknown-linux-gnu -j1
  ) >"$COMPARE/$CELL/cargo.jsonl" 2>"$COMPARE/$CELL/cargo.stderr"
  STATUS=$?
  printf '%s\n' "$STATUS" >"$COMPARE/$CELL/cargo.exit"
  test "$STATUS" -eq 0 || exit "$STATUS"
done
```

Retain prepared.json's actual digest, then seal:

```sh
python3 "$TOOLS/root_native_gate_comparison.py" seal \
  --plan "$COMPARE/prepared.json" --plan-sha256 "$PREPARED_SHA256" \
  --output "$COMPARE/sealed.json"
```

Sealing requires actual Cargo0/build-finished true, one Node libtest compiler-artifact with fresh:false, the exact manifest/lib source paths inside that cell, executable inside its new target and generated-file list, and unchanged full source bytes after compilation. It retains raw-log, stderr, source-after and ELF hashes/bytes. It does not accept a current-source manifest plus a stale cached ELF as proof. Use the actual sealed.json digest in all three native invocations. No compile failure is a mutant result.

## One original parent-death selector per fresh scope

For baseline, then bypass, then restored, ROOT launches the SAME comparison script with separate exact fresh user scopes and fresh labs directly inside the respective cell roots. Each uses the a128100%CPU/4GiB/swap0/pids256/Delegate=yes setup, initial sole driver PID, own control leaf before four-controller enable, then one own actor leaf. No ancestor/service writes or foreign migration. All original disposable test inputs remain: CPU1000, memory64MiB, pids256; pidfd2; guard60000/gate120000; provenance observation1/syscall271; existing1MiB bounds. Only test binary hash and private artifact/cgroup paths necessarily differ per cell. Each native actor has the original180s phase cap through self-exec/stdio/reap/cleanup.

```sh
for CELL in baseline bypass restored; do
  systemd-run --user --scope --collect --unit="hex-root-gate-$CELL-v1.scope" \
    --property=CPUQuota=100% --property=MemoryMax=4G --property=MemorySwapMax=0 \
    --property=TasksMax=256 --property=Delegate=yes \
    nice -n 10 python3 "$TOOLS/root_native_gate_comparison.py" cell \
      --plan "$COMPARE/sealed.json" --plan-sha256 "$SEALED_SHA256" \
      --cell "$CELL" --unit "hex-root-gate-$CELL-v1.scope" --lab "$COMPARE/$CELL/l" || exit "$?"
done
```

Run these only under ROOT's existing admitted finite outer guards. Exactly `supervise::completion_native_gate_tests::completion_controller_parent_death_cancels_gate` runs in every cell; neither of the other two old native passes is replayed. A cancelled/evidence/cleanup/phase failure remains UNVERIFIED and stops the batch; no retry or budget change. External ROOT receipts must record each exact driver's terminality and removed/empty scope after return, which a driver cannot observe about itself.

## Outcomes and attribution

Baseline/restored require actual process0, one completed1PASS/0FAIL/0IGNORE, both stdout/stderr EOF, no fallback kill and positive private cleanup/restoration, plus raw original t0/D/G, exact owner/native identities and ACK2, real same-sibling STOP, P<D<G, actual first/sibling127 notifications beforeG, sibling still kernel-T after first, pre/post-cleanup canaries absent, and empty uncertainty. Bypass requires one completed0PASS/1FAIL/0IGNORE with nonzero libtest exit **and** that same real held phase/P<D/G/STOP, first terminal guard failure observed beforeG, no first early127, exact native SIGKILL cleanup reaps and no canary. A binary exit1/101 or missing/late phase alone is not meaningful rejection. Observer-return/check times are used, not claimed kernel timestamps or debug-string guesses.

The direct driver ELF/source hashes, plan/config, raw typed status, parent.stop, ACK/tail/Creating files, parent.stdout/stderr, gate-result, new evidence trace, raw process output and resource/cleanup/restoration facts are retained and hashed before classification. Actual stdout/stderr EOF is required for completed output capture; the **gate notification tail's EOF/Open is separate evidence and is never an ACK/mutation prerequisite**. Symlinks are retained as link facts rather than followed. Expected-outcome-witnessed0 exit for the bypass cell means a qualified guard rejection, not a passing production test. Even a completed positive/red/restored comparison is only this Linux stand-in guard evidence, not VM/GPU/renderer/helper-wide/full-gate proof.
