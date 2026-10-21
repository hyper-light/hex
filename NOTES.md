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

kiln (`cp/t7-hex` at **7056744** — focused gate green on fedora/linux: `clippy -p kiln --all-targets
--locked -D warnings` and the `os::contain` tests both pass; macOS/Windows inert, target-gated. Full
gate runs in CI at push, which needs Ada's word):
- **Consumer #1, harness half.** Discovery: kiln's runner consumes no `Sandbox` trait — legs spawn
  via `proc::spawn_group`, harness via `contain::Group` (membership-only cgroups). So `contain_linux`
  now has a second backing: with `KILN_HEX_BOX=1` + a delegated subtree it makes each agent's group a
  **Hex `BoxRoot` box** (`<own>/hex/<id>` with `default_bounds`), vacating the runner to
  `<own>/supervisor` first; else today's membership-only. Reuses `hex-runtime`'s `BoxRoot` wholesale —
  **no new cgroup code in kiln**. `crates/kiln` gains a Linux-only dep on `hex-api`/`hex-runtime`.
  Every harness session (`Group::of`) is boxed for free. Staged behind the flag (default off) until
  e2e'd on a runner. `default_bounds` = fair-share `cpu.weight`/`io.weight` + a `pids.max` fork-bomb
  cap; **no memory cap yet** (a RAM-vs-concurrency policy — flagged to Kiln Distro).

## Next (in order)

1. **Consumer #1 — finish it.** (a) **Gate** the written harness half on fedora once the slot opens
   (`cargo clippy --all-targets`, `cargo test -p kiln contain`, then the full gate; refresh the lock
   first with a non-`--locked` resolve). (b) **Leg boxing** (the headline — "no build starves the
   machine"): box each host leg at `runtime.rs` `exec` (the host arm building `Command::new(&prog)` →
   `proc::spawn_group`, ~:1889). A leg isn't a session: its `job_id` holds `/`, which `BoxId` rejects,
   so map it to a valid id (sanitize + a short hash of the full id for uniqueness), create a box with
   leg bounds, enter the step command (write `0` to the box's `cgroup.procs` in a `pre_exec`, as
   `Group::enter` does), and remove the box when the leg ends (tie into the leg's existing
   `st.groups`/cleanup so a crash leaks none — T-T7.1.2). (c) **e2e**: a delegated-scope test of the
   boxed `contain::Group` path (like kiln-hex's conformance, under `systemd-run --user --scope -p
   Delegate=yes`) — needs a stable runner lab (asked Kiln Distro).
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
