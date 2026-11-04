//! The node's network-namespace shell (decision 4, goal 4 plan step 4): its refill thread against the
//! kernel this test runs on, privileged or not.

use super::*;
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

use rustix::thread::{LinkNameSpaceType, move_into_link_name_space};

/// Longest a test waits for a fact it needs: a bound, so a thread that never makes it true fails the
/// test instead of hanging it.
const PATIENCE: Duration = Duration::from_secs(30);

fn wait_for(what: &str, mut fact: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if fact() {
            return;
        }
        std::thread::park_timeout(Duration::from_millis(1));
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
    let pool = Netns::start(2).unwrap();
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
        idle: Mutex::new(Idle::new(netns_reuse::PER_OWNER, netns_reuse::PER_NODE)),
        held: Mutex::new(BTreeMap::new()),
        given_back: AtomicU64::new(0),
        checked: AtomicU64::new(0),
        unchecked: AtomicU64::new(0),
        lost_home: AtomicU64::new(0),
        made_on_path: AtomicU64::new(0),
        protocols: AtomicU64::new(0),
        through_files: AtomicU64::new(0),
        files_failed: AtomicU64::new(0),
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

/// Goal 5 extended step 6, the lead's second condition for a kept namespace's files (CLAUDE.md §2, §3): the
/// descriptor limit is checked at the node's start against all it may hold, raised to that, and where it can't be
/// the refusal names the shortfall. Do: ask of limits that hold it, and of limits under it. Expect: nothing asked of
/// the first; the soft limit raised, and a hard one under it too; the shortfall in the refusal's words.
#[test]
fn the_descriptor_limit_is_raised_to_all_a_node_may_hold_or_the_shortfall_refused() {
    use rustix::process::Rlimit;

    let needs = descriptors_a_node_may_hold();
    assert!(
        needs >= 65_536 * 8,
        "each live box's kept namespace and its own are in it: {needs}"
    );
    for enough in [
        Rlimit {
            current: Some(needs),
            maximum: Some(needs),
        },
        Rlimit {
            current: None,
            maximum: None,
        },
    ] {
        assert!(raised_limit(needs, &enough).is_none());
    }
    let soft = raised_limit(
        needs,
        &Rlimit {
            current: Some(1_024),
            maximum: Some(needs + 1),
        },
    )
    .unwrap();
    assert_eq!((soft.current, soft.maximum), (Some(needs), Some(needs + 1)));
    let low = Rlimit {
        current: Some(1_024),
        maximum: Some(4_096),
    };
    let both = raised_limit(needs, &low).unwrap();
    assert_eq!((both.current, both.maximum), (Some(needs), Some(needs)));
    let refused = short_of(needs, &low, "EPERM").to_string();
    for word in [
        needs.to_string(),
        4_096.to_string(),
        (needs - 4_096).to_string(),
    ] {
        assert!(refused.contains(&word), "{word} in {refused}");
    }
}

/// Goal 5 extended step 6, the lead's conditions for the check's own socket (2026-11-03T17:36Z; CLAUDE.md §2): the
/// netlink socket a check keeps in a namespace is held with it inside the idle namespaces' bound, and closed with
/// it when the bound puts it out. Do: give one owner `PER_OWNER` + 1 kept namespaces, each with a stand-in for its
/// socket, the write end of a pipe whose read end the test keeps, and drop what the bound puts out, as the checker
/// does. Expect: one put out, the first given, its pipe at end-of-file; each kept one's still open. A process another
/// test spawns holds a copy of every descriptor from its fork to its exec, so the end-of-file is waited for, the fact
/// itself, as each such copy closes.
#[test]
fn a_kept_namespaces_socket_is_held_in_its_bound_and_closed_when_the_bound_puts_it_out() {
    let mut idle = Idle::new(netns_reuse::PER_OWNER, netns_reuse::PER_NODE);
    let mut ends = Vec::new();
    let mut out = Vec::new();
    for _ in 0..=netns_reuse::PER_OWNER {
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
