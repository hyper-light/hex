# The exec seed's clone: `clone3(CLONE_PARENT)` needs `exit_signal=0`, and the rootless fallback

The evidence behind `docs/design/runtime-architecture.md`, decision 3's "The exec seed (goal 3, plan step 2)".
Accessed 2026-10-27/28. Man pages from man7.org (`clone(2)`, `setns(2)`, `pid_namespaces(7)`); kernel facts from
the Linux source named inline. Numbers marked `MEASURED` are from the spikes below; `DESIGN` marks a documented fact.

## The plan's one call works — with `exit_signal = 0`

Plan step 2 says the seed should, in one call, `clone3(CLONE_INTO_CGROUP | CLONE_PIDFD | CLONE_PARENT)` the program:
born in the container's cgroup (`CLONE_INTO_CGROUP`), with a pidfd for the agent (`CLONE_PIDFD`), and parented to
the seed's own parent — the agent — so the agent owns it (`CLONE_PARENT`). This is right, with one detail the plan
left out: **`CLONE_PARENT` requires `exit_signal = 0`.**

- `MEASURED` (cachyos 7.2.9 and fedora 6.19, `~/.cache/hex-t7/{seed-spike,clone-parent-probe}`): `clone3` with
  `CLONE_PARENT` and `exit_signal = SIGCHLD` returns `EINVAL`; with `CLONE_PARENT` and `exit_signal = 0` it
  **succeeds**, and the child's `getppid()` is the grandparent's (the caller's parent), i.e. `CLONE_PARENT` took
  effect. `DESIGN`: the kernel's `clone3_args_valid` (kernel/fork.c) rejects `CLONE_PARENT | CLONE_THREAD` together
  with a non-zero `exit_signal`, because such a child's termination is signalled to the *original* parent with
  *that parent's* chosen signal, not a new one — the child inherits the caller's exit signal (`copy_process`). A
  seed the node agent spawned has `SIGCHLD` as its own exit signal, so the program the seed clones reports to the
  agent with `SIGCHLD`, as the agent's other children do.
- (An earlier spike here used `exit_signal = SIGCHLD` and so saw only the `EINVAL`, and wrongly concluded
  `CLONE_PARENT` was impossible under `clone3`. The discriminator is `exit_signal`, not `clone3` vs legacy `clone`.)

So a **privileged** seed does one clone (`sys::clone_sibling`): it sets its children's pid namespace to the
container's (`setns(CLONE_NEWPID)`, which governs only the caller's *future* children, not the caller —
`pid_namespaces(7)`), then `clone3(CLONE_PARENT | CLONE_INTO_CGROUP | CLONE_PIDFD, exit_signal = 0)`. The program is
born in the container's cgroup and pid namespace, the agent's child from its first instruction, and the seed is
handed its pidfd at once — no intermediate, no reparenting, no race. The program then joins the container's other
namespaces itself (user first, mount last, cgroup last, as `hex __oci-exec`'s child does), becomes the container's
process and execs.

## The rootless fallback needs an intermediate

Setting the children's pid namespace is the one step a privileged seed can take that a rootless one cannot.

- `DESIGN`: `setns(fd, CLONE_NEWPID)` enters a pid namespace through `pidns_install` (kernel/pid_namespace.c),
  which requires `CAP_SYS_ADMIN` in **both** the caller's own user namespace and the user namespace that owns the
  target pid namespace. A rootless seed runs in the host user namespace without `CAP_SYS_ADMIN`, so for a container
  with a user+pid namespace of its own it is refused `EPERM`.
- `MEASURED` (cachyos 7.2.9): joining a rootless container's pid namespace from outside needs its user namespace
  joined first — `nsenter --user --preserve-credentials --pid` works, `--pid` alone is `EPERM`.

So on `EPERM` the seed forks a short-lived **intermediate** (`through_intermediate`): the intermediate joins the
container's user namespace first (which grants it `CAP_SYS_ADMIN` there), then the rest, then
`clone3(CLONE_INTO_CGROUP | CLONE_PIDFD)` of the program — **no `CLONE_PARENT`**, since the intermediate is now the
program's parent inside the joined namespaces. The program reparents to the agent (a child subreaper, decision 2)
when the intermediate exits; the intermediate answers the *seed* over an internal socketpair, and the seed reaps it
(`waitpid`) — so the program has reparented — *before* it forwards the answer to the agent, so the agent's
`waitid(P_PIDFD)` never meets a program that isn't yet its child. The intermediate is a fork of the seed, no exec,
gone before the program runs. Both paths keep "no process but the program" and "no `hex` exec per call".

## Why delete must reap the program

`DESIGN` + `MEASURED` (the HEX-RT-7 bug): the exec'd program is in the container's pid namespace but reparented out
to the agent. When the box is deleted, `cgroup.kill` ends the container's init (pid 1 of that namespace), but the
kernel holds the init in `zap_pid_ns_processes` (kernel/pid_namespace.c) until every process in the namespace is
reaped — and the init can't reap a child that is now the agent's. So the agent reaps the box's exec'd programs on
delete (`supervise::reap_programs` after `cgroup.kill`) and records each exit for the caller's `Proc` to read; the
`hex __oci-exec` command line's helper did this reap itself. Without it, delete's `wait_empty` times out with the
box "populated" but nothing in any `cgroup.procs` (the init, mid-teardown) — the shape the bug showed.

## What would change this

A kernel that let a rootless `setns(CLONE_NEWPID)` succeed (or let `clone3` choose the child's pid namespace
directly) would drop the intermediate entirely. A kernel that rejected `CLONE_PARENT` with `exit_signal = 0` would
force the intermediate on the privileged path too.
