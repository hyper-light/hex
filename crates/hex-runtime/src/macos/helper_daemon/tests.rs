//! The box helper daemon's membership listing, rootless (HEX-MAC-13). `proc_listpids(PROC_UID_ONLY)` finds
//! this process's own children by its own user id — the mechanism the helper uses on a box's pool user,
//! which a daemonizing member can't escape. `kill_user` is not exercised here (it would signal this test's
//! whole user); it is measured as root on a dedicated pool user by `bench/native/macos/boxlab.sh`.

use std::process::{Child, Command};

use super::*;

const MIB: u64 = 1 << 20;

fn m(pid: i32, footprint: u64) -> Member {
    Member { pid, footprint }
}

/// HEX-MAC-14: a fresh box arms each member at its apportioned share; a read that changes nothing re-arms
/// nothing; a member growing shifts shares; a member gone is forgotten; a tree at the bound ends the box.
#[test]
fn reapportion_arms_the_changed_and_ends_when_over() {
    // 256 MiB bound, 16 MiB reserve → 240 MiB apportioned.
    let mut t = Tracked::new(7000, 256 * MIB, 16 * MIB);

    // Two idle members: both armed at 120 MiB (240 / 2).
    let plan = t.reapportion(&[m(10, 0), m(11, 0)]);
    assert_eq!(plan, Plan::Arm(vec![(10, 120 * MIB), (11, 120 * MIB)]));

    // Same footprints again: nothing changed, so nothing to re-arm.
    assert_eq!(t.reapportion(&[m(10, 0), m(11, 0)]), Plan::Arm(vec![]));

    // One member now holds 100 MiB: headroom 140 MiB split as 70 each → 10's share 170, 11's 70. Both change.
    let plan = t.reapportion(&[m(10, 100 * MIB), m(11, 0)]);
    assert_eq!(plan, Plan::Arm(vec![(10, 170 * MIB), (11, 70 * MIB)]));

    // Member 11 gone: 10 alone gets all 240 MiB; 11 is forgotten.
    let plan = t.reapportion(&[m(10, 100 * MIB)]);
    assert_eq!(plan, Plan::Arm(vec![(10, 240 * MIB)]));
    assert!(!t.armed.contains_key(&11));

    // The tree reaches the apportionable bound: end the box, reporting the real bound.
    assert_eq!(
        t.reapportion(&[m(10, 140 * MIB), m(11, 100 * MIB)]),
        Plan::End {
            footprint: 240 * MIB,
            bound: 256 * MIB
        }
    );
}

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
