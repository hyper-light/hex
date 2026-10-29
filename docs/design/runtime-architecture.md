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
- **Nothing per container but the container.** With the shim gone, two things were still kept per idle
  container that crun doesn't keep (Evidence). Built in plan step 2:
  - **One device program per policy** (`oci/devshare.rs`). Containers whose device policies are the same
    share one loaded program. `create` still loads its own copy, which tells it the program's tag. The hints
    file in the state root (`.device-programs.json`: at most 64 programs, read up to 64 KiB) names a program
    loaded before with that tag. `create` attaches that one only once the kernel shows it's the same
    program: the same type, tag and lengths, and the same translated instructions byte for byte (f29). Its
    own copy then drops, and the kernel frees it. A stale or wrong hint costs the sharing, never the policy.
    A spec without `linux.resources` keeps the default device policy, as runc's does (f30).
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

**What would falsify it.**
- **The shim's removal:** an OCI operation that needs a process per container. runtime-tools' 28 passing tests,
  crun's set on this host, are the check.
- **Sharing:** a device program that keeps state per container, such as a map, which Hex's doesn't (it reads
  only its context, f28). Or a kernel that charges each attachment as much as a program, so that sharing saves
  nothing: the density phase's vmalloc and percpu rise per container would show it, and they're 0.
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
state once, opens the FIFO the init is blocked on, takes its byte and removes the FIFO; it writes no state. A
container's status is derived as crun derives it (`state::derive`; libcrun's `libcrun_get_container_state_string`):
`created` while its init lives and the FIFO is there, `running` once the FIFO is gone, `stopped` once the init no
longer lives (by its pid and recorded start time). A state that a start from before this derivation marked
`running` reads as running while its init lives. Create records whether the bundle names poststart hooks, so start
reads `config.json` only when it does. The node agent tries an oci box's start first, reading none of the box's
record on that path, and judges the init's liveness by the pidfd its supervisor holds, a `poll`.

**Evidence.** Step 1 cut start into its parts: half of it (0.12-0.13 ms) was the rewrite of `state.json` as
running, with a second read of the state and a read of `config.json` for hooks the bundle didn't have. Without them,
in the breakdown run `exec-breakdown-20261028T224731Z`, start was 0.084-0.090 ms against libcrun's 0.083-0.089.

**Tests.** The derivation as a pure function, against an oracle table of every combination (recorded status, the
FIFO there or not, the init live with its start time, live with another, a zombie or gone): `created`, `running` or
`stopped` as the runtime-spec defines them (`state::tests`, AC-T7.1.4). runtime-tools and the cgroups lab hold start
and state to the runtime-spec on the command line.

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
whether that keeps up with sustained churn is the sustained phase's to show (goal 4 plan step 5). A make that fails
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
failed make where it may not and serves takes on the path (HEX-NETNS-11).

**Sources.** `docs/research/runtime/netns-pool.md`: the kernel's `net/core/net_namespace.c` at v6.19; namespaces(7),
setns(2) and user_namespaces(7); containerd v2.2.0's `pkg/netns`; Lambda's networking account (Vogels, 2026); AFaaS
(Chai et al., OSDI 2025); Scrivano's `unshare -n` timings (2022); SOCK, for the history.

**What would falsify it.** A create from the pool costing more than 5% over a create without a namespace, at p50
through the seam, on any machine measured (fedora, jupiter, the Mac Studio): the pool then isn't taking the
namespace's work off the path. Or a sustained phase in which more than 1% of creates found the pool dry: one thread
refilling to 32 isn't keeping up.

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
4. **Network namespaces off the path** (decision 4): done when Hex's create through the seam with a network
   namespace is at most 5% above its create without one, at p50, on fedora, jupiter and the Mac Studio, and the
   sustained phase reports how often the pool ran dry.
5. **The seed** (decision 5): done when create p50 through the seam is at most a quarter of libcrun's in-process
   create p50, and create+start likewise.
6. **Mount namespaces without copying** (decision 6): done when create on a 7.x host is measured with and without
   `OPEN_TREE_NAMESPACE`, and the faster path is the default.
7. **The process class through the seam** (decision 7): done when run-of-`true` p50 is at most a quarter of a systemd
   scope's, and held memory per idle box is at most a scope's.
