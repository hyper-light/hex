//! The node's ipc and uts namespace shell (goal 5 extended, step 4): its refill thread against the kernel this
//! test runs on, privileged or not.

use super::*;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Longest a test waits for a fact it needs: a bound, so a thread that never makes it true fails the test instead
/// of hanging it.
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

fn ns_of(fd: &OwnedFd) -> PathBuf {
    std::fs::read_link(format!("/proc/self/fd/{}", fd.as_fd().as_raw_fd())).unwrap()
}

fn own(kind: &str) -> PathBuf {
    std::fs::read_link(format!("/proc/self/ns/{kind}")).unwrap()
}

/// The namespace of kind `kind` that this process's `hex-ipcuts` thread is in.
fn refill_thread_in(kind: &str) -> PathBuf {
    std::fs::read_dir("/proc/self/task")
        .unwrap()
        .map(|task| task.unwrap().path())
        .find(|task| {
            std::fs::read_to_string(task.join("comm")).is_ok_and(|c| c.trim() == "hex-ipcuts")
        })
        .map(|task| std::fs::read_link(task.join("ns").join(kind)).unwrap())
        .expect("the refill thread")
}

/// HEX-IPCUTS-1: the refill thread hands the pool each pair it makes, or counts each make that fails, and a create
/// is served either way. Do: start a pool of 2 and wait for its thread's first outcome. Expect, where the agent may
/// make these namespaces (as root): the pool fills to its bound; two takes hand over pairs that are neither this
/// process's nor each other's; the thread makes more in their place and ends up back in this process's own
/// namespaces, holding none it made. Expect, where it may not: one failed make, counted and nothing made; then a
/// take answered on the path, counted, which asks the thread for one more, and that make fails and is counted too.
#[test]
fn the_refill_thread_fills_the_pool_or_counts_each_make_that_fails() {
    let pool = IpcUtsPool::start(2).unwrap();
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
    let first = pool.take().unwrap().expect("a pair made ahead");
    let second = pool.take().unwrap().expect("a second pair made ahead");
    for (kind, a, b) in [
        ("ipc", &first.ipc, &second.ipc),
        ("uts", &first.uts, &second.uts),
    ] {
        assert_ne!(ns_of(a), own(kind), "a {kind} namespace of its own");
        assert_ne!(ns_of(b), own(kind), "a {kind} namespace of its own");
        assert_ne!(ns_of(a), ns_of(b), "no two pairs share a {kind} namespace");
    }
    wait_for("makes in the taken ones' places", || {
        pool.counts().unwrap().made_ahead == 4
    });
    for kind in ["ipc", "uts"] {
        wait_for("the refill thread back home", || {
            refill_thread_in(kind) == own(kind)
        });
    }
    let c = pool.counts().unwrap();
    assert_eq!((c.taken, c.made_on_path, c.failed), (2, 0, 0), "{c:?}");
}

/// HEX-IPCUTS-2: a pool with a bound of 0 starts no thread and serves nothing. Do: take from it, then say a container
/// makes its own. Expect: the take answered on the path; both counted as made on the path, nothing made or taken.
#[test]
fn a_bound_of_0_makes_none_and_counts_every_container() {
    let pool = IpcUtsPool::start(0).unwrap();
    assert!(pool.take().unwrap().is_none());
    pool.on_path().unwrap();
    let c = pool.counts().unwrap();
    assert_eq!(
        (c.made_ahead, c.taken, c.made_on_path, c.failed),
        (0, 0, 2, 0),
        "{c:?}"
    );
}
