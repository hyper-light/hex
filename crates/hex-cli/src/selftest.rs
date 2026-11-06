//! `hex selftest-box <act> [args]`: small real programs a node agent runs inside a box to prove the
//! box holds its bounds — a program that uses memory is killed in a box bounded below it, one that
//! starts processes is refused past the box's process bound. The runtime conformance suite
//! (`hex-api`) drives the same acts, so the real classes are checked against the same contract as
//! the simulator. Every act returns an exit status; none panics.

use std::io::{Read, Write};
use std::process::{Command, Stdio};

#[cfg(target_os = "linux")]
mod tree;

/// Memory taken per chunk while growing, so a box's bound kills the act partway, not after a single
/// huge allocation the allocator might refuse first.
const CHUNK: usize = 1 << 20;

/// Fills `buf` with an incompressible xorshift stream, so the memory the act takes shows in the box's
/// footprint on every OS. Linux's cgroup counts the resident pages whatever they hold, but macOS's
/// physical-footprint ledger — against which a jetsam per-process cap (the native `process` class's
/// `_ext` backstop) and the watcher's rule both read — counts a page at its *compressed* size, so a box
/// filled with zeros (or any compressible pattern) stays far under its bound and is never bounded. An
/// incompressible fill can't be shrunk, so the footprint is the allocation's true size and the bound
/// bites. Writing a word at a time (not byte) keeps it cheap; a trailing partial word is left as is.
fn fill_incompressible(buf: &mut [u8], state: &mut u64) {
    let mut x = *state;
    let mut i = 0usize;
    while i < buf.len() {
        x ^= x.wrapping_shl(13);
        x ^= x.wrapping_shr(7);
        x ^= x.wrapping_shl(17);
        if let Some(slot) = buf.get_mut(i..i.saturating_add(8)) {
            slot.copy_from_slice(&x.to_ne_bytes());
        }
        i = i.saturating_add(8);
    }
    *state = x;
}

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
        // hold <bytes> <millis>: takes and touches `bytes`, then sits that long — a box held near its
        // bound, for the macOS class bench to measure its watcher's cost (bench/native/macos).
        "hold" => Some(hold(
            args.first()
                .and_then(|b| b.parse::<u64>().ok())
                .unwrap_or(0),
            args.get(1).and_then(|m| m.parse::<u64>().ok()).unwrap_or(0),
        )),
        "spawn" => Some(spawn(
            args.first()
                .and_then(|n| n.parse::<u32>().ok())
                .unwrap_or(0),
        )),
        "echo" => Some(echo()),
        // tree <dir>: a manifest of everything under `dir`, for HEX-GUEST-21's comparison of a staged
        // root with containerd's unpack of the same image (selftest/tree.rs).
        #[cfg(target_os = "linux")]
        "tree" => Some(tree::tree(args.first().map(String::as_str).unwrap_or("."))),
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
        // win-breakaway: tries to start a child outside the box's job (CREATE_BREAKAWAY_FROM_JOB).
        // Prints whether Windows refused it and exits 0 when refused, 1 when the child escaped — the
        // Windows process class forbids breakaway, so a member asking for it is refused.
        #[cfg(windows)]
        "win-breakaway" => Some(win_breakaway()),
        // win-grandchild: starts a grandchild (a child that outlives this member's own start) and
        // prints its process id, then waits — so a box delete can be shown to end the grandchild too.
        #[cfg(windows)]
        "win-grandchild" => Some(win_grandchild()),
        _ => None,
    }
}

/// Windows: a child started with `CREATE_BREAKAWAY_FROM_JOB` from inside a job that forbids breakaway.
/// `CreateProcess` fails (access denied), so the spawn is refused; the box's member reports it and
/// exits 0. If the child ever started, it escaped the box: end it and exit 1.
#[cfg(windows)]
fn win_breakaway() -> u8 {
    use std::os::windows::process::CommandExt;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return 1,
    };
    match Command::new(exe)
        .arg("selftest-box")
        .arg("wait")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_BREAKAWAY_FROM_JOB)
        .spawn()
    {
        Ok(mut child) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = writeln!(std::io::stdout(), "breakaway=escaped");
            1
        }
        Err(_) => {
            let _ = writeln!(std::io::stdout(), "breakaway=refused");
            0
        }
    }
}

/// Windows: starts a grandchild (a `wait` member) in the box, prints its process id, and waits. The
/// grandchild keeps running until the box's job ends it, so a delete can be shown to reach it.
#[cfg(windows)]
fn win_grandchild() -> u8 {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return 1,
    };
    match Command::new(exe)
        .arg("selftest-box")
        .arg("wait")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => {
            let _ = writeln!(std::io::stdout(), "grandchild={}", child.id());
            let _ = std::io::stdout().flush();
            loop {
                std::thread::park();
            }
        }
        Err(_) => 1,
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
    let mut rng = 0x9e37_79b9_7f4a_7c15u64;
    while taken < target {
        let want = CHUNK.min(target.saturating_sub(taken));
        let mut chunk = vec![0u8; want];
        fill_incompressible(&mut chunk, &mut rng);
        held.push(chunk);
        taken = taken.saturating_add(want);
    }
    // Hold it long enough that a bound's kill lands before exit; park briefly, then let go.
    let keep = held.len();
    std::thread::park_timeout(std::time::Duration::from_millis(200));
    u8::from(keep == usize::MAX)
}

/// Takes and touches `bytes`, then sits for `millis`, holding the memory: a box kept near its bound for
/// its watcher's cost to be measured. Exits 0 unless the allocation fails.
fn hold(bytes: u64, millis: u64) -> u8 {
    let target = usize::try_from(bytes).unwrap_or(usize::MAX);
    let mut held: Vec<Vec<u8>> = Vec::new();
    let mut taken = 0usize;
    let mut rng = 0x9e37_79b9_7f4a_7c15u64;
    while taken < target {
        let want = CHUNK.min(target.saturating_sub(taken));
        let mut chunk = vec![0u8; want];
        fill_incompressible(&mut chunk, &mut rng);
        held.push(chunk);
        taken = taken.saturating_add(want);
    }
    std::thread::park_timeout(std::time::Duration::from_millis(millis));
    u8::from(held.len() == usize::MAX)
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
