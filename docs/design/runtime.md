# The node runtime: the `process` and `oci` classes (CP-T7.1)

The decisions CP-T7.1 makes, with their sources, evidence and what would falsify them (CLAUDE.md
§5). Requirement IDs: AC-T7.1.* and T-T7.1.* in `docs/requirements.toml`.

## What a box is

A **box** is the unit a node runs: a resource bound holding a tree of processes. One `Runtime`
trait (`hex-api`) covers every class; CP-T7.1 builds two:

- **`process`** — host processes in a cgroup v2 box (Linux). No namespaces: the programs are kiln's
  own CI legs and harnesses, trusted code that must not *starve the machine*, not untrusted code to
  isolate (that is the `firecracker` and `shards` classes, CP-T7.2).
- **`oci`** — an OCI runtime-spec container: namespaces, a private root filesystem, cgroups,
  capabilities, rootless through user namespaces. It runs container CI legs with no Docker daemon.

The trait is sans-IO in spirit: `create`, `exec`, `signal`, `inspect`, `usage`, `list`, `delete`,
each returning a typed `Refusal`. A simulator (`hex_api::sim`) and a conformance suite
(`hex_api::conformance`, cases `HEX-RT-1..9`) define the contract; the simulator, the `process`
class and the `oci` class all pass it, so laptop and fleet run one code path (CLAUDE.md §8).

## Decision: Hex's own OCI runtime, not youki's libcontainer (AC-T7.1.4)

**The choice.** The `oci` class is Hex's own runtime, not a vendored youki/libcontainer and not a
shell-out to runc or crun.

**Sources.** OCI Runtime Specification v1.3.0; `opencontainers/runtime-tools` validation suite;
youki `v0.7.0`; runc 1.5.1 and crun 1.27 as reference runtimes.

**Evidence** (measured 2026-10-20 on fedora.attlocal.net, kernel 6.19, cgroup v2, SELinux
enforcing; `bench/oci-validation/`, baseline in `bench/oci-validation/baseline/`):

1. **The no-panic rule (CLAUDE.md §1) is decisive.** youki's production libraries do not meet it.
   Clippy with Hex's production lints on `libcontainer` + `libcgroups` at `v0.7.0` reports **379**
   violations: 104 `arithmetic_side_effects`, 84 `unwrap`/`expect`, 49 `indexing_slicing`, 23
   `string`/slice slicing, 29 truncating casts, and **48 `unsafe` blocks** without Hex's
   `// SAFETY:` discipline. CLAUDE.md requires that *vendored code Hex changes follows the same
   rule*; vendoring youki would make Hex own and re-apply a 379-site rewrite of someone else's
   container internals across every re-vendor — a standing burden that fights the bar. crun is C, so
   it can be neither vendored as Rust nor held to the rule; it would be an external binary
   dependency, which the "no Docker daemon" consumer is trying to shed.
2. **No reference runtime passes the suite whole in a realistic node environment,** so "100%"
   (AC-T7.1.4) is measured against the *applicable* subset, as youki's own CI does (its
   `oci_integration_tests.sh` skips cgroup-v1 and apparmor cases on a v2 host). On the identical
   harness the references scored: crun 28 pass / 30 fail, youki 21 / 37, runc 1 / 57 (runc's near-zero
   is an SELinux/memfd re-exec quirk of the nested privileged lab, not a runc defect). The fails are
   dominated by the suite's cgroup-v1 paths (`/sys/fs/cgroup/pids/...`), apparmor/SELinux host setup,
   and test-harness assumptions — not runtime bugs. Hex's own runtime is therefore held to: **pass
   every validation test crun passes on the same host**, and name every test it does not, with why.
3. **Start latency (AC-T7.1.3, ≤50 ms p50) is reachable without a general-purpose runtime.** Hex's
   runtime does only what its classes need and shares the cgroup machinery the `process` class
   already has, so there is no daemon, no image-graph driver and no Go runtime in the path.

**What would falsify it.** If Hex's own runtime cannot reach crun-parity on the applicable
validation subset within CP-T7.1–T7.2, or if its start latency exceeds 50 ms p50 with the image
present, the decision is revisited in favour of vendoring youki and paying down its lint debt. The
measurements live in `bench/` and are re-run per the bar.

## The `process` box (Linux) — AC-T7.1.1, AC-T7.1.2, AC-T7.1.5

**Source.** The kernel's cgroup v2 documentation (`Documentation/admin-guide/cgroup-v2.rst`);
Borg/Twine's resource isolation (EuroSys'15, OSDI'20).

- Each box is a cgroup directory under a root the node agent owns (`<delegated>/hex-boxes/<id>`).
  Bounds map to the kernel's own files and units (`hex_core::cgroup`): `cpu.weight` (1..=10000),
  `cpu.max` (quota/period in µs), `memory.max`/`memory.high` (bytes), `io.weight`, `io.latency`
  (µs) and `io.max` (per disk), `pids.max`. A bound whose controller isn't delegated is refused by
  name before anything is made (CLAUDE.md §3).
- **The io bound targets whole disks.** The kernel takes `io.latency`/`io.max` per block device, not
  per partition, so Hex follows a path's filesystem to its disk(s) — through a partition to its
  parent, through device-mapper (LUKS/LVM) to the disks below — in `hex_runtime::linux::disks`.
  `io.weight` and `io.latency` are the lever against the 2026-10-19 disk-starvation incident
  (AC-T7.1.2: another box's fsync p99 ≤ 2× idle while one box builds at full tilt).
- **Programs start *inside* the box.** `exec` opens the box's `cgroup.procs` and, in the forked
  child before `execve` (the one `unsafe` site, `hex_runtime::linux::sys`), writes `0` to it, so the
  program and everything it forks are bound from their first instruction — no race where a child
  escapes the bound. Processes are watched by `pidfd` (`waitid(P_PIDFD)`), so a pid is never confused
  with a later process's.
- **Delegation.** cgroup v2's no-internal-process rule means the node agent must run in a leaf
  beside the box root, and its systemd unit needs `Delegate=yes`. On kiln's runners the agent shares
  a service cgroup, so the conformance test runs its boxes in a private `systemd-run --user --scope
  -p Delegate=yes`.
- **Memory the node agent adds per box (AC-T7.1.5, ≤1 MiB):** a box is a handful of small files and
  one `BoxRecord`; the agent keeps no per-box threads or buffers. Measured in `bench/`.

## Durability and recovery — T-T7.1.2

The node agent writes one record per box (`hex_core::record`, `hex_node::store`): a temp file,
`fsync`, atomic `rename`, then an `fsync` of the directory, so a crash leaves the old record or the
whole new one, never a torn one (CLAUDE.md §4; this is the fix for kiln's 500 ms-delayed,
un-fsynced session file, master plan §1.2). The record names a box's phase (`creating`/`ready`/
`deleting`) *before* the machine changes, so after a crash recovery (`hex_core::recovery`) knows
what every leftover is: a box `ready` with its cgroup present (and, for `oci`, its init alive) is
adopted; a half-made or half-deleted box, a cgroup with no record, or an unreadable record is
scrubbed. Each scrub step tolerates its target being gone already, so a crash *during* recovery is
recovered by running it again. A model-based property test crashes the agent after every step of
every generated history and asserts: every box left is whole and adopted, every box that was whole
survives, nothing is leaked.

## Decision: OCI images become bundles through kiln's registry client, not skopeo/umoci (consumer #2)

**The choice.** The `hex` CI executor runs a container leg by handing the `oci` class a bundle — a
rootfs and a `config.json` — that it builds itself from the image, using kiln's own registry client
(`ci/registry`, which already speaks the OCI distribution and image specs). It pulls the image,
verifies every layer and the config against its digest, then applies the layers in order into a
rootfs, honouring whiteouts (`.wh.<name>`) and opaque directories (`.wh..wh..opq`). No `skopeo`, no
`umoci`, no Docker daemon.

**Sources.** OCI Image Format Specification v1.1.0 (manifest, config, the `…layer.v1.tar+gzip`/`+zstd`
media types, the whiteout rules); OCI Distribution Specification v1.1.0 (digest-addressed pull);
kiln's `ci/registry`, which already pulls and verifies OCI artifacts for the container CI path.

**Evidence / rationale.**
1. **No new external dependency, and the no-Docker consumer stays dependency-free.** AC-T7.1.1's
   second half is that container legs run *without a Docker daemon*; shelling out to `skopeo`/`umoci`
   would trade one external binary for two. kiln already carries an OCI-speaking registry client, so
   the executor adds only tar extraction + whiteout handling, kiln's own code held to the bar.
2. **Digests are checked before anything is applied (CLAUDE.md §3, §4).** Every blob is verified
   against the digest its manifest names before a byte reaches the rootfs, so a corrupted or
   substituted layer is a typed refusal, not a poisoned container.
3. **The bundle is the seam.** Unpacking is image-spec work (layers → a tree); running is
   runtime-spec work (namespaces, cgroups, caps), already Hex's `oci` class. The executor owns no
   runtime logic and the `oci` class owns no image logic.

**What would falsify it.** If kiln's registry client can't pull or verify an image class the CI path
needs, or if layer application can't reach parity with a reference unpacker on the runtime-tools
image corpus, the executor falls back to a vendored `umoci`-equivalent held to the bar. Measured when
consumer #2's lab lands.

## Decision: the `hex` executor runs a leg's steps in a persistent container via `hex oci exec` (consumer #2 run-path)

**The choice.** A container leg is a *persistent* job container, the docker executor's shape: from the
unpacked bundle the executor runs `hex oci create` + `hex oci start` once with a pause init (the
container stays up while the leg runs), then runs each step with `hex oci exec <id> [--cwd …] [--env
K=V …] [--user u:g] -- <shell> <script>` — foreground, like `docker exec` — and `hex oci delete
--force <id>` at the leg's end. The executor **shells out to the `hex` binary** exactly as kiln's
docker executor shells out to `docker`: each step is a host subprocess, so kiln's existing per-step
machinery (stdout/stderr streaming, `::add-mask::`, env files, outputs) is reused unchanged, and kiln
needs **no `hex-node` library dependency** (correcting an earlier plan — see below).

**The primitive this needs (to build).** `hex oci exec` does not exist yet: the `oci` class today is
`create`/`start`/`state`/`kill`/`delete` only, a standalone CLI-driven runtime (`hex_runtime::linux::
oci::run::Runtime`). `exec` joins the *running* container's namespaces — the init pid is in the
container's `state` (`oci/state.rs`), and `setns` is in place (`oci/sys.rs`) — then `chdir`s, drops to
the requested user, sets the environment and execs the program in the foreground, inheriting the
caller's stdin/stdout/stderr (so a foreground `hex oci exec` has none of the detached-`create` stdio
deadlock; the caller captures it as a normal subprocess) and returning its exit status. This is the
runc/crun `exec` analog (not in the OCI runtime spec, but the de-facto lifecycle extension both
provide). It is new `oci`-runtime + `hex-cli` work, then re-vendored into kiln.

**Correction to the earlier plan.** An earlier note had the executor drive the `oci` class through
hex-node's `Node::create(ClassSpec::Oci(…))` + `Node::exec`. That is not implementable: `Node::create`
**refuses** the `oci` class (`hex-node/src/node.rs` → `Refusal::Unsupported(Class::Oci)`) and
`Node::exec` runs only the `process` class. The `oci` class is reached through the `hex oci` CLI, not
the node — so the executor drives the CLI, as above.

**Sources.** OCI Runtime Specification v1.3.0 (the container lifecycle `create`/`start`/`delete`);
runc and crun `exec` (join the container's namespaces via `setns(2)` on `/proc/<init>/ns/*`, then exec
in the foreground); GitHub Actions' job-container model (one container per job, a command per step).

**Evidence / rationale.**
1. **One execution model across executors.** docker legs are a persistent job container plus a
   subprocess per step; mirroring that (persistent `oci` container plus `hex oci exec` per step) lets
   a hex leg reuse kiln's step runtime verbatim — the `exec` plan at `runtime.rs` becomes a `hex oci
   exec` argv where a docker leg's is a `docker exec` argv — instead of a parallel async path.
2. **Steps share one machine.** GitHub runs a job's steps on one machine; a step may leave a process,
   a mount or an in-memory state for the next. A persistent container holds that; one-container-per-
   step would not (it would keep only on-disk rootfs/workspace state).
3. **Foreground `exec` dodges the stdio problem.** Because `hex oci exec` waits for the step and
   inherits the caller's fds, kiln captures a step's output as it captures any subprocess — no need
   for `hex oci create` to grow an explicit container-stdio contract for this path.

**What would falsify it.** If joining a pid namespace for `exec` (which needs a `fork` after `setns`
so the child is in the container's pid namespace) makes foreground exit-code capture unreliable, or
if a persistent pause init is unacceptable overhead, the fallback is one container per step over the
shared `Rootfs::Dir` with the step's exit read from `hex oci state`. Measured when consumer #2's lab
lands.

## Open (STATUS.md)

- The `process` box on macOS (QoS and limits) and Windows (job objects); today a typed refusal
  naming the platform (CLAUDE.md §12).
- The kiln integration's remaining half: **consumer #1 (host CI legs + harness in `process` boxes)
  is built** — kiln's `os::contain` grows a Hex-`BoxRoot` backing behind `KILN_HEX_BOX`, with the
  legs' parent box capped (`memory.high`) and the runner and agents protected (`memory.low`) so a
  build storm throttles builds, not the runner — pending end-to-end validation on a runner lab and
  then default-on. **Consumer #2 (the `hex` executor for container legs)** is decided (above) and to
  build.
