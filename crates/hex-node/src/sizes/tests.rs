//! The pools' bounds (Ada's rule of 2026-11-06; AC-T7.1.3, AC-T7.1.5): together they hold no more than the memory
//! the node may hold, and on the bench host none starts below the constant it replaced.

use std::num::NonZeroUsize;

use super::*;

fn host(memory: u64, parallelism: usize) -> Host {
    Host {
        memory,
        parallelism: NonZeroUsize::new(parallelism).unwrap(),
        threads: FEDORA_THREADS_MAX,
    }
}

/// A thread limit past the socket's need, as a large host's is: Linux sets `threads-max` from memory, one thread's
/// worth for every eight of its kernel stacks' (kernel/fork.c, `set_max_threads`), so a 64 GiB host has hundreds of
/// thousands.
const FEDORA_THREADS_MAX: u64 = 500_000;

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
    assert!(s.netns >= 32, "decision 4's pool: {s:?}");
    assert!(s.workers >= 128, "the socket's workers: {s:?}");
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

/// Ada's rule, 2026-11-06 (AC-T7.1.3): `serve::WORKERS`' 128 was twice a picked in-flight bound. Do: size the
/// socket's workers on hosts of many and of few threads. Expect: a worker for every call the socket admits at once
/// (its connections times their calls in flight), so no admitted call waits for one, but never more than the
/// kernel's thread limit leaves.
#[test]
fn every_call_the_socket_admits_has_a_worker_within_the_thread_limit() {
    let admitted = crate::serve::LIMITS.connections * crate::serve::LIMITS.in_flight;
    assert_eq!(Sizes::of(host(7_500 << 20, 32)).workers, admitted);
    let few = Host {
        threads: 100,
        ..host(7_500 << 20, 32)
    };
    assert_eq!(Sizes::of(few).workers, 100);
    let none = Host {
        threads: 0,
        ..host(7_500 << 20, 32)
    };
    assert_eq!(Sizes::of(none).workers, 1, "one worker, or no call runs");
}
