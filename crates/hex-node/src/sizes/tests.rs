//! The pools' bounds (Ada's rule of 2026-11-06; AC-T7.1.3, AC-T7.1.5): together they hold no more than the memory
//! the node may hold, and on the bench host none starts below the constant it replaced.

use std::num::NonZeroUsize;

use super::*;

fn host(memory: u64, parallelism: usize) -> Host {
    Host {
        memory,
        parallelism: NonZeroUsize::new(parallelism).unwrap(),
    }
}

/// Do: size the pools on hosts of any memory. Expect: the pools at their bounds hold at most that memory, but for
/// the one seed every node keeps.
#[test]
fn the_pools_at_their_bounds_hold_no_more_than_the_node_may() {
    for memory in [
        0,
        1,
        1 << 20,
        100 << 20,
        1 << 30,
        7_500 << 20,
        64 << 30,
        u64::MAX / 2,
    ] {
        let s = Sizes::of(host(memory, 8));
        let seed_floor = if s.seeds.get() == 1 { SEED_BYTES } else { 0 };
        assert!(
            s.bytes() <= memory.saturating_add(seed_floor),
            "{s:?} holds {} of {memory}",
            s.bytes()
        );
    }
}

/// Do: size the pools on fedora, the bench host, as its bench units run the node (32 hardware threads, 8 GiB of
/// `MemoryMax` with about 7.5 GiB left). Expect: no bound or start under the constant it replaced: 256 idle cgroups
/// and network namespaces, 32 seeds and ipc and uts pairs, 8 cgroups made ahead, each owner's 32.
#[test]
fn on_the_bench_host_no_pool_is_smaller_than_the_constant_it_replaced() {
    let s = Sizes::of(host(7_500 << 20, 32));
    assert_eq!(s.start, 32);
    assert!(s.idle_cgroups >= 256, "{s:?}");
    assert!(s.idle_netns >= 256, "{s:?}");
    assert!(s.seeds.get() >= 32, "{s:?}");
    assert!(s.ipc_uts >= 32, "{s:?}");
    assert!(s.ahead >= 32, "{s:?}");
    assert_eq!(s.procs_alive(), s.idle_cgroups + s.ahead);
}

/// Do: size the pools on a host with no memory to spare. Expect: one seed and nothing else held ahead.
#[test]
fn a_host_with_no_memory_to_spare_keeps_one_seed_and_nothing_ahead() {
    let s = Sizes::of(host(0, 4));
    assert_eq!(s.seeds.get(), 1);
    assert_eq!(
        (s.idle_cgroups, s.idle_netns, s.ahead, s.ipc_uts),
        (0, 0, 0, 0)
    );
}

/// Do: read this host's facts. Expect: some memory and a parallelism, as every Linux host gives.
#[test]
fn this_host_gives_its_facts() {
    let h = Host::now().unwrap();
    assert!(h.memory > 0);
    assert!(h.parallelism.get() >= 1);
}
