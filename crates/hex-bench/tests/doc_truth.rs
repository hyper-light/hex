//! Doc-truth (CLAUDE.md §5 and §6): a figure the design quotes from a run is the run's. Between a
//! `<!-- record: PATH -->` marker and its `<!-- /record -->` in docs/design/runtime-architecture.md, every number is
//! one the record PATH holds, so a measured figure can't drift from the run that measured it. Decision 4's (CP-T7.1,
//! goal 4 plan step 1: what a network namespace costs, and what a pool of them holds) are the first.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::path::{Path, PathBuf};

const DESIGN: &str = "docs/design/runtime-architecture.md";
const OPEN: &str = "<!-- record: ";
const CLOSE: &str = "<!-- /record -->";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The numbers in `text`, as a figure is written: runs of digits with the commas and points between digits
/// (`6,127`, `0.571`, `6.19.10`), each with its commas dropped so `6,127` and `6127` are one figure.
fn numbers(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let mut n = String::new();
        while i < chars.len() {
            let c = chars[i];
            let joins =
                (c == ',' || c == '.') && chars.get(i + 1).is_some_and(char::is_ascii_digit);
            if c.is_ascii_digit() {
                n.push(c);
            } else if joins {
                if c == '.' {
                    n.push(c);
                }
            } else {
                break;
            }
            i += 1;
        }
        out.push(n);
    }
    out
}

/// `text` with every comma between two digits taken out, so its figures read as [`numbers`] gives them.
fn uncommaed(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    chars
        .iter()
        .enumerate()
        .filter(|&(i, &c)| {
            !(c == ','
                && i > 0
                && chars[i - 1].is_ascii_digit()
                && chars.get(i + 1).is_some_and(char::is_ascii_digit))
        })
        .map(|(_, &c)| c)
        .collect()
}

/// Whether `record` holds the figure `n` whole: not as a part of a longer run of digits.
fn holds(record: &str, n: &str) -> bool {
    let digit_at = |i: usize| record.as_bytes().get(i).is_some_and(u8::is_ascii_digit);
    record
        .match_indices(n)
        .any(|(at, _)| (at == 0 || !digit_at(at - 1)) && !digit_at(at + n.len()))
}

/// Each `(record path, quoted text)` the design marks.
fn marked(design: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = design;
    while let Some((_, after)) = rest.split_once(OPEN) {
        let (path, body) = after
            .split_once(" -->")
            .expect("a record marker ends with ` -->`");
        let (quoted, next) = body
            .split_once(CLOSE)
            .expect("a record marker is closed by `<!-- /record -->`");
        out.push((path.trim().to_owned(), quoted.to_owned()));
        rest = next;
    }
    out
}

/// Decision 4's quoted figures, the namespace's cost on three machines and the pool's measurements, are each
/// in bench/competitive/probes/netns_pool.md, the record they name.
#[test]
fn every_figure_the_design_marks_is_in_its_record() {
    let design = std::fs::read_to_string(repo().join(DESIGN)).expect("the design document");
    let spans = marked(&design);
    let netns: Vec<_> = spans
        .iter()
        .filter(|(path, _)| path == "bench/competitive/probes/netns_pool.md")
        .collect();
    assert!(
        netns.len() >= 2,
        "decision 4 marks its two tables of record, found {}",
        netns.len()
    );
    let mut checked = 0;
    let mut missing = Vec::new();
    for (path, body) in &spans {
        let record = std::fs::read_to_string(repo().join(path))
            .unwrap_or_else(|e| panic!("the record {path} the design names: {e}"));
        let record = uncommaed(&record);
        for n in numbers(body) {
            checked += 1;
            if !holds(&record, &n) {
                missing.push(format!("{n} (quoted from {path})"));
            }
        }
    }
    // Non-vacuity: the marked spans hold the figures they were written with, not an empty table.
    assert!(
        checked >= 60,
        "the marked spans quote {checked} figures; decision 4's two tables alone hold more"
    );
    assert!(
        missing.is_empty(),
        "figures the design quotes that their record doesn't hold: {missing:?}"
    );
}

#[test]
fn a_figure_is_read_whole_and_its_commas_dropped() {
    assert_eq!(numbers("6,127, 5,260/s"), ["6127", "5260"]);
    assert_eq!(numbers("+0.571 (1.04x)"), ["0.571", "1.04"]);
    assert_eq!(
        numbers("Linux 6.19.10, i9-14900HX"),
        ["6.19.10", "9", "14900"]
    );
    assert_eq!(
        uncommaed("1,029 against 1,079/s, and"),
        "1029 against 1079/s, and"
    );
}

#[test]
fn a_figure_held_inside_a_longer_one_is_not_held() {
    assert!(holds("one thread 6127/s", "6127"));
    assert!(!holds("one thread 16127/s", "6127"));
    assert!(!holds("0.5710 ms", "0.571"));
    assert!(holds("Linux 6.18.54.1", "6.18"));
}

#[test]
fn a_marker_names_its_record_and_closes() {
    let text = "a <!-- record: x.md -->\n| 1.5 |\n<!-- /record --> b <!-- record: y.md --> 2 <!-- /record -->";
    let spans = marked(text);
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].0, "x.md");
    assert!(spans[0].1.contains("1.5"));
    assert_eq!(spans[1].0, "y.md");
}
