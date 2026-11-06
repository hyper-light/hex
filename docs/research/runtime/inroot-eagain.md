# openat2(RESOLVE_IN_ROOT) and EAGAIN: why Linux refuses, and how long to try again

The sources behind `crates/hex-runtime/src/linux/oci/inroot.rs`'s `IN_ROOT_TRIES` and its retry (00f5f66, extended on
cp/t7-inroot-eagain). Accessed 2026-11-05; raw.githubusercontent.com at the revision named; sha256 of each file as
fetched in brackets. `DESIGN` facts are from text or code; `MEASURED` ones name their run.

- [E-1] openat2(2), man-pages (man7.org, fetched 2026-11-05) [c80f85bca4a6babd75fadb7f6730cd78a9a00f072df9346a9a20e01e22b28e4a].
- [E-2] Linux v6.19, `fs/namei.c` [2a7a5d9869d0baf0a35400183b49b1189c8bd5b33d45be8c448f43a34925aeb3].
- [E-3] libpathrs at 05a4b54, `src/syscalls.rs` [b7b38705609db2477a7495ea71066ffccc6c2e37806f7da68b82a774bdce37e8].
- [E-4] runc v1.5.2, `internal/pathrs/retry.go` [7ce4c674f3e41ba3dd0b0223bb7472035180980ffb83928cbccec9a6b7434344].
- [E-5] filepath-securejoin v0.4.1, `openat2_linux.go` [b912dc29842c0fc3dc90eb4f1f62cd4b23a9c4c909b106c5bb97dd743ac4ffe9].

## 1. Why Linux refuses (DESIGN)

- openat2(2), ERRORS: "EAGAIN how.resolve contains either RESOLVE_IN_ROOT or RESOLVE_BENEATH, and the kernel could not
  ensure that a ".." component didn't escape (due to a race condition or potential attack). The caller may choose to
  retry the openat2() call." [E-1]
- The walk samples two global sequence counts when it starts: `nd->m_seq = __read_seqcount_begin(&mount_lock.seqcount)`
  and `nd->r_seq = __read_seqcount_begin(&rename_lock.seqcount)` [E-2] `fs/namei.c:2637-2638`.
- After each `..` in a scoped lookup it checks them again. "If there was a racing rename or mount along our path, then we
  can't be sure that ".." hasn't jumped above nd->root (and so userspace should retry or use some fallback)": either
  count moved and the walk returns `-EAGAIN` (`:2195-2206`).
- `mount_lock` and `rename_lock` are each one lock for the whole host. So a mount in any mount namespace, an
  unprivileged user's included, or a rename anywhere, refuses a resolve that crossed a `..` meanwhile. A node's own
  creates mount without pause, so they refuse each other's.

## 2. How others try again (DESIGN)

| | budget | what's tried again | source |
|---|---|---|---|
| libpathrs | 128 tries | EAGAIN, scoped lookups only | [E-3] `src/syscalls.rs:864-873`, `:898-909`: "with 128 retries you only have a ~0.1% chance of hitting the error path (even with an attacker pounding on rename on all cores)" |
| runc v1.5.2 | libpathrs's 128 a call, called again until 2 ms pass | EAGAIN | [E-4] `retry.go:28-32`: "the single longest latency caused by -EAGAIN retries was ~800us (with the vast majority being closer to 400us)", on a 16-core host under heavy rename load |
| filepath-securejoin v0.4.1 | 10 tries | EAGAIN and EXDEV | [E-5] `openat2_linux.go:31-44`, `:51-58` |

Hex tries EAGAIN alone, as libpathrs and runc do. securejoin also tries again on EXDEV, which a scoped lookup's
final check returns when the path ended outside the root. Hex refuses that, as a refusal to resolve outside the root.

## 3. What Hex's budget meets (MEASURED, fedora, i9-14900HX, Linux 6.19.10)

The test `a_mount_elsewhere_doesnt_refuse_a_path_with_a_dotdot`, in a scratch build whose `in_root` tries up to
1,000,000 times and records each resolve's tries and time, 2,007 resolves a run, CPUs 16-31:

| storm | runs | resolves tried again | most tries in one resolve | slowest resolve |
|---|---|---|---|---|
| one mount loop (unit hex-inroot-measure-20261105T125754Z, load 15-20) | 10 | 0-5 a run | 3 | 40 us |
| 16 mount and 16 rename loops (unit hex-inroot-storm-20261105T130241Z, load 14-20) | 10 | 4-12 a run | 5 | 3,679 us |

No resolve came near 128 tries; the slowest under the storm was the CPU's wait, not tries. Under Ada's 2026-11-06 rule
(a bound is learned or derived from measured data, never copied) `IN_ROOT_TRIES` is twice the most this record
measured, 2 x 5 = 10, the failure-bound margin of 2; libpathrs's 128 was its copied value until then. The same unit's (a): the test at 7c04775 (128 tries), 20
runs, none refused. Past the budget, the refusal names the path, the component and the tries. Each retry is
counted on its thread (`inroot::retried`), the init's count as it is single-threaded; the tests read it to show the
race was met.

## 4. What would falsify it

A host where a resolve needs more than 10 tries under its normal load: the refusal says so by name, and the budget
is re-derived from that host's record (2 x its most), or moves to a wall-clock deadline as runc's.
