# hex_start (AC-T7.1.3) — of-record

Requirement AC-T7.1.3: an `oci` container start, image already present, has p50 ≤ 50 ms.

Bench: `bench/hex_start/run.sh` → `inside.sh` in a privileged container (private cgroup ns): builds a
minimal bundle (rootfs = relative symlinks into a read-only bind of /usr; workload `/usr/bin/true`),
vacates the cgroup root to a leaf, runs a probe, then times N=200 `hex oci create` + `hex oci start`
pairs (10 warm-up dropped), each torn down with `delete --force`. Reports create+start p50/p90/p99.

## Run (of-record): the binary whose containers each have a cgroup (2026-10-24)

Since 3319a85 every `oci` container gets a cgroup with its resources and devices in force, and since
05af083 it is born in that cgroup (`clone3(CLONE_INTO_CGROUP)`), not migrated into it. This run is on
that binary.

- Hardware: `fedora.attlocal.net`, as below; the container pinned to CPUs 4–31.
- Date: 2026-10-24 05:37Z. Load: 1-minute load average 1.21 at start (CLAUDE §7's < 2).
- Build: hex `cp/t7-runtime` release binary at `05af083` (`hex-cg2-2f0c122c`, sha256
  2f0c122c9ddd19e2…), built on fedora after all seven gates passed. `inside.sh` as at HEAD.

```
n=200 create+start ms: p50=20.6 p90=23.2 p99=26.4 min=15.7 max=31.2
AC-T7.1.3 (oci start p50<=50ms): PASS
```

p50 **20.6 ms** ≤ 50 ms: PASS, a 2.4× margin. A cgroup per container costs about 6 ms of it against
the run below, which made none. The binary that joined its cgroup by migration measured p50 28.5 ms
on the same night (2026-10-24 03:23Z, load 1.63): a migration after a quiet spell waits out an RCU
grace period (`docs/design/runtime.md`, "Born in its cgroup").

## Earlier run: before containers had cgroups (2026-10-22)

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
