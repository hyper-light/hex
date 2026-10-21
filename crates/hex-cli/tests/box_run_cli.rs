//! `hex box run` at the command line (AC-T7.1.1/AC-T7.1.2): it runs a program in a `process` box,
//! passes the program's exit status through, and a program over the box's memory bound is killed.
//!
//! A `process` box needs a cgroup v2 subtree the wrapper owns alone (its own cgroup, with the
//! controllers delegated). Each `hex box run` here is launched in its own transient
//! `systemd-run --user --scope -p Delegate=yes`, so the wrapper is alone in a delegated cgroup —
//! exactly how a kiln runner (with `Delegate=yes` on its unit) invokes it. Where `systemd --user`
//! isn't available the test prints SKIP rather than a false pass. Run on a cgroup-v2 host (fedora).
#![cfg(target_os = "linux")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::process::Command;

const HEX: &str = env!("CARGO_BIN_EXE_hex");

fn have_systemd() -> bool {
    Command::new("systemd-run")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Runs `hex box run <limits> -- program…` in its own delegated scope, capturing its output.
fn box_run(limits: &[&str], program: &[&str]) -> std::process::Output {
    let mut cmd = Command::new("systemd-run");
    cmd.args([
        "--user",
        "--scope",
        "--quiet",
        "-p",
        "Delegate=yes",
        "--",
        HEX,
        "box",
        "run",
    ]);
    cmd.args(limits).arg("--").args(program);
    cmd.output().expect("run hex box run in a scope")
}

#[test]
fn box_run_boxes_a_program_and_passes_its_status() {
    if !have_systemd() {
        eprintln!("SKIP: no systemd --user for a delegated scope; run on a cgroup-v2 host");
        return;
    }

    // Its stdout passes through and its exit status is the program's.
    let out = box_run(
        &[
            "--memory-max",
            "268435456",
            "--cpu-weight",
            "50",
            "--io-weight",
            "10",
        ],
        &["/bin/sh", "-c", "echo boxed; exit 7"],
    );
    assert_eq!(out.status.code(), Some(7), "status: {out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "boxed");

    // The program runs inside a box cgroup (a `.hexbox/<id>` path), not the host root.
    let out = box_run(
        &["--memory-max", "134217728"],
        &[
            "/bin/sh",
            "-c",
            "grep -o '[.]hexbox/[^ ]*' /proc/self/cgroup",
        ],
    );
    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(".hexbox/"),
        "cgroup: {:?}",
        String::from_utf8_lossy(&out.stdout)
    );

    // A program over the box's memory bound is killed by the kernel (128 + SIGKILL = 137).
    let out = box_run(
        &["--memory-max", "67108864"],
        &[HEX, "selftest-box", "hog", "536870912"],
    );
    assert_eq!(
        out.status.code(),
        Some(137),
        "a 512 MiB hog in a 64 MiB box should be killed: {out:?}"
    );

    // An unknown flag is a typed refusal with the box-setup status (125), never a silent run.
    let out = box_run(&["--frobnicate", "1"], &["/bin/true"]);
    assert_eq!(out.status.code(), Some(125), "unknown flag: {out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown flag"));
}
