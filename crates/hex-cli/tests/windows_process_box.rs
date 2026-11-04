//! The Windows `process` class against the runtime contract (AC-T7.1.1, Hex HEX-RT-1..9) and the
//! step's own scenarios: a member committing past its bound refused at the bound (the job's peak at or
//! below it), a member asking to break away refused, and delete ending a box's grandchildren. The
//! programs are `hex selftest-box` acts, so the real box is held to the same contract as the simulator
//! in `hex-api` and the other OSes' `process` classes. Runs on Windows (jupiter); the recorded run is
//! the native plan step 3's evidence.
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
use std::process::Command;
use std::time::{Duration, Instant};

use hex_api::conformance::{self, Act, Fixture};
use hex_api::{Bounds, BoxId, BoxSpec, ClassSpec, Exit, Program, Runtime, Signal, Stdio};
use hex_runtime::windows::WinRuntime;

const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;

struct BinFixture {
    exe: String,
}

impl BinFixture {
    fn act(&self, act: &Act) -> Program {
        let mut args = vec![self.exe.clone(), "selftest-box".to_owned()];
        match act {
            Act::Say(words) => args.extend(["say".to_owned(), (*words).to_owned()]),
            Act::Fail(code) => args.extend(["fail".to_owned(), code.to_string()]),
            Act::Wait => args.push("wait".to_owned()),
            Act::Hog(bytes) => args.extend(["hog".to_owned(), bytes.to_string()]),
            Act::Spawn(n) => args.extend(["spawn".to_owned(), n.to_string()]),
            Act::Echo => args.push("echo".to_owned()),
        }
        Program::of(&args.iter().map(String::as_str).collect::<Vec<_>>())
    }

    fn named(&self, act: &str) -> Program {
        Program::of(&[&self.exe, "selftest-box", act])
    }
}

impl Fixture for BinFixture {
    fn spec(&self, id: BoxId, bounds: Bounds) -> BoxSpec {
        BoxSpec {
            id,
            bounds,
            class: ClassSpec::Process,
        }
    }

    fn init(&self, id: BoxId, _act: &Act) -> BoxSpec {
        self.spec(id, Bounds::default())
    }

    fn program(&self, act: &Act) -> Program {
        self.act(act)
    }

    /// Windows has no §3-clean count of a member's own refused fork (the job's accounting doesn't move,
    /// and the only signal is a lossy completion message; docs/research/runtime/windows-pids-refused.md),
    /// so HEX-RT-6 checks the bound's deterministic effect here (the blocked program exits non-zero)
    /// while the class still counts the refusals it sees synchronously (Ada's call, 2026-11-03).
    fn refused_forks_counted(&self) -> bool {
        false
    }
}

fn runtime() -> WinRuntime {
    WinRuntime::new()
}

fn fixture() -> BinFixture {
    BinFixture {
        exe: env!("CARGO_BIN_EXE_hex").to_owned(),
    }
}

/// AC-T7.1.1: the Windows `process` class holds the runtime contract (HEX-RT-1..9; 10..13 are the oci
/// class's, which this one has nothing to check).
/// HEX-INST-2 (native plan step 6): an `oci` box on Windows is refused by name, and the refusal comes
/// from the features' state the node read without elevation (no read error): it names each of the
/// installer's features that's off and says the installer turns it on, or, with both on, what the class
/// still waits on. The installer's lab runs this before install and after, and prints which.
#[test]
fn an_oci_box_is_refused_naming_the_features_that_are_off() {
    let rt = runtime();
    let refused = rt
        .create(BoxSpec {
            id: BoxId::new("oci-on-windows").unwrap(),
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(std::path::PathBuf::from(r"C:\nowhere")),
        })
        .unwrap_err();
    let hex_api::Refusal::Platform { what, platform, why } = &refused else {
        panic!("not refused by name: {refused:?}");
    };
    assert_eq!((*what, *platform), ("the oci class", "windows"));
    eprintln!("the oci class: {why}");
    assert!(
        why.contains("Hex's installer turns") || why.contains("neither is built yet"),
        "{why}"
    );
    assert!(rt.list().unwrap().is_empty(), "the query's box is left");
}

#[test]
fn windows_process_boxes_hold_the_contract() {
    let failures = conformance::run(&runtime(), &fixture());
    assert!(failures.is_empty(), "conformance failures: {failures:#?}");
}

/// The step's criterion: a member committing past its box's 256 MiB bound is refused at the bound, and
/// the job's peak is at or below the bound. The commit is refused (the member exits non-zero), the box
/// counts reaching its memory bound, and the peak the job reports is at or below 256 MiB.
#[test]
fn a_box_past_its_bound_is_refused_at_the_bound() {
    let rt = runtime();
    let fx = fixture();
    let id = BoxId::new("win-over").unwrap();
    let _ = rt.delete(&id);
    let bound = 256 * MIB;
    rt.create(BoxSpec {
        id: id.clone(),
        bounds: Bounds {
            memory_max: Some(bound),
            ..Bounds::default()
        },
        class: ClassSpec::Process,
    })
    .expect("create");
    let mut p = rt
        .exec(&id, fx.act(&Act::Hog(2 * GIB)), Stdio::null())
        .expect("exec hog");
    let exit = p
        .wait_for(Duration::from_secs(30))
        .expect("wait")
        .expect("the hog ends");
    let usage = rt.usage(&id).expect("usage");
    rt.delete(&id).expect("delete");
    assert_ne!(exit, Exit::Code(0), "a hog over the bound exited 0");
    assert!(usage.oom_kills >= 1, "memory bound not counted: {usage:?}");
    assert!(
        usage.memory_peak_bytes <= bound,
        "peak {} over the bound {bound}",
        usage.memory_peak_bytes
    );
}

/// The step's scenario: a box under its bound runs its program to its own exit, reported with its code.
#[test]
fn a_box_under_its_bound_runs_and_reports_its_exit() {
    let rt = runtime();
    let fx = fixture();
    let id = BoxId::new("win-under").unwrap();
    let _ = rt.delete(&id);
    rt.create(BoxSpec {
        id: id.clone(),
        bounds: Bounds {
            memory_max: Some(256 * MIB),
            ..Bounds::default()
        },
        class: ClassSpec::Process,
    })
    .expect("create");
    let mut p = rt
        .exec(&id, fx.act(&Act::Fail(7)), Stdio::null())
        .expect("exec");
    let exit = p
        .wait_for(Duration::from_secs(30))
        .expect("wait")
        .expect("the program ends");
    assert_eq!(exit, Exit::Code(7));
    // Once the member has ended and its exit is read, the box is empty (wait on the fact: the ended
    // process leaves the job once its handle is closed).
    wait_until(
        || {
            rt.inspect(&id)
                .map(|i| i.state == hex_api::BoxState::Empty)
                .unwrap_or(false)
        },
        "the box is empty after its program ends",
    );
    rt.delete(&id).expect("delete");
    assert_eq!(rt.inspect(&id), Err(hex_api::Refusal::NoSuchBox(id)));
}

/// A named signal ends the box's job; the member's exit reads as that signal (TERM is 15 on the seam).
#[test]
fn a_box_takes_a_named_signal() {
    let rt = runtime();
    let fx = fixture();
    let id = BoxId::new("win-signal").unwrap();
    let _ = rt.delete(&id);
    rt.create(fx.spec(id.clone(), Bounds::default()))
        .expect("create");
    let mut p = rt
        .exec(&id, fx.act(&Act::Wait), Stdio::null())
        .expect("exec wait");
    wait_until(
        || {
            rt.inspect(&id)
                .map(|i| i.state == hex_api::BoxState::Running)
                .unwrap_or(false)
        },
        "the box runs",
    );
    rt.signal(&id, Signal::TERM).expect("signal TERM");
    let exit = p
        .wait_for(Duration::from_secs(30))
        .expect("wait")
        .expect("the program ends");
    rt.delete(&id).expect("delete");
    assert_eq!(exit, Exit::Signal(15), "TERM is 15 on the seam");
}

/// The step's criterion: a member asking to break away from its box's job is refused. The `win-breakaway`
/// act tries to start a child with `CREATE_BREAKAWAY_FROM_JOB`; the job forbids it, so the start fails
/// and the act exits 0.
#[test]
fn a_member_asking_to_break_away_is_refused() {
    let rt = runtime();
    let fx = fixture();
    let id = BoxId::new("win-breakaway").unwrap();
    let _ = rt.delete(&id);
    rt.create(fx.spec(id.clone(), Bounds::default()))
        .expect("create");
    let (mut r, w) = std::io::pipe().expect("pipe");
    let mut p = rt
        .exec(
            &id,
            fx.named("win-breakaway"),
            Stdio {
                stdout: Some(w),
                ..Stdio::null()
            },
        )
        .expect("exec win-breakaway");
    let exit = p
        .wait_for(Duration::from_secs(30))
        .expect("wait")
        .expect("the act ends");
    let mut out = String::new();
    r.read_to_string(&mut out).expect("read");
    rt.delete(&id).expect("delete");
    assert_eq!(exit, Exit::Code(0), "breakaway was not refused: {out:?}");
    assert!(out.contains("breakaway=refused"), "act said {out:?}");
}

/// A fork the node itself starts past the box's process bound is refused synchronously and counted
/// exactly (unlike a member's own fork, whose refusal only reaches the box asynchronously). A box
/// bounded to 2 holds two waiting members; the node's third exec is refused as Exhausted, and the box
/// counts it in `pids_refused`.
#[test]
fn a_node_started_process_past_the_bound_is_refused_and_counted() {
    let rt = runtime();
    let fx = fixture();
    let id = BoxId::new("win-pids").unwrap();
    let _ = rt.delete(&id);
    rt.create(BoxSpec {
        id: id.clone(),
        bounds: Bounds {
            pids_max: Some(2),
            ..Bounds::default()
        },
        class: ClassSpec::Process,
    })
    .expect("create");
    let mut held = Vec::new();
    for _ in 0..2 {
        held.push(
            rt.exec(&id, fx.act(&Act::Wait), Stdio::null())
                .expect("exec within the bound"),
        );
    }
    wait_until(
        || rt.usage(&id).map(|u| u.pids >= 2).unwrap_or(false),
        "the box holds its two members",
    );
    // The third member would exceed the bound: the node's own start is refused. (Match without
    // formatting the Ok, whose Box<dyn Proc> has no Debug.)
    match rt.exec(&id, fx.act(&Act::Wait), Stdio::null()) {
        Err(hex_api::Refusal::Exhausted {
            what: "processes", ..
        }) => {}
        Err(other) => panic!("third exec refused wrongly: {other:?}"),
        Ok(_) => panic!("third exec past the bound was not refused"),
    }
    let usage = rt.usage(&id).expect("usage");
    assert!(
        usage.pids_refused >= 1,
        "node-started refusal not counted: {usage:?}"
    );
    rt.delete(&id).expect("delete");
}

/// The step's criterion: delete ends a box's grandchildren. The `win-grandchild` act (the box's member,
/// a child of the node) starts a grandchild and prints its pid; after delete, that grandchild is gone.
#[test]
fn delete_ends_a_boxs_grandchildren() {
    let rt = runtime();
    let fx = fixture();
    let id = BoxId::new("win-grandchild").unwrap();
    let _ = rt.delete(&id);
    rt.create(fx.spec(id.clone(), Bounds::default()))
        .expect("create");
    let (mut r, w) = std::io::pipe().expect("pipe");
    let mut p = rt
        .exec(
            &id,
            fx.named("win-grandchild"),
            Stdio {
                stdout: Some(w),
                ..Stdio::null()
            },
        )
        .expect("exec win-grandchild");
    // The grandchild's pid, from the member's first line.
    let grandchild = read_grandchild_pid(&mut r);
    // Both the member and its grandchild are in the box now.
    wait_until(
        || rt.usage(&id).map(|u| u.pids >= 2).unwrap_or(false),
        "the box holds the member and its grandchild",
    );
    assert!(
        alive(grandchild),
        "the grandchild {grandchild} isn't running"
    );
    rt.delete(&id).expect("delete");
    let exit = p
        .wait_for(Duration::from_secs(30))
        .expect("wait")
        .expect("the member ends");
    assert!(matches!(exit, Exit::Signal(_)), "member exit {exit:?}");
    // The grandchild is ended too: wait on the fact (its pid gone), not a guessed delay.
    wait_until(|| !alive(grandchild), "the grandchild ends with the box");
}

/// The grandchild's pid from the member's `grandchild=<pid>` line, read from the pipe (blocking until
/// the member writes and flushes it).
fn read_grandchild_pid(r: &mut std::io::PipeReader) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut buf = Vec::new();
    let mut chunk = [0u8; 256];
    loop {
        if let Ok(text) = std::str::from_utf8(&buf)
            && let Some(line) = text.lines().find_map(|l| l.strip_prefix("grandchild="))
            && let Ok(pid) = line.trim().parse::<u32>()
        {
            return pid;
        }
        assert!(Instant::now() < deadline, "no grandchild pid: {buf:?}");
        let n = r.read(&mut chunk).expect("read");
        assert!(n > 0, "the member closed its output before a pid: {buf:?}");
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// Whether process `pid` is still running, by asking `tasklist` (pure std, no unsafe in the test): its
/// output names the pid only while it runs.
fn alive(pid: u32) -> bool {
    let out = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .expect("tasklist");
    let text = String::from_utf8_lossy(&out.stdout);
    text.contains(&format!("\"{pid}\""))
}

fn wait_until(mut cond: impl FnMut() -> bool, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        std::thread::yield_now();
    }
    panic!("timed out waiting for: {what}");
}
