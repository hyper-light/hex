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

/// Goal 6 step 6's HEX-INST-5 (Ada's 2026-11-06 constants rule): the descriptors a node needs are said by the node, at
/// its own bounds on this host and for this process's owner, so an installer gives its service that and the install
/// lab checks it, never a figure copied from one host's (the lab's 594,304 was fedora's before the constants changed).
/// Do: run `hex node --descriptors`. Expect: `own N files M need T`, T the sum, N past the standard streams, and M some
/// for a root node, which keeps namespaces with their `/proc/net` files, none for a rootless one, which keeps none.
#[cfg(target_os = "linux")]
#[test]
fn a_node_says_the_descriptors_it_needs() {
    let out = run(&["node", "--descriptors"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    let words: Vec<&str> = text.split_whitespace().collect();
    let ["own", own, "files", files, "need", need] = words.as_slice() else {
        panic!("{text:?}");
    };
    let (own, files, need): (u64, u64, u64) = (
        own.parse().unwrap(),
        files.parse().unwrap(),
        need.parse().unwrap(),
    );
    assert_eq!(need, own + files, "{text:?}");
    assert!(own > 3, "{text:?}");
    assert_eq!(
        files > 0,
        rustix::process::geteuid().is_root(),
        "a root node keeps namespaces with their files, a rootless one none: {text:?}"
    );
}
