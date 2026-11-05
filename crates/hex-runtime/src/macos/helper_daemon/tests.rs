//! The box helper daemon's membership listing, rootless (HEX-MAC-13). `proc_listpids(PROC_UID_ONLY)` finds
//! this process's own children by its own user id — the mechanism the helper uses on a box's pool user,
//! which a daemonizing member can't escape. `kill_user` is not exercised here (it would signal this test's
//! whole user); it is measured as root on a dedicated pool user by `bench/native/macos/boxlab.sh`.

use std::os::fd::{AsFd, OwnedFd};
use std::process::{Child, Command};
use std::sync::Mutex;
use std::thread;

use hex_api::{BoxId, Program};
use hex_core::helper::{START_FDS, VERSION};
use rustix::net::{AddressFamily, SocketFlags, SocketType};

use super::super::helper_client::Helper;
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

/// An in-memory stand-in for the daemon's privileged backend: it tracks a box's members rather than
/// spawning or killing real processes, so the daemon's dispatch, apportionment and uid pool are driven
/// end-to-end over a real socket without root or side effects. The real backend is `helper_sys` (§12).
#[derive(Default)]
struct Sim {
    inner: Mutex<SimState>,
}

#[derive(Default)]
struct SimState {
    next_pid: i32,
    members: Vec<Member>,
    arm_calls: u32,
}

impl Privileged for Sim {
    fn spawn(
        &self,
        _uid: u32,
        _program: &Program,
        fds: [OwnedFd; START_FDS],
        _leader: bool,
        _cap_mib: u32,
        _nproc: u32,
    ) -> std::io::Result<i32> {
        drop(fds); // the stdio descriptors rode across; a real spawn would dup2 them.
        let mut s = self.inner.lock().expect("sim lock");
        s.next_pid += 1;
        let pid = 1000 + s.next_pid;
        s.members.push(Member { pid, footprint: 0 });
        Ok(pid)
    }

    fn arm(&self, _pid: i32, _cap_mib: u32) -> std::io::Result<()> {
        self.inner.lock().expect("sim lock").arm_calls += 1;
        Ok(())
    }

    fn list(&self, _uid: u32) -> std::io::Result<Vec<Member>> {
        Ok(self.inner.lock().expect("sim lock").members.clone())
    }

    fn kill(&self, _uid: u32, signal: i32) -> std::io::Result<usize> {
        let mut s = self.inner.lock().expect("sim lock");
        let n = s.members.len();
        // KILL (9) ends them; another signal is delivered but leaves them running (the stand-in's model).
        if signal == 9 {
            s.members.clear();
        }
        Ok(n)
    }
}

fn prog() -> Program {
    Program {
        args: vec!["/bin/true".to_owned()],
        env: vec![],
        cwd: None,
        user: None,
    }
}

fn nulls() -> [OwnedFd; 3] {
    std::array::from_fn(|_| {
        OwnedFd::from(std::fs::File::open("/dev/null").expect("open /dev/null"))
    })
}

/// HEX-MAC-15 (the daemon end to end): over a real `SOCK_STREAM` socketpair, the node client's acquire,
/// start (stdio by SCM_RIGHTS), inspect, signal and end each drive the daemon's dispatch; the box takes a
/// pool user and gives it back on delete, each start is apportioned and armed, inspect counts the members,
/// and a deleted box is gone and its user reusable.
#[test]
fn the_daemon_serves_a_box_end_to_end() {
    let (client, server) = rustix::net::socketpair(
        AddressFamily::UNIX,
        SocketType::STREAM,
        SocketFlags::empty(),
        None,
    )
    .expect("socketpair");
    let _ = rustix::net::sockopt::set_socket_nosigpipe(&client, true);
    let _ = rustix::net::sockopt::set_socket_nosigpipe(&server, true);

    // The daemon serves `server` on its own thread until the client closes.
    let handle = thread::spawn(move || {
        let daemon = Daemon::new(Sim::default(), vec![7000, 7001], 64);
        let _ = daemon.serve(server.as_fd());
        daemon
    });

    let client = Helper::over(client, VERSION);
    let b1 = BoxId::new("b1").expect("box id");

    // Acquire takes a pool user (the last pushed, 7001).
    assert_eq!(client.acquire(&b1, 256 * MIB, 16 * MIB).unwrap(), 7001);
    // A box already acquired is refused by name.
    assert!(matches!(
        client.acquire(&b1, 256 * MIB, 16 * MIB),
        Err(hex_api::Refusal::Exists(_))
    ));

    // Two members start; each is apportioned and armed; inspect counts them.
    let pid1 = client
        .start(&b1, &prog(), nulls().each_ref().map(|f| f.as_fd()), true)
        .unwrap();
    let pid2 = client
        .start(&b1, &prog(), nulls().each_ref().map(|f| f.as_fd()), false)
        .unwrap();
    assert!(pid1 > 0 && pid2 > 0 && pid1 != pid2);
    assert_eq!(client.inspect(&b1).unwrap().members, 2);

    // A non-KILL signal reaches the members and leaves them running.
    client.signal(&b1, 15).unwrap();
    assert_eq!(client.inspect(&b1).unwrap().members, 2);

    // Delete ends the box; it is then gone, and its user is back in the pool to be acquired again.
    client.end(&b1).unwrap();
    assert!(matches!(
        client.inspect(&b1),
        Err(hex_api::Refusal::NoSuchBox(_))
    ));
    assert_eq!(client.acquire(&b1, 256 * MIB, 16 * MIB).unwrap(), 7001);

    drop(client);
    let daemon = handle.join().expect("daemon thread");
    // Each start re-apportioned and armed at least once.
    assert!(daemon.ops.inner.lock().unwrap().arm_calls >= 2);
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
