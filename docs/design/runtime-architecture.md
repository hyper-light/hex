# The runtime's architecture against the 4-5x bar (goal 4)

The competitive baseline (`bench/competitive/RESULT.md`, fedora, of record 2026-10-24) puts Hex's `oci` class
behind crun on create, start, create+start, CPU and memory, close behind on exec, level at concurrency 32 and
ahead on delete. Its `process` class is 2-4x ahead of systemd scopes on latency, throughput and CPU and behind
on memory. The research bar is 4-5x the best competitor on every axis (`docs/research/README.md`). This document
decides how the runtime gets there, axis by axis, and says with sources where an axis can't. It covers the
`process` and `oci` classes on Linux. The microVM classes (CP-T7.2) get their own decisions against their own
baseline, `bench/competitive/vm_bench.py`'s runs of Firecracker and Cloud Hypervisor.

The evidence:
- `docs/research/runtime/oci-runtimes-and-fast-start.md`: where a container start's time goes, and each
  technique with a measured speedup.
- kiln research `01a16e75781e74fdb1ddd0ca6edf2278`: the findings below, each graded (f1-f17).
- `bench/competitive/probes/oci_breakdown.md`: the breakdown probe, run 1, fedora, 2026-10-24T15:33Z, load 0.13
  at the start. It is Hex `hex-cg3-8947ea21` against crun 1.30.1. Its absolute numbers run slower than the
  rounds of record (crun's create p50 is 6.88 ms there against 2.99 in `RESULT.md`), so this document compares
  the probe's numbers only with each other, within its interleaved passes.

## Two layers, each compared class for class

A caller reaches a runtime one of two ways, and each layer has its own competitor.

1. **The OCI command line**, `hex oci create|start|exec|delete`. It is what the runtime-spec defines,
   runtime-tools validates and consumer #2's executor drives. Its competitor is crun's command line, as
   measured in `RESULT.md`. Every operation pays for a process spawn, exec and exit before the runtime does
   any work.
2. **The `Runtime` seam** (`hex-api`), called in process by what links Hex, or over the node agent's local socket.
   Its competitor is crun in process: libcrun, which crun is built to be linked as. Neither Fedora 44 nor Arch
   packages libcrun's headers, so the bench builds it from crun's pinned release (implementation goal 0).

The platform table in `RESULT.md` already compares a third layer, whole sandbox platforms through their APIs,
where Hex is 3-125x ahead.

## The bar, axis by axis

Each 4x target is a quarter of the best competitor's number (for throughput and density, 4x). The last column
names the decisions below that aim at the target, or the argument (A1-A4, after the decisions) for why it
can't be reached, with what the design reaches instead.

**The `oci` class** (ms unless stated; p50 / p99 / p99.9 / max):

| Axis | Hex | Best competitor | 4x target | How, or why not |
|---|---|---|---|---|
| create | 9.13 / 15.60 / 38.40 / 126.28 | crun 2.99 / 6.87 / 8.98 / 19.97 | 0.75 / 1.72 / 2.25 / 4.99 | Through the seam: decisions 1, 2, 4, 5 and 6. Through the command line: A2. |
| start | 6.74 / 11.63 / 24.06 / 140.20 | crun 0.51 / 1.14 / 1.43 / 1.91 | 0.13 / 0.29 / 0.36 / 0.48 | Neither layer: A2 for the command line, A3 for the seam. Decision 1 brings the command line to crun's level, and the seam starts with one write, as libcrun does. |
| exec | 1.85 / 3.35 / 11.07 / 22.98 | crun 1.64 / 3.50 / 4.28, runsc max 14.48 | 0.41 / 0.88 / 1.07 / 3.62 | Through the seam: decision 3. Through the command line: A2. |
| delete | 0.97 / 1.71 / 4.42 / 20.63 | runsc 5.19 / 9.03, crun 22.94 / 32.11 | 1.30 / 2.26 / 5.74 / 8.03 | Met at p50, p99 and p99.9 (5.2-5.3x). The max is decision 8's. |
| create+start of `true` | 16.26 / 29.30 / 68.88 / 934.36 | crun 3.34 / 6.00 / 8.88 / 28.02 | 0.84 / 1.50 / 2.22 / 7.01 | Through the seam: decisions 1, 2, 4, 5, 6 and 8. Through the command line: A2. |
| lifecycles/s at 1 / 8 / 32 | 52 / 325 / 1,030 | crun 87 / 1,015 / 1,038 | 348 / 4,060 / 4,152 | Through the seam: decisions 2, 3 and 5. At 32, the ceiling is open (below). |
| CPU per lifecycle | 7.02 | crun 4.68 | 1.17 | With a network namespace per container: A4. Without one: decisions 1, 2, 3 and 5. |
| held memory per idle container | 0.63 MiB | crun 0.39 | 0.10 MiB | A1. Decision 2 reaches crun's. |
| density (per GiB held) | 1,636 | crun 2,640 | 10,560 | A1. Decision 2 reaches crun's. |

**The `process` class** (ms unless stated):

| Axis | Hex `box run` | Best competitor | 4x target | How, or why not |
|---|---|---|---|---|
| run of `true`, p50 / p99 / p99.9 / max | 2.73 / 6.06 / 13.90 / 28.30 | scope 7.13 / 9.16 / 13.15 / 34.54 | 1.78 / 2.29 / 3.29 / 8.64 | Decision 7. |
| runs/s at 1 / 8 / 32 | 314 / 759 / 783 | scope 145 / 195, service 192 | 580 / 780 / 768 | Met at 32 (4.08x). Decision 7 for 1 and 8. |
| CPU per run | 2.51 | scope 7.81 | 1.95 | Decision 7. |
| held memory per idle box | 0.74 MiB | scope 0.28 | 0.07 MiB | A1. Decision 7 reaches the scope's. |
| density (per GiB held) | 1,388 | scope 3,615 | 14,460 | A1. Decision 7 reaches the scope's. |

**The ceiling at concurrency 32 is open.** Hex and crun top out near the same rate: 1,030 and 1,038
lifecycles/s in `RESULT.md`, and 1,029 and 1,171 in the probe. The network namespace isn't the reason:
without it the probe's ceiling moved only 5-7% (f17). Whether the bench's own driver caps it, a Python process
that spawns every call, is the probe's step 5, still to run. Until the bench can drive 4,152 lifecycles/s
itself (implementation goal 0), this axis can't be judged.

## Decisions

### 1. A container's state is written atomically, never fsynced

**The choice.** The `oci` class writes `state.json` as runc and crun do: a temporary file, then `rename(2)`, with no
`fsync` of the file or its directory. So do the node agent's box records, for every record that describes only
processes and cgroups.

**Sources.**
- rename(2), DESCRIPTION: the target is "atomically replaced, so that there is no point at which another
  process attempting to access newpath will find it missing".
- fsync(2), DESCRIPTION: fsync exists "so that all changed information can be retrieved even if the system
  crashes or is rebooted".
- crun, `src/libcrun/status.c:270-337` at 3fc26242: open, write, close, rename.
- runc, `libcontainer/container_linux.go:872-896` at 976d95ab: CreateTemp, WriteJSON, Close, Rename.

Neither competitor fsyncs (f3, f4).

**Evidence.**
- **The cost.** Hex fsyncs twice per state write (`crates/hex-runtime/src/linux/oci/state.rs:120-137`), at create,
  start and the container's stop (f2). In the probe, that cost about 6.6 ms of start and of create. With the
  state root on the bench container's overlay, start p50 was 8.39 and 8.17 ms; on a tmpfs, 1.68 and 1.70.
  Create was 12.42 and 12.39 against 5.74 and 5.78. crun's start stayed at 1.13-1.19 ms either way
  (`probes/oci_breakdown.md` §1; f14).
- **What the fsyncs buy: nothing** (f5). A process crash leaves the page cache, so a rename-atomic write already
  survives it. A system crash ends every container the state describes, and the default state root is under
  `/run` (`run.rs:78-87`), which is emptied at boot.
- **The node agent's records** are fsynced today as kiln's lesson from its lost session file (`runtime.md`,
  "Durability and recovery"). That file described sessions that outlive a restart of the host. A box doesn't:
  after a host crash, recovery finds no cgroup for any record and scrubs it, torn or whole.

**What would falsify it.** A record that must outlive the host, such as one for a persistent volume; that record
keeps its fsync. The probe's measured cost already rules out the other falsifier, a cost under a tenth of start.

### 2. No shim per container: one supervisor per node

**The choice.** `create` leaves no Hex process beside the container.
- **Through the command line,** the container's parent is the caller's subreaper, as with crun.
- **Through the `Runtime` seam,** the node agent is every container's parent. Built
  (`crates/hex-node/src/supervise.rs`):
  - **Subreaper.** The node agent sets `PR_SET_CHILD_SUBREAPER` when it opens. The shim's exit makes each
    init the agent's child once `create` returns.
  - **Watching each init.** `create` opens the init's pidfd and checks that the process is the agent's child
    with the start time `create` recorded (`/proc/<pid>/stat` fields 4 and 22), so a recycled pid is never
    taken for it.
  - **The budget.** The pidfds are held within a budget (`INIT_BUDGET`, 65,536 by default; set per node).
    Past it, `create` refuses with `Refusal::Exhausted` naming the budget before anything is made, so no
    init is left unwatched.
  - **Recording exits.** One thread waits on every pidfd through an epoll set. When one turns readable it
    reaps the init with `waitid(P_PIDFD)` and records the exit in the box's record. `inspect` reports it in
    `BoxInfo.exit` (an optional field, added append-only) and reaps an ended init itself if the thread
    hasn't yet, so a stopped box is never read without its exit.
  - **Orphans.** Being a subreaper, the agent also inherits a box's other orphans. A sweep every second
    (`SWEEP_PERIOD`, at most 4,096 children each) reaps those that have ended. It takes only zombies in the
    node's box cgroups that no `Proc` handle or watched init owns. Spawns and sweeps exclude each other, so a
    sweep never takes a child before its owner has claimed it.
  - **After a restart.** An agent restarted while its boxes run is no longer their inits' parent. It judges
    them by pid and start time as `state` does, and reports an init that ended out of its sight as
    `Exit::Unknown`.
- **`state`** derives `stopped` from the init's pid and start time, as crun's status file does with
  `process-start-time` (`status.c:258-268`), so nothing has to write it.
- **A zombie init.** Hex counts an init that has exited but not been reaped (state `Z`) as stopped. crun
  rejects only a changed start time or state `X` (`status.c:804`, on main: `if (status->process_start_time !=
  st.starttime || st.state == 'X')`), so it counts that zombie as running until it is reaped. Hex's reading
  is the runtime-spec's: a container is stopped once "the container process has exited" (runtime.md,
  Lifecycle step 10). Through the seam the difference lasts only until the agent reaps the init.
  (Kiln research `01a178e67ccd7764b6746e5880187422`, f19-f20.)

**Sources.**
- PR_SET_CHILD_SUBREAPER(2const): "A subreaper fulfills the role of init(1) for its descendant processes."
- pidfd_open(2): a child's pidfd "can be waited on using waitid(2)" (f7).

**Evidence.**
- **The second process and exec.** Hex's `create` execs the hex binary again as `__oci-init`
  (`run.rs:151-161`), and that shim waits on the container for its whole life (`init.rs:126-135`; f6).
- **What it holds.** In the probe, each idle container kept that shim at 3,140-3,328 KiB RSS beside its `sleep`.
  crun kept nothing; its containers' `sleep`s were reparented and ran alone (`probes/oci_breakdown.md` §3; f16).
  That is the gap between Hex's 0.63 MiB held per idle container and crun's 0.39.
- **What's left without the shim: a device program per container.** On the no-shim binary, Hex still holds
  13.2 KiB more per idle container than crun. That is the mean of three interleaved density rounds
  (12.4/11.9/15.4 KiB), from run `20261027T031736Z` (adacachy, binary 3e5444d8, 1,000 containers each;
  `bench/competitive/RESULT.md`).
  - **Where it is.** Nearly all of it is the cgroup's `kernel` memory: +11.8/+12.2/+14.1 KiB. shmem rose by
    nothing for either runtime, and Hex's state root takes 4 KiB per container against crun's 8, so the gap
    isn't runtime state on a tmpfs.
  - **The steady entries.** Within `kernel`:
    - **vmalloc** is +4,096 bytes in every round, and crun's is 0.
    - **percpu** is about +1.2 KiB.
    - **slab** is +4.1 to +6.6 KiB, and moves the most between rounds.
  - **What makes them.** A loaded BPF program takes those charges. `bpf_prog_alloc_no_stats` rounds the
    program up to whole pages and `__vmalloc`s it, then allocates its `aux` and its per-CPU `active` counters.
    `bpf_prog_alloc` adds the per-CPU `stats`. All of these use `bpf_memcg_flags`, so they're charged to the
    loader's cgroup (`kernel/bpf/core.c:102-118,149-159`, master).
  - **Why only Hex loads one.** The bench's bundle has no `linux.resources` (`oci_inside.sh:24`). Hex gives such
    a spec its deny-by-default device policy and loads that program for every container
    (`run.rs:180,637-656`; `resources.rs:815`). crun loads a device program only when the spec lists devices
    (`src/libcrun/cgroup-resources.c:1383` at 1.30.1's 079ff6a7: `if (need_bpf_dev && resources->devices_len)`).
    So `run.rs:180`'s comment, "as runc's and crun's do", is half wrong: runc applies the default policy to a
    resource-less spec, and crun doesn't.
  - **Not yet observed directly.** No one has seen the program attached to a Hex container's cgroup and absent
    from crun's yet; that needs bpftool inside the privileged bench container.
  - **What closes it** is plan step 2's decision. One default-policy program shared by every container keeps
    the policy and pays its pages once per node.
- **The third fsync.** The shim's stop write is the lifecycle's third fsync'd write.
- **Delete.** The probe found Hex's delete slower with its state on a tmpfs (4.13 against 2.05 ms). The shim
  writes the stopped state while the delete runs, and the probe doesn't explain the difference; delete is
  measured again once the shim is gone.

**What would falsify it.** An OCI operation that needs a process per container. runtime-tools' 28 passing tests,
crun's set on this host, are the check.

### 3. The fast path is the `Runtime` seam; the OCI command line stays for compatibility

**The choice.** The node agent implements the `oci` class behind `hex-api::Runtime`. Today it refuses it
(`crates/hex-node/src/node.rs:150-157`). It serves the class in process for callers that link Hex, and over a
local socket for the rest. No operation on this path spawns a process except the container's own. `hex oci`
stays the runtime-spec's command line, with decisions 1 and 2.

**Sources.** In a production serverless system, "the control-path latency takes 18ms–25ms, constituting
approximately 30%–40% of the overall cold start period", from binary calls between the high- and low-level
runtimes, and the fix was a plugin interface (Chai et al., OSDI 2025, §2.2, §4.1; f10).

**Evidence.** In the probe, spawning and waiting for `/usr/bin/true` cost 0.666 ms at p50, as the bench spawns
every call. `hex --version` cost 1.038 ms against `crun --version`'s 0.786 (`probes/oci_breakdown.md` §2; f15).
Every operation through any command line pays the first figure, and Hex's pays the second too. Through the seam,
neither.

**What would falsify it.** A seam call that costs as much as a spawn, measured by the in-process and socket drivers
(implementation goal 0).

### 4. Network-namespace work off the critical path (rewritten after the probe)

**What changed.** The plan behind this document expected pooled network namespaces and cgroups to lift the
throughput ceiling, from SOCK's finding: about 200 containers/s with unmodified network namespaces against 900/s
without them, "due to a single global lock" (Oakes et al., ATC 2018, §2.2; f8). The probe met that decision's
falsifier. On fedora's Linux 6.19, dropping the bundle's network namespace raised throughput at concurrency 32
only 5% for Hex (1,029 to 1,079/s) and 7% for crun (1,171 to 1,251/s) (`probes/oci_breakdown.md` §4; f17,
recorded against f8). SOCK's kernel predates the fixes to namespace creation (`oci-runtimes-and-fast-start.md`
§4.3). So this decision no longer rests on throughput. The cgroup pool is dropped: no measurement on this host
puts cgroup creation on the critical path, and CLAUDE.md §13 rules out building ahead of evidence.

**What the probe did show.** At concurrency 1, the network namespace costs both runtimes about 5.5 ms of CPU per
lifecycle: Hex 12.84 against 7.33 ms, crun 10.19 against 4.62. It costs Hex about 5.5 ms of latency, 44 against
58 lifecycles/s, but crun only about 1 ms, 68 against 73. crun keeps most of that work off its critical path,
and Hex doesn't.

**The choice.** First find what puts Hex's namespace work on its critical path: the bench measures create and delete
separately, with and without the namespace. Then move it off. The node agent keeps a bounded pool of fresh network
namespaces made ahead of demand, and `create` joins one with `setns`. A namespace is never handed to a second
container. The bound is the node's container capacity. A dry pool falls back to making the namespace on the
path, and the bench reports how often. Teardown happens after `delete` returns.

**Sources.**
- SOCK, as above, for what a namespace costs and why pools are made.
- AFaaS pre-allocates "1,000 veths and 600 cgroups" in production (Chai et al., §4.2; f10).
- The kernel's cgroup-v2.rst:194-200 and 620-631: create a cgroup and seed it with `CLONE_INTO_CGROUP`, once at
  start-up (f11). The class already does this (`runtime.md`, "Born in its cgroup").

**What would falsify it.** Hex's create and delete costing no more with the namespace than crun's do, which
leaves nothing to move. Or a pool whose refill can't keep up with the bench's sustained phase.

### 5. A seed per node clones containers

**The choice.** The node agent keeps a prepared, single-threaded helper that has already exec'd and set up
everything common to its containers. `create` asks it for a `clone3` into the container's cgroup and namespaces,
with `CLONE_PIDFD`. This replaces today's second exec of the hex binary per create.

**Sources.** SOCK's zygotes, Catalyzer's `sfork` and AFaaS's seeds (`oci-runtimes-and-fast-start.md` §5.1-5.4, §6
rank 2). AFaaS names the risks: a seed is serial, so a node keeps several, and unique state such as random
seeds must be re-initialized after the fork.

**Evidence.** Each create execs the hex binary a second time (f6). An exec of that binary costs about 1 ms on the
bench host (`hex --version`, 1.038 ms), about a sixth of create's 5.74 ms with the state on tmpfs.

**What would falsify it.** A seed's clone costing as much as the exec it replaces.

### 6. Mount namespaces without copying, where the kernel has it

**The choice.** On Linux 7.0 and later, `create` builds the container's mount namespace with
`open_tree(OPEN_TREE_NAMESPACE)`, and on 7.1 with `CLONE_EMPTY_MNTNS`, both detected at runtime. The classic
`unshare(CLONE_NEWNS)`, bind and `pivot_root` remain for older kernels, for create-time hooks and for user
namespaces, as crun does.

**Sources.** crun's maintainer measured `unshare(NEWNS)` plus `pivot_root` at 0.343 ms on a clean host and 0.917 ms
with many host mounts, against 0.044 ms with `OPEN_TREE_NAMESPACE`. Kernel 7.0 reports about 40% more
container-creation throughput (`oci-runtimes-and-fast-start.md` §4.2, S12, S13).

**Evidence.** Not measured on Hex's hosts yet. fedora runs 6.19, which lacks both; adacachy runs 7.2.

**What would falsify it.** The mount setup measured at under a tenth of create on a 7.x host.

### 7. The process class: no wrapper per box

**The choice.** Through the `Runtime` seam, the node agent's helper clones a box's program straight into its
cgroup with `clone3(CLONE_INTO_CGROUP | CLONE_PIDFD)`, the vfork-class spawn. The agent supervises it by pidfd and
removes the box when it's done, so no process stays beside an idle box. `hex box run` keeps its wrapper:
something has to remove a box's cgroup when its program ends, and on the command line nothing else is there to.
So the process class's density is compared through the seam.

**Sources.** cgroup-v2.rst (above). `oci-runtimes-and-fast-start.md` §4.7: a `pre_exec` closure forces Rust's
`Command` onto `fork`, and the `cgroup.procs` write in the child takes the kernel's global locks.

**Evidence.** Hex holds 0.74 MiB per idle box against a scope's 0.28, and `RESULT.md` puts the gap down to the
wrapper each box keeps (f13).

**What would falsify it.** The wrapper's share of held memory being small.

### 8. The tail: what bounds p99.9 and max

**The choice.** Nothing on the create, start or exec path waits on storage, or on kernel work a background task
could do instead. Decisions 1, 4 and 5 are how. Every phase is benchmarked at 10,000 samples, so p99.9 is real.

**Evidence.**
- **The tails today.** Hex's create+start max is 934 ms against crun's 28 ms, with 706 and 400 ms next in the
  same round (`RESULT.md`).
- **What the fsyncs do to them.** In the probe, removing them took start's p99.9 from 11.38-11.83 ms to
  2.10-2.13 ms, and its max from 11.56-12.83 to 2.14-2.17.
- **What's left.** Create's max was still 27.75 ms in one tmpfs pass, so other tail sources remain.

**What would falsify it.** Tails that stay after decisions 1, 4 and 5 land.

## Axes that can't reach 4x, and why

- **A1. Held memory and density per container or box.** A process in a cgroup of its own carries a kernel floor.
  - **The measurement.** On adacachy, a systemd scope box's cgroup held 298.0 KiB: 143.0 KiB of kernel memory,
    of which 65.9 KiB per-CPU data and 57.9 KiB page tables, beside 110.9 KiB of its process's anon
    (`probes/scope_charge.py`, 100 boxes; cgroup-v2.rst's `percpu`; f12).
  - **Why it bounds the axis.** crun's 0.39 MiB per idle container is about that floor plus the workload.
    4x below it is 0.1 MiB, less than the floor, so no runtime with one process per container in its own
    cgroup gets there.
  - **What the design reaches.** Decisions 2 and 7 bring Hex to the floor: level with crun and with systemd.
  - **What could beat it.** Only many agents sharing a process and a cgroup. Agent Substrate's actors on fewer
    pre-started workers are that, a different isolation class (`agent-sandbox-platforms.md` §3.12).
- **A2. Create, start and exec through any command line.** Spawning and waiting for `/usr/bin/true` costs 0.666 ms
  at p50 on the bench host (f15).
  - **Below the floor.** Start's 4x target (0.13 ms) and exec's (0.41 ms) are under that floor.
  - **At the floor.** Create's (0.75 ms) leaves under 0.1 ms for all of create's work, and create+start's
    (0.84 ms) is less than the two spawns it needs (1.33 ms).
  - **What the design reaches there.** About crun's numbers. Decision 1 brings start to 1.68 ms against crun's
    1.16 in the same run. Of the 0.52 ms left, about half is Hex's binary starting 0.25 ms slower than crun's.
- **A3. Start through the seam.** Start's work is one operation on the container's exec FIFO, for Hex
  (`run.rs:228-250`) as for crun, whose `start` writes one byte (`oci-runtimes-and-fast-start.md` §2.3). In
  process, nothing is left to be 4x faster at. Parity with libcrun is the bound.
- **A4. CPU per lifecycle with a network namespace per container.** In the probe, the namespace cost 5.57 ms of
  CPU per lifecycle for crun and 5.51 ms for Hex at concurrency 1 (`probes/oci_breakdown.md` §4). The 4x
  target, 1.17 ms, is less than the namespace alone, so it's reachable only for containers that don't ask for a
  network namespace of their own.

The plan also expected a fifth argument: sustained churn capped by the kernel's rate of making network
namespaces. The probe refuted its premise (decision 4), so it's dropped. The ceiling at concurrency 32 is open
instead, as above.

## How it is measured

`bench/competitive` gains, in implementation goal 0:
- an in-process driver for the `Runtime` seam, and the same over the node agent's socket;
- libcrun in process, built from crun 1.30.1's release, as the seam layer's competitor;
- a driver whose own ceiling, measured with a stand-in runtime, is above every throughput target;
- a sustained-churn phase ten times the size of any pool, beside the burst phases;
- create and delete timed separately with and without a network namespace (decision 4).

Each implementation goal is done only when the bench of record shows its number.

## Implementation goals

They go on the goal stack after this document. Each is done only when the bench, on fedora and of record,
shows its target.

0. **The bench:**
   - the in-process and socket seam drivers;
   - libcrun in process;
   - a driver able to exceed 4,152 lifecycles/s, shown by a stand-in runtime;
   - the sustained phase;
   - create and delete with and without a network namespace.
1. **Volatile state** (decision 1): no fsync of container state. Done when the probe's step 1 shows Hex's start
   and create p50 with the state root on the overlay within 10% of its tmpfs passes in the same run.
2. **No shim** (decision 2): done when the held memory per idle container is at most crun's and runtime-tools
   still passes crun's 28.
3. **The `oci` class behind the seam** (decision 3): done when exec p50 through the seam is at most a quarter of
   libcrun's in-process exec on the same run, and start through the seam is within 10% of libcrun's (A3).
4. **Network namespaces off the path** (decision 4): done when Hex's create costs no more with the namespace than
   without it, at p50, through the seam.
5. **The seed** (decision 5): done when create p50 through the seam is at most a quarter of libcrun's in-process
   create p50, and create+start likewise.
6. **Mount namespaces without copying** (decision 6): done when create on a 7.x host is measured with and without
   `OPEN_TREE_NAMESPACE`, and the faster path is the default.
7. **The process class through the seam** (decision 7): done when run-of-`true` p50 is at most a quarter of a systemd
   scope's, and held memory per idle box is at most a scope's.
