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
| held memory per idle container | 0.63 MiB | crun 0.39 | 0.10 MiB | A1. Decision 2 reaches crun's: parity, 0.998-1.002x on adacachy once each container's loopback is up as crun's is (run `20261027T165449Z`). The 0.98-0.99x of record on fedora (`20261027T061920Z`) left Hex's loopback down. |
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
`fsync` of the file or its directory. So do the node agent's box records, for every record whose directories all lie
on tmpfs or that names none: the node keeps its containers' state where runc and crun keep theirs, under its runtime
directory (`/run/hex-node` as root, `$XDG_RUNTIME_DIR/hex-node` rootless), each container named `<node key>-<box
id>`. A record naming a directory on disk, such as a rendered bundle's writable layer, is still fsynced, file and
directory, and no lock another box's create takes is held across a record write (goal 5 plan step 2). So create
holds the supervisor's spawning lock twice, across the shim's spawn and across the init's watch, and not across the
Ready record between them: an init that dies in that gap, and is reaped there by a sweep, refuses its create by
type and the box is scrubbed, where before the box was made showing the init's exit.

A box's record is written once, when the box is whole, and kept where it belongs (goal 5 extended, step 5): one
that must outlive the host in the node's state directory, fsynced; every other record beside the containers'
state, in a directory of the node's own under its runtime directory (`<node key>.records`), never synced, spread
over 64 subdirectories by the box's id so the boxes made and deleted at once rarely wait on one directory's lock
(goal 5 extended, step 6; recovery moves a record an older node left in the directory itself to its shard). Until the
record is written, each part a create makes lies at a path the box's id fixes: its cgroup (`<box root>/.oci/<id>`,
or `<box root>/<id>` for a process box), its rendered bundle (`<state>/bundles/<id>`) and its container's state
directory (`<node key>-<id>` in its shard of the runtime directory, below), the init inside the cgroup. So recovery
finds a box caught mid-create by listing those places, and removes what it left, naming the parts it found; it
still removes a box whose record says it was being made, as nodes before the step marked one before making it, and
moves a record such a node left in the state directory to its place. The oci runtime hands the node the state it
wrote, so create reads none back, and the box's info is what create made. The record was what claimed a box's id
while it was made; now its cgroup does, made first (mkdir is atomic): a create of an id another create has taken
since its check fails at that mkdir, having made nothing, and is refused as `Exists` without removing anything,
where before such a create's failure scrubbed the other's box.

A container's state directory is kept in one of 64 shards of its state root, `<root>/<shard>/<id>`, the shard two hex
digits of FNV-1a over the container's id (goal 5 extended, step 6, lever 3). A create's mkdir and a delete's rmdir of
it hold its parent's `i_rwsem` exclusively (Linux v6.19 `fs/namei.c:2860-2873`, `:4888`, `:5293`), so under one parent
every create and delete waited on every other's: 0.043 ms a lifecycle at concurrency 32, none at 1 (`create_serial.md`
run 25). The records' shards took the same wait away in run 20. The layout is the oci runtime's, so `hex oci --root`
and the node find each other's containers. A runtime before the shards kept a container directly under the root: one
there, and not in its shard, is read and removed where it is, and a create over it is refused as over any other;
recovery lists both places, adopting or removing what each holds (T-T7.1.2, `oci_box`'s upgrade test). The shard
directories are made as their first container needs them and never removed. An old-layout container whose id is
itself two hex digits shares its name with a shard; the node's containers are named `<node key>-<id>`, so none of its
own can. Records and the wire are unchanged. **What would falsify it:** the `i_rwsem` wait on the state root not
falling at concurrency 32, or a lifecycle's p50 or p99 at 1 or 32 rising against the one parent
(`docs/research/runtime/goal5-cgroup-lock.md`). **Measured, and kept:** the root's mkdirs and rmdirs, one each a
lifecycle under its `i_rwsem` at 32, are off the lock table, no shard waited on (`create_serial.md` run 27); in
host.sh's reading, the one goal 5's bar is measured by, <!-- record: bench/competitive/probes/reuse_pass.md -->a
lifecycle's p99 at 1 and 32 is 0.835 to 0.884 of the one parent's at both descriptor limits and its p50 at 32 0.88 to
0.90<!-- /record --> (`reuse_pass.md` run 20); the other readings are mixed within the host's noise, and the
lever's own cost is a second stat for a new id, about 9 us a lifecycle.

The Linux record store opens its two directories once, retaining two close-on-exec descriptors
shared by its clones. Each write opens only `.<id>.tmp` relative to its directory and renames that
file to `<id>.json` relative to the same descriptor: it does not walk the runtime-directory prefix
three times. A durable write also fsyncs the retained directory, without opening it again. The
operations remain open, write, close and atomic rename (plus the two fsyncs where required), with
the same JSON and recovery rules; other platforms keep the path-based write. The descriptor budget
is two per store, not one per box. See open(2), "Rationale for openat() and other directory file
descriptor APIs" ([Linux man-pages](https://man7.org/linux/man-pages/man2/open.2.html)), and
[renameat(2)](https://man7.org/linux/man-pages/man2/rename.2.html), whose directory-relative names
keep rename's atomic replacement. Goal 5 extended step 5 measures this through the same create
breakdown's record parts, including open, write, close and rename; it changes neither the measure
nor the 0.015 ms bar.

**Sources.**
- rename(2), DESCRIPTION: the target is "atomically replaced, so that there is no point at which another
  process attempting to access newpath will find it missing".
- fsync(2), DESCRIPTION: fsync exists "so that all changed information can be retrieved even if the system
  crashes or is rebooted".
- crun, `src/libcrun/status.c:270-337` at 3fc26242: open, write, close, rename.
- runc, `libcontainer/container_linux.go:872-896` at 976d95ab: CreateTemp, WriteJSON, Close, Rename.
- runc, `main.go`, `--root`: "root directory for storage of container state (this should be located in tmpfs)".
- crun(1), STATE: state under `/run/crun` as root, `$XDG_RUNTIME_DIR/crun` rootless.

Neither competitor fsyncs (f3, f4).

For the record written once (kiln research `01a190c51f137042a9697949c8fe2c7a`, f1-f5, f92 and f93):
- `crates/hex-node/src/node.rs` at 9db80fe, `create_oci`: the record written as Creating before anything was made,
  then as Ready once the init waited at its exec FIFO, and the container's state read back between (f1).
- `crates/hex-core/src/recovery.rs` at 9db80fe: a cgroup with no record removed, a record mid-create removed, a box's
  directories removed as its record named them (f2). The container's state directory and the rendered bundle lie
  at paths fixed by the box's id (f3), so recovery can find them by listing (f4).
- `bench/competitive/probes/levers.md` at 876136e: a record-sized write timed by filesystem (f92, f93).

**Evidence.**
- **The cost.** Hex fsyncs twice per state write (`crates/hex-runtime/src/linux/oci/state.rs:120-137`), at create,
  start and the container's stop (f2). In the probe, that cost about 6.6 ms of start and of create. With the
  state root on the bench container's overlay, start p50 was 8.39 and 8.17 ms; on a tmpfs, 1.68 and 1.70.
  Create was 12.42 and 12.39 against 5.74 and 5.78. crun's start stayed at 1.13-1.19 ms either way
  (`probes/oci_breakdown.md` §1; f14).
- **What the fsyncs buy: nothing** (f5). A process crash leaves the page cache, so a rename-atomic write already
  survives it. A system crash ends every container the state describes, and the default state root is under
  `/run` (`run.rs:78-87`), which is emptied at boot.
- **The node agent's records.** An oci box's record always named a directory (its container's state), so a seam
  create wrote two durable records, four fsyncs: 12.187 ms of create's 18.297 ms p50 in the bench container
  (create-breakdown-20261030T000822Z). The rule now judges each directory's filesystem: statfs of the directory, or
  of its nearest existing parent; unreadable counts as disk. A record whose directories all lie on tmpfs isn't
  synced: 0 fsync and 0 fdatasync over 5 seam lifecycles per driver with /run on tmpfs, against 30 with it on
  overlayfs (`probes/record_syncs.md`, record-syncs-20261030T002934Z). With the store's lock off the fsyncs too, a
  seam create's p50 is 0.77x libcrun's and the seam sustains 2,108 lifecycles a second at concurrency 32 against
  libcrun's 1,765 (`RESULT.md`, 20261030T004533Z).
- **The bookkeeping left on create's path** (run 4, create-breakdown-20261031T061841Z, before goal 5 extended step
  5): <!-- record: bench/competitive/probes/create_breakdown.md -->the record written as Creating (its temp file
  0.047 ms p50, its write 0.032), the container's state written (0.051) and read back (0.027), and the record
  written as Ready (0.066 and 0.069), of parts that sum to 2.134 ms<!-- /record -->: 0.292 ms in all, with the
  node's state on the bench container's overlay.
- **A record write by filesystem** (`levers.c`'s loop, a <!-- record: bench/competitive/probes/levers.md -->400-byte
  temp file written, closed and renamed: 6.7 us p50 on tmpfs, 49.6 on the overlay and 52.4 on btrfs<!-- /record -->),
  so a record kept beside the containers' state on tmpfs takes about 43 us of filesystem work off each write.
- **Nothing leaked at any point of a create** (T-T7.1.2): the agent killed with the box's cgroup made, with its
  bundle rendered (in a debug build, the agent armed by `hex node --crash-at` killing itself there, since nothing
  it opens can hold it at those points; a box's own spec can't arm it), with its cgroup and container state made,
  with its init cloned, inside its record write, and after it, its records kept on tmpfs and on disk, then
  restarted: the box killed mid-create is removed by what it left and the one made is taken back, nothing left of
  either (`crates/hex-cli/tests/node_socket.rs`);
  recovery's plan crashed after every step of generated histories against a model (`recovery.rs`).

**What would falsify it.** A record that must outlive the host, such as one for a persistent volume; that record
keeps its fsync. The probe's measured cost already rules out the other falsifier, a cost under a tenth of start. A
host whose runtime directory isn't tmpfs gets durable records: correct, but at the fsyncs' cost. That includes a
container's /run unless it mounts a tmpfs there, as the bench and lab containers now do. For the record written
once: a part a create makes at a path its id doesn't fix, which a crash before the record would leak (T-T7.1.2's
crash test kills the agent at each point of a create and finds nothing left); and a record write beside the
containers' state costing more than 0.015 ms of filesystem work in the create breakdown, the step's bar.

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
- **Nothing per container but the container.** With the shim gone, two things were still kept per idle
  container that crun doesn't keep (Evidence). Built in plan step 2:
  - **One device program per policy** (`oci/devshare.rs`). Containers whose device policies are the same
    share one loaded program. A spec without `linux.resources` keeps the default device policy, as runc's
    does (f30). As built in plan step 2, `create` loaded its own copy, which told it the program's tag, and
    attached the program a hints file named for that tag once the kernel showed it was the same program (f29).
    Goal 5's extension (its step 1) took the load off the create, with the node agent as the holder that
    outlives its containers (`oci/programs.rs`; research `01a190c51f137042a9697949c8fe2c7a` f40-f61, notes
    `docs/research/runtime/goal5-cgroup-lock.md`):
    - **The node holds each policy's program.** Once loaded, it stays open, at most 64 (`MAX_PROGRAMS`);
      past that the one used longest ago is closed, and it lives on in the cgroups it's attached to (f53). A
      program is found by Hex's own digest of its instructions (64-bit FNV-1a) and proven by the instructions
      themselves, so nothing need be loaded to find it, as a kernel tag would need. A create attaches the
      program held for its policy and loads only a policy the node holds none for (f60).
    - **The default sits once on the oci boxes' root.** A node's `oci` boxes live under a root of their own,
      `<box root>/.oci`, apart from its `process` boxes (f44), and a root node's start attaches the default
      policy's program there with `BPF_F_ALLOW_OVERRIDE`. A box with the default policy attaches nothing. A box
      with another attaches its own (MULTI), which then decides for it in the root's place (f40-f42). A
      policy that allows every device carries the allow-all program as its own, so it never takes the default
      (f43); so does a container in a user namespace of its own, which is held to its device list by
      construction, as before. The start asks once which program the root carries (`BPF_PROG_QUERY` takes
      `cgroup_mutex`, f61): one attached with OVERRIDE whose translation is the default's stays, so an agent
      restarted over its boxes leaves it; anything else is replaced in one attach. A rootless node loads
      nothing, as before. An `oci` box made before its node kept them apart is still found under the box root.
    - **The command line looks before it loads.** A process per container, it keeps the hints file in its
      state root (`.device-programs.json`: at most 64 programs, read up to 64 KiB), keyed by the same digest.
      A create opens the program a hint names and attaches it only when the kernel shows a cgroup device
      program whose translated instructions are the policy's byte for byte (f51, f52, f59); else it loads a
      copy and remembers it. On x86_64 and aarch64 the translation of Hex's program is the program (f55); a
      kernel that rewrote it would match nothing and load each time. A stale or planted hint costs a load,
      never the policy.
  - **Nothing of the runtime's in the program.** The create handshake's two pipes are made close-on-exec,
    and the shim sets close-on-exec again on the two ends it's handed (`run.rs:219-227`,
    `init.rs:691-702`). The program holds only its stdio, as crun's does (f26, f27).

**Sources.**
- PR_SET_CHILD_SUBREAPER(2const): "A subreaper fulfills the role of init(1) for its descendant processes."
- pidfd_open(2): a child's pidfd "can be waited on using waitid(2)" (f7).
- execve(2): "By default, file descriptors remain open across an execve()" (f23).
- Linux v7.2 `fs/pipe.c:184,193,898`: a pipe's pages are charged to the writer's memory cgroup, and a page
  read empty is kept for the next write (f24).
- Linux v7.2 `include/uapi/linux/bpf.h:1209-1210,7309-7314`: with `BPF_F_ALLOW_MULTI` a cgroup's programs run
  "in addition to" its ancestors', and a device program's context is the access type, major and minor (f28).
- Linux v7.2 `kernel/bpf/syscall.c:4959,5218-5247`: opening a program by id needs CAP_SYS_ADMIN; the info call
  dumps the translated instructions to a capable caller, or withholds them. `kernel/bpf/core.c:334`: the tag
  is 8 bytes of a SHA-256 of the instructions (f29).
- crun, `src/libcrun/container.c:1473` and `utils.c:2053` on main: every descriptor from 3 + `preserve_fds` is
  marked close-on-exec before the container's exec (f26).
- runc, `libcontainer/specconv/spec_linux.go:984` on main: "Append the default allowed devices to the end of
  the list", whatever the spec's resources (f30).
- The device program held by the node (goal 5 extended, step 1; research `01a190c51f137042a9697949c8fe2c7a`):
  Linux v6.19 `include/uapi/linux/bpf.h:1181,1187-1188`: `BPF_F_ALLOW_OVERRIDE` "the program in this cgroup yields
  to sub-cgroup program", and an OVERRIDE attach on top of an OVERRIDE one "will release old program and attach
  the new one" (f40, f42); `kernel/bpf/cgroup.c:444,459` (`compute_effective_progs`): a cgroup's effective
  programs are its own and only its MULTI ancestors' past them (f40); `:826`: a replacing attach must give the
  attachment's flags; `:1262`: `BPF_PROG_QUERY` writes `revision` back whatever the size passed, so Hex passes
  the whole query struct; `:1319`: the query takes `cgroup_mutex` (f61); `kernel/bpf/syscall.c:4620`: it needs
  CAP_NET_ADMIN or CAP_SYS_ADMIN. FNV-1a's offset basis and prime: IETF draft-eastlake-fnv §2.1.

**Evidence.**
- **The second process and exec.** Hex's `create` execs the hex binary again as `__oci-init`
  (`run.rs:151-161`), and that shim waits on the container for its whole life (`init.rs:126-135`; f6).
- **What it holds.** In the probe, each idle container kept that shim at 3,140-3,328 KiB RSS beside its `sleep`.
  crun kept nothing; its containers' `sleep`s were reparented and ran alone (`probes/oci_breakdown.md` §3; f16).
  That is the gap between Hex's 0.63 MiB held per idle container and crun's 0.39.
- **What was left without the shim: a device program and two pipes per container.** On the no-shim binary,
  Hex still held 11.4, 12.6 and 12.9 KiB more per idle container than crun in three interleaved density
  rounds. That's run `20261027T033009Z` (adacachy, binary 3e5444d8, 1,000 containers each;
  `bench/competitive/RESULT.md`). memory.stat's named entries account for all of each round's difference.
  Nearly all of it is the cgroup's `kernel` memory. shmem rose by nothing for either runtime, and Hex's state
  root takes 4 KiB per container against crun's 8, so the gap isn't runtime state on a tmpfs.
  - **A device program per container.** vmalloc rose 4,096 bytes per container in every round and percpu
    1.2 KiB, where crun's rose by nothing. A loaded BPF program takes those charges.
    `bpf_prog_alloc_no_stats` rounds the program up to whole pages and `__vmalloc`s it, then allocates its
    `aux` and its per-CPU `active` counters. `bpf_prog_alloc` adds the per-CPU `stats`. All of these use
    `bpf_memcg_flags`, so they're charged to the loader's cgroup (`kernel/bpf/core.c:102-118,149-159`,
    master). The bench's bundle has no `linux.resources` (`oci_inside.sh:24`). Hex gave such a spec its
    deny-by-default policy and loaded that program for every container. crun loads a device program only when
    the spec lists devices (`src/libcrun/cgroup-resources.c:1383` at 1.30.1's 079ff6a7: `if (need_bpf_dev &&
    resources->devices_len)`). runc applies its default rules to every container (f30).
  - **Two pipes per container.** Each idle container's program held four descriptors beyond its stdio: both
    ends of the create handshake's sync and map pipes. crun's held 0, 1 and 2 alone (f22). The pipes were
    made without close-on-exec, and the shim never set it again on the ends it was handed, so all four went
    on through the container's exec of its program (f23). An open pipe keeps its inode, files and buffers,
    and the sync pipe keeps the page its messages were written through (f24). In a lab of 500 idle
    containers a round, the cgroup the runtime ran in held 4.1 KiB per Hex container outside slab, percpu,
    vmalloc, page tables and stacks, and none per crun container: that page (f25).
  - **Each change measured** (adacachy, three interleaved rounds of 1,000 idle containers each; Hex less crun
    per container; f31):

    | binary | run | Hex less crun, KiB | held, Hex / crun |
    |---|---|---:|---:|
    | no shim | `20261027T033009Z` | +11.4 / +12.6 / +12.9 | |
    | the program shared | `20261027T034522Z` | +5.19 / +4.21 / +5.34 | |
    | the pipes closed, the program not shared | `20261027T044129Z` | +3.76 / +2.90 / +2.25 | 1.0090 / 1.0070 / 1.0054 |
    | both | `20261027T043711Z` | -3.31 / -2.75 / -3.90 | 0.9920 / 0.9934 / 0.9906 |

    Sharing took vmalloc's and percpu's rise to 0. Without it, they were back at +4.00 and +1.20 KiB. Either
    change alone leaves Hex above crun, and both bring it below.
  - **Every guarantee kept.** In the cgroups lab (`bench/oci-validation/cgroups.sh`), a second container takes
    the first's program and, the first deleted, still reads `/dev/zero` and is refused a disk. The started
    program's descriptors are 0, 1 and 2. All 32 checks pass. On 203ae0a's binary, the descriptor check
    failed with `0 1 2 3 4 5 6`. On the changed binary, `probes/oci_breakdown.sh` step 3 finds only the
    workload in Hex's container cgroups and no Hex process anywhere. runtime-tools bd3b0bd gives Hex and crun
    the same 9 passes and 49 failures on adacachy, none differing (f32).
- **The third fsync.** The shim's stop write is the lifecycle's third fsync'd write.
- **Delete.** The probe found Hex's delete slower with its state on a tmpfs (4.13 against 2.05 ms). The shim
  writes the stopped state while the delete runs, and the probe doesn't explain the difference; delete is
  measured again once the shim is gone.
- **The program held by the node** (goal 5 extended, step 1, on fedora; decision 5 has its create figures):
  - **No load or attach per create.** <!-- record: bench/competitive/probes/seed_path.md -->strace over 55 seam
    creates and the node's start (run 4): `BPF_PROG_LOAD` 0 and `BPF_PROG_ATTACH` 0; the start's `BPF_PROG_QUERY` 1,
    `BPF_PROG_GET_FD_BY_ID` 1 and `BPF_OBJ_GET_INFO_BY_FD` 2, keeping the root's program.<!-- /record -->
  - **Each box decided by its policy** (`crates/hex-cli/tests/oci_devices.rs`, as root in the seam root lab): a
    default box opens /dev/null and is refused /dev/kmsg; an allow-all box opens /dev/kmsg; a box whose policy also
    allows writing /dev/kmsg opens it, and one whose policy denies c 1:3 opens /dev/zero and is refused /dev/null,
    each box's own program deciding in the root's place; a process box opens /dev/kmsg. A
    restart keeps the root's program, a policy is loaded once, the bound closes the program used longest ago while
    its boxes keep their policy, and a planted hint is refused by its translation on the command line.

**What would falsify it.**
- **The shim's removal:** an OCI operation that needs a process per container. runtime-tools' 28 passing tests,
  crun's set on this host, are the check.
- **Sharing:** a device program that keeps state per container, such as a map, which Hex's doesn't (it reads
  only its context, f28). Or a kernel that charges each attachment as much as a program, so that sharing saves
  nothing: the density phase's vmalloc and percpu rise per container would show it, and they're 0.
- **The program held, the default at the oci boxes' root** (goal 5 extended, step 1): a box with no program of
  its own reaching a device the default denies; a box with a program of its own refused a device its policy
  allows, so something besides its own and its MULTI ancestors' decided for it; a `process` box refused a
  device (each is a case of `crates/hex-cli/tests/oci_devices.rs`). Or a default-policy create through the seam
  that loads or attaches a program (strace's `bpf` reading in `bench/competitive/probes/seed_path_inside.sh`).
- **Held memory at most crun's:** a density phase on fedora showing Hex's held memory per idle container above
  crun's. Of record (plan step 4, run `20261027T061920Z`, 92b3b91): 0.9849 and 0.9887 of crun's, 2 rounds of
  1,000 (`bench/competitive/RESULT.md`, "Of record on fedora"). That binary left each container's loopback
  down, which crun brings up. With it up (3bb57d3, adacachy run `20261027T165449Z`), Hex holds 0.9981, 1.0018 and
  1.0002 of crun's: parity, two rounds a hair above. A fedora run with loopback up is to come.

### 3. The fast path is the `Runtime` seam; the OCI command line stays for compatibility

**The choice.** The node agent implements the `oci` class behind `hex-api::Runtime`: from a runtime-spec
`Bundle`, and from the structured `OciSpec`, which it renders to one (`docs/design/runtime.md`, "a box
from an OciSpec"). It serves the class in process for callers that link Hex, and over a
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

#### The local socket (goal 3, plan step 4)

**What it is.** `hex node --socket <path> --state <dir> [--box-root <cgroup>]` runs the node agent as a process of
its own and serves the seam on a Unix `SOCK_SEQPACKET` socket (`crates/hex-node/src/serve.rs`). `hex-client`'s
`Client` is a `hex_api::Runtime` over it, so a caller that doesn't link Hex holds the same contract, the conformance
suite (HEX-RT-1 to 13) included (`crates/hex-cli/tests/node_socket.rs`, over a live `hex node`, on `oci` and `process`
boxes). `SOCK_SEQPACKET` because the kernel keeps each message whole and in order: "a sequenced-packet socket that is
connection-oriented, preserves message boundaries, and delivers messages in the order that they were sent"
(unix(7)). One message is one request or answer, so the codec needs no framing and a malformed message can't
desynchronize the next. A Unix socket also carries an exec's stdio as descriptors: `SCM_RIGHTS`, "Send or receive a
set of open file descriptors from another process" (unix(7)), received close-on-exec. And a message too large for
the receiver's buffer still reports its size, `MSG_TRUNC`: "return the real size of the packet or datagram, even when
it was longer than the passed buffer" (recv(2), for sequenced-packet sockets since Linux 3.4), so an oversized one is
refused naming its size and the bound.

**The wire** (`crates/hex-core/src/wire.rs`, a pure core): a 16-byte header (magic, version, kind, id) and a body
each kind defines; integers little-endian, strings and paths length-prefixed, options and enums tagged. Version 1's
bytes are recorded in its tests, and the format changes append-only: a later version adds kinds and appends fields,
and reads every earlier version's messages as they were. A reader refuses bytes past what a message's version
defines, so an extension an older agent can't apply is refused, never dropped (CLAUDE.md §3); a kind a later
version added is refused in an earlier version's message, as one that version doesn't define. A list's answer comes
in as many messages as its boxes need, so a dense node answers it whole. Version 2 (goal 4 plan step 2) adds one
request and its answer, the node's network namespace counts (decision 4), and writes every version 1 kind as
version 1 does but for its header's version. A client says hello in version 1, which an agent of any version reads,
learns the highest version the agent speaks from its answer, and writes the rest in the highest both speak; an
agent answers each request in its version, and refuses what it can't read in version 1, so a client of either
version reads the refusal.

**Bounds** (`serve::LIMITS`, told to each client in its hello), each refused past by type, naming it:

| Bound | Default | Past it | Why this size |
|---|---|---|---|
| connections | 64 | `Exhausted`, "connections to the node agent's socket", then closed | a node's local callers are few |
| bytes in one message | 131,072 | `Invalid`, naming the size and the bound | under the kernel's bound on a `SOCK_SEQPACKET` record, the send buffer less 32 bytes (`net/unix/af_unix.c`): 212,960 under the default 212,992-byte buffer, measured on adacachy (Linux 7.2.9) |
| requests in flight on a connection | 64 | `Exhausted`, "requests in flight on one connection" | twice the bench's 32 threads sharing one client |
| programs a connection watches | 1,024 | exec refused `Exhausted`, "programs watched by one connection" | 64 connections' worth is 65,536 pidfds, the supervisor's `INIT_BUDGET` |
| answers waiting to be sent on a connection | in flight + programs + 1, by construction | its requests aren't read until they drain | a client that doesn't read can't grow the agent's memory |
| workers running calls | 128 | calls wait in the queue the in-flight bounds hold | twice one connection's in-flight bound |

**The disconnect rule.** When a connection closes, every program its execs started that it still watches is sent
KILL through its pidfd, reaped, and counted in its box (`BoxInfo::abandoned`, appended to the seam with a default of
0, which inspect reports); none is left running unwatched. A program its client released first (it dropped the
`Proc`) runs on, as dropping an in-process `Proc` lets it. A request the agent read before the close is carried out;
one it hadn't read goes with the connection. The node ends the program rather than let it run to its end because:
- Its stdio are the gone client's pipes, so its output goes nowhere. Its next write raises SIGPIPE: "If all file
  descriptors referring to the read end of a pipe have been closed, then a write(2) will cause a SIGPIPE signal to be
  generated for the calling process" (pipe(7)). Until then it holds its box's CPU, memory and process budgets for
  work whose result no one can collect.
- No one is left to collect its end: the exit the agent would send has no reader, and a caller that comes back holds
  no `Proc` to wait on.
- An agent's step that outlives its caller can still act (write files, call out), and a caller that restarts and
  runs the step again runs it twice. Ending it makes "the caller went away" mean "its work stopped", the contract of
  a login session's processes ("Configures whether the processes of a user should be killed when the user logs
  out", logind.conf(5), `KillUserProcesses=`) and of a child bound to its parent (`PR_SET_PDEATHSIG(2const)`).
- The signal goes through the program's pidfd, so it can't reach a process given its pid since: "a PID file
  descriptor is a stable reference to a specific process" (pidfd_send_signal(2)).

Plan step 2 (below) makes an exec'd `oci` program the agent's own child, watched by the program's own pidfd, so
on the seam path the disconnect rule KILLs the program itself and no `hex __oci-exec` helper stands in for it. The command line keeps the helper: it ties the program to itself with `PR_SET_PDEATHSIG` once the
program has become the container's process, since "the parent-death signal setting is also cleared upon changes
to any of the following thread credentials: effective user ID, effective group ID, filesystem user ID, or
filesystem group ID" (`PR_SET_PDEATHSIG(2const)`), and a pipe the helper holds tells the program whether the
helper ended first. The gap that left — exec'ing a set-user-ID program clears the signal too (the same page) —
doesn't reach the seam path, where the program is the agent's own child, not a helper's.

**What would falsify the rule.** A caller that needs a program to outlive its connection: an executor that, when it
restarts, reattaches to a step still running rather than running it again. Then exec gains a detached form
(appended to the wire's exec), whose program the agent releases at the close, and the rule holds for the rest.

**Tests** (HEX-SOCK-n, cited beside each): 1, hostile bytes to the codec are refused by type, never a panic: every
truncation and bit flip, random bytes, unknown versions and kinds, trailing bytes, descriptors where none belong.
2, the format is append-only: version 1's bytes as recorded, every message round-trips, a long list splits. 3,
hostile messages to a live server are refused, and the connection is served after each. 4, connections past the
bound are refused naming it, and the next is served once one closes. 5, requests past the in-flight bound are refused
naming it, and every request is answered once. 6, the disconnect rule: a gone client's programs killed, reaped and
counted, a released one left running, a release sent just before a reset honoured. 7, programs past the bound are
refused naming it. 8, stdio crosses as descriptors, and only a pipe's right end is taken. 9, the plan's example over
a live `hex node`. 10, the node's network namespace counts cross the socket as the node holds them, a box with a
network namespace of its own counted over a live `hex node`, and a client of version 2 serves the seam to an
agent of version 1, writing in version 1 and refusing the counts by type, unsent.

**What it costs** (`bench/competitive/probes/socket_cost.md`, run 1: fedora, 2026-10-27, 0f41abb, both drivers in one
run, load 12 to 15, so not of record). A call over the socket costs 0.031 ms more than in process at p50: `inspect`
of a running box, whose work is the same either way, is 0.053 ms against 0.022 (p99 0.211 against 0.027). An exec of
`true` costs 0.204 ms more, its program's end being a second message, and a start 0.126 ms. Both are far under a
spawn, decision 3's falsifier: 0.666 ms on fedora (`probes/oci_breakdown.md` §2). Throughput holds: 42, 54 and 55
lifecycles/s at concurrency 1, 8 and 32 over the socket, against 43, 55 and 52 in process, with 32 threads sharing
one client.

#### The floor under exec, and where exec and start spend their time (goal 3, plan step 1)

The exec target, a quarter of libcrun's exec, is above the floor on fedora, so it stands. Any exec into a running
container pays at least the floor. It's measured here as a prepared single-threaded process that clones into the
container's cgroup with a pidfd, has the child join the container's namespaces and exec `/usr/bin/true`, and waits
(`bench/competitive/probes/exec_floor.c`). Against the libcrun exec that ran beside it, in each of three rounds
(`probes/exec_breakdown.md`, run 1: fedora, 2026-10-28, `hex 0.1.0 (f59dd3f)`, load 0.6 to 19 and I/O stalled, so not
of record for absolute numbers), the floor's p50 was 0.75 to 0.83 of a quarter of it: 0.409 ms against 0.511 at best
of 3. That leaves step 2's seed 0.09 to 0.14 ms over the floor for everything else an exec must do. The day's earlier
passes met the host in a state where every part of a call took about twice as long. The floor slowed more than
libcrun's exec there, and reached 0.96 of the target at worst.

The same run cut each traced exec and start into its parts at events of its critical path, and the parts sum to the
p50 of the untraced calls interleaved with them within 4.5% for exec and 7% for start, in each round. Exec, 1.41-1.42 ms
at p50 in the two rounds that met the same host, is 3.4x the floor. About half of it, 0.68 ms, is the `hex __oci-exec`
helper: its spawn and re-exec (0.43, of which 0.16 is the static-pie binary starting before its first system call), its
read of `config.json` and the container's namespaces, and the hop its own end adds before the node's wait returns. The
seed removes it. Start, 0.24-0.25 ms against libcrun's 0.10, is half the rewrite of `state.json` as running
(0.12-0.13), which libcrun doesn't make and plan step 3 removes, with the second read of the state.

#### Exec into a container: straight from the agent's thread, or through a seed (goal 3, plan step 2)

**What it is.** An exec through the seam spawns no `hex` process and re-execs no `hex` binary: the program the
container runs is the node agent's own child, which the supervisor watches by its pidfd, and no other process is
made for it. Where the agent may choose the container's pid namespace (a node that runs as root), its calling
thread starts the program the way posix_spawn does (`crates/hex-runtime/src/linux/oci/seed.rs`, `exec_direct`):

1. It sets its children's pid namespace to the container init's (`setns` with the init's pidfd and
   `CLONE_NEWPID`; that governs the calling thread's future children alone, `setns(2)`).
2. With every signal blocked, it clones the program with `clone3(CLONE_VM | CLONE_VFORK | CLONE_INTO_CGROUP |
   CLONE_PIDFD)` onto a stack of its own (`sys::clone_vfork`, an inline-assembly trampoline for x86_64 and aarch64;
   a guarded stack mapped once per thread). The program is born in the container's cgroup and pid namespace. It
   shares the agent's memory until its `execve`, so no page of the agent is copied for it and none torn down by its
   exec, and the thread waits until it has exec'd. The thread is pinned to its CPU for the clone, so the child,
   inheriting the pin, runs there as soon as the thread blocks, rather than wait for an idle CPU the kernel's fork
   balancing picks to wake; the child gives the pin back before anything else, so the program never runs pinned.
3. The child joins the container's other namespaces in one `setns` by the init's pidfd, becomes the container's
   process (`init::Becoming`: capabilities, user, seccomp filter, limits, made ready before the clone) and execs
   (its argv, environment and paths made before the clone too). It makes system calls alone on memory made before
   the clone: it runs in a multi-threaded process's memory, so it takes no lock and allocates nothing, as
   posix_spawn's child doesn't.
4. The thread's children's pid namespace is set back to the node's own.

Where the agent may not (a rootless node: choosing a pid namespace wants `CAP_SYS_ADMIN` in the caller's user
namespace and the namespace's owner's, `pidns_install` in kernel/pid_namespace.c), the exec goes to a seed: one of
a bounded pool of prepared, single-threaded `hex __oci-seed` processes (`crates/hex-node/src/seed.rs`), reached over
a `SOCK_SEQPACKET` socketpair (`crates/hex-core/src/seed.rs`, the `HEXS` codec) with the program's stdio and the
init's pidfd as `SCM_RIGHTS` descriptors. The seed forks a short-lived intermediate that joins the container's user
namespace first (which grants it the rest), then the others including the pid namespace, and clones the program
into the cgroup; the intermediate answers the seed, which reaps it, so the program has reparented to the agent (a
child subreaper, decision 2) before the agent is told its pidfd. A privileged seed (an architecture without the
trampoline, or a kernel without clone3 into a cgroup) starts the program in one clone of its own:
`clone3(CLONE_PARENT | CLONE_INTO_CGROUP | CLONE_PIDFD)` with `exit_signal` 0, the only value `clone3` takes with
`CLONE_PARENT` (`clone3_args_valid` in kernel/fork.c; `docs/research/runtime/seed-clone-parent.md`). The command
line's `hex oci exec` is unchanged.

Both paths join the namespaces the container's spec gives it (crun's exec joins the same), by the init's pidfd,
which names that process and never a later one given its pid. What an exec needs of the container — its bundle and
cgroup, as its state names them — is kept with the init the supervisor watches, from the box's create until the init
is reaped (`supervise::Container`), so an exec reads nothing of the box's from disk; it holds no descriptor for the
box, since every descriptor the agent holds is copied into an exec's child and closed again at its `execve`. The
program's stdio are the caller's pipes or the null device the agent holds open, borrowed, not duplicated. The seed
and the agent keep the bundles' parsed configs by the file's identity, so an exec costs one `stat` of `config.json`,
not a parse.

**Delete reaps the box's programs.** A container's init (pid 1 of the box's pid namespace) can't finish exiting
until every process in that namespace is reaped: the kernel holds it in `zap_pid_ns_processes` until then. An exec'd
program is such a process, and the agent's own child — not a descendant of the init — so the init can't reap it.
So on delete the agent reaps the box's exec'd programs after `cgroup.kill`, which frees the init to exit and the box
to empty, and records each exit for the caller's `Proc` to read. Each box bounds the programs the agent watches for
it at once (`crates/hex-node/src/supervise.rs`), refused past by type.

**Bounds**, each refused past by type: the seeds one node keeps, and the execs that wait for a free one
(`crates/hex-node/src/seed.rs`); the parsed configs a seed or the agent keeps (64, the oldest dropped first); a
thread's child stack (256 KiB, its overflow faulting on the guard page).

**Sources.** posix_spawn's clone (glibc `sysdeps/unix/sysv/linux/spawni.c`: `CLONE_VM | CLONE_VFORK` onto a stack
of the child's own, signals blocked, the child async-signal-safe); SOCK's zygotes, Catalyzer's `sfork` and AFaaS's
seeds (`oci-runtimes-and-fast-start.md` §5.1-5.4) for the seed pool.

**Evidence** (fedora, `kiln-a094a12d`, Intel Core i9-14900HX, Linux 6.19.10; `exec_breakdown.sh` with `CHECK=0`,
libcrun, the floor and the seam in each round, best of 2; loads named, so not of record for absolute numbers, the
comparisons being within each run). The traced execs' parts (`~/.cache/hex-t7/step6/segments.py`, p50 of 500
traced calls a run) showed where the time went and what each change took off:

| Run | What the seam did | libcrun exec | floor | seam exec | seam ÷ libcrun |
|---|---|---|---|---|---|
| `exec-breakdown-20261028T222232Z` (load 10) | the seed clones once (fork-style) | 1.866 | 0.366 | 0.769 | 0.41 |
| `…T223026Z` (load 16-18) | + the init's pidfd for the joins, parsed configs kept | 1.735 | 0.375 | 0.766 | 0.44 |
| `…T223528Z` (load 22-33) | + the execve made before the clone, `/dev/null` held | 2.012 | 0.429 | 0.742 | 0.37 |
| `…T224731Z` (load 3-5) | straight from the agent's thread, vfork-style | 1.983 | 0.325 | **0.487** | **0.245** |

The first three met the seed's cost: before the clone, 240 µs of the agent's bookkeeping, the hop to the seed and
its namespace opens (halved by the pidfd and the kept configs); then a fork-style clone of the seed (78-124 µs) and
a child that took 216-240 µs to its program's exec, against the floor's whole cycle of 0.33-0.47 ms. The fork
copies the seed's page tables, the child's writes copy pages, and the program's `execve` tears the copy down. A
vfork-style clone does none of that: in the floor's own modes, `posix_spawn` of `true` ran 0.06-0.09 ms under a
fork-style clone of it. Cloning from the agent's thread also drops the hop to the seed.

Two changes after the last row were each set against the build before them in one run (`bench/competitive/probes/
exec_ab.md`), the host in a slower state (below). The pin: the child first runs 18 µs after its fork rather than 86
(p50s, scheduler traces), and exec's p50 fell from 1.186-1.227 ms to 1.123-1.141. Keeping the container's bundle and
cgroup with the watched init took 29 µs off the agent's work before the clone (104 µs to 75), inside the rounds'
spread end to end.

**The host's state.** fedora runs a short burst of work at two speeds, and which depends on whether anything else
keeps its package awake. `exec_floor`'s `spin` gauge, a 0.1 ms chain of multiply-adds with no system call, took
0.085-0.097 ms in `…T224731Z` (load 3-5) and 0.224 ms in `exec-breakdown-20261028T231639Z` (load 2-3). With fedora at
rest, a burst ran at about a third of the speed it ran with one busy loop on a CPU outside the bench's (0.336 ms
against 0.104 on a P-core, 0.500 against 0.161 on an E-core; `probes/host_clock.sh`). At rest the floor took 2.4
times as long (0.785-0.815 ms) and libcrun's exec 1.9 times (3.817), since a long call ramps the clock itself: the
floor is 0.165 of libcrun's exec held awake and 0.21 at rest. The same build of the seam, against libcrun a minute
apart, was **0.203** of libcrun's exec held awake (0.372 ms against 1.834) and 0.260 at rest (0.996 against 3.831;
`probes/exec_ab.md`). At rest the kernel's part and the program's (the clone, the `execve`, `true` itself and its
exit) are about 1.0 ms of a traced exec's 1.19, and Hex's own about 170 µs: a quarter of libcrun's exec in that state
would take four fifths of Hex's own time away.

**Of record** (plan step 6; `bench/competitive/RESULT.md`, "Goal 3: the oci class through the seam"): host.sh's OCI
pass `~/.cache/hex-bench/out/20261029T001409Z` on fedora, 8224f97 (hex sha256 749fd88368188ade…, seam_oci
f4b4c60b87b6cabf…), load 0.86 to 3.58, best of 2, fedora at rest. Start p50 0.197 ms against libcrun's 0.196,
**1.005x** (bar 1.1, met). Exec p50 1.086 ms against libcrun's 3.756, **0.289x** (bar 0.25, not met at rest; 0.203
held awake in `exec-ab-20261029T002311Z`, not of record). The same pass on other machines (`bench/competitive/
docker_pass.sh`, the containers in Docker Desktop's Linux VM): jupiter (AMD Ryzen AI MAX+ 395, WSL2) exec **0.245x**,
start 0.815x; adas-mac-studio-3 (Apple M4 Max) exec **0.117x**, start 0.500x. Ada's call (2026-10-29): the three
passes together stand for goal 3's record; merged to main at 8cc9b43.

**What would falsify it.** A seam exec p50 above a quarter of libcrun's in one run on fedora. The floor (`exec_floor`,
step 1) measured beside libcrun in the same runs sits at 0.66-0.94 of that quarter (0.66 in the fast state, 0.82-0.85
in the slow), so the target stands; the seam has 0.06-0.17 ms over the floor to spend.

#### Start in one FIFO operation (goal 3, plan step 3)

**The choice.** Start does what crun's does: one operation on the exec FIFO. The runtime reads the container's
state once, opens the FIFO the init is blocked on, waits for its byte and removes the FIFO; it writes no state. A
container's status is derived as crun derives it (`state::derive`; libcrun's `libcrun_get_container_state_string`):
`created` while its init lives and the FIFO is there, `running` once the FIFO is gone, `stopped` once the init no
longer lives (by its pid and recorded start time). A state that a start from before this derivation marked
`running` reads as running while its init lives. Create records whether the bundle names poststart hooks, so start
reads `config.json` only when it does. The node agent tries an oci box's start first, reading none of the box's
record on that path, and watches the init's end through the pidfd its supervisor holds.

**A start holds both of the FIFO's ends, so no poll can lie** (found under goal 5 extended's load; Ada's call,
2026-11-03: a start that never depends on the hang-up). <!-- record: bench/competitive/probes/fifo_hup.md -->A poll on
the FIFO can report a hang-up as the init opens its end, before its byte: Linux's `pipe_poll` judges the hang-up
without the pipe's lock (v6.19 `fs/pipe.c:670`, `:691-692`), and an open for writing raises the writers' counter
before the writers (`:1192-1193`), so the read after it finds the init's end open and returns EAGAIN (`:374-376`). The
competitive bench saw one start refused so (`20261103T064217Z`), and from a reader opened read-only, as the start's
was, `probes/fifo_hup.c` saw 36 such polls in 18,000,000 (unit `hex-g5x-s6-verify21-20261103T201127Z`).<!-- /record -->
A hang-up needs a FIFO with no writer (`:691-692`), so the start opens its end to read and to write: Linux never blocks
that open and counts it a reader and a writer both (`:1202-1216`), so the init's blocking open to write returns at
once, and while the start holds its end the FIFO always has a writer. <!-- record: bench/competitive/probes/fifo_hup.md -->From
a reader opened so, `fifo_hup.c` saw none in 18,000,000 polls, every one of them input alone.<!-- /record --> The
init's end comes from its pidfd, polled beside the FIFO: the node agent hands the one its supervisor has held since
create, and the command line opens one by the state's pid and checks the start time create recorded before it trusts
it. The init's byte is counted (`FIONREAD`, under the pipe's lock, `:625-637`) and never read, so two starts at once
both see it and the FIFO's removal says which reports the start; the other finds the container running. An open that
crossed that removal and the close after it holds a new pipe on a FIFO with no name, the old pipe gone with its last
close (`:709-721`, `:1129-1147`), so a start looks for the name before it waits. Each wait ends at the start's
patience, a typed refusal; nothing sleeps (`run::wait_for_byte`).

**runc and crun beside it** (read at their tags, v1.5.2 and 1.30.1).
- runc's start opens the FIFO read-only with a blocking open, in a goroutine (`libcontainer/container_linux.go:236`,
  `:267-281`), and reads to end-of-file: a byte is the start, end-of-file alone "cannot start an already running
  container" (`:256-264`). Its init opens its end to write, blocking, writes `0` and closes it before its exec
  (`libcontainer/standard_init_linux.go:264-279`). It never polls the FIFO, so the race can't reach it. What it risks:
  while the open blocks it checks the init every 100 ms, by pid alone (`container_linux.go:242-251`), so it sees the
  init's end up to 100 ms late, and an init whose pid has gone to another process leaves the open blocked for good.
- crun has the init read and the start write. Create opens the FIFO read-only without blocking and hands it to the
  init (`src/libcrun/status.c:765`, `src/libcrun/container.c:2989-2993`); the init `select`s, then reads, and ends at
  any read error (`container.c:1309-1331`). The start opens its end to write, removes the FIFO, then writes its byte
  (`status.c:786-794`), so of two starts at once the second's removal fails before it writes. What it risks: the
  init's `select` is a poll the race can lie to (a start's open crossing it, the init reads EAGAIN and ends), and a
  start that ends between its removal and its write leaves the init reading end-of-file again and again, with no FIFO
  left for another start to open.
- Hex keeps runc's direction (the init writes, and a start waits for its byte) without its blocked open or its 100 ms
  tick, and decides between starts as crun does, by the FIFO's removal, after the byte rather than before it.

**Evidence.** Step 1 cut start into its parts: half of it (0.12-0.13 ms) was the rewrite of `state.json` as
running, with a second read of the state and a read of `config.json` for hooks the bundle didn't have. Without them,
in the breakdown run `exec-breakdown-20261028T224731Z`, start was 0.084-0.090 ms against libcrun's 0.083-0.089.

**Tests.** The derivation as a pure function, against an oracle table of every combination (recorded status, the
FIFO there or not, the init live with its start time, live with another, a zombie or gone): `created`, `running` or
`stopped` as the runtime-spec defines them (`state::tests`, AC-T7.1.4). With real FIFOs, pidfds and polls
(`run::tests`, AC-T7.1.4): a start sees the init's byte whichever end opened first; a writer that came and went without
a byte is no end while the init lives (with the start's end opened read-only, as before, the poll reports a hang-up
and the test fails); an init that ended without its byte is stopped by its pidfd, and one that wrote it and ended is
started; of two starts at once, 64 times, one starts the container and the other finds it running; an open crossing
another's removal and close finds no name; a pid given to a
later process is never taken for the init. runtime-tools and the cgroups lab hold start and state to the runtime-spec
on the command line.

#### Descriptors reach a program only on purpose (found under goal 5 extended, 2026-11-03)

**The choice.** The agent is multi-threaded, so a descriptor whose close-on-exec it cleared for one spawn reaches
whatever another thread starts in that window: another create's shim, an exec'd program, a hook. Such a process could
hold a create's handshake from its end-of-file, write a forged pid line into it, take its map acknowledgement, or read
and answer on a seed's socket. That is the class of CVE-2024-21626, where a descriptor runc leaked into its init let a
container reach the host. So the agent never clears the flag. The shim opens its pipes by the path of the agent's
descriptors for them (`/proc/<pid>/fd/<n>`), as it opens a network namespace made ahead, and the agent keeps its ends
until the shim's first word, watching the shim's pidfd until then. A seed's socket rides in as its stdin, which the
spawn duplicates into the child alone, and the seed moves it off fd 0 and puts `/dev/null` there.

**Before foreign code.** Whatever a window anywhere above it left a process holding, every descriptor from 3 up is
marked close-on-exec before foreign code runs (`linux::sys::mark_cloexec_from_3`): in `Becoming::apply`, which a
container's init, the exec helper and a seed's exec child all make, before the seccomp filter a strict profile would
refuse it under; in the `process` class's two children; in a hook's, which the agent starts itself; and at a seed's
start. `close_range(2)`; where the kernel
refuses it, each descriptor `/proc/self/fd` lists; with neither, the start is refused. It allocates nothing, so a
vfork-style child can make it. runc does the same before its init and before the container's exec, create and exec
both (v1.5.2 `libcontainer/container_linux.go:341-349`, `init_linux.go:300-306`, called from
`standard_init_linux.go:201` and `setns_init_linux.go:117`), and crun before each execv, create and exec both (1.30.1
`src/libcrun/container.c:1096`, `:1444`, `:3488`, `:3573`, falling back as Hex does at `utils.c:1959-1975`).

**Evidence.** runc v1.5.2 has the Go runtime dup2 the files it passes in the child, so its parent never clears the flag
(`libcontainer/container_linux.go:341-349`). Here, a test's `sleep` started in the window held a shim's sync pipe, and
the create's handshake with it, for the sleep's 30 s (fedora, unit `hex-g5x-s6-fdh1-20261103T211513Z`).

**Tests.** A bystander started inside the shim's spawn, as another thread's would be, holds no pipe past its stdio
(`run::tests`, AC-T7.1.4). With the flag cleared before the spawn, it held both of the create's (unit
`hex-g5x-s6-fdh2-20261103T211928Z`). A descriptor opened without the flag has it after the mark, by `close_range` and by
the walk of `/proc/self/fd` (`linux::sys::tests`), and a hook holds nothing the agent left open (`run::tests`).

**What would falsify it.** A bystander holding one of a create's descriptors with the flag never cleared: a descriptor
the agent opens without close-on-exec.

### 4. Network-namespace work off the critical path (restated on goal 4's measurements, 2026-10-29)

**What the probe showed first.** The plan behind this document expected pooled network namespaces and cgroups to
lift the throughput ceiling, from SOCK's finding: about 200 containers/s with unmodified network namespaces against
900/s without them, "due to a single global lock" (Oakes et al., ATC 2018, §2.2; f8). Probe run 1 met that
decision's falsifier: on fedora's Linux 6.19, dropping the bundle's network namespace raised throughput at
concurrency 32 only 5% for Hex (1,029 to 1,079/s) and 7% for crun (1,171 to 1,251/s) (`probes/oci_breakdown.md`
§4). SOCK's kernel predates the fixes to namespace creation (`oci-runtimes-and-fast-start.md` §4.3), so this
decision doesn't rest on throughput, and the cgroup pool was dropped: no measurement puts cgroup creation on the
critical path. What the probe did show was latency: at concurrency 1 the namespace cost Hex about 5.5 ms a
lifecycle, 44 against 58 lifecycles/s, but crun only about 1 ms, 68 against 73.

**What it shows now.** That gap went with the shim and the state fsyncs (decisions 1 and 2). Through the seam
today, a container's network namespace costs Hex's create about half a millisecond on each machine measured, and
its delete nothing; libcrun's create pays a little less to a little more (`bench/competitive/probes/netns_pool.md`;
kiln research `01a185679f9073d696fc7e1a7c21587e`, f2 and f3). The runs: `probes/netns_cost_inside.sh`, 2 rounds of
500, on fedora (2026-10-29T02:09Z, load 3.66 to 5.11, hex 749fd883 and seam_oci f4b4c60b built at 8224f97), on
jupiter (Docker Desktop's WSL2 VM, VM load 5.35 to 8.03) and on adas-mac-studio-3 (Docker Desktop's LinuxKit VM,
VM load 4.38 to 4.90), those two built at 3db2173, 8224f97's code; libcrun 1.30.1 on each. Create and delete p50
in ms, with the namespace and without, best of 2:

<!-- record: bench/competitive/probes/netns_pool.md -->
| Machine | Hex create | the namespace | libcrun create | the namespace | Hex delete |
|---|---|---|---|---|---|
| fedora (i9-14900HX, Linux 6.19.10) | 15.177 / 14.606 | +0.571 (1.04x) | 2.970 / 2.422 | +0.548 (1.23x) | 6.737 / 6.774 |
| jupiter (Ryzen AI MAX+ 395, WSL2 6.18) | 14.244 / 13.679 | +0.565 (1.04x) | 5.089 / 4.321 | +0.769 (1.18x) | 5.784 / 5.826 |
| adas-mac-studio-3 (M4 Max, LinuxKit 6.12) | 2.936 / 2.392 | +0.544 (1.23x) | 2.056 / 1.849 | +0.206 (1.11x) | 2.189 / 2.251 |
<!-- /record -->

What follows from it (`netns_pool.md`, "What follows from them", with the arithmetic):

<!-- record: bench/competitive/probes/netns_pool.md -->
| | fedora | jupiter | adas-mac-studio-3 |
|---|---|---|---|
| Hex's create without a namespace, over libcrun's | 6.03x | 3.17x | 1.29x |
| what the namespace costs Hex, over what it costs libcrun | | | 2.64x |
| the namespace's share of Hex's create | | | 22.7% |
<!-- /record -->

Delete pays nothing because the kernel already defers a namespace's teardown: its last reference queues it for a
work item (`__put_net`, `net/core/net_namespace.c:739-744` at v6.19), which neither the exiting process nor delete
waits for. The decision's first falsifier, Hex costing no more with the namespace than crun, holds on fedora and
jupiter but not on the Mac Studio. And on fedora and jupiter a create with the namespace is within 5% of one
without only because Hex's create is slow there: that gap is goal 5's (the seed), and once it closes, the
namespace's half millisecond is the share of create it already is on the Mac Studio. So what is left to move is
the namespace itself. The shim makes it with the container's other fresh namespaces (`unshare`, `init.rs`
`namespace_flags`), the kernel running every networking subsystem's per-namespace init to do so (`setup_net`,
`net_namespace.c:445-446`), and the container brings loopback up over route netlink before it is ready
(`init.rs:175-176`, `net.rs`).

**The choice.** The node agent keeps a bounded pool of fresh network namespaces, made ahead on a thread of its own:
the thread unshares a new one (`unshare(CLONE_NEWNET)` on that thread, as containerd makes a pod's,
`pkg/netns/netns_linux.go:115-116` at v2.2.0), keeps it alive by an open descriptor, close-on-exec ("As long as
this file descriptor remains open, the namespace will remain alive", namespaces(7)), and brings its loopback up
inside it. A create whose container makes a network namespace of its own takes one: its shim joins it with `setns`
in place of unsharing, and the container finds loopback up. Each pooled namespace goes to one container and is never
handed back or on, so nothing of one container's network reaches the next and no cleaning is needed (Lambda reuses a
released network after cleaning it; AFaaS warns that reusing a stack "introduces security risks by reusing the TCP/IP
stack from previous containers", Chai et al., OSDI 2025, p.504). A dry pool falls back to making the namespace on the
path, as today, and counts it. Teardown stays where the kernel puts it, after delete returns.

**Who may take one.** A namespace belongs to the user namespace its maker was in ("When a nonuser namespace is
created, it is owned by the user namespace in which the creating process was a member", user_namespaces(7)), and
joining a network namespace takes `CAP_SYS_ADMIN` in the one that owns it (setns(2)). A namespace the agent makes
ahead is the agent's user namespace's, so the pool serves containers in that user namespace only. A container in
one of its own, made by its spec or joined by path, as a rootless node's are, makes its network namespace on the
path, where it is its own; a container whose spec names its network namespace by path joins that one and takes
none. The pool counts a namespace made on the path for a container in a user namespace of its own as it counts a dry
take's, so a rootless node reports every namespace it makes as made on the path.

**How the node does it** (goal 4 plan step 4: `hex_node::netns`, `oci::net::make_ahead`). A root node starts one
thread when it opens, `hex-netns`, which makes a namespace whenever the pool asks for one and puts itself back in
its own namespace after each, on every path, as containerd's maker puts its thread back (`defer origNS.Set()`,
`netns_linux.go:121-122`), so the thread holds none of the namespaces it makes. A rootless node's pool has a bound
of 0 and no thread. The runtime asks the pool once it has read the spec, so a bundle is parsed once
(`run::Networks`): a container that makes a network namespace of its own and has no user namespace takes one; one in
a user namespace of its own is counted as made on the path; one whose network namespace is given by path, or that
has none, asks nothing. The take comes last, just before the shim, so a create refused before it takes none, and the
pool counts at the take, so a create that fails after it is counted all the same; a shut-down pool or a poisoned
lock refuses the create by type. The shim gets the namespace by the path of the agent's descriptor for it
(`/proc/<agent>/fd/<n>`), opens it before it changes any namespace, and joins it with `setns` in place of unsharing
`CLONE_NEWNET`; the agent's descriptor stays close-on-exec throughout and goes once the create's handshake ends. It
isn't handed over by clearing that flag: the agent is multi-threaded, so any process it started while the flag was
clear (another create's shim, a seed, a `process` box's program, a hook, an exec's clone) would inherit it, and
nothing closes a stray descriptor before `execve`, so another container's program would hold this one's namespace
(the first runtime side did this, 13636cb; HEX-NETNS-6 failed on it). The container's processes are then all that
hold its namespace, so it ends with them, and the kernel tears it down after delete returns.

**What it measures.** With the pool in the node (5257407), the step-2 phases through `docker_pass.sh` on
adas-mac-studio-3 (2026-10-29T04:09Z, VM load 3.51 to 5.88, 2 rounds of 1,000, `probes/netns_pool.md`), create p50 in
ms, best of 2 each:

<!-- record: bench/competitive/probes/netns_pool.md -->
| adas-mac-studio-3, 5257407 | with the namespace | without | with over without |
|---|---|---|---|
| Hex in process, from the pool | 1.723 | 1.714 | 1.01 |
| Hex over the socket, from the pool | 1.735 | 1.790 | 0.97 |
| libcrun, made on the path | 1.983 | 1.644 | 1.21 |
<!-- /record -->

Every create of the lifecycle phase took a namespace from the pool and none was made on the path, so on the machine
where the namespace was 23% of Hex's create it now costs a create about what a create without one costs; the
pass of record on fedora, jupiter and this Mac Studio, with the sustained phase, is goal 4 plan step 5's.

**Of record** (goal 4 plan step 5, 2026-10-29). The step-2 phases at 378b1d2 (the pool's production code unchanged since
5257407), 2 rounds of 1,000 and a sustained phase of 10,000 lifecycles at concurrency 32: on fedora by host.sh under
CI's load (16.84 to 9.44, not gated), on adas-mac-studio-3 by `docker_pass.sh` (VM load 3.84 to 6.30), and on jupiter by
`docker_pass.sh` at 6f73f9a (the same production code but for the reap race's fix; VM load 1.71 to 8.44). Create p50 in
ms, best of 2 each, and how many of the sustained phase's creates found the pool dry (`bench/competitive/RESULT.md`):

<!-- record: bench/competitive/RESULT.md -->
| Machine | | with the namespace | without | with over without | sustained /s | made on the path, of the sustained phase's creates |
|---|---|---|---|---|---|---|
| fedora | Hex in process | 11.323 | 23.545 | 0.48 | 26 | 0 of 10,000 |
| | Hex over the socket | 14.143 | 14.580 | 0.97 | 51 | 0 of 10,000 |
| | libcrun | 3.057 | 2.406 | 1.27 | 1,294 | |
| adas-mac-studio-3 | Hex in process | 1.699 | 1.717 | 0.99 | 553 | 0 of 10,000 |
| | Hex over the socket | 1.725 | 1.813 | 0.95 | 352 | 0 of 10,000 |
| | libcrun | 1.970 | 1.629 | 1.21 | 1,429 | |
| jupiter | Hex in process | 8.590 | 8.298 | 1.04 | 108 | 0 of 10,000 |
| | Hex over the socket | 12.714 | 12.661 | 1.00 | 66 | 0 of 10,000 |
| | libcrun | 3.935 | 3.405 | 1.16 | 959 | |
<!-- /record -->

Neither falsifier is met. No create from the pool costs more than 5% over one without a namespace: on the Mac Studio,
where the namespace was 23% of Hex's create, it now costs what a create without one costs; fedora's in-process ratio
pairs a quiet round with a busy one (each round's own was 0.47 and 1.05, Hex's create moving between 11 and 25 ms as
CI came and went), and over the socket its rounds held at 0.97 and 0.98. And no sustained phase found the pool dry: 0
of 10,000 on each machine, by each driver, in each round, so one refill thread keeps up with 553 lifecycles a second.
Ada's call (2026-10-29): merged to main at 6446160 on fedora's and the Mac Studio's passes; jupiter's followed,
and holds the same: 1.04 in process and 1.00 over the socket, libcrun 1.16, the pool never dry.

**The bound and the refill.** Measured by `probes/netns_pool.c` (one thread making namespaces as the pool would and
holding them in a cgroup of its own), two rounds of 500 each: on fedora at 2026-10-29T03:00Z (load 5.14 to 4.14) and
on adas-mac-studio-3 at 03:01Z (VM load 2.44 to 2.13), bench/competitive at 5ee4d1f with these probes:

<!-- record: bench/competitive/probes/netns_pool.md -->
| Machine | make p50 | make p99 | one thread | charged to the maker's cgroup | the host's Slab | the host's Percpu |
|---|---|---|---|---|---|---|
| fedora | 0.152, 0.161 ms | 0.333, 0.401 ms | 6,127, 5,260/s | 108.5, 107.8 KiB (slab 97.6, percpu 10.1) | +244.3, +236.3 KiB | +105.2, +98.6 KiB |
| adas-mac-studio-3 | 0.407, 0.258 ms | 1.119, 0.711 ms | 2,280, 3,545/s | 320.4 KiB (slab 287.1, percpu 33.2) | +622.0, +616.0 KiB | +140.0, +139.0 KiB |
<!-- /record -->

What a held namespace costs the kernel at most, the host's Slab and Percpu rises together (which other work on the
host moves as well, so an upper bound), and what a pool of them holds:

<!-- record: bench/competitive/probes/netns_pool.md -->
| | fedora | adas-mac-studio-3 |
|---|---|---|
| kernel memory per held namespace, at most | 349.5, 334.9 KiB (0.34 MiB) | 762.0, 755.0 KiB (0.74 MiB) |
| a pool of 32 | | 24384.0 KiB (23.8 MiB) |
<!-- /record -->

The pool holds at most **32**: the most creates the bench's busiest phase has in flight at once (concurrency 32), so
a burst of its whole width finds a namespace each, at most 24 MiB of the kernel's memory on the heavier machine. Its
32 descriptors sit beside the agent's one pidfd per box, which an exec's copy of the agent's descriptor table
(`STATUS.md`, Open) hardly notices. One thread refills whenever the pool is below its bound, at 2,280/s or more;
and it keeps up with sustained churn: no sustained phase of record found the pool dry (above). A make that fails
stops the refill until the next take, so a kernel that refuses every make costs that thread one failed make a take,
never a loop of them (CLAUDE.md §2). Takes go oldest first, so under churn each namespace is handed out within a
bound's worth of takes of its making, none left in the pool while others come and go. The node counts what its
containers' namespaces were (made ahead, taken from the pool, made on a create's path), and seam_oci reads the counts
around each of its phases, in process (`Node::netns`) and over the socket (the wire's version 2), so the sustained
phase reports how many of its creates found the pool dry (goal 4 plan step 2).

**Where its memory is charged.** The kernel charges what it can of a namespace to the cgroup of the task that makes
it, when it is made (`struct net` is `SLAB_ACCOUNT`, `net_namespace.c:1259-1261`), and doesn't move it after. A
pooled namespace's 108-320 KiB so stays charged to the node agent, not to the box that joins it. The node reports
what its pool holds, and a density reading of Hex counts the agent's cgroup with its boxes', so a box isn't read as
lighter than it is (AC-T7.1.5 bounds what the agent adds per box).

**Tests.** The pool's policy is a pure core, `hex_core::netns` (goal 4 plan step 3), held to an oracle under random
takes, made and failed makes, creates it can't serve and shutdowns, with every path taken over the run: it never
holds more than its bound, and a namespace made past it is refused and handed back (HEX-NETNS-1); it hands each
namespace out once, the oldest first, and never takes one back (HEX-NETNS-2); a take from it dry is answered with a
namespace made on the path, and counted, as is a create it can't serve, which takes nothing (HEX-NETNS-3); after
shutdown a take is refused, and what it held is handed back and counted as discarded (HEX-NETNS-4); it asks for the
room below its bound until a make fails, then for nothing until the next take (HEX-NETNS-5). Its counts (made ahead,
taken, made on the path, discarded, failed) are what the bench reads, and they equal the oracle's. A doc-truth test
holds the core's bound to this decision's. The node's shell (goal 4 plan step 4): a stand-in shim holds no
descriptor of the namespace made ahead it is handed, reaches it by its path, and finds the agent's descriptor
close-on-exec meanwhile (HEX-NETNS-6); the runtime takes for a container that makes its own namespace with no user
namespace, counts one in a user namespace of its own on the path, asks nothing for one given by path or none, and
refuses the create when the pool refuses (HEX-NETNS-7, against a recording fake); as root, two boxes take one each,
their namespaces differ from each other's, an earlier box's and the host's, loopback up, and the agent holds neither
(HEX-NETNS-8, `oci_box`); a pool of 0 makes a box's namespace on the path, loopback up, counted so (HEX-NETNS-9);
rootless, the box's namespace is made in its own user namespace, which owns it, nothing taken (HEX-NETNS-10); and
the refill thread fills its pool to the bound as root, each namespace its own with loopback up, or counts each
failed make where it may not and serves takes on the path (HEX-NETNS-11). The hex-api conformance suite passes over
the socket on `oci` boxes that each take a pooled namespace as root, or make their own on the path rootless
(`node_socket`), as it does in process on boxes from OciSpecs with a private network (`oci_spec_box`).

**Sources.** `docs/research/runtime/netns-pool.md`: the kernel's `net/core/net_namespace.c` at v6.19; namespaces(7),
setns(2) and user_namespaces(7); containerd v2.2.0's `pkg/netns`; Lambda's networking account (Vogels, 2026); AFaaS
(Chai et al., OSDI 2025); Scrivano's `unshare -n` timings (2022); SOCK, for the history.

**What would falsify it.** A create from the pool costing more than 5% over a create without a namespace, at p50
through the seam, on any machine measured (fedora, jupiter, the Mac Studio): the pool then isn't taking the
namespace's work off the path. Or a sustained phase in which more than 1% of creates found the pool dry: one thread
refilling to 32 isn't keeping up. That one has been met (goal 5 plan step 2's pass of record on fedora,
`bench/competitive/RESULT.md`, 20261030T004533Z): once the record left create's path and the seam ran at about 2,100
creates a second, the pool ran dry for 181-279 of each sustained phase's 10,000 creates (1.8-2.8%), each made on the
path and counted, none failed. At goal 4's 553 a second at most it kept up; at 2,100 one refill thread doesn't. Open
(`STATUS.md`).

### 5. A seed per node clones containers (restated on goal 5's measurements, 2026-10-30)

**What the plan expected.** This decision first rested on one figure: create's second exec of the hex binary, about
1 ms on fedora (`hex --version`, 1.038 ms; kiln research `01a1877e05cc73b495447bf5113ebda3`, f3), a sixth of a create
with its state on tmpfs. Measured whole through the seam today, that exec is a twentieth of create.

**What a seam create is made of** (goal 5 plan step 1; `bench/competitive/probes/create_breakdown.md`, run 1). The
create breakdown traced 1,500 seam creates in process, in the OCI pass's bench container on fedora (`kiln-a094a12d`,
Intel Core i9-14900HX, Linux 6.19.10, the container on CPUs 4-31), 2026-10-30T00:08:22Z to 00:12:35Z. The 1-minute
load was 7.17 at the start and 6.85 at the end, from other work on the host, which ran in its at-rest clock state
(`probes/host_clock.sh`). Binaries: `hex 0.1.0 (ef64dbc)`, sha256 596969cb3a612644; `seam_oci` sha256
0656cae0a8464d57; libcrun 1.30.1 in process beside it, in the same three rounds. The node's state was where the OCI pass
keeps it, on the container's overlay over fedora's btrfs.

**The bench's `/run` was overlayfs.** In the OCI pass's container `/run` is the overlay too (the run's environment;
only `/dev/shm` is a tmpfs there, as step 2 found), and `oci_bench.py` keeps every runtime's state under it, libcrun's
in `/run/libcrun` (`oci_bench.py:70`). So in this run both runtimes' state sat on overlayfs, where a host's `/run` is a
tmpfs. This is the baseline: today's code in today's container. Step 2 mounts a tmpfs at `/run` in the pass's
containers, so a pass matches a real host.

Each traced create is cut into 35 parts at system calls on its critical path, from the call's marker to its return,
across the node agent, the shim and the container's init. The parts' p50s summed are 99.5% of the p50 of the untraced
creates between them (99.1-100.1% by round), so they account for the whole create (the step's criterion: within 10%).
By group (ms; each group its parts' p50s summed):

<!-- record: bench/competitive/probes/create_breakdown.md -->
| Group | r1 | r2 | r3 | pooled | share |
|---|---|---|---|---|---|
| the box's record | 12.785 | 12.698 | 13.049 | 12.841 | 70.5% |
| the init's set-up, to the exec FIFO | 2.534 | 2.485 | 2.507 | 2.508 | 13.8% |
| the node's and the runtime's own work | 1.386 | 1.354 | 1.360 | 1.365 | 7.5% |
| the shim's spawn and exec | 0.888 | 0.868 | 0.878 | 0.878 | 4.8% |
| the cgroup, made and bounded | 0.477 | 0.463 | 0.474 | 0.473 | 2.6% |
| the handshake | 0.141 | 0.143 | 0.143 | 0.142 | 0.8% |
| the parts, summed | 18.211 | 18.011 | 18.411 | 18.207 | |
| the creates between the traced ones, p50 | 18.185 | 17.997 | 18.573 | 18.297 | |
| libcrun's create, p50 | 5.055 | 4.814 | 5.183 | 5.015 | |
<!-- /record -->

**The levers, in order.** Each takes the parts named off create's path. Its share is those parts' share of create's
p50 (18.297 ms):

<!-- record: bench/competitive/probes/create_breakdown.md -->
| Lever | Plan step | Its parts | ms | Share of create |
|---|---|---|---|---|
| the record off create's path | 2 | the record's four fsyncs | 12.187 | 66.6% |
| the seed, several per node | 3 and 4 | the shim's spawn and exec | 0.878 | 4.8% |
| a pool of cgroups made ahead | 5 | the cgroup's making: the box root listed and the mkdir | 0.372 | 2.0% |
<!-- /record -->

1. **The record off create's path** (plan step 2) comes first.
   - **Why first.** Its fsyncs are two thirds of create. The store also holds its one write lock across them
     (`store.rs:51`; f5), so under concurrency creates wait on each other's fsyncs and run one at a time (f8). No other
     lever shows at concurrency 32 while they stand.
   - **What it costs.** Each durable write's pair of fsyncs, the file's and its directory's, took 6.028 and 6.159 ms at
     p50 on the overlay. The fsync probe found 7.138 and 7.481 ms for the same pair on fedora's `/home` (f6).
   - **Why it can go.** Runtimes keep a container's state where a host crash takes it with the containers it describes:
     runc's `--root` asks for tmpfs, and crun keeps its state under `/run/crun` (f11). Step 2 holds the box's record to
     that rule, so its writes leave the fsyncs and the lock behind.
   - **What stays.** The record's temp files, writes and renames take 0.654 ms on the overlay. On a tmpfs runtime
     directory they take less. In the bench container `/run` is the overlay as well (the run's environment), so step 2's
     pass needs a tmpfs there for the record to land where the node would put it.
   - **What would falsify it:** Hex's create through the seam on fedora not falling by about the fsyncs' 12.187 ms once
     step 2 is in. Or the sustained phase's rate at concurrency 32 not rising once the lock is no longer held across
     them.
2. **The seed, several per node** (plan steps 3 and 4) is next.
   - **What it removes.** The shim's spawn and exec, 0.878 ms (4.8%), which a seed does once and never again: the
     clone of the agent (0.084 ms), the hex binary's execve (0.256), its start as a static-pie binary (0.354), and
     Rust's start-up up to its config (0.184).
   - **What stays.** Today's shim already does a seed's per-create work after its exec, single-threaded: it reads and
     parses the spec, makes the namespaces and clones the init into its cgroup. That takes 0.527 ms at p50 (parts 19, 21
     and 22), and this serial part sets a seed's rate (f14). The init's own set-up after its clone (1.974 ms: its root
     mounted and pivoted, the process become) runs in the init, beside the seed's next clone.
   - **The plan's other conditions stand.** A seed's child execs the container's program, so the program's address
     space is randomized afresh (f17). Any state unique to the container is made anew in the child before that exec
     (f15).
   - **What would falsify it:** a seed's clone of a container's init, from the request's arrival to the init's first run,
     costing as much as the spawn and exec it replaces: 0.878 ms in this run. Step 4's bar, `hex --version`'s 1.038 ms,
     is a spawn, exec and exit of the same binary, timed by the earlier probe's spawning process (f1, f3).
     **Met (goal 5 plan step 4, `bench/competitive/probes/seed_path.md`):**
     <!-- record: bench/competitive/probes/seed_path.md -->a seed's clone of a container's init, from the request's
     arrival at the seed to the init's first run, is 0.234 ms p50 (p99 1.050; run 3), and to the init's birth 0.209 to
     0.256 ms p50 in the probe's three runs: under a quarter of the 1.038 ms exec it replaces, and under a third of the
     0.878 ms of the shim's spawn and exec.<!-- /record -->
3. **A pool of cgroups made ahead** (plan step 5) comes last, if at all.
   - **What it would remove.** Making and bounding the box's cgroup takes 0.473 ms (2.6%): listing the box root to count
     its boxes (0.094 ms), the `mkdir` (0.278) and writing the bounds (0.101). A cgroup made ahead still has its bounds
     written when it is taken.
   - **Its share.** Of what's left once the first two levers are in, 5.142 ms, the cgroup is 9.2% and its making 7.2%.
     That is under step 5's tenth, so on this run the pool isn't built. Step 5 measures the share again with this probe
     once steps 2 and 4 are in, and builds the pool only if it has reached a tenth.
   - **What others found.** SOCK measured reusing a cgroup at twice the speed of making one, on Linux 4.13, and pooled
     them; AFaaS pools them too (f16). AFaaS's own lesson is that "not all kernel resources require pooling; it is
     crucial to identify the minimal necessary set" (Chai et al., OSDI 2025, §8, p.512).
   - **What would falsify it:** the cgroup, made and bounded, at a tenth or more of what's left of a create once steps 2
     and 4 are in.
   - **Step 5's measure** (2026-10-31): <!-- record: bench/competitive/probes/create_breakdown.md -->with steps 2 and 4
     in, the cgroup, made and bounded, is 0.184 ms of a create whose parts sum to 2.134 ms, 8.6%, and its making 0.139
     (6.6%; run 4, create-breakdown-20261031T061841Z).<!-- /record --> That is under the tenth, so no pool is built and
     goal 5 plan step 5 closes on this record. The cgroup's work goes instead to goal 5's added step for fewer
     `cgroup_mutex` holds per lifecycle, which reuses a box's cgroup for the next box of the same owner once it's
     clean (Ada's call, 2026-10-31).
   - **What step 4 adds.** At concurrency 32 the rate is what `cgroup_mutex` admits, and a cgroup made and removed per
     lifecycle is most of what holds it (below, "Concurrency 32"). A pool that makes cgroups ahead and removes them
     after use still takes the lock as often, so under a sustained rate it doesn't lift that ceiling (under a burst it
     can move the holds out of the burst); only one that reuses a cgroup from box to box would.

**What's left, and the bar.** Goal 5's bar is Hex's create p50 at most a quarter of libcrun's in the same run: 1.254 ms
here. The three levers can't reach it. Once the record's fsyncs and the shim's spawn and exec are gone, 5.142 ms is left:
1.025 of libcrun's create in the same run, and 4.101 of the bar. Nearly all of it is what any runtime does to make a
fresh container, and libcrun does it too, in its whole create of 5.015 ms. By part:

<!-- record: bench/competitive/probes/create_breakdown.md -->
| What's left | ms | Share |
|---|---|---|
| the init's root: mounted, then pivot_root | 1.839 | 35.8% |
| the init's other set-up: the shim's config read, the namespaces joined and made, the init's clone, the process become | 0.669 | 13.0% |
| the record's temp files, writes and renames, without their fsyncs | 0.654 | 12.7% |
| the device program, loaded and attached | 0.583 | 11.3% |
| the node's and the runtime's other work | 0.523 | 10.2% |
| the cgroup, made and bounded | 0.473 | 9.2% |
| the container's state written (state.json) and read back | 0.259 | 5.0% |
| the handshake | 0.142 | 2.8% |
<!-- /record -->

Even with the init's root, the device program, the record's and the state's writes and the cgroup's making each taken
away whole, 1.435 ms is left: still 1.145 of the bar. Three of those parts are measured here as levers beyond this
plan:
- **The init's root** (35.8% of what's left). The mounts take 0.966 ms and `pivot_root` 0.873.
  - `pivot_root` walks every thread on the host, to move each one whose root or working directory was the old root
    (`chroot_fs_refs`, Linux v6.19 `fs/fs_struct.c:60-61`). So it grows with a node's density: the host had 3,310 to
    3,752 threads during this run.
  - Decision 6 makes the container's mount namespace from its tree, `open_tree(OPEN_TREE_NAMESPACE)` and `setns`, in
    place of `unshare(CLONE_NEWNS)`, the root's bind and `pivot_root`, as crun does where it can
    (`oci-runtimes-and-fast-start.md` §4.2). That needs Linux 7.0, and fedora runs 6.19.
- **The device program** (11.3%). It is loaded and verified with each create (0.491 ms), because the program a policy
  shares ends with the last cgroup it is attached to, and in the lifecycle phase no other container holds it. A node
  that kept its policies' programs loaded would only attach them: decision 2's sharing, with the agent as the holder
  that outlives every container. Built in goal 5's extension, step 1 (decision 2): the node holds each policy's
  program, and the default policy's sits on the oci boxes' root, so a default-policy create loads and attaches
  nothing. <!-- record: bench/competitive/probes/create_breakdown.md -->Its parts fell from 0.304 and 0.311 ms in the
  base binary's runs to nothing, and Hex's create p50 from 2.217 and 2.171 ms to 1.825, 1.955 and 1.787 on one host
  state, libcrun's beside them 2.903 to 3.084 (run 5). With both binaries in every round of a run, step 1's create p50
  was 0.762, 0.833 and 0.681 of the base's (run 6), and 0.789 and 0.794 (run 7).<!-- /record -->
- **The record's and the state's writes** (17.7%). They are files made, written and renamed on the overlay, and on the
  tmpfs step 2 moves them to they cost less.

Beyond those, a quarter of libcrun's create needs most of a fresh container's own set-up done before the create asks for
it: its namespaces, root and process. None of these levers does that. That is the measured case for containers made
ahead, which needs research of its own and isn't decided here.

The tails follow the fsyncs. Hex's create p99.9 was 36.007 to 55.114 ms by round, against libcrun's 7.376 to 14.212, and
each fsync's p99 was 5.719 to 6.333 ms.

**What would falsify this account.**
- The breakdown's parts summed falling outside 10% of create's p50 in a later run of the probe: the parts then miss
  part of create.
- Once steps 2 and 4 are in, Hex's create through the seam on fedora more than 10% above the 5.142 ms they leave here
  plus the seed's clone: a lever then took less off than its parts measure.

**The choice** (built, goal 5 plan step 4). The node agent starts its seeds once.
- **The seed.** Each is `hex __oci-seed`, exec'd once and single-threaded, with what's common to its containers already
  set up (`crates/hex-runtime/src/linux/oci/seed.rs`, `serve_create`).
- **A create.** It hands an idle seed the container's spec and state directory over its socket, with the init's sync and
  map pipes and a network namespace made ahead as `SCM_RIGHTS` descriptors (`hex_core::seed::Create`). The seed clones
  the container's init with `clone3(CLONE_PARENT | CLONE_INTO_CGROUP | CLONE_PIDFD | the namespace flags the spec asks
  for)` (f12), as Hex's exec seed clones already (f13), and passes the init's pidfd back over `SCM_RIGHTS`. The
  `CLONE_PARENT` makes the init the node agent's own child, so the agent reaps it by its pidfd (`waitid(P_PIDFD)`) as it
  reaps a shim-forked init; `exit_signal` is 0, the one value `clone3` takes with `CLONE_PARENT`.
  - **One clone serves root and rootless.** `clone(2)`'s ERRORS (man-pages 6.19) still bars `CLONE_PARENT` with
    `CLONE_NEWPID`, but the kernel dropped that bar before v3.12 (Biederman, "fork: Allow CLONE_PARENT after
    setns(CLONE_NEWPID)", commit `1f7f4dde5c94`), and v6.19's `copy_process` refuses `CLONE_PARENT` only from an init
    (`SIGNAL_UNKILLABLE`, `kernel/fork.c:2010-2012`) and `CLONE_NEWUSER`/`CLONE_NEWPID` only with `CLONE_THREAD`
    (`2018-2022`); `clone3_args_valid` refuses `CLONE_PARENT` with a nonzero `exit_signal` (`2911-2913`). A rootless
    container's init is born unmapped in its new user namespace by the same clone and asks the runtime for its maps over
    the sync pipe, which the agent writes against the pid the seed hands back, as the shim did before it forked; so no
    intermediate fork is needed (shown by the lab's `oci_box` tests, which make every seam container through one such
    clone: rootless on adacachy's Linux 7.2 and as root on fedora's 6.19).
  - **Namespaces by path.** The init joins the network, ipc, uts, mount and cgroup namespaces its spec names by path
    itself, after its user namespace's maps, in the shim's order (`init::join_namespace_paths`). A pid or user namespace
    by path can't be joined that way: a pid namespace's `setns` moves only the caller's later children, and the clone
    has already made the fresh namespaces, owned by the seed's user namespace. So a given bundle that joins either is
    made through the shim, as the command line makes every container, and counted (`Node::creates_through_shim`): the
    one kind of seam create that still re-execs the hex binary. The rendered bundles never join by path; a pod's
    containers sharing a network namespace (`ClassSpec::Bundle`) go through the seed as root, and rootless, where joining
    a namespace another container's user namespace owns needs that user namespace joined too, through the shim.
- **The child.** It does what the shim's child does today (`init::clone_child`), up to the exec FIFO, then execs the
  container's program, so the program's address space is randomized afresh (f17) and any state unique to the container
  is made anew in the child after the clone (f15). Its standard streams are the seed's, the null device, as a `process`
  box's program's and an exec's unset streams are; a seam create made through the shim gives the shim the same, so no
  seam container holds the node agent's streams, as one did when the shim inherited them. The command line's `create`
  still passes its caller's streams on, as runc's does.
- **The shim.** Its re-exec (`hex __oci-init`) leaves the seam's create (`create_in_cgroup_seeded`); the command line
  keeps it (`create`).
- **No `hex` exec per create.** <!-- record: bench/competitive/probes/seed_path.md -->strace of `execve` over 55 seam
  creates on fedora, in each of the probe's three runs: the hex binary is exec'd twice, both `__oci-seed` (the node's
  first seed and its spare, started ahead of the creates), and `__oci-init` (the shim) and `__oci-exec` not at all;
  each create's init execs only its own program (`/usr/bin/sleep`, `/usr/bin/true`), never hex: 0 per
  create.<!-- /record --> The exception is the pid or user namespace by path above, made through the shim and counted.
- **The box count.** A create counts the node's boxes by the box root's link count, its subdirectories plus two on
  cgroup2 (`kernfs_refresh_inode`, Linux v6.19 `fs/kernfs/inode.c`), where it listed the root before:
  <!-- record: bench/competitive/probes/create_serial.md -->at concurrency 32 that listing, under the root's
  `i_rwsem`, was 5.710 ms of each lifecycle's waits (run 0).<!-- /record -->
- **Seeds start ahead of the creates.** A thread of the node's create pool starts each seed with its lock let go and
  keeps one spare up idle beyond the creates waiting (`createpool::SPARE`), the first as soon as the node's hex binary
  is named, so no create's path holds a seed's exec, nor the pool's lock while one starts. A burst past the spare waits
  in line for a seed to free or to start, while the thread adds more, up to the bound.

**Concurrency 32 (plan step 4's fourth criterion: measured, not met).** The criterion asks the sustained rate at
concurrency 32 to be at least 8x the throughput at concurrency 1 in the same run. What holds the rate at concurrency
32 is the kernel's `cgroup_mutex`, one lock for the whole host (`bench/competitive/probes/create_serial.md`):

<!-- record: bench/competitive/probes/create_serial.md -->
- **The measurement.** The OCI pass at ca45e1a on fedora (`20261030T041910Z`, a load of 21.81, not of record) read
  1,942 against 394 lifecycles a second in process, 4.93x, and 1,994 against 316 over the socket, 6.31x; libcrun
  beside it ran 1,593 against 73.
- **The lock is the ceiling.** In run 2 the box root's exclusive holds filled 90% of the run's time, and `cgroup_mkdir`
  and `cgroup_rmdir` inside them, each waiting for and holding the lock, 87%. In run 4 the lock was held 83.4% of the
  time and handed off 6.8% more, so it was free under a tenth of it; the runtime's own takers waited for it 1.789 ms a
  lifecycle, and the kworkers 4.104 ms more.
- **Not the network namespaces.** Without a network namespace of its own the sustained phase ran 1,340/s against
  1,359/s with one, the lock held 83.0% of the time against 83.4% (run 4).
- **Each lifecycle takes the lock 17 times.** The runtime's `mkdir` and `rmdir` of the container's cgroup, its
  `cgroup.kill`, its device program's attach and its clone into the cgroup; and the kernel's work for the removed
  cgroup in kworkers: five csses offlined, six released, and its BPF state released.
- **libcrun contends on the same lock.** Its lifecycle takes it more often, and at its lower rate the lock was held
  74.6% of the time (run 4), while libcrun spends 4.069 ms of each lifecycle in its own polling sleeps (run 1).
- **Hex's own waits are small beside it.** Its futexes took 0.601 ms of a lifecycle at concurrency 32 and its record
  directory 0.671, against 7.756 on the box root.
<!-- /record -->

- **Where the kernel takes the lock** (Linux v6.19). `mkdir`, `rmdir` and `cgroup.kill` take it through
  `cgroup_kn_lock_live` (`kernel/cgroup/cgroup.c:1732`). A clone with `CLONE_INTO_CGROUP` takes it in
  `cgroup_css_set_fork` (`cgroup.c:6694-6695`) and holds it to `cgroup_css_set_put_fork`'s unlock (`6803`), at the end
  of `cgroup_post_fork` (`6968`). The kworkers take it in `css_killed_work_fn` (`6049`), `css_release_work_fn`
  (`5629`) and `cgroup_bpf_release` (`kernel/bpf/cgroup.c:314`), which runs for every removed cgroup, a program
  attached or not. Inside it `cgroup_mkdir` and `cgroup_rmdir` take the hierarchy's `kernfs_rwsem` to write, for each
  interface file they make and once to remove them (`fs/kernfs/dir.c:796`, `1554`), and every lookup in the cgroup
  filesystem takes that to read as it revalidates (`1167`, `1185`). `mkdir` and `rmdir` hold their parent's `i_rwsem`
  across all of it, from `__start_dirop` (`fs/namei.c:2873`). kernfs leaves the dentry its `mkdir` made negative
  (`kernfs_iop_mkdir`, `fs/kernfs/dir.c:1259-1277`) and drops a negative dentry once its directory has changed
  (`1170`), so each lifecycle's two first lookups of its new cgroup go the slow way, under that `i_rwsem`.
- **The ceiling moves with the host's load.** A lock handed off is unusable until its woken waiter runs, and that
  wait grows with the host's other work, so the rate at concurrency 32, and the ratio with it, depend on the load in a
  way the rate at concurrency 1 doesn't.
- **What would raise it: fewer holds of the lock per lifecycle.** A cgroup reused from one box to the next rather than
  made and removed for each would take the `mkdir`, the `rmdir` and the kworkers' 12 holds off it. Fewer controllers
  enabled for a box would each take an offline, a release and their interface files off; one device program attached
  at the box root for boxes that share a policy would take the attaches; a kill that doesn't go through `cgroup.kill`
  would take that one. The attaches went in goal 5's extension, step 1 (the default policy's program on the oci boxes'
  root): <!-- record: bench/competitive/probes/create_serial.md -->a default-policy box's lifecycle takes the lock
  15.996 times for its own cgroup at concurrency 1 and 15.999 at 32, none an attach (run 6), where run 4's took 17;
  host-wide, counting the kernel's work for every cgroup on the host, 16.06 against run 4's 17.05 (run 5).<!-- /record -->
- **What wouldn't.** A pool that makes cgroups ahead and removes them after use takes the lock as often: under a
  sustained rate it moves no hold. Under a burst it can move up to 14 of the 17 out of the burst, if the making comes
  before it and the removal after. Spreading the boxes' cgroups over several parents would part their directories'
  `i_rwsem`s but not the lock: with the lock free under a tenth of the time, about a tenth more at most.
- **So no lever of this step reaches 8x.** The faster a lifecycle runs alone, the further 8x of it sits above what the
  lock admits. The criterion is put to Ada with this measurement; no cgroup change is made for it here.

**The cgroup lock, fewer holds per lifecycle (goal 5 extended, step 6; research `01a190c51f137042a9697949c8fe2c7a`,
f19-f50, f61-f65, f81, f82; `docs/research/runtime/goal5-cgroup-lock.md`).** Two levers take `cgroup_mutex` holds off a
lifecycle without changing what a box may do.
- **A box ended through its init's pidfd** (f45-f50). An `oci` box whose init is the first process of a pid namespace
  its create made (the state's `pid_namespace`, recorded at create) is deleted by `pidfd_send_signal(SIGKILL)` to the
  init, which takes no `cgroup_mutex` (`kernel/signal.c`) and ends the namespace with it (`zap_pid_ns_processes`).
  - The agent reaps the box's exec'd programs, since the init's end waits for them, and then waits for the init's
    pidfd to read as ended.
  - By then every process of the namespace is released (`kernel/pid_namespace.c:263-265`). The init leaves its cgroup
    at its last switch (`kernel/sched/core.c:5156`, `cgroup_task_dead`), so a cgroup that holds nothing else empties
    with no write.
  - A thread still listed in the cgroup's subtree whose process lives and isn't the init's is outside the namespace
    (f47): the delete falls back to `cgroup.kill` at once, on that fact rather than a timeout, counted.
  - `process` boxes, inits without a pid namespace of their own, and boxes taken back after a restart (whose init the
    agent no longer holds) keep `cgroup.kill` (f49).
  - Every `cgroup.kill` here is the box's kill as the delete flake's fix (5013c4b, T-T7.1.2) left it, which this step
    is built on: `cgroup.kill`, then, while the box is still populated, a bounded sweep of its subtree's processes,
    each sent a process-directed SIGKILL through a process pidfd once a re-read shows it still in the box. That
    reaches a thread group whose leader has exited, which `cgroup.kill` misses
    (`bench/competitive/probes/delete_flake.md`).
- **A cgroup handed from box to box of one owner** (f20, f23, f27-f39, f61-f65, f81, f82). A box whose spec names an
  owner (`BoxSpec::owner`, append-only on the record and on the wire, whose version 3 carries it) is given the owner's
  idle cgroup when one reads clean. That takes off its `mkdir`, its `rmdir` and the kworkers' 12 holds (f62). The
  owner's boxes reuse by default; an owner opts out by `reuse: false` (Ada's call, 2026-10-31). The pool is a pure core,
  `hex_core::reuse`, with the node's shell in `crates/hex-node/src/cgroups.rs`.
  - *Naming.* Such a cgroup is named by its slot, `.oci/.<n>`, since a cgroup can't be renamed (f35), and the box's
    record names the slot, with the counters the cgroup had when the box was given it (f64).
  - *Numbering.* Slots are numbered once each, from past every slot already under the root, so a slot being removed is
    never made again under its name.
  - *Choosing.* The runtime asks for the cgroup once it has read the spec (`run::Choose`): the owner's idle cgroups are
    keyed by the device policy whose program they carry, so one handed on under the same policy has its program
    attached already and the create attaches nothing.
  - *The check.* A cgroup is handed on only after reads that take no lock (f61; Linux v6.19 `cgroup_events_show`,
    `cgroup_stat_show` under RCU, `cgroup_subtree_control_show`, `cgroup_type_show`, `cgroup_freeze_show`,
    `memory_oom_group_show`, `memory_current_read`). They must show:
    - `populated 0` and not frozen;
    - no live or dying sub-cgroup;
    - an empty `subtree_control`, and `cgroup.type` `domain`;
    - `cgroup.freeze` 0, and `memory.oom.group` 0, as Hex leaves it;
    - `memory.current` at most `RESIDUAL`, which is under the next box's `memory.max`.
  - *Every bound written again.* Bounds the next box doesn't set are written at the kernel's defaults
    (`hex_core::cgroup::writes_all`), and none of those writes takes the lock (f25). `cpu.max` alone is read first
    and written only where its values differ, compared as values, never text (`hex_core::cgroup::CpuMax`); a read
    or a parse that fails writes it (the lead's ruling, 2026-11-03T01:19Z, which departs from the plan's "every
    bound is written again" while keeping its effect). Its write takes the host-wide `cfs_constraints_mutex` and
    walks every task group on the host (`kernel/sched/core.c:9461-9462`, `:9624`; f26), so its cost grows with them.
    <!-- record: bench/competitive/probes/cpu_max_write.md -->On fedora's 279 cgroups it took 6.6 us p50 against 2.5
    for `pids.max`, and the read 2.8 (run 1, 2,000 each); an earlier reading on adacachy's 3,348 took 68.5 us
    against 1.5.<!-- /record --> Its read takes no lock (`cpu_max_show`, `:10104`), so the bound is in force either
    way.
  - *Usage.* It is reported as deltas from a baseline read when the box is given the cgroup, and the peak through a
    `memory.peak` descriptor written then (Linux 6.12 on, f31). No per-box `pids.peak` is reported (f32), and
    `memory.reclaim` never runs (f29). The agent holds that descriptor for each box given a cgroup another box left,
    one each for the box's life, and every descriptor the agent holds rides along each exec's copy of its descriptor
    table (decision 3): a cost that grows with those boxes, which a fresh slot's box doesn't add, since its peak is
    its cgroup's own.
  - *Who never shares.* A box with no owner, an owner that opts out, or a box that could change its own cgroup gets a
    cgroup of its own, named by its id and removed with it, as every box did before. Such a box holds CAP_SYS_ADMIN in
    any set, or its spec gives no capability sets (it keeps its creator's, all of root's), or it holds CAP_BPF with
    CAP_NET_ADMIN, which load a cgroup device program (`kernel/bpf/syscall.c:2930-2941`, v6.19) and attach it to the
    cgroup it can open. The plan named CAP_SYS_ADMIN; the other two reach the same files.
  - *Bounds.* Idle cgroups are bounded at `PER_OWNER` 32 for an owner and `PER_NODE` 256 for the node; past either the
    oldest is evicted. The bench's busiest phase runs 32 boxes at once, and RunD measured the load-tracking walk over
    cgroups at 7.6% of a node's cycles past 10,000 (f65).
  - *Removal.* A thread of the node's removes what the bounds, a failed check or a failed create put out, off create's
    path. It ends anything left in the cgroup, then takes its sub-cgroups apart deepest first; with its queue full, the
    caller removes the cgroup itself.
  - *Ordering and recovery.* A delete gives the slot back only once the box's record is gone, so a crash never leaves a
    slot to two boxes. Recovery removes every slot no readable record names, and takes live boxes back by their
    recorded slot (T-T7.1.2).
  - *Fallback.* Any failed check removes the idle cgroup and makes a fresh one, so the worst case is the path every box
    took before.
  - *Never a cgroup `cgroup.kill` was written to.* Linux kills every task later cloned into such a cgroup with
    `CLONE_INTO_CGROUP`. The clone's kill count is sampled from the forking task's own cgroup, and the child is killed
    when the target's differs (`kernel/cgroup/cgroup.c:6702-6705`, `:6768`, `:6902`, `:6941`, v6.19). Measured on
    adacachy's 7.2.9 and fedora's 6.19.10 (`bench/competitive/probes/kill_seq.md`), and seen as inits dying before
    their ready signal in fedora's first OCI pass with an owner. No file a check can read shows the count, so the
    node keeps it: a box sent KILL, or whose delete fell back to `cgroup.kill`, has its slot removed with it,
    counted (`discarded_killed`). Upstream fixed it in 8e3599202166 (7.3-rc1, `Cc: stable`; in linux-7.2.y by
    7.2.15, not in linux-6.19.y by 6.19.14), and the guard stays on every kernel, since Hex runs on kernels with
    and without it.
- **A network namespace handed from box to box of one owner** (Ada's call, 2026-11-03; research
  `docs/research/runtime/goal5-netns-reuse.md`). Once the cgroup and mount locks are off a lifecycle, what holds
  concurrency 32 is the making of each box's network namespace. Its loopback is registered under the host's one RTNL,
  its sysctl tables under the host's one `sysctl_lock` inside that hold, and the loopback is brought up under RTNL
  again (`bench/competitive/probes/create_serial.md`, run 10). Decision 4's pool makes them ahead, which moves that
  off the create but not off the host. So a box that may share its owner's namespaces is given one its owner's earlier
  box left, and that making, bringing up and teardown leave its lifecycle. The pure core is `hex_core::netns_reuse`,
  with the node's shell in `crates/hex-node/src/netns.rs` and the check in `crates/hex-runtime/src/linux/oci/netcheck.rs`.
  - *Who may share* (`Spec::may_share_its_network`):
    - its spec gives capability sets naming neither CAP_NET_ADMIN nor CAP_SYS_ADMIN, without which it can change
      none of its namespace's links, addresses, routes, rules or firewall tables;
    - `/proc/sys` is read-only, or no proc is mounted, so it can write none of the namespace's sysctls;
    - it has no user namespace of its own;
    - its owner keeps reuse on.
    Any other box's namespace is its own, never kept, counted by why.
  - *Kept.* The node holds each such box's namespace from its create to its delete: one from decision 4's pool, or,
    with the pool dry, one the node makes on the path as the pool would, so that it holds it.
  - *The check* (`netcheck::check`) runs when the box and its record are gone, on the node's checker thread, which
    enters the namespace with `setns` and goes back after. It keeps one netlink socket of its own in the namespace,
    made at its first check and closed with it, for its dumps. It requires:
    - no socket but its own: `sockstat`'s "sockets: used", every socket from its making to its freeing (Linux v6.19
      net/core/sock.c:2316-2319, :2417-2418), at 1, the check's own alone; and past 1 each protocol's sockets in use
      (`/proc/net/protocols`) at most a fresh namespace's but for the check's own. A unix, netlink or packet socket
      counts there from its making, a UDP one once bound, a TCP one once listening or connected and until its
      connection has ended. The count alone can't decide past 1: a closed netlink socket stays in it until an RCU
      grace period has passed (net/netlink/af_netlink.c:794-796), so a box's last reads would read as sockets left,
      the more so under load (the first root lab's TIME_WAIT case found it, `hex-g5x-s6-netlab-20261103T054433Z`),
      and for the same reason the check's own socket is kept, never closed between checks. A box that may share its
      namespace can't write to that socket: NETLINK_ROUTE takes a message to another socket only from one with
      CAP_NET_ADMIN over the namespace's owner (net/core/rtnetlink.c:7038; net/netlink/af_netlink.c:1842-1847,
      :904-908);
    - no TCP connection in TIME_WAIT, IPv4's or IPv6's (`sockstat`'s "tw");
    - loopback the only link (`/proc/net/dev`);
    - no iptables table;
    - addresses, routes and rules equal to a fresh namespace's, routes' exceptions with them, each family's dump
      read by itself, since none of those takes RTNL where an `AF_UNSPEC` one does. The fresh one's are read once
      its `::1` is no longer tentative, which its DAD work clears once it gets RTNL.
    Nothing runs in an idle namespace, so what the check saw holds until it's handed on. A namespace that fails is
    closed, counted by why; one given back with the checker's line full is closed unchecked, counted.
  - *Bounds.* At most `PER_OWNER` 32 idle for one owner and `PER_NODE` 256 for the node, as the cgroups', the oldest
    closed past either. An owner's oldest is handed on first, so a connection a box left in TIME_WAIT has had the
    longest to end.
  - *What can't be reset stays within the owner.* The namespace's counters run on, its identity and its hash seed stay
    the same, so the next box can read its predecessor's datagram counts and tell it was given its namespace (both
    tested). A connection in TIME_WAIT can't be reset, so it refuses the namespace (tested).
  - *The TCP metrics cache crosses within the owner, no guard* (the lead's approval, 2026-11-03T06:12Z). Each
    destination's last round-trip time and window are saved per namespace as a connection ends (Linux v6.19
    `tcp_update_metrics`, net/ipv4/tcp_metrics.c:340-352; entries keyed by namespace, `:54`, `:242`, `:284`), read
    by the next connection to it (`tcp_init_metrics`, `:464`), and kept an hour (`:143`). Flushing them needs
    CAP_NET_ADMIN over the host, not just the namespace (`TCP_METRICS_CMD_DEL` is `GENL_ADMIN_PERM`, `:979-984`).
    In a namespace with loopback alone, a box's connections are to itself, so what its owner's next box can read is
    the round-trip times of the predecessor's loopback connections whose TIME_WAIT had ended before it did; one
    still in TIME_WAIT refuses the namespace. The namespace's own `net.ipv4.tcp_no_metrics_save` (`:352`) would stop
    the saving; no box that may share can set it, `/proc/sys` being read-only.
  - *A socket never hashed crosses within the owner, unseen* (the lead's approval, 2026-11-03T06:40Z). An inet socket
    enters its protocol's count only once hashed: a UDP one at its bind (Linux v6.19 net/ipv4/udp.c:333), a TCP one
    at its listen or connect (net/ipv4/inet_hashtables.c:747, :811). So a TCP or UDP socket never bound, or a TCP one
    bound but neither listening nor connected, counts in no protocol: `sockstat`'s count has it, which sends the
    check to the protocols, and there it isn't, so the check passes it. Refusing every count past the check's own
    would refuse each box whose last netlink reads are still waiting out their grace period too. The box's own
    processes are gone when it's checked, so only a process outside the box can still hold one, and through it bind
    or connect in the namespace once the owner's next box has it. That stays within one owner because no namespace is
    handed to another owner's box: the core's model test checks it after every step, and the root lab gives three
    boxes of another owner, beside an idle namespace of the first's, fresh ones.
  - *The check's own socket stays with its namespace, within the owner* (the lead's go, with its conditions,
    2026-11-03T17:36Z). The check keeps one NETLINK_ROUTE socket in each namespace it checks, made at its first check
    and kept with the namespace, idle, handed on and held for the owner's next box, and closed with it, put out by the
    bounds, refused by a check or let go with the node. So a box given a checked namespace sees one more socket in its
    `/proc/net/netlink` than a fresh namespace has, a route socket with a portid, the node's (Linux v6.19
    net/netlink/af_netlink.c:2703), and one more in `sockstat`'s count. It can't send to it: NETLINK_ROUTE lets any
    socket receive but none send to another socket (`NL_CFG_F_NONROOT_RECV` without `NL_CFG_F_NONROOT_SEND`,
    net/core/rtnetlink.c:7038) unless it holds CAP_NET_ADMIN over the namespace's owner
    (net/netlink/af_netlink.c:1844-1846, :904-908; a connect to it likewise, :1083), and a box that may share holds
    none. Nor can it read it: a socket's queue is read through its descriptor, which the node alone holds. `sockstat`
    counts it from its making to its freeing (net/core/sock.c:2316-2319, :2417-2418), which is why it's kept rather than
    made and closed at each check, and it stays within one owner, as its namespace does. Each kept namespace holds at
    most one, so the sockets are within the namespaces' bounds: `PER_NODE` idle, `PER_NODE` on the checkers' line and
    one for each live box. Tested: a box's send to it refused with EPERM, its send to the kernel gone, the socket's
    queue and drops unchanged and the namespace kept again at its next check (root lab); one the bound puts out closed
    with its namespace. *What would falsify it*: a box that may share reaching the socket, so that a check could read
    its message as a dump's answer; or the socket outliving its namespace.
  - *A kept namespace's `/proc/net` files stay open with it, so a later check needn't enter it* (the lead's go, with its
    four conditions, 2026-11-03). From a namespace's first clean check on, its `sockstat`, `dev`, `ip_tables_names` and
    `ip6_tables_names` are held with it, open. Such a file stays the namespace's wherever its holder stands, having taken
    the namespace as it opened (`seq_open_net`, `single_open_net`, Linux v6.19 fs/proc/proc_net.c:39-50, :183), a read
    from its start reads it anew (`seq_read_iter`, fs/seq_file.c:185-188), and the kept socket answers for its own
    namespace, so a later check makes no `setns`, `openat` or `close`: those were 58 us of a check's 149 on-CPU at
    concurrency 1 and 44 of its 83 at 32 (`create-serial-checker-20261103T193211Z`). A socket past the check's own
    enters to read the protocols, as before. An iptables file is absent while its module isn't loaded (fedora's kernel has
    `ip_tables_names` and no `ip6_tables_names`), and comes to be in every namespace when it loads
    (`register_pernet_subsys`, net/ipv4/netfilter/ip_tables.c:1838-1857, net/ipv6/netfilter/ip6_tables.c:1845-1864), so a
    namespace holds those there are, a check looks by name where it stands for one it doesn't hold, and one that has come
    to be sends it in, which opens it. A box can't reach them: they are the agent's descriptors, close-on-exec, so no program inherits them;
    its `/proc/<pid>/fd` shows them only to a process with ptrace access to it (kernel/ptrace.c:276), which a box
    short of the agent's capabilities hasn't (security/commoncap.c:164-182), where a box sees the agent's pid at all; and
    what they read is that one namespace's counts, its owner's.
    The conditions: at most four such files a kept namespace, besides its own descriptor and the socket's. The
    descriptor limit stays as the node was given it, never raised: a box without an rlimit of its own sees the limit it
    would without the files (the lead, 2026-11-03T23:07Z). As the node starts it works out how many namespaces may hold
    them, what the soft `RLIMIT_NOFILE` leaves past the node's own descriptors at its bounds (the kept namespaces', each
    live box's, its connections' and programs', the seeds'), four a namespace (`FilesBound`), and says so on the agent's
    stderr with both numbers. A namespace past that bound holds none and each of its checks enters, counted
    (`files_bounded`); a limit too small for the node's own work shows where it runs out, as that call's typed refusal.
    At systemd's default soft limit of 1024 the bound is 0, so where nodes run today no namespace holds its files and
    the saving can't appear: what lets it is the node's soft limit, past its own descriptors at its bounds (about 330
    thousand), which the installer sets. Every bench record says which bound its node ran with and how many checks went
    through the files and how many through the protocols (`interleaved_pool.py`). They close with the
    namespace on every path, tested by counting them by their links: refused by way of the files or of entering,
    released, put out by the owner's bound, with the node's end while a box's namespace holds them; a node recovered
    holds none; and all the process's descriptors are back where they began (root lab). A failed read drops them and
    looks by entering, counted (`files_failed`, as `through_files` counts the checks they answered), and refuses nothing
    on its own.
  - *Another owner's box* gets a fresh namespace, counted `Owner`, and the idle one stays its owner's, as the cgroup's
    does (Ada's call). The counts the node keeps (`ReuseCounts`: the core's, and how many namespaces were given back,
    checked and closed unchecked, and how many checks read the protocols) are internal; no `hex_*` metric names them
    until Observe agrees the names (CLAUDE.md §10).
  - *What would falsify it*:
    - concurrency 32 not rising once the namespace's making is off the lifecycle;
    - a namespace passing the check while it holds what the check reads;
    - the check's own cost at concurrency 32 rivalling the making's;
    - a namespace reaching another owner's box, which would carry what crosses within an owner, the TCP metrics and a
      socket never hashed, to another;
    - eligible boxes being rare, because specs grant CAP_NET_ADMIN or keep `/proc/sys` writable.
- **A box's cgroup files opened through its directory, held open** (3ea3ffa).
  <!-- record: bench/competitive/probes/create_serial.md -->Once the namespace was reused, the largest wait at
  concurrency 32 was the node's, to read a kernfs root's lock on a path walk: 2.649 ms of each lifecycle (run 12),
  3.082 ms in locks mode and 0.083 with no network namespace at all (run 13).<!-- /record --> The runtime opened each
  cgroup file by its whole path. Each walk revalidates every kernfs dentry it passes under its root's lock (Linux v6.19
  fs/kernfs/dir.c:1145-1146, :1185), `/sys`'s among them, which every network device made or removed takes to write.
  - The box root holds its directory open (`BoxRoot`), and a slot's or a box's cgroup is opened by its name relative to
    it and holds its own (`BoxCgroup`).
  - Each file is then opened by its name relative to that directory.
  - The create's lookups of a cgroup named by the box's id stay, as one name each relative to the root: without them
    a failed slotted create's scrub would reach an id-named cgroup left outside the records (the lead, 2026-11-03).
  - A test moves every directory above a slot away and reads and writes its files through the slot. On the code
    before, it fails at its first read.
  <!-- record: bench/competitive/probes/reuse_pass.md -->With it, reuse on kept 10.17 of 12 CPUs busy at concurrency
  32 and ran 5.76x, against 3.74x (run 7, the base moved from 148d12e to 16e52a9 with it).<!-- /record -->
- **Four checkers** (893ea05). <!-- record: bench/competitive/probes/reuse_pass.md -->One checker took 127.2 and
  130.8 us a namespace, so it checked at most about 7,700 a second while concurrency 32 gave back 8,364, and 12% of the
  boxes were given a fresh namespace (run 7).<!-- /record --> Each such namespace was made and torn down under RTNL.
  - `CHECKERS` threads take from the one line, which stays bounded at `PER_NODE`, a namespace given back with it full
    closed unchecked, counted.
  - Each holds its own `/proc/thread-self/net` open and opens each file of the checked namespace by its name after
    the `setns`. A name looked up there names the file of the namespace the thread is in (fs/proc/proc_net.c:268-301),
    looked up afresh at each open (:123, :217). So no check walks `/proc`, `thread-self` and the thread's directory
    five times.
  - The check's CPU runs beside concurrency 1's lifecycle but competes with concurrency 32's: the work off concurrency
    1's critical path that caps concurrency 32 (the lead, 2026-11-03).
- **The checks' CPU** (8753abe, 4c54e58, ad6f333). <!-- record: bench/competitive/probes/create_serial.md -->On the
  E-cores a check cost 151.2 and 157.1 us of CPU at concurrency 1; with the buffers each checker holds from one check
  to the next and reads straight into, 138.7 and 129.5 (run 15). The dumps' answers then took 13.7 and 14.5 us a check
  to allocate, the kernel sizing each by the socket's largest read, 32 KiB; read 8 KiB at a time, 7.2 and 7.0 (run
  16). With the namespace's own socket kept and sockstat's count read first, 141.4 and 141.2 us fell to 94.2 and 97.0,
  the protocols never opened (run 18).<!-- /record --> What the check decides is unchanged.
- **`RESIDUAL`, 16 MiB.** What a box whose processes have all ended leaves charged is the charge cached per CPU and some
  kernel memory (f30). <!-- record: bench/competitive/probes/reuse_residual.md -->The bench's lifecycle left at most
  1,748,992 bytes in 256 idle cgroups of fedora's OCI pass 20261103T044133Z. Over six runs of test 2 on adacachy
  (Linux 7.2.9), a box that burned CPU, held 64 MiB and had a program OOM-killed left 3,719,168 to 7,471,104 bytes
  charged, 3.5 to 7.1 MiB, anon, shmem and dirty file pages 0 in each; on fedora (Linux 6.19.10, as root)
  5,054,464.<!-- /record --> A box that leaves files in memory leaves tens of megabytes (f27: 85 MB, and test 3's 64
  MiB file), which the next box would be charged for. The threshold sits above both clean cases, 2.2 times test 2's
  most, and under the third.
- **Test 3's other-owner case: kept, Ada's call (2026-11-03).** A box of another owner never gets the idle cgroup: it
  gets a fresh one, counted (`Refused::Owner`). The idle cgroup stays its owner's, waiting for that owner's next box,
  and leaves only under the per-owner and per-node bounds. The plan's test read "A's is removed". Removing a clean idle
  cgroup because another owner's box can't have it would add the very holds this lever takes off. Test 3 asserts the
  keeping and the hand-on, and `hex_core::reuse`'s model test's oracle encodes it.
- **Tests.** `hex_core::reuse`'s model test against an oracle covers:
  - random creates, deletes, owners, policies, privileges and failing checks;
  - the bounds, and a cgroup handed only to its owner under its policy when clean;
  - never a fresh slot while a usable one is idle, and no slot made twice;
  - a 20,000-step run that counts every path: handed on, fresh, missed, each refusal and each eviction.

  `crates/hex-cli/tests/oci_reuse.rs` runs the plan's first three tests on a node:
  - the pidfd end with an inotify watch on the cgroup's directory, which sees no `cgroup.kill` write, and the same for
    a box whose init ended before its delete;
  - the outsider's fallback, and the `process` box's `cgroup.kill`;
  - B given A's slot, its CPU, peak and event deltas its own, while the cgroup's files still hold A's;
  - each guard, counted by its reason, and a box sent KILL, whose cgroup is removed rather than kept.

  `crates/hex-cli/tests/node_socket.rs` crashes a real agent with an idle slot and live boxes, and its restart removes
  the slot and takes the boxes back. `crates/hex-cli/tests/oci_cgroupfs_box.rs` probes a rootless box holding
  CAP_SYS_ADMIN with namespaces of its own: unmounting its cgroup filesystem and mounting a fresh one, it reaches no
  other box's cgroup.
- **Measured** (fedora, 2026-11-03, a loaded host: each figure a count or a ratio within its own run):
  - *The holds* (the first criterion, at most 3: met). <!-- record: bench/competitive/probes/create_serial.md -->One
    owner's box takes `cgroup_mutex` 1.087 times a lifecycle at concurrency 32, 1.023 at concurrency 1 and 1.092
    without a network namespace: its init's clone into the cgroup it was handed. The lock is held 12.4% of the time. A
    box of no owner takes it 15.000 times, where run 6's took 15.999, its delete now through its init's pidfd, and
    holds it 87.1% of the time (runs 7 and 8).<!-- /record -->
  - *The rate* (the second, at least 8x, a minimum: not met). The bar is 8x on 4-15 with interleaved pairs the
    method of record, since they control the load's drift that makes the criterion's own phases swing (the lead,
    2026-11-03T17:47Z). Option 1, each pair's concurrency 1 on 4-15 and its 32 on all of 4-31, is measured beside it
    for Ada's choice of the bar, and neither is called met until she makes it.
    <!-- record: bench/competitive/probes/reuse_pass.md -->The merge of steps 2, 3 and 6 reads 4.78x in process and
    4.92x over the socket on 4-15, and 7.55x and 7.16x as option 1, every task of each half on its CPUs as read back
    (run 12). Step 6 alone read 1.84x and 1.59x in that hour, under about 7,500 host threads (runs 12,
    13).<!-- /record --> <!-- record: bench/competitive/probes/create_serial.md -->A third of its lone lifecycle's CPU
    was pivot_root's walk of the host's threads, under the host's mount lock (run 22).<!-- /record --> Step 6 alone
    keeps the classic root, made with pivot_root, so its figures depend on the host's thread count; the merge's
    pruned root, step 2's, makes none, and its figures don't.
    <!-- record: bench/competitive/probes/reuse_pass.md -->As the criterion's text reads it,
    on host.sh's own CPUs, 4-31, flattered there since a lone lifecycle runs on the E-cores too: the merge 25.12x in
    process (its rounds 24.88x to 26.48x) and 23.55x over the socket, step 6 alone 8.29x (7.93x to 9.32x) and 8.01x;
    on 4-15 the merge 7.74x and 7.80x, step 6 alone 6.44x and 5.95x. Interleaved, on 4-31 the merge 18.96x and step
    6 alone 5.89x; on 4-15 5.11x and 3.09x (run 10).<!-- /record -->
    <!-- record: bench/competitive/probes/reuse_pass.md
    -->Sustained at concurrency 32 over throughput at concurrency 1 in one OCI pass, one owner's boxes: 4.32x and
    4.21x in process, 4.41x and 4.81x over the socket (run 1, 4-31, as host.sh runs it). With steps 2 and 3 merged in,
    an experiment of record for no branch, 4.05x to 4.83x, against 4.62x to 5.41x for this step alone beside it (run
    2, 4-31). On CPUs 4-15, the P-core threads: 2.89x to 3.32x at f7c4065 (run 3) and 2.47x to 2.86x at 610bba9
    (run 4). The merge on 4-15 sustained 4,407 to 4,784 a second without network namespaces and 902 to 1,136 with
    them, 85.9 to 87.5% of whose creates made theirs on the path (run 5).<!-- /record --> Each lever raised both rates,
    the rate at concurrency 1 as much as the other, and that rate moves most with the host's load (run 5's was a
    fifth to a half of run 4's), so the ratio is now measured with the two interleaved in each round (the lead, from
    Ada, 2026-11-03T06:01Z).
    <!-- record: bench/competitive/probes/create_serial.md -->Once the cgroup lock is free, what holds concurrency 32 is
    the mount namespace's lock, one for the whole host, 5.349 ms a lifecycle (run 9); with steps 2 and 3 in too, RTNL
    in making each box's network namespace, 6.670 ms, and sysctl's lock, 12.58% of the samples spinning (run
    10).<!-- /record --> So the second falsifier below tripped: the holds fell and 8x didn't follow. The next
    host-wide locks are taken in making a network namespace a box, which a pool that makes them ahead moves off the
    create but not off the host: hence the namespace handed from box to box of one owner, below.
    <!-- record: bench/competitive/probes/reuse_pass.md -->Interleaved on 4-15, the ratio pooled over adjacent pairs:
    3.74x with the namespace handed on, on steps 2 and 3 at 148d12e (run 6); 5.76x with the cgroup files opened
    through their directory, on 16e52a9 (run 7); 4.87x with four checkers, concurrency 32 at 8,914 a second where run
    7's was 8,364, but concurrency 1 at 1,832 on a quieter host where run 7's was 1,453 (run 8). With the checks'
    three changes (the checks' CPU, above), 6.17x in process against 5.53x without them in the same A B B A, and
    concurrency 32 at 10,531 a second against 9,808; over the socket 7.19x against 7.14x (run 9). A lifecycle's CPU
    at concurrency 32 is 1.36 to 1.44 times its CPU at 1 there, where run 6's pass without a network namespace, at
    load 37.4 to 39.0, gave 1.93, so run 8's ceiling of about 6.6 times concurrency 1 was its load's and is withdrawn.
    On run 9's figures, all of concurrency 1's work beside its own path gone puts concurrency 32 at about 7.2 times
    concurrency 1, and its CPUs kept busy as well, 10.2 of 12 now, at about 8.4 times (run 9).<!-- /record -->
    <!-- record: bench/competitive/probes/create_serial.md -->That work is 272 to 299 us a lifecycle on the E-cores, 17%
    of concurrency 1's CPU: the checkers 162 to 191, step 3's pool of each box's `/dev` 97 to 101, the seeds' starter
    and the namespace refill 9 to 11 (run 17).<!-- /record --> The bar, 8x on 4-15 or option 1, is Ada's call.
  - *Reuse off* (the third: met, Ada's call, 2026-11-03T06:01Z: faster than before passes).
    <!-- record: bench/competitive/probes/create_breakdown.md -->On boxes of no owner, this step's create p50 against
    the binary before it in the same rounds: over 100 rounds in five runs on 4-15, a mean paired difference of
    -0.083 ms, standard error 0.028 by a bootstrap over the rounds, 74 rounds under, 10 within 0.02 ms and 16 over
    (run 14). The window's precision, a standard error of 0.01 ms, would take about 800 rounds; at the precision
    reached the step is faster, not slower. Run 13's three runs on 4-31 read the same: 0.064 and 0.074 ms under, 0.028
    over.<!-- /record -->
  - *The bounds* (the fourth). The model test checks after every step that the pool keeps both bounds and hands a
    cgroup only to its owner, and test 3 gives another owner's box a fresh cgroup, counted. Every phase of the bench
    ended with 32 idle cgroups, `PER_OWNER`, removed when the bench ended.
- **What would falsify it:**
  - `create_serial.sh` locks mode counting more than 3 holds per lifecycle on one owner's boxes at concurrency 32 (the
    plan's bar), or more than today's on unowned ones;
  - the sustained rate at concurrency 32 not rising past 8x concurrency 1 once the holds fall;
  - a box handed a cgroup the check called clean that wasn't;
  - the reuse rate on one owner's boxes near 0 because what's left charged sits over `RESIDUAL`;
  - a delete that ends with the cgroup populated, or a pidfd end that took the lock.

**The seeds a node keeps (plan step 3).** The node's policy for its seeds is a pure core, `hex_core::seeds`
(`crates/hex-core/src/seeds.rs`, 5f0b474), which step 4's shell drives (`crates/hex-node/src/createpool.rs`).
- **The bound.** At most **32** seeds, up or starting, one to a slot. A seed serves one create at a time: "only one
  instance can be cloned from it at a time (serially)" (AFaaS, OSDI 2025, §8; f14). The bench's busiest phase has 32
  creates in flight (concurrency 32, the figure decision 4's pool holds), so with 32 seeds no create of that phase
  waits for a seed that is busy. The shell starts seeds as creates wait for them rather than all 32 at once (one spare
  beyond the creates waiting, above), so a create can wait while one starts. At concurrency 1 one seed serves every
  create, its clone a seventh of a create (0.209 ms p50 against the pass's 1.50 ms create p50 in process,
  `20261030T041910Z`). At concurrency 32 clones into a cgroup queue on `cgroup_mutex` (above), so the creates in
  flight hold their seeds longer and how many are busy follows that queue:
  <!-- record: bench/competitive/probes/seed_path.md -->a sustained phase on a loaded fedora started all 32 seeds and
  had all 32 busy at once, 45 of its 10,000 creates waiting and none refused (run 2); under a lighter load 21 started
  and 20 at most were busy (run 3).<!-- /record --> Those busy seeds are the clones' queue on the lock, not a need for
  32: fewer would move the queue into the pool's line without changing the rate. The core counts the most seeds busy
  at once, so a run shows how many it used.
  - **Their memory.** <!-- record: bench/competitive/probes/seed_path.md -->A seed holds 1.5 to 2.4 MiB `VmRSS`, the
    busier seed the larger (run 2: 58 samples of a node's two seeds while it served 400 creates after 20 on fedora;
    `Pss` 0.7 to 1.7 MiB, the two sharing the binary's pages), so the bound caps the seeds at about 78 MiB, counted as
    if they shared nothing.<!-- /record --> They run in the node agent's cgroup, so their memory is charged to the
    node's own budget there, and a density reading that counts the agent's cgroup with its boxes' counts them, as
    decision 4's pooled namespaces are counted.
- **Handing out seeds.** A create is handed an idle seed that is up, never one starting, busy or gone. With none idle it
  waits in line, and each seed that frees goes to the create that has waited longest.
- **Waiting.** At most 128 creates wait: the socket's 128 workers, so no create over the socket is refused for room.
  Past that, a create is refused by type at once, and one handed no seed within 1 s is refused by type.
  - A seed frees once it has cloned: 0.209 ms at p50 at concurrency 1 (step 4's measurement), a pace at which a full
    line of 128 behind a single seed clears in about 27 ms. At concurrency 32 on a loaded host a clone's fork into the
    cgroup alone averaged 1.014 ms (`create_serial.md`, run 3), a pace at which the same line takes about 130 ms.
  - So a create still waiting after a second has found the seeds stuck, and its refusal lets the scheduler place it
    elsewhere.
- **A dead or hung seed.** One reported dead, or busy on one create for 30 s (decision 3's bound on a seed's answer,
  far past a clone), leaves its slot, and its create is refused by type. The slot is started again and the restart
  counted.
  - Each seed carries a number no other has, so a late report about one that left is refused, never taken for the
    slot's next seed.
  - A start that fails, or a seed that dies before it serves a create, stops the starting until the next create. A node
    whose seeds can't start tries each slot once a create, never in a loop.
- **What the bench reads.** Creates asked, handed, waited for, done, and refused for a full line, a wait past its bound,
  a seed's death or a seed's hang. Seeds started, restarted, failed to start, dead and hung. The most busy at once, and
  the creates each slot's seeds were handed.
- **Tests** (HEX-SEEDS-1 to 6). A proptest runs against an oracle under random starts, failed starts, creates, claims,
  completions, deaths, hangs and the clock moving on. After every event it checks the core against the oracle and the
  shell's own record of its seeds, with a non-vacuity counter on each of 23 paths. It checks that:
  - the bound is never passed (HEX-SEEDS-1);
  - no create is handed a seed starting, busy or gone, and no seed serves two (HEX-SEEDS-2);
  - creates wait in the order they came, and are refused past the line or the wait bound (HEX-SEEDS-3);
  - a death or a hang refuses its create, the slot is started again, and late reports are refused (HEX-SEEDS-4);
  - a failed start, or a seed dead before serving, stops the starting until the next create (HEX-SEEDS-5);
  - the counts equal the oracle's and keep their identities (HEX-SEEDS-6).

  Unit tests take the plan's two cases (every seed busy past the wait bound; a seed dying mid-create), and a hang, the
  line's order, a failed start and a late report.
- **What would falsify the bound.** As first written: a sustained phase at concurrency 32 in which creates waited
  for a seed. That tripped (45 of 10,000 waited, above). With the seeds started as creates ask for them, a create waits
  while a seed starts, which that falsifier didn't foresee, and the count can't tell those waits from waits for a busy
  seed. So it is restated as one a run can trip: with more creates in flight than seeds (concurrency 64 over the
  socket's 128 workers, against bounds of 16, 32 and 64), creates refused for a wait past its bound with no seed hung,
  or the sustained rate rising with the bound. Not yet run.

**Sources.** `docs/research/runtime/seed-per-node.md`: the code a create runs today, where runc and crun keep state,
clone(2), AFaaS, Firecracker and SOCK on seeds, unique state and cgroup pools, and pivot_root's walk. Kiln research
`01a1877e05cc73b495447bf5113ebda3`, f1-f17. `bench/competitive/probes/create_breakdown.md`, run 1;
`bench/competitive/probes/seed_path.md` (a seed's clone, the execs, a seed's memory, the seeds' counts) and
`create_serial.md` (where creates wait under concurrency). Linux v6.19 `kernel/fork.c` (`copy_process`,
`clone3_args_valid`) and commit `1f7f4dde5c94`; Linux v6.19 `kernel/cgroup/cgroup.c` (`cgroup_kn_lock_live`,
`cgroup_css_set_fork`, `css_killed_work_fn`, `css_release_work_fn`), `fs/namei.c` (`__start_dirop`), `fs/kernfs/dir.c`
(`kernfs_iop_mkdir`, `kernfs_dop_revalidate`) and `fs/kernfs/inode.c` (`kernfs_refresh_inode`).

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
4. **Network namespaces off the path** (decision 4): done when Hex's create through the seam with a network
   namespace is at most 5% above its create without one, at p50, on fedora, jupiter and the Mac Studio, and the
   sustained phase reports how often the pool ran dry.
5. **The seed** (decision 5): done when create p50 through the seam is at most a quarter of libcrun's in-process
   create p50, and create+start likewise.
6. **Mount namespaces without copying** (decision 6): done when create on a 7.x host is measured with and without
   `OPEN_TREE_NAMESPACE`, and the faster path is the default.
7. **The process class through the seam** (decision 7): done when run-of-`true` p50 is at most a quarter of a systemd
   scope's, and held memory per idle box is at most a scope's.
