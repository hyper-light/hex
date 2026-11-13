//! Joint plan step 12, AC-T7.4.1: the Shards class on Agentfile images (every case of `hex_api::conformance::
//! IMAGE_CASES`, HEX-IMG-1 on) through a real `hex node` on Linux KVM, macOS HVF or Windows WHP, each box a microVM
//! Shards' D231 boots from a pinned image under shards-init (docs/design/shards-image-class.md), and the run that
//! takes the class's first wait records: `image_boot`, `image_stats` and `image_stop`
//! (bench/native/guest/records.tsv).
//!
//! The node runs in the waits census (`HEX_WAITS_CENSUS`, `hex_node::census`), which arms no wait: there the image
//! waits measure while no record holds them (`hex_runtime::guest::ImageWaits::new`), and the census file keeps
//! each answer of `hex.image.boot`, `hex.image.stats` and `hex.image.stop`. At its end the test prints, for each,
//! its count, fastest and slowest (`IMAGE-RECORD`); and, as it goes, every answer the client's own waits record
//! (`IMAGE-STEP`, each call's answer as the client saw it) and each case's verdict and time (`IMAGE-CASE`).
//!
//! Every case passes: HEX-IMG-4's guest time per vCPU among them, which Shards gives an image run since D241, read as
//! its run host reads its VM's (`Stats::microvm`, Shards e0cc059c and on).
//!
//! Explicit inputs only, each by its own variable; a missing one fails by name. Run alone, `--ignored`; on Linux
//! inside a cgroup delegated to this binary (`HEX_DELEGATED`), as native_firecracker_smoke is, where the node holds
//! its boxes; on macOS and Windows as it is, where the node holds them as `hex node` there does:
//! - the image, with a `/bin/sh` (HEX-IMG-3's command), in the node's Shards home before the node starts: either
//!   `HEX_IMAGE_CONTEXT`, an Agentfile image's build context (a harness image, which no registry need serve), built
//!   there and pinned by the manifest digest its build says, as Shards' store finds a pinned image by content
//!   (`Store::holding`): `crates/hex-cli/tests/image_harness` is one any arch builds, its base pinned by its digest
//!   and its one harness `/bin/sh -c cat`, so a run of its members alone has a member on every host; or else
//!   `HEX_IMAGE`, `NAME@sha256:...`, pulled there by its digest;
//! - `SHARDS_BIN`: the `shards` binary, the node's and the pull's or build's; `SHARDS_KERNEL` and `SHARDS_INIT`: the
//!   kernel the run hosts boot (D231's own, as `shards run` takes it), so no first boot downloads one, and the
//!   shards-init that `shards` build carries (its build.rs's `OUT_DIR/shards-init`), which a run takes only beside a
//!   kernel given so; the node passes both to its run hosts as their environment;
//! - the node's Shards guest configuration, as `hex node` reads it: `HEX_GUEST_KERNEL`, `HEX_GUEST_INITRD`,
//!   `HEX_GUEST_CPUS`, `HEX_GUEST_MEMORY_MIB`, `HEX_GUEST_COST_FILE`, `HEX_GUEST_COST_SHA256` and
//!   `HEX_GUEST_COST_EVIDENCE`;
//! - `HEX_IMAGE_CPU_MILLI` and `HEX_IMAGE_MEMORY_BYTES`: the workload each VM is admitted for, which the guest
//!   cost sizes (`Allocation::vm_size`);
//! - `HEX_IMAGE_WORK`: a directory of this run's own, made here; `HEX_IMAGE_WAIT_MS`: the run's explicit deadline
//!   for the whole suite (the node's start waits for the node while it lives, `image_node::Node::start`);
//!   `HEX_WAITS_CENSUS_MAX`: the census's most lines.
//!
//! Run on Linux: build it with `cargo test --release --locked -p hex-cli --test image_real --no-run`, which names the
//! test binary it built (`target/release/deps/image_real-<hash>`), then start that binary alone in the delegated
//! scope, with those set: `systemd-run --user --scope -p Delegate=yes --quiet -- env HEX_DELEGATED=1 <inputs>
//! <binary> --ignored --nocapture`. Never cargo inside the scope: cargo stays in the scope's own cgroup, and the node
//! can't hand its box root controllers from a cgroup holding processes of its own (`cgroup.subtree_control`,
//! EBUSY). On macOS and Windows: `cargo test --release --locked -p hex-cli --test image_real -- --ignored
//! --nocapture` with those set.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use hex_api::conformance::{self, Act, Fixture};
use hex_api::{
    Allocation, Bounds, BoxId, BoxSpec, Class, ClassSpec, Program, Runtime, ShardsImage,
};
use hex_client::Client;

#[path = "image_node/mod.rs"]
mod image_node;
#[cfg(target_os = "linux")]
use image_node::own_cgroup;
use image_node::{Node, allocation, built, input, number, pull, records, step};

/// The name an image built from `HEX_IMAGE_CONTEXT` is tagged with in the node's Shards home; the cases pin it by
/// its digest.
const BUILT: &str = "hex-image-real/harness";

/// Where the cases' image comes from.
enum Source {
    /// An Agentfile image's build context, built into the node's Shards home.
    Context(PathBuf),
    /// An image pinned by its digest, pulled there.
    Pinned(String),
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
            band: 0,
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

#[test]
#[ignore = "native: KVM, HVF or WHP, a Shards guest configuration and an image (on Linux a delegated cgroup)"]
fn the_shards_image_class_holds_its_contract_through_a_real_node() {
    #[cfg(target_os = "linux")]
    assert!(
        std::env::var_os("HEX_DELEGATED").is_some(),
        "run inside a cgroup delegated to this binary (HEX_DELEGATED)"
    );
    let source = match std::env::var_os("HEX_IMAGE_CONTEXT") {
        Some(context) => Source::Context(PathBuf::from(context)),
        None => {
            let image = input("HEX_IMAGE");
            ShardsImage::new(&image).check().unwrap();
            Source::Pinned(image)
        }
    };
    let shards = PathBuf::from(input("SHARDS_BIN"));
    let kernel = PathBuf::from(input("SHARDS_KERNEL"));
    assert!(kernel.is_file(), "SHARDS_KERNEL {kernel:?} is no file");
    let init = PathBuf::from(input("SHARDS_INIT"));
    assert!(init.is_file(), "SHARDS_INIT {init:?} is no file");
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
    let allocation = allocation();
    eprintln!("IMAGE each VM {:?}", allocation.vm_size().unwrap());
    // Linux's node holds its boxes in a child of this binary's own delegated cgroup; macOS's and Windows' where
    // their `hex node` does.
    #[cfg(target_os = "linux")]
    let boxes = Some(own_cgroup("hex-image-real").join("hex-image-node"));
    #[cfg(not(target_os = "linux"))]
    let boxes: Option<PathBuf> = None;
    let image = match source {
        Source::Context(context) => built(&shards, &context, &work, BUILT),
        Source::Pinned(image) => {
            pull(&shards, &image, &work.join("state"));
            image
        }
    };
    ShardsImage::new(&image).check().unwrap();
    let node = Node::start(&work, boxes.as_deref());
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
    assert!(failures.is_empty(), "the image cases failed: {failures:#?}");
}

/// Joint plan step 12 (AC-T7.4.1; ROOT's HVF run at c8b4ce6b, whose cases were refused for an image with no member): the
/// in-tree harness context, `image_harness/`, which `HEX_IMAGE_CONTEXT` and image_record's home take on any host, an
/// aarch64 one's (macOS on HVF, Windows on arm64) among them, where no harness build of its arch is at hand. Its base is
/// pinned by its digest, an index each home pulls its own arch's image of as a node's does, and its one harness runs
/// `/bin/sh -c cat`, so a run of the image's members alone has a member. Do read its Agentfile and its harness's startup
/// descriptor. Expect one FROM, pinned (`NAME@sha256:<64 hex>`); one HARNESS, from its own directory; and that
/// harness's descriptor in Shards' simple form (`entrypoint` and `options`), saying `/bin/sh -c cat`.
#[test]
fn the_in_tree_harness_context_pins_its_base_and_runs_one_member() {
    let context = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/image_harness");
    let agentfile = fs::read_to_string(context.join("Agentfile")).unwrap();
    let said: Vec<&str> = agentfile
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let [from, harness] = said.as_slice() else {
        panic!("one FROM and one HARNESS: {said:?}");
    };
    let (name, digest) = from
        .strip_prefix("FROM ")
        .and_then(|base| base.split_once("@sha256:"))
        .unwrap_or_else(|| panic!("a base pinned by its digest: {from}"));
    assert!(
        !name.is_empty() && !name.contains(char::is_whitespace),
        "{from}"
    );
    assert!(
        digest.len() == 64
            && digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "{from}"
    );
    assert_eq!(*harness, "HARNESS cat FROM ./cat");
    let descriptor: serde_json::Value =
        serde_json::from_slice(&fs::read(context.join("cat/harness.json")).unwrap()).unwrap();
    assert_eq!(
        descriptor,
        serde_json::json!({"entrypoint": "/bin/sh -c", "options": ["cat"]})
    );
}
