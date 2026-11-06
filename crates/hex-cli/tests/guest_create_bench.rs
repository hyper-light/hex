//! HEX-GUEST-22 (native plan step 5, docs/design/shards-guest.md §4a): what a create of a multi-layer image
//! costs through a real Shards guest, the record the guest's layer store is measured by. Each round creates
//! and deletes an `oci` box of each image in turn (interleaved), through `GuestRuntime` from this host, and
//! times each create: the first of an image after the guest's boot, and every later one. It prints each
//! create's milliseconds and, per image, p50 / p99 / max of the later ones.
//!
//! Given as (else it says SKIP and why):
//! - a guest: `HEX_GUEST_KERNEL`, `HEX_GUEST_INITRD`, `SHARDS_BIN`, as HEX-GUEST-19;
//! - `HEX_GUEST22_IMAGES`: a folder of images, each `<name>/layers.txt` naming its layer tarballs lowest
//!   first (bench/native/guest-tree's layout);
//! - `HEX_GUEST22_ROUNDS`: how many rounds, the record's N.
//!
//! Run: `cargo test --release -p hex-cli --test guest_create_bench -- --nocapture` with those set.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use hex_api::{Bounds, BoxId, BoxSpec, ClassSpec, Network, OciSpec, Program, Rootfs, Runtime};
use hex_runtime::guest::{GuestConfig, GuestRuntime};

fn given(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

/// The value at the `percent`th percentile of `sorted`, by nearest rank.
fn at(sorted: &[f64], percent: usize) -> f64 {
    let rank = (sorted.len() * percent)
        .div_ceil(100)
        .clamp(1, sorted.len());
    sorted[rank - 1]
}

#[test]
fn a_multi_layer_create_through_a_real_guest_is_timed() {
    let (Some(kernel), Some(initrd), Some(binary), Some(images), Some(rounds)) = (
        given("HEX_GUEST_KERNEL"),
        given("HEX_GUEST_INITRD"),
        given("SHARDS_BIN"),
        given("HEX_GUEST22_IMAGES"),
        std::env::var("HEX_GUEST22_ROUNDS").ok(),
    ) else {
        eprintln!(
            "SKIP: no guest or images given (HEX_GUEST_KERNEL, HEX_GUEST_INITRD, SHARDS_BIN, \
             HEX_GUEST22_IMAGES, HEX_GUEST22_ROUNDS)"
        );
        return;
    };
    let rounds: usize = rounds.parse().unwrap();
    let mut names: Vec<String> = std::fs::read_dir(&images)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| images.join(n).join("layers.txt").exists())
        .collect();
    names.sort();
    let layers: Vec<Vec<PathBuf>> = names
        .iter()
        .map(|n| {
            std::fs::read_to_string(images.join(n).join("layers.txt"))
                .unwrap()
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| images.join(n).join(l))
                .collect()
        })
        .collect();
    // The guest's tmpfs holds half its memory (tmpfs(5)): twice the images' layers, unpacked, and Shards'
    // least besides, so the largest round fits.
    let unpacked: u64 = layers
        .iter()
        .flatten()
        .map(|l| {
            std::io::copy(
                &mut flate2::read::GzDecoder::new(std::fs::File::open(l).unwrap()),
                &mut std::io::sink(),
            )
            .unwrap()
        })
        .sum();
    let config = GuestConfig {
        binary,
        kernel,
        initrd,
        vcpus: 1,
        memory_mib: (shards_api::min_memory_mib(std::env::consts::ARCH)
            + (2 * unpacked).div_ceil(1 << 20))
        .next_multiple_of(2),
        shares: Vec::new(),
        staged: hex_client::guest::stage::STAGED.to_owned(),
    };
    eprintln!(
        "images {names:?}, {rounds} rounds, guest memory {} MiB",
        config.memory_mib
    );
    let node = GuestRuntime::new(Arc::new(shards_api::Shards::new()), config);
    let mut times: Vec<Vec<f64>> = vec![Vec::new(); names.len()];
    for round in 0..rounds {
        for (i, name) in names.iter().enumerate() {
            let id = BoxId::new(&format!("b{round}-{i}")).unwrap();
            let spec = BoxSpec {
                id: id.clone(),
                bounds: Bounds::default(),
                class: ClassSpec::Oci(Box::new(OciSpec {
                    rootfs: Rootfs::Layers(layers[i].clone()),
                    init: Program::of(&["/bin/true"]),
                    mounts: Vec::new(),
                    hostname: None,
                    network: Network::Private,
                    hosts: Vec::new(),
                })),
            };
            let began = Instant::now();
            node.create(spec)
                .unwrap_or_else(|why| panic!("{name}: create: {why}"));
            let ms = began.elapsed().as_secs_f64() * 1e3;
            node.delete(&id).unwrap();
            eprintln!("round {round} {name}: create {ms:.1} ms");
            times[i].push(ms);
        }
    }
    for (i, name) in names.iter().enumerate() {
        let first = times[i][0];
        let mut later = times[i][1..].to_vec();
        later.sort_by(f64::total_cmp);
        if later.is_empty() {
            eprintln!("{name}: first {first:.1} ms");
            continue;
        }
        eprintln!(
            "{name}: first {first:.1} ms; later (N={}) p50 {:.1} p99 {:.1} max {:.1} ms",
            later.len(),
            at(&later, 50),
            at(&later, 99),
            later[later.len() - 1]
        );
    }
}
