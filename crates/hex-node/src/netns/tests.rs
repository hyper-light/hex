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

/// The step at which this process's make ahead (`net::make_ahead`) fails, if one does: unshare(2)
/// takes `CLONE_NEWNET` only from a caller with CAP_SYS_ADMIN in its user namespace, and rtnetlink
/// takes loopback's RTM_NEWLINK only with CAP_NET_ADMIN (rtnetlink(7)). uid 0 alone gives neither: a
/// container runtime's default set, a Kubernetes pod's root's (kiln CI's), holds neither.
fn failing_step() -> Option<(Step, hex_core::netns::Missing)> {
    use rustix::thread::CapabilitySet;
    let held = rustix::thread::capabilities(None).unwrap().effective;
    let (sys_admin, net_admin) = (
        held.contains(CapabilitySet::SYS_ADMIN),
        held.contains(CapabilitySet::NET_ADMIN),
    );
    let step = if !sys_admin {
        Step::Unshare
    } else {
        Step::Loopback
    };
    hex_core::netns::Missing::of(sys_admin, net_admin).map(|missing| (step, missing))
}

/// HEX-NETNS-14: the agent's stderr line for a failed make names the step and why, errno and text.
#[test]
fn a_failed_make_is_said_with_its_step_and_reason() {
    let reason = Reason::new(
        Some(1),
        "unshare failed: Operation not permitted (os error 1)",
    );
    assert_eq!(
        failed_line(Step::Unshare, &reason),
        "hex node: a network namespace made ahead failed at unshare: errno 1: unshare failed: \
         Operation not permitted (os error 1)\n"
    );
}

/// HEX-NETNS-11: the refill thread hands the pool each namespace it makes, or counts each make that
/// fails, and a create is served either way. Do: start a pool of 2 and wait for its thread's first
/// outcome. Expect, where the agent may make network namespaces (CAP_SYS_ADMIN and CAP_NET_ADMIN):
/// the pool fills to its bound; a take hands over a namespace that isn't this process's, its loopback
/// up, and the thread makes another in its place. Expect, where it may not: one failed make, counted
/// under the step the missing capability fails (HEX-NETNS-12) and nothing made; then a take answered
/// on the path, counted, which asks the thread for one more, and that make fails and is counted too.
#[test]
fn the_refill_thread_fills_the_pool_or_counts_each_make_that_fails() {
    let pool = Netns::start(2).unwrap();
    wait_for("first make", || {
        let c = pool.counts().unwrap();
        c.made_ahead + c.failed > 0
    });
    let failing = failing_step();
    assert_eq!(
        pool.counts().unwrap().failed > 0,
        failing.is_some(),
        "a make fails exactly where CAP_SYS_ADMIN or CAP_NET_ADMIN is missing"
    );
    if let Some((step, missing)) = failing {
        hex_testkit::skip(
            "netns::tests::the_refill_thread_fills_the_pool_or_counts_each_make_that_fails",
            &missing.to_string(),
            "its full pool (made ahead, taken, loopback up) isn't tested here; its failed makes are",
        )
        .unwrap();
        let c = pool.counts().unwrap();
        assert_eq!((c.made_ahead, c.failed), (0, 1), "{c:?}");
        let mut by = hex_core::netns::FailedBy::default();
        let last = match step {
            Step::Unshare => {
                by.unshare = 1;
                &c.last_failed.unshare
            }
            _ => {
                by.loopback = 1;
                &c.last_failed.loopback
            }
        };
        assert_eq!(c.failed_by, by, "counted under the step that failed");
        // HEX-NETNS-14: the kernel's refusal is kept, errno and text: EPERM, a capability missing.
        let reason = last.as_ref().expect("the step's reason is kept");
        let eperm = rustix::io::Errno::PERM.raw_os_error();
        assert_eq!(reason.errno(), Some(eperm), "{reason}");
        assert!(reason.text().contains("not permitted"), "{reason}");
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
