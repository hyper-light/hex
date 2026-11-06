//! The node's network-namespace shell (decision 4, goal 4 plan step 4): its refill thread against the
//! kernel this test runs on, privileged or not.

use super::*;
/// The slowest recorded make of a network namespace by the pool's refill thread. MEASURING: the record to come
/// is `bench/competitive/probes/waits_census.md`'s `netns.make` row, which then sets this (the lead's rule of
/// 2026-11-06 for test patiences).
const MAKE_RECORDED: Option<Duration> = None;

/// Longest a test waits for the refill thread's fact: twice [`MAKE_RECORDED`]; unbounded while measuring.
const PATIENCE: Duration = match MAKE_RECORDED {
    Some(slowest) => slowest.saturating_mul(2),
    None => Duration::MAX,
};
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

use rustix::thread::{LinkNameSpaceType, move_into_link_name_space};

use crate::sizes::{Host, Sizes};

/// The pools' bounds on fedora, the bench host, as its bench units run the node: 32 hardware threads, about 7.5 GiB
/// of memory to hold, and a thread limit past the socket's need (`crate::sizes`).
fn bench() -> Sizes {
    Sizes::of(
        Host {
            memory: 7_500 << 20,
            parallelism: std::num::NonZeroUsize::new(32).unwrap(),
            threads: 500_000,
            descriptors: 524_288,
        },
        &hex_core::demand::Peaks::default(),
    )
}

/// Ada's rule and the lead's calls, 2026-11-06 (CLAUDE.md §2; goal 6 step 6's installer reads it): the descriptors a
/// root node holds at its learned pools' bounds on fedora's figures, a first start's. Do: bound the files under no
/// limit at the bench host's sizes, nothing held beside its bounds. Expect: the node's own 406,026 (342,634 with six
/// memory pools and the device programs uncounted; 331,086 at the old constants, the socket's connections then among
/// them): the pools' shares a seventh each with the device programs among them (440 seeds, 1,439 idle namespaces),
/// and the programs held, 65,536 at their bound, now counted; and every kept namespace there may be holding its files
/// four each past that, 679,682 in all.
#[test]
fn the_nodes_own_descriptors_on_fedoras_figures() {
    let unlimited = rustix::process::Rlimit {
        current: None,
        maximum: None,
    };
    let b = FilesBound::of(&unlimited, root_pools(), 0);
    assert_eq!(b.own, 406_026, "{:?}", bench());
    assert_eq!(b.own + b.namespaces * FILES_A_KEPT, 679_682);
}

/// A root node's pools on the bench host.
fn root_pools() -> Pools {
    Pools::root(bench())
}

/// Waits for `fact`, giving the CPU up between looks rather than sleeping a picked period, for at most
/// [`PATIENCE`], the longest a case waits for a fact: a thread that never makes it true fails the test
/// instead of hanging it.
fn wait_for(what: &str, mut fact: impl FnMut() -> bool) {
    let deadline = Instant::now().checked_add(PATIENCE);
    while deadline.is_none_or(|d| Instant::now() < d) {
        if fact() {
            return;
        }
        std::thread::yield_now();
    }
    panic!("no {what} within {PATIENCE:?}");
}

/// Whether loopback is up in network namespace `ns`: a thread of its own joins it, then listens on
/// 127.0.0.1 and connects, which a namespace whose loopback is down refuses (net.rs).
fn loopback_up_in(ns: &OwnedFd) -> bool {
    let ns = ns.try_clone().unwrap();
    std::thread::spawn(move || {
        move_into_link_name_space(ns.as_fd(), Some(LinkNameSpaceType::Network)).unwrap();
        // The listener lives until the connect is answered.
        std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|listener| std::net::TcpStream::connect(listener.local_addr()?))
            .is_ok()
    })
    .join()
    .unwrap()
}

/// HEX-NETNS-11: the refill thread hands the pool each namespace it makes, or counts each make that
/// fails, and a create is served either way. Do: start a pool of 2 and wait for its thread's first
/// outcome. Expect, where the agent may make network namespaces (as root): the pool fills to its
/// bound; a take hands over a namespace that isn't this process's, its loopback up, and the thread
/// makes another in its place. Expect, where it may not: one failed make, counted and nothing made;
/// then a take answered on the path, counted, which asks the thread for one more, and that make fails
/// and is counted too.
#[test]
fn the_refill_thread_fills_the_pool_or_counts_each_make_that_fails() {
    let pool = Netns::start(2, FilesBound::now(root_pools()).unwrap(), bench()).unwrap();
    wait_for("first make", || {
        let c = pool.counts().unwrap();
        c.made_ahead + c.failed > 0
    });
    if pool.counts().unwrap().failed > 0 {
        assert_ne!(
            rustix::process::geteuid().as_raw(),
            0,
            "root makes namespaces ahead"
        );
        let c = pool.counts().unwrap();
        assert_eq!((c.made_ahead, c.failed), (0, 1), "{c:?}");
        assert!(pool.take().unwrap().is_none(), "a dry take");
        assert_eq!(pool.counts().unwrap().made_on_path, 1);
        wait_for("second failed make", || pool.counts().unwrap().failed == 2);
        return;
    }
    wait_for("the pool's bound", || {
        pool.counts().unwrap().made_ahead == 2
    });
    let ns = pool.take().unwrap().expect("a namespace made ahead");
    assert_ne!(
        std::fs::read_link(format!("/proc/self/fd/{}", ns.as_fd().as_raw_fd())).unwrap(),
        std::fs::read_link("/proc/self/ns/net").unwrap(),
        "a namespace of its own"
    );
    assert!(loopback_up_in(&ns), "its loopback is up");
    wait_for("a make in the taken one's place", || {
        pool.counts().unwrap().made_ahead == 3
    });
    let c = pool.counts().unwrap();
    assert_eq!((c.taken, c.made_on_path, c.failed), (1, 0, 0), "{c:?}");
}

/// Goal 5 extended step 3 (AC-e's reader; T-T7.1.4): a create of an owner's box that finds the pool dry is
/// given a namespace the node makes on its path, which the box's init joins as it joins one from the pool,
/// and the node counts it apart from the pool's other takes made on the path, whose containers make their
/// own. Do: a node keeping namespaces, its pool empty and never refilled; take for an owner's box that may
/// share. Expect, where the agent may make network namespaces (as root): a namespace that isn't this
/// process's, its loopback up, kept for the owner, counted once by the pool as made on the path and once by
/// the node; where it may not: the take refused, the pool's count risen and the node's not.
#[test]
fn a_dry_take_for_an_owners_box_is_made_by_the_node_and_counted() {
    let state = Arc::new(KeptState {
        idle: Mutex::new(Idle::new(bench().starts.owner_netns, bench().idle_netns)),
        held: Mutex::new(BTreeMap::new()),
        given_back: AtomicU64::new(0),
        checked: AtomicU64::new(0),
        unchecked: AtomicU64::new(0),
        lost_home: AtomicU64::new(0),
        made_on_path: AtomicU64::new(0),
        protocols: AtomicU64::new(0),
        through_files: AtomicU64::new(0),
        files_failed: AtomicU64::new(0),
        files_bounded: AtomicU64::new(0),
        files_bound: FilesBound::now(root_pools()).unwrap(),
        files_held: Arc::new(AtomicU64::new(0)),
        check_ns: AtomicU64::new(0),
    });
    let netns = Netns {
        shared: Arc::new(Shared {
            pool: Mutex::new(Pool::new(1)),
            wake: Condvar::new(),
        }),
        refill: None,
        kept: Some(Keeping {
            state,
            returns: None,
            checkers: Vec::new(),
        }),
    };
    let owner = BoxOwner::named("dry-owner").unwrap();
    let choice = netns.choice(Some(&owner));
    let taken = choice.take_for(Ok(()));
    let pool = netns.counts().unwrap();
    let node = netns.reuse_counts().unwrap().unwrap();
    assert_eq!(
        (pool.taken, pool.made_on_path),
        (0, 1),
        "the pool counts the dry take: {pool:?}"
    );
    match taken {
        Ok(Some(ns)) => {
            assert_eq!(node.made_on_path, 1, "the node counts its make: {node:?}");
            assert_ne!(
                std::fs::read_link(format!("/proc/self/fd/{}", ns.as_fd().as_raw_fd())).unwrap(),
                std::fs::read_link("/proc/self/ns/net").unwrap(),
                "a namespace of its own"
            );
            assert!(loopback_up_in(&ns), "its loopback is up");
            assert!(choice.kept().is_some(), "kept for the owner's next box");
        }
        Ok(None) => panic!("a take for an owner's box that may share is never left to its clone"),
        Err(e) => {
            assert_ne!(
                rustix::process::geteuid().as_raw(),
                0,
                "root makes namespaces: {e}"
            );
            assert_eq!(
                node.made_on_path, 0,
                "nothing made, nothing counted: {node:?}"
            );
        }
    }
}

/// systemd's default soft limit on a service's descriptors: `DefaultLimitNOFILE=` is `1024:524288`
/// (systemd-system.conf(5), "DefaultLimitNOFILE=", since v240), the soft half the one a unit gets unless set.
const SYSTEMD_SOFT_NOFILE: u64 = 1_024;

/// Ada's 2026-11-06 rule (CLAUDE.md §2): the descriptors the agent holds past its bounds are counted, not
/// stated. Do: hold eight files open and count; count under a soft limit of none. Expect: at least the eight;
/// the count refused by type, naming the limit, since more are listed than it allows.
#[test]
fn the_descriptors_beside_the_bounds_are_counted() {
    use rustix::process::Rlimit;

    let held: Vec<std::fs::File> = (0..8)
        .map(|_| std::fs::File::open("/proc/self/stat").unwrap())
        .collect();
    let limit = rustix::process::getrlimit(rustix::process::Resource::Nofile);
    let counted = open_descriptors(&limit).unwrap();
    assert!(counted >= 8, "{counted} counted with eight held");
    let none = Rlimit {
        current: Some(0),
        maximum: Some(0),
    };
    assert!(
        matches!(open_descriptors(&none), Err(Refusal::Os { why, .. }) if why.contains("soft limit's 0")),
        "past the soft limit the count is refused by type"
    );
    drop(held);
}

/// Goal 5 extended step 6, the lead's second condition for a kept namespace's files as he restated it
/// (2026-11-03T23:07Z, 2026-11-04T09:15Z; CLAUDE.md §2, §3): the limit the node was given stays as it is, and what
/// it leaves past the node's own descriptors at its bounds is how many namespaces may hold their four files. Do: ask
/// of a soft limit at systemd's default, of one just past the node's own, of one leaving room for two namespaces
/// and a little more, and of none. Expect: none hold files at the default; two at the room for two; every namespace
/// there may be, under no limit; the node's own the same each time and past 65,536 boxes' four descriptors.
#[test]
fn the_files_bound_is_what_the_soft_limit_leaves_past_the_node_four_a_namespace() {
    use rustix::process::Rlimit;

    let beside = open_descriptors(&rustix::process::getrlimit(
        rustix::process::Resource::Nofile,
    ))
    .unwrap();

    let at = |soft: Option<u64>| {
        FilesBound::of(
            &Rlimit {
                current: soft,
                maximum: soft,
            },
            root_pools(),
            beside,
        )
    };
    let own = at(Some(SYSTEMD_SOFT_NOFILE)).own;
    let boxes = u64::try_from(hex_runtime::MAX_BOXES).unwrap();
    assert!(
        own >= boxes * 4,
        "each live box's own descriptors and its kept namespace's are in it: {own}"
    );
    assert_eq!(
        at(Some(SYSTEMD_SOFT_NOFILE)).namespaces,
        0,
        "systemd's default holds none"
    );
    assert_eq!(at(Some(own)).namespaces, 0, "just the node's own");
    assert_eq!(
        at(Some(own + 3)).namespaces,
        0,
        "short of one namespace's four"
    );
    assert_eq!(at(Some(own + 2 * 4 + 3)).namespaces, 2);
    let unlimited = at(None);
    assert_eq!(unlimited.soft, None);
    assert_eq!(unlimited.own, own);
    assert!(unlimited.namespaces >= boxes, "{unlimited:?}");
    assert_eq!(
        at(Some(u64::MAX)).namespaces,
        unlimited.namespaces,
        "never more than there may be namespaces"
    );
}

/// Goal 5 extended step 6, the files' bound past the node's own descriptors (CLAUDE.md §2; the lead's finding of
/// 2026-11-05T07:50Z on the composed tree): decision 4's network namespaces and step 4's ipc and uts pairs made ahead
/// are the node's own too. Do: bound the files under one soft limit with no pools, with each pool alone, with both at
/// a root node's bounds, and with each pool's bound raised. Expect: the node's own grows by each pool's held
/// descriptors, one a network namespace and two a pair, with its refill thread's and each call's takes beside, and
/// the namespaces holding files fall to what that leaves.
#[test]
fn the_files_bound_counts_the_namespaces_both_pools_make_ahead() {
    use rustix::process::Rlimit;

    let beside = open_descriptors(&rustix::process::getrlimit(
        rustix::process::Resource::Nofile,
    ))
    .unwrap();

    let at = |netns: usize, ipc_uts: usize| {
        FilesBound::of(
            &Rlimit {
                current: Some(1 << 22),
                maximum: Some(1 << 22),
            },
            Pools {
                netns,
                ipc_uts,
                devfs: true,
                sizes: bench(),
            },
            beside,
        )
    };
    let none = at(0, 0);
    let n = |count: usize| u64::try_from(count).unwrap();
    let takers = n(bench().seeds.get());
    assert_eq!(
        at(bench().netns, 0).own - none.own,
        n(bench().netns) + 2 + takers
    );
    assert_eq!(
        at(0, bench().ipc_uts).own - none.own,
        2 * (n(bench().ipc_uts) + 2) + 2 * takers
    );
    let root = at(bench().netns, bench().ipc_uts);
    assert_eq!(
        root.own - none.own,
        n(bench().netns) + 2 + 2 * (n(bench().ipc_uts) + 2) + 3 * takers
    );
    assert_eq!(
        root,
        FilesBound::of(
            &Rlimit {
                current: Some(1 << 22),
                maximum: Some(1 << 22)
            },
            root_pools(),
            beside
        )
    );
    let raised = at(4_096, 4_096);
    assert_eq!(
        raised.own - root.own,
        (4_096 - n(bench().netns)) + 2 * (4_096 - n(bench().ipc_uts))
    );
    let soft = raised.own + 10 * FILES_A_KEPT + 3;
    let under = |pools| {
        FilesBound::of(
            &Rlimit {
                current: Some(soft),
                maximum: Some(soft),
            },
            pools,
            beside,
        )
        .namespaces
    };
    assert_eq!(
        under(Pools {
            netns: 4_096,
            ipc_uts: 4_096,
            devfs: true,
            sizes: bench(),
        }),
        10,
        "what the limit leaves past the pools, four a namespace"
    );
    assert_eq!(
        under(root_pools()),
        10 + (raised.own - root.own + 3) / FILES_A_KEPT
    );
}

/// Goal 5 of record, the files' bound past the node's own descriptors (CLAUDE.md §2; the lead, 2026-11-05T11:45Z):
/// the mounts made ahead are the node's own too, step 3's prepared `/dev` pool and goal 6's prepared proc pool,
/// with their mask sources. Do: bound the files under one soft limit with the namespace pools at a root node's
/// bounds, with and without the mounts made ahead. Expect: the node's own grows by both pools held at their bounds
/// (the seeds' bound each, one descriptor a `/dev`, two a proc: its pid namespace and its tree), the one maker's peak
/// while it makes a proc (six), the two mask sources, and each call's takes of a `/dev` and a proc beside; a
/// rootless node keeps none.
#[test]
fn the_files_bound_counts_the_mounts_made_ahead() {
    use rustix::process::Rlimit;

    let beside = open_descriptors(&rustix::process::getrlimit(
        rustix::process::Resource::Nofile,
    ))
    .unwrap();

    let at = |devfs: bool| {
        FilesBound::of(
            &Rlimit {
                current: Some(1 << 22),
                maximum: Some(1 << 22),
            },
            Pools {
                devfs,
                ..root_pools()
            },
            beside,
        )
    };
    let n = |count: usize| u64::try_from(count).unwrap();
    let held = n(bench().seeds.get());
    assert_eq!(
        at(true).own - at(false).own,
        held + 2 * held + 6 + 2 + 3 * n(bench().seeds.get())
    );
    assert!(
        root_pools().devfs,
        "a root node keeps the mounts made ahead"
    );
}

/// Goal 5 extended step 6, the files' bound past the node's own descriptors (CLAUDE.md §2; the lead, 2026-11-05T11:58Z):
/// step 3's prepared `/dev` mounts are the node's own too. Do: bound the files under one soft limit with the namespace
/// pools at a root node's bounds, with and without the mounts made ahead. Expect: the node's own grows by at least the
/// `/dev` pool held at its bound (the seeds' bound, one descriptor a `/dev`), its maker's peak (two: its context and
/// its tree) and each call's take of a `/dev`; here the proc pool and mask sources are counted beside them
/// ([`the_files_bound_counts_the_mounts_made_ahead`]), and the one maker's peak is a proc's.
#[test]
fn the_files_bound_counts_the_prepared_dev_mounts() {
    use rustix::process::Rlimit;

    let beside = open_descriptors(&rustix::process::getrlimit(
        rustix::process::Resource::Nofile,
    ))
    .unwrap();

    let at = |devfs: bool| {
        FilesBound::of(
            &Rlimit {
                current: Some(1 << 22),
                maximum: Some(1 << 22),
            },
            Pools {
                devfs,
                ..root_pools()
            },
            beside,
        )
    };
    let n = |count: usize| u64::try_from(count).unwrap();
    let dev = n(bench().seeds.get()) + 2 + n(bench().seeds.get());
    let grown = at(true).own - at(false).own;
    assert!(
        grown >= dev,
        "the /dev pool's part ({dev}) is within the mounts made ahead's count ({grown})"
    );
}

/// Goal 5 extended step 6, the bound on namespaces holding files (CLAUDE.md §2): a place under it is taken before a
/// check opens files and given back as it drops. Do: take places under a bound of two until one is refused, drop one,
/// take again. Expect: two taken, the third refused; one dropped frees one; the count back to none at the end.
#[test]
fn a_files_slot_is_taken_up_to_the_bound_and_given_back_as_it_drops() {
    let held = Arc::new(AtomicU64::new(0));
    let first = FilesSlot::take(&held, 2).expect("the first of two");
    let second = FilesSlot::take(&held, 2).expect("the second of two");
    assert!(FilesSlot::take(&held, 2).is_none(), "past the bound");
    assert_eq!(held.load(Ordering::Acquire), 2);
    drop(first);
    let third = FilesSlot::take(&held, 2).expect("the place the first gave back");
    assert!(
        FilesSlot::take(&held, 0).is_none(),
        "a bound of none takes none"
    );
    drop((second, third));
    assert_eq!(held.load(Ordering::Acquire), 0);
}

/// Goal 5 extended step 6, the lead's conditions for the check's own socket (2026-11-03T17:36Z; CLAUDE.md §2): the
/// netlink socket a check keeps in a namespace is held with it inside the idle namespaces' bound, and closed with
/// it when the bound puts it out. Do: give one owner its bound + 1 kept namespaces, each with a stand-in for its
/// socket, the write end of a pipe whose read end the test keeps, and drop what the bound puts out, as the checker
/// does. Expect: one put out, the first given, its pipe at end-of-file; each kept one's still open. A process another
/// test spawns holds a copy of every descriptor from its fork to its exec, so the end-of-file is waited for, the fact
/// itself, as each such copy closes.
#[test]
fn a_kept_namespaces_socket_is_held_in_its_bound_and_closed_when_the_bound_puts_it_out() {
    let mut idle = Idle::new(bench().starts.owner_netns, bench().idle_netns);
    let mut ends = Vec::new();
    let mut out = Vec::new();
    for _ in 0..=bench().starts.owner_netns {
        let (read, write) = rustix::pipe::pipe_with(
            rustix::pipe::PipeFlags::CLOEXEC | rustix::pipe::PipeFlags::NONBLOCK,
        )
        .unwrap();
        let ns = rustix::fs::open(
            "/dev/null",
            rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .unwrap();
        out.extend(idle.give(
            "o",
            KeptNet {
                ns,
                socket: Some(write),
                files: None,
                slot: None,
            },
        ));
        ends.push(read);
    }
    assert_eq!(out.len(), 1, "one past the owner's bound is put out");
    drop(out);
    let mut byte = [0u8; 1];
    wait_for(
        "the end-of-file of the socket of the first given, put out",
        || rustix::io::read(&ends[0], &mut byte) == Ok(0),
    );
    for end in &ends[1..] {
        assert_eq!(
            rustix::io::read(end, &mut byte),
            Err(rustix::io::Errno::AGAIN),
            "each kept namespace holds its socket open"
        );
    }
}
