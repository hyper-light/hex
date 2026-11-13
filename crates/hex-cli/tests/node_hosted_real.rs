//! Joint plan step 39, AC-T7.4.1 (docs/design/node-under-shards-init.md §9): Hex's node under shards-init, with Hex's
//! own PID 1 kept, booted to its readiness and measured.
//!
//! The probe boots the node image (`scripts/guest/node.Agentfile`, built by `scripts/guest/node-image.sh`) through
//! shards-api's image boot, as the node's shards-init form is to boot it. Hex's init runs as the run's own command
//! under the nesting grant (Shards' D234), PID 1 of the workload's own PID namespace. Its READY=1 comes only once init
//! has done its duties there and its node and relay have each said they are ready, so a READY is that init's whole
//! boot under shards-init. Each run prints:
//! - its boot, from `Images::boot` to its return;
//! - its readiness, from the same start to READY;
//! - its stop, to the run's end;
//! - the run's CPU time, memory and tasks as its init reads its cgroup (STATS).
//!
//! The probe ends with n, p50, p90, p99 and max of each. The host's own cost of the VM, its VM and network processes,
//! is read beside it (ROOT's).
//!
//! Every wait is for its fact, `Duration::MAX`, and follows its party: the run host's life bounds the boot, and the
//! run's end refuses the readiness, the stats and the stop.
//!
//! Explicit inputs, each a variable; a missing one fails by name:
//! - `SHARDS_BIN`: the `shards` binary;
//! - `SHARDS_HOME`: the Shards home whose store holds `HEX_GUEST_IMAGE`, the node image pinned by digest
//!   (`TAG@sha256:...`, node-image.sh's output);
//! - `HEX_GUEST_CPUS` and `HEX_GUEST_MEMORY_MIB`: the node VM's size, as the direct boot's;
//! - `HEX_NODE_HOSTED_RUNS`: how many runs, n.
//!
//! `SHARDS_KERNEL` and `SHARDS_INIT`, where set, go to the run host as a run takes them (image_real.rs's).
//!
//! Run: `cargo test --release --locked -p hex-cli --test node_hosted_real -- --ignored --nocapture` with those set,
//! where Shards boots VMs: HVF on macOS, WHP on Windows, KVM on Linux. The direct boot's record, its counterpart, is
//! `guest_boot_record.rs`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::ffi::OsString;
use std::time::{Duration, Instant};

use shards_api::image::{
    ImageShards, ImageSpec, Images, Keep, Narrowing, Ready, RunEnd, StatsScope,
};

fn given(name: &str) -> OsString {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| panic!("{name} is unset"))
}

fn number<T: std::str::FromStr>(name: &str) -> T
where
    T::Err: std::fmt::Debug,
{
    given(name).to_str().unwrap().parse().unwrap()
}

/// The node VM's image boot, as docs/design/node-under-shards-init.md §4 has it: Hex's init its command, under the
/// nesting grant, with no deadline (the following wait's), no published port and every grant of the image dropped.
fn spec() -> ImageSpec {
    let mut env = vec![(OsString::from("SHARDS_HOME"), given("SHARDS_HOME"))];
    for name in ["SHARDS_KERNEL", "SHARDS_INIT"] {
        if let Some(value) = std::env::var_os(name) {
            env.push((OsString::from(name), value));
        }
    }
    ImageSpec {
        image: given("HEX_GUEST_IMAGE").into_string().unwrap(),
        vcpus: number("HEX_GUEST_CPUS"),
        memory_mib: number("HEX_GUEST_MEMORY_MIB"),
        command: Some(vec!["/init".to_owned()]),
        members: Vec::new(),
        publish: Vec::new(),
        egress: Narrowing {
            egress: Keep::Only(Vec::new()),
            mcp: Keep::Only(Vec::new()),
            destinations: Keep::Only(Vec::new()),
            dns: false,
            members: None,
        },
        credentials: Vec::new(),
        shares: Vec::new(),
        nesting: true,
        ready_by: None,
        binary: Some(given("SHARDS_BIN").into()),
        env,
    }
}

/// `values`' nearest-rank p50, p90, p99 and max, sorted in place.
fn summary(values: &mut [u128]) -> String {
    values.sort_unstable();
    let rank = |p: usize| values[(values.len() * p).div_ceil(100).max(1) - 1];
    format!(
        "n={} p50={} p90={} p99={} max={}",
        values.len(),
        rank(50),
        rank(90),
        rank(99),
        values[values.len() - 1]
    )
}

#[test]
#[ignore = "a measurement: run with --ignored --nocapture, release, where Shards boots VMs"]
fn the_node_under_shards_init_boots_to_ready_with_hexs_own_init() {
    let spec = spec();
    let n: usize = number("HEX_NODE_HOSTED_RUNS");
    let images = ImageShards::new();
    let (mut boots, mut readies, mut stops, mut memory) = (vec![], vec![], vec![], vec![]);
    println!(
        "NODE-HOSTED image={} vcpus={} memory_mib={} n={n}",
        spec.image, spec.vcpus, spec.memory_mib
    );
    for i in 1..=n {
        let began = Instant::now();
        let guest = images.boot(&spec, Duration::MAX).unwrap();
        let booted = began.elapsed();
        let ready = guest.wait_ready(None, Duration::MAX).unwrap();
        let ready_at = began.elapsed();
        // No deadline was given, so READY=1 is on time, from Hex's init once its node and relay are ready.
        assert_eq!(ready, Some(Ready::OnTime), "run {i}");
        let stats = guest.stats(StatsScope::Run, Duration::MAX).unwrap();
        let stopping = Instant::now();
        guest.stop().unwrap();
        let end = guest.wait_for(Duration::MAX).unwrap();
        let stopped = stopping.elapsed();
        assert_eq!(end, Some(RunEnd::Stopped), "run {i}");
        println!(
            "NODE-HOSTED {i} boot_us={} ready_us={} stop_us={} cpu_ns={:?} memory_bytes={:?} pids={:?}",
            booted.as_micros(),
            ready_at.as_micros(),
            stopped.as_micros(),
            stats.cpu_ns,
            stats.memory_bytes,
            stats.pids
        );
        boots.push(booted.as_micros());
        readies.push(ready_at.as_micros());
        stops.push(stopped.as_micros());
        memory.push(u128::from(stats.memory_bytes.unwrap()));
    }
    println!("NODE-HOSTED boot_us {}", summary(&mut boots));
    println!("NODE-HOSTED ready_us {}", summary(&mut readies));
    println!("NODE-HOSTED stop_us {}", summary(&mut stops));
    println!("NODE-HOSTED memory_bytes {}", summary(&mut memory));
}
