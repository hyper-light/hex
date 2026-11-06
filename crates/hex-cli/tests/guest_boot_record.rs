//! A host class's boot-to-ready and stop record (native plan step 5, constants; the lead's rule that a
//! class's seeds come from its own records, bench/native/guest/records.tsv): boots the guest image through
//! `shards_api::Shards` N times, as `hex_runtime::guest::GuestRuntime` does, and prints each boot's time from
//! `boot` to the relay's first bytes on its control port (the connection's token), and each stop's time
//! from `stop` to the guest's end. Portable: it is how a host without Python (jupiter) takes its record.
//!
//! Ignored by default: `cargo test -p hex-cli --release --test guest_boot_record -- --ignored --nocapture`,
//! with `SHARDS_BIN`, `HEX_GUEST_KERNEL`, `HEX_GUEST_INITRD` and `BOOT_RECORD_N` (default 40).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use hex_core::stream;
use hex_runtime::guest::{CLASS, GuestConfig};
use shards_api::Guests;

fn given(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} is unset")))
}

#[test]
#[ignore = "a measurement: run with --ignored --nocapture, release, where a guest boots"]
fn boot_to_ready_and_stop() {
    let config = GuestConfig {
        binary: given("SHARDS_BIN"),
        kernel: given("HEX_GUEST_KERNEL"),
        initrd: given("HEX_GUEST_INITRD"),
        vcpus: shards_api::max_vcpus(),
        // Shards' least guest and twice HEX-GUEST-19's measured peak, as guest_real sizes it.
        memory_mib: shards_api::min_memory_mib(std::env::consts::ARCH) + 2 * 284,
        shares: Vec::new(),
        staged: hex_client::guest::stage::STAGED.to_owned(),
    };
    let n: usize = std::env::var("BOOT_RECORD_N").map_or(40, |v| v.parse().unwrap());
    let shards = shards_api::Shards::new();
    println!("RECORD class={CLASS} n={n}");
    for i in 1..=n {
        let began = Instant::now();
        let guest = shards.boot(&config.spec()).unwrap();
        // The relay's first bytes, its token: the guest is ready (the facts alone; no clock).
        let mut conn = guest.connect(stream::CONTROL_PORT, Duration::MAX).unwrap();
        let mut token = [0u8; stream::TOKEN_BYTES];
        conn.read_exact(&mut token).unwrap();
        let ready = began.elapsed();
        drop(conn);
        let stopping = Instant::now();
        guest.stop().unwrap();
        let end = guest.wait_for(Duration::MAX).unwrap();
        let stopped = stopping.elapsed();
        println!(
            "RECORD {i} boot_ready_us={} stop_us={} end={end:?}",
            ready.as_micros(),
            stopped.as_micros()
        );
    }
}
