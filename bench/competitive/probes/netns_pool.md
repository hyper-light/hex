# What a container's network namespace costs, and what a pool of them would

Goal 4 of the runtime design (decision 4 of `docs/design/runtime-architecture.md`; kiln plan
`01a185679f8a7616ab21db7fb980ffce`, research `01a185679f9073d696fc7e1a7c21587e`) moves the network-namespace work
off create's path with a pool of fresh namespaces made ahead. Two probes measure what that rests on:

- **`netns_cost_inside.sh`**: what a container's network namespace costs create and delete today, Hex's `oci` class
  through the `Runtime` seam (`seam_oci`'s lifecycle phase on a bundle with the namespace and on one without)
  beside libcrun in process (`oci_bench.py`'s lifecycle and lifecycle_nonet), and a bare `unshare -n true` against
  `unshare -u true`; 2 rounds of 500 in a shuffled order, best of 2.
- **`netns_pool.c`, `netns_pool.py`, `netns_pool_inside.sh`**: what an idle pooled namespace holds and how fast one
  thread makes them. A single-threaded process makes namespaces as the pool would (each a fresh one by
  `unshare(CLONE_NEWNET)`, kept alive by a descriptor, loopback brought up inside), and holds them, in a cgroup of
  its own. The driver reads what that cgroup holds before and after with `cgmem.py` (memory.current less its file
  cache, read once still across two of the kernel's flushes), with its kernel parts, and the host's `/proc/meminfo`
  beside it, since the kernel charges only some of a namespace's memory to the cgroup that makes it (`struct net`
  is `SLAB_ACCOUNT`, `net/core/net_namespace.c:1259-1261` at v6.19; not every pernet subsystem's allocation is).
  2 rounds of 500.

Both run in the bench container as root: on fedora with host.sh's OCI-pass flags (privileged, a private cgroup
namespace, no network, CPUs 4-31, the host's `/usr`), elsewhere in `docker_pass.sh`'s image on Docker Desktop's VM.

## Records

### What the namespace costs today (netns_cost_inside.sh)

fedora, `kiln-a094a12d` (Intel Core i9-14900HX, Linux 6.19.10-300.fc44), 2026-10-29T02:09Z, load 3.66 at the start
and 5.11 at the end, the package awake (`host_clock.sh`: 0.110 ms on a P-core, 0.166 on an E-core); hex 749fd883
and seam_oci f4b4c60b, built at 8224f97; libcrun 1.30.1 (`~/.cache/hex-t7/g4/out-20261029T020919Z`):

```
unshare -u true: 678 us each; unshare -n true: 1119 us each; unshare -u true again: 732 us each
hex create p50 best of 2: with a network namespace 15.177 ms, without 14.606 ms, the namespace +0.571 ms (1.04x)
hex delete p50 best of 2: with a network namespace 6.737 ms, without 6.774 ms, the namespace -0.037 ms (0.99x)
libcrun create p50 best of 2: with a network namespace 2.970 ms, without 2.422 ms, the namespace +0.548 ms (1.23x)
libcrun delete p50 best of 2: with a network namespace 10.354 ms, without 10.332 ms, the namespace +0.023 ms (1.00x)
```

jupiter (AMD Ryzen AI MAX+ 395, Docker Desktop's WSL2 VM, Linux 6.18.54.1-microsoft-standard-WSL2, 32 CPUs),
2026-10-29, VM load 5.35 at the start and 8.03 at the end; hex and seam_oci built at 3db2173 (8224f97's code):

```
unshare -u true: 1404 us each; unshare -n true: 2007 us each; unshare -u true again: 1318 us each
hex create p50 best of 2: with a network namespace 14.244 ms, without 13.679 ms, the namespace +0.565 ms (1.04x)
hex delete p50 best of 2: with a network namespace 5.784 ms, without 5.826 ms, the namespace -0.042 ms (0.99x)
libcrun create p50 best of 2: with a network namespace 5.089 ms, without 4.321 ms, the namespace +0.769 ms (1.18x)
libcrun delete p50 best of 2: with a network namespace 10.586 ms, without 10.588 ms, the namespace -0.002 ms (1.00x)
```

adas-mac-studio-3 (Apple M4 Max, Docker Desktop's LinuxKit VM, Linux 6.12.65-linuxkit aarch64, 14 CPUs), 2026-10-29,
VM load 4.38 at the start and 4.90 at the end; hex and seam_oci built at 3db2173:

```
unshare -u true: 332 us each; unshare -n true: 787 us each; unshare -u true again: 332 us each
hex create p50 best of 2: with a network namespace 2.936 ms, without 2.392 ms, the namespace +0.544 ms (1.23x)
hex delete p50 best of 2: with a network namespace 2.189 ms, without 2.251 ms, the namespace -0.062 ms (0.97x)
libcrun create p50 best of 2: with a network namespace 2.056 ms, without 1.849 ms, the namespace +0.206 ms (1.11x)
libcrun delete p50 best of 2: with a network namespace 11.591 ms, without 11.790 ms, the namespace -0.200 ms (0.98x)
```

### What an idle pooled namespace holds, and how fast one thread makes them (netns_pool)

fedora, the same host, 2026-10-29T03:00Z, load 5.14 at the start and 4.14 at the end, bench/competitive at 5ee4d1f
with these probes (`~/.cache/hex-bench/out/netns-pool-20261029T030006Z`):

```
round 1: 500 namespaces held; make p50 0.152 ms, p99 0.333 ms, one thread 6127/s; the cgroup holds 108.5 KiB each (kernel 107.7: slab 97.6, percpu 10.1); the host's Slab +244.3 KiB, Percpu +105.2 KiB, MemAvailable -303.6 KiB each
round 2: 500 namespaces held; make p50 0.161 ms, p99 0.401 ms, one thread 5260/s; the cgroup holds 107.8 KiB each (kernel 107.7: slab 97.6, percpu 10.1); the host's Slab +236.3 KiB, Percpu +98.6 KiB, MemAvailable -628.2 KiB each
```

adas-mac-studio-3, the same VM, 2026-10-29T03:01Z, VM load 2.44 at the start and 2.13 at the end, CPUs 2-13:

```
round 1: 500 namespaces held; make p50 0.407 ms, p99 1.119 ms, one thread 2280/s; the cgroup holds 320.4 KiB each (kernel 320.4: slab 287.1, percpu 33.2); the host's Slab +622.0 KiB, Percpu +140.0 KiB, MemAvailable -774.5 KiB each
round 2: 500 namespaces held; make p50 0.258 ms, p99 0.711 ms, one thread 3545/s; the cgroup holds 320.4 KiB each (kernel 320.4: slab 287.1, percpu 33.2); the host's Slab +616.0 KiB, Percpu +139.0 KiB, MemAvailable -836.1 KiB each
```

The cgroup's share is steady to the 0.1 KiB across rounds; the host's Slab and Percpu move with other work on the
host as well, and MemAvailable most of all (fedora is shared), so they bound what a namespace costs the kernel
system-wide from above rather than measure it.
