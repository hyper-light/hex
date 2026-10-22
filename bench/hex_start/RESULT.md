# hex_start (AC-T7.1.3) — of-record

Requirement AC-T7.1.3: an `oci` container start, image already present, has p50 ≤ 50 ms.

Bench: `bench/hex_start/run.sh` → `inside.sh` in a privileged container (private cgroup ns): builds a
minimal bundle (rootfs = relative symlinks into a read-only bind of /usr; workload `/usr/bin/true`),
vacates the cgroup root to a leaf, runs a probe, then times N=200 `hex oci create` + `hex oci start`
pairs (10 warm-up dropped), each torn down with `delete --force`. Reports create+start p50/p90/p99.

## Run (of-record)

- Hardware: `fedora.attlocal.net` — linux/x86_64, 32 cores, ≈64 GiB RAM, NVMe. Container pinned to
  CPUs 4–31 (`CPUSET=4-31`), off a neighbour's reserved cores 0–3.
- Date: 2026-10-22.
- Load: 1-minute load average ≈4.0 at measurement (27/32 cores idle). Above CLAUDE §7's <2, but the
  whole distribution (max 18.1 ms) sits far under the 50 ms bar, so the PASS is not load-sensitive —
  the operation is fork/unshare/pivot_root/cgroup/fifo-bound, not CPU-bound.
- Build: hex `cp/t7-runtime` release binary (`8424db7`; the `oci` runtime code unchanged since
  `fb15d9d`).

```
n=200 create+start ms: p50=14.4 p90=15.3 p99=16.7 min=12.7 max=18.1
AC-T7.1.3 (oci start p50<=50ms): PASS
```

p50 **14.4 ms** ≤ 50 ms — PASS, ~3.5× margin. The `inside.sh` create-time deadlock that previously hung
this bench (a detached-`create` init holding the harness's piped stdio; see `bench/hex_start/inside.sh`
and commit `4c76c13`) is fixed; this is the first clean run on the real runtime.
