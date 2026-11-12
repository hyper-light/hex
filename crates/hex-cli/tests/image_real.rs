//! Joint plan step 12, AC-T7.4.1: the Shards class on Agentfile images (every case of `hex_api::conformance::
//! IMAGE_CASES`, HEX-IMG-1 on) through a real `hex node` on Linux KVM, each box a microVM Shards' D231 boots from a pinned image
//! under shards-init (docs/design/shards-image-class.md), and the run that takes the class's first wait records:
//! `image_boot`, `image_stats` and `image_stop` (bench/native/guest/records.tsv).
//!
//! The node runs in the waits census (`HEX_WAITS_CENSUS`, `hex_node::census`), which arms no wait: there the image
//! waits measure while no record holds them (`hex_runtime::guest::ImageWaits::recorded`), and the census file keeps
//! each answer of `hex.image.boot`, `hex.image.stats` and `hex.image.stop`. At its end the test prints, for each,
//! its count, fastest and slowest (`IMAGE-RECORD`); and, as it goes, every answer the client's own waits record
//! (`IMAGE-STEP`, each call's answer as the client saw it) and each case's verdict and time (`IMAGE-CASE`).
//!
//! HEX-IMG-4 awaits Shards' guest time per vCPU (`Upstream::GuestCpuTimePerVcpu`), which no Shards revision gives
//! yet: it must fail naming that, and every other case pass. Once Shards gives it, HEX-IMG-4 passes and this test
//! says so, failing until its expectation goes.
//!
//! Explicit inputs only, each by its own variable; a missing one fails by name. Run alone, `--ignored`, inside a
//! cgroup delegated to this binary (`HEX_DELEGATED`), as native_firecracker_smoke is:
//! - `HEX_IMAGE`: the image, `NAME@sha256:...`, with a `/bin/sh` (HEX-IMG-3's command), pulled by its digest into
//!   the node's Shards home before the node starts;
//! - `SHARDS_BIN`: the `shards` binary, the node's and the pull's; `SHARDS_KERNEL`: the kernel the run hosts boot
//!   (D231's own, as `shards run` takes it), so no first boot downloads one;
//! - the node's Shards guest configuration, as `hex node` reads it: `HEX_GUEST_KERNEL`, `HEX_GUEST_INITRD`,
//!   `HEX_GUEST_CPUS`, `HEX_GUEST_MEMORY_MIB`, `HEX_GUEST_COST_FILE`, `HEX_GUEST_COST_SHA256` and
//!   `HEX_GUEST_COST_EVIDENCE`;
//! - `HEX_IMAGE_CPU_MILLI` and `HEX_IMAGE_MEMORY_BYTES`: the workload each VM is admitted for, which the guest
//!   cost sizes (`Allocation::vm_size`);
//! - `HEX_IMAGE_WORK`: a directory of this run's own, made here; `HEX_IMAGE_WAIT_MS`: the run's explicit deadline,
//!   for the node's start and the whole suite; `HEX_WAITS_CENSUS_MAX`: the census's most lines.
//!
//! Run: `cargo test --release --locked -p hex-cli --test image_real -- --ignored --nocapture` with those set.
#![cfg(target_os = "linux")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio as HostStdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use hex_api::conformance::{self, Act, Failure, Fixture};
use hex_api::{
    Allocation, Bounds, BoxId, BoxSpec, CapacityResources, Class, ClassSpec, Program, Runtime,
    ShardsImage,
};
use hex_client::Client;

/// The variable `name`'s value, which the run needs: failing by name where it is unset.
fn input(name: &str) -> String {
    std::env::var(name)
        .unwrap_or_else(|_| panic!("{name} is unset (crates/hex-cli/tests/image_real.rs)"))
}

fn number(name: &str) -> u64 {
    input(name)
        .parse()
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// This binary's own cgroup, made a parent: it moves into a leaf, as native_firecracker_smoke does.
fn own_cgroup() -> PathBuf {
    let line = fs::read_to_string("/proc/self/cgroup").unwrap();
    let own = Path::new("/sys/fs/cgroup").join(line.trim().strip_prefix("0::/").unwrap());
    if own.file_name().is_some_and(|name| name == "hex-image-real") {
        return own.parent().unwrap().to_path_buf();
    }
    let leaf = own.join("hex-image-real");
    fs::create_dir_all(&leaf).unwrap();
    fs::write(leaf.join("cgroup.procs"), std::process::id().to_string()).unwrap();
    own
}

/// The node's Shards home and cache (`hex-node`'s `shards_native_paths`: beside its state's records), whose store
/// its image VMs boot from.
fn shards_dirs(state: &Path) -> (PathBuf, PathBuf) {
    (
        state.join("shards-native-home"),
        state.join("shards-native-cache"),
    )
}

/// Pulls `image` by its digest into the node's Shards home, then ends the daemon the pull started there, so no
/// process of the pull's runs beside the node.
fn pull(shards: &Path, image: &str, state: &Path) {
    let (home, cache) = shards_dirs(state);
    let shards_in_home = |args: &[&str]| {
        let status = Command::new(shards)
            .args(args)
            .env("SHARDS_HOME", &home)
            .env("SHARDS_CACHE", &cache)
            .stdin(HostStdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "shards {args:?}: {status}");
    };
    let begun = Instant::now();
    shards_in_home(&["pull", image]);
    eprintln!("IMAGE pulled {image} in {:?}", begun.elapsed());
    shards_in_home(&["daemon", "stop"]);
}

/// A `hex node` with the Shards class, in the waits census, ended by its own PID when dropped: its image VMs' run
/// hosts then see their channel close and end their VMs (D231's run ends with its owner).
struct Node {
    child: Child,
    socket: PathBuf,
}

impl Node {
    fn start(work: &Path, own: &Path, wait: Duration) -> Self {
        let socket = work.join("node.sock");
        let stderr = work.join("node.stderr");
        let begun = Instant::now();
        let mut child = Command::new(env!("CARGO_BIN_EXE_hex"))
            .arg("node")
            .arg("--socket")
            .arg(&socket)
            .arg("--state")
            .arg(work.join("state"))
            .arg("--box-root")
            .arg(own.join("hex-image-node"))
            .env("HEX_WAITS_CENSUS", work.join("census.txt"))
            .env("HEX_WAITS_CENSUS_MAX", input("HEX_WAITS_CENSUS_MAX"))
            .stdin(HostStdio::null())
            .stdout(HostStdio::piped())
            .stderr(fs::File::create(&stderr).unwrap())
            .spawn()
            .unwrap();
        // The node's own "ready <socket>" line is the fact waited on; the run's deadline only guards it.
        let (tx, rx) = mpsc::channel();
        let stdout = child.stdout.take().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let want = format!("ready {}", socket.display());
        let until = begun + wait;
        loop {
            match rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
                Ok(line) if line == want => break,
                Ok(line) => eprintln!("node: {line}"),
                Err(error) => panic!(
                    "node not ready: {error}; stderr: {}",
                    fs::read_to_string(&stderr).unwrap_or_default()
                ),
            }
        }
        eprintln!(
            "IMAGE node ready after {:?} (pid {})",
            begun.elapsed(),
            child.id()
        );
        Self { child, socket }
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        if self.child.try_wait().unwrap().is_none() {
            self.child.kill().unwrap();
        }
        let status = self.child.wait().unwrap();
        eprintln!("IMAGE node ended: {status}");
    }
}

/// Shards image VMs of the pinned image, each admitted for the run's workload: running its members alone, or a
/// command for the act a case asks of its run.
struct ImageBoxes {
    image: String,
    allocation: Allocation,
}

/// The command a run's act is, in the image's own shell: the image cases ask a run to fail with a code (HEX-IMG-3)
/// and a member's program to say words (HEX-IMG-5, refused before it runs).
fn command(act: &Act) -> Vec<String> {
    let shell = |script: String| vec!["/bin/sh".to_owned(), "-c".to_owned(), script];
    match act {
        Act::Fail(code) => shell(format!("exit {code}")),
        Act::Say(words) => vec!["/bin/echo".to_owned(), (*words).to_owned()],
        other => panic!("the image cases ask no {other:?} of a run"),
    }
}

impl ImageBoxes {
    fn box_spec(&self, id: BoxId, bounds: Bounds, command: Option<Vec<String>>) -> BoxSpec {
        BoxSpec {
            id,
            bounds,
            class: ClassSpec::ShardsImage(Box::new(ShardsImage {
                command,
                ..ShardsImage::new(&self.image)
            })),
            owner: None,
            devices: Vec::new(),
            allocation: Some(self.allocation),
            route: None,
        }
    }
}

impl Fixture for ImageBoxes {
    fn spec(&self, id: BoxId, bounds: Bounds) -> BoxSpec {
        self.box_spec(id, bounds, None)
    }

    fn init(&self, id: BoxId, act: &Act) -> BoxSpec {
        let command = match act {
            // A run that waits is the image's members alone: it holds until its stop.
            Act::Wait => None,
            act => Some(command(act)),
        };
        self.box_spec(id, Bounds::default(), command)
    }

    fn program(&self, act: &Act) -> Program {
        let args = command(act);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        Program::of(&args)
    }
}

/// Every answer the client's own waits record, as the census line it is, said as it comes.
fn step(name: &'static str, sample: hex_core::patience::Sample) {
    eprint!(
        "IMAGE-STEP {}",
        hex_core::patience::census_line(name, sample)
    );
}

/// Each image wait's answers in the node's census file, `name nanos` (a refusal `name !nanos`, a run-out
/// `name >nanos`): its count, fastest and slowest answer, the slowest the record's.
fn records(census: &Path) {
    let text = fs::read_to_string(census).unwrap();
    if let Some(full) = text.lines().find(|l| l.starts_with("census full")) {
        eprintln!("IMAGE-RECORD cut: {full}");
    }
    for wait in ["hex.image.boot", "hex.image.stats", "hex.image.stop"] {
        let answers: Vec<u64> = text
            .lines()
            .filter_map(|l| l.strip_prefix(wait)?.strip_prefix(' ')?.parse().ok())
            .collect();
        let other = text
            .lines()
            .filter(|l| {
                l.strip_prefix(wait)
                    .and_then(|rest| rest.strip_prefix(' '))
                    .is_some_and(|v| v.starts_with('!') || v.starts_with('>'))
            })
            .count();
        let (fastest, slowest) = (answers.iter().min(), answers.iter().max());
        eprintln!(
            "IMAGE-RECORD {wait} n={} fastest_ns={fastest:?} slowest_ns={slowest:?} refused_or_ran_out={other}",
            answers.len()
        );
    }
}

#[test]
#[ignore = "native: KVM, a delegated cgroup, a Shards guest configuration and a pinned image (HEX_IMAGE)"]
fn the_shards_image_class_holds_its_contract_through_a_real_node() {
    assert!(
        std::env::var_os("HEX_DELEGATED").is_some(),
        "run inside a cgroup delegated to this binary (HEX_DELEGATED)"
    );
    let image = input("HEX_IMAGE");
    ShardsImage::new(&image).check().unwrap();
    let shards = PathBuf::from(input("SHARDS_BIN"));
    let kernel = PathBuf::from(input("SHARDS_KERNEL"));
    assert!(kernel.is_file(), "SHARDS_KERNEL {kernel:?} is no file");
    let wait = Duration::from_millis(number("HEX_IMAGE_WAIT_MS"));
    assert!(!wait.is_zero(), "HEX_IMAGE_WAIT_MS is 0");
    let work = PathBuf::from(input("HEX_IMAGE_WORK"));
    assert!(!work.exists(), "a run's own directory: {work:?} exists");
    fs::create_dir_all(work.join("state")).unwrap();
    // A VM's start boots it, which the client's learned bounds (an OCI box's records) would refuse first: every
    // wait here measures, and the run's deadline bounds the whole suite instead.
    hex_core::patience::measure_all();
    assert!(
        hex_core::patience::census(step),
        "a census sink was installed before this one"
    );
    // Admitted as the node's own cost sizes a VM, from the same configuration the node reads.
    let cost = hex_runtime::guest::GuestConfig::from_env_with_native_inputs()
        .unwrap()
        .cost;
    let allocation = cost
        .allocate(CapacityResources {
            cpu_milli: number("HEX_IMAGE_CPU_MILLI"),
            mem_bytes: number("HEX_IMAGE_MEMORY_BYTES"),
            disk_bytes: 0,
        })
        .unwrap();
    eprintln!("IMAGE each VM {:?}", allocation.vm_size().unwrap());
    let own = own_cgroup();
    pull(&shards, &image, &work.join("state"));
    let node = Node::start(&work, &own, wait);
    let socket = node.socket.clone();
    let (done, verdicts) = mpsc::channel();
    std::thread::spawn(move || {
        let client = Client::connect(&socket).unwrap();
        assert!(
            client.classes().contains(&Class::Shards),
            "{:?}",
            client.classes()
        );
        let fixture = ImageBoxes { image, allocation };
        let begun = Instant::now();
        let failures = conformance::run_image(&client, &fixture);
        eprintln!(
            "IMAGE-CASE all {} in {:?}",
            conformance::IMAGE_CASES.len(),
            begun.elapsed()
        );
        let left = client.list().unwrap();
        let _ = done.send((failures, left));
    });
    let (failures, left) = verdicts
        .recv_timeout(wait)
        .unwrap_or_else(|e| panic!("the image cases did not end within {wait:?}: {e}"));
    drop(node);
    records(&work.join("census.txt"));
    for case in conformance::IMAGE_CASES {
        let verdict = failures
            .iter()
            .find(|f| f.case == case.id)
            .map_or("passed".to_owned(), |f| format!("failed: {}", f.why));
        eprintln!("IMAGE-CASE {} {verdict}", case.id);
    }
    assert!(left.is_empty(), "a box is left: {left:?}");
    // Guest time per vCPU awaits Shards: HEX-IMG-4 fails naming it, typed, and nothing else fails.
    let (owed, failed): (Vec<&Failure>, Vec<&Failure>) =
        failures.iter().partition(|f| f.case == "HEX-IMG-4");
    assert!(failed.is_empty(), "the image cases failed: {failed:#?}");
    assert!(
        owed.iter()
            .any(|f| f.why.contains("guest time per vCPU awaits")),
        "HEX-IMG-4 passed, or failed for another reason than the guest time Shards owes: {owed:#?}; once Shards \
         gives guest time per vCPU, this expectation goes"
    );
}
