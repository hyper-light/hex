//! The probe's bench comes to its end (native plan step 8, so the stdio record is retaken without retries):
//! no round waits on an end that afunix.sys may not deliver. A `conn` stream ends with its connection's
//! graceful close, which now and then never wakes a read waiting for it in another process (the hostile-frame
//! finding of docs/design/runtime.md; here ~1 in 100 `conn` streams on a quiet jupiter, 2026-11-09, each hang
//! with the stream's bytes all in). 2,000 rounds keep at least one such stream with probability above
//! 1 - 0.99^2000, so a probe that waits for its streams' ends fails this test, not one run in a hundred.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// Rounds in the run: each a stream from every candidate.
const ROUNDS: usize = 2000;
/// Longest the run may take: its 2,000 rounds of 64 KiB streams take seconds; a bound, so a hung run fails
/// the test instead of hanging it.
const PATIENCE: Duration = Duration::from_secs(60);

/// Native plan step 8: do run the bench, 2,000 rounds of each candidate's 64 KiB stream. Expect it to end
/// within the bound, every round's row written, every stream's bytes in.
#[test]
fn the_bench_comes_to_its_end() {
    let mut node = Command::new(env!("CARGO_BIN_EXE_stdio_probe"))
        .args(["bench", &ROUNDS.to_string(), "65536"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("start the probe");
    let pid = node.id();
    let out = node.stdout.take().expect("the probe's stdout");
    let rows = std::thread::spawn(move || {
        BufReader::new(out)
            .lines()
            .map_while(Result::ok)
            .filter(|l| l.starts_with(|c: char| c.is_ascii_digit()))
            .count()
    });
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(node.wait());
    });
    match rx.recv_timeout(PATIENCE) {
        Ok(status) => assert!(status.expect("wait for the probe").success()),
        Err(_) => {
            // The run this test started, and its caller, ended by the pid recorded as it started.
            let _ = Command::new("taskkill")
                .args(["/T", "/F", "/PID", &pid.to_string()])
                .output();
            panic!(
                "the bench hadn't ended after {PATIENCE:?}: a round waited on an end that never came"
            );
        }
    }
    assert_eq!(rows.join().expect("the rows' reader"), ROUNDS * 4);
}
