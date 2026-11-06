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

/// HEX-INST-9 (native plan step 6): `hex setup features` runs the node's own bounded feature query and
/// prints each feature's state, one `<name> on|off` line each, Containers then HypervisorPlatform, for the
/// Windows installer to read; off Windows it is refused by name, naming the platform.
#[test]
fn setup_features_prints_each_features_state_or_refuses_off_windows() {
    let out = run(&["setup", "features"]);
    if cfg!(windows) {
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        let text = String::from_utf8(out.stdout).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text:?}");
        for (line, name) in lines.iter().zip(["Containers", "HypervisorPlatform"]) {
            let state = line.strip_prefix(name).and_then(|r| r.strip_prefix(' '));
            assert!(matches!(state, Some("on" | "off")), "{line:?}");
        }
    } else {
        assert_eq!(out.status.code(), Some(2), "{out:?}");
        let err = String::from_utf8(out.stderr).unwrap();
        assert!(
            err.contains(&format!("isn't available on {}", std::env::consts::OS)),
            "{err}"
        );
    }
}
