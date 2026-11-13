//! Node VM rows (joint plan step 39): kept apart from box and pool rows, read back as written, replaced whole, and
//! let go.

use super::*;
use hex_api::{Allocation, Exit};
use hex_core::record::{NodeVmPhase, RECORD_VERSION};

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
    assert_eq!(store.node_vm(&booting.run).unwrap(), Some(booting.clone()));
    let ready = record(7, NodeVmPhase::Ready);
    store.put_node_vm(&ready).unwrap();
    assert_eq!(
        store.node_vm(&ready.run).unwrap(),
        Some(ready.clone()),
        "replaced whole"
    );
    store.remove_node_vm(&ready.run).unwrap();
    assert_eq!(store.node_vm(&ready.run).unwrap(), None);
}
