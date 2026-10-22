# CP-T7.1 — where it stands (2026-10-21)

Hex: branch `cp/t7-runtime` (pushed to github.com/hyper-light/hex, branch only — **fb15d9d**).
kiln: worktree `~/Projects/kiln/.claude/worktrees/t7-hex`, branch `cp/t7-hex` (local; off origin/main
4989921 which has kiln-seams). Nothing pushed to any main (needs Ada + a merge train).

## In flight (resume here)

- **Bounds refinement — COMMITTED (`cp/t7-hex` e08fe84), gated green** (fedora: `clippy -p kiln
  --all-targets --locked -D warnings` + the `os::contain` tests; cross-targets unaffected). The
  machine-protection the coordinator asked for (2026-10-21): legs sit under a dedicated parent
  `<own>/hex-legs` capped at `memory.high = machine_mem − max(15%, 8 GiB)` (`protect_machine`,
  `Boxing{agents,legs}`, `LEG_ROOT`), and `<own>/supervisor` + `<own>/hex` get `memory.low`, so a
  build storm throttles builds, not the runner. Agent/leg boxes keep fair-share cpu/io + pids (no
  per-box memory). Still behind `KILN_HEX_BOX` until the runner-lab e2e.
- **Benchmarks — detached job RUNNING on fedora** (pid 3947515, `setsid`): builds hex release, waits
  for load<2 sustained 5 min (deadline 06:00Z), records noisy_neighbor (AC-T7.1.2) + hex_start
  (AC-T7.1.3) with conditions to `~/.cache/hex-t7/bench-20261021T220059Z/`. Check that dir when back.
- **Consumer #2 — decided (docs/design `27ba823`), to build:** the `hex` executor unpacks OCI images
  via kiln's `ci/registry` (no skopeo/umoci), hands a bundle to the `oci` class. Touchpoints: proto.rs
  CAP, probe.rs `executors()`, updates.rs `runner_build`, dispatch.rs `runner_view`, runtime.rs
  dispatch, local.rs validation.
- **hex_start stale containers — CLEANED + harness FIXED (2026-10-22).** The leaks were `--rm`
  containers left when `inside.sh` hung (never exited, so `--rm` never fired). With the coordinator's
  go I removed **17** positively tied to the harness (image `registry.fedoraproject.org/fedora:44`,
  command `bash /opt/lab/inside.sh`, mounts /opt/hex+/opt/lab, created during my runs) by container
  ID; left the other 25 (other tracks'). `run.sh` now names + labels its container
  (`kiln.bench=hex_start`) and removes it via a trap + a 600 s timeout, so a hang can't leak again.
  **Deeper finding:** all 17 were *running* (hung) → `inside.sh` / the oci-start loop appears to HANG
  under the privileged-docker + private-cgroupns bench shape — likely the real **AC-T7.1.3** (hex_start
  p50) blocker, not just the load window. Diagnose `inside.sh` in a fedora slot. noisy_neighbor
  (AC-T7.1.2, no docker) is unaffected; the running bench will still record it.
- **RESUMED 2026-10-22** on an account with room. The overnight bench was **OOM-killed at 22:16Z**
  (a load-137 spike) while still in its wait-loop — **no numbers recorded**. Re-launched (pid 830857,
  self-relative 12h budget) and waiting for a load<2 window. Holding heavy fedora builds meanwhile so
  the bench can catch its quiet window; consumer #2 code is being written locally and gated later.

## Consumer #2 plan — the `hex` CI executor (container legs, no Docker)

Runner-direct, **no kiln-seams change** (`HexSandbox` deliberately refuses `Boundary::Container` —
"the hex CI executor runs those"). The runner unpacks the OCI image with kiln's own registry client,
applies layers+whiteouts into one rootfs dir, and runs it via Hex's in-process `oci` class (hex-node
`ClassSpec::Oci`, exec-per-step = the `docker exec` analog). Add `hex-node` to `crates/kiln`
(Linux-only, beside hex-api/hex-runtime). kiln's binary must route the `__oci-init` re-exec (the oci
runtime re-execs `current_exe() __oci-init`) → `hex_runtime::linux::oci::init::run()` in main.rs,
before clap.

1. **Unpacker** (new `crates/kiln/src/runner/oci_image.rs`, the novel core): reuse
   `crate::ci::integrations::oci` (`parse_reference`, `Registry::for_name`, `resolve`, `pub(crate)
   call`/`client`). GET manifest by digest → parse `{config, layers[]}`; for an index pick the
   runner-arch child. GET+verify config blob → `ImageConfig{Entrypoint,Cmd,Env,WorkingDir,User}`.
   Each layer lowest-first: GET blob → verify sha256 vs descriptor (pattern: `ci/registry/mirror.rs`
   blob() + `ci/services/cas.rs`) → gunzip (flate2) → untar into the rootfs applying OCI whiteouts
   (`.wh.<name>` deletes the target; `.wh..wh..opq` clears the dir) with path-safety (no `..`/abs
   escape). zstd layers refused by name (no zstd dep yet). Out: merged rootfs `Dir` + `ImageConfig`.
   Unit-test `apply_layer` on synthetic tars — no network.
2. **Run path** (runtime.rs): branch parallel to the docker container leg (`try_init_containers`
   ~:1733 `executor.kind=="docker" && image`): kind=="hex" → unpack → hex-node `Node::create(BoxSpec{
   class: ClassSpec::Oci(OciSpec{rootfs: Rootfs::Dir(merged), init: pause/entrypoint, mounts from
   container_map/PathMap, network from a.network})})`; per step `Node::exec(id, Program{args,env,cwd,
   user}, Stdio pipes)` (arm parallel to exec() ~:1890) bridged into the spawn_group/Reading/Processor
   stdout/stderr/exit loop.
3. **Dispatch** (Area-2 anchors): `CAP_HEX = 1<<28` (proto.rs ~:262); probe.rs `executors()` :82 insert
   "hex" when Hex-oci usable (Linux + delegated cgroup); updates.rs `runner_build` :2197 add `| hex`;
   dispatch.rs `runner_view` :589 synthesize "hex" from CAP_HEX; **dispatch.rs `assignment` :1468
   carry the image for "hex" too** (currently only "docker" — easy miss); local.rs :409 accept "hex";
   untrusted → add to `ci.untrusted.executors`.
4. **__oci-init routing** in kiln main.rs (before clap).

Status: **Increment 1 (unpacker core) DONE** — `crates/kiln/src/runner/oci_image.rs` (cp/t7-hex
**7da905f**): manifest/index/image-config parsing, `pick_platform`, `verify` (digest), `layer_reader`
(gzip; zstd refused), and `apply_layer` (OCI whiteouts + path-escape refusal via `safe_rel`). Pure +
cross-platform; 7 unit tests. Gated on macOS **and fedora/Linux** (the primary target): `clippy -p
kiln --all-targets --locked -D warnings` clean + the 7 tests pass. (A macOS `cargo test` link emits a harmless `linker_messages` eh_frame>16MB
warning on the big kiln test binary — not clippy, not the Linux gate.) **Increment 2 (`unpack` orchestration) WRITTEN** — `b302f31`: `resolve`→manifest→config→layers via
`ci/integrations/oci`, stream + verify each blob (one redirect followed), `apply_layer` into one
rootfs → rootfs + `ImageConfig`. Compiles + lints on macOS (`clippy -p kiln --lib -D warnings`); it's
network, so it's integration-tested against a real registry in the fedora slot. **Remaining (fedora
slot):** the run-path (add the `hex-node` dep; `Node::create(ClassSpec::Oci)` + `exec`-per-step +
stdio bridge; route `__oci-init` in kiln `main`) and the dispatch touchpoints (CAP_HEX, probe/updates/
dispatch/local), then the unpack integration test — plus the KILN_HEX_BOX smoke and the
inside.sh/hex_start diagnosis.

### Fedora slot playbook (one sitting, coordinator's order; **pin every run `taskset -c 4-31`** off Shards' CPUs 0–3, with `nice`)

1. **inside.sh hang — DIAGNOSED from code (off-slot) + FIXED (`4c76c13`).** Root cause is a
   **create-time deadlock**, not an unclean delete: `hex oci create` is detached and its init inherits
   the harness's stdout/stderr; after `create` returns, the shim parent (waitpid, init.rs:90–99) and the
   container child (blocked opening `exec.fifo` WRONLY until `start`, init.rs:165–168) both still hold
   those fds. The timed loop used `subprocess.run(capture_output=True)`, whose `communicate()` drains the
   pipes to EOF — impossible until the container execs+exits, which needs `start`, which the harness
   can't send while blocked reading `create`. Deadlock on the *first* `create` (→ the leaked `--rm`
   containers). The shell probe and the runtime-tools suite (file-redirected fds, oci-validation/
   inside.sh:16) don't drain a pipe inline, so neither hits it; this would deadlock *any* compliant
   detached runtime. Fix applied to inside.sh (CLAUDE §4 — wait on process exit, not pipe EOF): stdio →
   /dev/null (success) + a reused file (errors). **Slot step 1 is now just: run hex_start, read the
   p50.** Separately, for consumer #2: `hex oci create` should grow an explicit container-stdio contract
   (fds/paths; default /dev/null) distinct from the CLI's own diagnostics, so kiln captures step output
   by draining AFTER start — never by reading create to EOF (same deadlock would bite the leg bridge).
2. **unpack integration test** (`runner::oci_image::unpack`): pull a small public image
   (`docker.io/library/alpine`, anonymous token) and assert the rootfs (`/bin/sh` via symlink, `/etc`)
   + `ImageConfig`. Write `#[test] #[ignore]` (run with `--ignored`; needs network), driven by kiln's
   async test runner (check how kiln tests block_on). Proves `apply_layer` end-to-end on a real image.
3. **run-path + dispatch — DESIGN CORRECTED (2026-10-22, `docs/design/runtime.md`).** The playbook's
   `Node::create(ClassSpec::Oci)`+`Node::exec` plan is **wrong**: `Node::create` refuses the oci class
   (`hex-node/src/node.rs` → `Unsupported(Class::Oci)`) and `Node::exec` is process-class only. The oci
   class is CLI-driven (`hex oci create/start/state/kill/delete`), with **no `exec`** yet. Corrected
   design: the hex executor **shells out to the `hex` binary** like the docker executor shells to
   `docker` — persistent job container (`hex oci create`+`start`, pause init), **`hex oci exec <id> --
   … -- <shell> <script>`** per step (foreground, reuses kiln's per-step Command/stdio/mask machinery;
   no hex-node dep, no stdio-contract change). **Blocker/primitive to build first:** `hex oci exec`
   does not exist — new oci-runtime (`setns` into the init's namespaces, fork for pid-ns, exec
   foreground, return exit) + `hex-cli` subcommand, then re-vendor into kiln. THEN kiln dispatch:
   CAP_HEX=1<<28 (proto.rs + the power-of-two test ~:790), probe.rs `executors()`, updates.rs
   `runner_build`, dispatch.rs `runner_view`/caps/assignment image-carry, kiln-ci routing executor→cap,
   local.rs `--executor` list. (Explore agent mapping the kiln touchpoints precisely.)
4. **KILN_HEX_BOX smoke:** scratch kiln server + a lab runner (transient `systemd --scope -p
   Delegate=yes`, `KILN_HEX_BOX=1`) + one real dispatched host leg → confirm it's boxed; then flip the
   default on.

### CAP_HEX / `hex`-executor dispatch edit map (from the Explore sweep, 2026-10-22)

**Key finding:** executor NAME and capability BIT are **decoupled** — there is no generic name→cap
map. `CAP_FIRECRACKER`/`CAP_SHARDS` are *dead bits* (defined, never enforced); those executors route by
NAME only (`facts.executors` ↔ `Rule.executor`). So `hex` can route by name alone (firecracker/shards
pattern) OR wire CAP_HEX like CAP_DOCKER. The only place name↔bit link is `dispatch::runner_view`
(strips a facts entry unless its cap is advertised). **Do NOT advertise hex (probe/updates) until the
runtime.rs run-path exists**, or a hex leg dispatches to a runner that can't run it.

Inert scaffolding (safe to land before the run-path; nothing advertises hex): 1) `proto.rs:262` add
`pub const CAP_HEX: u32 = 1 << 28;`. 2) `proto.rs:621` add `CAP_HEX` to the `ci` power-of-two test array
(the `all` array at ~:789 does NOT need it). 6) `dispatch.rs:26` add `CAP_HEX` to the `use`. 7)
`dispatch.rs:592`/`:598` `runner_view`: synth "hex" when `caps&CAP_HEX!=0`, strip when `==0`. 8)
`dispatch.rs:1468` image-carry: `matches!(executor.as_deref(), Some("docker")|Some("hex"))`. 9)
`local.rs:409`+`:411` add `"hex"` to the `--executor` `matches!` + bail!. 10) `local.rs:69` help text.
11) `attest.rs:102` `isolation()` bucket ("container" if container-like). Advertisement (land WITH the
run-path): 3) new `runner/hex.rs` `find()`/`available()` (parallel `docker.rs:33/46`) + `mod hex;`. 4)
`probe.rs` insert hex detection **before** the `:85` docker early-return. 5) `updates.rs:2192`/`:2197`
`let hex = if runner::hex::available() {CAP_HEX} else {0};` OR'd into Build caps (NOT into `RUNNER_CAPS`).
Execution (the real work, needs `hex oci exec`): `runtime.rs:1012,1687,1735,2603` add hex arms +
`runner/hex.rs` `Spec`/run path. **Non-parallel traps:** `dispatch.rs:107 route_of` requires CAP_DOCKER
from the *spec* (container/services/`docker://`) — hex has no implicit spec trigger, so don't mirror it
unless defining what implies "needs hex"; `dispatch.rs:1301` docker-socket option is docker-only;
routing.rs `rootless_docker`/`Untrusted` are hard-coded `e=="docker"`; `runtime.rs:3084 "docker"` is an
action `runs.using` type, NOT the executor — leave it.

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
  **1.73x of-record** (idle 3616µs → loaded 6267µs), PASS ≤2x, on **fedora NVMe** at 1-min load **1.16**
  in a sustained quiet window, 2026-10-22 05:11:33Z (recorded `bench/noisy_neighbor/RESULT.md`; the AC
  names adacachy, run on fedora NVMe per the coordinator to keep adacachy for production). **AC-T7.1.3**
  hex_start = **14.4ms p50** of-record (p99 16.7, max 18.1, N=200), PASS ≤50ms, fedora NVMe (cpuset
  4-31), 2026-10-22 (`bench/hex_start/RESULT.md`; first clean run after the inside.sh deadlock fix
  4c76c13). MSRV lowered to 1.99.0 so kiln builds it.

kiln (`cp/t7-hex`):
- Runner systemd unit sets **`Delegate=yes`** (install.rs) — prerequisite for boxing.
- **Vendored Hex** at fb15d9d into `vendor/hex` (MANIFEST `[projects.hex]`; `vendor.py check` 0 diffs;
  excluded from kiln's workspace; kiln lock has the 4 hex library crates).
- **crates/kiln-hex**: `HexSandbox` implements `kiln_seams::sandbox::Sandbox` over Hex's node runtime
  (host Launch → process box; container/microvm/egress refused by name). **9/10 SANDBOX conformance**
  on fedora (`conformance.rs`, needs `HEX_BIN` + delegation; SANDBOX-7 egress deferred to CP-T7.7).
  Builds + clippies clean on kiln's 1.99.0.

**Landing dependency — CP-T4.1 Sandbox seam (agreed with T4 `01a15e86`, 2026-10-22):** T4's
`cp/t4-isolation` reshapes `kiln-seams::sandbox`: `Limits.memory_bytes: Option<u64>` +
`Limits.processes: Option<u32>` (`None` = no limit, `Limits::NONE` all-None); `Launch.egress` →
`enum Egress { Any, Only(Vec<String>) }` (`Any` = host net; `Only(vec![])` = none; `Only(hosts)` =
those); and a new required `Sandbox::holds() -> Holds{memory,processes,weights,no_egress,egress_by_host}`
with `Holds::check(&launch)` refusing `Unsupported` by name for whatever isn't held (conformance
SANDBOX-5/6/7/10 check a sandbox refuses exactly what it doesn't hold). **Agreed order: T4 lands first,
then I adapt `cp/t7-hex` on rebase** (coordinator orders landing). My edits (mechanical, mine to make):
`kiln-hex/src/imp.rs:56` egress → accept `Egress::Any`, refuse `Egress::Only(_)`; `imp.rs:64-65` drop
the `Some(..)` wrap (`memory_bytes`/`processes` are now `Option`, hex `Bounds` already takes `Option`);
`imp.rs:83` real impl `holds()` = memory/processes/weights **true**, no_egress/egress_by_host **false**;
`portable.rs:19` stub `holds()` = **all false**; `tests/conformance.rs:63` → `Some(..)`.
**Resolved with T4 (2026-10-22):** (a) the impl calls `Self::HOLDS.check(&launch)?` at the top of
`launch()` and returns `HOLDS` from `holds()` — one source of truth — so **drop both** the manual egress
refusal (imp.rs:56-60) **and** the manual weight-range check (imp.rs:46-50): `check` refuses an off-scale
weight as `Invalid` first, then whatever's unheld. (Weight scale confirmed by T4: the seam uses cgroup
v2's own — `WEIGHT_MIN=1`, `WEIGHT_MAX=10_000`, `WEIGHT_EVEN=100`, sandbox.rs:34-37 — identical to hex's,
so dropping the manual range check loses no bound.) (b) Egress:
accept `Any`, refuse `Only(vec![])` ("a run with no egress") and `Only(hosts)` ("egress to named hosts
alone"). (c) **Defaults trap (bites tests):** `Egress::default() = Only(vec![])` (none),
`Limits::default() = 1 GiB / 64 proc`, and `Launch::of(program,args)` uses both — so any Launch built
from defaults asks for no-egress (+ real limits), which Hex refuses. Audit every Launch I construct
(conformance.rs + any example) and set `egress: Egress::Any` + `Limits::NONE` where I mean today's
host / no-limit behaviour. Consumer #2's run-path is runner-direct (no seam), so **not** affected —
landing-time rebase only. T4 sends the landed commit; if the coordinator picks Hex-first, T4 pings on
its main-merge.

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
4. **AC-T7.1.3** hex_start p50 ≤50ms — **DONE: 14.4ms of-record** (`bench/hex_start/RESULT.md`), the
   first clean run after the `inside.sh` deadlock fix (`4c76c13`). [AC-T7.1.2 also of-record **1.73x**.]
5. **T-T7.1.1** CI labs on the hex executor == docker (after #2).
6. Adversarial check (fresh subagent) of every AC before declaring the goal met.

## Infra note

Benchmarks need a load<2 window and fedora rarely has one: it runs the tracks' gates (T3/T4/T6/T10),
so its load swings 5→25 and spiked to 254 (OOM) on 2026-10-21. A long bench run is refused or cut by
that load/OOM — **not** by a runner restart. (Correction: an earlier note here said "the runner
restarts every few minutes"; the coordinator checked the journal and process ages and fedora's two
runners have been stable, so that was wrong. What cut earlier long runs was load/OOM, and once an mcp
`run` shell reset — never a machine restart. All builds/tests this session completed cleanly.) The fix
is a named quiet slot on the bench calendar, which the coordinator gives. noisy_neighbor's of-record
**1.73x** landed 2026-10-22 05:11Z in a sustained quiet window on **fedora NVMe** (load 1.16),
superseding the 1.30x ~load-3 provisional (`bench/noisy_neighbor/RESULT.md`); hex_start still awaits
the slot (and the `inside.sh` fix), on **fedora NVMe** (not adacachy — production). Builds: `kiln run`
with `CARGO_TARGET_DIR=$HOME/.cache/hex-t7/target` (hex) or `$HOME/.cache/kiln-t7-hex/target` (kiln,
cwd=the kiln worktree). Format + lockfile locally. See memory `hex-kiln-integration-plan`,
`hex-cgroup-delegation`.
