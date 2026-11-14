//! Node VM rows (joint plan step 39): kept apart from box and pool rows, read back as written, replaced whole, and
//! let go.

use std::path::Path;

use super::*;
use hex_api::{Allocation, Exit, NodeVmPhase};
use hex_core::record::RECORD_VERSION;

/// A node VM's admitted size, its cost's own allocation: one CPU and the least memory.
fn allocation() -> Allocation {
    let fixed = hex_api::CapacityResources {
        cpu_milli: 1000,
        mem_bytes: hex_api::MIN_MEMORY,
        disk_bytes: 0,
    };
    hex_api::RuntimeCost {
        id: [3; 32],
        floor: fixed,
        ceiling: fixed,
        quantum: hex_api::CapacityResources {
            cpu_milli: 1,
            mem_bytes: 1,
            disk_bytes: 1,
        },
        guest_overhead: hex_api::CapacityResources::default(),
        overhead: hex_api::CapacityResources::default(),
    }
    .allocate(hex_api::CapacityResources::default())
    .unwrap()
}

/// A process box's record, ready.
fn a_box(id: &str) -> hex_core::record::BoxRecord {
    let spec = hex_api::BoxSpec {
        route: None,
        devices: Vec::new(),
        id: hex_api::BoxId::new(id).unwrap(),
        bounds: hex_api::Bounds::default(),
        class: hex_api::ClassSpec::Process,
        owner: None,
        allocation: None,
        band: 0,
    };
    hex_core::record::BoxRecord::new(spec, hex_core::record::Phase::Ready, vec![])
}

/// `row` as its store keeps it, at `place` in its node's start order, which its first write was given: a fresh store's
/// first place is one past none.
fn placed(row: &NodeVmRecord, place: u64) -> NodeVmRecord {
    NodeVmRecord {
        started: Some(place),
        ..row.clone()
    }
}

fn record(run: u8, phase: NodeVmPhase) -> NodeVmRecord {
    NodeVmRecord {
        version: RECORD_VERSION,
        run: NodeRun::from_bytes([run; 16]),
        image: "hex-node:rev@sha256:00".to_owned(),
        allocation: allocation(),
        phase,
        diagnostics: None,
        exit: if phase == NodeVmPhase::Ended {
            Some(Exit::Code(0))
        } else {
            None
        },
        last_resort: None,
        started: None,
    }
}

/// ROOT's ruling of 2026-11-13: a node VM's record lies apart from every box's, so no box path sees it. Do: keep a
/// node VM's record beside a box's, read both kinds back, replace it, then let it go. Expect: the box rows list the box
/// alone and the pool rows nothing; the node VM rows the record as written, then as replaced, then none.
#[test]
fn a_node_vms_row_is_kept_apart_from_box_and_pool_rows() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("state"), &dir.path().join("run")).unwrap();
    store.put(&a_box("a-box")).unwrap();
    let booting = record(7, NodeVmPhase::Booting);
    store.put_node_vm(&booting).unwrap();
    let boxes: Vec<_> = store.all().unwrap().into_keys().collect();
    assert_eq!(
        boxes,
        [hex_api::BoxId::new("a-box").unwrap()],
        "the box rows see no node VM"
    );
    assert!(
        store.all_pools().unwrap().is_empty(),
        "the pool rows see no node VM"
    );
    assert_eq!(
        store.node_vm(&booting.run).unwrap(),
        Some(placed(&booting, 1))
    );
    let ready = record(7, NodeVmPhase::Ready);
    store.put_node_vm(&ready).unwrap();
    assert_eq!(
        store.node_vm(&ready.run).unwrap(),
        Some(placed(&ready, 1)),
        "replaced whole"
    );
    store.remove_node_vm(&ready.run).unwrap();
    assert_eq!(store.node_vm(&ready.run).unwrap(), None);
}

/// Joint plan step 39 (ROOT, 2026-11-13: a node VM's row is its receipt's one source): do keep three node VMs' rows,
/// written out of their runs' order, beside a box's, and list them; then put a record file in their directory whose
/// name is no run's. Expect the three listed whole, in their runs' order, the box not among them; then the listing
/// refused for the file whose name is no run's.
#[test]
fn every_node_vms_row_is_listed_in_its_runs_order() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("state"), &dir.path().join("run")).unwrap();
    store.put(&a_box("a-box")).unwrap();
    let rows = [
        record(9, NodeVmPhase::Ended),
        record(2, NodeVmPhase::Booting),
        record(5, NodeVmPhase::Ready),
    ];
    for row in &rows {
        store.put_node_vm(row).unwrap();
    }
    assert_eq!(
        store.all_node_vms().unwrap(),
        [
            placed(&rows[1], 2),
            placed(&rows[2], 3),
            placed(&rows[0], 1)
        ]
    );
    std::fs::write(store.node_vm_directory().join("a-box.json"), b"{}\n").unwrap();
    assert!(matches!(store.all_node_vms(), Err(Refusal::Invalid { .. })));
}

/// Each wait of these cases on a fact of another process's: its word, its end, a row let go.
static FACT: hex_core::patience::Patience =
    hex_core::patience::Patience::measuring("test.hex_node.node_vm_rows");

/// The run of these cases' rows, `n` in each byte.
fn run(n: u8) -> NodeRun {
    NodeRun::from_bytes([n; 16])
}

/// Where a row of run `n` keeps its run host's diagnostics, in the test's directory, written as a run host would.
fn diagnosed(dir: &Path, n: u8, phase: NodeVmPhase) -> NodeVmRecord {
    let mut row = record(n, phase);
    let diagnostics = dir.join(format!("node-{n}.log"));
    std::fs::write(&diagnostics, b"its run host's words\n").unwrap();
    row.diagnostics = Some(diagnostics);
    row
}

fn store_in(dir: &Path) -> Store {
    Store::open(&dir.join("state"), &dir.join("run")).unwrap()
}

/// This test binary run as one of these cases' other processes, `role` naming it, in `dir`.
fn role(role: &str, dir: &Path) -> std::process::Command {
    let full = concat!(module_path!(), "::", "node_vm_role");
    let test = full.split_once("::").map_or(full, |(_, test)| test);
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env("HEX_NODE_VM_ROLE", role)
        .env("HEX_NODE_VM_DIR", dir);
    command
}

/// These cases' other processes, run alone where `HEX_NODE_VM_ROLE` names one (an ordinary run passes at once):
/// - `owner`, a node that runs node VM 7: it writes its row Booting, so it holds its lock, then ends;
/// - `holding owner`, the same, then starts a child of its own that outlives it, which on Unix holds a descriptor of
///   its lock file's open file description, as a child forked without an exec would; then reads its row back, as a
///   node does, says so, and ends when its stdin ends;
/// - `child`: the owner's child, which goes once its test's directory holds `done`, or no longer is, saying so
///   (`bye`).
#[test]
fn node_vm_role() {
    use std::io::{BufRead, Write};
    let (Some(role), Some(dir)) = (
        std::env::var_os("HEX_NODE_VM_ROLE"),
        std::env::var_os("HEX_NODE_VM_DIR"),
    ) else {
        return;
    };
    let dir = PathBuf::from(dir);
    if role == "child" {
        // Its pid, for a run that kills its test to stop it by.
        std::fs::write(dir.join("child.pid"), std::process::id().to_string()).unwrap();
        hex_testkit::waits::until(&FACT, "its test's word to go", || {
            dir.join("done").exists() || !dir.exists()
        })
        .unwrap();
        let _ = std::fs::write(dir.join("bye"), b"");
        return;
    }
    let store = store_in(&dir);
    let row = diagnosed(&dir, 7, NodeVmPhase::Booting);
    store.put_node_vm(&row).unwrap();
    if role == "owner" {
        return;
    }
    let mut child = role_child(&dir);
    #[cfg(unix)]
    if let Some(lock) = store.node_vm_lock(&row.run) {
        child.stdin(lock);
    }
    #[cfg(not(unix))]
    child.stdin(std::process::Stdio::null());
    #[expect(
        clippy::zombie_processes,
        reason = "it outlives this process, as the case asks: it is never waited for here"
    )]
    let _outlives = child.spawn().unwrap();
    assert_eq!(store.node_vm(&row.run).unwrap(), Some(placed(&row, 1)));
    assert_eq!(store.all_node_vms().unwrap(), [placed(&row, 1)]);
    let mut out = std::io::stdout().lock();
    out.write_all(b"HEX_NODE_VM_OWNER_READY\n").unwrap();
    out.flush().unwrap();
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
}

/// The owner's child, its stdio none but what its caller gives its stdin.
fn role_child(dir: &Path) -> std::process::Command {
    let mut child = role("child", dir);
    child
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    child
}

/// Joint plan step 39 (ROOT, 2026-11-13: a row from an earlier boot is ended at start): do write node VM 7's row
/// Ready as a node of an earlier boot of the host left it, its lock held by no process, beside its diagnostics, then
/// start a node. Expect the row let go, its owner ended, and its diagnostics with it.
#[test]
fn w39_a_node_vm_row_of_an_earlier_boot_is_let_go_at_start() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let row = diagnosed(dir.path(), 7, NodeVmPhase::Ready);
    std::fs::create_dir_all(store.node_vm_directory()).unwrap();
    std::fs::write(
        store
            .node_vm_directory()
            .join(format!("{}.json", name(&row.run).unwrap())),
        row.encode().unwrap(),
    )
    .unwrap();
    assert_eq!(store.node_vm(&row.run).unwrap(), Some(row.clone()));
    assert_eq!(
        store.settle_node_vms().unwrap().rows,
        [(row.run, NodeVmSettled::OwnerEnded)]
    );
    assert_eq!(store.node_vm(&row.run).unwrap(), None);
    assert!(!row.diagnostics.unwrap().exists());
}

/// Joint plan step 39 (ROOT, 2026-11-13: a row from an earlier process whose run is gone is ended at start): do run a
/// node that writes node VM 7's row Booting and ends, then start another. Expect the row let go, its owner ended (D231:
/// no microVM outlives its owner), with its lock file and its diagnostics.
#[test]
fn w39_a_node_vm_row_whose_owner_ended_is_let_go_at_start() {
    let dir = tempfile::tempdir().unwrap();
    let ended = role("owner", dir.path()).status().unwrap();
    assert!(ended.success(), "{ended}");
    let store = store_in(dir.path());
    assert_eq!(
        store.node_vm(&run(7)).unwrap().map(|row| row.phase),
        Some(NodeVmPhase::Booting)
    );
    assert_eq!(
        store.settle_node_vms().unwrap().rows,
        [(run(7), NodeVmSettled::OwnerEnded)]
    );
    assert_eq!(store.node_vm(&run(7)).unwrap(), None);
    assert_eq!(
        std::fs::read_dir(store.node_vm_directory())
            .unwrap()
            .count(),
        0,
        "its lock file let go with its row"
    );
    assert!(!dir.path().join("node-7.log").exists());
}

/// Joint plan step 39 (ROOT, 2026-11-13: a row whose run still lives is followed; the lock is its owner's alone, never
/// a child's): do run a node that writes node VM 7's row Booting, starts a child of its own that outlives it (on Unix
/// sharing its lock file's open file description, as a child forked without an exec would), and reads its row back;
/// start another node while it runs; then end the first. Expect the row followed, its receipt still Booting while its
/// owner runs, its lock outliving the owner's reads of its row; then, the owner ended and its child still running, the
/// row let go with its lock file and its diagnostics.
#[test]
fn w39_a_node_vm_row_whose_owner_runs_is_followed_and_no_child_of_it_keeps_its_lock() {
    use std::io::{BufRead, Write};
    let dir = tempfile::tempdir().unwrap();
    eprintln!("node VM rows test directory: {}", dir.path().display());
    let mut owner = role("holding owner", dir.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut said = std::io::BufReader::new(owner.stdout.take().unwrap());
    loop {
        let mut line = String::new();
        assert_ne!(
            said.read_line(&mut line).unwrap(),
            0,
            "the owner ended unready"
        );
        if line.ends_with("HEX_NODE_VM_OWNER_READY\n") {
            break;
        }
    }
    let store = store_in(dir.path());
    assert_eq!(
        store.settle_node_vms().unwrap().rows,
        [(run(7), NodeVmSettled::Followed)]
    );
    assert_eq!(
        store.node_vm(&run(7)).unwrap().map(|row| row.phase),
        Some(NodeVmPhase::Booting),
        "booked while its owner runs"
    );
    let mut ends = owner.stdin.take().unwrap();
    ends.write_all(b"end\n").unwrap();
    drop(ends);
    assert!(owner.wait().unwrap().success());
    hex_testkit::waits::until(&FACT, "the row let go", || {
        store.node_vm(&run(7)).unwrap().is_none()
    })
    .unwrap();
    assert!(
        !dir.path().join("bye").exists(),
        "the owner's child still runs, its lock file's description open where it shares it"
    );
    assert_eq!(
        std::fs::read_dir(store.node_vm_directory())
            .unwrap()
            .count(),
        0,
        "its lock file let go with its row"
    );
    assert!(!dir.path().join("node-7.log").exists());
    std::fs::write(dir.path().join("done"), b"").unwrap();
    hex_testkit::waits::until(&FACT, "the owner's child gone", || {
        dir.path().join("bye").exists()
    })
    .unwrap();
}

/// Joint plan step 39: do write node VM 7's row Ended, its end kept, then start a node. Expect the row let go, with its
/// diagnostics.
#[test]
fn w39_an_ended_node_vm_row_is_let_go_at_start() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let row = diagnosed(dir.path(), 7, NodeVmPhase::Ended);
    store.put_node_vm(&row).unwrap();
    assert_eq!(
        store.settle_node_vms().unwrap().rows,
        [(row.run, NodeVmSettled::Ended)]
    );
    assert_eq!(store.node_vm(&row.run).unwrap(), None);
    assert!(!row.diagnostics.unwrap().exists());
}

/// Joint plan step 39: a node's start settles no row of its own. Do write node VM 7's row Booting in this node, read it
/// back, then settle the rows as a start does. Expect nothing settled, the row kept as written.
#[test]
fn w39_a_nodes_own_node_vm_row_is_never_settled() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let row = diagnosed(dir.path(), 7, NodeVmPhase::Booting);
    store.put_node_vm(&row).unwrap();
    assert_eq!(store.node_vm(&row.run).unwrap(), Some(placed(&row, 1)));
    assert_eq!(store.settle_node_vms().unwrap().rows, []);
    assert_eq!(store.node_vm(&row.run).unwrap(), Some(placed(&row, 1)));
}

/// Why a last resort ended node VM `run`: 3 GiB held of a 2 GiB budget in band 0, its fleet past its budget.
fn end_of(run: &NodeRun) -> LearnedEnd {
    LearnedEnd {
        id: hex_api::LearnedHolder::Node(*run),
        held: 3 << 30,
        budget: 2 << 30,
        band: 0,
        fleet_held: 9 << 30,
        fleet_growth: 1 << 30,
        fleet_budget: 8 << 30,
        unmoved: Some(hex_api::Upstream::LiveMove),
    }
}

/// `row` as an earlier node process left it: its bytes in its place, its lock held by no process.
fn left(store: &Store, row: &NodeVmRecord) {
    std::fs::create_dir_all(store.node_vm_directory()).unwrap();
    std::fs::write(
        store
            .node_vm_directory()
            .join(format!("{}.json", name(&row.run).unwrap())),
        row.encode().unwrap(),
    )
    .unwrap();
}

/// Joint plan step 39, place 1 (C5, intent first; ROOT, 2026-11-13): a node VM row's last-resort end is its learner's
/// to write, and a phase write made from a read before it keeps it, so a reap racing the intent never drops it. Do
/// write node VM 7's row Booting, then Ready; put why the last resort is ending it into the row; then write it Ended
/// from the Ready record read before that end was. Expect the row Ended with its exit, its end kept.
#[test]
fn w39_a_node_vm_rows_phase_write_keeps_its_last_resort() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    store.put_node_vm(&record(7, NodeVmPhase::Booting)).unwrap();
    let ready = record(7, NodeVmPhase::Ready);
    store.put_node_vm(&ready).unwrap();
    let end = end_of(&run(7));
    left(
        &store,
        &NodeVmRecord {
            last_resort: Some(end.clone()),
            ..ready.clone()
        },
    );
    let ended = NodeVmRecord {
        phase: NodeVmPhase::Ended,
        exit: Some(Exit::Signal(9)),
        ..ready
    };
    store.put_node_vm(&ended).unwrap();
    let kept = store.node_vm(&run(7)).unwrap().unwrap();
    assert_eq!(
        (kept.phase, kept.exit, kept.last_resort),
        (NodeVmPhase::Ended, Some(Exit::Signal(9)), Some(end))
    );
}

/// Joint plan step 39, place 1 (ROOT, 2026-11-13: an Ended row's last-resort end, and one whose owner ended, are read
/// back before the row goes): do leave, as an earlier node process would, node VM 4's row Ended and node VM 5's Ready,
/// no process holding its lock, each with why the last resort ended it, and node VM 6's Ended with none; then start a
/// node. Expect all three let go, and the two ends read back in their runs' order.
#[test]
fn w39_a_nodes_start_reads_back_each_let_go_rows_last_resort_end() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut ended = record(4, NodeVmPhase::Ended);
    ended.last_resort = Some(end_of(&run(4)));
    let mut unowned = record(5, NodeVmPhase::Ready);
    unowned.last_resort = Some(end_of(&run(5)));
    for row in [&ended, &unowned, &record(6, NodeVmPhase::Ended)] {
        left(&store, row);
    }
    let settled = store.settle_node_vms().unwrap();
    assert_eq!(
        settled.rows,
        [
            (run(4), NodeVmSettled::Ended),
            (run(5), NodeVmSettled::OwnerEnded),
            (run(6), NodeVmSettled::Ended),
        ]
    );
    assert_eq!(settled.ends, [end_of(&run(4)), end_of(&run(5))]);
    assert!(store.all_node_vms().unwrap().is_empty());
}

/// Joint plan step 39, place 3 (ROOT, 2026-11-13: a node VM's start place is its real start order among the host's
/// holders): do write node VM 7's row Booting, then Ready; then reopen the store as a restarted node does. Expect its row
/// to hold a place in the node's start order from its first write, the same place after its Ready write, and the next
/// place given past it, before and after the reopening.
#[test]
fn w39_a_node_vm_rows_place_in_its_nodes_start_order_is_given_at_its_first_write() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    store.put_node_vm(&record(7, NodeVmPhase::Booting)).unwrap();
    let place = store.node_vm(&run(7)).unwrap().unwrap().started;
    assert!(place.is_some(), "no place given at its first write");
    store.put_node_vm(&record(7, NodeVmPhase::Ready)).unwrap();
    assert_eq!(store.node_vm(&run(7)).unwrap().unwrap().started, place);
    assert!(Some(store.next_start().unwrap()) > place);
    drop(store);
    let reopened = store_in(dir.path());
    assert!(
        Some(reopened.next_start().unwrap()) > place,
        "the order runs on past the node VM's place"
    );
}

/// Joint plan step 39 (ROOT, 2026-11-13): one node VM serves a node's oci class, its one guest, so a node holds one node
/// VM Ready at a time, and a second is a damaged row, refused, typed. Do write node VM 1's row Booting, then Ready; node
/// VM 2's Booting, then Ready; then node VM 1's Ended, and node VM 2's Ready again. Expect node VM 2's first Ready refused
/// as a damaged row, naming the one guest, its row left Booting; then, node VM 1 ended, node VM 2 Ready.
#[test]
fn w39_a_node_holds_one_node_vm_ready_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    store.put_node_vm(&record(1, NodeVmPhase::Booting)).unwrap();
    store.put_node_vm(&record(1, NodeVmPhase::Ready)).unwrap();
    store.put_node_vm(&record(2, NodeVmPhase::Booting)).unwrap();
    let refused = store.put_node_vm(&record(2, NodeVmPhase::Ready));
    assert!(
        matches!(&refused, Err(Refusal::Invalid { why, .. }) if why.contains("one guest")),
        "{refused:?}"
    );
    assert_eq!(
        store.node_vm(&run(2)).unwrap().map(|row| row.phase),
        Some(NodeVmPhase::Booting)
    );
    store.put_node_vm(&record(1, NodeVmPhase::Ended)).unwrap();
    store.put_node_vm(&record(2, NodeVmPhase::Ready)).unwrap();
    assert_eq!(
        store.node_vm(&run(2)).unwrap().map(|row| row.phase),
        Some(NodeVmPhase::Ready)
    );
}

/// The node VM store's node_vm_unfollowed line (CLAUDE.md §10; ROOT, 2026-11-14): its run and its why JSON strings.
/// Do: form it for run 0xab's owner, refused in words holding a quote, a backslash and a newline. Expect: one line, the
/// run's name and the words JSON strings.
#[test]
fn an_unfollowed_node_vms_line_is_one_line_its_values_json_strings() {
    let why = hex_api::Refusal::Broken("a \"b\" \\ c\nd".into());
    assert_eq!(
        unfollowed_line(&run(0xab), &why),
        format!(
            "hex.guest event=node_vm_unfollowed run=\"{}\" why=\"the node agent's state is unusable: a \\\"b\\\" \\\\ \
             c\\nd\"",
            "ab".repeat(16)
        )
    );
}
