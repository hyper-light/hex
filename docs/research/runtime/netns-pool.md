# Network namespaces made ahead: what they cost, how they are kept, who may join them

Source notes for decision 4 of `docs/design/runtime-architecture.md` (goal 4; kiln research
`01a185679f9073d696fc7e1a7c21587e`, where each finding is graded from these sources). Measurements of Hex's own are
in `bench/competitive/probes/netns_pool.md`; this note holds what others document.

## What the kernel does to make and end one

- **Making one runs every networking subsystem's per-namespace init**, under a reader's hold on the lock that
  guards their list. Linux v6.19, `net/core/net_namespace.c`
  (<https://raw.githubusercontent.com/torvalds/linux/v6.19/net/core/net_namespace.c>): `copy_net_ns` takes
  "rv = down_read_killable(&pernet_ops_rwsem);" (line 577) and calls `setup_net`, which walks
  "list_for_each_entry(ops, &pernet_list, list) {" and calls "error = ops_init(ops, net);" for each (lines
  445-446). So its cost follows the networking modules a host has loaded.
- **Ending one is deferred**: the last reference queues it for a work item, "/* Cleanup the network namespace in
  process context */" ... "queue_work(netns_wq, &net_cleanup_work);" (`__put_net`, lines 739-744), and
  `cleanup_net` ends in `rcu_barrier()` (line 703). A process that leaves a namespace, or exits from it, doesn't
  wait for that work; nor does Hex's delete.
- **Only part of it is charged to a cgroup**: `struct net` comes from a cache made `SLAB_PANIC|SLAB_ACCOUNT`
  (lines 1259-1261), charged to the memory cgroup of the task that makes the namespace; other subsystems' per-namespace
  allocations aren't all accounted. A namespace's memory is charged when it is made, to the maker's cgroup, and stays
  there.

## How one is kept with no process in it, and joined

- namespaces(7) (<https://man7.org/linux/man-pages/man7/namespaces.7.html>), the /proc/[pid]/ns/ directory: opening
  one of its files returns a handle for that namespace, and "As long as this file descriptor remains open, the
  namespace will remain alive, even if all processes in the namespace terminate."
- setns(2) (<https://man7.org/linux/man-pages/man2/setns.2.html>), Network, IPC, and UTS namespaces: "In order to
  reassociate itself with a new network, IPC, time, or UTS namespace, the caller must have the CAP_SYS_ADMIN
  capability both in its own user namespace and in the user namespace that owns the target namespace."
- user_namespaces(7) (<https://man7.org/linux/man-pages/man7/user_namespaces.7.html>): "When a nonuser namespace is
  created, it is owned by the user namespace in which the creating process was a member at the time of the creation
  of the namespace. Privileged operations on resources governed by the nonuser namespace require that the process
  has the necessary capabilities in the user namespace that owns the nonuser namespace." So a namespace the node
  agent makes ahead belongs to the agent's user namespace: a container in a user namespace of its own (a rootless
  node's) could join it only to hold no capabilities over its own network.

## Who makes them ahead

- **containerd** makes a pod's network namespace before its sandbox container exists, on a dedicated OS thread, and
  pins it with a bind mount; the sandbox then joins it by path. v2.2.0 `pkg/netns/netns_linux.go`
  (<https://raw.githubusercontent.com/containerd/containerd/v2.2.0/pkg/netns/netns_linux.go>): "// create a new
  netns on the current thread" / "err = unix.Unshare(unix.CLONE_NEWNET)" (lines 115-116), then "// mount point. This
  causes the namespace to persist, even when there" (line 125).
- **AWS Lambda** moved network creation off the invoke path entirely, pre-creating every network a worker will use
  at its boot: "The best solution, it turned out, was to stop creating networks on demand altogether." Under the RTNL
  lock, creating thousands at once queued until it was reordered: "Pool network namespaces first, create veth pairs
  inside the namespace before moving them to root". It reuses a network once it is released, with "the security
  hygiene required to safely reuse a network after release". (W. Vogels, "The invisible engineering behind Lambda's
  network", All Things Distributed, 2026-04-22,
  <https://www.allthingsdistributed.com/2026/04/the-invisible-engineering-behind-lambdas-network.html>; a vendor's
  account, no method.)
- **AFaaS** pre-creates independent network endpoints rather than share a stack, since reuse "introduces security
  risks by reusing the TCP/IP stack from previous containers" (Chai et al., OSDI 2025, p.504; quoted in
  `oci-runtimes-and-fast-start.md` §4.3). Hex hands each pooled namespace to one container and never back, so no state
  crosses between containers and no cleaning is needed.

## What a namespace costs over time

- G. Scrivano (Red Hat blog, 2022-11-16, <https://www.redhat.com/en/blog/speed-oci-containers>): "Creating and
  destroying a network namespace used to be very expensive." hyperfine of `unshare -n true` on Fedora 24: "Time
  (mean ± σ): 47.7 ms ± 51.4 ms"; on Linux 5.19.15, after netfilter's per-namespace hooks were freed with `call_rcu`:
  "Time (mean ± σ): 1.5 ms ± 0.5 ms".
- SOCK (Oakes et al., ATC 2018, §2.2) found network namespaces capped churn at about 200 containers/s through "a
  single global lock" on Linux 4.13; decision 4's earlier text and probe run 1 (`probes/oci_breakdown.md` §4)
  recorded that this no longer holds on fedora's 6.19.
