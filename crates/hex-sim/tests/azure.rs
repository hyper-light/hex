//! AC-T7.5.2 on a recorded day where its precondition holds: day 1 of Azure's Packing Trace 2020
//! (Hadary et al., "Protean: VM Allocation Service at Scale", OSDI 2020; CC BY 4.0;
//! `bench/sched/azure_trace.py`, `bench/sched/g4_huawei.md`), each VM request taken as an OCI
//! container, a Firecracker microVM or a Shards microVM by its shape and its owner's kind (G2's
//! rule), each owner a tenant.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::disallowed_macros
    )
)]

mod common;

use common::{describe, recorded, run_within};
use hex_sim::fair::Policy;
use hex_sim::model::{Resources, Time};

const TRACE: &str = "azure-packing2020-day1.trace";
const DAY: Time = 86_400_000_000_000;

/// AC-T7.5.2: do day 1 of Azure's Packing Trace 2020 as `azure_trace.py` makes it (its four busiest
/// tenants whose every request is at most a quarter of a host, as four equal-weight owners, on the
/// 431 hosts at which each asks for more than a quarter over the day; their requests' arrivals,
/// lifetimes and sizes as recorded), measured over the day; expect each owner's part of the held
/// share under contention within 2% of 25% (5,000 ppm) under Hex's policy (bands, weighted DRF,
/// preemption of over-share work), and (non-vacuity) submit order on the same input to miss it.
/// Plain DRF without preemption is measured beside them.
#[test]
fn four_tenants_of_a_recorded_azure_day_each_hold_a_quarter_under_contention() {
    let day = recorded(TRACE);
    let total = day.nodes.iter().fold(Resources::default(), |a, n| {
        a.checked_add(&n.capacity).unwrap()
    });
    // The precondition, over the day: each owner asks for more than a quarter of the cluster.
    let mut asked = vec![(0u128, 0u128); day.owners.len()];
    for w in &day.workloads {
        let held = u128::from(w.runs_for.unwrap().min(DAY - w.arrives));
        let o = &mut asked[usize::try_from(w.tenancy.owner.0).unwrap()];
        o.0 += u128::from(w.need.cpu_milli) * held;
        o.1 += u128::from(w.need.mem_bytes) * held;
    }
    for (o, &(c, m)) in asked.iter().enumerate() {
        let quarter = |cap: u64| u128::from(cap) * u128::from(DAY) / 4;
        assert!(
            c > quarter(total.cpu_milli) || m > quarter(total.mem_bytes),
            "owner {o} asks for no more than a quarter"
        );
    }
    eprintln!(
        "AC-T7.5.2 azure day 1: {} requests of {} owners on {} hosts",
        day.workloads.len(),
        day.owners.len(),
        day.nodes.len()
    );
    let (hex, _) = run_within(&day, Policy::Drf, 1_000_000, DAY);
    let (drf, _) = run_within(&day, Policy::Drf, 0, DAY);
    let (fifo, _) = run_within(&day, Policy::Fifo, 0, DAY);
    describe("AC-T7.5.2 azure hex", &hex);
    describe("AC-T7.5.2 azure plain DRF", &drf);
    describe("AC-T7.5.2 azure FIFO", &fifo);
    assert!(hex.contended_ns > 0, "no contention");
    assert!(
        hex.worst_from_equal_ppm() <= 5_000,
        "Hex's policy misses the bar"
    );
    assert!(
        fifo.worst_from_equal_ppm() > 5_000,
        "FIFO meets the bar too: the test can't fail"
    );
}
