//! AC-T7.4.1 producer of the macOS managed CPU control's measured response. On this Mac it measures what
//! the native watcher's stop costs: a one-shot `NOTE_CRITICAL` kqueue timer (the watcher's own,
//! `Kqueue::wait`) is armed for `--wait-nanos`, and when it fires a running member is stopped and read
//! until the kernel reports it stopped. One sample is the time from the timer's due instant to that
//! observed stop: the wake's lateness plus the stop's delivery. It writes the distribution as this host
//! class's row at the head of the cost evidence (the producer's own section, replacing an older row of
//! the same host), the raw samples beside it, and the cost re-keyed to the new evidence's SHA-256.
//!
//! cargo run --release -p hex-cli --example macos_cpu_response -- --cost-file <cost.json>
//!   --cost-sha256 <hex> --evidence <evidence> --wait-nanos <n> --samples <n> --bound-nanos <n> --out <dir>
//!
//! `--wait-nanos`, `--samples` and `--bound-nanos` are the measurement's own parameters, the operator's
//! to choose and record: a sample that doesn't see the stop within `--bound-nanos` refuses the run.
// A measurement example, not production code (CLAUDE.md): the test crates' opt-outs.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::process::ExitCode;

#[cfg(target_os = "macos")]
fn main() -> ExitCode {
    let args: Result<Vec<String>, String> = std::env::args_os()
        .skip(1)
        .map(|word| {
            word.into_string()
                .map_err(|_| "arguments must be UTF-8".to_owned())
        })
        .collect();
    match args.and_then(measure) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("macos_cpu_response: {why}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() -> ExitCode {
    eprintln!("macos_cpu_response measures a macOS host; run it on the Mac whose row it writes");
    ExitCode::FAILURE
}

#[cfg(target_os = "macos")]
fn measure(args: Vec<String>) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    let value = |name: &str| -> Result<&str, String> {
        let at = args
            .iter()
            .position(|word| word == name)
            .ok_or_else(|| format!("{name} is required"))?;
        args.get(at + 1)
            .map(String::as_str)
            .ok_or_else(|| format!("{name} needs a value"))
    };
    let number = |name: &str| -> Result<u64, String> {
        value(name)?
            .parse::<u64>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(|| format!("{name} must be a positive integer"))
    };
    let cost_file = PathBuf::from(value("--cost-file")?);
    let evidence_file = PathBuf::from(value("--evidence")?);
    let out = PathBuf::from(value("--out")?);
    let mut expected = [0u8; 32];
    let digits = value("--cost-sha256")?;
    if digits.len() != 64 {
        return Err("--cost-sha256 must be 64 hex digits".to_owned());
    }
    for (byte, at) in expected.iter_mut().zip((0..64).step_by(2)) {
        *byte = u8::from_str_radix(digits.get(at..at + 2).unwrap_or("zz"), 16)
            .map_err(|_| "--cost-sha256 must be 64 hex digits".to_owned())?;
    }
    let wait = Duration::from_nanos(number("--wait-nanos")?);
    let samples = usize::try_from(number("--samples")?).map_err(|e| e.to_string())?;
    let bound = Duration::from_nanos(number("--bound-nanos")?);

    // The inputs as the node reads them: the cost, its SHA-256 and the evidence its id hashes.
    let (cost, _, _) =
        hex_runtime::guest::load_runtime_evidence(&cost_file, expected, &evidence_file)
            .map_err(|e| e.to_string())?;
    let evidence = std::fs::read(&evidence_file).map_err(|e| e.to_string())?;
    let (mut rows, original) =
        hex_runtime::guest::cpu_response_split(&evidence).map_err(|e| e.to_string())?;
    let host = hex_runtime::macos::machine_model().map_err(|e| e.to_string())?;

    let mut member = std::process::Command::new("/usr/bin/yes")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("starting the busy member: {e}"))?;
    let pid = i32::try_from(member.id()).map_err(|e| e.to_string())?;
    let target = rustix::process::Pid::from_raw(pid).ok_or("member pid")?;
    let kq = hex_runtime::macos::sys::Kqueue::new().map_err(|e| e.to_string())?;
    let mut events = Vec::new();
    let mut taken = Vec::with_capacity(samples);
    let result = (|| -> Result<(), String> {
        for _ in 0..samples {
            let due = Instant::now() + wait;
            kq.wait(Some(wait), None, &mut events)
                .map_err(|e| format!("the watcher timer: {e}"))?;
            rustix::process::kill_process(target, rustix::process::Signal::STOP)
                .map_err(|e| format!("stopping the member: {e}"))?;
            let deadline = due + bound;
            loop {
                if hex_runtime::macos::sys::run_state(pid).map_err(|e| e.to_string())?
                    == Some(hex_runtime::macos::sys::Run::Stopped)
                {
                    break;
                }
                if Instant::now() > deadline {
                    return Err(format!(
                        "no stop seen within --bound-nanos {}",
                        bound.as_nanos()
                    ));
                }
                std::thread::yield_now();
            }
            taken.push(Instant::now().saturating_duration_since(due));
            rustix::process::kill_process(target, rustix::process::Signal::CONT)
                .map_err(|e| format!("continuing the member: {e}"))?;
        }
        Ok(())
    })();
    let _ = member.kill();
    let _ = member.wait();
    result?;

    let raw: String = taken
        .iter()
        .map(|d| format!("{}\n", d.as_nanos()))
        .collect();
    taken.sort_unstable();
    // Nearest rank: the smallest sample with at least that share of samples at or below it.
    let rank = |share: usize| -> Duration {
        let at = (samples * share).div_ceil(100).max(1) - 1;
        taken.get(at).copied().unwrap_or_default()
    };
    let row = hex_runtime::guest::CpuResponse {
        host: host.clone(),
        samples: u64::try_from(samples).map_err(|e| e.to_string())?,
        p50: rank(50),
        p90: rank(90),
        p99: rank(99),
        max: taken.last().copied().unwrap_or_default(),
        raw: Sha256::digest(raw.as_bytes()).into(),
    };
    rows.retain(|known| known.host != host);
    rows.push(row.clone());
    rows.sort_by(|a, b| a.host.cmp(&b.host));
    let mut written = hex_runtime::guest::cpu_response_section(&rows).map_err(|e| e.to_string())?;
    written.extend_from_slice(original);
    let mut rekeyed = cost;
    rekeyed.id = Sha256::digest(&written).into();
    let cost_bytes = serde_json::to_vec(&rekeyed).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let samples_file = out.join(format!("cpu-response-{host}.samples"));
    std::fs::write(&samples_file, raw.as_bytes()).map_err(|e| e.to_string())?;
    std::fs::write(out.join("evidence"), &written).map_err(|e| e.to_string())?;
    std::fs::write(out.join("cost.json"), &cost_bytes).map_err(|e| e.to_string())?;
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    println!(
        "host {host} samples {} p50 {:?} p90 {:?} p99 {:?} max {:?}",
        row.samples, row.p50, row.p90, row.p99, row.max
    );
    println!(
        "raw samples {} sha256 {}",
        samples_file.display(),
        hex(&row.raw)
    );
    println!(
        "evidence {} (cost id {})",
        out.join("evidence").display(),
        hex(&rekeyed.id)
    );
    println!(
        "cost {} HEX_GUEST_COST_SHA256={}",
        out.join("cost.json").display(),
        hex(&Sha256::digest(&cost_bytes))
    );
    Ok(())
}
