//! A node VM's record (joint plan step 39): its row's bytes round trip, and a record not whole is refused.

use super::*;

/// A node VM's admitted size, its cost's own allocation: two CPUs and the least memory.
fn allocation() -> Allocation {
    let fixed = hex_api::CapacityResources {
        cpu_milli: 2_000,
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

fn record(phase: NodeVmPhase, exit: Option<Exit>) -> NodeVmRecord {
    NodeVmRecord {
        version: RECORD_VERSION,
        run: NodeRun::from_bytes([9; 16]),
        image: "hex-node:rev@sha256:00".to_owned(),
        allocation: allocation(),
        phase,
        diagnostics: Some(PathBuf::from("/state/node-vms/run.log")),
        exit,
    }
}

/// A node VM's record as its row's bytes and back. Do: encode a booting and an ended one, and decode them. Expect: the
/// same records, each row one line.
#[test]
fn a_node_vms_record_round_trips_as_one_line() {
    for kept in [
        record(NodeVmPhase::Booting, None),
        record(NodeVmPhase::Ended, Some(Exit::Code(0))),
    ] {
        let bytes = kept.encode().unwrap();
        assert_eq!(bytes.iter().filter(|b| **b == b'\n').count(), 1);
        assert_eq!(NodeVmRecord::decode(&bytes).unwrap(), kept);
    }
}

/// A record not whole is refused, never guessed at. Do: check one of another version, one naming no image, an ended
/// run with no exit, a booting one with an exit, and bytes cut short. Expect: each refused.
#[test]
fn a_node_vms_record_not_whole_is_refused() {
    let mut other = record(NodeVmPhase::Booting, None);
    other.version += 1;
    let mut unnamed = record(NodeVmPhase::Booting, None);
    unnamed.image.clear();
    for bad in [
        other,
        unnamed,
        record(NodeVmPhase::Ended, None),
        record(NodeVmPhase::Booting, Some(Exit::Code(0))),
    ] {
        assert!(bad.check().is_err(), "{bad:?}");
    }
    let bytes = record(NodeVmPhase::Ready, None).encode().unwrap();
    assert!(NodeVmRecord::decode(&bytes[..bytes.len() - 1]).is_err());
}

/// Joint plan step 39: a node VM's receipt is held to the VM it sizes, not to a workload of none. Do: check a record
/// whose receipt was admitted for a VM of two CPUs and 256 MiB within a profile from none to that, and one booking past
/// what it charges. Expect: the first whole, the second refused.
#[test]
fn a_node_vms_receipt_is_held_to_the_vm_it_sizes() {
    let vm = hex_api::CapacityResources {
        cpu_milli: 2_000,
        mem_bytes: 256 << 20,
        disk_bytes: 0,
    };
    let sized = hex_api::RuntimeCost {
        id: [4; 32],
        floor: hex_api::CapacityResources::default(),
        ceiling: vm,
        quantum: hex_api::CapacityResources {
            cpu_milli: 1_000,
            mem_bytes: 1 << 20,
            disk_bytes: 1,
        },
        guest_overhead: hex_api::CapacityResources::default(),
        overhead: hex_api::CapacityResources::default(),
    }
    .allocate(vm)
    .unwrap();
    let mut kept = record(NodeVmPhase::Ready, None);
    kept.allocation = sized;
    kept.check().unwrap();
    kept.allocation.booked = Some(hex_api::CapacityResources {
        cpu_milli: 3_000,
        ..vm
    });
    assert!(kept.check().is_err());
}
