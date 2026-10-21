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

kiln (`cp/t7-hex` — **consumer #1 complete behind `KILN_HEX_BOX`** (default off), two commits:
**7056744** (harness sessions) + **a724766** (CI legs). Gated green on fedora: `clippy -p kiln
--all-targets --locked -D warnings` + the `os::contain` tests (linux), and `cargo-zigbuild` clippy for
aarch64-apple-darwin + x86_64-pc-windows-gnu. Full gate runs in CI at push, which needs Ada's word):
- Discovery: kiln's runner consumes no `Sandbox` trait — legs spawn via `proc::spawn_group`, harness
  via `contain::Group` (membership-only cgroups). So `contain_linux` gains a second backing: with
  `KILN_HEX_BOX=1` + a delegated subtree each agent/leg group is a **Hex `BoxRoot` box**
  (`<own>/hex/<id>` with `default_bounds`), the runner vacating `<own>` to `<own>/supervisor` first;
  else today's membership-only. Reuses `hex-runtime`'s `BoxRoot` wholesale — **no new cgroup code in
  kiln**; `crates/kiln` gains a Linux-only dep on `hex-api`/`hex-runtime`.
- **Harness sessions** (`Group::of`) boxed for free. **CI legs** boxed at `runtime.rs` `exec`
  (`leg_box(job_id)` → `<own>/hex/leg.<hash>`, entered per host step, killed+removed at leg end;
  `sweep_leg_boxes()` at runner start clears crash leftovers — T-T7.1.2). cgroup boxing is Linux-only;
  macOS/Windows/none are no-op stubs so `runtime.rs` stays cfg-free (kiln's os-layer convention).
- `default_bounds` = fair-share `cpu.weight`/`io.weight` + a `pids.max` fork-bomb cap; **no memory cap
  yet** (a RAM-vs-concurrency policy — flagged to Kiln Distro). Staged behind the flag until validated
  end-to-end on a runner (needs a stable lab).

## Next (in order)

1. **Consumer #1 — e2e validation + default-on.** A delegated-scope test of the boxed path (like
   kiln-hex's conformance, under `systemd-run --user --scope -p Delegate=yes`): a boxed agent and a
   boxed leg each enforce their bounds and leave no cgroup behind. Needs a stable runner lab (asked
   Kiln Distro). Then flip `KILN_HEX_BOX` on by default.
2. **Consumer #2 — the `hex` CI executor** (container legs, no Docker). Add `kind == "hex"`
   (proto.rs CAP, probe.rs `executors()`, updates.rs `runner_build`, dispatch.rs `runner_view`,
   runtime.rs dispatch, local.rs validation — the map). It needs to unpack an OCI image to a rootfs
   without docker (skopeo+umoci, or kiln's registry client) then `hex oci create/start/…`. Larger.
3. **Build the `hex` binary in kiln** (`.kiln/ci.yaml` release-binary job → `dist/hex-$(uname -m)`)
   and install it on runners, so consumer #2 activates (needs a deploy/merge-train step).
4. **AC-T7.1.3** hex_start p50 ≤50ms + of-record **AC-T7.1.2** noisy_neighbor — harnesses ready
   (`bench/hex_start/` needs docker + a hex release binary; `noisy_neighbor` bench needs a delegated
   subtree). **Blocked on a load<2 lull** (fedora draining from the OOM; its bench slot was T3's until
   18:30Z). hex_start on fedora NVMe; of-record noisy_neighbor named on fedora NVMe too (not adacachy).
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
