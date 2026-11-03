# The init's ipc and uts namespaces made ahead (goal 5 extended, step 4)

Source notes for decision 5's paragraph on the pair made ahead (docs/design/runtime-architecture.md). The plan step rests
on kiln research `01a190c51f137042a9697949c8fe2c7a`, f88-f91 (`bench/competitive/probes/levers.md`): `CLONE_NEWIPC` adds
30-35 us to a clone, `CLONE_NEWUTS` 7-8, and two `setns` into namespaces made ahead cost 6 us.

## What the kernel does (Linux v6.19)

- **Making an ipc namespace** (`ipc/namespace.c:38-103`, `create_ipc_ns`): a ucount of the maker's user namespace
  (`UCOUNT_IPC_NAMESPACES`, with a flush of pending frees when at its limit, :45-56), the namespace itself, then a mqueue
  filesystem of its own (`mq_init_ns`, :72; `ipc/mqueue.c:1626-1642`, its kernel mount made by `mq_create_mount`,
  :469-489, through `fs_context_for_mount` and `fc_mount_longterm`), and two sysctl tables of its own, each copied and
  registered (`ipc/mq_sysctl.c:123,146`, `ipc/ipc_sysctl.c:251,293`). All of it is on a clone's path when the clone makes
  the namespace.
- **Making a uts namespace** (`kernel/utsname.c:36-70`, `clone_uts_ns`): a ucount, a slab object, and the maker's names
  copied under `uts_sem` (:58). A pair made by the node's thread starts with the thread's names, the node's, as a
  container's clone starts with the seed's: the same names. The thread goes back to its own pair after each make, so no
  later pair copies a name a container set.
- **Freeing an ipc namespace** is deferred already (`ipc/namespace.c:150-209`): the last `put_ipc_ns` queues it, and one
  work item frees a batch after one `synchronize_rcu`. A pair made ahead changes nothing at a container's end.
- **Joining** (`kernel/nsproxy.c:563-601`, `setns`): a namespace descriptor with a type named must be of that type, or
  `EINVAL` (:573-576), so a pair handed over crossed is refused rather than joined; each call copies the caller's
  nsproxy (`prepare_nsset`, :348-352). `ipcns_install` (`ipc/namespace.c:233-244`) and `utsns_install`
  (`kernel/utsname.c:127-140`) take `CAP_SYS_ADMIN` in the namespace's owning user namespace and the caller's: a
  container in a user namespace of its own can't join a pair the node made, so its clone makes its own.
- **What lives in the pair**: a mqueue mount takes the mounter's ipc namespace (the init mounts `/dev/mqueue` after it
  joins), System V objects and the ipc sysctls are the namespace's, and `sethostname` writes the uts namespace the caller
  is in. So the init joins before it mounts, writes sysctls or sets the hostname.

## What step 4 measured (fedora, `bench/competitive/probes/create_breakdown.md` and `netns_pool.md`)

- A pair made by one thread, as the pool's does: about 21 us, 43,000 a second on an E-core; 6.9 KiB charged to the maker.
- Pool on against drained in one run: the clone (part 16) fell 0.143 to 0.104 ms, about 39 us, more than f88-f91's
  30-35 + 7-8 under a busier host; the joins put about 11 us back (part 17, 0.041 to 0.052), twice f91's 6, since each
  `setns` copies an nsproxy and the init's first steps pay for it.
- The first build (run A) gave most of the clone's saving back: a wake of the refill thread per take, whose make then ran
  beside the next create's clone, and the init's close of the pair (the last references, freed on the path). Waking the
  thread only once the pool is half empty, and leaving the pair for the exec to close, took the whole create from
  0.985 of drained in run A to 17-37 us faster than drained, pair by pair, in every round of run B.
