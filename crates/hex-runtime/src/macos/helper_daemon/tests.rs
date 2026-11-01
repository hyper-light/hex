//! The box helper daemon's membership listing, rootless (HEX-MAC-13). `proc_listpids(PROC_UID_ONLY)` finds
//! this process's own children by its own user id — the mechanism the helper uses on a box's pool user,
//! which a daemonizing member can't escape. `kill_user` is not exercised here (it would signal this test's
//! whole user); it is measured as root on a dedicated pool user by `bench/native/macos/boxlab.sh`.

use std::process::{Child, Command};

use super::*;

/// A few sleeping children of this process, killed when dropped.
struct Sleepers(Vec<Child>);

impl Drop for Sleepers {
    fn drop(&mut self) {
        for c in &mut self.0 {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// HEX-MAC-13: every live child this process started is found in its user's listing, by pid.
#[test]
fn members_lists_the_users_live_processes() {
    let uid = rustix::process::getuid().as_raw();
    let sleepers = Sleepers(
        (0..3)
            .filter_map(|_| Command::new("/bin/sleep").arg("30").spawn().ok())
            .collect(),
    );
    assert_eq!(sleepers.0.len(), 3, "spawned three sleepers");
    let want: Vec<i32> = sleepers
        .0
        .iter()
        .map(|c| i32::try_from(c.id()).expect("a pid"))
        .collect();

    // Wait on the fact, not the clock (CLAUDE.md §4): poll the listing on a counted budget until every
    // child is in it. The kernel knows a child at fork, so the first read usually already has them.
    let mut pids = std::collections::BTreeSet::new();
    for _ in 0..100_000u32 {
        pids = members(uid)
            .expect("list the user's members")
            .iter()
            .map(|m| m.pid)
            .collect();
        if want.iter().all(|p| pids.contains(p)) {
            break;
        }
        std::hint::spin_loop();
    }
    for pid in &want {
        assert!(
            pids.contains(pid),
            "member {pid} not found in the user's listing"
        );
    }
    // This process itself is one of its user's processes.
    let me = rustix::process::getpid().as_raw_nonzero().get();
    assert!(pids.contains(&me), "the listing holds this process {me}");
}
