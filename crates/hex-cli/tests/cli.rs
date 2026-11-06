//! The `hex` binary says what it is (kiln's CP-M1: the bar's first gated code), run as a process.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::disallowed_macros
    )
)]

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hex"))
        .args(args)
        .output()
        .expect("the binary runs")
}

#[test]
fn version_prints_the_name_version_and_commit() {
    let out = run(&["--version"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.starts_with(concat!("hex ", env!("CARGO_PKG_VERSION"), " (")),
        "{text}"
    );
    assert!(text.ends_with(")\n"), "{text}");
}

#[test]
fn help_prints_what_it_is_and_the_usage() {
    let out = run(&["--help"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("hex: Hex"), "{text}");
    assert!(text.contains("usage: hex"), "{text}");
}

#[test]
fn an_unknown_argument_is_refused_with_the_usage() {
    let out = run(&["--frobnicate"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let text = String::from_utf8(out.stderr).unwrap();
    assert!(text.contains("unknown arguments: --frobnicate"), "{text}");
    assert!(text.contains("usage: hex"), "{text}");
}

#[test]
fn no_argument_prints_the_usage_and_fails() {
    let out = run(&[]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(
        String::from_utf8(out.stderr)
            .unwrap()
            .starts_with("usage: hex")
    );
}

#[test]
fn an_argument_that_isnt_unicode_is_refused_not_a_panic() {
    assert_eq!(hex_cli::reply(&[None]).status, 2);
    assert!(hex_cli::reply(&[None]).text.contains("isn't Unicode"));
}

/// The native bench's acts (native plan step 7): do `climb` 3 MiB, expect it to report each MiB it
/// touched, one line each, and exit 0, so a bound's overshoot is read from the program itself.
#[test]
fn climb_reports_each_mib_it_touches() {
    let out = run(&["selftest-box", "climb", &(3u64 << 20).to_string()]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "1\n2\n3\n");
}

/// Do `serve`, expect "up" on stdout once it runs, and it to keep running until it is killed: a start
/// is timed to that line, the same for every driver.
#[test]
fn serve_says_up_and_runs_on() {
    use std::io::{BufRead, BufReader};
    let mut child = Command::new(env!("CARGO_BIN_EXE_hex"))
        .args(["selftest-box", "serve"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line, "up\n");
    assert!(child.try_wait().unwrap().is_none(), "serve ended");
    child.kill().unwrap();
    child.wait().unwrap();
}
