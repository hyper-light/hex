# The exec seed can't use `clone3(CLONE_PARENT)`: why the intermediate, measured

The evidence behind `docs/design/runtime-architecture.md`, decision 3's "The exec seed (goal 3, plan step 2)",
specifically the deviation from the plan's one-call `clone3(CLONE_INTO_CGROUP | CLONE_PIDFD | CLONE_PARENT)`.
Accessed 2026-10-27. Man pages are from man7.org (`clone(2)`, `setns(2)`) at that day's text; every number marked
`MEASURED` is from the spike below, run that day; `DESIGN` marks a documented fact.

## The claim the plan rests on, and why it can't hold

Plan step 2 says the seed should, in one call, `clone3(CLONE_INTO_CGROUP | CLONE_PIDFD | CLONE_PARENT)` the program:
born in the container's cgroup (`CLONE_INTO_CGROUP`), with a pidfd for the agent (`CLONE_PIDFD`), and reparented to
the seed's own parent — the agent — so the agent owns it (`CLONE_PARENT`). Three facts make that one call impossible
and shape the implementation instead.

1. **`clone3` rejects `CLONE_PARENT` with `EINVAL`** — `MEASURED`, on two kernels (below), plainly and inside an
   unprivileged user namespace, with and without `CLONE_PIDFD`. So the single call the plan names returns `EINVAL`
   and nothing is cloned.
2. **Legacy `clone(CLONE_PARENT)` works but can't carry `CLONE_INTO_CGROUP`** — `DESIGN`. `CLONE_INTO_CGROUP`
   ("since Linux 5.7") sets the child's cgroup through `clone3`'s `cl_args.cgroup` field: it "allows the child
   process to be created in a different version 2 cgroup" and "has effect only for version 2 cgroups" (`clone(2)`).
   That field exists only for `clone3` (the `clone(2)` argument table leaves its `clone()` row blank); legacy
   `clone(2)` has no way to pass it. So `CLONE_PARENT` and `CLONE_INTO_CGROUP` can't be combined in one call on
   Linux.
3. **A process can't join a PID namespace itself** — `DESIGN`. `setns(2)` and `unshare(2)` with a PID namespace
   "do not ... change the PID namespace of the calling process, because doing so would change the caller's idea of
   its own PID"; they cause "children subsequently created by the caller" to be placed in the namespace instead
   (`pid_namespaces(7)`). So a reused seed can't `setns` *itself* into a container's PID namespace to then clone a
   sibling; only a process it forks can be placed there, by that forked process joining the namespace and cloning
   its own child.

Together: the program must be cloned by a process that has itself joined the container's namespaces (fact 3), that
process can't be the reused seed (a user-namespace join is irreversible, so the seed couldn't serve the next
container), and the clone that puts the program in the cgroup can't also reparent it (facts 1, 2). So a per-exec
**intermediate** joins the namespaces and `clone3(CLONE_INTO_CGROUP | CLONE_PIDFD)`s the program (no `CLONE_PARENT`);
the program reaches the agent not by `CLONE_PARENT` but by **subreaper adoption** — the intermediate exits, and the
orphaned program reparents to the agent, which is `PR_SET_CHILD_SUBREAPER` (decision 2, `crates/hex-node/src/supervise.rs`),
the same path a container's init already takes.

## The measurement

Spike `~/.cache/hex-t7/seed-spike/cp.c` (and the fedora copy under `~/.cache/hex-t7/clone-parent-probe/`). It forks
a child that calls `clone3` with the given flags and reports the result; `exit_signal = SIGCHLD`, no stack (the
child continues on a copy of the caller's stack, as after `fork`):

```c
struct ca { unsigned long long flags, pidfd, child_tid, parent_tid, exit_signal, stack,
            stack_size, tls, set_tid, set_tid_size, cgroup; };
static void t(const char *w, unsigned long long f) {
  pid_t c = fork();
  if (c == 0) { struct ca a; memset(&a, 0, sizeof a); a.flags = f; a.exit_signal = SIGCHLD;
    long r = syscall(SYS_clone3, &a, sizeof a);
    if (r == -1) { fprintf(stderr, "[%s] clone3 FAILED: %s\n", w, strerror(errno)); _exit(1); }
    if (r == 0) _exit(0);
    fprintf(stderr, "[%s] clone3 OK child=%ld\n", w, r);
    int s; waitpid(r, &s, 0); _exit(0); }
  int s; waitpid(c, &s, 0);
}
int main(void) {
  t("clone3 flags=0", 0);
  t("clone3 CLONE_PARENT", CLONE_PARENT);
  t("clone3 CLONE_PARENT|CLONE_PIDFD", CLONE_PARENT | 0x1000ULL);
  return 0;
}
```

`MEASURED` results, identical on both kernels, and identical again when the whole program is run under
`unshare --user --map-root-user` (an unprivileged user namespace):

| host | kernel | `clone3 flags=0` | `clone3 CLONE_PARENT` | `clone3 CLONE_PARENT\|CLONE_PIDFD` |
|---|---|---|---|---|
| cachyos (local) | 7.2.9-2-cachyos | OK | `EINVAL` (Invalid argument) | `EINVAL` |
| fedora-2 | 6.19.10-300.fc44 | OK | `EINVAL` | `EINVAL` |

cachyos measured 2026-10-27 in the step-2 spike; fedora measured the same day via a kiln run (`gcc -O2`,
`unshare -Ur` second pass). `flags=0` succeeding on both shows the probe and the `clone_args` layout are right, so
the `EINVAL` is `CLONE_PARENT`'s, not the call's. Two kernels (a 7.x and a 6.x, the two Hex runs on) rather than
one, since the plan's call is kernel behaviour, not Hex's.

## The pid-namespace join order the intermediate uses (for completeness)

`MEASURED`, cachyos 7.2.9: joining a container's PID namespace from outside needs its user namespace joined first
when the container is rootless — `nsenter --user --preserve-credentials --pid` works, `nsenter --pid` alone is
`EPERM`. The intermediate therefore joins user first, then the rest, then clones (the cgroup namespace is left for
the cloned child to join once it is inside the cgroup, as `hex __oci-exec` already does: joining it first makes the
clone fail `ENOENT`, `crates/hex-runtime/src/linux/oci/exec.rs`).

## What this would change if it were wrong

If a later kernel accepted `clone3(CLONE_PARENT)` (or added `CLONE_INTO_CGROUP` to legacy `clone`), the intermediate
could be dropped and the seed could clone the program in one call. The design note's falsifier is unaffected: the
seam exec's cost is what the bench measures, intermediate or not.
