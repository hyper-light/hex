//! Exact ROOT005 native original/control port. Production snapshot/cleanup behavior is unchanged.
//! Kernel peer-PID is UNAVAILABLE here; the private fixture joins native Child receipts instead.
//! The original control models one explicit interleaving: the watcher has not run yet.
use super::*;
use std::io::{Read as _, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Command, Stdio};

use hex_api::{BackingKey, BackingPhase, CapacityResources, Execution, Proc as _, RuntimeCost};
use hex_core::record::{NativeRecord, PoolRecord, RECORD_VERSION};
use shards_api::{Identity, ManagedProcess as _, TerminalScope};

use super::super::managed::{Controller, Ownership, Scope};
use super::super::proc::{Prepared, Reaping};

// Same test hang guard and evidence as macos/tests.rs; not a runtime reaction allowance.
const GUARD: Duration = Duration::from_millis(39_507);
const WORKER: &str = "macos::process::scope_completion_tests::native_scope_worker";
const ROLE: &str = "HEX_SCOPE_CONTROL_ROLE";
const ENDPOINT: &str = "HEX_SCOPE_CONTROL_ENDPOINT";

fn command(role: &str, endpoint: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", WORKER, "--ignored", "--nocapture"])
        .env(ROLE, role)
        .env(ENDPOINT, endpoint)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    command
}

fn incoming(listener: &UnixListener, deadline: Instant) -> UnixStream {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "private worker connection deadline");
        let timeout = Timespec::try_from(left).unwrap();
        let mut descriptors = [PollFd::new(listener, PollFlags::IN)];
        match poll(&mut descriptors, Some(&timeout)) {
            Err(rustix::io::Errno::INTR) => continue,
            Ok(0) => panic!("private worker connection deadline"),
            Ok(_) => {}
            Err(error) => panic!("private worker connection readiness: {error}"),
        }
        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_read_timeout(Some(deadline.saturating_duration_since(Instant::now())))
                    .unwrap();
                stream.set_write_timeout(Some(GUARD)).unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(error) => panic!("private worker accept: {error}"),
        }
    }
}

#[derive(Debug)]
struct Birth {
    pid: i32,
    started: u64,
    session: i32,
}

fn read_birth(stream: &mut UnixStream, role: u8, expected_pid: i32) -> Birth {
    let mut bytes = [0; 17];
    stream.read_exact(&mut bytes).unwrap();
    assert_eq!(bytes[0], role);
    let birth = Birth {
        pid: i32::from_be_bytes(bytes[1..5].try_into().unwrap()),
        started: u64::from_be_bytes(bytes[5..13].try_into().unwrap()),
        session: i32::from_be_bytes(bytes[13..17].try_into().unwrap()),
    };
    assert_eq!(
        sys::peer_euid(&*stream).unwrap(),
        rustix::process::geteuid().as_raw()
    );
    // ROOT005 has no kernel peer-PID wrapper. This TEST-ONLY private endpoint joins the
    // direct Child receipt (or its immutable controller's spawned Child receipt), not a
    // kernel peer-PID observation. Do not describe it as the stronger old cc2 fact.
    assert_eq!(birth.pid, expected_pid);
    assert!(birth.pid > 0 && birth.started > 0 && birth.session > 0);
    let actual = sys::rusage(birth.pid).unwrap().unwrap();
    assert_eq!(actual.started, birth.started);
    assert!(!actual.ended);
    birth
}

fn tell_birth(stream: &mut UnixStream, role: u8) {
    let pid = rustix::process::getpid().as_raw_nonzero().get();
    let actual = sys::rusage(pid).unwrap().unwrap();
    assert!(actual.started > 0 && !actual.ended);
    let session = rustix::process::getsid(None)
        .unwrap()
        .as_raw_nonzero()
        .get();
    let mut bytes = [0; 17];
    bytes[0] = role;
    bytes[1..5].copy_from_slice(&pid.to_be_bytes());
    bytes[5..13].copy_from_slice(&actual.started.to_be_bytes());
    bytes[13..17].copy_from_slice(&session.to_be_bytes());
    stream.write_all(&bytes).unwrap();
}

fn pool() -> PoolRecord {
    let fixed = CapacityResources {
        cpu_milli: 1000,
        mem_bytes: hex_api::MIN_MEMORY,
        disk_bytes: 0,
    };
    let cost = RuntimeCost {
        id: [3; 32],
        floor: fixed,
        ceiling: fixed,
        quantum: CapacityResources {
            cpu_milli: 1,
            mem_bytes: 1,
            disk_bytes: 1,
        },
        guest_overhead: CapacityResources::default(),
        overhead: CapacityResources::default(),
    };
    PoolRecord {
        version: RECORD_VERSION,
        key: BackingKey {
            pool: [5; 32],
            generation: [7; 16],
        },
        execution: Execution::ShardsShared,
        cost,
        allocation: Some(cost.allocate(CapacityResources::default()).unwrap()),
        devices: Vec::new(),
        phase: BackingPhase::Granted,
        init: None,
        exit: None,
    }
}

fn unobserved_box(controller: &Birth) -> ProcessBox {
    let record = pool();
    record.check().unwrap();
    let native = NativeRecord::Pool(&record);
    // These are only existing watcher test model seeds. No watcher starts and this ROOT source
    // has no managed CPU control API; no enforcement/performance conclusion uses these values.
    let one = std::num::NonZeroU64::new(1).unwrap();
    let shared = Shared::new(one, one, Duration::from_nanos(1)).unwrap();
    let scope = Scope::new(native, Ownership::Child).unwrap();
    scope.held(native.owner()).unwrap();
    scope.running(native.owner()).unwrap();
    let bounds = Bounds::default();
    bounds.check().unwrap();
    let boxed = ProcessBox::prepared(
        native.owner().try_owned().unwrap(),
        &bounds,
        &shared,
        Some((controller.pid, controller.started)),
        Some(scope),
    )
    .unwrap();
    boxed
        .identity
        .set(Identity {
            pid: u32::try_from(controller.pid).unwrap(),
            started: controller.started,
            vsock: "/test-only-not-an-incarnation".into(),
        })
        .unwrap();
    let (process, registered) =
        Prepared::new()
            .unwrap()
            .bind(controller.pid, None, Reaping::Retained);
    registered.unwrap();
    *boxed.controller.lock().unwrap() = Controller::Held {
        process,
        failure: None,
    };
    assert!(boxed.watcher.lock().unwrap().is_none());
    boxed
}

/// AC-T7.4.1: an unobserved live descendant outside the PGID prevents whole-scope release.
/// This port runs exact ROOT005 production, independently of the old 2ef/858 original.
#[test]
#[ignore = "ROOT-only native original; spawns three exact owned test processes, no VM/GPU"]
fn unobserved_escaped_descendant_prevents_scope_release() {
    sys::retained_children().unwrap();
    let directory = tempfile::Builder::new()
        .prefix("hx-scope-")
        .tempdir()
        .unwrap();
    let endpoint = directory.path().join("s");
    let listener = UnixListener::bind(&endpoint).unwrap();
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now().checked_add(GUARD).unwrap();
    // This unreaped direct child pins its PID until the one BoxProc waiter consumes its exit.
    let controller_child = command("controller", &endpoint)
        .process_group(0)
        .spawn()
        .unwrap();
    let mut control = incoming(&listener, deadline);
    let controller_pid = i32::try_from(controller_child.id()).unwrap();
    let controller_birth = sys::rusage(controller_pid).unwrap().unwrap();
    assert!(controller_birth.started > 0 && !controller_birth.ended);
    let controller = read_birth(&mut control, b'C', controller_pid);
    assert_eq!(controller.started, controller_birth.started);
    assert_eq!(
        controller_child.id(),
        u32::try_from(controller.pid).unwrap()
    );
    assert_eq!(sys::group_of(controller.pid).unwrap(), Some(controller.pid));
    let mut child_pid = [0; 4];
    control.read_exact(&mut child_pid).unwrap();
    let child_pid = i32::try_from(u32::from_be_bytes(child_pid)).unwrap();
    assert!(child_pid > 0 && child_pid != controller.pid);
    let child_birth = sys::rusage(child_pid).unwrap().unwrap();
    assert!(child_birth.started > 0 && !child_birth.ended);
    let mut escaped = incoming(&listener, deadline);
    let escape = read_birth(&mut escaped, b'E', child_pid);
    assert_eq!(escape.started, child_birth.started);
    assert_ne!(escape.session, controller.session);
    assert_eq!(sys::group_of(escape.pid).unwrap(), Some(escape.pid));
    assert_ne!(escape.pid, controller.pid);
    let ended = Kqueue::new().unwrap();
    assert!(!ended.watch_end(escape.pid).unwrap());
    let boxed = unobserved_box(&controller);
    control.write_all(b"X").unwrap();
    let controller_exit = {
        let mut held = boxed.controller.lock().unwrap();
        let Controller::Held { process, .. } = &mut *held else {
            panic!("missing same native waiter")
        };
        process
            .wait_for(deadline.saturating_duration_since(Instant::now()))
            .unwrap()
    };
    let observation = boxed.terminal();
    let controller_fact = sys::rusage(controller.pid).unwrap();
    let escape_fact = sys::rusage(escape.pid).unwrap();
    let mut group = sys::listing().unwrap();
    let group_full = sys::group(controller.pid, &mut group).unwrap();
    let group_facts: Vec<_> = group
        .iter()
        .map(|pid| (*pid, sys::rusage(*pid).unwrap()))
        .collect();
    eprintln!(
        "ROOT005 ORIGINAL scope facts: kernel_peer_pid=UNAVAILABLE controller_child_id={} controller={controller:?} exit={controller_exit:?} native={controller_fact:?} group={group_facts:?} full={group_full} escaped={escape:?} native={escape_fact:?} observed={observation:?}",
        controller_child.id()
    );

    // Cleanup precedes the oracle. No PID/PGID kill or inferred release: the exact private peer
    // gets its lifeline command, its pre-registered native exit is observed, then the one native
    // waiter reaps the direct controller. A missing absence witness is an incomplete fixture.
    let release = escaped.write_all(b"X");
    drop(escaped);
    let mut events = Vec::new();
    ended
        .wait(
            None,
            Some(deadline.saturating_duration_since(Instant::now())),
            &mut events,
        )
        .unwrap();
    let escaped_exit = events
        .iter()
        .any(|event| matches!(event, sys::Event::Exit { pid, .. } if *pid == escape.pid));
    let controller_reap = {
        let mut held = boxed.controller.lock().unwrap();
        let Controller::Held { process, .. } = &mut *held else {
            panic!("missing same native waiter")
        };
        process.finalize_retained()
    };
    let after = sys::rusage(escape.pid).unwrap();
    let absent = after.is_none_or(|usage| usage.started != escape.started);
    eprintln!(
        "CLEANUP scope facts: lifeline={release:?} escaped_exit={escaped_exit} controller_reap={controller_reap:?} escaped_after={after:?} absence={absent}"
    );
    assert!(
        release.is_ok() && escaped_exit && controller_reap.is_ok() && absent,
        "INCOMPLETE cleanup; do not attribute this as a semantic red/green"
    );
    assert!(controller_exit.is_some());
    assert!(
        controller_fact.is_some_and(|usage| usage.started == controller.started && usage.ended)
    );
    assert!(escape_fact.is_some_and(|usage| usage.started == escape.started && !usage.ended));
    assert!(!group_full);
    assert!(
        group_facts
            .iter()
            .all(|(_, usage)| usage.is_some_and(|usage| usage.ended))
    );
    assert_eq!(boxed.status().unwrap().escaped, 0);
    assert!(
        !matches!(
            observation,
            Ok(Some(shards_api::ManagedEnd {
                scope: TerminalScope::Released,
                ..
            }))
        ),
        "live escaped birth was released from controller exit plus PGID observation"
    );
}

/// The helper roles have no VM/GPU/native-runtime effects. Only the parent above starts them.
#[test]
#[ignore = "internal ROOT native fixture role; requires exact parent-owned endpoint"]
fn native_scope_worker() {
    let role = std::env::var(ROLE).expect("internal fixture role");
    if role == "nproc" {
        nproc_control();
        return;
    }
    let endpoint = std::env::var_os(ENDPOINT).expect("internal fixture endpoint");
    if role == "escape" {
        rustix::process::setsid().unwrap();
    }
    let mut stream = UnixStream::connect(Path::new(&endpoint)).unwrap();
    stream.set_read_timeout(Some(GUARD)).unwrap();
    stream.set_write_timeout(Some(GUARD)).unwrap();
    match role.as_str() {
        "controller" => {
            tell_birth(&mut stream, b'C');
            let child = command("escape", Path::new(&endpoint)).spawn().unwrap();
            stream.write_all(&child.id().to_be_bytes()).unwrap();
            let mut command = [0];
            stream.read_exact(&mut command).unwrap();
            assert_eq!(command, *b"X");
            // The escaped child is intentionally live. Its parent's exit reassigns its reaping
            // to launchd; its exact private lifeline remains owned by the root fixture.
        }
        "escape" => {
            tell_birth(&mut stream, b'E');
            let mut command = [0];
            stream.read_exact(&mut command).unwrap();
            assert_eq!(command, *b"X");
        }
        other => panic!("unknown internal fixture role {other}"),
    }
}

fn nproc_control() {
    use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
    assert_ne!(
        rustix::process::getuid().as_raw(),
        0,
        "root bypasses XNU NPROC"
    );
    assert_ne!(
        rustix::process::geteuid().as_raw(),
        0,
        "root can raise hard limits"
    );
    let zero = Rlimit {
        current: Some(0),
        maximum: Some(0),
    };
    setrlimit(Resource::Nproc, zero).unwrap();
    assert_eq!(getrlimit(Resource::Nproc), zero);
    match Command::new(std::env::current_exe().unwrap())
        .arg("--list")
        .spawn()
    {
        Err(error) => assert_eq!(error.raw_os_error(), Some(libc::EAGAIN)),
        Ok(mut child) => {
            let mut waiter =
                super::super::proc::BoxProc::watch(i32::try_from(child.id()).unwrap(), None)
                    .unwrap();
            let _ = child.kill();
            assert!(waiter.wait_for(GUARD).unwrap().is_some());
            panic!("NPROC0 permitted a process creation");
        }
    }
    assert_eq!(
        setrlimit(
            Resource::Nproc,
            Rlimit {
                current: Some(1),
                maximum: Some(1)
            }
        ),
        Err(rustix::io::Errno::PERM)
    );
    assert_eq!(getrlimit(Resource::Nproc), zero);
    let child = std::thread::Builder::new().spawn(|| 13u64).unwrap();
    assert_eq!(child.join().unwrap(), 13);
}

/// Independent kernel primitive check; it does not certify HVF/Vulkan or pre-seal ownership.
#[test]
#[ignore = "ROOT-only native child: changes only its own irreversible hard NPROC limit"]
fn native_nproc_zero_refuses_spawn_and_hard_raise_but_preserves_threads() {
    let child = command("nproc", Path::new("unused")).spawn().unwrap();
    let mut waiter =
        super::super::proc::BoxProc::watch(i32::try_from(child.id()).unwrap(), None).unwrap();
    assert_eq!(
        waiter.wait_for(GUARD).unwrap(),
        Some(hex_api::Exit::Code(0))
    );
}
