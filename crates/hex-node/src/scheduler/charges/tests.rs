//! The node's learned-charge shell (joint plan step 14, AC-T7.4.1; `docs/design/learned-charge.md`).

use super::*;
use hex_api::{RuntimeCost, Usage};
use hex_sim::charge::SEEDABLE;
use hex_sim::model::{NodeId, Resources};

const GIB: u64 = 1 << 30;
const WINDOW: Duration = Duration::from_millis(100);

/// A default Shards VM's profile: every CPU of a 16-core host and 8 GiB, its envelope its floor and ceiling.
fn cost() -> RuntimeCost {
    let envelope = CapacityResources {
        cpu_milli: 16_000,
        mem_bytes: 8 * GIB,
        disk_bytes: 0,
    };
    RuntimeCost {
        id: [14; 32],
        floor: envelope,
        ceiling: envelope,
        quantum: CapacityResources {
            cpu_milli: 1,
            mem_bytes: 1,
            disk_bytes: 1,
        },
        guest_overhead: CapacityResources::default(),
        overhead: CapacityResources::default(),
    }
}

fn key(seq: u64) -> Key {
    Key {
        owner: 0,
        epoch: 1,
        seq,
    }
}

fn vm(seq: u64, cpu_usec: u64, memory: u64, allocation: hex_api::Allocation) -> MeasuredVm {
    MeasuredVm {
        key: key(seq),
        class: Class::Shards,
        allocation,
        usage: Usage {
            cpu_usec,
            memory_bytes: memory,
            ..Usage::default()
        },
    }
}

fn node() -> NodeSpec {
    let mut node = NodeSpec::new(
        NodeId(0),
        Resources {
            cpu_milli: 16_000,
            mem_bytes: 64 * GIB,
            ..Resources::default()
        },
    );
    node.costs[Class::Shards.index()] = Some(cost());
    node
}

/// Joint plan step 14 (AC-T7.4.1), C2 through the node's shell: do read a VM booked at its whole receipt at a tenth
/// of a core and 1 GiB for SEEDABLE windows; expect no prior while it is cold, then the class's prior its learned
/// tail, a new receipt booked at it, and the node's capacity the machine's throughout: the room is never shifted (C8).
#[test]
fn w14_the_shell_learns_a_prior_and_leaves_the_capacity_the_machines() {
    let allocation = cost().allocate(CapacityResources::default()).unwrap();
    let mut charges = Charges::new(WINDOW, 8).unwrap();
    let start = Instant::now();
    let mut local = node();
    charges.apply(&mut local).unwrap();
    assert_eq!(local.priors, [None; Class::COUNT]);
    for t in 0..=SEEDABLE as u64 {
        let at = start + WINDOW * u32::try_from(t).unwrap();
        // 100 ms of CPU in each 100 ms window... a tenth of it: 10,000 us.
        charges
            .observe(&[vm(1, t * 10_000, GIB, allocation)], at)
            .unwrap();
        if t == SEEDABLE as u64 - 1 {
            let mut cold = node();
            charges.apply(&mut cold).unwrap();
            assert_eq!(cold.priors, [None; Class::COUNT], "cold at {t}");
            assert_eq!(cold.capacity, node().capacity, "cold at {t}");
        }
    }
    let mut warm = node();
    charges.apply(&mut warm).unwrap();
    let learned = CapacityResources {
        cpu_milli: 100,
        mem_bytes: GIB,
        disk_bytes: 0,
    };
    assert_eq!(
        warm.priors[Class::Shards.index()],
        Some(Prior {
            profile: [14; 32],
            booking: learned,
        })
    );
    assert_eq!(warm.capacity, node().capacity);
    // A new VM's receipt is booked at the prior.
    let booked = hex_sim::model::booked_at(Some(allocation), warm.priors[Class::Shards.index()]);
    assert_eq!(booked.unwrap().booking(), learned);
}

/// Joint plan step 14 (AC-T7.4.1), C3: do stop reading a VM; expect it ended in the shell.
#[test]
fn w14_a_vm_no_longer_read_has_ended() {
    let allocation = cost().allocate(CapacityResources::default()).unwrap();
    let mut charges = Charges::new(WINDOW, 8).unwrap();
    let start = Instant::now();
    charges
        .observe(
            &[vm(1, 0, GIB, allocation), vm(2, 0, GIB, allocation)],
            start,
        )
        .unwrap();
    charges
        .observe(&[vm(1, 1_000, GIB, allocation)], start + WINDOW)
        .unwrap();
    assert!(charges.node.holds(&key(1)));
    assert!(!charges.node.holds(&key(2)));
    assert_eq!(charges.last.len(), 1);
}

/// Joint plan step 14 (AC-T7.4.1, C8): a capacity answer's learned holdings are bounded by a node's most boxes, so a
/// node never has more running VMs to report than its answer may carry.
#[test]
fn w14_learned_holdings_are_bounded_by_a_nodes_boxes() {
    assert_eq!(hex_api::MAX_LEARNED_HOLDINGS, hex_runtime::MAX_BOXES);
}
