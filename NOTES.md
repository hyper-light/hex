# CP-T7.1 — where it stands (paused for usage, 2026-10-20)

Branch `cp/t7-runtime`. Paused at the master's §5 usage rule (account > 80% of the weekly window;
Hex is the lowest-weight running track). Resume by continuing the "Next" list below.

## Done and committed (full gate green on fedora at commit d087571)

- **hex-api**: the `Runtime` seam — `BoxId`, `Bounds` (cgroup v2 names/units), `BoxSpec`/`ClassSpec`
  for `process` and `oci`, the `Runtime` trait, `SimRuntime`, and the conformance suite
  `HEX-RT-1..9`. Simulator + both real classes pass it.
- **hex-core** (sans-IO): bounds→cgroup writes, cgroup files→`Usage`, box records, and the
  crash-recovery plan with a model-based crash-at-every-step property test (T-T7.1.2).
- **`process` class** (hex-runtime, Linux): cgroup v2 boxes under a delegated root; programs started
  straight into the box via a `pre_exec` `cgroup.procs` join; pidfd waits; disk discovery for the io
  bounds. Real-cgroup conformance + crash recovery green (`hex-cli/tests/process_box.rs`).
- **`oci` class** (hex-runtime, Linux): Hex's own OCI runtime (`hex oci {create,start,state,kill,
  delete}`), re-exec shim + container init: namespaces (incl. user-ns maps, setns for ns paths),
  rootfs + mounts + devices + masked/readonly + pivot_root, caps (bounding before setuid, restore
  with KEEPCAPS, ambient), rlimits, oom_score_adj, hooks. **crun parity: 28/58 runtime-tools, the
  same set crun passes on this cgroup-v2 host** (AC-T7.1.4). Evidence: `bench/oci-validation/`.
- **node agent** (hex-node, Linux): `Runtime` over `process`, durable records, recovery.
- **Decision** (docs/design/runtime.md): Hex's own OCI runtime over youki/crun, by evidence
  (youki 0.7.0 = 379 no-panic-lint violations; crun is C; no runtime passes the suite whole here).
- **AC-T7.1.5** (node memory ≤1 MiB/box): **PASS, 98 bytes/box** (`cargo bench -p hex-bench
  --bench node_memory`, 500 boxes, fedora).

## Next (in order)

1. **AC-T7.1.3** (oci start p50 ≤50 ms): the lab `bench/hex_start/` works (probe ok) but the p50
   wasn't captured before the pause — fedora was overloaded/restarting. Re-run on a quiet fedora
   (`bash bench/hex_start/run.sh <release hex> 200 <image> <outdir>`; reads `<outdir>/result.txt`).
2. **AC-T7.1.2** (noisy_neighbor fsync p99 ≤2× idle, on adacachy): lab not yet written. Two
   `process` boxes on one disk; one runs a disk-saturating build, the other measures fsync p99 with
   a tight `io.latency`/low `io.weight`; compare to idle. Needs a bench-calendar slot (load <2).
3. **kiln integration** (first consumers; worktree `~/Projects/kiln/.claude/worktrees/t7-hex`, branch
   `cp/t7-hex`, not yet started): host CI legs + harness processes in `process` boxes (wrap
   `crates/kiln/src/runner/runtime.rs` `exec` and `harness` spawn via `hex_runtime::linux::process::
   join_on_exec`; the runner's systemd unit needs `Delegate=yes` — see memory `hex-cgroup-delegation`);
   and a `hex` CI executor running container legs with no Docker daemon (new executor kind `hex`
   alongside `docker`, per the kiln survey: wire.rs:136, probe `executors()`, dispatch, runtime.rs
   kind checks). Build against a small adapter until `kiln-seams` `Sandbox`/`Placement` land
   (cp/m2-seams); message agent 01a15845efc372bc989c70970815c8e7 to agree shapes (already asked).
4. **T-T7.1.1** (CI labs on the hex executor == docker): once the executor exists, run kiln's
   conformance labs (`ci/conformance/`).
5. Adversarial check (a fresh subagent) of every AC before declaring the goal met.

## Notes

- Builds/tests run on fedora via `kiln run` with `CARGO_TARGET_DIR=$HOME/.cache/hex-t7/target`.
  Format + lockfile locally (cheap, no compile): `cargo fmt --all` / `cargo generate-lockfile` with
  `--manifest-path`. Full gate: `bash scripts/gates.sh` (was green at d087571; the hex-bench crate
  and bench labs were added after and compile via `cargo bench`, but the full workspace gate has not
  been re-run since — run it first on resume).
- fedora was unstable (runner restarts, 0 idle cores) during the benchmark attempts; latency numbers
  need a quiet slot per the bar.
