//! Decision 2's seam half (docs/design/runtime-architecture.md; AC-T7.1.1, T-T7.1.2): through
//! `hex_node::Node`, the node agent is each `oci` box's parent, holds its init's pidfd and records
//! how the init ended. "do X, expect Y" (§6):
//! - a crash: SIGKILL an agent while its box runs, restart it, end the init: the restarted agent
//!   reads the box running, then empty, and nothing of it is left;
//! - create a box: its init's parent is the agent;
//! - a box whose program exits 7: inspect reports `Code(7)` and the init's pid is gone (reaped);
//! - SIGKILL a box's init: inspect reports `Signal(9)`;
//! - past the init budget: create is refused with `Exhausted`, naming it, and leaves nothing;
//! - the hex-api conformance suite on the `oci` class, its exit-report cases included;
//! - decision 4's network namespaces (goal 4 plan step 4): a box whose bundle makes a network
//!   namespace of its own runs in one no other box has had, loopback up. HEX-NETNS-9: from a node whose
//!   pool holds none (bound 0) it is made on the create's path, counted so. HEX-NETNS-8, as root: two
//!   boxes take one each from the node's pool, and the node keeps no descriptor of either.
//!   HEX-NETNS-10, rootless: made on the path in the box's own user namespace, which owns it, nothing
//!   taken;
//! - a box's process orphaned to the agent, once ended, is reaped by the sweep: no zombie is kept;
//! - goal 5 plan step 2: 31 boxes are made at once while another box's durable record write is held
//!   open, none waiting on it, and none of their records synced with their container state on tmpfs;
//!   two nodes with state directories of their own each make and start a box of the same id, two
//!   containers, and deleting one leaves the other running; a box made before the container state
//!   moved, its record naming `<state>/oci/<id>`, is taken back, inspected and deleted by today's node;
//! - goal 5 extended step 5 (T-T7.1.2): a node restarted over records in both its places finds each
//!   box's where it belongs, the boxes whose directories all lie on tmpfs among its runtime directory's
//!   records and one with its rendered bundle's writable layer on disk in its state directory,
//!   fsynced; it takes back the live ones and removes the one whose init ended, and leaves nothing once
//!   they're deleted;
//!
//! Rootless where the caller isn't root, in a delegated cgroup v2 subtree, as oci_seccomp.rs; where
//! neither is possible it prints SKIP and why rather than pass falsely. The crash runs first, before
//! this process opens any node: an agent's orphans go to its nearest subreaper, and this process
//! becomes one when it opens a node, so a restart in a process that never was the init's ancestor is
//! the real one's.
#![cfg(target_os = "linux")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio as Std};
use std::time::{Duration, Instant};

use hex_api::conformance::{self, Act, Fixture};
use hex_api::{
    Bounds, BoxId, BoxInfo, BoxSpec, BoxState, ClassSpec, Exit, Network, OciSpec, Refusal, Rootfs,
    Runtime,
};
use hex_core::record::{BoxRecord, Phase, ProcId};
use hex_node::{Node, OCI_BOXES, records_dir};
use hex_runtime::linux::cgroup::BoxRoot;
use hex_runtime::linux::disks;
use hex_runtime::linux::oci::run::{ClassicReason, Held, Runtime as OciRuntime};

const TEST: &str = "oci_boxes_report_their_inits_exit_through_the_seam";
/// Runs just the prepared-device oracle through the existing root/delegation fixture, in a fresh
/// process so it cannot become a subreaper before the full suite's agent-crash check.
const PREPARED_DEV_ENV: &str = "HEX_OCI_BOX_PREPARED_DEV";
const PREPARED_DEV_PASS: &str = "prepared /dev oracle completed with actual pooled takes";
const PROPAGATION_ENV: &str = "HEX_OCI_BOX_SHARED_SOURCES";
const PROPAGATION_PASS: &str =
    "shared-source propagation oracle completed with four pruned creates";
/// Both entry points can move the test process between cgroups. Serialize their parent fixtures;
/// recover the test-only lock after a failed full suite so the independent oracle still runs.
static LAB_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// Remember the delegated parent before the first fixture moves this process into its agent leaf.
static LAB_PARENT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
/// Set in the re-exec'd copy of this test that plays the agent the crash kills.
const AGENT_ENV: &str = "HEX_OCI_BOX_AGENT";

#[test]
fn oci_boxes_report_their_inits_exit_through_the_seam() {
    let exe = env!("CARGO_BIN_EXE_hex").to_owned();
    let cache = cache_dir();
    if let Some(dirs) = std::env::var_os(AGENT_ENV) {
        play_the_agent(Path::new(&dirs), &exe);
    }
    let _serial = LAB_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    match prepare_cgroup(&cache) {
        Ok(own) => exercise(&own, &cache, &exe),
        Err(direct) if std::env::var_os("HEX_DELEGATED").is_none() => {
            let has_systemd = Command::new("systemd-run")
                .arg("--version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !has_systemd {
                eprintln!(
                    "SKIP: own cgroup isn't ours to subdivide ({direct}) and no systemd --user for a private scope"
                );
                return;
            }
            let status = Command::new("systemd-run")
                .args(["--user", "--scope", "--quiet", "-p", "Delegate=yes"])
                .arg(std::env::current_exe().expect("test binary path"))
                .args([TEST, "--exact", "--nocapture", "--test-threads=1"])
                .env("HEX_DELEGATED", "1")
                .status()
                .expect("run the test in a delegated scope");
            assert!(status.success(), "the delegated re-run failed");
        }
        Err(direct) => {
            eprintln!("SKIP: no usable cgroup v2 delegation ({direct}); run on a cgroup-v2 host")
        }
    }
}

/// AC-T7.1.4, goal 5 step 3: run the prepared-/dev ordering and isolation oracle independently of
/// the earlier full-suite checks. A failure in either test remains visible to the test harness.
#[test]
fn prepared_devs_match_ordinary_setup_and_isolate_containers() {
    if host_id("-u") != 0 {
        eprintln!("SKIP: the prepared /dev oracle requires the privileged root lab");
        return;
    }
    let _serial = LAB_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let parent = prepare_cgroup(&cache_dir()).expect("a delegated parent for the root oracle");
    let own = parent.join(format!("hex-devfs-oracle-{}", std::process::id()));
    std::fs::create_dir(&own).expect("a fresh cgroup for the independent oracle");
    // The full suite already moved our parent process into hex-agent. Move only this fresh child
    // into its own empty cgroup before exec, so its fixture can delegate controllers to its boxes.
    let output = Command::new("/bin/sh")
        .args([
            "-e",
            "-c",
            r#"printf '%s' "$$" > "$1/cgroup.procs"; shift; exec "$@""#,
            "hex-devfs-oracle",
        ])
        .arg(&own)
        .arg(std::env::current_exe().expect("test binary path"))
        .args([TEST, "--exact", "--nocapture", "--test-threads=1"])
        .env(PREPARED_DEV_ENV, "1")
        .output()
        .expect("run the prepared /dev oracle through the root fixture");
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    eprint!("{log}");
    assert!(
        output.status.success(),
        "the independent prepared /dev oracle failed"
    );
    assert!(
        log.lines().any(|line| line == PREPARED_DEV_PASS),
        "the root oracle did not run; a skipped delegation is not evidence"
    );
}

/// AC-T7.1.4: rootfs and bind sources are deliberately shared; every mount stays in the box.
#[test]
fn pruned_shared_sources_do_not_leak_mounts_to_the_agent() {
    if host_id("-u") != 0 {
        eprintln!("SKIP: the shared-source propagation oracle requires the privileged root lab");
        return;
    }
    let _serial = LAB_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let parent = prepare_cgroup(&cache_dir()).expect("a delegated parent for the root oracle");
    let own = parent.join(format!("hex-propagation-oracle-{}", std::process::id()));
    std::fs::create_dir(&own).expect("a fresh cgroup for the independent oracle");
    // The full suite already moved our parent process into hex-agent. Move only this fresh child
    // into its own empty cgroup before exec, so its fixture can delegate controllers to its boxes.
    let output = Command::new("/bin/sh")
        .args([
            "-e",
            "-c",
            r#"printf '%s' "$$" > "$1/cgroup.procs"; shift; exec "$@""#,
            "hex-propagation-oracle",
        ])
        .arg(&own)
        .arg(std::env::current_exe().expect("test binary path"))
        .args([TEST, "--exact", "--nocapture", "--test-threads=1"])
        .env(PROPAGATION_ENV, "1")
        .output()
        .expect("run the shared-source propagation oracle through the root fixture");
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    eprint!("{log}");
    assert!(
        output.status.success(),
        "the independent shared-source propagation oracle failed"
    );
    assert!(
        log.lines().any(|line| line == PROPAGATION_PASS),
        "the root oracle did not run; a skipped delegation is not evidence"
    );
}

/// Where one run keeps its bundles, records and box root.
struct Lab {
    exe: String,
    rootfs: PathBuf,
    bundles: tempfile::TempDir,
    rootless: bool,
    uid: u32,
    gid: u32,
}

impl Lab {
    fn new(cache: &Path, exe: &str) -> Self {
        let (uid, gid) = (host_id("-u"), host_id("-g"));
        let bundles = tempfile::Builder::new()
            .prefix("ocibox-")
            .tempdir_in(cache)
            .expect("bundles dir");
        let rootfs = bundles.path().join("rootfs");
        build_rootfs(&rootfs);
        Self {
            exe: exe.to_owned(),
            rootfs,
            bundles,
            rootless: uid != 0,
            uid,
            gid,
        }
    }

    /// A bundle whose init runs `args`, in a fresh directory.
    fn bundle(&self, args: &[String]) -> PathBuf {
        self.bundle_with(args, false)
    }

    /// A bundle whose init runs `args`, with a network namespace of its own when `network` is set,
    /// its sysfs mounted at `/sys` to read it by.
    fn bundle_with(&self, args: &[String], network: bool) -> PathBuf {
        let dir = tempfile::Builder::new()
            .prefix("bundle-")
            .tempdir_in(self.bundles.path())
            .expect("bundle dir")
            .keep();
        std::fs::write(dir.join("config.json"), self.config(args, network)).expect("config.json");
        dir
    }

    fn config(&self, args: &[String], network: bool) -> String {
        let (mut namespaces, mappings) = if self.rootless {
            (
                r#"{"type":"user"},{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"}"#
                    .to_owned(),
                format!(
                    r#","uidMappings":[{{"containerID":0,"hostID":{},"size":1}}],"gidMappings":[{{"containerID":0,"hostID":{},"size":1}}]"#,
                    self.uid, self.gid
                ),
            )
        } else {
            (
                r#"{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"}"#.to_owned(),
                String::new(),
            )
        };
        let mut sys = String::new();
        if network {
            namespaces.push_str(r#",{"type":"network"}"#);
            sys.push_str(
                r#",
    {"destination": "/sys", "type": "sysfs", "source": "sysfs", "options": ["nosuid","noexec","nodev","ro"]}"#,
            );
        }
        self.config_with(args, &namespaces, &mappings, &sys)
    }

    /// A bundle's config.json with its `linux.namespaces` entries, id mappings and extra mounts as given.
    fn config_with(&self, args: &[String], namespaces: &str, mappings: &str, sys: &str) -> String {
        self.config_ex(args, namespaces, mappings, sys, "", "")
    }

    /// `config_with` with, besides, a top-level `hooks` fragment and extra fields in the `linux` object
    /// (`rootfsPropagation`), each a JSON fragment with its own leading comma or empty. Goal 5 plan step 2's
    /// classic-path reasons (createContainer hooks) and rootfsPropagation tests use it.
    fn config_ex(
        &self,
        args: &[String],
        namespaces: &str,
        mappings: &str,
        sys: &str,
        hooks: &str,
        linux_extra: &str,
    ) -> String {
        let args = json_args(args);
        format!(
            r#"{{
  "ociVersion": "1.3.0",
  "root": {{"path": "{rootfs}", "readonly": false}},
  "process": {{
    "cwd": "/",
    "args": {args},
    "env": ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"],
    "user": {{"uid": 0, "gid": 0}},
    "noNewPrivileges": true
  }},
  "mounts": [
    {{"destination": "/proc", "type": "proc", "source": "proc"}},
    {{"destination": "/dev", "type": "tmpfs", "source": "tmpfs", "options": ["nosuid","mode=755","size=65536k"]}},
    {{"destination": "/tmp", "type": "tmpfs", "source": "tmpfs", "options": ["nosuid","nodev"]}},
    {{"destination": "/usr", "type": "bind", "source": "/usr", "options": ["rbind","ro"]}},
    {{"destination": "/hex", "type": "bind", "source": "{exe}", "options": ["bind","ro"]}}{sys}
  ],
  "linux": {{"namespaces": [{namespaces}]{mappings}{linux_extra}}}{hooks}
}}"#,
            rootfs = self.rootfs.display(),
            exe = self.exe,
        )
    }

    /// A box whose init runs `args`.
    fn running(&self, id: BoxId, args: &[String]) -> BoxSpec {
        BoxSpec {
            id,
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(self.bundle(args)),
            owner: None,
        }
    }

    /// A box whose init waits, in a network namespace of its own.
    fn networked(&self, id: BoxId) -> BoxSpec {
        BoxSpec {
            id,
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(self.bundle_with(&Self::act(&Act::Wait), true)),
            owner: None,
        }
    }

    /// The `hex selftest-box` program for `act`, as the container sees the binary (`/hex`).
    fn act(act: &Act) -> Vec<String> {
        let mut args = vec!["/hex".to_owned(), "selftest-box".to_owned()];
        match act {
            Act::Say(words) => args.extend(["say".to_owned(), (*words).to_owned()]),
            Act::Fail(code) => args.extend(["fail".to_owned(), code.to_string()]),
            Act::Wait => args.push("wait".to_owned()),
            Act::Hog(bytes) => args.extend(["hog".to_owned(), bytes.to_string()]),
            Act::Spawn(n) => args.extend(["spawn".to_owned(), n.to_string()]),
            Act::Echo => args.push("echo".to_owned()),
        }
        args
    }

    fn node(&self, box_root: &Path, state: &Path) -> Node {
        Node::open(box_root, state, state)
            .expect("open a node")
            .with_oci_helper(PathBuf::from(&self.exe))
    }
}

impl Fixture for Lab {
    fn spec(&self, id: BoxId, bounds: Bounds) -> BoxSpec {
        BoxSpec {
            id,
            bounds,
            class: ClassSpec::Bundle(self.bundle(&Self::act(&Act::Wait))),
            owner: None,
        }
    }

    fn init(&self, id: BoxId, act: &Act) -> BoxSpec {
        BoxSpec {
            id,
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(self.bundle(&Self::act(act))),
            owner: None,
        }
    }

    fn program(&self, act: &Act) -> hex_api::Program {
        let args = Self::act(act);
        hex_api::Program::of(&args.iter().map(String::as_str).collect::<Vec<_>>())
    }
}

fn exercise(own: &Path, cache: &Path, exe: &str) {
    let lab = Lab::new(cache, exe);
    if std::env::var_os(PROPAGATION_ENV).is_some() {
        assert!(
            !lab.rootless,
            "the shared-source propagation oracle needs root"
        );
        rootfs_propagation_shows_in_the_box_not_the_agent(&lab, own);
        eprintln!("{PROPAGATION_PASS}");
        return;
    }
    if std::env::var_os(PREPARED_DEV_ENV).is_some() {
        assert!(!lab.rootless, "the prepared /dev oracle needs root");
        prepared_devs_preserve_order_and_are_never_shared(&lab, own);
        eprintln!("{PREPARED_DEV_PASS}");
        return;
    }
    // First, while this process is no node's and so no subreaper; it also finds out whether a
    // container runs here at all (rootless needs unprivileged user namespaces).
    if let Err(e) = crash_and_restart(&lab, own) {
        eprintln!("SKIP: rootless OCI unavailable here ({e})");
        return;
    }
    parent_and_exit_code(&lab, own);
    killed_init(&lab, own);
    budget(&lab, own);
    conformance_on_oci(&lab, own);
    network_namespaces(&lab, own);
    program_alone(&lab, own);
    programs_hold_their_stdio_alone(&lab, own);
    exec_reads_no_state(&lab, own);
    orphans_are_reaped(&lab, own);
    held_record_write(&lab, own);
    two_nodes_one_box_id(&lab, own);
    a_box_made_before_the_move_is_adopted(&lab, own);
    a_node_upgraded_over_boxes_at_the_old_place_recovers_both_places(&lab, own);
    records_in_both_places_are_found_after_a_restart(&lab, own);
    created_through_a_seed_in_its_cgroup_and_namespaces(&lab, own);
    each_seed_cloned_container_is_randomized_afresh(&lab, own);
    a_seed_that_dies_mid_create_is_refused_and_restarted(&lab, own);
    a_bundle_joining_a_namespace_by_path_runs_through_the_seam(&lab, own);
    a_box_root_counts_its_boxes_without_listing(&lab, own);
    escapes_are_contained(&lab, own);
    mount_namespaces_made_each_way(&lab, own);
    procs_made_ahead_carry_their_masks(&lab, own);
    masked_directories_are_the_nodes_one_empty_filesystem(&lab, own);
    a_bind_and_a_volume_are_present_the_host_unreachable(&lab, own);
    classic_reasons_are_counted(&lab, own);
    an_init_that_refuses_or_dies_mid_build_leaves_nothing(&lab, own);
    rootfs_propagation_shows_in_the_box_not_the_agent(&lab, own);
    every_mount_kind_is_in_the_box_as_its_spec_asks(&lab, own);
    prepared_devs_preserve_order_and_are_never_shared(&lab, own);
}

/// AC-T7.1.4, goal 5 step 3: compare ordinary and prepopulated /dev on hostile mount layouts, on the pruned
/// path and on the classic one a createRuntime hook takes through the shim (goal 5 extended step 3).
/// Expect byte-identical refusals, matching devices/links/options on success, a pooled take for each case on
/// each path, two actual pooled takes for live containers, and no cross-container mutation or
/// host-namespace attachment.
fn prepared_devs_preserve_order_and_are_never_shared(lab: &Lab, own: &Path) {
    if lab.rootless {
        return;
    }
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    let ordinary_state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let pooled_state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let ordinary = lab
        .node(&own.join("devfs-ordinary"), ordinary_state.path())
        .with_prepared_dev_mounts(false)
        .unwrap();
    let pooled = lab.node(&own.join("devfs-pooled"), pooled_state.path());
    let before_mounts = agent_mount_points();

    let bundle = |case: &str, dest: Option<&str>, shim: bool| {
        let dir = tempfile::Builder::new()
            .prefix("devfs-bundle-")
            .tempdir_in(lab.bundles.path())
            .unwrap()
            .keep();
        let root = dir.join("rootfs");
        build_rootfs(&root);
        if case == "alias" {
            std::os::unix::fs::symlink("/dev/null", root.join("alias")).unwrap();
        }
        if case == "file-component" {
            std::fs::write(root.join("blocked"), "file").unwrap();
        }
        if case == "dev-symlink" {
            std::fs::rename(root.join("dev"), root.join("devices")).unwrap();
            std::os::unix::fs::symlink("devices", root.join("dev")).unwrap();
        }
        let mut spec: serde_json::Value =
            serde_json::from_str(&lab.config(&Lab::act(&Act::Wait), false)).unwrap();
        spec["root"]["path"] = serde_json::json!(root);
        if let Some(dest) = dest {
            spec["mounts"].as_array_mut().unwrap().push(serde_json::json!({
                "destination":dest, "type":"tmpfs", "source":"late", "options":["mode=701","size=64k"]
            }));
        }
        if shim {
            spec["hooks"] =
                serde_json::json!({"createRuntime":[{"path":"/usr/bin/true","args":["true"]}]});
        }
        std::fs::write(dir.join("config.json"), serde_json::to_vec(&spec).unwrap()).unwrap();
        dir
    };
    let create = |node: &Node, id: &BoxId, dir: &Path| {
        node.create(BoxSpec {
            id: id.clone(),
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(dir.to_owned()),
            owner: None,
        })
    };
    let prime = bundle("prime", None, false);
    let id = BoxId::new("prime").unwrap();
    create(&pooled, &id, &prime).unwrap();
    pooled.delete(&id).unwrap();
    wait_for(
        || pooled.dev_mounts().unwrap().made >= hex_core::devfs::PER_KEY as u64,
        "the prepared /dev pool to fill after its first key",
    );
    assert_eq!(pooled.dev_mounts().unwrap().failed, 0);
    let root_of = |node: &Node, id: &BoxId| {
        PathBuf::from(format!("/proc/{}/root", init_pid(&node.container_dir(id))))
    };
    let snapshot = |node: &Node, id: &BoxId| {
        let root = root_of(node, id);
        let mut out = Vec::new();
        for name in [
            "null", "zero", "full", "random", "urandom", "tty", "fd", "stdin", "stdout", "stderr",
            "ptmx",
        ] {
            let path = root.join("dev").join(name);
            let m = std::fs::symlink_metadata(&path).unwrap();
            out.push(format!(
                "{name} {:o} {} {} {} {:?}",
                m.mode(),
                m.uid(),
                m.gid(),
                m.rdev(),
                std::fs::read_link(&path).ok()
            ));
        }
        let pid = init_pid(&node.container_dir(id));
        for m in listed_mounts(&std::fs::read_to_string(format!("/proc/{pid}/mountinfo")).unwrap())
        {
            if m.point == "/dev" || m.source == "late" {
                out.push(format!(
                    "{} {} {} {:?} {:?}",
                    m.point, m.fstype, m.source, m.opts, m.sb_opts
                ));
            }
        }
        out
    };
    for (case, target, shim) in [
        ("plain", None),
        ("dev-symlink", None),
        ("default-node", Some("/dev/null")),
        ("through-node", Some("/dev/null/child")),
        ("default-link", Some("/dev/fd/extra")),
        ("alias", Some("/alias")),
        ("file-component", Some("/blocked/child")),
        ("parent", Some("/dev/shm/../null")),
    ]
    .into_iter()
    .flat_map(|(case, target)| [(case, target, false), (case, target, true)])
    {
        let dir = bundle(case, target, shim);
        let id = BoxId::new(&format!("{case}-{}", if shim { "shim" } else { "pruned" })).unwrap();
        let through_shim = pooled.creates_through_shim();
        let run = |node: &Node| match create(node, &id, &dir) {
            Err(e) => Err(e),
            Ok(_) => {
                let got = snapshot(node, &id);
                node.delete(&id).unwrap();
                Ok(got)
            }
        };
        let a = run(&ordinary);
        if case != "parent" {
            wait_for(
                || {
                    let c = pooled.dev_mounts().unwrap();
                    c.made > c.taken + c.discarded
                },
                "a prepared mount for the oracle case",
            );
        }
        let taken = pooled.dev_mounts().unwrap().taken;
        let b = run(&pooled);
        assert_eq!(
            a, b,
            "prepared /dev changed {case} (through the shim: {shim})"
        );
        if case != "parent" {
            assert_eq!(
                pooled.dev_mounts().unwrap().taken,
                taken + 1,
                "{case} must consume a prepared mount (including guarded fallback; through the shim: {shim})"
            );
            assert_eq!(
                pooled.creates_through_shim() - through_shim,
                u64::from(shim),
                "{case} took the path it was meant to (through the shim: {shim})"
            );
        }
    }

    let first = BoxId::new("isolated-a").unwrap();
    let second = BoxId::new("isolated-b").unwrap();
    wait_for(
        || {
            let c = pooled.dev_mounts().unwrap();
            c.made >= c.taken + c.discarded + 2
        },
        "two prepared mounts for isolation",
    );
    let taken = pooled.dev_mounts().unwrap().taken;
    create(&pooled, &first, &prime).unwrap();
    create(&pooled, &second, &prime).unwrap();
    assert_eq!(pooled.dev_mounts().unwrap().taken, taken + 2);
    let aroot = root_of(&pooled, &first);
    let broot = root_of(&pooled, &second);
    assert_ne!(
        std::fs::metadata(aroot.join("dev")).unwrap().dev(),
        std::fs::metadata(broot.join("dev")).unwrap().dev(),
        "a fresh superblock per container"
    );
    std::fs::write(aroot.join("dev/private-marker"), "first only").unwrap();
    std::fs::set_permissions(
        aroot.join("dev/null"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    assert!(!broot.join("dev/private-marker").exists());
    assert_eq!(
        std::fs::metadata(broot.join("dev/null")).unwrap().mode() & 0o777,
        0o666
    );
    pooled.delete(&first).unwrap();
    assert!(
        std::fs::metadata(broot.join("dev/zero"))
            .unwrap()
            .file_type()
            .is_char_device()
    );
    pooled.delete(&second).unwrap();
    assert_eq!(
        agent_mount_points(),
        before_mounts,
        "prepared mounts never attach in the agent"
    );
    eprintln!(
        "prepared /dev: fixed pool counters, hostile ordering/refusal oracle, exact devices/links/options, distinct superblocks and isolated mutation all pass"
    );
}

/// Goal 5 plan step 3 (T-T7.1.4; CLAUDE.md §6): every mount kind a spec can ask for is in the box's own
/// mountinfo with the source, target and options its spec asked, on each root path. One spec asks for
/// them all: proc, tmpfs (`/dev`, `/tmp`, `/dev/shm`, and two whose access times the spec sets:
/// `strictatime`, as Docker's `/dev` asks, and `noatime,nodiratime`), devpts, mqueue, sysfs and a
/// read-only cgroup2 in the box's own cgroup namespace, a recursive bind read-only (`/usr`) and
/// read-write (a volume), a plain bind of a directory and of a file read-only (the file's target missing
/// from the image, so it is made), masked paths (files and a directory) and read-only paths (a directory
/// and a file). As root the pruned path makes one box and the classic path
/// another, a createRuntime hook routing it through the shim; rootless the classic path makes it, in the
/// box's own user namespace. Each box is checked the same way, so a change to how the init mounts that
/// alters what a mount is fails here on the path it changed.
fn every_mount_kind_is_in_the_box_as_its_spec_asks(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-mount-kinds"), state.path());
    let vol = lab.bundles.path().join("kinds-vol");
    let ro_dir = lab.bundles.path().join("kinds-ro");
    let file = lab.bundles.path().join("kinds-file");
    for dir in [&vol, &ro_dir] {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("marker"), "from the host\n").unwrap();
    }
    std::fs::write(&file, "a bound file\n").unwrap();
    // A rootless box's user namespace has no group 5 to give its terminals (bundle.rs, default_mounts).
    let pts = if lab.rootless {
        r#"["nosuid","noexec","newinstance","ptmxmode=0666","mode=0620"]"#
    } else {
        r#"["nosuid","noexec","newinstance","ptmxmode=0666","mode=0620","gid=5"]"#
    };
    let mounts = format!(
        r#",
    {{"destination": "/dev/pts", "type": "devpts", "source": "devpts", "options": {pts}}},
    {{"destination": "/dev/shm", "type": "tmpfs", "source": "shm", "options": ["nosuid","noexec","nodev","mode=1777","size=65536k"]}},
    {{"destination": "/dev/mqueue", "type": "mqueue", "source": "mqueue", "options": ["nosuid","noexec","nodev"]}},
    {{"destination": "/sys", "type": "sysfs", "source": "sysfs", "options": ["nosuid","noexec","nodev","ro"]}},
    {{"destination": "/sys/fs/cgroup", "type": "cgroup2", "source": "cgroup", "options": ["nosuid","noexec","nodev","ro"]}},
    {{"destination": "/strict", "type": "tmpfs", "source": "tmpfs", "options": ["nosuid","strictatime","mode=755"]}},
    {{"destination": "/noatime", "type": "tmpfs", "source": "tmpfs", "options": ["noatime","nodiratime"]}},
    {{"destination": "/vol", "type": "bind", "source": "{vol}", "options": ["rbind","rw"]}},
    {{"destination": "/ro-dir", "type": "bind", "source": "{ro_dir}", "options": ["bind","ro"]}},
    {{"destination": "/etc/kinds-file", "type": "bind", "source": "{file}", "options": ["bind","ro"]}}"#,
        vol = vol.display(),
        ro_dir = ro_dir.display(),
        file = file.display(),
    );
    // Masks the kernel may lack are looked for in the agent's own /proc and /sys (one kernel), and the box
    // is held to a mask only where the path is there to mask.
    let masked = [
        "/proc/kcore",
        "/proc/timer_list",
        "/proc/scsi",
        "/sys/firmware",
    ];
    let readonly = ["/proc/sys", "/proc/sysrq-trigger"];
    let linux_extra = format!(
        r#","maskedPaths":{},"readonlyPaths":{}"#,
        json_args(&masked.map(str::to_owned)),
        json_args(&readonly.map(str::to_owned))
    );
    let (namespaces, mappings) = if lab.rootless {
        (
            r#"{"type":"user"},{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"},{"type":"network"},{"type":"cgroup"}"#
                .to_owned(),
            format!(
                r#","uidMappings":[{{"containerID":0,"hostID":{},"size":1}}],"gidMappings":[{{"containerID":0,"hostID":{},"size":1}}]"#,
                lab.uid, lab.gid
            ),
        )
    } else {
        (
            r#"{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"},{"type":"network"},{"type":"cgroup"}"#
                .to_owned(),
            String::new(),
        )
    };
    let hooks = r#","hooks":{"createRuntime":[{"path":"/usr/bin/true","args":["true"]}]}"#;
    // (the path, its hooks): rootless the classic path alone; as root the pruned path, and the classic one,
    // which a create-time hook takes through the shim.
    let paths: &[(&str, &str)] = if lab.rootless {
        &[("classic", "")]
    } else {
        &[("pruned", ""), ("classic", hooks)]
    };
    for &(path, hooks) in paths {
        let dir = tempfile::Builder::new()
            .prefix("bundle-")
            .tempdir_in(lab.bundles.path())
            .unwrap()
            .keep();
        std::fs::write(
            dir.join("config.json"),
            lab.config_ex(
                &Lab::act(&Act::Wait),
                &namespaces,
                &mappings,
                &mounts,
                hooks,
                &linux_extra,
            ),
        )
        .unwrap();
        let id = BoxId::new(&format!("kinds-{path}")).unwrap();
        let (pruned, shim) = (node.pruned_creates(), node.creates_through_shim());
        node.create(BoxSpec {
            id: id.clone(),
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(dir),
            owner: None,
        })
        .unwrap_or_else(|e| panic!("create the {path} box: {e:?}"));
        node.start(&id).expect("start");
        let took = (
            node.pruned_creates() - pruned,
            node.creates_through_shim() - shim,
        );
        let (exit, mi) = run_in(&node, &id, &["/usr/bin/cat", "/proc/self/mountinfo"]);
        let (vol_exit, vol_out) = run_in(&node, &id, &["/usr/bin/cat", "/vol/marker"]);
        let ro_write = exit_in(&node, &id, &["/usr/bin/touch", "/ro-dir/written"]);
        node.delete(&id).expect("delete");

        let want_took = match (lab.rootless, path) {
            (true, _) => (0, 0),
            (false, "pruned") => (1, 0),
            _ => (0, 1),
        };
        assert_eq!(
            took, want_took,
            "the {path} box was made on the {path} path (pruned, through the shim)"
        );
        assert_eq!(exit, Exit::Code(0), "read the {path} box's mountinfo: {mi}");
        let listed = listed_mounts(&mi);
        let at = |point: &str| {
            listed
                .iter()
                .rev()
                .find(|m| m.point == point)
                .unwrap_or_else(|| panic!("no mount at {point} in the {path} box:\n{mi}"))
        };
        let has = |m: &ListedMount, opts: &[&str], sb: &[&str]| {
            for o in opts {
                assert!(
                    m.opts.iter().any(|x| x == o),
                    "{} in the {path} box has {o}: {m:?}",
                    m.point
                );
            }
            for o in sb {
                assert!(
                    m.sb_opts.iter().any(|x| x == o),
                    "{}'s filesystem in the {path} box has {o}: {m:?}",
                    m.point
                );
            }
        };
        // Filesystems: the type and source the spec named, its per-mount and filesystem options.
        let proc = at("/proc");
        assert_eq!(
            (proc.fstype.as_str(), proc.source.as_str()),
            ("proc", "proc")
        );
        has(proc, &["rw"], &[]);
        let dev = at("/dev");
        assert_eq!(
            (dev.fstype.as_str(), dev.source.as_str()),
            ("tmpfs", "tmpfs")
        );
        has(dev, &["rw", "nosuid"], &["mode=755", "size=65536k"]);
        let tmp = at("/tmp");
        assert_eq!(
            (tmp.fstype.as_str(), tmp.source.as_str()),
            ("tmpfs", "tmpfs")
        );
        has(tmp, &["rw", "nosuid", "nodev"], &[]);
        let pts = at("/dev/pts");
        assert_eq!(
            (pts.fstype.as_str(), pts.source.as_str()),
            ("devpts", "devpts")
        );
        let pts_sb: &[&str] = if lab.rootless {
            &["mode=620", "ptmxmode=666"]
        } else {
            &["mode=620", "ptmxmode=666", "gid=5"]
        };
        has(pts, &["rw", "nosuid", "noexec"], pts_sb);
        let shm = at("/dev/shm");
        assert_eq!((shm.fstype.as_str(), shm.source.as_str()), ("tmpfs", "shm"));
        has(shm, &["rw", "nosuid", "nodev", "noexec"], &["size=65536k"]);
        // Its mode=1777 is tmpfs's own default, which tmpfs lists by naming no mode (v6.19
        // mm/shmem.c:4930-4931), where /dev's mode=755 is named.
        assert!(
            !shm.sb_opts.iter().any(|o| o.starts_with("mode=")),
            "/dev/shm in the {path} box has tmpfs's default mode, 1777, as asked: {shm:?}"
        );
        let mq = at("/dev/mqueue");
        assert_eq!(
            (mq.fstype.as_str(), mq.source.as_str()),
            ("mqueue", "mqueue")
        );
        has(mq, &["rw", "nosuid", "nodev", "noexec"], &[]);
        let sys = at("/sys");
        assert_eq!(
            (sys.fstype.as_str(), sys.source.as_str()),
            ("sysfs", "sysfs")
        );
        has(sys, &["ro", "nosuid", "nodev", "noexec"], &[]);
        let cgroup = at("/sys/fs/cgroup");
        assert_eq!(
            (cgroup.fstype.as_str(), cgroup.source.as_str()),
            ("cgroup2", "cgroup")
        );
        has(cgroup, &["ro", "nosuid", "nodev", "noexec"], &[]);
        // Access times: strictatime shows as neither relatime nor noatime (proc_pid_mountinfo(5)).
        let strict = at("/strict");
        assert_eq!(
            (strict.fstype.as_str(), strict.source.as_str()),
            ("tmpfs", "tmpfs")
        );
        has(strict, &["rw", "nosuid"], &["mode=755"]);
        assert!(
            !strict
                .opts
                .iter()
                .any(|o| o == "relatime" || o == "noatime"),
            "/strict in the {path} box keeps every access time, as strictatime asks: {strict:?}"
        );
        let noatime = at("/noatime");
        assert_eq!(
            noatime.fstype, "tmpfs",
            "/noatime in the {path} box: {noatime:?}"
        );
        has(noatime, &["rw", "noatime", "nodiratime"], &[]);
        // Binds: the source's filesystem and the source within it, read-only or read-write as asked.
        for (point, source, opt) in [
            ("/usr", Path::new("/usr"), "ro"),
            ("/hex", Path::new(&lab.exe), "ro"),
            ("/vol", vol.as_path(), "rw"),
            ("/ro-dir", ro_dir.as_path(), "ro"),
            ("/etc/kinds-file", file.as_path(), "ro"),
        ] {
            let m = at(point);
            assert_eq!(
                (m.dev.clone(), m.root.clone()),
                bind_source_of(source),
                "{point} in the {path} box is a bind of {}: {m:?}",
                source.display()
            );
            has(m, &[opt], &[]);
        }
        assert_eq!(
            (vol_exit, vol_out.trim()),
            (Exit::Code(0), "from the host"),
            "the {path} box reads its volume"
        );
        assert_ne!(
            ro_write,
            Exit::Code(0),
            "the {path} box can't write its read-only bind"
        );
        // Masks: a file under a bind of a /dev/null, a directory under an empty read-only tmpfs.
        for point in masked {
            let m = listed.iter().rev().find(|m| m.point == point);
            match (Path::new(point).exists(), Path::new(point).is_dir(), m) {
                (false, _, _) => {}
                (true, true, Some(m)) => {
                    assert_eq!(m.fstype, "tmpfs", "{point} masked in the {path} box: {m:?}");
                    has(m, &["ro"], &[]);
                }
                (true, false, Some(m)) => assert!(
                    m.root.ends_with("/null"),
                    "{point} in the {path} box is masked by a /dev/null: {m:?}"
                ),
                (true, _, None) => panic!("{point} isn't masked in the {path} box:\n{mi}"),
            }
        }
        // Read-only paths: each bound over itself, read-only, still the box's proc.
        for point in readonly {
            let m = at(point);
            assert_eq!(m.fstype, "proc", "{point} in the {path} box: {m:?}");
            assert_eq!(
                m.root,
                point.trim_start_matches("/proc"),
                "{point} in the {path} box is its own proc entry: {m:?}"
            );
            has(m, &["ro"], &[]);
        }
    }
    eprintln!(
        "mount kinds: proc, tmpfs, devpts, mqueue, sysfs, binds (recursive, plain, a file, read-only and \
         read-write), masks and read-only paths are each as their spec asks, on the {} path(s)",
        paths
            .iter()
            .map(|(p, _)| *p)
            .collect::<Vec<_>>()
            .join(" and ")
    );
}

/// What a bind of `path` lists in mountinfo as its filesystem and its root within it: the device of the
/// agent's mount that holds `path` (the topmost at the longest mount point it lies under), and that mount's
/// root joined to `path` below its mount point. A filesystem's own device, as mountinfo names it, can differ
/// from a file's `st_dev` (a btrfs subvolume's is its own), so the bind is matched by the table's.
fn bind_source_of(path: &Path) -> (String, String) {
    let real = std::fs::canonicalize(path).unwrap();
    let mounts = listed_mounts(&std::fs::read_to_string("/proc/self/mountinfo").unwrap());
    let holder = mounts
        .iter()
        .filter(|m| real.starts_with(&m.point))
        .max_by_key(|m| Path::new(&m.point).components().count())
        .map(|deepest| {
            mounts
                .iter()
                .rev()
                .find(|m| m.point == deepest.point)
                .unwrap()
        })
        .unwrap_or_else(|| panic!("no mount holds {}", real.display()));
    let below = real.strip_prefix(&holder.point).unwrap();
    // `path` may be the mount point itself (a lab that binds the host's /usr in), and joining an
    // empty path adds a trailing separator mountinfo never shows.
    let root = if below.as_os_str().is_empty() {
        PathBuf::from(&holder.root)
    } else {
        Path::new(&holder.root).join(below)
    };
    (holder.dev.clone(), root.to_string_lossy().into_owned())
}

/// One `/proc/<pid>/mountinfo` line (proc_pid_mountinfo(5)): the filesystem's device, the mount's root
/// within it, the mount point, the per-mount options, the filesystem type, its source and the
/// filesystem's own options.
#[derive(Debug)]
struct ListedMount {
    dev: String,
    root: String,
    point: String,
    opts: Vec<String>,
    fstype: String,
    source: String,
    sb_opts: Vec<String>,
}

fn listed_mounts(text: &str) -> Vec<ListedMount> {
    let split = |s: &str| s.split(',').map(str::to_owned).collect();
    text.lines()
        .filter_map(|line| {
            let (left, right) = line.split_once(" - ")?;
            let left: Vec<&str> = left.split(' ').collect();
            let right: Vec<&str> = right.split(' ').collect();
            Some(ListedMount {
                dev: (*left.get(2)?).to_owned(),
                root: (*left.get(3)?).to_owned(),
                point: (*left.get(4)?).to_owned(),
                opts: split(left.get(5)?),
                fstype: (*right.first()?).to_owned(),
                source: (*right.get(1)?).to_owned(),
                sb_opts: split(right.get(2)?),
            })
        })
        .collect()
}

/// Goal 5 plan step 2 (T-T7.1.4), the pruned-seed root's security criteria: a container on that path
/// can't reach the host (the research probe's E0-E3, `goal5-root.md` §3-§4; f66-f80). As root — where a
/// create takes the pruned seed — run each escape in its own box with full capabilities and a marker on
/// the host, outside any rootfs; each must exit 0, the box held. E0 a nested user namespace is allowed
/// (the root isn't a plain chroot); E1 the classic chroot escape stays in the rootfs; E2 unmounting the
/// root and `setns`-ing into the box's own mount namespace reveals only the seed's empty read-only tmpfs,
/// not the host; E3 a nested proc mount past a mask is refused. Skipped rootless, where the classic path
/// runs (a pruned seed is a root node's).
fn escapes_are_contained(lab: &Lab, own: &Path) {
    if lab.rootless {
        return;
    }
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-escape"), state.path());
    escapes_held_on(lab, &node, "esc");
    // The pruned path served every escape box: none fell back to the shim (all eligible).
    assert_eq!(
        node.creates_through_shim(),
        0,
        "the escape boxes take the pruned path"
    );
    eprintln!(
        "escapes contained: e0 userns allowed, e1/e2 marker unreached, e3 nested proc refused"
    );
}

/// Runs each escape box (`hex selftest-box escape`, e0-e3) on `node`, ids prefixed `tag`, and holds each
/// to exiting 0: e0 a nested user namespace allowed, e1 and e2 a root-only marker beside the rootfs
/// unreached, e3 a nested proc past a mask refused.
fn escapes_held_on(lab: &Lab, node: &Node, tag: &str) {
    // A marker only root can read, a sibling of the rootfs and so outside any box's root: a box that
    // opens it has reached the host.
    let marker = lab.bundles.path().join("escape-marker");
    std::fs::write(&marker, "hex-escape-marker").unwrap();
    let marker = marker.to_string_lossy().into_owned();
    for mode in ["e0", "e1", "e2", "e3"] {
        let id = BoxId::new(&format!("{tag}-{mode}")).unwrap();
        let spec = lab.running(
            id.clone(),
            &[
                "/hex".to_owned(),
                "selftest-box".to_owned(),
                "escape".to_owned(),
                mode.to_owned(),
                marker.clone(),
            ],
        );
        node.create(spec).expect("create the escape box");
        node.start(&id).expect("start the escape box");
        let info = wait_exit(node, &id);
        assert_eq!(
            info.exit,
            Some(Exit::Code(0)),
            "escape {mode} is contained on {tag} (the box held)"
        );
        node.delete(&id).expect("delete the escape box");
    }
}

/// HEX-MNTNS-2, HEX-MNTNS-3, HEX-MNTNS-4 (decision 6's update, T-T7.1.4): ask a root node for each way of
/// making a pruned create's mount namespace (copy, empty, from its tree, and none), make a box with a
/// read-only `/usr` bind and run it, then the escape boxes. Expect each box counted under the way its seed
/// made the namespace: from its tree where the probe finds the call (Linux 7.0) and the asked node refuses
/// the create by type where it doesn't; empty on Linux 7.1 and the seed's copy below it (the seed's
/// fallback, counted as made); the default the probe's. Expect `/usr` read-only and the root writable in
/// each box, every escape held, no create through the shim, and the agent's own mount table the same
/// after as before.
fn mount_namespaces_made_each_way(lab: &Lab, own: &Path) {
    use hex_runtime::linux::oci::mntns::{self, MountNamespace, OpenTreeNamespace};
    if lab.rootless {
        return;
    }
    let probe = mntns::probe().expect("probe open_tree(OPEN_TREE_NAMESPACE)");
    let empty_here = kernel_at_least(7, 1);
    eprintln!("probe {probe:?}");
    let agent_before = agent_mount_points();
    for (asked, want) in [
        ("copy", Some(MountNamespace::Copy)),
        (
            "empty",
            Some(if empty_here {
                MountNamespace::Empty
            } else {
                MountNamespace::Copy
            }),
        ),
        (
            "tree",
            if probe == OpenTreeNamespace::Available {
                Some(MountNamespace::FromTree)
            } else {
                None
            },
        ),
        ("default", Some(mntns::default_way(probe).unwrap())),
    ] {
        let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
        let node = Node::open(
            &own.join(format!("hex-oci-mntns-{asked}")),
            state.path(),
            state.path(),
        )
        .expect("open a node");
        let node = if asked == "default" {
            node
        } else {
            node.with_mount_namespace(asked)
        }
        .with_oci_helper(PathBuf::from(&lab.exe));
        let id = BoxId::new(&format!("mntns-{asked}")).unwrap();
        let dir = tempfile::Builder::new()
            .prefix("bundle-")
            .tempdir_in(lab.bundles.path())
            .unwrap()
            .keep();
        std::fs::write(
            dir.join("config.json"),
            lab.config_with(
                &Lab::act(&Act::Wait),
                r#"{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"}"#,
                "",
                "",
            ),
        )
        .unwrap();
        let made = node.create(BoxSpec {
            id: id.clone(),
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(dir),
            owner: None,
        });
        let Some(want) = want else {
            assert!(
                made.is_err(),
                "a namespace from its tree asked where it can't be made is refused ({probe:?})"
            );
            continue;
        };
        made.unwrap_or_else(|e| panic!("create a box with its namespace made {asked}: {e}"));
        node.start(&id).expect("start");
        let (mi_exit, mi) = run_in(&node, &id, &["/usr/bin/cat", "/proc/self/mountinfo"]);
        let root_write = exit_in(&node, &id, &["/usr/bin/touch", "/written"]);
        node.delete(&id).expect("delete");
        assert_eq!(
            node.mount_namespaces_made(want),
            1,
            "asked {asked}, the box's namespace was made {want:?}"
        );
        assert_eq!(node.creates_through_shim(), 0, "asked {asked}: no shim");
        assert_eq!(mi_exit, Exit::Code(0), "read the box's mountinfo: {mi}");
        let mounts = mountinfo(&mi);
        let usr = mounts
            .iter()
            .rev()
            .find(|(point, ..)| point == "/usr")
            .unwrap_or_else(|| panic!("/usr in the box made {asked}: {mi}"));
        assert!(
            usr.1.split(',').any(|o| o == "ro"),
            "/usr read-only: {usr:?}"
        );
        assert_eq!(
            root_write,
            Exit::Code(0),
            "asked {asked}: the root is writable"
        );
        escapes_held_on(lab, &node, &format!("esc-{asked}"));
        assert_eq!(
            node.creates_through_shim(),
            0,
            "asked {asked}: escapes made pruned"
        );
        assert_eq!(
            node.mount_namespaces_made(want),
            5,
            "asked {asked}: the escape boxes' namespaces were made {want:?} too"
        );
        eprintln!("mount namespace asked {asked}: made {want:?}, escapes held");
    }
    assert_eq!(
        agent_mount_points(),
        agent_before,
        "the agent's mount table is unchanged by every way"
    );
}

/// HEX-MNTNS-6 (decision 6's update, part 2; T-T7.1.4): on a root node, make two boxes whose spec masks and
/// makes read-only paths beneath `/proc`, the second once the node has made a proc ahead. Expect the second's
/// proc taken from the pool (counted), its init pid 1 of the pid namespace that proc shows, and each box to
/// see the same: every mask and read-only path at the same point with the same read-only flag, a masked
/// file empty, a masked directory empty, a read-only path refusing a write, a missing path skipped.
fn procs_made_ahead_carry_their_masks(lab: &Lab, own: &Path) {
    if lab.rootless {
        return;
    }
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-procs"), state.path());
    let paths = r#","maskedPaths":["/proc/kcore","/proc/keys","/proc/timer_list","/proc/acpi","/proc/scsi","/proc/not-there"],"readonlyPaths":["/proc/sys","/proc/sysrq-trigger","/proc/bus"]"#;
    let dir = tempfile::Builder::new()
        .prefix("bundle-")
        .tempdir_in(lab.bundles.path())
        .unwrap()
        .keep();
    std::fs::write(
        dir.join("config.json"),
        lab.config_ex(
            &Lab::act(&Act::Wait),
            r#"{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"}"#,
            "",
            "",
            "",
            paths,
        ),
    )
    .unwrap();
    // What a box sees of its proc: each mount beneath it with whether it is read-only, a masked file's
    // size, whether a masked directory lists anything, whether a read-only path takes a write, and its
    // pid 1's command line.
    let view = |id: &BoxId| {
        node.create(BoxSpec {
            id: id.clone(),
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(dir.clone()),
            owner: None,
        })
        .expect("create a box with masks beneath /proc");
        node.start(id).expect("start");
        let (_, mi) = run_in(&node, id, &["/usr/bin/cat", "/proc/self/mountinfo"]);
        let (_, kcore) = run_in(&node, id, &["/usr/bin/wc", "-c", "/proc/kcore"]);
        let (_, acpi) = run_in(&node, id, &["/usr/bin/ls", "-A", "/proc/acpi"]);
        let sysrq = exit_in(
            &node,
            id,
            &["/usr/bin/sh", "-c", "echo h > /proc/sysrq-trigger"],
        );
        let (_, init) = run_in(&node, id, &["/usr/bin/cat", "/proc/1/cmdline"]);
        node.delete(id).expect("delete");
        let beneath: BTreeSet<(String, bool)> = mountinfo(&mi)
            .into_iter()
            .filter(|(point, ..)| point.starts_with("/proc/"))
            .map(|(point, opts, ..)| (point, opts.split(',').any(|o| o == "ro")))
            .collect();
        (beneath, kcore, acpi, sysrq, init)
    };
    // Below Linux 7.1 the kernel won't hand over a pid namespace with no process in it, so the node makes
    // none ahead: its pool stops at its first try, and both boxes are made the ordinary way.
    let ahead = kernel_at_least(7, 1);
    let before = node.proc_mounts().unwrap();
    let first = view(&BoxId::new("procs-a").unwrap());
    wait_for(
        || {
            let c = node.proc_mounts().unwrap();
            if ahead {
                c.made > before.made
            } else {
                c.failed_namespace > before.failed_namespace
            }
        },
        "the node's first try at a proc made ahead",
    );
    let taken = node.proc_mounts().unwrap().taken;
    let second = view(&BoxId::new("procs-b").unwrap());
    let after = node.proc_mounts().unwrap();
    if ahead {
        assert_eq!(
            after.taken,
            taken + 1,
            "the second box's proc was made ahead"
        );
    } else {
        assert_eq!(after.taken, taken, "no proc made ahead below 7.1");
        assert_eq!(
            after.disabled,
            before.disabled + 1,
            "the second create counted disabled"
        );
    }
    assert_eq!(node.creates_through_shim(), 0, "both boxes made pruned");
    for (which, (beneath, kcore, acpi, sysrq, init)) in [("first", &first), ("second", &second)] {
        for point in ["/proc/kcore", "/proc/keys", "/proc/timer_list"] {
            assert!(
                beneath.iter().any(|(p, _)| p == point),
                "{which}: {point} is masked: {beneath:?}"
            );
        }
        for point in ["/proc/sys", "/proc/sysrq-trigger", "/proc/bus"] {
            assert!(
                beneath.contains(&(point.to_owned(), true)),
                "{which}: {point} is read-only: {beneath:?}"
            );
        }
        assert!(
            !beneath.iter().any(|(p, _)| p == "/proc/not-there"),
            "{which}: a missing path is skipped"
        );
        assert_eq!(kcore.split_whitespace().next(), Some("0"), "{which}: kcore");
        assert_eq!(acpi.trim(), "", "{which}: a masked directory is empty");
        assert_ne!(
            *sysrq,
            Exit::Code(0),
            "{which}: a read-only path refuses a write"
        );
        assert!(
            init.contains("selftest-box"),
            "{which}: pid 1 is the box's init: {init:?}"
        );
    }
    assert_eq!(
        first.0, second.0,
        "the made-ahead proc's mounts are the ordinary one's"
    );
    eprintln!(
        "procs made ahead: taken {}, made {}, the masks and read-only paths as ordinary setup's",
        after.taken, after.made
    );
}

/// HEX-MNTNS-8 (decision 6's update; T-T7.1.4): on a root node, make two boxes on the pruned path and two through
/// the shim (a createRuntime hook), each masking Docker's directories beneath `/proc`, with procs made ahead off
/// so each init masks them itself. Expect every masked directory of all four boxes on one filesystem, the node's
/// one empty read-only tmpfs, as crun binds one empty directory: no superblock made per masked directory. Where
/// the kernel won't clone a detached mount (before Linux 6.15, v6.8 `fs/namespace.c:2588`), expect each box made
/// all the same, each masked directory an empty read-only tmpfs of its own.
fn masked_directories_are_the_nodes_one_empty_filesystem(lab: &Lab, own: &Path) {
    if lab.rootless {
        return;
    }
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab
        .node(&own.join("hex-oci-maskdirs"), state.path())
        .with_prepared_procs(false)
        .expect("procs made ahead off");
    let paths = r#","maskedPaths":["/proc/acpi","/proc/asound","/proc/scsi","/proc/kcore"]"#;
    let hooks = r#","hooks":{"createRuntime":[{"path":"/usr/bin/true","args":["true"]}]}"#;
    let mut devs = BTreeSet::new();
    let mut masked = 0;
    for (n, (path, hook)) in [
        ("pruned", ""),
        ("pruned", ""),
        ("shim", hooks),
        ("shim", hooks),
    ]
    .into_iter()
    .enumerate()
    {
        let dir = tempfile::Builder::new()
            .prefix("bundle-")
            .tempdir_in(lab.bundles.path())
            .unwrap()
            .keep();
        std::fs::write(
            dir.join("config.json"),
            lab.config_ex(
                &Lab::act(&Act::Wait),
                r#"{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"}"#,
                "",
                "",
                hook,
                paths,
            ),
        )
        .unwrap();
        let id = BoxId::new(&format!("maskdirs-{n}")).unwrap();
        let shim = node.creates_through_shim();
        node.create(BoxSpec {
            id: id.clone(),
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(dir),
            owner: None,
        })
        .expect("create a box masking directories");
        assert_eq!(
            node.creates_through_shim() - shim,
            u64::from(path == "shim"),
            "box {n} made {path}"
        );
        node.start(&id).expect("start");
        let (_, mi) = run_in(&node, &id, &["/usr/bin/cat", "/proc/self/mountinfo"]);
        node.delete(&id).expect("delete");
        // proc_pid_mountinfo(5): the third field is the filesystem's major:minor, the fifth the mount point.
        for line in mi.lines() {
            let fields: Vec<&str> = line.split(' ').collect();
            if let (Some(dev), Some(point)) = (fields.get(2), fields.get(4))
                && ["/proc/acpi", "/proc/asound", "/proc/scsi"].contains(point)
            {
                // The fields after " - ": the filesystem's type, its source, its superblock's options.
                let fs = line.split(" - ").nth(1).unwrap_or_default();
                assert!(
                    fs.starts_with("tmpfs ") && fields.get(5).is_some_and(|o| o.starts_with("ro")),
                    "{point} in box {n} is masked by a read-only tmpfs: {line}"
                );
                devs.insert((*dev).to_owned());
                masked += 1;
            }
        }
    }
    assert!(masked >= 4, "masked directories to compare: {masked}");
    if kernel_clones_a_detached_mount() {
        assert_eq!(
            devs.len(),
            1,
            "one empty filesystem for every masked directory of every box: {devs:?}"
        );
    } else {
        assert_eq!(
            devs.len(),
            masked,
            "a tmpfs of its own for every masked directory: {devs:?}"
        );
    }
    eprintln!("masked directories: {masked} across 4 boxes, on {devs:?}");
}

/// Whether `open_tree(OPEN_TREE_CLONE)` clones a mount outside the caller's namespace, a detached tmpfs's: Linux
/// 6.15's `may_copy_tree` admits it (v6.19 `fs/namespace.c:2928-2948`), v6.8 refuses it with `EINVAL` (`:2588`).
fn kernel_clones_a_detached_mount() -> bool {
    use rustix::mount::{FsMountFlags, FsOpenFlags, MountAttrFlags, OpenTreeFlags};
    use std::os::fd::AsFd;
    let fs = rustix::mount::fsopen("tmpfs", FsOpenFlags::FSOPEN_CLOEXEC).expect("fsopen tmpfs");
    rustix::mount::fsconfig_create(fs.as_fd()).expect("make the tmpfs");
    let mount = rustix::mount::fsmount(
        fs.as_fd(),
        FsMountFlags::FSMOUNT_CLOEXEC,
        MountAttrFlags::empty(),
    )
    .expect("mount the tmpfs, detached");
    match rustix::mount::open_tree(
        &mount,
        "",
        OpenTreeFlags::OPEN_TREE_CLONE
            | OpenTreeFlags::AT_EMPTY_PATH
            | OpenTreeFlags::OPEN_TREE_CLOEXEC,
    ) {
        Ok(_) => true,
        Err(rustix::io::Errno::INVAL) => false,
        Err(e) => panic!("cloning a detached tmpfs: {e}"),
    }
}

/// Goal 5 plan step 2 (T-T7.1.4): a spec binding `/usr` read-only and a volume. Both are in the
/// container's mountinfo with the options asked, and the bundle's config.json and the node's state
/// directory are unreachable from inside — the pruned root holds nothing of the host but what the spec
/// mounts. Root only: a pruned seed is a root node's, and the pruned path is what keeps the host out.
fn a_bind_and_a_volume_are_present_the_host_unreachable(lab: &Lab, own: &Path) {
    if lab.rootless {
        return;
    }
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-vol"), state.path());
    let vol = lab.bundles.path().join("volume");
    std::fs::create_dir_all(&vol).unwrap();
    std::fs::write(vol.join("marker"), "from the volume\n").unwrap();
    let volume = format!(
        ",\n    {{\"destination\": \"/vol\", \"type\": \"bind\", \"source\": \"{}\", \"options\": [\"rbind\",\"rw\"]}}",
        vol.display()
    );
    let dir = tempfile::Builder::new()
        .prefix("bundle-")
        .tempdir_in(lab.bundles.path())
        .unwrap()
        .keep();
    std::fs::write(
        dir.join("config.json"),
        lab.config_with(
            &Lab::act(&Act::Wait),
            r#"{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"}"#,
            "",
            &volume,
        ),
    )
    .unwrap();
    let id = BoxId::new("vol").unwrap();
    node.create(BoxSpec {
        id: id.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Bundle(dir.clone()),
        owner: None,
    })
    .expect("create a box with a read-only bind and a volume");
    node.start(&id).expect("start");
    let pruned = node.pruned_creates();
    // Gather what the box sees before deleting, so a failed check leaves no box running.
    let (mi_exit, mi) = run_in(&node, &id, &["/usr/bin/cat", "/proc/self/mountinfo"]);
    let (vol_exit, vol_out) = run_in(&node, &id, &["/usr/bin/cat", "/vol/marker"]);
    let config = dir.join("config.json").to_string_lossy().into_owned();
    let state_path = state.path().to_string_lossy().into_owned();
    let config_reach = exit_in(&node, &id, &["/usr/bin/test", "-e", &config]);
    let state_reach = exit_in(&node, &id, &["/usr/bin/test", "-e", &state_path]);
    node.delete(&id).expect("delete");

    assert!(
        pruned >= 1,
        "the bind-and-volume box is made on the pruned path, {pruned} pruned"
    );
    assert_eq!(mi_exit, Exit::Code(0), "read the box's mountinfo: {mi}");
    let mounts = mountinfo(&mi);
    let usr = mounts
        .iter()
        .rev()
        .find(|(point, ..)| point == "/usr")
        .expect("/usr in the box");
    assert!(
        usr.1.split(',').any(|o| o == "ro"),
        "/usr is read-only: {usr:?}"
    );
    let vol_mount = mounts
        .iter()
        .rev()
        .find(|(point, ..)| point == "/vol")
        .expect("/vol in the box");
    assert!(
        vol_mount.1.split(',').any(|o| o == "rw"),
        "/vol is read-write: {vol_mount:?}"
    );
    assert_eq!(
        (vol_exit, vol_out.trim()),
        (Exit::Code(0), "from the volume"),
        "the volume's content is there"
    );
    assert_ne!(
        config_reach,
        Exit::Code(0),
        "the bundle's config.json is unreachable from inside the box"
    );
    assert_ne!(
        state_reach,
        Exit::Code(0),
        "the node's state directory is unreachable from inside the box"
    );
    eprintln!(
        "bind+volume: /usr read-only and /vol read-write are in the box; config.json and the state dir are unreachable"
    );
}

/// Whether this kernel's release is at least `major.minor`, read from `/proc/sys/kernel/osrelease`, and said.
fn kernel_at_least(major: u32, minor: u32) -> bool {
    let release = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap();
    let mut v = release
        .trim()
        .split(['.', '-'])
        .map(|p| p.parse::<u32>().unwrap_or(0));
    let here = (v.next().unwrap(), v.next().unwrap());
    eprintln!("kernel {}", release.trim());
    here >= (major, minor)
}

/// `argv` in box `id` through the seam, its exit and stdout once it ends.
fn run_in(rt: &dyn Runtime, id: &BoxId, argv: &[&str]) -> (Exit, String) {
    let (mut out, w) = std::io::pipe().unwrap();
    let mut proc = rt
        .exec(
            id,
            hex_api::Program::of(argv),
            hex_api::Stdio {
                stdout: Some(w),
                ..hex_api::Stdio::null()
            },
        )
        .expect("exec in the box");
    let mut s = String::new();
    std::io::Read::read_to_string(&mut out, &mut s).unwrap();
    let exit = proc
        .wait_for(conformance::PATIENCE)
        .unwrap()
        .expect("the program ends");
    (exit, s)
}

/// `argv` in box `id`, its exit alone.
fn exit_in(rt: &dyn Runtime, id: &BoxId, argv: &[&str]) -> Exit {
    run_in(rt, id, argv).0
}

/// Each `/proc/<pid>/mountinfo` line as (mount point, per-mount options, optional/propagation fields,
/// filesystem type) (proc_pid_mountinfo(5)); tuples, so an unread field is no dead-code warning.
fn mountinfo(text: &str) -> Vec<(String, String, String, String)> {
    text.lines()
        .filter_map(|line| {
            let (left, right) = line.split_once(" - ")?;
            let left: Vec<&str> = left.split(' ').collect();
            Some((
                (*left.get(4)?).to_owned(),
                (*left.get(5)?).to_owned(),
                left.get(6..).map(|o| o.join(" ")).unwrap_or_default(),
                right.split(' ').next().unwrap_or("").to_owned(),
            ))
        })
        .collect()
}

/// The mount points in this process's (the agent's) own mount namespace, as a set, so a failed create
/// can be shown to have left none of a container's mounts behind (goal 5 plan step 2).
fn agent_mount_points() -> std::collections::BTreeSet<String> {
    mountinfo(&std::fs::read_to_string("/proc/self/mountinfo").expect("read the agent's mountinfo"))
        .into_iter()
        .map(|(point, ..)| point)
        .collect()
}

/// Goal 5 plan step 2 (T-T7.1.4): creates that can't take the pruned path take the classic path, each
/// counted under its reason. Rootless, where the node's seeds don't prune: a create is counted under
/// `Rootless`, made by a seed (not through the shim) and not pruned. As root, specs a pruned seed can't
/// serve: a user namespace of its own (`UserNamespace`), createContainer hooks (`Hooks`) and a mount
/// namespace joined by path (`JoinsByPath`), each through the shim, and none counted under `Rootless`.
/// A create routes classic at create — the counter bumps there, before the box runs — so creating then
/// deleting each is enough to count it.
fn classic_reasons_are_counted(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-reasons"), state.path());
    if lab.rootless {
        let id = BoxId::new("reason-rootless").unwrap();
        let before = node.classic_creates_for(ClassicReason::Rootless);
        node.create(lab.running(id.clone(), &Lab::act(&Act::Wait)))
            .expect("create a box on the rootless node");
        assert_eq!(
            node.classic_creates_for(ClassicReason::Rootless),
            before + 1,
            "a rootless node's create is counted under Rootless"
        );
        assert_eq!(
            node.creates_through_shim(),
            0,
            "a rootless node's create is made by a seed, not through the shim"
        );
        assert_eq!(
            node.pruned_creates(),
            0,
            "a rootless node's seeds don't prune"
        );
        node.delete(&id).expect("delete the rootless box");
        eprintln!(
            "classic reasons: a rootless node's create counted under Rootless, made by a seed, none pruned"
        );
        return;
    }
    // A mount namespace to join by path: a running box's init has one of its own.
    let host = BoxId::new("reason-host").unwrap();
    node.create(lab.running(host.clone(), &Lab::act(&Act::Wait)))
        .expect("create the box whose mount namespace is joined");
    node.start(&host).expect("start it");
    let host_mnt = format!("/proc/{}/ns/mnt", init_pid(&node.container_dir(&host)));
    let base = r#"{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"}"#;
    let userns = r#"{"type":"user"},{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"}"#;
    let mappings = r#","uidMappings":[{"containerID":0,"hostID":0,"size":1}],"gidMappings":[{"containerID":0,"hostID":0,"size":1}]"#;
    let by_path = format!(
        r#"{{"type":"pid"}},{{"type":"mount","path":"{host_mnt}"}},{{"type":"ipc"}},{{"type":"uts"}}"#
    );
    let hooks = r#","hooks":{"createContainer":[{"path":"/usr/bin/true","args":["true"]}]}"#;
    let cases: [(&str, ClassicReason, &str, &str, &str); 3] = [
        ("userns", ClassicReason::UserNamespace, userns, mappings, ""),
        ("hooks", ClassicReason::Hooks, base, "", hooks),
        (
            "mount-by-path",
            ClassicReason::JoinsByPath,
            &by_path,
            "",
            "",
        ),
    ];
    for (name, reason, namespaces, mappings, hooks) in cases {
        let (shim_before, reason_before, pruned_before) = (
            node.creates_through_shim(),
            node.classic_creates_for(reason),
            node.pruned_creates(),
        );
        let dir = tempfile::Builder::new()
            .prefix("bundle-")
            .tempdir_in(lab.bundles.path())
            .unwrap()
            .keep();
        std::fs::write(
            dir.join("config.json"),
            lab.config_ex(&Lab::act(&Act::Wait), namespaces, mappings, "", hooks, ""),
        )
        .unwrap();
        let id = BoxId::new(&format!("reason-{name}")).unwrap();
        // The create routes classic at create (the counter bumps there) whether or not the classic path
        // then completes; delete cleans it up if it did.
        let _ = node.create(BoxSpec {
            id: id.clone(),
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(dir),
            owner: None,
        });
        let _ = node.delete(&id);
        assert_eq!(
            node.classic_creates_for(reason),
            reason_before + 1,
            "{name} is counted under {reason:?}"
        );
        assert_eq!(
            node.creates_through_shim(),
            shim_before + 1,
            "{name} took the classic path"
        );
        assert_eq!(
            node.pruned_creates(),
            pruned_before,
            "{name} did not take the pruned path"
        );
    }
    assert_eq!(
        node.classic_creates_for(ClassicReason::Rootless),
        0,
        "a root node counts no create under Rootless"
    );
    node.delete(&host).expect("delete the host box");
    eprintln!(
        "classic reasons: a user namespace (UserNamespace), createContainer hooks (Hooks) and a mount namespace by path (JoinsByPath) each counted, none pruned, none under Rootless"
    );
}

/// Goal 5 plan step 2 (T-T7.1.4): rootfsPropagation shared, slave and private each show on the box's
/// root in its own mountinfo, and no mount the pruned init makes reaches the agent's mount namespace.
/// Each box binds a uniquely named volume; were the init's bind to propagate back to the agent (a shared
/// root sharing a peer group with the agent's mounts), that volume's source would be a mount in the
/// agent's mountinfo. Root only.
fn rootfs_propagation_shows_in_the_box_not_the_agent(lab: &Lab, own: &Path) {
    if lab.rootless {
        return;
    }
    // v6.19 fs/namespace.c:1259-1288: clone_mnt keeps the source's peer/master unless asked to
    // change it. open_tree's detached copy calls __do_loopback (:3032, :2963-2966). A private
    // source cannot acquire a master by MS_SLAVE: fs/pnode.c:93-117 and mount_namespaces(7),
    // "Propagation type transitions". The lab must establish its own shared inputs.
    struct SharedSource(PathBuf);
    impl SharedSource {
        fn make(path: &Path) -> Self {
            rustix::mount::mount_bind(path, path).unwrap_or_else(|e| {
                panic!("binding a propagation-test source {}: {e}", path.display())
            });
            let source = Self(path.to_owned());
            rustix::mount::mount_change(path, rustix::mount::MountPropagationFlags::PRIVATE)
                .unwrap_or_else(|e| panic!("isolating a propagation-test source: {e}"));
            rustix::mount::mount_change(path, rustix::mount::MountPropagationFlags::SHARED)
                .unwrap_or_else(|e| panic!("sharing a propagation-test source: {e}"));
            source
        }
        fn peer(&self) -> String {
            let text = std::fs::read_to_string("/proc/self/mountinfo").unwrap();
            let mounts = mountinfo(&text);
            let source = mounts
                .iter()
                .rev()
                .find(|(point, ..)| Path::new(point) == self.0)
                .unwrap_or_else(|| panic!("source is not a mount: {}", self.0.display()));
            source
                .2
                .split_whitespace()
                .find_map(|tag| tag.strip_prefix("shared:"))
                .unwrap_or_else(|| panic!("source must be shared before create: {source:?}"))
                .to_owned()
        }
    }
    impl Drop for SharedSource {
        fn drop(&mut self) {
            let private = rustix::mount::mount_change(
                &self.0,
                rustix::mount::MountPropagationFlags::PRIVATE
                    | rustix::mount::MountPropagationFlags::REC,
            );
            let detached = rustix::mount::unmount(&self.0, rustix::mount::UnmountFlags::DETACH);
            if private.is_err() || detached.is_err() {
                if std::thread::panicking() {
                    eprintln!("shared-source cleanup: private={private:?}, detach={detached:?}");
                } else {
                    panic!("shared-source cleanup: private={private:?}, detach={detached:?}");
                }
            }
        }
    }
    struct Created<'a> {
        node: &'a Node,
        id: BoxId,
        live: bool,
    }
    impl Created<'_> {
        fn delete(&mut self) {
            self.node
                .delete(&self.id)
                .expect("delete the propagation-test box");
            self.live = false;
        }
    }
    impl Drop for Created<'_> {
        fn drop(&mut self) {
            if self.live
                && let Err(e) = self.node.delete(&self.id)
            {
                if std::thread::panicking() {
                    eprintln!("propagation-test box cleanup: {e}");
                } else {
                    panic!("propagation-test box cleanup: {e}");
                }
            }
        }
    }
    let table = || {
        std::fs::read_to_string("/proc/self/mountinfo")
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>()
    };
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab
        .node(&own.join("hex-oci-prop"), state.path())
        .with_prepared_dev_mounts(false)
        .unwrap();
    for prop in ["plain", "shared", "slave", "private"] {
        let case = tempfile::Builder::new()
            .prefix("shared-sources-")
            .tempdir_in(lab.bundles.path())
            .unwrap();
        let rootfs = case.path().join("rootfs");
        build_rootfs(&rootfs);
        let bound = case.path().join("bind-source");
        std::fs::create_dir_all(bound.join("child")).unwrap();
        std::fs::write(bound.join("masked"), "source file").unwrap();
        std::fs::write(bound.join("readonly"), "source file").unwrap();
        let root_source = SharedSource::make(&rootfs);
        let bind_source = SharedSource::make(&bound);
        let root_peer = root_source.peer();
        let bind_peer = bind_source.peer();
        eprintln!("{prop}: root source shared:{root_peer}, bind source shared:{bind_peer}");
        let before = table();
        let mut config: serde_json::Value =
            serde_json::from_str(&lab.config(&Lab::act(&Act::Wait), false)).unwrap();
        config["root"]["path"] = serde_json::json!(rootfs);
        if prop != "plain" {
            config["linux"]["rootfsPropagation"] = serde_json::json!(prop);
        }
        config["linux"]["maskedPaths"] = serde_json::json!(["/proc/kcore", "/shared-bind/masked"]);
        config["linux"]["readonlyPaths"] =
            serde_json::json!(["/proc/sys", "/shared-bind/readonly"]);
        config["mounts"].as_array_mut().unwrap().extend([
            serde_json::json!({"destination":"/dev/pts", "type":"devpts", "source":"devpts",
                "options":["nosuid","noexec","newinstance","ptmxmode=0666","mode=0620"]}),
            serde_json::json!({"destination":"/dev/shm", "type":"tmpfs", "source":"shm",
                "options":["nosuid","nodev","mode=1777","size=64k"]}),
            serde_json::json!({"destination":"/dev/mqueue", "type":"mqueue", "source":"mqueue"}),
            serde_json::json!({"destination":"/shared-bind", "type":"bind", "source":bound,
                "options":["rbind","rw"]}),
            serde_json::json!({"destination":"/shared-bind/child", "type":"tmpfs", "source":"bind-child",
                "options":["mode=700","size=64k"]}),
        ]);
        std::fs::write(
            case.path().join("config.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        let id = BoxId::new(&format!("prop-{prop}")).unwrap();
        let pruned = node.pruned_creates();
        node.create(BoxSpec {
            id: id.clone(),
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(case.path().to_owned()),
            owner: None,
        })
        .unwrap_or_else(|e| panic!("create the {prop} shared-source box: {e:?}"));
        let mut made = Created {
            node: &node,
            id: id.clone(),
            live: true,
        };
        assert_eq!(node.pruned_creates(), pruned + 1, "{prop} must run pruned");
        let during = table();
        let gained: Vec<_> = during.difference(&before).collect();
        assert_eq!(
            during, before,
            "{prop}: agent gained mounts during create: {gained:?}"
        );
        node.start(&id).expect("start the propagation-test box");
        let (exit, mi) = run_in(&node, &id, &["/usr/bin/cat", "/proc/self/mountinfo"]);
        assert_eq!(exit, Exit::Code(0), "read the {prop} box's mountinfo: {mi}");
        let mounts = mountinfo(&mi);
        let root = mounts
            .iter()
            .rev()
            .find(|(point, ..)| point == "/")
            .unwrap_or_else(|| panic!("{prop}: box has no root mount: {mi}"));
        let tags: Vec<_> = root.2.split_whitespace().collect();
        match prop {
            "slave" => {
                assert!(
                    tags.contains(&format!("master:{root_peer}").as_str()),
                    "{prop}: exact root master:{root_peer} required: {root:?}"
                );
                assert!(
                    !tags.iter().any(|tag| tag.starts_with("shared:")),
                    "{root:?}"
                );
            }
            "shared" => {
                let peer = tags
                    .iter()
                    .find_map(|tag| tag.strip_prefix("shared:"))
                    .unwrap_or_else(|| panic!("shared root has no peer group: {root:?}"));
                assert_ne!(
                    peer, root_peer,
                    "box must not share its source's peer group"
                );
            }
            _ => assert!(
                !tags
                    .iter()
                    .any(|tag| tag.starts_with("shared:") || tag.starts_with("master:")),
                "{prop}: root must be private: {root:?}"
            ),
        }
        let bound_mount = mounts
            .iter()
            .find(|(point, ..)| point == "/shared-bind")
            .unwrap();
        assert!(
            !bound_mount
                .2
                .split_whitespace()
                .any(|tag| tag.starts_with("shared:") || tag.starts_with("master:")),
            "the default bind must be private: {bound_mount:?}"
        );
        assert_eq!(table(), before, "{prop}: agent changed while box ran");
        made.delete();
        let after = table();
        let gained: Vec<_> = after.difference(&before).collect();
        assert_eq!(
            after, before,
            "{prop}: agent gained mounts after delete: {gained:?}"
        );
    }
    eprintln!(
        "rootfsPropagation plain/shared/slave/private: shared root and bind inputs checked; agent table unchanged before/during/after each pruned box"
    );
}

/// Goal 5 plan step 2 (T-T7.1.4; CLAUDE.md §3, §6): the pruned init refusing or dying mid-build leaves
/// nothing of the container in the agent's own namespace. Two ways in, both after the rootfs is attached
/// and before the root switch: (1) a filesystem mount of a type the kernel doesn't have — the init
/// refuses at `fsopen`, by type, naming the mount and why; (2) the init killed between its mounts' attach
/// and the `chroot` (a debug-only crash point armed on the seed) — refused by type, its mounts
/// dying with its own namespace. Each time: the box is gone, its directory gone, the agent's mounts
/// unchanged. Root only: a pruned seed is a root node's, and the pruned path is what keeps the
/// container's mounts out of the agent's namespace.
fn an_init_that_refuses_or_dies_mid_build_leaves_nothing(lab: &Lab, own: &Path) {
    if lab.rootless {
        return;
    }

    // (1) An unattachable filesystem mount: the pruned init reaches it after attaching the rootfs and
    // refuses at `fsopen`, by type, naming the mount and why; the agent then leaves nothing and serves on.
    {
        let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
        let box_root = own.join("hex-oci-attach-fail");
        let node = lab.node(&box_root, state.path());
        let bad =
            ",\n    {\"destination\": \"/bad\", \"type\": \"nosuchfs\", \"source\": \"nosuchfs\"}";
        let dir = tempfile::Builder::new()
            .prefix("bundle-")
            .tempdir_in(lab.bundles.path())
            .unwrap()
            .keep();
        std::fs::write(
            dir.join("config.json"),
            lab.config_with(
                &Lab::act(&Act::Wait),
                r#"{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"}"#,
                "",
                bad,
            ),
        )
        .unwrap();
        let id = BoxId::new("attach-fail").unwrap();
        let before = agent_mount_points();
        let refused = node.create(BoxSpec {
            id: id.clone(),
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(dir.clone()),
            owner: None,
        });
        assert!(
            matches!(&refused, Err(Refusal::Os { why, .. }) if why.contains("nosuchfs") && why.contains("/bad")),
            "the unattachable mount is refused by type, naming the mount and why: {refused:?}"
        );
        assert_eq!(
            node.creates_through_shim(),
            0,
            "the create took the pruned path, not a classic fallback"
        );
        assert_eq!(
            node.inspect(&id),
            Err(Refusal::NoSuchBox(id.clone())),
            "the refused box is cleaned up"
        );
        assert!(
            !box_root.join(OCI_BOXES).join(id.to_string()).exists(),
            "the refused box's directory is gone"
        );
        assert_eq!(
            agent_mount_points(),
            before,
            "no mount of the refused container is in the agent's namespace"
        );
        // The node serves a clean create after the refusal: the pruned path isn't wedged.
        let ok = BoxId::new("attach-fail-then-ok").unwrap();
        node.create(lab.running(ok.clone(), &Lab::act(&Act::Wait)))
            .expect("the next create succeeds");
        node.start(&ok).expect("start the next box");
        assert_eq!(node.inspect(&ok).unwrap().state, BoxState::Running);
        node.delete(&ok).expect("delete");
    }

    // (2) The init killed between the mounts' attach and the root switch: debug builds only, where the
    // crash point exists. Every seed of this node is armed, so it serves no clean create — the refusal
    // path's recovery is shown in (1).
    #[cfg(debug_assertions)]
    {
        let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
        let box_root = own.join("hex-oci-killed-build");
        let node = Node::open(&box_root, state.path(), state.path())
            .expect("open a node")
            .with_inject(hex_runtime::linux::oci::seed::INJECT_DIE_BEFORE_SWITCH)
            .with_oci_helper(PathBuf::from(&lab.exe));
        let id = BoxId::new("killed-build").unwrap();
        let before = agent_mount_points();
        let refused = node.create(lab.running(id.clone(), &Lab::act(&Act::Wait)));
        assert!(
            matches!(&refused, Err(Refusal::Os { .. }) | Err(Refusal::Broken(_))),
            "the init killed mid-build refuses the create by type: {refused:?}"
        );
        assert_eq!(
            node.creates_through_shim(),
            0,
            "the killed create took the pruned path, not a classic fallback"
        );
        assert_eq!(
            node.inspect(&id),
            Err(Refusal::NoSuchBox(id.clone())),
            "the killed box is cleaned up"
        );
        assert!(
            !box_root.join(OCI_BOXES).join(id.to_string()).exists(),
            "the killed box's directory is gone"
        );
        assert_eq!(
            agent_mount_points(),
            before,
            "no mount of the killed container is in the agent's namespace"
        );
    }

    // (3) Only the node's side can arm the crash point, never a container: a box on a node that armed
    // nothing, whose own spec sets the inject variable in its process env, builds its root and runs — the
    // init never saw it (the env reaches the workload at execve, after the root is built, and the node
    // clears it from the seed besides, so no box can kill its own init through it). Debug builds only,
    // where the crash point exists to be proven inert.
    #[cfg(debug_assertions)]
    {
        let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
        let box_root = own.join("hex-oci-spec-inject");
        let node = lab.node(&box_root, state.path());
        let inject = hex_runtime::linux::oci::seed::ENV_INJECT;
        let point = hex_runtime::linux::oci::seed::INJECT_DIE_BEFORE_SWITCH;
        let dir = tempfile::Builder::new()
            .prefix("bundle-")
            .tempdir_in(lab.bundles.path())
            .unwrap()
            .keep();
        let config = lab
            .config_with(
                &Lab::act(&Act::Wait),
                r#"{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"}"#,
                "",
                "",
            )
            .replace(
                r#""env": ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"]"#,
                &format!(
                    r#""env": ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin", "{inject}={point}"]"#
                ),
            );
        assert!(
            config.contains(&format!("{inject}={point}")),
            "the box's spec carries the inject variable in its process env"
        );
        std::fs::write(dir.join("config.json"), config).unwrap();
        let id = BoxId::new("spec-inject").unwrap();
        node.create(BoxSpec {
            id: id.clone(),
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(dir),
            owner: None,
        })
        .expect("the box whose spec sets the inject variable is created: the init never saw it");
        node.start(&id).expect("start");
        assert_eq!(
            node.inspect(&id).unwrap().state,
            BoxState::Running,
            "the box runs: its own env did not arm the crash point, only the node's side can"
        );
        node.delete(&id).expect("delete");
    }

    eprintln!(
        "mid-build: an unattachable mount is refused naming it, a killed init refused, and a container's own env cannot arm the crash point; each leaves no box, directory or mount in the agent's namespace"
    );
}

/// Goal 5 plan step 4: the box bound's count is the root's link count, not a listing (a listing waits
/// behind every box's `mkdir` and `rmdir` beside it). Do: make three boxes under a box root, then remove
/// one. Expect: the count equals the listing's at each step — 0, 3, then 2.
fn a_box_root_counts_its_boxes_without_listing(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let root = BoxRoot::open(
        &own.join("hex-oci-count"),
        disks::under(state.path()).unwrap(),
    )
    .unwrap();
    assert_eq!((root.count().unwrap(), root.boxes().unwrap().len()), (0, 0));
    let ids: Vec<BoxId> = ["c1", "c2", "c3"]
        .iter()
        .map(|n| BoxId::new(n).unwrap())
        .collect();
    let made: Vec<_> = ids
        .iter()
        .map(|id| root.create(id, &Bounds::default()).unwrap())
        .collect();
    assert_eq!((root.count().unwrap(), root.boxes().unwrap().len()), (3, 3));
    made[0].remove().unwrap();
    assert_eq!((root.count().unwrap(), root.boxes().unwrap().len()), (2, 2));
    for cg in &made[1..] {
        cg.remove().unwrap();
    }
}

/// Goal 5 plan step 4 (decision 5, T-T7.1.4): a given bundle that joins a namespace by path is made
/// through the seam, as the shim made it before. Do: run box A with a network namespace of its own;
/// create and start box B whose bundle names A's network namespace by path — and, rootless, A's user
/// namespace too, which a rootless container needs to join a namespace A's user namespace owns. Expect: B
/// runs in A's network namespace (and A's user namespace, rootless). As root, B's init joined A's network
/// namespace itself after the seed's clone, so no create went through the shim; rootless, a user namespace
/// by path can't be joined in one clone, so B was made through the shim and counted once — the one kind of
/// create that still re-execs the hex binary. Either way both inits' streams are the null device, a seed's
/// and a seam shim's alike, never the node agent's.
fn a_bundle_joining_a_namespace_by_path_runs_through_the_seam(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-joinpath"), state.path());
    let host = BoxId::new("join-host").unwrap();
    node.create(lab.networked(host.clone()))
        .expect("create the box whose namespaces are joined");
    node.start(&host).expect("start it");
    let host_init = init_pid(&node.container_dir(&host));
    let path = |ns: &str| format!("/proc/{host_init}/ns/{ns}");
    let rest = r#"{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"}"#;
    let namespaces = if lab.rootless {
        format!(
            r#"{{"type":"user","path":"{}"}},{{"type":"network","path":"{}"}},{rest}"#,
            path("user"),
            path("net")
        )
    } else {
        format!(r#"{{"type":"network","path":"{}"}},{rest}"#, path("net"))
    };
    let dir = tempfile::Builder::new()
        .prefix("bundle-")
        .tempdir_in(lab.bundles.path())
        .unwrap()
        .keep();
    std::fs::write(
        dir.join("config.json"),
        lab.config_with(&Lab::act(&Act::Wait), &namespaces, "", ""),
    )
    .unwrap();
    let joiner = BoxId::new("join-path").unwrap();
    node.create(BoxSpec {
        id: joiner.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Bundle(dir),
        owner: None,
    })
    .expect("a bundle joining a namespace by path is made through the seam");
    node.start(&joiner).expect("start the joiner");
    let joined = init_pid(&node.container_dir(&joiner));
    assert_eq!(
        netns_of(joined),
        netns_of(host_init),
        "it runs in the network namespace it joined by path"
    );
    for (who, pid) in [("the seed-made box", host_init), ("the joiner", joined)] {
        assert_eq!(
            streams(pid),
            ["/dev/null"; 3].map(PathBuf::from),
            "{who}'s init has the null device for its streams"
        );
    }
    if lab.rootless {
        let userns = |pid: i32| std::fs::read_link(format!("/proc/{pid}/ns/user")).unwrap();
        assert_eq!(
            userns(joined),
            userns(host_init),
            "and the user namespace it joined"
        );
        assert_eq!(
            node.creates_through_shim(),
            1,
            "a user namespace by path is made through the shim, counted"
        );
    } else {
        assert_eq!(
            node.creates_through_shim(),
            0,
            "a network namespace by path is joined by the seed's clone, not the shim"
        );
    }
    node.delete(&joiner).expect("delete the joiner");
    node.delete(&host).expect("delete the box it joined");
}

/// The agent the crash kills: a node in this re-exec'd process makes and starts a box whose init
/// waits, says `ready <agent pid> <its container's state dir>` on stdout, and waits to be killed.
fn play_the_agent(dirs: &Path, exe: &str) -> ! {
    let lab_rootfs = dirs.join("rootfs");
    let box_root = PathBuf::from(std::fs::read_to_string(dirs.join("box_root")).unwrap());
    let state = dirs.join("state");
    let bundle = dirs.join("bundle");
    let node = Node::open(&box_root, &state, &state)
        .expect("agent: open a node")
        .with_oci_helper(PathBuf::from(exe));
    let id = BoxId::new("crash").unwrap();
    assert!(lab_rootfs.exists());
    let made = node.create(BoxSpec {
        id: id.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Bundle(bundle),
        owner: None,
    });
    match made {
        // Waits to be killed either way, as the harness ends it.
        Err(e) => println!("unavailable {e}"),
        Ok(_) => {
            node.start(&id).expect("agent: start");
            println!(
                "ready {} {}",
                std::process::id(),
                node.container_dir(&id).display()
            );
        }
    }
    loop {
        std::thread::park();
    }
}

/// T-T7.1.2 crash injection: SIGKILL the agent while its box runs; a restarted agent, not the
/// init's parent, reads the box running while the init runs and empty once it has ended (an exit it
/// couldn't collect: `Unknown`), and nothing of the box is left. `Err` when no container can be
/// made here.
fn crash_and_restart(lab: &Lab, own: &Path) -> Result<(), String> {
    let dirs = tempfile::Builder::new()
        .prefix("crash-")
        .tempdir_in(lab.bundles.path())
        .unwrap();
    let box_root = own.join("hex-oci-crash");
    let state = dirs.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(
        dirs.path().join("box_root"),
        box_root.to_string_lossy().as_bytes(),
    )
    .unwrap();
    std::os::unix::fs::symlink(&lab.rootfs, dirs.path().join("rootfs")).unwrap();
    let bundle = dirs.path().join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    std::fs::write(
        bundle.join("config.json"),
        lab.config(&Lab::act(&Act::Wait), false),
    )
    .unwrap();

    let mut agent = Command::new(std::env::current_exe().unwrap())
        .args([TEST, "--exact", "--nocapture", "--test-threads=1"])
        .env(AGENT_ENV, dirs.path())
        .stdout(Std::piped())
        .spawn()
        .expect("start the agent");
    // libtest writes `test <name> ... ` before the test's own output, on the same line.
    let mut out = BufReader::new(agent.stdout.take().unwrap());
    let (agent_pid, container): (i32, PathBuf) = loop {
        let mut line = String::new();
        assert!(
            out.read_line(&mut line).unwrap() > 0,
            "the agent ended before it was ready"
        );
        if let Some((_, ready)) = line.split_once("ready ") {
            let (pid, dir) = ready.trim().split_once(' ').unwrap();
            break (pid.parse().unwrap(), PathBuf::from(dir));
        }
        if let Some((_, why)) = line.split_once("unavailable ") {
            agent.kill().unwrap();
            agent.wait().unwrap();
            return Err(why.trim().to_owned());
        }
    };
    let init = init_pid(&container);
    assert_eq!(parent_of(init), agent_pid, "the init is the agent's child");

    agent.kill().unwrap();
    agent.wait().unwrap();
    // Orphaned again: its new parent is the agent's nearest subreaper, not this process.
    wait_for(|| parent_of(init) != agent_pid, "the init to be reparented");

    let restarted = lab.node(&box_root, &state);
    let id = BoxId::new("crash").unwrap();
    let summary = restarted.recover().expect("recover");
    assert!(summary.adopted.contains(&id), "not adopted: {summary:?}");
    let running = restarted.inspect(&id).unwrap();
    assert_eq!(
        (running.state, running.exit),
        (BoxState::Running, None),
        "while its init runs"
    );

    kill(init, 9);
    let ended = wait_exit(&restarted, &id);
    assert_eq!(
        (ended.state, ended.exit),
        (BoxState::Empty, Some(Exit::Unknown)),
        "once its init has ended"
    );
    wait_for(
        || !Path::new(&format!("/proc/{init}")).exists(),
        "the ended init to be reaped by its parent",
    );
    restarted.delete(&id).expect("delete");
    assert!(
        !box_root.join(OCI_BOXES).join("crash").exists(),
        "the box's cgroup is left"
    );
    assert!(restarted.list().unwrap().is_empty(), "a box is left");
    Ok(())
}

/// Decision 2: once create returns, the init is the agent's child; its program's exit 7 is what
/// inspect reports, and the init is reaped (its pid gone).
fn parent_and_exit_code(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-exit"), state.path());
    let id = BoxId::new("exit7").unwrap();
    let info = node
        .create(lab.running(id.clone(), &strings(&["/bin/sh", "-c", "exit 7"])))
        .expect("create");
    assert_eq!((info.state, info.exit), (BoxState::Created, None));
    let init = init_pid(&node.container_dir(&id));
    assert_eq!(
        parent_of(init),
        i32::try_from(std::process::id()).unwrap(),
        "the init's parent is the agent"
    );
    node.start(&id).expect("start");
    let ended = wait_exit(&node, &id);
    assert_eq!(
        (ended.state, ended.exit),
        (BoxState::Empty, Some(Exit::Code(7)))
    );
    assert!(
        !Path::new(&format!("/proc/{init}")).exists(),
        "the ended init is reaped"
    );
    node.delete(&id).expect("delete");
}

/// SIGKILL a running box's init: inspect reports signal 9, and no zombie of it remains.
fn killed_init(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-kill"), state.path());
    let id = BoxId::new("killed").unwrap();
    node.create(lab.running(id.clone(), &Lab::act(&Act::Wait)))
        .expect("create");
    node.start(&id).expect("start");
    let init = init_pid(&node.container_dir(&id));
    kill(init, 9);
    let ended = wait_exit(&node, &id);
    assert_eq!(
        (ended.state, ended.exit),
        (BoxState::Empty, Some(Exit::Signal(9)))
    );
    assert!(
        !Path::new(&format!("/proc/{init}")).exists(),
        "no zombie of it remains"
    );
    node.delete(&id).expect("delete");
}

/// Past the init budget create is refused, typed and naming it, and makes nothing; a delete gives
/// the place back.
fn budget(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let box_root = own.join("hex-oci-budget");
    let node = lab
        .node(&box_root, state.path())
        .with_init_budget(1)
        .unwrap();
    let wait = Lab::act(&Act::Wait);
    let first = BoxId::new("first").unwrap();
    node.create(lab.running(first.clone(), &wait))
        .expect("the first create fits the budget");
    let second = BoxId::new("second").unwrap();
    let refused = node.create(lab.running(second.clone(), &wait));
    assert_eq!(
        refused,
        Err(Refusal::Exhausted {
            what: "container init pidfds",
            bound: 1
        })
    );
    assert_eq!(
        node.inspect(&second),
        Err(Refusal::NoSuchBox(second.clone()))
    );
    assert!(
        !box_root.join(OCI_BOXES).join("second").exists(),
        "a refused create made a cgroup"
    );
    assert!(
        !node.container_dir(&second).exists(),
        "a refused create made state"
    );
    node.delete(&first).expect("delete");
    node.create(lab.running(second.clone(), &wait))
        .expect("a delete gives its place back");
    node.delete(&second).expect("delete");
}

/// AC-T7.1.1: the `oci` class through the node holds the runtime contract, HEX-RT-11 and 12 (the
/// init's exit) included.
fn conformance_on_oci(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-conf"), state.path());
    let failures = conformance::run(&node, lab);
    assert!(failures.is_empty(), "conformance failures: {failures:#?}");
}

/// Decision 4 (goal 4 plan step 4): each box whose bundle makes a network namespace of its own runs in
/// a namespace no other box has had, loopback up (its flags, read in the box from the namespace's
/// sysfs, are IFF_UP | IFF_LOOPBACK), and the node counts where the namespace came from. Namespace
/// inode numbers are compared while every box named holds its own, since the kernel reuses a freed
/// one's.
/// - HEX-NETNS-9: a node whose pool holds none (bound 0) makes a box's namespace on the create's path
///   and counts it so: made on the path 1, nothing made ahead or taken.
/// - HEX-NETNS-10, rootless, the same node: the namespace is made in the box's own user namespace,
///   which owns it, as the box's init mounting its sysfs shows (a sysfs mount takes `CAP_SYS_ADMIN` in
///   the user namespace that owns the mounter's network namespace, `kobj_ns_current_may_mount`).
/// - HEX-NETNS-8, as root: once a node's pool holds two, two boxes take one each, their namespaces
///   differ from each other's, from the first box's and from the host's, and the node keeps no
///   descriptor of either: the pool's copy is dropped once handed over.
fn network_namespaces(lab: &Lab, own: &Path) {
    let host = netns_of(std::process::id());
    let dry_state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let dry = lab
        .node(&own.join("hex-oci-netns-dry"), dry_state.path())
        .with_netns_bound(0)
        .unwrap();
    let first = BoxId::new("netns-dry").unwrap();
    dry.create(lab.networked(first.clone())).expect("create");
    dry.start(&first).expect("start");
    let made = netns_of(init_pid(&dry.container_dir(&first)));
    assert_ne!(made, host, "the box's network namespace is its own");
    assert_eq!(loopback_flags(&dry, &first), "0x9", "its loopback is up");
    let counts = dry.netns().unwrap();
    assert_eq!(
        (counts.made_ahead, counts.taken, counts.made_on_path),
        (0, 0, 1),
        "(made ahead, taken, made on the path)"
    );
    if lab.rootless {
        dry.delete(&first).expect("delete");
        return;
    }

    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-netns"), state.path());
    wait_for(
        || {
            let c = node.netns().unwrap();
            c.made_ahead >= c.taken + c.discarded + 2
        },
        "the node's pool to hold two namespaces",
    );
    let mut seen = vec![host, made];
    let mut taken = Vec::new();
    for name in ["netns-a", "netns-b"] {
        let id = BoxId::new(name).unwrap();
        node.create(lab.networked(id.clone())).expect("create");
        node.start(&id).expect("start");
        let ns = netns_of(init_pid(&node.container_dir(&id)));
        assert!(!seen.contains(&ns), "{name}'s {ns} is no other's: {seen:?}");
        assert!(
            !held_by_this_process(&ns),
            "the node keeps no descriptor of {name}'s {ns}"
        );
        assert_eq!(loopback_flags(&node, &id), "0x9", "{name}'s loopback is up");
        seen.push(ns);
        taken.push(id);
    }
    let counts = node.netns().unwrap();
    assert_eq!(
        (counts.taken, counts.made_on_path),
        (2, 0),
        "(taken, made on the path)"
    );
    for id in &taken {
        node.delete(id).expect("delete");
    }
    dry.delete(&first).expect("delete");
}

/// The network namespace process `pid` is in, as its `/proc` link names it (`net:[<inode>]`).
fn netns_of(pid: impl std::fmt::Display) -> String {
    std::fs::read_link(format!("/proc/{pid}/ns/net"))
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

/// Whether this process, the node agent, holds a descriptor of network namespace `ns`.
fn held_by_this_process(ns: &str) -> bool {
    std::fs::read_dir("/proc/self/fd").unwrap().any(|fd| {
        std::fs::read_link(fd.unwrap().path()).is_ok_and(|link| link.to_string_lossy() == ns)
    })
}

/// Loopback's flags in box `id`'s network namespace, read in the box.
fn loopback_flags(rt: &dyn Runtime, id: &BoxId) -> String {
    let (mut out, w) = std::io::pipe().unwrap();
    let mut proc = rt
        .exec(
            id,
            hex_api::Program::of(&["/usr/bin/cat", "/sys/class/net/lo/flags"]),
            hex_api::Stdio {
                stdout: Some(w),
                ..hex_api::Stdio::null()
            },
        )
        .expect("exec cat");
    let mut flags = String::new();
    std::io::Read::read_to_string(&mut out, &mut flags).unwrap();
    assert_eq!(
        proc.wait_for(conformance::PATIENCE).unwrap(),
        Some(Exit::Code(0))
    );
    flags.trim().to_owned()
}

/// Goal 3 plan step 2: no process is made per exec but the program. Exec a non-hex `sleep` into a
/// running box through the seam, and find the exec'd program is the agent's own direct child — no
/// `hex __oci-exec` helper stands beside it, as one did before the seed (agent → helper → program; now
/// agent → program) — and that a second exec spawns no new `hex __oci-seed`: the seed is reused, so no
/// `hex` binary is exec'd per call.
fn program_alone(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-alone"), state.path());
    let id = BoxId::new("alone").unwrap();
    // A non-hex, long-lived init and program, so "no hex process beside it" is plain to see.
    node.create(lab.running(id.clone(), &["/usr/bin/sleep".to_owned(), "600".to_owned()]))
        .expect("create");
    node.start(&id).expect("start");
    let agent = std::process::id();

    let mut first = node
        .exec(
            &id,
            hex_api::Program::of(&["/usr/bin/sleep", "300"]),
            hex_api::Stdio::null(),
        )
        .expect("exec a program");
    let pid = first.pid();
    assert_eq!(
        first.try_wait().expect("try_wait"),
        None,
        "the program is still running while we look at it"
    );
    // It is the agent's own child: no `hex __oci-exec` helper stands between the agent and the program.
    // Cloned straight from the agent's thread (as root) it is born so; through a rootless node's seed,
    // the seed reaps its intermediate before the agent is told, so it has reparented by the time `exec`
    // returns.
    assert_eq!(
        ppid(pid),
        Some(agent),
        "the exec'd program's parent is the node agent itself"
    );
    let children = children_of(agent);
    assert!(
        !children.iter().any(|&c| cmdline_has(c, "__oci-exec")),
        "no hex __oci-exec helper runs beside the program"
    );
    // Goal 3 plan step 2's acceptance: no process is made per exec but the program, and no hex binary is
    // exec'd per call. A node that runs as root clones the program straight from its own thread; a
    // rootless one hands the exec to a reusable seed. Either way a second exec adds to the agent's
    // children its program alone (the diff below). A `hex __oci-seed` is up whichever way the node runs:
    // the box's create is made by a create seed as of goal 5, so the seed count no longer tells a root
    // node from a rootless one — the diff does.
    assert!(
        children.iter().any(|&c| cmdline_has(c, "__oci-seed")),
        "a create seed is up, having made the box"
    );
    let before: std::collections::BTreeSet<u32> = children.into_iter().collect();
    let second = node
        .exec(
            &id,
            hex_api::Program::of(&["/usr/bin/sleep", "300"]),
            hex_api::Stdio::null(),
        )
        .expect("a second exec");
    let after: std::collections::BTreeSet<u32> = children_of(agent).into_iter().collect();
    assert_eq!(
        after.difference(&before).copied().collect::<Vec<_>>(),
        [second.pid()],
        "the second exec added no process but its program: no helper, no seed, no hex binary exec'd"
    );
    assert_eq!(
        ppid(second.pid()),
        Some(agent),
        "the second program is the agent's child too"
    );

    node.delete(&id).expect("delete ends the programs");
    // Each program was a member of the box cgroup, so delete's cgroup.kill reached it: reap it (the
    // caller's own Proc, as HEX-RT-7 does) and find it ended by a signal, no zombie left.
    for mut p in [first, second] {
        let exit = p
            .wait_for(Duration::from_secs(5))
            .expect("wait for a deleted program")
            .expect("the program ended with the box");
        assert!(
            matches!(exit, Exit::Signal(_)),
            "delete ended the program by a signal: {exit:?}"
        );
    }
}

/// A program holds its stdio alone, whatever the agent above it holds (CVE-2024-21626's class; decision 3,
/// "Descriptors reach a program only on purpose"; AC-T7.1.4). Do: leave a descriptor open in this process, the
/// agent, without close-on-exec, as a window above a program would; start a box's program and exec a second into
/// it. Expect: each comes to hold nothing past 0, 1 and 2.
fn programs_hold_their_stdio_alone(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-stdio"), state.path());
    let id = BoxId::new("stdio").unwrap();
    let stray = rustix::fs::open(
        c"/dev/null",
        rustix::fs::OFlags::RDONLY,
        rustix::fs::Mode::empty(),
    )
    .unwrap();
    node.create(lab.running(id.clone(), &["/usr/bin/sleep".to_owned(), "600".to_owned()]))
        .expect("create");
    node.start(&id).expect("start");
    let mut exec = node
        .exec(
            &id,
            hex_api::Program::of(&["/usr/bin/sleep", "300"]),
            hex_api::Stdio::null(),
        )
        .expect("exec a program");
    for pid in [
        u32::try_from(init_pid(&node.container_dir(&id))).unwrap(),
        exec.pid(),
    ] {
        // Its loader holds a descriptor for a moment after the exec, so what's waited for is their end; a
        // stray stays.
        let deadline = Instant::now() + conformance::PATIENCE;
        loop {
            let held: Vec<String> = std::fs::read_dir(format!("/proc/{pid}/fd"))
                .unwrap()
                .map(|fd| fd.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|fd| !["0", "1", "2"].contains(&fd.as_str()))
                .collect();
            if cmdline_has(pid, "/usr/bin/sleep") && held.is_empty() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "program {pid} also holds {held:?}, or never became its program"
            );
            std::thread::yield_now();
        }
    }
    drop(stray);
    node.delete(&id).expect("delete ends the programs");
    exec.wait_for(Duration::from_secs(5))
        .expect("wait for a deleted program")
        .expect("the program ended with the box");
}

/// Goal 3 plan step 6: an exec into a running box reads nothing of the box's from disk; what it needs
/// of the container is kept with the init the agent watches. Move the container's state file away, exec
/// `true` into the box, and find it ran and exited 0; with the file back, the box deletes as usual.
fn exec_reads_no_state(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-nostate"), state.path());
    let id = BoxId::new("nostate").unwrap();
    node.create(lab.running(id.clone(), &Lab::act(&Act::Wait)))
        .expect("create");
    node.start(&id).expect("start");
    let file = node.container_dir(&id).join("state.json");
    let aside = node.container_dir(&id).join("state.json.aside");
    std::fs::rename(&file, &aside).expect("move the container's state away");
    let ran = node
        .exec(
            &id,
            hex_api::Program::of(&["/usr/bin/true"]),
            hex_api::Stdio::null(),
        )
        .and_then(|mut p| p.wait_for(Duration::from_secs(5)));
    std::fs::rename(&aside, &file).expect("put the container's state back");
    assert_eq!(
        ran,
        Ok(Some(Exit::Code(0))),
        "exec ran its program with the container's state file gone"
    );
    node.delete(&id).expect("delete");
}

/// `/proc/<pid>/stat` field 4, the parent pid.
fn ppid(pid: u32) -> Option<u32> {
    stat_field(pid, 4)
}

/// `/proc/<pid>/stat` field `n` (4 the parent pid, 5 the process group, 6 the session, proc(5)), counted
/// from 1 and read past the parenthesized command name (field 2, which may itself hold spaces and
/// parentheses), as a number.
fn stat_field(pid: u32, n: usize) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, rest) = stat.rsplit_once(')')?;
    rest.split_whitespace().nth(n.checked_sub(3)?)?.parse().ok()
}

/// Whether any argument in `/proc/<pid>/cmdline` (NUL-separated) is exactly `needle`.
fn cmdline_has(pid: u32, needle: &str) -> bool {
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .map(|b| b.split(|&c| c == 0).any(|a| a == needle.as_bytes()))
        .unwrap_or(false)
}

/// The processes whose parent is `agent`, by scanning `/proc`.
fn children_of(agent: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return out;
    };
    for entry in entries.flatten() {
        if let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>()
            && ppid(pid) == Some(agent)
        {
            out.push(pid);
        }
    }
    out
}

/// A box's program forks a child and exits, orphaning the child to the agent (its subreaper); the
/// child waits on a FIFO, so it ends when the test says. Once it has ended the sweep reaps it within
/// a [`hex_node::supervise::SWEEP_PERIOD`]: its pid goes, rather than stay a zombie of the agent.
fn orphans_are_reaped(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-orphan"), state.path());
    let fifo = state.path().join("go");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let id = BoxId::new("orphaner").unwrap();
    node.create(BoxSpec {
        id: id.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Process,
        owner: None,
    })
    .unwrap();
    let (mut out, w) = std::io::pipe().unwrap();
    // stdout and stderr go before the FIFO opens (which blocks), so the orphan never holds the pipe
    // the test reads to its end.
    let script = format!("read x >/dev/null 2>&1 < {} & echo $!", fifo.display());
    let mut proc = node
        .exec(
            &id,
            hex_api::Program::of(&["/bin/sh", "-c", &script]),
            hex_api::Stdio {
                stdout: Some(w),
                ..hex_api::Stdio::null()
            },
        )
        .unwrap();
    assert_eq!(
        proc.wait_for(conformance::PATIENCE).unwrap(),
        Some(Exit::Code(0))
    );
    let mut text = String::new();
    std::io::Read::read_to_string(&mut out, &mut text).unwrap();
    let orphan: i32 = text.trim().parse().unwrap();
    assert_eq!(
        parent_of(orphan),
        i32::try_from(std::process::id()).unwrap(),
        "the orphan is the agent's"
    );
    std::fs::write(&fifo, b"go\n").unwrap();
    wait_for(
        || !Path::new(&format!("/proc/{orphan}")).exists(),
        "the ended orphan to be reaped",
    );
    node.delete(&id).unwrap();
}

/// Goal 5 plan step 2: no create waits on another box's record write, its fsyncs included. 32 boxes
/// are created at once from 32 threads; then 31 more while a box with a durable record (an `OciSpec`
/// box, its rendered bundle on disk) is held inside its record write, made once its container is (goal
/// 5 extended step 5): its temp file is a FIFO no one has opened, so the write can't go on until this
/// test reads it. All 31 are made while it's held,
/// and with the node's container state on tmpfs none of their records is synced. Let go, the held
/// write fails at its fsync (a FIFO can't be synced), which shows it was a durable one. Both runs'
/// latencies are printed: the 31 beside the held write against the 32 with none.
fn held_record_write(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-held"), state.path());
    let wait = Lab::act(&Act::Wait);
    let specs = |prefix: &str, n: usize| -> Vec<BoxSpec> {
        (0..n)
            .map(|i| lab.running(BoxId::new(&format!("{prefix}{i}")).unwrap(), &wait))
            .collect()
    };
    // Each create timed on a thread of its own, all started at once; their latencies in the order they
    // came, a create that didn't come within the patience missing.
    let create_all = |specs: Vec<BoxSpec>| -> Vec<Duration> {
        let n = specs.len();
        let (done, made) = std::sync::mpsc::channel();
        std::thread::scope(|s| {
            for spec in specs {
                let (node, done) = (&node, done.clone());
                s.spawn(move || {
                    let t = Instant::now();
                    let made = node.create(spec).map(|_| t.elapsed());
                    let _ = done.send(made);
                });
            }
            (0..n)
                .map_while(|_| made.recv_timeout(conformance::PATIENCE).ok())
                .map(|m| m.expect("create"))
                .collect()
        })
    };

    let free = specs("free", 32);
    let ids: Vec<BoxId> = free.iter().map(|s| s.id.clone()).collect();
    let alone = create_all(free);
    assert_eq!(alone.len(), 32, "32 creates at once, none held");
    for id in &ids {
        node.delete(id).unwrap();
    }

    let fifo = state.path().join(".held.tmp");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let held = BoxId::new("held").unwrap();
    let held_spec = BoxSpec {
        id: held.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Oci(Box::new(OciSpec {
            rootfs: Rootfs::Dir(lab.rootfs.clone()),
            init: hex_api::Program::of(&["/hex", "selftest-box", "wait"]),
            mounts: Vec::new(),
            hostname: None,
            network: Network::Host,
            hosts: Vec::new(),
        })),
        owner: None,
    };
    let busy = specs("busy", 31);
    let ids: Vec<BoxId> = busy.iter().map(|s| s.id.clone()).collect();
    let synced = node.record_syncs();
    // Nothing here panics before the held write is let go, so a create that waited on it ends too and
    // the scope's joins return.
    let (beside, synced_beside, held_made) = std::thread::scope(|s| {
        let holder = s.spawn(|| node.create(held_spec));
        wait_for(
            blocked_opening_a_fifo,
            "the held create to block in its record write",
        );
        let (done, made) = std::sync::mpsc::channel();
        for spec in busy {
            let (node, done) = (&node, done.clone());
            s.spawn(move || {
                let t = Instant::now();
                let made = node.create(spec).map(|_| t.elapsed());
                let _ = done.send(made);
            });
        }
        let beside: Vec<Result<Duration, Refusal>> = (0..31)
            .map_while(|_| made.recv_timeout(conformance::PATIENCE).ok())
            .collect();
        let synced_beside = node.record_syncs();
        // Let the held write go on: its open completes, and it writes the record into the FIFO.
        let mut held_bytes = Vec::new();
        let read = std::fs::File::open(&fifo).and_then(|mut f| f.read_to_end(&mut held_bytes));
        (beside, synced_beside, (read, holder.join().unwrap()))
    });
    let (read, held_made) = held_made;
    read.unwrap();
    std::fs::remove_file(&fifo).unwrap();
    assert_eq!(
        beside.len(),
        31,
        "every create beside the held write was made while it was held"
    );
    let beside: Vec<Duration> = beside.into_iter().map(|m| m.expect("create")).collect();
    assert!(
        matches!(
            &held_made,
            Err(Refusal::Os {
                op: "flushing a record",
                ..
            })
        ),
        "the held write, let go, failed at its fsync: {held_made:?}"
    );
    if on_tmpfs(node.container_dir(&held).parent().unwrap()) {
        assert_eq!(
            synced_beside, synced,
            "the 31 boxes' records, their container state on tmpfs alone, were not synced"
        );
    } else {
        eprintln!(
            "held_record_write: the node's container state isn't on tmpfs here, so its boxes' records are synced too"
        );
    }
    let ms = |d: &Duration| d.as_secs_f64() * 1e3;
    let (alone, beside) = (sorted_ms(&alone), sorted_ms(&beside));
    eprintln!(
        "held_record_write: create p50 {:.3} ms, max {:.3} ms with 32 at once; p50 {:.3} ms, max {:.3} ms with 31 beside a held durable write",
        ms(&alone[alone.len() / 2]),
        ms(&alone[alone.len() - 1]),
        ms(&beside[beside.len() / 2]),
        ms(&beside[beside.len() - 1]),
    );
    for id in &ids {
        node.delete(id).unwrap();
    }
    assert_eq!(node.inspect(&held), Err(Refusal::NoSuchBox(held.clone())));
    assert!(node.list().unwrap().is_empty(), "a box is left");
}

/// Goal 5 plan step 2: two nodes on one host, each with a state directory of its own, keep their
/// containers' state in the one root under the runtime directory; a box of the same id in each gets a
/// container of its own, named for its node. Both are made and start, each its own init; deleting one
/// leaves the other running, its state where it was.
fn two_nodes_one_box_id(lab: &Lab, own: &Path) {
    let (state_a, state_b) = (
        tempfile::tempdir_in(lab.bundles.path()).unwrap(),
        tempfile::tempdir_in(lab.bundles.path()).unwrap(),
    );
    let a = lab.node(&own.join("hex-oci-two-a"), state_a.path());
    let b = lab.node(&own.join("hex-oci-two-b"), state_b.path());
    let id = BoxId::new("same").unwrap();
    let wait = Lab::act(&Act::Wait);
    a.create(lab.running(id.clone(), &wait))
        .expect("create in a");
    b.create(lab.running(id.clone(), &wait))
        .expect("create in b");
    let (in_a, in_b) = (a.container_dir(&id), b.container_dir(&id));
    assert_ne!(in_a.file_name(), in_b.file_name(), "two containers' names");
    if in_a.parent() != in_b.parent() {
        eprintln!(
            "two_nodes_one_box_id: no runtime directory here, so each node keeps its containers in its state directory"
        );
    }
    for node in [&a, &b] {
        node.start(&id).expect("start");
        assert_eq!(node.inspect(&id).unwrap().state, BoxState::Running);
    }
    assert_ne!(init_pid(&in_a), init_pid(&in_b), "two inits");
    a.delete(&id).expect("delete in a");
    assert!(!in_a.exists(), "a's container state is left");
    assert_eq!(
        b.inspect(&id).unwrap().state,
        BoxState::Running,
        "b's box runs on"
    );
    assert!(
        in_b.join("state.json").exists(),
        "b's container state is kept"
    );
    b.delete(&id).expect("delete in b");
    assert!(!in_b.exists(), "b's container state is left");
}

/// T-T7.1.2 (goal 5 extended step 5): a node restarted over records in both its places. Do: make two
/// boxes whose directories all lie on tmpfs (from bundles, their container state under the runtime
/// directory) and one from an `OciSpec` of layers (its rendered bundle's writable layer on disk, in the
/// state directory); end one
/// tmpfs box's init; then open the node again on the same directories and recover. Expect: each record
/// where it belongs, before the restart and after it (the tmpfs boxes' among the runtime directory's
/// records, unsynced; the disk box's in the state directory, fsynced, file and directory); the live
/// boxes taken back and the one whose init ended removed; and, once the two are deleted (the disk
/// box's record synced again as it's marked going), nothing of any box left in either place.
fn records_in_both_places_are_found_after_a_restart(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let box_root = own.join("hex-oci-places");
    let id = |n: &str| BoxId::new(n).unwrap();
    let (on_tmpfs_live, on_tmpfs_ended, on_disk) =
        (id("on-tmpfs-live"), id("on-tmpfs-ended"), id("on-disk"));
    let node = lab.node(&box_root, state.path());
    let runtime = node.container_dir(&on_disk).parent().unwrap().to_owned();
    if !on_tmpfs(&runtime) {
        eprintln!(
            "SKIP records_in_both_places_are_found_after_a_restart: the runtime directory {} isn't tmpfs here",
            runtime.display()
        );
        return;
    }
    let records = records_dir(&runtime, state.path()).unwrap();
    // The records in a place, by file name: in it, or in its shards for the runtime record directory's
    // (goal 5 extended, step 6).
    let kept = |dir: &Path| -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".json") {
                out.insert(name);
            } else if entry.file_type().unwrap().is_dir() && name.len() == 2 {
                for inner in std::fs::read_dir(entry.path()).unwrap() {
                    let inner = inner.unwrap().file_name().to_string_lossy().into_owned();
                    if inner.ends_with(".json") {
                        out.insert(inner);
                    }
                }
            }
        }
        out
    };
    let names = |ids: &[&BoxId]| -> BTreeSet<String> {
        ids.iter().map(|id| format!("{id}.json")).collect()
    };
    let wait = Lab::act(&Act::Wait);
    for id in [&on_tmpfs_live, &on_tmpfs_ended] {
        node.create(lab.running(id.clone(), &wait))
            .expect("create a box on tmpfs");
    }
    assert_eq!(
        node.record_syncs(),
        hex_node::store::Syncs::default(),
        "the tmpfs boxes' records aren't synced"
    );
    node.create(BoxSpec {
        id: on_disk.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Oci(Box::new(OciSpec {
            rootfs: Rootfs::Layers(vec![lab.rootfs.clone()]),
            init: hex_api::Program::of(&["/hex", "selftest-box", "wait"]),
            mounts: Vec::new(),
            hostname: None,
            network: Network::Host,
            hosts: Vec::new(),
        })),
        owner: None,
    })
    .expect("create a box with its bundle's writable layer on disk");
    assert_eq!(
        node.record_syncs(),
        hex_node::store::Syncs { files: 1, dirs: 1 },
        "the disk box's record is synced, file and directory"
    );
    let placed = || (kept(&records), kept(state.path()));
    assert_eq!(
        placed(),
        (
            names(&[&on_tmpfs_live, &on_tmpfs_ended]),
            names(&[&on_disk])
        ),
        "each record in its place"
    );
    node.signal(&on_tmpfs_ended, hex_api::Signal::KILL).unwrap();
    wait_exit(&node, &on_tmpfs_ended);
    drop(node);

    let node = lab.node(&box_root, state.path());
    let summary = node.recover().expect("recover");
    let adopted: BTreeSet<BoxId> = summary.adopted.iter().cloned().collect();
    let removed: BTreeSet<BoxId> = summary.removed.iter().map(|(id, _)| id.clone()).collect();
    assert_eq!(
        (adopted, removed),
        (
            [on_tmpfs_live.clone(), on_disk.clone()]
                .into_iter()
                .collect(),
            [on_tmpfs_ended.clone()].into_iter().collect()
        ),
        "{summary:?}"
    );
    assert_eq!(
        placed(),
        (names(&[&on_tmpfs_live]), names(&[&on_disk])),
        "each box taken back found where it belongs, the removed one in neither place"
    );
    for id in [&on_tmpfs_live, &on_disk] {
        assert_eq!(node.inspect(id).unwrap().state, BoxState::Created, "{id}");
        node.delete(id).unwrap();
    }
    assert_eq!(
        node.record_syncs(),
        hex_node::store::Syncs { files: 1, dirs: 1 },
        "the disk box's record, marked going, is synced again"
    );
    assert_eq!(
        placed(),
        (BTreeSet::new(), BTreeSet::new()),
        "a record is left"
    );
    for id in [&on_tmpfs_live, &on_tmpfs_ended, &on_disk] {
        assert!(
            !box_root.join(OCI_BOXES).join(id.as_str()).exists(),
            "{id}'s cgroup is left"
        );
        assert!(
            !node.container_dir(id).exists(),
            "{id}'s container state is left"
        );
    }
    assert!(
        !state.path().join("bundles").join(on_disk.as_str()).exists(),
        "the disk box's bundle is left"
    );
}

/// T-T7.1.2 across goal 5 plan step 2's upgrade: a node before defda14 kept a box's container state in
/// its state directory, `<state>/oci/<id>`, and the box's record names that directory. A box made so,
/// its init waiting, is taken back by a node of today restarted on the same state directory, which reads
/// its state where the record names it (created, not empty with an exit no one saw) and deletes it,
/// leaving nothing of it.
fn a_box_made_before_the_move_is_adopted(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let box_root = own.join("hex-oci-legacy");
    let id = BoxId::new("legacy").unwrap();
    let bundle = lab.bundle(&Lab::act(&Act::Wait));
    // The box as the node before defda14 made it: its cgroup under the box root with its bounds, its
    // container under `<state>/oci` named by its id, and its record naming that directory.
    let root = BoxRoot::open(&box_root, disks::under(state.path()).unwrap()).unwrap();
    let cgroup = root.create(&id, &Bounds::default()).unwrap();
    let _cleanup = Killing(&root, id.clone());
    let before = state.path().join("oci");
    let oci = OciRuntime::with_helper(before.clone(), PathBuf::from(&lab.exe));
    oci.create_in_cgroup(id.as_str(), &bundle, None, cgroup, Held::default())
        .unwrap();
    let made = oci.state(id.as_str()).unwrap();
    let mut record = BoxRecord::new(
        BoxSpec {
            id: id.clone(),
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(bundle),
            owner: None,
        },
        Phase::Ready,
        vec![before.join(id.as_str())],
    );
    record.init = Some(ProcId {
        pid: u32::try_from(made.pid).unwrap(),
        start_ticks: made.start_time.unwrap(),
    });
    as_an_older_node_wrote(state.path(), &record);

    let node = lab.node(&box_root, state.path());
    assert_ne!(
        node.container_dir(&id),
        before.join(id.as_str()),
        "today's node keeps its containers elsewhere"
    );
    let summary = node.recover().expect("recover");
    assert!(summary.adopted.contains(&id), "not adopted: {summary:?}");
    let info = node.inspect(&id).unwrap();
    assert_eq!(
        (info.state, info.exit),
        (BoxState::Created, None),
        "its state, read where its record names it"
    );
    node.delete(&id).expect("delete");
    assert!(
        !before.join(id.as_str()).exists(),
        "its container state is left"
    );
    assert!(!box_root.join(id.as_str()).exists(), "its cgroup is left");
    assert_eq!(node.inspect(&id), Err(Refusal::NoSuchBox(id.clone())));
}

/// T-T7.1.2 across goal 5 extended step 1's move of the `oci` boxes under `<box root>/.oci`: a node upgraded
/// over boxes its older self made at `<box root>/<id>`, beside boxes of its own at `<box root>/.oci/<id>`, finds
/// both places in its recovery. Do: with today's node, make a box whose init waits (`.oci/new-live`), and leave a
/// cgroup there with no record (`.oci/new-orphan`); as the node before the move made boxes, its cgroup under the
/// box root itself, make a box whose init waits (`old-live`, its record ready), a cgroup there with no record
/// (`old-orphan`) and one whose record says it was being made (`old-mid`); then open today's node again and
/// recover. Expect: the two live boxes taken back, each where it was; the other three removed, their cgroups
/// gone; and, the two deleted, nothing of any box left at either place.
fn a_node_upgraded_over_boxes_at_the_old_place_recovers_both_places(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let box_root = own.join("hex-oci-upgrade");
    let oci_dir = box_root.join(OCI_BOXES);
    let id = |n: &str| BoxId::new(n).unwrap();
    let (old_live, old_orphan, old_mid) = (id("old-live"), id("old-orphan"), id("old-mid"));
    let (new_live, new_orphan) = (id("new-live"), id("new-orphan"));

    // Today's node first: a box of its own, and where it keeps a box's container.
    let first = lab.node(&box_root, state.path());
    first
        .create(lab.running(new_live.clone(), &Lab::act(&Act::Wait)))
        .expect("create at the new place");
    let container = first.container_dir(&old_live);
    drop(first);
    let oci_boxes = BoxRoot::open(&oci_dir, disks::under(state.path()).unwrap()).unwrap();
    let _new = Killing(&oci_boxes, new_live.clone());
    oci_boxes.create(&new_orphan, &Bounds::default()).unwrap();

    // What the node before the move left, made as it made it: each cgroup under the box root itself.
    let root = BoxRoot::open(&box_root, disks::under(state.path()).unwrap()).unwrap();
    let _old = Killing(&root, old_live.clone());
    let bundle = lab.bundle(&Lab::act(&Act::Wait));
    let runtime = OciRuntime::with_helper(
        container.parent().unwrap().to_owned(),
        PathBuf::from(&lab.exe),
    );
    let name = container.file_name().unwrap().to_str().unwrap();
    let cgroup = root.create(&old_live, &Bounds::default()).unwrap();
    runtime
        .create_in_cgroup(name, &bundle, None, cgroup, Held::default())
        .unwrap();
    let made = runtime.state(name).unwrap();
    let spec = |id: &BoxId| BoxSpec {
        id: id.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Bundle(bundle.clone()),
        owner: None,
    };
    let mut live = BoxRecord::new(spec(&old_live), Phase::Ready, vec![container.clone()]);
    live.init = Some(ProcId {
        pid: u32::try_from(made.pid).unwrap(),
        start_ticks: made.start_time.unwrap(),
    });
    as_an_older_node_wrote(state.path(), &live);
    root.create(&old_orphan, &Bounds::default()).unwrap();
    root.create(&old_mid, &Bounds::default()).unwrap();
    as_an_older_node_wrote(
        state.path(),
        &BoxRecord::new(spec(&old_mid), Phase::Creating, Vec::new()),
    );

    let node = lab.node(&box_root, state.path());
    let summary = node.recover().expect("recover");
    let adopted: BTreeSet<BoxId> = summary.adopted.iter().cloned().collect();
    let removed: BTreeSet<BoxId> = summary.removed.iter().map(|(id, _)| id.clone()).collect();
    assert_eq!(
        (adopted, removed),
        (
            [old_live.clone(), new_live.clone()].into_iter().collect(),
            [old_orphan.clone(), old_mid.clone(), new_orphan.clone()]
                .into_iter()
                .collect()
        ),
        "{summary:?}"
    );
    for (gone, place) in [
        (&old_orphan, &box_root),
        (&old_mid, &box_root),
        (&new_orphan, &oci_dir),
    ] {
        assert!(
            !place.join(gone.as_str()).exists(),
            "{gone}'s cgroup is left"
        );
    }
    assert!(
        box_root.join(old_live.as_str()).exists() && oci_dir.join(new_live.as_str()).exists(),
        "each box taken back where it was"
    );
    for id in [&old_live, &new_live] {
        assert_eq!(node.inspect(id).unwrap().state, BoxState::Created, "{id}");
        node.delete(id)
            .unwrap_or_else(|e| panic!("delete {id}: {e}"));
    }
    for (gone, place) in [(&old_live, &box_root), (&new_live, &oci_dir)] {
        assert!(
            !place.join(gone.as_str()).exists(),
            "{gone}'s cgroup is left"
        );
    }
    assert!(node.list().unwrap().is_empty(), "a box is left");
    eprintln!(
        "oci_box upgrade: adopted {old_live} at the old place and {new_live} at .oci; removed {old_orphan}, \
         {old_mid} and {new_orphan}; nothing left after the deletes"
    );
}

/// Goal 5 plan step 4 (T-T7.1.4), the lab test as root and rootless: a container made through a seed has
/// its init in its own cgroup and the namespaces its spec asks for, and no `hex` binary was exec'd for
/// the create. Do: create and start a box whose init is a non-hex program (`/usr/bin/sleep`) with a
/// network namespace of its own; read the init's `/proc/<pid>/cgroup`, `ns/*` and `exe`. Expect: the
/// init is in the box's cgroup, in fresh pid, mount, ipc, uts and network namespaces (and a user
/// namespace rootless), its `exe` is the program (not the `hex` binary), the create was handed a seed and
/// done with it and none went through the shim (the pool's counts: the `hex __oci-init` re-exec the seed
/// replaces didn't run), and its streams are the null device, the seed's own.
fn created_through_a_seed_in_its_cgroup_and_namespaces(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let box_root = own.join("hex-oci-seedns");
    let node = lab.node(&box_root, state.path());
    let id = BoxId::new("nsbox").unwrap();
    // A non-hex init, with a network namespace of its own, so the exe is plainly the program and every
    // namespace the spec asks for can be read.
    let bundle = lab.bundle_with(&strings(&["/usr/bin/sleep", "600"]), true);
    let before = node.create_seeds().unwrap();
    node.create(BoxSpec {
        id: id.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Bundle(bundle),
        owner: None,
    })
    .expect("create through a seed");
    let after = node.create_seeds().unwrap();
    assert_eq!(
        (
            after.handed - before.handed,
            after.done - before.done,
            node.creates_through_shim()
        ),
        (1, 1, 0),
        "the create was handed a seed and done with it, and none went through the shim: {after:?}"
    );
    node.start(&id).expect("start");
    let init = u32::try_from(init_pid(&node.container_dir(&id))).unwrap();
    // Wait until the init has exec'd its program, so `exe`, the namespaces and the cgroup are the
    // container's and not the seed's clone mid-setup.
    wait_for(
        || std::fs::read_link(format!("/proc/{init}/exe")).is_ok_and(|p| p.ends_with("sleep")),
        "the init to exec its program",
    );

    let exe = std::fs::read_link(format!("/proc/{init}/exe")).unwrap();
    assert!(
        exe.ends_with("sleep") && !exe.to_string_lossy().contains("hex"),
        "the init is the program, not a hex shim: {}",
        exe.display()
    );
    // Its cgroup is the box's own (read in this process's cgroup namespace: the full path).
    let cgroup = std::fs::read_to_string(format!("/proc/{init}/cgroup")).unwrap();
    assert!(
        cgroup
            .lines()
            .any(|l| l.starts_with("0::") && l.ends_with("/nsbox")),
        "the init is in the box's cgroup: {cgroup:?}"
    );
    // Each namespace its spec makes is its own (differs from the agent's); a rootless box also has a
    // user namespace of its own.
    let agent = std::process::id();
    let mut kinds = vec!["pid", "mnt", "ipc", "uts", "net"];
    if lab.rootless {
        kinds.push("user");
    }
    for ns in kinds {
        let theirs = std::fs::read_link(format!("/proc/{init}/ns/{ns}")).unwrap();
        let ours = std::fs::read_link(format!("/proc/{agent}/ns/{ns}")).unwrap();
        assert_ne!(theirs, ours, "the init has its own {ns} namespace");
    }
    // It leads a session and a process group of its own (its first act), so nothing it or its program
    // does reaches the agent's: both ids are its own pid, kept across its exec.
    assert_eq!(
        (stat_field(init, 5), stat_field(init, 6)),
        (Some(init), Some(init)),
        "the init leads its own process group and session"
    );
    // The seed cloned the init as the agent's own child (`CLONE_PARENT`), and the init's streams are the
    // seed's: the null device, never the agent's.
    assert_eq!(ppid(init), Some(agent), "the init is the agent's own child");
    assert_eq!(
        streams(init),
        ["/dev/null"; 3].map(PathBuf::from),
        "the init has the null device for its streams"
    );
    node.delete(&id).expect("delete");
}

/// Goal 5 plan step 4 (T-T7.1.4), the re-initialization test: two containers cloned from one seed are
/// each randomized afresh, so neither carries the seed's layout (f15, f17: the child execs the
/// container's program, so its address space is randomized anew, not the seed's copied one). Do: create
/// two boxes in turn from the same idle seed and read each init's stack base, and the seed's, from
/// `/proc/<pid>/maps`. Expect: the two inits' stack bases differ, and each differs from the seed's — the
/// value each child "makes" (its ASLR'd layout) is its own.
fn each_seed_cloned_container_is_randomized_afresh(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-reinit"), state.path());
    let agent = std::process::id();
    let base = |pid: u32| {
        wait_for(
            || stack_base(pid).is_some(),
            "a process to have a readable stack mapping",
        );
        stack_base(pid).unwrap()
    };
    let make = |id: &str| -> u32 {
        let bid = BoxId::new(id).unwrap();
        node.create(lab.running(bid.clone(), &strings(&["/usr/bin/sleep", "600"])))
            .expect("create");
        node.start(&bid).expect("start");
        let init = u32::try_from(init_pid(&node.container_dir(&bid))).unwrap();
        wait_for(
            || std::fs::read_link(format!("/proc/{init}/exe")).is_ok_and(|p| p.ends_with("sleep")),
            "the init to exec its program",
        );
        init
    };

    let one = make("reinit1");
    // The seed that cloned it, now idle: the agent's child running `hex __oci-seed`. The next create
    // reuses it, so both inits are cloned from this one seed.
    let seed = children_of(agent)
        .into_iter()
        .find(|&c| cmdline_has(c, "__oci-seed"))
        .expect("a create seed is up");
    let (v1, vs) = (base(one), base(seed));
    let two = make("reinit2");
    let v2 = base(two);
    assert_ne!(v1, v2, "two containers from one seed differ: {v1} vs {v2}");
    assert_ne!(v1, vs, "the first differs from the seed's: {v1} vs {vs}");
    assert_ne!(v2, vs, "the second differs from the seed's: {v2} vs {vs}");
    node.delete(&BoxId::new("reinit1").unwrap())
        .expect("delete");
    node.delete(&BoxId::new("reinit2").unwrap())
        .expect("delete");
}

/// Goal 5 plan step 4 (T-T7.1.4), crash injection: a seed that dies mid-create refuses that create by
/// type, its box is cleaned up, and the seed is restarted, counted, so the next create succeeds. Do:
/// create and delete a box, so the pool holds two idle seeds, its first (the one the next create is
/// handed: the core hands the lowest idle slot) and the spare it started beside it; stop the first; create
/// a second box on another thread and, once the pool has handed it the stopped seed, kill that seed; then
/// create a third. Expect: the second create is refused by type and left no box; the pool counts the
/// death and the create its death refused; the third create succeeds and the pool counts a restart.
fn a_seed_that_dies_mid_create_is_refused_and_restarted(lab: &Lab, own: &Path) {
    let state = tempfile::tempdir_in(lab.bundles.path()).unwrap();
    let node = lab.node(&own.join("hex-oci-seedcrash"), state.path());
    let agent = std::process::id();
    let first = BoxId::new("crash-a").unwrap();
    node.create(lab.running(first.clone(), &Lab::act(&Act::Wait)))
        .expect("the first create, through the pool's first seed");
    node.delete(&first)
        .expect("delete the first box, leaving its seed idle");
    // The pool's first seed, the one started earliest: it served the first create, and is in the lowest
    // slot, so the next create is handed it.
    let seed = children_of(agent)
        .into_iter()
        .filter(|&c| cmdline_has(c, "__oci-seed"))
        .min_by_key(|&c| start_ticks(c))
        .expect("a create seed is up");
    let seed = i32::try_from(seed).unwrap();
    kill(seed, 19); // SIGSTOP: it holds whatever create it is handed, unanswered
    let before = node.create_seeds().unwrap();

    let dead = BoxId::new("crash-b").unwrap();
    let refused = std::thread::scope(|scope| {
        let creating =
            scope.spawn(|| node.create(lab.running(dead.clone(), &Lab::act(&Act::Wait))));
        wait_for(
            || node.create_seeds().unwrap().handed > before.handed,
            "the second create to be handed the stopped seed",
        );
        // Mid-create: the seed holds the create, which waits on its answer. It dies.
        kill(seed, 9);
        creating.join().unwrap()
    });
    assert!(
        matches!(refused, Err(Refusal::Os { .. }) | Err(Refusal::Broken(_))),
        "a create whose seed dies is refused by type: {refused:?}"
    );
    assert_eq!(
        node.inspect(&dead),
        Err(Refusal::NoSuchBox(dead.clone())),
        "its box is cleaned up"
    );
    assert!(
        !own.join("hex-oci-seedcrash")
            .join(OCI_BOXES)
            .join("crash-b")
            .exists()
    );
    let counts = node.create_seeds().unwrap();
    assert_eq!(
        (
            counts.died - before.died,
            counts.refused_died - before.refused_died
        ),
        (1, 1),
        "the seed's death and the create it refused are counted: {counts:?}"
    );

    // The next create is handed a live seed, and the dead one's slot is started again, counted.
    let next = BoxId::new("crash-c").unwrap();
    node.create(lab.running(next.clone(), &Lab::act(&Act::Wait)))
        .expect("the next create succeeds");
    node.start(&next).expect("start the next box");
    assert_eq!(node.inspect(&next).unwrap().state, BoxState::Running);
    wait_for(
        || node.create_seeds().unwrap().restarted > before.restarted,
        "the dead seed's slot to be started again",
    );
    node.delete(&next).expect("delete");
}

/// The start address of `pid`'s main-thread stack, from `/proc/<pid>/maps` (`[stack]`), as a string;
/// `None` while it can't be read. Randomized afresh at each `execve`, so it tells one container's
/// address space from another's and from the seed's.
fn stack_base(pid: u32) -> Option<String> {
    let maps = std::fs::read_to_string(format!("/proc/{pid}/maps")).ok()?;
    maps.lines()
        .find(|l| l.ends_with("[stack]"))
        .and_then(|l| l.split('-').next())
        .map(str::to_owned)
}

/// What `pid`'s standard streams are open on: the targets of `/proc/<pid>/fd/0`, `1` and `2`.
fn streams<P: std::fmt::Display>(pid: P) -> [PathBuf; 3] {
    [0, 1, 2].map(|fd| std::fs::read_link(format!("/proc/{pid}/fd/{fd}")).unwrap())
}

/// When `pid` started, in clock ticks after boot (`/proc/<pid>/stat` field 22).
fn start_ticks(pid: u32) -> u64 {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let (_, rest) = stat.rsplit_once(')').unwrap();
    rest.split_whitespace().nth(19).unwrap().parse().unwrap()
}

/// Kills whatever is left in a box's cgroup when dropped, so a check that fails leaves no container
/// running, holding this test's output open.
struct Killing<'a>(&'a BoxRoot, BoxId);

impl Drop for Killing<'_> {
    fn drop(&mut self) {
        if let Some(cg) = self.0.get(&self.1) {
            let _ = cg.kill();
        }
    }
}

fn sorted_ms(latencies: &[Duration]) -> Vec<Duration> {
    let mut sorted = latencies.to_vec();
    sorted.sort_unstable();
    sorted
}

/// Writes `record` where a node before goal 5 extended step 5 kept every record: its state directory.
fn as_an_older_node_wrote(state: &Path, record: &BoxRecord) {
    std::fs::write(
        state.join(format!("{}.json", record.spec.id)),
        record.encode().unwrap(),
    )
    .unwrap();
}

/// Whether a thread of this process is blocked opening a FIFO no one has opened the other end of: the
/// kernel shows it waiting in `wait_for_partner` (fs/pipe.c), or in `fifo_open` where that's inlined.
fn blocked_opening_a_fifo() -> bool {
    std::fs::read_dir("/proc/self/task")
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|task| std::fs::read_to_string(task.path().join("wchan")).ok())
        .any(|wchan| matches!(wchan.trim(), "wait_for_partner" | "fifo_open"))
}

/// Whether `dir` lies on tmpfs (include/uapi/linux/magic.h, `TMPFS_MAGIC`).
fn on_tmpfs(dir: &Path) -> bool {
    rustix::fs::statfs(dir).is_ok_and(|fs| i128::from(fs.f_type) == 0x0102_1994)
}

/// Polls inspect until the box reports an exit: the fact the test needs, bounded so a runtime that
/// never reports one fails rather than hangs.
fn wait_exit(rt: &dyn Runtime, id: &BoxId) -> BoxInfo {
    let mut last = None;
    wait_for(
        || {
            let info = rt.inspect(id).unwrap();
            let done = info.exit.is_some();
            last = Some(info);
            done
        },
        "the box to report its init's exit",
    );
    last.unwrap()
}

fn wait_for(mut cond: impl FnMut() -> bool, what: &str) {
    let deadline = Instant::now() + conformance::PATIENCE;
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

/// The init's pid, from the state (`"pid":<n>`) in its container's state dir.
fn init_pid(container: &Path) -> i32 {
    let text = std::fs::read_to_string(container.join("state.json")).unwrap();
    let (_, after) = text.split_once("\"pid\":").unwrap();
    after
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| (*a).to_owned()).collect()
}

/// Field 4 of `/proc/<pid>/stat`; 0 once it's gone.
fn parent_of(pid: i32) -> i32 {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| {
            s.rsplit_once(')')
                .and_then(|(_, rest)| rest.split_whitespace().nth(1).map(str::to_owned))
        })
        .and_then(|p| p.parse().ok())
        .unwrap_or(0)
}

fn kill(pid: i32, sig: i32) {
    let status = Command::new("kill")
        .args([format!("-{sig}"), pid.to_string()])
        .status()
        .unwrap();
    assert!(status.success(), "kill -{sig} {pid}");
}

/// `args` as a JSON array: Debug quoting is JSON's for the plain ASCII the tests use.
fn json_args(args: &[String]) -> String {
    assert!(args.iter().all(|a| a.is_ascii() && !a.contains('\\')));
    format!("{args:?}")
}

/// A rootfs of the host's `/usr`, bound in by each bundle, with the host's top-level links into it
/// (`/bin`, `/lib64`, …) so the loader and `/bin/sh` resolve as on the host.
fn build_rootfs(rootfs: &Path) {
    for d in ["proc", "dev", "sys", "tmp", "usr"] {
        std::fs::create_dir_all(rootfs.join(d)).unwrap();
    }
    std::fs::File::create(rootfs.join("hex")).unwrap();
    for link in ["bin", "sbin", "lib", "lib64"] {
        if let Ok(target) = std::fs::read_link(Path::new("/").join(link)) {
            let target = target.to_string_lossy().trim_start_matches('/').to_owned();
            std::os::unix::fs::symlink(target, rootfs.join(link)).unwrap();
        }
    }
}

fn host_id(flag: &str) -> u32 {
    let out = Command::new("id").arg(flag).output().expect("id");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("id is a number")
}

/// This process's cgroup directory under /sys/fs/cgroup.
fn own_cgroup() -> PathBuf {
    let own = std::fs::read_to_string("/proc/self/cgroup").expect("/proc/self/cgroup");
    let rel = own
        .lines()
        .find_map(|l| l.strip_prefix("0::"))
        .expect("a 0:: cgroup line")
        .trim_start_matches('/');
    PathBuf::from("/sys/fs/cgroup").join(rel)
}

/// Moves this process into a leaf of its own cgroup and confirms every controller is delegated, as
/// oci_seccomp.rs does; returns the own cgroup dir, or why it isn't ours to subdivide.
fn prepare_cgroup(disk: &Path) -> Result<PathBuf, String> {
    let own = LAB_PARENT.get_or_init(own_cgroup).clone();
    let leaf = own.join("hex-agent");
    std::fs::create_dir_all(&leaf).map_err(|e| format!("making a leaf for the agent: {e}"))?;
    std::fs::write(leaf.join("cgroup.procs"), format!("{}", std::process::id()))
        .map_err(|e| format!("moving self into a leaf: {e}"))?;
    let probe = hex_runtime::linux::cgroup::BoxRoot::open(
        &own.join("hex-probe"),
        hex_runtime::linux::disks::under(disk).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;
    let missing: Vec<&str> = hex_core::cgroup::CONTROLLERS
        .into_iter()
        .filter(|c| !probe.controllers().contains(*c))
        .collect();
    let _ = std::fs::remove_dir(own.join("hex-probe"));
    if missing.is_empty() {
        Ok(own)
    } else {
        Err(format!(
            "{} can't hand down the {} controllers",
            own.display(),
            missing.join(", ")
        ))
    }
}

fn cache_dir() -> PathBuf {
    let cache = PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".cache/hex-t7/ocibox");
    std::fs::create_dir_all(&cache).expect("cache dir");
    cache
}
