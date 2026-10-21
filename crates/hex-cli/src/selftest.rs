//! `hex selftest-box <act> [args]`: small real programs a node agent runs inside a box to prove the
//! box holds its bounds — a program that uses memory is killed in a box bounded below it, one that
//! starts processes is refused past the box's process bound. The runtime conformance suite
//! (`hex-api`) drives the same acts, so the real classes are checked against the same contract as
//! the simulator. Every act returns an exit status; none panics.

use std::io::{Read, Write};
use std::process::{Command, Stdio};

/// The page the memory act touches, so the kernel accounts the memory to the box.
const PAGE: usize = 4096;
/// Memory taken per chunk while growing, so a box's bound kills the act partway, not after a single
/// huge allocation the allocator might refuse first.
const CHUNK: usize = 1 << 20;

/// Runs act `argv` (`argv[0]` the act, the rest its arguments) and returns the exit status for the
/// process. `None` means the act isn't one of these.
pub fn run(argv: &[String]) -> Option<u8> {
    let (act, args) = argv.split_first()?;
    match act.as_str() {
        "say" => {
            let line = format!("{}\n", args.join(" "));
            Some(u8::from(
                write_all(&mut std::io::stdout(), line.as_bytes()).is_err(),
            ))
        }
        "fail" => {
            let _ = std::io::stderr().write_all(b"failing\n");
            Some(args.first().and_then(|c| c.parse::<u8>().ok()).unwrap_or(1))
        }
        "wait" => loop {
            std::thread::park();
        },
        "hog" => Some(hog(args
            .first()
            .and_then(|b| b.parse::<u64>().ok())
            .unwrap_or(0))),
        "spawn" => Some(spawn(
            args.first()
                .and_then(|n| n.parse::<u32>().ok())
                .unwrap_or(0),
        )),
        "echo" => Some(echo()),
        _ => None,
    }
}

/// Takes and touches `bytes` of memory, a chunk at a time, then holds it briefly. In a box bounded
/// below `bytes` the kernel kills this partway (the caller sees a signal and counts the kill); on an
/// unbounded box it reaches `bytes` and exits 0.
fn hog(bytes: u64) -> u8 {
    let target = usize::try_from(bytes).unwrap_or(usize::MAX);
    let mut held: Vec<Vec<u8>> = Vec::new();
    let mut taken = 0usize;
    while taken < target {
        let want = CHUNK.min(target.saturating_sub(taken));
        let mut chunk = vec![0u8; want];
        let mut i = 0;
        while i < chunk.len() {
            if let Some(b) = chunk.get_mut(i) {
                *b = 1;
            }
            i = i.saturating_add(PAGE);
        }
        held.push(chunk);
        taken = taken.saturating_add(want);
    }
    // Hold it long enough that a bound's kill lands before exit; park briefly, then let go.
    let keep = held.len();
    std::thread::park_timeout(std::time::Duration::from_millis(200));
    u8::from(keep == usize::MAX)
}

/// Starts `n` processes at once (each a `wait` act). Exits 0 when all start, 2 when one is refused
/// (a box's process bound): the caller sees the non-zero exit and counts the refused fork.
fn spawn(n: u32) -> u8 {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return 1,
    };
    let mut children = Vec::new();
    let mut refused = false;
    for _ in 0..n {
        match Command::new(&exe)
            .arg("selftest-box")
            .arg("wait")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => children.push(child),
            Err(_) => {
                refused = true;
                break;
            }
        }
    }
    for mut child in children {
        let _ = child.kill();
        let _ = child.wait();
    }
    u8::from(refused).saturating_mul(2)
}

/// Copies stdin to stdout until stdin ends.
fn echo() -> u8 {
    let mut stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut buf = [0u8; 1 << 16];
    loop {
        match stdin.read(&mut buf) {
            Ok(0) => return 0,
            Ok(n) => {
                if write_all(&mut stdout, buf.get(..n).unwrap_or_default()).is_err() {
                    return 1;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return 1,
        }
    }
}

fn write_all<W: Write>(w: &mut W, bytes: &[u8]) -> std::io::Result<()> {
    w.write_all(bytes)?;
    w.flush()
}
