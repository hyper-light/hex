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
        // fsync-probe <file> <iters>: the noisy-neighbour victim — times fsync over small writes
        // and prints "p50_us=<n> p99_us=<n>" (AC-T7.1.2).
        "fsync-probe" => Some(fsync_probe(
            args.first().map(String::as_str).unwrap_or(""),
            args.get(1).and_then(|n| n.parse().ok()).unwrap_or(200),
        )),
        // disk-hog <file> <seconds>: the noisy-neighbour builder — writes and fsyncs a large buffer
        // in a tight loop to saturate the disk for that long.
        "disk-hog" => Some(disk_hog(
            args.first().map(String::as_str).unwrap_or(""),
            args.get(1).and_then(|n| n.parse().ok()).unwrap_or(10),
        )),
        _ => None,
    }
}

/// The victim: writes `BLOCK` bytes and fsyncs `iters` times at `path`, printing the p50 and p99
/// fsync latency in microseconds. A box with a tight `io.latency` target keeps this p99 near idle
/// even while a sibling box saturates the disk.
fn fsync_probe(path: &str, iters: u32) -> u8 {
    use std::io::Seek;
    const BLOCK: usize = 4096;
    let mut file = match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
    {
        Ok(f) => f,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "fsync-probe: {path}: {e}");
            return 1;
        }
    };
    let buf = [b'x'; BLOCK];
    let mut us: Vec<u128> = Vec::with_capacity(iters as usize);
    for _ in 0..iters {
        let _ = file.rewind();
        if file.write_all(&buf).is_err() {
            return 1;
        }
        let t = std::time::Instant::now();
        if file.sync_data().is_err() {
            return 1;
        }
        us.push(t.elapsed().as_micros());
    }
    us.sort_unstable();
    // Percentile by integer index (no float cast in production code): index = len*p/100, clamped.
    let pick = |p: usize| {
        let i = us.len().saturating_mul(p).checked_div(100).unwrap_or(0);
        us.get(i.min(us.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0)
    };
    let _ = writeln!(std::io::stdout(), "p50_us={} p99_us={}", pick(50), pick(99));
    0
}

/// The builder: writes and fsyncs a 1 MiB buffer in a tight loop for `seconds`, saturating the
/// disk — the load the victim must be protected from.
fn disk_hog(path: &str, seconds: u64) -> u8 {
    let mut file = match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
    {
        Ok(f) => f,
        Err(_) => return 1,
    };
    let buf = vec![b'z'; 1 << 20];
    let deadline = std::time::Instant::now().checked_add(std::time::Duration::from_secs(seconds));
    let mut offset = 0u64;
    while deadline.is_none_or(|d| std::time::Instant::now() < d) {
        use std::io::{Seek, SeekFrom};
        let _ = file.seek(SeekFrom::Start(offset));
        if file.write_all(&buf).is_err() {
            return 1;
        }
        let _ = file.sync_data();
        offset = offset
            .checked_add(buf.len() as u64)
            .filter(|o| *o < (1 << 30))
            .unwrap_or(0);
    }
    0
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
