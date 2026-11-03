# Goal 5 extended, step 6's second criterion: reusing a box's network namespace within one owner

Research for Ada's decision, not a plan step and not code (the lead, 2026-11-03T04:17Z). Step 6's second criterion,
sustained lifecycles a second at concurrency 32 at least 8x the throughput at concurrency 1 in one OCI pass, isn't met.
These are the measurements that point at the network namespace, the kernel's reasons for its cost, the state a reused
namespace can carry, what a check before handing one on would need, and what would show the lever wrong. Whether it
becomes a step is Ada's.

Sources: Linux at the v6.19 tag from `https://raw.githubusercontent.com/torvalds/linux/v6.19/<path>`, read 2026-11-03
(sha256, first 16 hex digits):
- `net/core/net_namespace.c` 5da2bfdcb0945a23, `net/core/rtnetlink.c` fa6c25afbf2e0679, `net/core/dev.c`
  24f1d8a24ac03b8b, `drivers/net/loopback.c` dcf721b20057b24a;
- `fs/proc/proc_sysctl.c` 268362f9043ecf61, `net/sysctl_net.c` b8845c76bcca1cda, `net/ipv4/devinet.c`
  668cbb8c5d6efedf, `net/ipv6/addrconf.c` 4ccf8c491769a185, `net/core/neighbour.c` 7ff01d3e6666a559;
- `net/netfilter/nfnetlink.c` a7bfd7ce2a84c511, `net/ipv4/netfilter/ip_tables.c` 5da74ea2600c965e,
  `net/core/sock_diag.c` d8cffa3cb5a77fc6, `include/net/tcp.h` a3fcc84152d92cff, `drivers/net/tun.c`
  7f7c67f1d582344a, `net/ipv4/fib_frontend.c` 391726dc21dc80e0, `net/core/fib_rules.c` 1bc370a66cd0624c,
  `include/net/net_namespace.h` 97772506f68440c9;
- and at master, read the same day: `net/core/rtnetlink.c` 0c361efa25f4658e, `net/core/dev.c` 93881a2b82a42a3a.

Each kernel fact below is what that code says; none is graded in kiln's research yet.

## What the measurements show

- **The cap moved twice.** With step 6 alone, one owner's boxes take `cgroup_mutex` about once a lifecycle, and the
  waits at concurrency 32 are the mount namespace's lock (`bench/competitive/probes/create_serial.md`, runs 9 and 11).
  With steps 2 and 3 merged in as well, an experiment of record for no branch, the mount lock's waits are gone. What's
  left is RTNL: 6.670 ms a lifecycle at concurrency 32, 3.560 of it in `register_netdev` under `loopback_net_init` and
  3.110 in `rtnl_newlink`. 11.44% of the on-CPU samples spin for it, and 12.58% spin on `sysctl_lock` (run 10).
- **The pool that makes namespaces ahead ran dry.** In the merge's OCI pass, 9,561 to 9,608 of each sustained phase's
  10,000 creates made their network namespace on the path. The pool's thread made only 386 to 437 ahead
  (`bench/competitive/probes/reuse_pass.md`, run 2).
- **The ratio stayed under 8x.** With steps 2, 3 and 6 it was 4.05x to 4.83x (4-31, as host.sh runs it), because each
  lever raised the rate at concurrency 1 as much as at 32. On CPUs 4-15 at f7c4065, step 6 alone reads 2.89x to 3.32x
  (run 3).
- **Pending:** the OCI pass of the merge with every bundle given no network namespace of its own (`NETNS=0`) against
  the same with one (`NETNS=1`), in fedora unit `hex-g5x-s6-verify3b-20261103T030643Z`. Its ratio is the most a
  reused namespace could give. If it doesn't reach 8x either, the namespace is not the last cap.

## Why a network namespace costs what it does (Linux v6.19)

- **Making one takes a host-wide lock per lifecycle.**
  - `copy_net_ns` (`net/core/net_namespace.c:551`) takes `pernet_ops_rwsem` to read (`:577`) and runs `setup_net`
    (`:436`). That calls every registered pernet operation's init (`:445-449`), then takes `net_rwsem` to write to
    list the namespace (`:450`).
  - Loopback's init, `loopback_net_init` (`drivers/net/loopback.c:207`, registered at `:236`), calls
    `register_netdev(dev)` (`:218`).
  - `register_netdev` takes `rtnl_net_lock_killable(net)` (`net/core/dev.c:11519`). That is `rtnl_lock_killable()`
    and then the namespace's own mutex (`net/core/rtnetlink.c:229-236`), on `static DEFINE_MUTEX(rtnl_mutex);` (`:76`).
- **The device's sysctl tables are registered inside that hold.**
  - `register_netdevice` (`net/core/dev.c:11295`, `ASSERT_RTNL()` at `:11303`) runs the NETDEV_REGISTER notifiers
    (`:11444`) before RTNL is let go.
  - The notifiers include IPv4's `inetdev_init` (`net/ipv4/devinet.c:266`, `devinet_sysctl_register` at `:291`), IPv6's
    `ipv6_add_dev` (`net/ipv6/addrconf.c:377`, `addrconf_sysctl_register` at `:460`) and the neighbour tables'
    `neigh_sysctl_register` (`net/core/neighbour.c:3815`, `register_net_sysctl_sz` at `:3886`).
  - Each goes through `register_net_sysctl_sz` → `__register_sysctl_table` (`net/sysctl_net.c:163-173`). That takes
    the host's one `sysctl_lock` spinlock (`fs/proc/proc_sysctl.c:88`, at `:1389` and `:1398`, and in `get_subdir`,
    `:995`, at `:1002` and `:1011`).
  - So spinning on `sysctl_lock` lengthens every RTNL hold, and both show in run 10's samples.
- **Bringing the loopback up takes RTNL again.** The init's `RTM_NEWLINK` (`crates/hex-runtime/src/linux/oci/net.rs`)
  is a doit without `RTNL_FLAG_DOIT_UNLOCKED` (`net/core/rtnetlink.c:7061-7062`). So `rtnetlink_rcv_msg` runs it under
  `rtnl_lock()` (`:6954-6968`). The pool does the same for each namespace it makes ahead.
- **Unmaking one is batched.**
  - The last reference puts the namespace on `cleanup_list` and queues one work item, `net_cleanup_work`
    (`net/core/net_namespace.c:737`, `__put_net` at `:739-745`). It runs on a single-threaded `netns_wq` (`:1264`).
  - `cleanup_net` takes every namespace queued so far (`llist_del_all`, `:668`), then `pernet_ops_rwsem` to read
    (`:670`) and `net_rwsem` to write (`:673`).
  - It undoes the pernet operations in reverse (`ops_undo_list`, `:696`): an expedited RCU grace period (`:242-245`),
    then one `rtnl_lock()` for the whole batch to unregister every device (`ops_exit_rtnl_list`, `:165-190`, `:173`,
    `unregister_netdevice_many` at `:187`). Then it waits on `rcu_barrier()` (`:703`).
  - So the teardown's RTNL hold is amortized over a batch, and the making's isn't.
- **Not fixed in mainline yet.** At master on 2026-11-03, `rtnl_net_lock_killable` still takes `rtnl_lock_killable()`
  first (`net/core/rtnetlink.c:230-238`), and `register_netdev` still calls it (`net/core/dev.c:11810-11816`). The
  per-namespace RTNL work (`RTNL_FLAG_DOIT_PERNET`, `CONFIG_DEBUG_NET_SMALL_RTNL`) adds a namespace's own mutex under
  the global one, not in place of it.
- **SOCK measured the same in 2018** (Oakes et al., USENIX ATC 2018, PDF sha256 46eaf854117d0158, p. 59).
  - "Network namespaces are more problematic for both creation and cleanup due to a single global lock that is shared
    across network namespaces". Its reference [13] is Eric Dumazet, "Re: net: cleanup net is slow", lkml, April 2017.
  - "With unmodified network namespaces, throughput peaks at about 200 c/s (containers/second). With minor
    optimizations (disabling IPv6 and eliminating the costly broadcast code), it is possible to churn over 400 c/s.
    However, eliminating network namespaces entirely provides throughput of 900 c/s."
  - SOCK's lean containers make no network namespace: "Network namespaces are unnecessary because requests arrive over
    Unix domain socket" (§4.1, p. 62).

## How a pool and a reuse move those costs

- **A pool that makes namespaces ahead** (decision 4) takes the making off the create's path but not off the host.
  - Each lifecycle still costs one `setup_net`, one loopback registration under RTNL with its sysctl tables, one
    loopback brought up under RTNL, and its share of a batched teardown.
  - The sustained rate is bounded by how fast the host can do that work under its one RTNL and one `sysctl_lock`,
    whoever does it. That's step 4's argument against a cgroup pool made ahead: it moves no hold.
  - In run 2's merge the creates making theirs on the path took RTNL ahead of the pool's thread, which made a twentieth
    of what was taken.
- **A namespace handed from box to box of one owner** takes the making, the loopback's registration and bringing-up,
  and the teardown off the lifecycle altogether.
  - On the reuse path a lifecycle takes no RTNL and no `sysctl_lock` for its network namespace, but for what the check
    below reads.
  - The ceiling it lifts is RTNL's, measured at 6.670 ms of waiting a lifecycle at concurrency 32 (run 10). The
    `NETNS=0` pass will measure how much of the rate that was.

## What a box can change in its network namespace, and with what capability

- **Links, addresses, routes, rules, neighbours, queueing disciplines and namespace ids**: every rtnetlink request but a
  get needs CAP_NET_ADMIN over the namespace's user namespace. `if (kind != RTNL_KIND_GET &&
  !netlink_net_capable(skb, CAP_NET_ADMIN))` (`net/core/rtnetlink.c:6885`).
- **Interface flags and addresses by ioctl** (`SIOCSIFFLAGS`, `SIOCSIFADDR` and their kin): CAP_NET_ADMIN
  (`devinet_ioctl`, `net/ipv4/devinet.c:1062`, checks at `:1099-1111`).
- **A tun or tap device**: CAP_NET_ADMIN to make one (`tun_set_iff`, `drivers/net/tun.c:2692`, `:2757`).
- **nftables and iptables**: nfnetlink refuses without CAP_NET_ADMIN (`net/netfilter/nfnetlink.c:661`), and so do
  iptables' set and get (`net/ipv4/netfilter/ip_tables.c:1625`, `:1654`).
- **Sockets**: any process may bind, listen and connect; raw and packet sockets need CAP_NET_RAW, which Docker's
  defaults grant. Each closes with its process, but a TCP connection closed actively lingers in TIME_WAIT for
  `TCP_TIMEWAIT_LEN` (60 s, `include/net/tcp.h:141`). Destroying a socket through sock_diag needs CAP_NET_ADMIN
  (`sock_diag_destroy`, `net/core/sock_diag.c:313-315`).
- **The namespace's sysctls**:
  - With CAP_NET_ADMIN, a writer gets the table's owner bits for everyone (`net_ctl_permissions`,
    `net/sysctl_net.c:42-54`).
  - Without it, the table's own mode applies, and the namespace's root is its owner (`test_perm`,
    `fs/proc/proc_sysctl.c:425-434`): for a namespace a root node made, global root. So a root node's box running as
    uid 0 can write the writable `net.*` sysctls of its namespace without CAP_NET_ADMIN, unless `/proc/sys` is
    read-only in it.
  - A few handlers check CAP_NET_ADMIN themselves (`devinet_sysctl_forward`, `net/ipv4/devinet.c:2520-2530`).

So a box that holds neither CAP_NET_ADMIN nor CAP_SYS_ADMIN, whose `/proc/sys` is read-only, can leave its namespace
with nothing changed but TIME_WAIT connections and counters.

## The check a reused namespace would need

- **Who may share one** (from the spec, as `could_change_its_cgroup` decides for cgroups):
  - no CAP_NET_ADMIN or CAP_SYS_ADMIN in any set, with capability sets given;
  - `/proc/sys` read-only (OCI `readonlyPaths`), or a user namespace in which the box's uid 0 isn't the namespace
    owner's root;
  - a namespace the node made, never one joined by path;
  - the same owner, never another.
- **What to read before handing one on**, over netlink and sock_diag sockets the node opens inside each namespace when
  it makes it (a socket stays in the namespace it was made in, so no `setns` per check):
  - no socket left but TIME_WAIT ones, by sock_diag dumps of the namespace's inet, unix and netlink sockets;
  - lo the only link, up, holding 127.0.0.1/8 and ::1/128. The address dump takes no RTNL (`inet_dump_ifaddr`,
    `RTNL_FLAG_DUMP_UNLOCKED`, `net/ipv4/devinet.c:2890-2891`). The link dump does: `rtnl_dump_ifinfo` has no such flag
    (`net/core/rtnetlink.c:7065-7066`), so `rtnl_dumpit` takes `rtnl_lock()` around it (`:6813-6826`). A check that reads links
    takes the lock once, unless it reads `/proc/net/dev` from a thread in the namespace or relies on the eligibility
    above (no box without CAP_NET_ADMIN can make a link);
  - the routing tables and rules at their defaults, both dumped without RTNL (`inet_dump_fib`,
    `net/ipv4/fib_frontend.c:1699-1700`; `fib_nl_dumprule`, `net/core/fib_rules.c:1451-1452`);
  - no nftables table (a nfnetlink dump in the namespace) and no iptables table (the namespace's
    `/proc/net/ip_tables_names` empty).
- **What can't be reset**:
  - TIME_WAIT connections, for up to 60 s, unless the node destroys them (it holds CAP_NET_ADMIN);
  - the namespace's counters (`/proc/net/snmp` and its kin), which run on;
  - its identity (`net_cookie`, `include/net/net_namespace.h:171`) and its hash seed (`hash_mix`, `:119-122`). So a box
    can tell it was given its owner's earlier box's namespace, and anything keyed on the seed is shared within the
    owner.

## The boundary across one owner's boxes

- **What others share within one owner.**
  - Lambda's slots run "a single function", and "The MicroVM and the process the function is running in are both
    re-used" across its serial invocations, its network with it (research f39, Firecracker, NSDI 2020).
  - Kubernetes puts every container of a Pod in one network namespace that outlives them: "Every container in a Pod
    shares the network namespace, including the IP address and network ports" (`kubernetes/website`,
    `content/en/docs/concepts/workloads/pods/_index.md:336-338`, read 2026-11-03, sha256 dbc592a96241fb0b).
- **What crosses from one box to its owner's next:** TIME_WAIT and port state, the counters, the namespace's identity
  and seed, and anything the check misses. Nothing crosses owners, the line the cgroup's reuse holds (f39).
- **What stays out:** a box that could change its namespace (CAP_NET_ADMIN, CAP_SYS_ADMIN, a writable `/proc/sys`)
  always gets a fresh one, as a box that could change its cgroup gets a fresh cgroup.

## What would show the lever wrong

- The `NETNS=0` pass not lifting concurrency 32 to 8x concurrency 1: then the network namespace isn't the last cap.
- A namespace that passes the check while carrying one of the state classes above: a test for each, as test 3 has a case
  for each of the cgroup's guards.
- The check costing what the making does, at concurrency 32 (the link dump's RTNL hold among it).
- The next box's binds or connections failing on TIME_WAIT or port state its owner's earlier box left.
- Eligible boxes being rare: specs that grant CAP_NET_ADMIN or keep `/proc/sys` writable.
