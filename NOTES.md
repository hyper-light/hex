# CP-T7.1 — where it stands (2026-10-21)

Hex: branch `cp/t7-runtime` (pushed to github.com/hyper-light/hex, branch only — **fb15d9d**).
kiln: worktree `~/Projects/kiln/.claude/worktrees/t7-hex`, branch `cp/t7-hex` (local; off origin/main
4989921 which has kiln-seams). Nothing pushed to any main (needs Ada + a merge train).

## Done, committed, verified

Hex (`cp/t7-runtime`, full gate green at fb15d9d — fmt, clippy Linux/macOS/Windows, check-production,
tests, vendor):
- **hex-api** Runtime seam (`BoxId`, `Bounds`, `BoxSpec`/`ClassSpec`, trait, `SimRuntime`, conformance
  `HEX-RT-1..9`). **hex-core** sans-IO (cgroup writes, usage, records, crash recovery property test,
  T-T7.1.2). **process** class (real-cgroup conformance + crash recovery, `process_box.rs`). **oci**
  class = Hex's own OCI runtime at **crun parity 28/58 runtime-tools** (AC-T7.1.4, `bench/oci-validation/`).
  **node agent**. Decision in `docs/design/runtime.md`.
- **`hex box run`** (process box CLI, `box_run_cli.rs`), **`hex oci …`**, **`hex selftest-box`**.
- **AC-T7.1.5** node memory = **98 B/box** (node_memory bench). **AC-T7.1.2** noisy_neighbor =
  **1.30x** (idle 4822µs → loaded 6262µs), PASS ≤2x, on fedora at 1-min load ~3 (the AC names adacachy;
  rerun there for the of-record number — mechanism proven). MSRV lowered to 1.99.0 so kiln builds it.

kiln (`cp/t7-hex`):
- Runner systemd unit sets **`Delegate=yes`** (install.rs) — prerequisite for boxing.
- **Vendored Hex** at fb15d9d into `vendor/hex` (MANIFEST `[projects.hex]`; `vendor.py check` 0 diffs;
  excluded from kiln's workspace; kiln lock has the 4 hex library crates).
- **crates/kiln-hex**: `HexSandbox` implements `kiln_seams::sandbox::Sandbox` over Hex's node runtime
  (host Launch → process box; container/microvm/egress refused by name). **9/10 SANDBOX conformance**
  on fedora (`conformance.rs`, needs `HEX_BIN` + delegation; SANDBOX-7 egress deferred to CP-T7.7).
  Builds + clippies clean on kiln's 1.99.0.

## Next (in order)

1. **Consumer #1 — host CI legs + harness in process boxes** (the incident fix). The lynchpin: the
   runner must vacate its own cgroup to a leaf and enable controllers (cgroup v2 no-internal-process)
   before any child cgroup can carry limits. Plan: extend `crates/kiln/src/os/contain_linux.rs`
   (where `hold_agents()`/`Base::under_own` set up) to (a) move the runner to `<svc>/supervisor` +
   enable cpu/io/memory/pids on `<svc>` (best-effort; graceful fallback = today's membership-only),
   (b) add `Group::limits(bounds)` writing via `hex_core::cgroup::writes`. Then box each host leg
   (`runtime.rs:1889`, the host-only arm) and each harness session (`session.rs:2305`) by creating a
   limited cgroup and entering the command into it (reuse `Group::enter`). Add `hex-core` as a
   `crates/kiln` dep. Unit-test the limit computation; e2e needs a runner lab (see infra note).
2. **Consumer #2 — the `hex` CI executor** (container legs, no Docker). Add `kind == "hex"`
   (proto.rs CAP, probe.rs `executors()`, updates.rs `runner_build`, dispatch.rs `runner_view`,
   runtime.rs dispatch, local.rs validation — the map). It needs to unpack an OCI image to a rootfs
   without docker (skopeo+umoci, or kiln's registry client) then `hex oci create/start/…`. Larger.
3. **Build the `hex` binary in kiln** (`.kiln/ci.yaml` release-binary job → `dist/hex-$(uname -m)`)
   and install it on runners, so consumers #1/#2 activate (needs a deploy/merge-train step).
4. **AC-T7.1.3** hex_start p50 — harness ready (`bench/hex_start/`), **blocked by fedora runner
   instability** (frequent restarts kill the loop) + load swings; needs a stable quiet bench slot.
5. **T-T7.1.1** CI labs on the hex executor == docker (after #2).
6. Adversarial check (fresh subagent) of every AC before declaring the goal met.

## Infra note

fedora's kiln runner restarts every few minutes (kills long `kiln run`s) and its load swings 2→30
(other tracks' CI). noisy_neighbor landed in a ~load-3 window; hex_start's longer loop keeps getting
cut. A bench-calendar slot (quiet machine, load<2, nothing else building) is needed for hex_start and
for the of-record noisy_neighbor on adacachy. Builds: `kiln run` with
`CARGO_TARGET_DIR=$HOME/.cache/hex-t7/target` (hex) or `$HOME/.cache/kiln-t7-hex/target` (kiln,
cwd=the kiln worktree). Format + lockfile locally. See memory `hex-kiln-integration-plan`,
`hex-cgroup-delegation`.
