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

## Decision: every `oci` container gets its own cgroup, with `linux.resources` in force (AC-T7.1.4, AC-T7.1.1)

**The gap.** Until 2026-10-23 the `oci` class parsed `linux.cgroupsPath` and ignored it, and never
read `linux.resources`. Its containers had no cgroup, no limits and no device rules. A container
breaking CLAUDE.md §2 ("every resource has a bound"), and a root container could `mknod` and open
any host device despite a spec that denied them. The competitive baseline found it
(`bench/competitive/RESULT.md`): Hex's `delete` was not comparable with runc's and crun's, which
remove a cgroup.

**The choice.**
- **Where.** `cgroupsPath` when absolute is relative to the cgroup v2 mount point, as the spec
  requires; a relative one is rooted there too, and an unset one is `/<id>`, as crun's cgroupfs driver
  does (`src/libcrun/cgroup-cgroupfs.c`, `make_cgroup_path`). Paths with empty, `.` or `..`
  components, control characters or the systemd driver's `slice:prefix:name` form are refused (the
  spec lets a runtime refuse a value). `create` makes the cgroup before the shim starts, hands each
  ancestor the controllers the container needs, and refuses an existing cgroup that holds processes
  (the spec's own example of a cgroup unfit for create).
- **Who is inside.** Only the container. The shim stays out, so the limits bound the workload alone,
  and it exits once it has forked the container (runtime-architecture.md, decision 2). The shim opens the cgroup's directory and its `cgroup.procs` before it
  touches any namespace. The container is born in the cgroup: the shim clones it there with
  `clone3(CLONE_INTO_CGROUP)` (Linux 5.7), as crun does (below, "Born in its cgroup"). It then
  unshares its cgroup namespace, so the namespace's root is its own cgroup. Where it can't be born
  there, it joins as its first act by writing `0` to that `cgroup.procs`. The kernel checks a
  migration against the credentials and cgroup namespace the file was opened with (Linux 5.16, the
  fix for CVE-2021-4197), so the join works from inside a user namespace too.
- **What is in force.** `linux.resources` becomes cgroup v2 writes by crun's documented conversions
  (`crun.1.md`, "CGROUP v2"): `memory.limit` → `memory.max`, `reservation` → `memory.low`, `swap` →
  `memory.swap.max` as `swap − limit`; `cpu.shares` → `cpu.weight` by crun's quadratic in log2
  (`src/libcrun/cgroup-internal.h`, `convert_shares_to_weight`: 2 → 1, 1024 → 100, 262144 → 10000,
  rounded up); `quota`/`period` → `cpu.max`; `cpus`/`mems` → `cpuset.*`; blkio weights → `io.weight`
  linearly onto 1-10000; throttles → `io.max`; hugepages → `hugetlb.<size>.max`; `pids.limit` →
  `pids.max`; RDMA → `rdma.max`; `unified` keys as given, last. Unlike Hex's boxes, an `oci`
  container's memory limit doesn't also set `memory.swap.max` 0: the spec gives swap its own field,
  and runc and crun leave swap alone when it is unset.
- **What is refused, by name.** The spec: "If the conversion is not possible the runtime MUST
  generate an error" (config-linux.md, "Unified"). cgroup v2 has no `memory.kernel`, `kernelTCP`,
  `swappiness` or OOM-killer switch, no realtime CPU bandwidth, no blkio leaf weight, and no
  `net_cls`/`net_prio`; each, when set, refuses the container. So does a controller the resources
  need that the parent can't hand down.
- **Devices.** cgroup v2 controls device access with a `BPF_PROG_TYPE_CGROUP_DEVICE` program
  attached to the cgroup (`Documentation/admin-guide/cgroup-v2.rst`, "Device controller"). The
  steps:
  - It starts from deny, as runc's emulator does (its zero value) and crun's program does (it ends
    denying).
  - The spec's list, which "MUST" be applied in order, runs through cgroup v1's own update rules
    (`security/device_cgroup.c`, `devcgroup_update_access`).
  - crun's default devices are allowed last (`src/libcrun/cgroup-resources.c`, `default_devices`):
    `mknod` of any node, and the nodes the spec says a runtime "MUST also supply" (null, zero, full,
    random, urandom, tty), plus console, ptmx, `pts/*` and tun. runc appends the same list, but
    console.
  - The result, a default and its exceptions, is compiled to a program that decides as v1's checks did
    (`match_exception` under a deny default, `match_exception_partial` under an allow default), and
    loaded and attached with `BPF_F_ALLOW_MULTI`.

  A spec with no list gets the same policy as a deny-all one, as under runc and crun. Only a list that
  ends allowing everything gets no program. At most 256 exceptions, under the verifier's
  4,096-instruction limit for unprivileged programs. The first version started from allow and added
  no defaults. runtime-tools' generated spec denies every device, so every init's `mknod` of
  `/dev/null` was refused, and the validation suite fell from 28 passes to 4; a test now pins that a
  deny-all list keeps the supplied devices.
- **Delete and kill.** `delete` writes `cgroup.kill`, waits (bounded, 10 s, as `hex box run`'s
  cleanup) for `cgroup.events` to say the cgroup is empty, and removes it; past the bound it refuses
  and keeps the state for a later delete. `kill --all` signals every process in the cgroup, not the
  process group, which a process leaves with `setsid`. A failed `create` ends whatever it started in
  the cgroup and removes it.

**Evidence.** The spec text is runtime-spec `main` at 469dd5b6233c (2026-10-16), config-linux.md
"Control groups", "Cgroups Path" and "Unified". crun's table and code are at commit 48514cefa6dd
(2026-10-23). The conversions are pinned by tests against reference values computed independently
with IEEE doubles and libm `pow`, as crun's C computes them. The device compiler is checked
model-based: for random rule lists and requests, the compiled program, run by a small eBPF
interpreter in the test, decides exactly as cgroup v1's checks do on the state the rules leave
(`hex_runtime::linux::oci::resources`). A root-only lab test on fedora exercises the kernel side.

**What would falsify it.** A spec, from runc's `spec` or containerd, whose device list or resources
this refuses or decides differently from runc or crun on the same host. It also named a measurement
showing that the join, a migration through `cgroup.procs`, is a material part of `create`'s cost.
That measurement came (next).

### Born in its cgroup (2026-10-24)

**The finding.** With a cgroup per container, `create`'s p50 went from 8.0 ms to 25.1 and 27.2 ms
(two rounds, fedora, `bench/competitive/`), and `hex_start`'s create+start p50 from 14.4 ms to
28.5 ms. A migration write-locks the cgroup threadgroup lock (`kernel/cgroup/cgroup.c`,
`cgroup_attach_lock`), a percpu rwsem unless the hierarchy is mounted `favordynmods`. Its writer
enters through `rcu_sync_enter`, which waits out a full RCU grace period (`synchronize_rcu`) unless
another writer passed one within the last grace period (`kernel/rcu/sync.c`). So a migration that
comes soon after another is cheap, and one after a quiet spell costs a grace period. A benchmark's
creates, and a node's, come after quiet spells. `hex box run`'s bench, back to back, didn't show it.

**Evidence.** `bench/competitive/probes/join_cost.py`, fedora, 2026-10-24, kernel 6.19, load 1.23:
200 migrations per gap between two sibling cgroups.

| gap before the migration | p50 | p90 | max |
|---|---|---|---|
| 0 ms | 0.004 ms | 0.005 ms | 0.073 ms |
| 1 ms | 0.101 ms | 0.119 ms | 0.151 ms |
| 5 ms | 0.114 ms | 0.129 ms | 0.242 ms |
| 20 ms | 9.745 ms | 13.610 ms | 24.971 ms |
| 50 ms | 9.872 ms | 17.721 ms | 26.863 ms |
| 100 ms | 11.987 ms | 18.986 ms | 22.981 ms |
| 200 ms | 9.901 ms | 16.854 ms | 27.881 ms |

runc's and youki's creates, which also join by migration, take 27.5 and 21.9 ms on the same host;
crun's, which clones into the cgroup, 3.0 ms. cgroup-v2.rst, on `favordynmods`: "The static usage
pattern of creating a cgroup, enabling controllers, and then seeding it with CLONE_INTO_CGROUP is not
affected by this option."

**The choice.** The shim clones the container with `clone3(CLONE_INTO_CGROUP)`, so it is born in its
cgroup and nothing migrates it (`oci/sys.rs`, `fork_into_cgroup`). libc's `CLONE_INTO_CGROUP` is an
`i32` that overflows, so the constant and `struct clone_args` come from `include/uapi/linux/sched.h`.
The container joins by migration, as before, in three cases. `clone3` answers `ENOSYS`: before
Linux 5.3, or under a seccomp profile like Docker's default, which answers `clone3` so for a process
without `CAP_SYS_ADMIN` (`moby/profiles`, `seccomp/default.json`). It answers `E2BIG`: before 5.7,
which doesn't know the `cgroup` field. Or the container makes or joins a user namespace. A birth in a
cgroup is checked against the credentials the clone runs with (`cgroup_css_set_fork`), and once the
shim is in the new namespace those are no longer the caller's, while a migration is checked against
the credentials `cgroup.procs` was opened with.

**Evidence of the change.** `bench/oci-validation/cgroups_inside.sh`'s check 4 enables the kernel's
`cgroup_attach_task` tracepoint, in a trace instance of its own, across one `hex oci create`, and
wants no migration into the new container's cgroup. The binary before the change failed it (got 1);
the binary after passes it, and the lab's other 18 checks, and runtime-tools' 28 of 30, test for
test (fedora, 2026-10-24, `hex-cg2-2f0c122c`).

**What would falsify it.** A trace that shows a migration in a `create` without a user namespace, or
a spec this starts differently from before the change. The `process` class still joins its boxes by
migration, in `pre_exec`; its bench runs back to back, so it pays the grace period only when runs are
spaced, and moving its births into their cgroups is the same change for a different spawn path.

### Rootless devices: enforced by construction, not the program (2026-10-26)

**The finding.** The `oci` class runs rootless (its own decision). A rootless runtime can't load the
device program: `bpf(BPF_PROG_LOAD)` for a `BPF_PROG_TYPE_CGROUP_DEVICE` program, and the attach to
the cgroup, are privileged operations (`kernel/bpf/syscall.c`; the cgroup device controller is
CAP_SYS_ADMIN), and an unprivileged process has no such capability in the cgroup's owning (initial)
user namespace — being uid 0 inside its own user namespace does not grant it. Measured: a `bpf` load
from an unprivileged process returns `EPERM`. A spec with no device list, or any ordinary one, gets
the deny-by-default allow-list (above), which compiles to a program — so **every** rootless start
tried to load one and failed, and the class could start no standard container at all.

**The choice.** When the container makes a user namespace (`spec.makes_namespace("user")`, the same
signal `init.rs` uses), a deny-by-default allow-list is enforced by construction instead of by the
program, and the program is not loaded:

- Only the policy's devices get nodes. The init creates the default devices and the spec's
  `linux.devices`, no others (`init.rs`, `make_default_devices`/`make_device`).
- No new node can be made. `mknod` has no effect across the user-namespace boundary — CAP_MKNOD in
  the container's namespace does not reach the host (`user_namespaces(7)`), which `init.rs` already
  relies on to bind devices rather than create them.
- Access to any node — one the init made, or one baked into the image's rootfs — is governed by the
  mapped host uid's DAC. The container's "root" is an unprivileged host uid that owns nothing
  dangerous, so it can open the world-readable nodes a container needs (`/dev/null`, `/dev/zero`) and
  not the owned ones (`/dev/mem`, a disk).

So the policy's *outcome* — the allowed nodes usable, the rest not — holds without the program. This
is what crun, runc and podman do for rootless containers (they skip the device cgroup). An
allow-by-default policy with denials (`a *:* rwm` then a deny) has no construction that reproduces it:
a denied node the init still creates would be reachable. Rootless refuses such a spec by name
(`run::attach_device_program`, a typed `Config` error) rather than silently under-enforce it
(CLAUDE.md §3). The privileged path (a container with no user namespace, e.g. a root-run bench)
loads the program as before.

**Evidence.** Measured on the running hardware (adacachy, kernel 7.2.9-2-cachyos, 2026-10-26): in a
user namespace mapped to container-root (`unshare -Ur`), `mknod` of a new block node is refused
(`EPERM`); `/dev/null` and `/dev/zero` (root:root/root, mode 0666) open for read; `/dev/nvme0n1`
(root:disk, 0660) and `/dev/mem` (root:kmem, 0640) are denied. The behavioral test
`crates/hex-cli/tests/oci_seccomp.rs` then starts a rootless container end to end (it could not
before this change) and the deny-mkdir seccomp probe reports 42, the control 10 (AC-T7.1.4). crun's
rootless device handling is at commit 48514cefa6dd (`src/libcrun/linux.c`, the device cgroup is set up
only with a cgroup manager, absent rootless); runc's is in `libcontainer/`'s rootless cgroup manager,
which applies no device rules.

**What would falsify it.** A rootless container that opens a device its deny-by-default policy
denies — a dangerous node (e.g. `/dev/mem`, a disk, `/dev/kmsg`) world-accessible or owned by the
runtime's unprivileged host uid, reachable despite the policy. Or the refusal firing on a policy a
rootless container *can* enforce by construction (a deny-by-default allow-list). Or a host where an
unprivileged `bpf` load of a cgroup device program succeeds, which would mean the program could be
loaded rootless after all.

## The `process` box (Linux) — AC-T7.1.1, AC-T7.1.2, AC-T7.1.5

**Source.** The kernel's cgroup v2 documentation (`Documentation/admin-guide/cgroup-v2.rst`);
Borg/Twine's resource isolation (EuroSys'15, OSDI'20).

- Each box is a cgroup directory under a root the node agent owns (`<delegated>/hex-boxes/<id>`).
  Bounds map to the kernel's own files and units (`hex_core::cgroup`): `cpu.weight` (1..=10000),
  `cpu.max` (quota/period in µs), `memory.max`/`memory.high` (bytes), `io.weight`, `io.latency`
  (µs) and `io.max` (per disk), `pids.max`. A bound whose controller isn't delegated is refused by
  name before anything is made (CLAUDE.md §3).
- **Decision: a memory bound covers swap.** A box with `memory.max` also gets `memory.swap.max` 0.
  - *Evidence.* `memory.swap.max` is the cgroup's "swap usage hard limit", default `max`, and at it
    "anonymous memory of the cgroup will not be swapped out" (`cgroup-v2.rst`, [CG-1] in
    `docs/research/runtime/cgroup-v2-resource-control.md`). Without it, a box past `memory.max`
    swaps instead of being killed. On 2026-10-23, `hex box run`'s conformance test saw a 512 MiB hog
    finish in a 64 MiB box on fedora, a host with zram swap. Operators report 20-30 minute
    brownouts, with the OOM path never running, on zram-only swap [CG-44][CG-45].
  - *Kernels without swap accounting* have no `memory.swap.max`. A box there is made only if no swap
    is on (`/proc/swaps`); with swap on, its memory bound is refused by name, never silently weakened.
  - *Falsified by* a workload class that needs swap under a bound. That would add an explicit swap
    bound to `Bounds`, not lift this default.
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

**The primitive (built, 2026-10-24).** `hex oci exec [--cwd dir] [--env K=V]... [--user uid[:gid]]
<id> [--] <program> [args]...`, runc's command shape (`oci/exec.rs`, `hex-cli/src/oci.rs`):
- **How it runs.** `exec` turns itself into a helper, `hex __oci-exec`, by execve, so it is
  single-threaded, as joining a user or mount namespace needs, and a signal its caller sends lands on
  the helper. The helper opens the cgroup and each of the container init's namespaces
  (`/proc/<pid>/ns/*`) that differ from its own, before joining any. It joins them, user first and
  mount last, then clones the child straight into the container's cgroup (`clone3(CLONE_INTO_CGROUP)`,
  as `create` does, with the same fallback to a migration), and through the pid namespace into the
  container's pid namespace. The cgroup namespace is the child's to join, once it's in the cgroup:
  where cgroup2 is mounted `nsdelegate`, the kernel lets a clone into a cgroup only a caller that sees
  both its own cgroup and the target from its cgroup namespace (Linux v7.2
  `kernel/cgroup/cgroup.c:5371-5377`), and from the container's namespace the helper's own cgroup is
  out of view. Joined first, the clone failed with ENOENT (kiln research
  `01a178e67ccd7764b6746e5880187422`, f33).
- **What the program becomes.** The container's process, by the same code as its first process
  (`init::become_process`: OOM score, limits, seccomp filter, capabilities around the uid change,
  no_new_privs, umask). The request overrides the working directory, the environment by key, and the
  user; a user override drops the spec's supplementary groups.
- **The seccomp filter and no_new_privs.** no_new_privs is exactly the spec's `noNewPrivileges`. A
  filter without it needs CAP_SYS_ADMIN in the caller's user namespace (seccomp(2): "capability in its
  user namespace, or the thread must" already have no_new_privs set), so Hex loads it before the user
  and capabilities change, as runc does (`libcontainer/standard_init_linux.go:196-209` on main); the
  profile then also binds Hex's own calls up to `execve`. With no_new_privs, the filter goes on last
  (`:244-249`), so as few of Hex's calls as possible run under it. Hex had loaded every filter through
  seccompiler's `apply_filter`, which sets no_new_privs first: runtime-tools on fedora failed ten
  tests crun passes with `has expected noNewPrivileges` (kiln research
  `01a178e67ccd7764b6746e5880187422`, f34-f36).
- **Foreground.** The program runs on the caller's stdin, stdout and stderr, so a foreground `hex oci
  exec` has none of the detached `create`'s stdio deadlock, and its status is exec's: the exit code,
  or 128 plus the signal that ended it. 126 means it couldn't start, 127 that the program isn't there.
- **Signals.** SIGHUP, SIGINT, SIGQUIT and SIGTERM sent to the helper are passed on to the program,
  read from a `signalfd`, so a caller that ends a step ends the program, not just the helper.

It runs in a created or running container and refuses a stopped one. The lab checks each of these
against the kernel (`bench/oci-validation/cgroups_inside.sh`, section 5), including no migration into
the cgroup, by the `cgroup_attach_task` tracepoint. It's the runc/crun `exec` analog (not in the OCI
runtime spec, but the de-facto lifecycle extension both provide). Next it is re-vendored into kiln.

**Correction to the earlier plan.** An earlier note had the executor drive the `oci` class through
hex-node's `Node::create(ClassSpec::Oci(…))` + `Node::exec`. That is not implementable: `Node::create`
**refuses** the `oci` class (`hex-node/src/node.rs` → `Refusal::Unsupported(Class::Oci)`) and
`Node::exec` runs only the `process` class. The `oci` class is reached through the `hex oci` CLI, not
the node — so the executor drives the CLI, as above. (Since 2026-10-27 the node serves the `oci`
class too, from a `Bundle` or a structured `OciSpec`, below; the executor's choice of the command line
stands on point 1.)

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

## Decision: a box from an OciSpec runs as `docker run` runs its image (runtime goal 3, plan step 5)

**The choice.** The node agent serves the structured `ClassSpec::Oci(OciSpec)` by rendering it to a
runtime-spec bundle in a directory of the box's own (`<state>/bundles/<id>`) and creating that bundle as it
creates a `ClassSpec::Bundle`. The rendering is a pure core, `hex_core::bundle::render`, with golden tests;
the node only checks the host (the root and each mount's source are there; the bundles directory isn't on
overlayfs) and writes the files. What the container gets:

- **Its root.** `Rootfs::Dir` is used in place. `Rootfs::Layers` is an overlay with the layers as its lower
  directories, top first, over the box's writable layer (`upper/`, with overlayfs's `work/`). It is the
  first mount in `config.json`, at `/`, so the container mounts it in its own mount namespace, and every
  other mount lands on it. Nothing is mounted on the host: the mount goes with the container's last process,
  and delete removes the writable layer with the bundle. A rootless container mounts it in its user namespace,
  where overlayfs keeps its attributes as `user.overlay.` (`userxattr`).
- **What `docker run` gives an image that names nothing else**, as far as the runtime-spec carries it:
  Docker's fourteen capabilities, mounts, masked and read-only paths (with the host's CPUs'
  `thermal_throttle`, as Docker and containerd mask them), pid, ipc, uts, mount and cgroup namespaces,
  devices denied but those every container has, no `noNewPrivileges` and no rlimits. `/sys/fs/cgroup` is
  the box's own cgroup2, read-only. `/etc/hosts` is Docker's lines and then the spec's, bound in from the
  bundle.
- **The spec's fields.** `init` is `process`: its arguments, environment, working folder (`/` unless given) and
  user. Mounts are binds: read-only ones not recursive, since a recursive bind remounted read-only leaves the
  mounts beneath it writable. `hostname` is the UTS hostname. `Network::Private` is a network namespace of
  the box's own, `Network::Host` none.
- **A node that isn't root** gives the container a user namespace mapping its own uid and gid, alone, to
  root, with runc's rootless changes: no `gid=5` for the terminals and, on the host's network, the host's
  `/sys` bound in (a sysfs needs a network namespace the user namespace owns). That bind must be
  recursive, and it carries the host's cgroup2 mount, writable under the read-only `/sys`, where a box's
  processes, running as the node's uid, could write every box's cgroup files. So the box's own cgroup2 goes
  over it, with a read-only tmpfs between: mount(2) refuses a filesystem over its own root (EBUSY).
- **Refused by name, nothing made** (CLAUDE.md §3): any value `OciSpec::check` refuses (a relative path, a
  `..`, a NUL, a hostname or hosts name that isn't 1-64 or 1-253 printable characters without a space or
  `#`, an address that isn't one, no layers, a mount over `/`), which the simulator refuses too (HEX-RT-13);
  then a user a rootless node can't map, more than 500 layers (`OVL_MAX_STACK`), overlay options past the
  4,095 bytes a mount(2) call carries, a layer or bundle path holding overlayfs's `,` `:` or `\`, a
  `config.json` past the runtime's 1 MiB, a record past the store's 1 MiB, and a root or mount source that
  isn't on the host.

Two fixes to the `oci` class came with it, each found by the step's lab, each a failing check first:

- **Loopback is brought up** in a network namespace a container makes, as runc and crun do: a fresh
  namespace's `lo` is down (flags 0x8), so even 127.0.0.1 was unreachable in a Hex container. One rtnetlink
  RTM_NEWLINK sets IFF_UP on interface 1 (`oci/net.rs`), built as bytes, with no `unsafe`.
- **Every path in a container's root is resolved inside it** (`oci/inroot.rs`). The init sets the root up
  before it pivots, while `/` is the host's, and it made a missing mount target with an ordinary path walk:
  a layer whose `/etc` was an absolute link to a host directory made the node write `hosts` there, on the
  host, as the node's user, as root for a root node. Now each mount target, masked and read-only path,
  device node and `/dev` link is opened with `openat2(RESOLVE_IN_ROOT)`, missing parts are made relative to
  the directory it resolved to, and a mount goes on the descriptor through `/proc/self/fd`, as runc mounts.
  A path whose link leads to nothing in the root is refused by name. Every `Bundle` box gets this fix too.

**Sources** (`docs/research/runtime/oci-spec-defaults.md`): moby's default spec (`daemon/pkg/oci/defaults.go`,
`caps/defaults.go`) and `/etc/hosts` (`daemon/libnetwork/etchosts`); containerd's default spec
(`pkg/oci/spec.go`, `mounts.go`); runc's `ToRootless` (`libcontainer/specconv/example.go:161-220`) and
`mountViaFds` (`libcontainer/mount_linux.go:146-200`); the kernel at v6.19 (overlayfs.rst, `OVL_MAX_STACK`,
`copy_mount_options`, "Refuse the same filesystem on the same mount point", `LOOPBACK_IFINDEX`);
openat2(2).

**Evidence.**
- Golden tests (`crates/hex-core/src/bundle.rs`, `testdata/bundle/`): four shapes (layers or a directory;
  root or rootless; a private or the host's network), each `config.json` compared field by field with one
  written from this section, and every hostile value above refused by name.
- Doc-truth (`hex-node`, `node::rendered`): every shape parses through the runtime's own model and check
  with each field where it was rendered, so a misnamed field can't be dropped unseen.
- The lab (`crates/hex-cli/tests/oci_spec_box.rs`), on adacachy, Linux 7.2.9, 2026-10-27, both rootless
  (uid 1000, a delegated systemd scope) and as root (a privileged container, the bench's setting). The plan
  step's example, layers [base, app] with `sleep infinity`, one read-only bind and a private network: the
  root is the overlay over the box's own layer; `/usr` is the bind and refuses a write; the hostname,
  `/etc/hosts` and network namespace are the spec's, the network holding `lo` alone, up; a new file, a
  changed layer file and a removed one land in the box's layer as a file, a copy and a whiteout, the layers
  untouched. A directory root on the host's network writes in place. After every delete, nothing of the box
  is left: no bundle, no container state, no cgroup, no record, no mount. HEX-RT-1 to 13 pass on
  OciSpec boxes. A link out of the root writes nothing on the host and is refused by name, and a link inside
  it is followed there. Recovery removes a half-made bundle whose layer has a mode-0 directory and a deep
  tree.
- runtime-tools bd3b0bd on adacachy (`bench/oci-validation/run_hex.sh`, fedora:44, Linux 7.2.9,
  2026-10-27), the same suite against `hex` before this step (a78b37f) and after it: 26 of 58 pass for
  both and no test's result differs. The 26 include `default`, `mounts`, `linux_devices`,
  `root_readonly_true` and `linux_rootfs_propagation`, which go through every path the resolver now opens;
  the 32 that fail for both fail for this host's reasons, as they did before (cgroup v1 files the suite
  looks for, among them).

**What would falsify it.** A leg that passes under `docker run` failing in an OciSpec box because of a
default here (T-T7.1.1): `noNewPrivileges` left off, no rlimit, or `/etc/hosts` written fresh where Docker
copies the host's (the host's network with no lines added). An image whose layers' options pass a page,
which the new mount API's `lowerdir+` (Linux 6.8) would carry. A kernel that refuses unprivileged
overlayfs.

**Open, for Ada.** Docker also applies its default seccomp profile and, where the host has AppArmor, the
`docker-default` profile. An OciSpec box gets neither: the spec names none, and Hex applies no LSM profile
(Ada's call, 2026-10-27, keeps those refused). Docker's default spec also has a time namespace, which Hex's
runtime doesn't make. Whether OciSpec boxes get the default seccomp profile (Hex compiles containers/common's)
is a security posture for the person to choose.

## Decision (proposed 2026-10-31, for Ada): each OS runs its own boxes natively; a Linux guest only for Linux images

Ada's requirement (2026-10-31): "Why can't we run natively on Windows and MacOS? This is a requirement." The
evidence is `docs/research/runtime/native-platforms.md` (§n below are its sections).

- **Hex runs natively on all three OSes:** the node agent, its seam, its store and the scheduler are one code
  path (CLAUDE.md §8). Only the boxes differ, behind the one `Runtime` trait, and every OS's classes pass its
  conformance suite (`HEX-RT-1..9`).
- **`process`:**
  - Linux: cgroup v2, as today.
  - Windows: a job object per box, nested under the node's. Memory is the job's memory limit; CPU a weight
    or a hard cap; processes the active-process limit; delete is kill-on-close (§2). Limit events come from
    the job's accounting, read synchronously: completion-port messages may be lost, and the memory
    notification Windows calls guaranteed is set seconds late under load, or never once the member's
    commit has fallen back (CLAUDE.md §3; bench/install/records/windows-jupiter-20261105.txt entry 21).
  - macOS: a box runs as a user of its own, from a pool made at install, as Nix's build users are. A user
    is the only marker macOS lists that a member inherits and can't drop, since macOS can't follow forks
    (§3). So a privileged helper starts each box, and `RLIMIT_NPROC` of the box's user caps its processes in
    the kernel.
    macOS's memory bound has **three layers** (Ada, 2026-11-01). Layer one is **rootless and
    kernel-enforced** (step 1's `_ext` backstop): every member Hex spawns is armed at `posix_spawn` with a
    *fatal* per-process physical-footprint cap at the box's **whole** bound (`posix_spawnattr_setjetsam_ext`,
    the spawn SPI the macOS SDK ships no prototype for; flags from `bsd/sys/spawn_internal.h`), so the kernel
    `SIGKILL`s a member the instant its footprint crosses the bound — 0 overshoot, no privilege. It bounds a
    single-member box, and any one runaway member, exactly, and lets the watcher read a fully-armed box
    holding at most one member only when it changes (a fork, an end). The cap is per process, doesn't survive
    a fork, and caps each member at the whole bound, so it does not bound a multi-member tree's footprint
    *sum* — layers two and three do. Layer two, the **primary** for a many-member box, is kernel-enforced and
    event-driven, armed by the privileged helper (step 2; root alone suffices, `kern_memorystatus.c:9192-9193`):
    a per-task physical-footprint ledger limit (`task_set_phys_footprint_limit`) apportioned across the box's
    members so the limits sum to the bound, so the kernel fires a Mach `EXC_RESOURCE` the instant the tree
    crosses it (by pigeonhole some member crosses its share) — near-zero overshoot, ~zero watcher CPU,
    bounding the whole tree, and re-arming a forked child, which layer one can't. Layer three is
    **defense-in-depth**, the whole bound on a node with no helper: Hex's own polling rule, a timeshare
    thread (no priority over other programs, Ada's rule 2026-11-03) that reads each member's footprint
    (`proc_pid_rusage`) on `NOTE_CRITICAL` kqueue timers, every (bound + 64 MiB - footprint) / the node's
    machine class's recorded worst aggregate rate, learned up as it sees faster growth (51.05 GB/s for the
    M4 Max; floor 250 µs, no ceiling), and ends the box past its bound (an eviction by a stated rule,
    CLAUDE.md §2). It holds step 1's 64 MiB bar up to one member per core, its recorded limit; past that its
    thread doesn't run often enough, and the ledger layer holds the box (Ada's two-layer bound, 2026-11-01;
    the lead, 2026-11-06). A rootless, kernel-enforced per-process cap thus **does** exist — at spawn, layer
    one — but re-arming a *running* task and apportioning a bound across members need root
    (`memorystatus_control` → `EPERM`, `task_set_phys_footprint_limit` →
    `KERN_NO_ACCESS` unprivileged; `RLIMIT_AS`/`RLIMIT_RSS` → `EINVAL` and unenforced, `EVFILT_MEMORYSTATUS`
    absent, `proc_rlimit_control` has no memory cap; `bench/native/macos/mem_bound_probe.c`,
    `docs/research/runtime/macos-spawn-jetsam-cap.md`). Built and measured on the Mac Studio through the
    class's own path: layer one holds a 256 MiB single-member box to **0 MiB** overshoot for ~0.1% of a core
    held at its bound (`bench/native/macos/RESULT.md`, record `macos-box-ext-b285d07-20261101T223556Z.txt`);
    layer three alone holds the bound to ≤48 MiB with up to one member per core (~12-15% of a core held) but,
    being an eviction by a rule, can't tightly bound a box filling more than every core (28 members spike) —
    which is exactly what the step-2 ledger layer catches. Paced by headroom plus the bar at the recorded
    worst aggregate rate (no ceiling, 2026-11-06), layer three costs a many-member 256 MiB box 1.5-2.5% of a
    core far from its bound and 4-7.5% near it, and a one-member armed box nothing
    (`records/watcher-headroom-cpu-f18e6ee-20261106T1737Z.txt`). Under load the watcher's wake comes late
    (up to 79 ms past its wait on Mac16,9, its timeshare thread waiting for a core beside the box's own
    members and other programs; `records/watcher-wakes-load-0cb6e11-20261108T2114Z.txt`), and a box filling
    at the recorded rate passes its bar by what it grows meanwhile. So layer three holds such a box stopped
    (SIGSTOP of its group) across a wait it could pass its bar in, at the rate it was seen growing (the
    recorded rate while it takes on members) over the wait plus the latest lateness the node's watchers have
    seen (`hex_core::watch::Watch::holds`; the lead, 2026-11-08: the box's own bound enforced, as Linux's
    `memory.high` throttles a cgroup near its limit, slowing no other program). A held box runs only for its
    watcher's reads, at most one fewer of its members than the machine has cores, and is let go by a run in
    which it didn't grow; a member started beside another waits stopped for the watcher's next read; the
    watcher continues only what it stopped, so a member the box's own programs stopped stays stopped. It
    does not yet hold the bar: light checks on adas-mac-studio-3 (2026-11-08, launchd
    `hex-macos-resume-hold4-20261108T215447Z`, `-hold6-20261108T223549Z`, `-tip5-20261108T224311Z`, ambient
    load about 3) passed 3 of 8 runs, with 5 of 72 passes at 4, 8 and 14 members past 64 MiB (worst 175 MiB),
    and beside 7 or 14 CPU-bound programs a box passes it by up to 565 MiB. What stretches a held box's run
    is not yet taken apart. CPU follows the box's declared weight, as
    `cpu.weight` sets a box's share on Linux: a box at the default weight runs unclamped, at the QoS the
    node's own programs run at, and one whose weight is under the default runs at background QoS (darwin
    background: lowest CPU priority, I/O throttled). No clamp keeps the watcher ahead of a box (the lead,
    2026-11-06, under Ada's 2026-11-03 rule). A box that asks for confinement gets App Sandbox's, which every
    child inherits (§3). The class's own purpose is trusted code that must not starve
    the machine, and that purpose doesn't need it. Under the helper the ledger limits sum to the bound less a
    reserve (`hex_core::boxuser`), the fork window's room; layer three there is event-driven, with no
    period (the lead's call, 2026-11-06, under Ada's rule that no constant is picked): the daemon watches every
    member for its forks and its end (`EVFILT_PROC`, which root may place on any process), and each fork or end
    wakes a sweep of its own box (its watch's tag names the box), while a crossing, whose corpse doesn't name
    one, re-reads every box; each sweep arms a forked member at once and ends a box past its bound
    (`Daemon::pump_members`, `Daemon::pump_crossings`). A member that forks before its watch is placed is
    caught by the sweep listing the box again after each watch, until a listing finds no member it hadn't
    watched; from then on every fork posts an event (`docs/research/runtime/macos-box-user-and-ledger.md`).
    The helper's privileged calls are `crates/hex-runtime/src/macos/helper_sys.rs`, CLAUDE.md §12: its lines
    `d4252f3` (the file and its fork, user drop, ledger and exception arming) and `a960732` (the `mach_msg`
    crossing receive) were approved by Ada on 2026-11-06, as written, and so was `caa6e9f` (the child's
    `RLIMIT_NOFILE`). A node with no helper runs the
    class as step 1's, and says so in its inspect (`BoxInfo.membership`).

    **A program's end** is the helper's to collect, since it is the program's parent and `waitid` answers no
    one else (`docs/research/runtime/native-platforms.md` §3, "A box user's program ends where only the
    helper can see how"). The helper watches each program it starts from its start and reaps it, in its
    sweep or when asked, holding each end until the node reads it once: at most `RLIMIT_NPROC` programs a
    box (a start past that drops the box's oldest unread end, or is refused while all still run), and a
    deleted box's ends for as many boxes as the pool has users. The node waits on the program's `NOTE_EXIT`
    on a kqueue of its own and then asks for its end (`Wait`), so a wait is the fact, never a poll, and no
    end is made up as `Unknown` because the node couldn't reap a program it doesn't parent.
- **`oci`:**
  - Linux: as today.
  - Windows: Windows images run natively, process-isolated through HCS (the OCI spec's Windows section).
    Devices come only by class grant, and mounts are read-only where the spec says (§2). Production is
    Windows Server's; Windows 10 and 11 take it for development, with host and image versions matching.
  - Linux images on macOS and Windows: Hex's Linux node runs in a Linux guest that Shards boots through the
    OS's hypervisor (Hypervisor.framework, the Windows Hypervisor Platform), reached over vsock (§4). The
    class runs unchanged inside, goal 5's device programs and `mount_setattr` included. Hex reaches Shards
    through a Shards `-api` crate (CLAUDE.md §9), to be agreed with Shards' owner. Where Shards has no backend
    yet (macOS on x86_64, Windows on arm64), Hex gives a typed refusal naming that backend until it lands.
- **Setup is the installer's (Ada's call, 2026-10-31):** Hex's installer asks for admin rights once, as Docker
  Desktop's does. On Windows it turns on the Containers and Windows Hypervisor Platform features (usually a reboot);
  on macOS it makes the box users' pool and installs the privileged helper. After that, every class works with no
  further step. A node whose machine lacks what its installer sets up refuses the class by name, saying what is
  missing.
- **What would falsify it:** §6. **Measured before it is built on:** §7.

## Decision (native plan step 6, 2026-11-04): the installer, per OS — admin rights once

The plan's step: "Hex's installer asks for admin rights once", turning on what each OS's classes need, a node
lacking any of it "refuses that class by name, saying what's missing", and "Uninstall undoes it". The evidence is
`docs/research/runtime/installer-per-os.md` (§n below are its sections).

- **One script per OS, run once by a person with admin rights:** `scripts/install/linux.sh` (bash, as root),
  `scripts/install/windows.ps1` (PowerShell, elevating itself once through UAC) and `scripts/install/macos.sh`
  (bash, as root through `sudo`). Each has the same three verbs: `install`, `uninstall` and `check`, and
  `--dry-run` on the first two.
- **The plan is the record:** every thing install makes is one `<kind> <target>` line of a plan computed before
  anything is made (a file, a link, a folder it made, a feature it turned on, a user, a service). Install
  writes the plan to its manifest first, so an install cut short is undone too; `--dry-run` prints the plan
  and makes nothing; uninstall undoes the manifest's lines, Hex's own and nothing else (a folder only once
  empty, a directory only by Hex's names, a feature only if install turned it on), then lists every line
  still present, which is none. `check <plan>` is that listing on its own: on a machine fresh of Hex, and
  after uninstall, it lists nothing (CLAUDE.md §3: nothing is left silently).
- **Linux** (§1): the classes need a cgroup v2 subtree the node owns with `cpu`, `io`, `memory` and `pids`
  (`hex_core::cgroup::CONTROLLERS`), and the node a descriptor limit of 594,304: its own descriptors at its
  bounds (330,112) and every kept namespace's held `/proc/net` files (4 × 66,048). Install puts each named
  user's node in a **system** unit, the template `hex-node@.service`, run as that user (`User=%i`), its cgroup
  delegated to it (`Delegate=yes`: every controller the system has, §1.2; `DelegateSubgroup=agent`, so the node
  runs in the leaf `agent` and its boxes in `hex` beside it and cgroup v2's no-internal-processes rule holds,
  §1.1; systemd ≥254, refused by name below it) and `LimitNOFILE=1048576`, soft and hard (the kernel's default
  `fs.nr_open` and dockerd's old value, `docs/research/runtime/nofile-limit.md` §2-§3). Its socket is
  `/run/hex/<user>/node.sock`, its containers' state on the tmpfs under `/run/hex/<user>`, its records under
  `/var/lib/hex/<user>`. Not a user unit: systemd's own `user@.service` delegates only `pids memory cpu`, never
  `io` (v244-v259, §1.3); a user manager reads the controllers it may hand on only when it starts (§1.4); and a
  user unit can't take a hard limit above its manager's (nofile-limit.md §1.3). Measured in the lab: with a user
  unit, a drop-in raising `user@.service`'s delegation and limit, and the running manager re-executed, the node
  got `io` but stayed clamped at 524,288 descriptors; only a restart would give it more, and a restart ends
  everything under the manager, a CI runner's work included (the lead: never). A system unit takes both from
  root and touches no user's manager (HEX-INST-7: the lab's manager keeps its pid across install). A box whose
  spec names no `RLIMIT_NOFILE` gets Docker 29's 1024:524288, not the node's own (Ada, 2026-11-06;
  `hex_core::limits`, capped at the node's hard limit, which an unprivileged process can't raise): the `oci`
  init and every program exec'd into a container add it where the spec names none, and the Linux `process`
  class sets it in the child before `execve` (HEX-INST-6). `hex box run` isn't the node but wraps a program
  for its caller, so its program keeps the caller's limit (the lead, 2026-11-06). A `process` box always gets
  1024:524288, or the node's hard limit where that is lower: its spec (`BoxSpec`'s `Process` class and
  `Program`) has no rlimits to name one, and none is added, since the native boxes don't need it and a new
  field in the process spec is an API and wire change beyond this step (the lead, 2026-11-06). An `oci` box
  keeps a limit its spec names (end to end in `node_socket.rs`). On macOS only box-user boxes get the default
  (the helper's child sets it before `execve`); plain boxes keep the embedding process's, since posix_spawn
  sets no rlimit and a fork in the threaded node isn't safe (`docs/research/runtime/nofile-limit.md` §6).
  Rootless `oci` boxes need
  nothing more: each maps only the node's own uid (`hex_core::bundle`), so no subordinate id ranges (§1.6).
  Ubuntu 24.04's `apparmor_restrict_unprivileged_userns` (§1.6) would need an AppArmor profile for `hex`; no
  Ubuntu machine is in this step's bar, so that is a gap the installer doesn't fill yet (STATUS.md).
- **Windows** (§2): the `process` class's job objects need no admin. Install turns on the `Containers`
  (Windows images through HCS, plan step 4) and `HypervisorPlatform` (the Shards guest for Linux images,
  plan step 5) optional features with `Enable-WindowsOptionalFeature -Online -All -NoRestart`, records each
  feature it turned on (those `-All` brought too), and asks before the one reboot Windows asks for
  (`RestartNeeded`). It puts `hex.exe` in `%ProgramFiles%\Hex`.
- **macOS** (§3): install makes the box users' pool as Nix's installer makes its build users (§3.1: `dscl`, a
  group and hidden users with shell `/sbin/nologin` and home `/var/empty`, from a uid range it checks is free,
  as Sequoia's move of Apple's own users into 300-304 pushed Nix's to 351) and installs step 2's
  privileged helper as a launchd daemon (`/Library/LaunchDaemons`, `launchctl bootstrap system`), a plain
  property list rather than `SMAppService`, which needs a signed app bundle and the user's approval in System
  Settings (§3.4); an unprivileged node tells it is there by `launchctl print system/<label>` (exit 0, else
  113) and by its socket answering (§3.5). Without the helper the `process` class still runs, as a process
  group under step 1's watcher, and its inspect says so: Ada's call in the plan's step 2 (2026-11-01), "Both
  run on a node with the helper; polling alone runs on one without". The refusal by name covers only what
  can't run without the helper. Concretely (`scripts/install/macos.sh`): the pool's group is `hexbox`, and the users
  whose nodes may reach the helper are put in a group `hexnode`; the two gids are learned at install time:
  the first run of 2 ids in Apple's role-account range, 450-499 (`sysadminctl`'s usage on macOS 26.4, §3.1),
  that no user, group or process holds. The helper makes the pool's users (`_hexbox1`, `_hexbox2`, …) on
  demand, each at the first role uid then free, recorded in the manifest before it is made (the lead's
  call, 2026-11-06: no picked pool size; one account path, the helper's). Any taken name is refused by name before anything is made,
  and so is a range with no free run (`--first-id` names a start instead). The helper runs from a root-owned copy of `hex` in `/Library/PrivilegedHelperTools` (Docker's place
  for its helper, §3.2), never from `/usr/local/bin`, which Homebrew gives the user on Intel Macs, so no user can
  replace what runs as root. Its socket is `/var/db/hex/box-helper.sock` (persistent, so launchd's start at boot
  finds its folder), in a folder `root:hexnode 0750`: a new file takes its folder's group (open(2)'s BSD
  rule) and the plist's `Umask` 7 makes it `0770`, so only root and `hexnode` connect and another local user
  can't start or end someone's boxes. Uninstall boots the daemon out first, then ends every process left at a
  pool uid (a box's members outlive the helper that started them) before deleting the users, in rounds
  bounded by the installer's wait.
- **A node lacking it refuses the class by name** (`hex_core::setup`): on Linux, a box root whose parent lacks a
  controller, isn't the node's user's, or holds processes refuses the box root of both classes, naming what's
  missing and the installer (HEX-INST-1); on Windows, the `oci` class names each feature that's off and says
  the installer turns it on (HEX-INST-2), the state read without elevation from `Win32_OptionalFeature`
  (§2.4: Microsoft lists no restriction on reading it, to be measured as a standard user on jupiter) by
  PowerShell run as the member of a job-object box of its own, so its wait is the class's bounded one and the
  box's end takes whatever it left, within `hex_core::setup::FEATURE_QUERY_PATIENCE` (twice the slowest
  recorded run); `hex_core::setup::features_off` parses its listing and refuses anything else by name. The
  installer asks the same query through `hex setup features` (one `<name> on|off` line each) rather than
  querying Windows itself, so there is one query and one bound (HEX-INST-9).
- **Done when** (the plan's criterion): on a machine fresh of Hex the one admin step leaves every class working
  with no further step, and uninstall leaves nothing of Hex's behind, checked by listing what install made.
  The bar is not narrowed per OS: on Windows the `oci` class is native plan steps 4 and 5, so Windows' part is
  met only once they land and its install lab is run again then (the lead, 2026-11-04: sequencing, not a
  change to the bar).
  On Linux, `bench/install/linux_lab.sh` boots a fresh Fedora as a container with systemd its init, shows the
  by-name refusal before install, installs, passes the conformance suite over the installed node's socket for
  both classes as an ordinary user (HEX-INST-3), uninstalls and lists nothing left (HEX-INST-4).
  On macOS, `bench/install/macos_lab.sh` (one `sudo` run by an administrator, building as the invoking user)
  shows nothing of the plan before install and the box-user contract unmet without the helper, installs, passes
  the box-user HEX-RT contract over the installed helper's socket as that ordinary user, shows a user outside
  `hexnode` refused the socket (HEX-INST-6), uninstalls, and lists nothing left by the plan and by `dscl` and
  `launchctl` on their own (HEX-INST-7).
- **What would falsify it:** a system unit's `Delegate=yes` with `User=` that didn't hand the user every
  controller, or its `LimitNOFILE=` that the node didn't get (the lab reads both); an install that disturbed a
  user's running manager (the lab's runs before install and keeps its pid); a
  Windows feature whose state an unprivileged node can't read; a box user macOS lets a member drop.

## Decision: the Windows `oci` class is an HCS silo holding a Hex job (native plan step 4, 2026-11-04)

The native plan's step 4 builds the `oci` class for Windows images on Windows: process isolation through the Host
Compute Service, as the OCI runtime spec's Windows section defines it. Sources:
`docs/research/runtime/windows-hcs.md` (cited below as §n of that note).

- **A box is an HCS compute system, process-isolated, with a job of Hex's nested inside it.** Hex creates the
  container through HCS v2 (`computecore.dll`, schema 2.1, no hosting system, `ShouldTerminateOnLastHandleClosed` so
  a node agent that dies takes its containers with it; note §2). Every program Hex runs in the box — its init and
  each exec — is created *suspended inside a job object Hex owns* and then resumed, as a `process` box's member is
  (`hex_core::job`, step 3). That job nests under the container's silo (`nested-jobs.md:27,43`, note §5), so the box's
  `memory_max`, `pids_max` and CPU bound hold on its members by the `process` class's own measured rule, while the
  container's ~20 system processes (note §5) stay outside the box's count.
  - **Why not the silo's own job:** HEX-RT-6 bounds a box to 8 processes, and the silo holds more than 20 of its own;
    the silo's job is reachable only by an implementation-detail name, as SYSTEM, through an unbound NT call (note §5).
  - **Why a launcher:** HCS can't create a process suspended (note §4), and a member it starts runs at once, so a job
    assigned afterward races the member's forks. Hex's own `hex` binary is mapped read-only into each box at a fixed
    path, and HCS starts it as a launcher. The node opens the launcher by pid and duplicates into it the box's job
    (with only `JOB_OBJECT_ASSIGN_PROCESS`) and the caller's three standard handles (`DuplicateHandle`, note §4).
    The launcher reads one bounded request on its stdin and creates the member suspended in the job with only those
    three handles inheritable. It resumes the member, replies with the member's pid, and holds the member's handle
    until the node has opened it, so the pid can't be reused in between. Then the launcher exits. The node waits on
    the member's own handle, reads its exit as a `process` box's (`hex_core::job::exit`), and the member reads and
    writes the caller's pipes directly, with no copier thread. The request and reply are a sans-IO wire in
    `hex_core::hcs`, bounded, and every malformed one is refused by name.
  - **The container as a whole** is bounded too, by HCS, as the spec's Windows section says: `Memory.SizeInMB` and
    `Processor.Maximum`/`Weight` from the bundle's `windows.resources`. A `ClassSpec::Oci` box's container is bounded
    at its box bound plus the silo's own measured baseline (note §9, probe 3). Neither bound is dropped: a bound the
    job can't hold is refused by name, as the `process` class refuses it.
- **Signals and ends are the `process` class's:** KILL and TERM end the box's job (HCS has only console controls,
  hcsshim maps TERM to a shutdown and KILL to terminate, and Docker ends the container on either; note §4), so TERM
  ends this class's init, and its fixture says so to the differential suite (`term_ends_init`, true); a member's exit reads as the
  signal; any other signal is refused by name. An init that has ended leaves the box `Empty` with its exit, and an
  exec into it is refused `NotRunning`. Delete ends the job, terminates the compute system, detaches the scratch and
  removes what the box made. Every HCS wait has a deadline, and a timed-out operation is cancelled, never abandoned
  (note §2).
- **Layers by `vmcompute.dll`'s layer calls, as hcsshim, runhcs and Docker make them** (changed 2026-11-06 on
  jupiter's evidence, the lead's approval; note §11). The box's scratch is a folder holding a copy of the base layer's
  `blank.vhdx`; `ActivateLayer` attaches it as a volume, `PrepareLayer` puts the storage filter over it with the
  read-only layers as its parents (each named by `hex_core::hcs::layer_guid`, the id the document gives it),
  `GetLayerMountPath` gives the volume, and the volume's root is the container's `Storage.Path`. Teardown is
  `UnprepareLayer` then `DeactivateLayer`; after a container has run, `UnprepareLayer` answers
  `ERROR_FLT_DO_NOT_DETACH` (the container's end took the filter), matched alone. The calls are undocumented, so
  they're resolved at run time like HCS's, and a missing one refuses the class by name. hcsshim's timed retry and
  global lock are not copied (CLAUDE.md §4); 32 boxes started at once on jupiter each got their own volume.
  - **Why not the documented calls:** on Windows 11 25H2 (26200), a scratch made by them (a `blank.vhdx` copy or a
    fresh `HcsFormatWritableLayerVhd` disk, attached, `HcsInitializeWritableLayer`, `HcsAttachLayerStorageFilter`)
    never finished starting: the silo stopped after one service host, and `HcsStartComputeSystem` ended only when
    cancelled. The same document over a vmcompute scratch started in 0.6 s (note §11). What would falsify this: a
    host where the documented sequence boots a silo, which returns the class to documented calls.
- **"A device not granted isn't visible" is checked as reachable** (the lead's ruling, 2026-11-06, for Ada's
  report). The step's words: "A device not granted isn't visible". Under process isolation a silo's PnP listing is
  the host's: `CM_Get_Device_Interface_ListW` for the display adapter class gives 2 on jupiter's host, 2 in a Hex box
  and 2 in a Docker `--isolation process` container, none granted. Microsoft's guide says a grant means "all devices
  which identify as implementing the requested class will be plumbed into the container"
  (`hardware-devices-in-containers.md:39`): a grant makes devices reachable, not listed. So the probe opens each
  listed interface (a query open, no access asked): ungranted, USB, keyboard, mouse and disk classes open 0 of n in a
  box; granted, n of n (note §11). The test uses the USB device class with both cases, since jupiter has none of
  Microsoft's four supported classes and HCS refuses a display-class grant at create for Docker and Hex alike. Only
  Hyper-V isolation, which is not this class, would hide the listing itself.
- **Hex's Windows binaries carry their own C runtime** (`.cargo/config.toml`, `+crt-static` for both Windows
  targets): the launcher runs inside the image, and nanoserver has `ucrtbase.dll` but no Visual C++ runtime, so an
  `hex.exe` importing `VCRUNTIME140.dll` couldn't load there (note §11). It's the Windows twin of the static-glibc
  decision, and `crates/hex-cli/tests/windows_static_crt.rs` refuses a build importing `VCRUNTIME*`/`MSVCP*`.
- **The spec, as the OCI Windows section defines it:** a `ClassSpec::Bundle` reads `config.json`'s `windows` section
  (`layerFolders` topmost first with the scratch last, `devices`, `resources`) and its mounts. A `ClassSpec::Oci` box
  renders the same, its `Rootfs::Layers` lowest first. Devices are granted only by interface class, with `class` and
  `vpci-class-guid` both meaning one. A mount is read-only where its options say `ro`, and nested destinations are
  refused. A field Hex doesn't render is refused by its name (`hyperv`, which is a Hyper-V container, not this class;
  `credentialSpec`; `servicing`; `network.endpointList` until Hex holds HNS endpoints; a uid/gid `user`; a
  `root.readonly` of true; a `Rootfs::Dir`, which a Windows container can't run from). A spec isn't half-applied.
- **Versions, refused by name before HCS is asked:** the host's build and kind come from `RtlGetVersion`, and the
  image's build from its base layer's `Files\Windows\System32\ntdll.dll`. Microsoft's table decides (note §7): a
  Windows Server host runs its own build, and a Server 2025 host runs Server 2022 images too. A Windows 11 host runs
  Server 2022 images, and Server 2025 images from 24H2 (26100) on. A Windows 10 host runs none process-isolated. Any
  other pair is refused, naming both builds. HCS's own mismatch code (0x80370101, or 0xC0370101) maps to the same
  refusal. Production is Windows Server's; Windows 10 and 11 hosts are for development (`hyperv-container.md:76`).
- **The Containers feature, refused by name:** Hex resolves the HCS, storage and virtual-disk calls at run time
  (`LoadLibraryExW` from System32, `GetProcAddress`), because windows-sys links them at load time and a missing
  `computecore.dll` would stop the whole node from starting (note §7). A missing module or export means the class is
  refused, saying the Containers feature is off and Hex's installer turns it on. `classes()` lists `oci` only where
  HCS loads.
- **Unsafe:** the HCS, vmcompute layer, process-handle, version and device calls sit in one file,
  `crates/hex-runtime/src/windows/oci/sys.rs`, named in CLAUDE.md §12 by a commit of its own (c4961c1, which replaced aadbd4e's list), approved by Ada as written on 2026-11-06.
- **What would falsify it:** a job handed into the silo that doesn't nest, or that the member escapes; the
  launcher being unopenable for `PROCESS_DUP_HANDLE` from the elevated node; the documented storage calls unable to
  make a scratch HCS starts; HCS refusing an image the table allows on jupiter, or starting one it forbids (each a
  probe in note §9, run on jupiter before the class claims it).
- **Done when (the step's criteria):** HEX-RT-1..9 pass for this class on jupiter with a matching nanoserver or
  servercore image. A device not granted isn't visible, a read-only mount refuses writes, and every gate passes.
  Create, start and delete through the seam are measured against Docker's and containerd's (runhcs) on jupiter, with
  the same image in the same run, and recorded in `bench/`.

## Decision: one differential suite on every OS, the simulator its oracle (native plan step 7)

CLAUDE.md §8 asks that one suite pass on every platform. The conformance suite (`hex_api::conformance`,
HEX-RT-1..13) checks each call alone; the differential suite (`hex_api::differential`, HEX-DIFF-1..6) adds
scripts of calls on several boxes and programs at once, played on the class under test and on the
simulator, and compares what each step shows (`docs/research/runtime/differential-suite.md`).

- **One suite, run as each OS's tests:** `differential::run` is the conformance suite and then every
  scenario. Each class's own test calls it with its own fixture: Linux `process` (`process_box.rs`), `oci`
  from bundles and from specs (`oci_box.rs`, `oci_spec_box.rs`) and both over the node's socket
  (`node_socket.rs`); macOS `process` (`macos_process_box.rs`); Windows `process`
  (`windows_process_box.rs`). A class that lands later (Windows `oci`, the Shards guest) joins by calling it.
- **The oracle is the simulator for the same class**, made the same way (process, bundle or spec). A step
  shows a normalized fact: a refusal's kind, a box's state and its init's exit, a program's exit (or only
  whether it exited 0, or whether a signal ended it, where OSes differ by design), its output, whether
  list shows a box, whether a memory kill is counted.
- **A reading waits for the fact:** a step that reads a box reads again until it shows the oracle's
  answer or `PATIENCE` passes, so a late report passes and a missing one fails (CLAUDE.md §4).
- **A divergence is fixed where the contract says.** The first two it found:
  - A Windows box with nothing running took a signal, where the simulator, Linux and macOS refuse it as
    `NotRunning`; Windows now refuses it (HEX-DIFF-2).
  - An `oci` box signalled TERM kept its init running on fedora, where the simulator ended it. The kernel
    is right: a PID namespace's init takes from an ancestor namespace only the signals it has a handler
    for, and KILL and STOP, which are forced (pid_namespaces(7), man-pages 6.19); Docker's stop waits out
    the same rule before it kills. The simulator was wrong, and now models it: a signal other than KILL
    ends a box's exec'd programs and leaves its init running (HEX-DIFF-1, HEX-DIFF-2).
  - On the Mac Studio, a macOS box whose members had all ended refused the next program (it was spawned
    into the dead leader's process group, which no process can join), and took a signal with no member
    alive (killpg's ESRCH read as success). The next program now leads a new group, and such a signal is
    refused as `NotRunning`, as on every other class (HEX-DIFF-1, HEX-DIFF-2).
- **What TERM does to an `oci` box's init is the class's** (the lead's ruling, 2026-11-05). Linux's is the
  rule above. Windows has no POSIX signals: HCS's ProcessSignal carries console control events only, and
  hcsshim maps TERM to a shutdown and KILL to terminate (`internal/signals/signal.go:185-197`), so Windows'
  `oci` class ends the container's job. Both are right for their platform, so the fixture says which
  (`Fixture::term_ends_init`, false by default), and the oracle is a simulator made with that answer
  (`SimRuntime::with_term_ending_init`), as HEX-RT-6 checks the effect of a refused fork where the count is
  not kept. Neither class's behavior changes for the suite.
- **Non-vacuity:** runtimes that are the simulator but for one lie (a signal of nothing taken, a deleted
  box kept, a box stuck running, an exec refused after the last program ended) each fail the scenario that
  shows it, at the step named; one that reports a box late passes; a runtime whose init ends on TERM passes
  with a fixture that says so and fails HEX-DIFF-2 without (`crates/hex-api/src/differential.rs` tests).
- **What would falsify it:** a behavior two classes show differently that no scenario reaches (the
  scenarios grow with it), or the simulator diverging from the design text (then the simulator is fixed
  and every class re-run).

## Decision: a program is held to its OS's exec bound, before any spawn (HEX-RT-14)

`Program` carried an environment of any size, and only Linux's execve stopped it (`E2BIG`, after the spawn
began); Windows bounds no environment block at all. CLAUDE.md §2 and §3 ask a bound and a typed refusal, so every
class refuses a program past its OS's exec bound as `Invalid` naming the program, the sizes and the bound's source,
before anything is spawned (`docs/research/runtime/exec-size-bounds.md`).

- **One count:** `Program::exec_size` counts as Linux's execve does (each string and its NUL, the filename again, a
  pointer per string), which is at least what XNU counts.
- **Each OS's bound, from its source:** Linux reads the kernel's own formula at run time (a quarter of
  `RLIMIT_STACK`, capped at three quarters of `_STK_LIM`, never under `ARG_MAX`; 32 pages a string; `fs/exec.c:410-416`,
  `binfmts.h:15`), not glibc's `sysconf(_SC_ARG_MAX)`, which skips the cap. macOS reads `kern.argmax` (XNU's
  `NCARGS`). Windows, whose block Microsoft documents as unbounded, takes a census of real container environments with
  a stated margin, and 32,767 characters a variable (native step 4). The simulator holds Linux's floor, `ARG_MAX`.
- **Whose stack limit, and when it is read:** a box's own (its spec's `process.rlimits` `RLIMIT_STACK`), else the
  node's, read once and kept, so an exec makes no system call for it. A limit lowered after that reading is learned
  where the program meets it, on the failure path alone: as root, the agent's vfork-style child reports the kernel's
  `E2BIG` through memory the two share, and the agent reaps it and refuses the program; rootless, the seed (started
  under the lower limit, which its programs inherit) refuses the program by its own reading with a refusal of its own
  on the seed wire (code 5, appended). Either way the node reads its limit again, once, and refuses the next exec
  itself. Rootless, the seed wire's one request (160 KiB, `hex_core::seed::MAX_REQUEST`) bounds a program too: one
  past it is refused by name before it is sent, and the seed kept. **Open gap:** a rootless program between that
  request and the kernel's own bound (a quarter of `RLIMIT_STACK`, 2 MiB at an 8 MiB stack) is refused, where runc and
  crun, bounded only by the kernel, run it; as root it meets only the kernel's. Closing it needs a request carried over
  several `SOCK_SEQPACKET` records, a kind appended to the seed wire (STATUS.md, native step 7).
- **Over the node's socket** the wire's one-message bound is tighter than Linux's, so the client refuses past it, as a
  program, before encoding.
- **Tested by** HEX-RT-14 on every class's conformance run (a fixture that states no bound fails it), and a unit test of
  the count against the kernel's arithmetic.
- **What would falsify it:** a program the count admits that the OS's exec refuses with `E2BIG` (the count is short),
  or a kernel whose formula changes; either is fixed in the count or the read, and HEX-RT-14 re-run.

## Decision (proposed, native plan step 8): the Windows node, on one completion port, its boxes outliving it

Step 8 puts Hex's node agent on Windows over the one socket core every OS shares (`hex_node::serve`, its `Os`
trait; step 8a, `cp/t7-native-s8` 6c900ccc). This proposes Windows' `Os`, how an exec's stdio reaches its caller,
and how a box outlives its node so a restarted node takes it back (T-T7.1.2). It's proposed: the parts marked
*to measure* are checked on jupiter before the code lands, and the stdio part waits on its record.

**Events: one I/O completion port, every source on it by a documented call.** The core's `Os::wait` is
level-triggered readiness (epoll, kqueue); a completion port reports finished operations. The Windows `Os`
turns each source into a completion on the one port (`GetQueuedCompletionStatusEx`):
- *Wake*: `PostQueuedCompletionStatus` with the wake's key.
- *A program's end*: `RegisterWaitForSingleObject` on the member's process handle, its callback posting the
  program's token to the port. Not the job's `JOBOBJECT_ASSOCIATE_COMPLETION_PORT` messages, which the plan
  names: Microsoft documents that, but for notification limits, their delivery "is not guaranteed", a lossy
  source CLAUDE.md §3 forbids (`docs/research/runtime/windows-job-objects.md` §4). The process handle's
  signal is the kernel's own end of the process, so no end is missed, and a pid given out again can't be
  mistaken for it (the handle names the process, not the pid).
- *A connection readable*: one overlapped `WSARecv` into the connection's buffer, posted only while the
  core reads the connection and no message read from it waits, so a client the core has stopped reading
  holds one buffer and what was decoded from it. Its completion is the bytes (or the peer's end, a read of
  0). *Measured* (jupiter, 2026-11-08, `stdio_probe iocp`, record
  `bench/native/records/iocp-jupiter-07da2bc9-*`): a zero-byte `WSARecv` is a readiness signal on AF_UNIX
  as on TCP, pending while quiet and completing without taking bytes, on data and on the peer's end. The
  node posts real reads, which need none of that, so it reads in one step what readiness would take two
  for.
- *A connection writable*: `Os::send` (a whole message or none) issues one overlapped `WSASend` that owns
  the message; while one is outstanding the next send is `WouldBlock`, and its completion is "writable".
  The bound on a connection's waiting answers stays the core's. *Measured* (the same record): one 8 MiB
  overlapped send is taken whole at once, before its peer reads anything, so a send's completion says the
  kernel holds the bytes, not that the peer read them, and the socket doesn't push back at that size: the
  core's bound on waiting answers is what bounds them.
- *A connection waiting*: overlapped `AcceptEx` on the listener, one at a time. *Measured* (the same record):
  afunix.sys completes it on the port when a client connects, and the socket it took reads its peer's pid
  once `SO_UPDATE_ACCEPT_CONTEXT` is set. The accepted socket joins the port before the accept starts, and
  once only (a second join is refused, `ERROR_INVALID_PARAMETER`: `tests/windows_node.rs` found it).
- *Who the peer is*: `SIO_AF_UNIX_GETPEERPID`, then the process's user from its token, as Linux's
  `SO_PEERCRED`.
Messages are framed over the stream (`hex_core::stream`), as on macOS: Windows' AF_UNIX is `SOCK_STREAM`
alone. **Falsifier:** a completion that never comes for a source that fired (a lost end or read), shown by a
test that counts every program's end and every message against the port's completions.

**An exec's stdio: a transport of its own, chosen by measurement (step 8's record, to be committed).**
Windows' AF_UNIX passes no handles, so the four candidates are measured on jupiter by
`bench/native/windows/stdio_probe` (first byte and throughput, N >= 299 interleaved, the machine's load over
the last second recorded each round): the streams framed over the caller's own connection; a connection per
stream; a named pipe per stream; and an anonymous pipe whose read end the node duplicates into the caller's
process (`DuplicateHandle`, the caller named by `SIO_AF_UNIX_GETPEERPID`). Only the caller may read or write
its streams. Framed and duplicated streams have no door a second process can knock on: they're the caller's
own connection or handle. A connection per stream and a named pipe per stream are doors, so the node takes
them only from the caller's pid (`SIO_AF_UNIX_GETPEERPID`; `GetNamedPipeClientProcessId`, the pipe first
instance only and remote clients refused). The probe's refusal check gives a second process of the caller's
user the door first: the node refused it for both and still served the caller every byte (jupiter,
2026-11-07, `stdio_probe refuse`). A process of *another* user, on a temporary account Ada approved, is
refused before any node code by the socket file's own permissions (`WSAEACCES`), and, granted the file, by
each door's check, while the caller gets every byte; `hex node` refuses such a connection by name (jupiter,
2026-11-08, `stdio_probe refuse-as`, `bench/native/records/other-user-jupiter-20261108T0220Z.txt`).
What no transport can stop: a process of the caller's own user may open the caller's process and take any
handle in it (`PROCESS_DUP_HANDLE`), its own connection included, so "only the caller" means only through
the node, as Linux's `SCM_RIGHTS` means it. The fastest candidate that passes is chosen when the timed run of
record is in.

**Recovery: a box outlives its node through a keeper, and the restarted node takes it back (T-T7.1.2;
the lead's decision, 2026-11-08).** Today a Windows box ends with the node's last handle: its job is
kill-on-close (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`), and an `oci` box's compute system is made with
`ShouldTerminateOnLastHandleClosed` (the HCS decision above). That is clean up by construction. Recovery as
on Linux takes a live box back, which needs the box's job to outlive the node *without* losing the
guarantee that no box outlives Hex. Both facts force the shape:
- *A named job's name lapses with its last handle.* *Measured* (jupiter, 2026-11-08,
  `sys::tests::a_named_jobs_name_lapses_with_its_last_handle_though_its_member_runs`,
  `cp/t7-native-windows-outlive`): a named job opens by name while a handle to it is held, but once its last
  handle closes its member runs on and its name is gone. The object manager drops a temporary object's name
  with its last handle, though processes still hold the object. So a name alone can't carry a box past the
  node: something that outlives the node must hold a handle to each box's job, or the next node finds nothing
  and the box's processes are left in a job no one can open (a §3 leak).
- *The holder is a keeper, and kill-on-close stays.* Each box's job is a *named* job object (`CreateJobObjectW`
  with a name under the node's own namespace, `Local\hex-<node>-<box>`), kept kill-on-close, and the node
  **and** a keeper process each hold a handle to it (the node duplicates the job into the keeper,
  `DuplicateHandle` with `PROCESS_DUP_HANDLE`, and closes it there on delete, `DUPLICATE_CLOSE_SOURCE`).
  Then: the node crashes → the keeper's handle keeps the job and its name, and the next node reopens it by
  name (`OpenJobObjectW`) and takes the box back, or ends it (`TerminateJobObject`); the keeper dies while
  the node lives → the node sees its end on its port and starts another, duplicating the handles again; both
  are gone → the last handle closes and kill-on-close ends the boxes, so nothing is left in a job no one can
  open. The keeper is wholly passive — it only holds handles — so it needs no trust and no logic of its own;
  it is `hex keep`, started by the node. This replaces "refuse by name with boxes recorded": kill-on-close is
  the backstop, not a refusal. *Built and measured* (jupiter, 2026-11-08, `cp/t7-native-windows-outlive`,
  `windows::tests` 6/6): a box outlives its runtime and the next takes it back as running; a box no node
  takes back is ended by name; a box whose node and keeper are both gone is ended by kill-on-close; a keeper
  that dies while its node lives is replaced.
- *The keeper's own lifetime.* The keeper holds its handles and waits, with no timer, until its stop is
  asked — a node going down cleanly, or a node replacing it. It never exits on its own: a node that never
  returns leaves its boxes running, as a box on Linux waits for its node with no timer (the installer's
  service restarts the node, and recovery is its first act). No picked hold constant. The node signals the
  stop event only on a clean shutdown, never on a respawn: the stop is shared by the state folder, so
  signalling it would also stop the new keeper.
- *Breakaway.* The keeper holds handles only; it need not be a member of the node's own job. A child
  inherits the node's job membership unless it is made to break away, so the keeper's lifetime is that job's.
  Under a kiln run the node (and so the keeper) sits in kiln's job, which kiln holds: the node can die inside
  the run and the keeper lives on, and the run ending ends them both (a dev/test node's boxes don't outlive
  the run, as a crashed Linux node's wait on the service). *To measure* (under a kiln run on a clean
  jupiter): that the keeper survives the node's death within the run. The installer's service is the real
  case, and the SCM runs a service in *no* job unless the service makes one, so a keeper the service starts
  is in no job and outlives any node; that is measured for real when the service exists (step 6). No
  temporary service is made on jupiter (an admin change not approved).
- *The `oci` side* (step 4's agent, `cp/t7-native-s8-oci`). An outliving node's `oci` box is its named job,
  held by the keeper as a `process` box's is, and its compute system is made without
  `ShouldTerminateOnLastHandleClosed` (`hcs::Lifetime::Outlives`), so the container runs on past the node's
  last handle; the next node reopens it by its id (`HcsOpenComputeSystem`; HCS naming no such system is
  nothing to take back). The box's scratch marker is its record (`hcs::BoxRecord`): its scratch folder and
  what Hex made, that it outlives, its system's id, that a start was asked (written before HCS starts the
  container), and its init's pid and creation time (`GetProcessTimes`, written once it runs), each write
  whole (a `.next` file moved over the marker), its size bound derived as `MAX_WIRE` counts a folder. The
  scratch becomes the container's only once HCS has made the container, so a create that fails before that
  undoes it and leaves nothing attached. At a node's start, reclaim leaves an outliving box's record and
  scratch while HCS still holds its system (`hcs::at_start`) and reclaims every other. `adopt` takes the box
  back (`hcs::take_back`) only while HCS holds its system: as made when no start was asked, and with its
  init only when the process at the init's pid has the recorded creation time and still runs. A start its
  node didn't finish recording, an ended init, or a pid given to another process since is lost, and `end`
  ends its container, scratch and record (`oci::end`). An outliving node keeps these records under its own
  state folder (`<state>\boxes`): every runtime made in process keeps its records in the machine's shared
  `%ProgramData%\hex\boxes` and reclaims that whole folder when made, so a node or a suite starting beside a
  running bench would detach a live bench box's scratch. *From step 4*: a box's scratch survives a node kill
  (the crash test asserts `sandbox.vhdx` is still there after the kill, in the created and the started
  phase; green, jupiter 2026-11-08 00:12Z), and with the flag on HCS ends a killed node's compute system
  up to 2 s on record (one idle sample at 1 s resolution), to be retaken on a clean jupiter
  (`cp/t7-native-s4`); with the flag off it holds the
  container all through that bound (`windows_oci_box`'s `outlived`). *Built and measured* (jupiter,
  2026-11-08, `windows_oci_box`): an outliving box's container and scratch wait for its next node, and a
  container reopened by id is ended through the reopened handle (kiln run r105); a node's `oci` box outlives
  its runtime, the next runtime on the node's name takes it back running and runs a program in it through
  the reopened container, and its delete leaves nothing; a box no node takes back is ended with its
  container, scratch and record (kiln run r160, 3/3; the tree before fails each at `outlived`, HCS ending the
  container 315-753 µs after its last handle).
- A box's record names each member by its pid and its creation time (`GetProcessTimes`), so the restarted
  node, opening the pid, tells the member from a process given its pid since, as macOS uses a process's
  start time. The record names the keeper the same way (its pid and creation time), so the next node adopts
  the live keeper or starts a new one.
- The record's phases stay Linux's (`creating`/`ready`/`deleting`, written before the machine changes), so
  a node killed mid-create or mid-delete leaves a box recovery finishes or removes.
- Linux images in the WHP guest: the guest outlives its node, as on macOS, and is taken back the same way:
  the restarted node re-attaches to the running guest, then the guest's own node recovers its boxes from
  its own records (T-T7.1.2 inside the guest). The macOS decision on `cp/t7-native-s8` (step 8's core) sets
  how; the Windows guest follows it, so a guest box is no longer removed because its guest went with the node.
What it costs: one idle keeper process per node, and a handle duplicated into it per box; a node that dies
and never restarts leaves its boxes running (the keeper holds them), as a crashed Linux node's boxes wait
for the service to bring it back.
**Falsifier:** the crash-restart tests (step 8's): the node killed with boxes running, mid-create and
mid-delete, then restarted on the same state directory; each must fail on the tree before this change and
pass after.

## Decision (native plan step 8, 2026-11-08): a macOS rootless box outlives its node and is taken back with a new watcher

T-T7.1.2 asks that a node restarted after a crash take each live box back, as Linux's does; the step names "macOS
rootless boxes left with no watcher" as what must change to meet it. Linux's boxes are cgroups, which outlive the
node by construction. A macOS rootless box is its process group (the leader's pid), held by a watcher thread in
the node, so the node's death leaves the members running without a bound.

- **What survives.** The members do: when a process's parent exits, XNU reparents its children to launchd
  (`bsd/kern/kern_exit.c`, `proc_reparentlocked` to `initproc`), which reaps them, so they run on, unbounded,
  until the node is back. Their process group is unchanged.
- **What names them.** The group id with its leader's start time (`proc_pid_rusage`, `ri_proc_start_abstime`),
  read at the leader's spawn while it is the node's unreaped child, and recorded with the box in the node's
  state (the record's `init`, a `ProcId`, its `start_ticks` holding the Mach start time). The group is still the
  box's when its leader runs with that start time, or when no process has the leader's pid. A live process with
  that pid and another start time was given it since and leads a group of its own: the box is then taken back
  empty, and the rootless gap the class already states (a recycled leader pid) is why box users close it.
- **Taken back.** The restarted node runs Linux's decisions (`hex_core::recovery`) over its records
  (`hex_node::recorded::Recorded::recover`). A record made is taken back: the class makes a box on the recorded
  group (`ProcessBox::adopt`), and its watcher lists, measures and ends the members as it does its own, since none
  of that needs parenthood. The members' own ends are launchd's to reap, and their exit statuses aren't the
  node's. A record being made or removed is removed, and what its group still runs is killed
  (`process::end_group`).
- **The fact it waits on.** XNU's `EVFILT_PROC` catches only events after its attach (`bsd/kern/kern_event.c`,
  `filt_procattach`). So the node's socket re-reads a watched program's `proc_pid_rusage` after attaching and
  reports an end that came before. Measured on adas-mac-studio.local-2 through `hex node`: without it, 2 of 12
  suite runs stalled 60 s on a lost end; with it, 0 of 12.
- **Evidence.** crates/hex-cli/tests/macos_node_recovery.rs: `hex node` killed (SIGKILL) with a box running, one
  empty, one mid-create (held in its record write by a FIFO at the write's temp path, and seen held there by
  `sample(1)`) and one mid-delete (its record deleting, its member running). Restarted on the same state, it takes
  back the running box with its member still running, which a delete then ends, and the empty one. It leaves
  nothing of the others: no record, no temp file, no member. The tests failed on the tree before the change
  (NoSuchBox; the temp file left; the member not ended) and pass after.
- **What would falsify it.** A rootless member that changes its process group without `setsid` (`setpgid` into
  another group) leaves the box as a daemonizing one does, and is lost to recovery as it is to the watcher. A
  macOS that stopped reparenting orphans to launchd, or reaped a group's members with its leader, would end
  boxes with their node. Box users name a box by its uid, which recovery reads the same way, so neither gap is
  theirs.

## Open (STATUS.md)

- Each OS's own boxes, natively (the decision above, proposed): under way. The **macOS `process` class is
  built** (`hex_runtime::macos`, native plan step 1): HEX-RT-1..9 pass on the Mac Studio, measured in
  RESULT.md. Rootless, memory is bounded by two layers that need no privilege: the kernel caps each member
  Hex spawns at the box's whole bound with a *fatal* per-process jetsam limit (`posix_spawnattr_setjetsam_ext`,
  step 1's `_ext` backstop — 0 overshoot for a single-member box, the watcher ceiling-paced at ~0.1% of a
  core; `bench/native/macos/RESULT.md`, record `macos-box-ext-b285d07-20261101T223556Z.txt`,
  `docs/research/runtime/macos-spawn-jetsam-cap.md`), and beneath it the polling rule holds a box of up to one
  member per core. That cap is per process (a fork escapes it) and doesn't bound a multi-member footprint
  sum, and re-arming a running task or apportioning a bound across members needs root
  (`memorystatus_control` → `EPERM`, `task_set_phys_footprint_limit` → `KERN_NO_ACCESS` unprivileged;
  `bench/native/macos/mem_bound_probe.c`). So the **primary, whole-tree bound is step 2's** (Ada,
  2026-11-01): the installer's privileged helper arms a per-task physical-footprint ledger limit apportioned
  across the box's members (so the limits sum to the bound) and catches the Mach `EXC_RESOURCE` it fires —
  event-driven, near-zero overshoot, ~zero watcher CPU, bounding the whole tree and a forked child. The
  polling rule **stays as the defense-in-depth layer** beneath both (Ada: "it's a good defense in depth
  mechanism"), and is the whole bound on a node with no helper. Step 1's single-member overshoot (0 MiB) and
  held watcher CPU (~0.1%) are met rootless by the `_ext` cap; the multi-member-tree bound at overcommit is
  step 2's.
  **Step 2 (the box user, the helper, the kernel bound) is under way** (`cp/t7-native-s2`): the
  apportionment core, the node↔helper protocol (`hex_core::helper`, hostile-input tested), its node client,
  the helper daemon and the box-user backend are built; the root measurements on the Mac Studio (the
  elastic bound's overshoot and CPU, the `EXC_RESOURCE` path, the box user's listing, `RLIMIT_NPROC` and
  delete) need Ada.
- The **Windows `process` class is built** (`hex_runtime::windows`, native plan step 3): a box is a job
  object, its limits the kernel's and hard (`hex_core::job`, `docs/research/runtime/windows-job-objects.md`,
  `bench/native/windows/RESULT.md`). A member is created suspended inside the job
  (`PROC_THREAD_ATTRIBUTE_JOB_LIST`, so nothing runs outside it) and resumed; breakaway is refused; delete
  (kill-on-close / `TerminateJobObject`) ends the tree. The job refuses whole a commit that would cross
  its commit limit, so the limit is the box's bound itself; its peak (`PeakJobMemoryUsed`) counts the
  refused ask, so the box reports its committed peak as that peak at most the bound, and reaching the
  bound is the peak past the limit (`hex_core::job::reached_bound`, `committed_peak`; `job_events` e20,
  bench/install/records/windows-jupiter-20261105.txt entry 22), from the job's accounting with no completion port (no message queue to
  bound). It was the `JobObjectNotificationLimitInformation` violation until that was
  shown late or lost under load: on jupiter, a member refused at the cap and ended at once left it unset in
  nearly every pass, and one held past the line saw it after up to 26 s or not within 30 s
  (`job_events` e18; bench/install/records/windows-jupiter-20261105.txt entry 21). The peak can't fall, so a box reads as having reached its bound
  once, not once per refused commit, which the job doesn't count. CPU is a weight or a hard cap of the whole machine (`milli × 10 / processors`).
  HEX-RT-1..9 pass on jupiter. **`pids_refused` on Windows** counts only what Hex observes synchronously —
  the node's own `exec` into a full box returns `ERROR_NOT_ENOUGH_QUOTA` (surfaced as `Exhausted`) — since
  Windows gives no guaranteed count of a member's own refused forks: its accounting doesn't move, and the
  completion-port message is lossy, which §3 forbids as a source (researched across hcsshim, Docker,
  Kubernetes, Chromium, BuildXL and a process-notify driver — none counts it;
  `docs/research/runtime/windows-pids-refused.md`). So **HEX-RT-6 on Windows checks the bound's
  deterministic effect** (the blocked program exits non-zero), the exact-count assertion staying on
  kernels that keep it (Linux's `pids.events`) — Ada's decision (2026-11-03). Windows'
  `oci` (HCS) and Linux images through a Shards guest follow. Off a platform whose class isn't built yet,
  `process`/`oci` give a typed refusal naming it (CLAUDE.md §12).
- **The installer** (native plan step 6, the decision above): **Linux's is built** and its fresh-machine lab
  passes on fedora (`bench/install/linux_lab.sh`: refused by name before, the conformance suite on both classes
  over the installed node, nothing left after uninstall). **Windows' is built and lint-clean, and open**: it
  waits on jupiter, an administrator's UAC prompt and a reboot each way (the feature query's standard-user
  access and time are measured there), and is met only once the `oci` class's steps 4 and 5 land and the
  install lab runs again. **macOS's is built** (`scripts/install/macos.sh`, step 2's box users and helper)
  and its lab (`bench/install/macos_lab.sh`) waits on an administrator's one `sudo` run on the Mac Studio. Not
  covered yet: Ubuntu 24.04's restriction on unprivileged user namespaces (an AppArmor profile for `hex`).
- The kiln integration's remaining half: **consumer #1 (host CI legs + harness in `process` boxes)
  is built** — kiln's `os::contain` grows a Hex-`BoxRoot` backing behind `KILN_HEX_BOX`, with the
  legs' parent box capped (`memory.high`) and the runner and agents protected (`memory.low`) so a
  build storm throttles builds, not the runner — pending end-to-end validation on a runner lab and
  then default-on. **Consumer #2 (the `hex` executor for container legs)** is decided (above) and to
  build.
