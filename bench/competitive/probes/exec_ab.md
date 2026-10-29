# Builds of Hex's exec set against each other, and against libcrun, in one run

Goal 3 of the runtime design (decision 3 of `docs/design/runtime-architecture.md`; kiln plan
`01a17c0ccc6f721e84990c5ca370de3e`) is judged by Hex's exec p50 through the `Runtime` seam over libcrun's in-process
exec p50, read from one run. A change to the exec path is judged the same way, against the build before it: on
fedora, a host whose speed for a short burst of work swings by more than the change does (below), two builds measured
in different runs can't be told apart, so this probe measures them in one.

## How it runs

`exec_ab.sh <name>=<hex>:<seam_oci> ...` copies each build's binaries into the run's directory and runs
`exec_ab_inside.sh` in the bench container with host.sh's OCI-pass flags (privileged, a private cgroup namespace, no
network, pinned to CPUs 4-31, the host's `/usr`), on the OCI pass's bundles. Each of `ROUNDS` rounds (3) runs, in a
shuffled order, libcrun's lifecycle phase in process (`oci_bench.py libcrun`, the exec and start each build is set
against) and each build's `seam_oci` lifecycle phase (create, start, exec of `true` and delete of a `sleep`
container, each timed alone; `N` 600 timed after `WARMUP` 60). `exec_ab.py summarize` writes each round's p50s and
each build's over libcrun's in the same round. With `TRACE=1`, each build then runs its lifecycle once more with an
ftrace instance of the run's own recording, between `seam_oci`'s markers, the scheduler's events beside the system
calls `exec_breakdown.md` cuts on; `exec_ab.py sched` reads where the scheduler ran each exec.

## The host's state

fedora (Intel Core i9-14900HX: P-cores on CPUs 0-15, E-cores on 16-31; `intel_pstate` active, EPP
`balance_performance`, on mains) runs the bench's execs on E-cores in most calls (450 of 500 traced in
exec-breakdown-20261028T224731Z, 420 of 500 in exec-breakdown-20261028T231639Z). How fast such a core runs a short
burst swings with the host's state: `exec_floor.c`'s `spin` gauge (a 100,000-step dependent multiply-add chain, no
system call) took 0.085-0.097 ms in exec-breakdown-20261028T224731Z (load 3-5) and 0.224 ms in
exec-breakdown-20261028T231639Z (load 2-3), and every exec with it:

| run | spin p50 | libcrun exec p50 | floor p50 | seam exec p50 | seam over libcrun |
|---|---|---|---|---|---|
| exec-breakdown-20261028T224731Z | 0.085-0.097 ms | 1.983 ms | 0.325-0.332 ms | 0.487 ms | 0.245 |
| exec-breakdown-20261028T231639Z | 0.224 ms | 3.817 ms | 0.785-0.815 ms | 1.140 ms | 0.299 |

The slow state stretches the floor 2.4x and libcrun's exec 1.9x, so the ratio of the shorter call to the longer one
grows with it: the floor itself is 0.165 of libcrun's exec in the fast state and 0.21 in the slow one.

**The slow state is fedora at rest.** `host_clock.sh` times a 100,000-step multiply-add chain, 200 times 2 ms apart,
on the P-cores and on the E-cores, with the host as it is and then with one busy loop on CPU 0 (outside the bench's
CPUs) holding the package awake. With nothing else running (2026-10-29T00:22Z, load 0.41, the same program by hand):
0.336 ms on a P-core and 0.500 on an E-core at rest, 0.104 and 0.161 held. With someone else's work on the host
(00:28Z, load 2.49): 0.101 and 0.173 at rest, 0.095 and 0.163 held. A burst that finds an idle package runs at about a
third of its clock; one busy core anywhere keeps it up. A call that runs long ramps the clock itself, a short one
doesn't, so at rest a short call's ratio to a long one grows. The same build against libcrun, a minute apart
(`exec_ab.sh`, `N` 600, best of 2):

| run | fedora | libcrun exec p50 | seam exec p50 | seam over libcrun | libcrun start p50 | seam start p50 |
|---|---|---|---|---|---|---|
| exec-ab-20261029T002311Z | held awake | 1.834 ms | 0.372 ms | 0.203 | 0.079 ms | 0.078 ms |
| exec-ab-20261029T002359Z | at rest | 3.831 ms | 0.996 ms | 0.260 | 0.198 ms | 0.174 ms |

The build is 8224f97's (hex sha256 749fd88368188ade, seam_oci f4b4c60b87b6cabf); load 0.45 to 2.53 across the
first, 2.53 to 2.80 across the second (the busy loop's own share in the first).

## Records

Each run is on fedora (kernel 6.19.10-300.fc44.x86_64), in its directory under `~/.cache/hex-bench/out/`, with every
binary it ran. These predate this harness, made by an earlier copy of its scripts (the same container, bundles and
shuffled rounds; a variant chosen by a temporary switch, `HEXX_PIN`, in one build of 4a6cfee plus the pin, never
committed); with two builds, `exec_ab.sh` repeats them. Slow state throughout (libcrun's exec 3.87-3.97 ms).

### The pin

Whether the clone's child runs on the calling thread's CPU (the thread pinned to it for the clone, the child
inheriting the pin and giving it back first thing, `crates/hex-runtime/src/linux/oci/seed.rs`) or on the CPU the
kernel's fork balancing picks.

ab-pinab-20261028T233714Z (load 0.85 at start, 3.25 at end; its libcrun rounds failed, a missing `/run/err`), exec
p50 (ms):

| | r1 | r2 | r3 |
|---|---|---|---|
| no pin | 1.222 | 1.186 | 1.227 |
| pin | 1.137 | 1.123 | 1.141 |
| pin, and the child gives the thread back its CPUs | 1.174 | 1.088 | 1.175 |

ab-pin3-20261028T234403Z (load 0.86 at start, 3.03 at end), exec p50 (ms) and over libcrun's in the same round:

| | r1 | r2 | r3 |
|---|---|---|---|
| libcrun | 3.881 | 3.969 | 3.894 |
| pin | 1.170 (0.301) | 1.145 (0.288) | 1.127 (0.289) |
| pin, and the child gives the thread back its CPUs | 1.145 (0.295) | 1.150 (0.290) | 1.151 (0.296) |
| pin held until the caller's wait ends | 1.074 (0.277) | 1.109 (0.279) | 1.116 (0.287) |

The traces (tr-pin3-20261028T235234Z, 150 execs a variant, load 0.6-1.6), by `exec_ab.py sched`, p50 (µs):

| part | no pin | pin | pin, the child gives back the thread's CPUs |
|---|---|---|---|
| before the clone | 88 | 104 | 108 |
| fork to the child's first run | 86 | 18 | 19 |
| the program's exit to the agent's wake | 165 | 146 | 158 |
| the call | 1238 | 1191 | 1232 |

Pinned, the child runs 18 µs after its fork rather than 86; the pin's own calls (`sched_getaffinity` and
`sched_setaffinity`) cost about 16 µs before the clone, so exec's p50 falls by 50-60 µs, about 5%. The pin stays, the
plain one: giving the thread its CPUs back from the child measured no better, and holding the pin past the call
leaves the caller's thread held to one CPU after `exec` returns, which `exec` must not do. What none of them
removes: in 40 to 58 of 150 execs the kernel's exec balancing (`sched_exec`, kernel/sched/core.c) moves the program
to another CPU at its `execve`, which the program can't be kept from without starting it pinned.

### Exec reads nothing from disk

ab-cache-20261029T000144Z (load 2.20 at start, 3.34 at end): `old` is the pin's build above (`HEXX_PIN` unset, the
plain pin), `new` the same with what an exec needs of the container kept with the init the supervisor watches
(`crates/hex-node/src/supervise.rs`, `Container`), so an exec reads no state file, and with its stdio borrowed rather
than duplicated. Exec p50 (ms) and over libcrun's in the same round:

| | r1 | r2 | r3 |
|---|---|---|---|
| libcrun | 3.877 | 3.885 | 3.865 |
| old | 1.157 (0.299) | 1.121 (0.288) | 1.085 (0.281) |
| new | 1.090 (0.281) | 1.115 (0.287) | 1.097 (0.284) |

The trace (tr-cache-20261029T000552Z) has the time before the clone at 75 µs p50, from 104: 29 µs gone, inside the
rounds' spread here. Start: libcrun 0.201-0.203 ms, old 0.190-0.204, new 0.192-0.212.

### Where a slow-state exec goes

From the traces above (a traced call runs about 4% longer than its untraced twin in the same process: 1.192 ms
against 1.141 in exec-breakdown-20261028T231639Z), an exec of `true` in the slow state,
p50s: about 75 µs in the agent before the clone, 60-75 µs in the clone (`copy_process`), about 240 µs in the
`execve`, about 550 µs running `true` itself, and about 150 µs from its `exit_group` to the agent's wake; the agent's
own work after the vfork releases it, about 55 µs, runs on the program's CPU before the program finishes its
`execve`. The kernel's and the program's part is about 1.0 ms of the traced call's 1.19, Hex's own about 170 µs. A
quarter of libcrun's exec in that state (0.97 ms) would take about four fifths of Hex's own time away.
