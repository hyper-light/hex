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

/// The display adapter's device interface class (`GUID_DEVINTERFACE_DISPLAY_ADAPTER`), the DirectX GPU
/// class Windows grants containers (`hardware-devices-in-containers.md`; `gpu-acceleration.md:37`).
const DISPLAY_ADAPTER: &str = "5B45201D-F2F2-4F3B-85BB-30FF1F953599";

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

/// The step's criterion: a device not granted isn't visible. Do: on a host with a display adapter, list
/// that class's interfaces from inside a box granted nothing. Expect: the host lists at least one, the box
/// none.
fn a_device_not_granted_isnt_visible(lab: &Lab) {
    let host = hex_runtime::windows::oci::devices(DISPLAY_ADAPTER).expect("the host's devices");
    assert!(
        host >= 1,
        "the host lists no display adapter, so a box's view can't be checked against it"
    );
    let rt = lab.runtime();
    let id = made(
        &rt,
        lab.spec_with(
            BoxId::new("hex-oci-devices").unwrap(),
            Bounds::default(),
            &Act::Wait,
            json!([]),
        ),
    );
    let (exit, out) = run(&rt, lab, &id, &["win-devices", DISPLAY_ADAPTER]);
    rt.delete(&id).expect("delete");
    assert_eq!(exit, Exit::Code(0), "the probe failed: {out:?}");
    assert_eq!(
        out.trim(),
        "0",
        "a box granted nothing sees {out:?} display adapters"
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
    contract(&lab);
    a_device_not_granted_isnt_visible(&lab);
    a_read_only_mount_refuses_writes(&lab);
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
