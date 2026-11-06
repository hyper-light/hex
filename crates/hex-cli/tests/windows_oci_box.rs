//! The Windows `oci` class against the runtime contract (AC-T7.1.1, Hex HEX-RT-1..13) and the native
//! plan step 4's own criteria: a device not granted isn't visible in a box, a read-only mount refuses
//! writes, and an image the host doesn't run is refused by name before HCS is asked. The boxes run a
//! Windows image process-isolated through HCS; their programs are `hex selftest-box` acts, which every box
//! sees at `C:\hex` (the launcher's folder), so the real box is held to the same contract as the
//! simulator and the other classes.
//!
//! Runs on Windows with the Containers feature on and an image's layer folders named in
//! `HEX_WINDOWS_LAYERS` (topmost first, `;`-separated: a matching nanoserver or servercore image's,
//! e.g. containerd's windows snapshotter folders), and a mismatched image's in
//! `HEX_WINDOWS_MISMATCH_LAYERS` (both written by bench/native/windows/oci_setup.ps1). Without the first
//! it prints SKIP and checks nothing; the step's recorded run (jupiter) sets them, and a SKIP there fails it.
#![cfg(target_os = "windows")]
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

use hex_api::conformance::{self, Act, Fixture};
use hex_api::differential;
use hex_api::{Bounds, BoxId, BoxSpec, ClassSpec, Exit, Program, Runtime, Stdio};
use hex_runtime::windows::WinRuntime;
use serde_json::json;

/// The USB device interface class (`GUID_DEVINTERFACE_USB_DEVICE`, usbiodef.h): every host has one, and on
/// jupiter a box opens none of them ungranted and all of them granted (the lead's ruling, 2026-11-06). It
/// isn't in Microsoft's table of supported classes (hardware-devices-in-containers.md:45-79), whose four
/// jupiter has none of, and the display class it lists is refused at create by HCS for Docker and Hex alike.
const USB_DEVICE: &str = "A5DCBF10-6530-11D2-901F-00C04FB951ED";

struct Lab {
    /// The folder holding `hex.exe`, mapped into every box as its launcher.
    bin: PathBuf,
    /// The image's read-only layer folders, topmost first.
    layers: Vec<String>,
    /// Bundles and scratch folders, one per box.
    root: tempfile::TempDir,
}

impl Lab {
    fn new() -> Option<Self> {
        let layers: Vec<String> = std::env::var("HEX_WINDOWS_LAYERS")
            .ok()?
            .split(';')
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect();
        if layers.is_empty() {
            return None;
        }
        let exe = PathBuf::from(env!("CARGO_BIN_EXE_hex"));
        Some(Self {
            bin: exe.parent().unwrap().to_path_buf(),
            layers,
            root: tempfile::tempdir().unwrap(),
        })
    }

    fn runtime(&self) -> WinRuntime {
        WinRuntime::new().with_launcher(self.bin.clone())
    }

    /// `hex selftest-box <act>` as a box sees it.
    fn args(act: &Act) -> Vec<String> {
        let mut args = vec![r"C:\hex\hex.exe".to_owned(), "selftest-box".to_owned()];
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

    /// A bundle for box `id` whose init runs `args`, with `mounts` and the spec's `windows` extras.
    fn bundle(&self, id: &BoxId, args: &[String], mounts: serde_json::Value) -> PathBuf {
        let dir = self.root.path().join(format!("bundle-{id}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let scratch = self.root.path().join(format!("scratch-{id}"));
        let mut folders = self.layers.clone();
        folders.push(scratch.to_str().unwrap().to_owned());
        let config = json!({
            "ociVersion": "1.2.0",
            "process": {"args": args, "cwd": "C:\\"},
            "mounts": mounts,
            "windows": {"layerFolders": folders},
        });
        std::fs::write(dir.join("config.json"), config.to_string()).unwrap();
        dir
    }

    fn spec_with(
        &self,
        id: BoxId,
        bounds: Bounds,
        act: &Act,
        mounts: serde_json::Value,
    ) -> BoxSpec {
        let dir = self.bundle(&id, &Self::args(act), mounts);
        BoxSpec {
            id,
            bounds,
            class: ClassSpec::Bundle(dir),
        }
    }
}

impl Fixture for Lab {
    fn spec(&self, id: BoxId, bounds: Bounds) -> BoxSpec {
        self.spec_with(id, bounds, &Act::Wait, json!([]))
    }

    fn init(&self, id: BoxId, act: &Act) -> BoxSpec {
        self.spec_with(id, Bounds::default(), act, json!([]))
    }

    fn program(&self, act: &Act) -> Program {
        let args = Self::args(act);
        Program::of(&args.iter().map(String::as_str).collect::<Vec<_>>())
    }

    /// A member's own refused fork has no §3-clean count on Windows; the bound's effect is checked
    /// (docs/research/runtime/windows-pids-refused.md; Ada's call, 2026-11-03), as for the process class.
    fn refused_forks_counted(&self) -> bool {
        false
    }

    /// HEX-RT-14: Windows' exec bound, the class's own (hex_core::hcs::exec_limit).
    fn exec_limit(&self) -> Option<hex_api::ExecLimit> {
        Some(hex_core::hcs::exec_limit())
    }

    /// TERM ends the box, init included: Windows has no POSIX signals. HCS's ProcessSignal carries only
    /// console controls (Virtualization-Documentation hcs/SchemaReference.md:796-808), hcsshim maps TERM to
    /// a shutdown and KILL to terminate (internal/signals/signal.go:185-197), Docker ends the container on
    /// either (libcontainerd/local/local_windows.go:713-750), and the OCI spec's Windows section defines no
    /// signals (runtime-spec runtime.md:131-136 only asks that kill send one). The class ends its job
    /// (docs/research/runtime/windows-hcs.md §4; HEX-DIFF-1, 2; the lead's ruling, 2026-11-05).
    fn term_ends_init(&self) -> bool {
        true
    }
}

/// Runs `hex selftest-box <args>` in started box `id` and returns its exit and stdout.
fn run(rt: &WinRuntime, lab: &Lab, id: &BoxId, args: &[&str]) -> (Exit, String) {
    let (mut r, w) = std::io::pipe().unwrap();
    let mut program = lab.program(&Act::Wait);
    program.args = [r"C:\hex\hex.exe", "selftest-box"]
        .iter()
        .chain(args)
        .map(|a| (*a).to_owned())
        .collect();
    let mut p = rt
        .exec(
            id,
            program,
            Stdio {
                stdout: Some(w),
                ..Stdio::null()
            },
        )
        .expect("exec");
    let exit = p
        .wait_for(conformance::PATIENCE)
        .expect("wait")
        .expect("the program ends");
    let mut out = String::new();
    r.read_to_string(&mut out).unwrap();
    (exit, out)
}

fn made(rt: &WinRuntime, spec: BoxSpec) -> BoxId {
    let id = spec.id.clone();
    let _ = rt.delete(&id);
    rt.create(spec).expect("create");
    rt.start(&id).expect("start");
    id
}

/// AC-T7.1.1: the Windows `oci` class holds the runtime contract, HEX-RT-1..13, and the differential
/// suite holds it to the simulator's trace for a bundle-made box (HEX-DIFF-1..6, CLAUDE.md §8).
fn contract(lab: &Lab) {
    let failures = differential::run(&lab.runtime(), lab);
    assert!(failures.is_empty(), "conformance failures: {failures:#?}");
}

/// The step's criterion: "a device not granted isn't visible", read as reachable (the lead's ruling,
/// 2026-11-06: a process-isolated silo lists the host's devices under every runtime, and a grant plumbs
/// them in). Do: on a host with USB devices, probe that class from a box granted nothing, then from one
/// granted it. Expect: ungranted, the host's devices are listed and none opens; granted, they open.
fn a_device_not_granted_isnt_visible(lab: &Lab) {
    let host = hex_runtime::windows::oci::devices(USB_DEVICE).expect("the host's devices");
    assert!(
        host.reachable >= 1,
        "the host opens no USB device, so a box's view can't be checked against it: {host:?}"
    );
    let rt = lab.runtime();
    let mut seen = Vec::new();
    for granted in [false, true] {
        let id = BoxId::new(&format!("hex-oci-devices-{granted}")).unwrap();
        let mut spec = lab.spec(id.clone(), Bounds::default());
        if granted {
            let ClassSpec::Bundle(dir) = &spec.class else {
                panic!("the lab makes bundle boxes")
            };
            let path = dir.join("config.json");
            let mut cfg: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            cfg["windows"]["devices"] = json!([{"id": USB_DEVICE, "idType": "class"}]);
            std::fs::write(&path, cfg.to_string()).unwrap();
            spec = BoxSpec {
                class: ClassSpec::Bundle(dir.clone()),
                ..spec
            };
        }
        let id = made(&rt, spec);
        let (exit, out) = run(&rt, lab, &id, &["win-devices", USB_DEVICE]);
        rt.delete(&id).expect("delete");
        assert_eq!(exit, Exit::Code(0), "the probe failed: {out:?}");
        seen.push(out.trim().to_owned());
    }
    assert_eq!(
        seen[0],
        format!("{} 0", host.listed),
        "ungranted: the host's USB devices are listed and none opens"
    );
    assert_eq!(
        seen[1],
        format!("{} {}", host.listed, host.reachable),
        "granted: the box opens the host's USB devices"
    );
}

/// The step's criterion: a read-only mount refuses writes. Do: map one host folder read-only and another
/// read-write, write a file into each from inside the box. Expect: the read-only write is refused and
/// leaves nothing on the host; the read-write one lands.
fn a_read_only_mount_refuses_writes(lab: &Lab) {
    let ro = lab.root.path().join("ro");
    let rw = lab.root.path().join("rw");
    std::fs::create_dir_all(&ro).unwrap();
    std::fs::create_dir_all(&rw).unwrap();
    let rt = lab.runtime();
    let id = made(
        &rt,
        lab.spec_with(
            BoxId::new("hex-oci-mounts").unwrap(),
            Bounds::default(),
            &Act::Wait,
            json!([
                {"destination": "C:\\ro", "source": ro.to_str().unwrap(), "options": ["ro"]},
                {"destination": "C:\\rw", "source": rw.to_str().unwrap()},
            ]),
        ),
    );
    let refused = run(&rt, lab, &id, &["write", r"C:\ro\f"]);
    let wrote = run(&rt, lab, &id, &["write", r"C:\rw\f"]);
    rt.delete(&id).expect("delete");
    assert_ne!(
        refused.0,
        Exit::Code(0),
        "a write to a read-only mount: {refused:?}"
    );
    assert!(refused.1.starts_with("refused"), "{refused:?}");
    assert!(!ro.join("f").exists(), "the refused write reached the host");
    assert_eq!(
        wrote.0,
        Exit::Code(0),
        "a write to a read-write mount: {wrote:?}"
    );
    assert!(
        rw.join("f").exists(),
        "the read-write write didn't reach the host"
    );
}

/// The step's criterion: an image whose version doesn't match the host's is refused by name, before HCS
/// is asked. Do: make a box from the layers in `HEX_WINDOWS_MISMATCH_LAYERS`, an image Microsoft's table
/// says this host doesn't run (oci_setup.ps1 pulls Server 2019's nanoserver; a client's own `ntdll.dll`
/// is no mismatch, since Windows 11 25H2's is build 26100, Server 2025's). Expect: create refused as the
/// image, naming the image's build and the host's, and no box made.
fn a_mismatched_image_is_refused_by_name(lab: &Lab) {
    let layers: Vec<String> = std::env::var("HEX_WINDOWS_MISMATCH_LAYERS")
        .expect("HEX_WINDOWS_MISMATCH_LAYERS names a mismatched image's layers (oci_setup.ps1)")
        .split(';')
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect();
    let base = layers.last().expect("a mismatched image's layers");
    let image = hex_runtime::windows::oci::sys::file_build(&format!(
        r"{base}\Files\Windows\System32\ntdll.dll"
    ))
    .unwrap();
    let host = hex_runtime::windows::oci::sys::host();
    // Both halves of the table on this host: the lab's own image is accepted (on jupiter, Windows 11 25H2
    // at 26200 runs nanoserver:ltsc2025 at 26100, the half a host-ntdll premise got wrong), the other
    // refused.
    let own_base = lab.layers.last().expect("the lab's image layers");
    let own = hex_runtime::windows::oci::sys::file_build(&format!(
        r"{own_base}\Files\Windows\System32\ntdll.dll"
    ))
    .unwrap();
    assert!(
        hex_core::hcs::image_runs(host, own).is_ok(),
        "the lab's image (build {own}) is refused on this host ({host:?})"
    );
    assert!(
        hex_core::hcs::image_runs(host, image).is_err(),
        "the mismatched image (build {image}) runs on this host ({host:?}): pick another"
    );
    let lab_mismatch = Lab {
        bin: lab.bin.clone(),
        layers,
        root: tempfile::tempdir().unwrap(),
    };
    let rt = lab.runtime();
    let id = BoxId::new("hex-oci-mismatch").unwrap();
    match rt.create(lab_mismatch.spec(id.clone(), Bounds::default())) {
        Err(hex_api::Refusal::Platform { what, why, .. }) => {
            assert_eq!(what, "the oci class's image");
            assert!(why.contains(&image.to_string()), "{why}");
            assert!(why.contains(&host.build.to_string()), "{why}");
        }
        other => panic!("a mismatched image (build {image}): {other:?}"),
    }
    assert!(rt.inspect(&id).is_err(), "a refused create made a box");
}

/// The volumes attached on this host (`mountvol`'s `\\?\Volume{..}` lines).
fn volumes() -> Vec<String> {
    let out = std::process::Command::new("mountvol").output().unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with(r"\\?\Volume{"))
        .map(str::to_owned)
        .collect()
}

/// Whether HCS holds a compute system named `id` (`hcsdiag list`).
fn hcs_holds(id: &BoxId) -> bool {
    let out = std::process::Command::new("hcsdiag")
        .arg("list")
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).contains(id.as_str())
}

/// Native step 4's teardown (the lead's condition, 2026-11-06): do: start a box, write in it, delete it.
/// Expect: nothing left — no compute system, no volume beyond those attached before, no scratch folder;
/// `UnprepareLayer`'s ERROR_FLT_DO_NOT_DETACH is matched alone, so any other failure fails the delete.
fn a_deleted_box_leaves_nothing(lab: &Lab) {
    let before = volumes();
    let rt = lab.runtime();
    let id = made(
        &rt,
        lab.spec(BoxId::new("hex-oci-teardown").unwrap(), Bounds::default()),
    );
    let wrote = run(&rt, lab, &id, &["write", r"C:\scratch-was-here"]);
    assert_eq!(wrote.0, Exit::Code(0), "{wrote:?}");
    assert!(
        volumes().len() > before.len(),
        "the started box shows no volume of its own"
    );
    rt.delete(&id).expect("delete");
    assert!(!hcs_holds(&id), "HCS still holds {id}");
    assert_eq!(volumes(), before, "a volume outlived its box");
    assert!(
        !lab.root.path().join(format!("scratch-{id}")).exists(),
        "the scratch folder outlived its box"
    );
}

/// Native step 4 (the lead's condition, 2026-11-06): copies of one `blank.vhdx` share a volume identity, so
/// do: create and start `n` boxes at once, each writing a file of its own. Expect: every one starts, and
/// none sees another's file — each runs on a volume of its own. `n` is 8 and 32, the plan's concurrency
/// points (its throughput is measured at 1, 8 and 32: bench/competitive/README.md:24; 1 is every other case).
fn boxes_started_at_once_each_have_their_own_volume(lab: &Lab, n: usize) {
    let rt = lab.runtime();
    let ids: Vec<BoxId> = (0..n)
        .map(|i| BoxId::new(&format!("hex-oci-at-once-{n}-{i}")).unwrap())
        .collect();
    let specs: Vec<BoxSpec> = ids
        .iter()
        .map(|id| lab.spec(id.clone(), Bounds::default()))
        .collect();
    std::thread::scope(|s| {
        let started: Vec<_> = specs
            .into_iter()
            .map(|spec| {
                let rt = &rt;
                s.spawn(move || {
                    let id = spec.id.clone();
                    let _ = rt.delete(&id);
                    rt.create(spec).and_then(|_| rt.start(&id))
                })
            })
            .collect();
        for (id, h) in ids.iter().zip(started) {
            let got = h.join().unwrap();
            assert!(got.is_ok(), "box {id} of {n} started at once: {got:?}");
        }
    });
    for (i, id) in ids.iter().enumerate() {
        let wrote = run(&rt, lab, id, &["write", &format!(r"C:\mark-{i}")]);
        assert_eq!(wrote.0, Exit::Code(0), "{id}: {wrote:?}");
    }
    for (i, id) in ids.iter().enumerate() {
        let other = (i + 1) % n;
        let (exit, seen) = run(&rt, lab, id, &["exists", &format!(r"C:\mark-{other}")]);
        assert_eq!(exit, Exit::Code(0));
        assert_eq!(
            seen.trim(),
            "no",
            "{id} sees box {other}'s file: they share a volume"
        );
    }
    for id in &ids {
        rt.delete(id).expect("delete");
    }
}

/// AC-T7.1.1 and native step 4: the Windows oci class end to end on this host's image, in order (one
/// runtime's boxes at a time, so the cases don't contend for one container image's scratch copies).
#[test]
fn windows_oci_boxes_hold_the_contract_and_the_steps_criteria() {
    let Some(lab) = Lab::new() else {
        eprintln!("SKIP: HEX_WINDOWS_LAYERS names no image layers");
        return;
    };
    eprintln!("RUN: layers {:?}", lab.layers);
    a_mismatched_image_is_refused_by_name(&lab);
    a_deleted_box_leaves_nothing(&lab);
    a_killed_nodes_scratch_is_reclaimed_at_the_next_start(&lab);
    boxes_started_at_once_each_have_their_own_volume(&lab, 8);
    boxes_started_at_once_each_have_their_own_volume(&lab, 32);
    contract(&lab);
    a_read_only_mount_refuses_writes(&lab);
    a_device_not_granted_isnt_visible(&lab);
    eprintln!("PASS: HEX-RT-1..13, device, read-only mount, version refusal");
}

/// A box's scratch path never escapes the lab's temp root (a sanity check on the fixture itself).
#[test]
fn the_fixture_keeps_each_boxs_files_under_its_root() {
    let lab = Lab {
        bin: PathBuf::from(r"C:\x"),
        layers: vec![r"D:\l".to_owned()],
        root: tempfile::tempdir().unwrap(),
    };
    let dir = lab.bundle(&BoxId::new("b").unwrap(), &["a".to_owned()], json!([]));
    assert!(dir.starts_with(lab.root.path()));
    assert!(dir.join("config.json").exists());
}

/// The bundle a crash test's child node makes, at a path the parent knows: `root\bundles\<id>`, its
/// scratch `root\bundles\<id>-scratch` (a bundle's scratch is the caller's folder, last in layerFolders).
fn crash_bundle(root: &std::path::Path, id: &str, layers: &[String]) -> (PathBuf, PathBuf) {
    let dir = root.join("bundles").join(id);
    let scratch = root.join("bundles").join(format!("{id}-scratch"));
    std::fs::create_dir_all(&dir).unwrap();
    let mut folders = layers.to_vec();
    folders.push(scratch.to_str().unwrap().to_owned());
    let config = json!({
        "ociVersion": "1.2.0",
        "process": {"args": [r"C:\hex\hex.exe", "selftest-box", "wait"], "cwd": "C:\\"},
        "windows": {"layerFolders": folders},
    });
    std::fs::write(dir.join("config.json"), config.to_string()).unwrap();
    (dir, scratch)
}

/// The child half of the crash test: when HEX_CRASH_NODE is `<phase>;<root>`, act as a node that
/// creates (phase `created`) or creates and starts (phase `started`) one box under `root`, say READY, and
/// wait to be killed. Without it, nothing.
#[test]
fn crash_child_node() {
    let Ok(arg) = std::env::var("HEX_CRASH_NODE") else {
        return;
    };
    let (phase, root) = arg.split_once(';').unwrap();
    let root = PathBuf::from(root);
    let lab = Lab::new().expect("the crash test runs with HEX_WINDOWS_LAYERS");
    let rt = WinRuntime::with(4, root.join("scratch")).with_launcher(lab.bin.clone());
    let id = BoxId::new(&format!("hex-crash-{phase}")).unwrap();
    let (dir, _) = crash_bundle(&root, id.as_str(), &lab.layers);
    rt.create(BoxSpec {
        id: id.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Bundle(dir),
    })
    .expect("create");
    if phase == "started" {
        rt.start(&id).expect("start");
    }
    println!("READY");
    loop {
        std::thread::park();
    }
}

/// Native step 4 (the lead, 2026-11-06): a node killed with boxes made leaves no scratch behind once a
/// node starts again on the same scratch root. Do: run a child node that creates a box (and, once, starts
/// it), kill it, and start a new runtime on its scratch root. Expect: no sandbox.vhdx copy, no activated
/// volume (vmcompute names no mount path for the scratch), no scratch marker, and no HCS system of the
/// box's id. The container ends with the node by design (ShouldTerminateOnLastHandleClosed, kill-on-close);
/// the scratch is what a crash leaves (CLAUDE.md §2).
fn a_killed_nodes_scratch_is_reclaimed_at_the_next_start(lab: &Lab) {
    let exe = std::env::current_exe().unwrap();
    for phase in ["created", "started"] {
        let root = lab.root.path().join(format!("crash-{phase}"));
        std::fs::create_dir_all(&root).unwrap();
        let mut child = std::process::Command::new(&exe)
            .args([
                "--exact",
                "crash_child_node",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HEX_CRASH_NODE", format!("{phase};{}", root.display()))
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut out = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        loop {
            line.clear();
            let n = std::io::BufRead::read_line(&mut out, &mut line).unwrap();
            assert!(n > 0, "the child node ended before READY");
            if line.trim() == "READY" {
                break;
            }
        }
        child.kill().unwrap();
        child.wait().unwrap();
        let id = format!("hex-crash-{phase}");
        let scratch = root.join("bundles").join(format!("{id}-scratch"));
        assert!(
            scratch.join("sandbox.vhdx").exists(),
            "{phase}: the killed node left no scratch to reclaim"
        );
        // A node starting on the same scratch root reclaims what the killed one made.
        let _rt = WinRuntime::with(4, root.join("scratch")).with_launcher(lab.bin.clone());
        assert!(
            !scratch.join("sandbox.vhdx").exists(),
            "{phase}: the scratch copy outlived its node"
        );
        assert!(
            hex_runtime::windows::oci::sys::layer_mount_path(scratch.to_str().unwrap()).is_err(),
            "{phase}: the scratch volume is still activated"
        );
        let markers: Vec<_> = std::fs::read_dir(root.join("scratch"))
            .map(|d| d.filter_map(Result::ok).map(|e| e.path()).collect())
            .unwrap_or_default();
        assert!(markers.is_empty(), "{phase}: markers left {markers:?}");
        assert!(
            !hcs_holds(&BoxId::new(&id).unwrap()),
            "{phase}: HCS still holds {id}"
        );
    }
}
