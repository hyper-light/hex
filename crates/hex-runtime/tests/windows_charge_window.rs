//! Joint plan step 14 (AC-T7.4.1; `docs/design/learned-charge.md` C1): the probe whose result a Windows node's charge
//! window derives from. A job's CPU hard cap holds per scheduling interval, which Windows doesn't document
//! (`JOBOBJECT_CPU_RATE_CONTROL_INFORMATION`), so `WinRuntime::charge_window` is refused, typed, until this measures it
//! on the host.
//!
//! Each run starts a busy loop (one spinning thread per logical processor but one, so the sampler keeps a processor of
//! its own) in a job of its own, its demand past every cap measured, and samples the job's CPU time
//! (`JobObjectBasicAndIoAccountingInformation`'s `BasicInfo`, which is `JobObjectBasicAccountingInformation`) every
//! 1 ms for 10 s, each sample at its own deadline and stamped when it was read. The first run is uncapped: the longest
//! gap between two of its CPU time's steps is the accounting's own step (Windows charges CPU time as its clock ticks),
//! so a gap longer than that in a capped run is the cap stopping the job. Then one run per cap, 10%, 20% and 50% of
//! the machine (`JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP`, 1,000, 2,000 and 5,000 of 10,000). For each it prints one
//! `HEX-CHARGE-WINDOW` line: the rate the job got, and n, p50, p99 and max (nearest rank) of its runs (from the first
//! step after a stop to the last before the next), its stops (the gaps past the accounting's step) and its periods
//! (from one run's start to the next's): the period is the interval the cap holds over.
//!
//! Run on a Windows host with nothing else busy (ROOT's, on jupiter), from the repository's root:
//!   cargo test --locked -p hex-runtime --test windows_charge_window -- --ignored --nocapture --test-threads=1
//! With `HEX_CHARGE_WINDOW_OUT` naming a directory, each run's raw samples go there too, `<cap>.csv` (`uncapped.csv`
//! for the first): the sample's time since the run's start and the job's CPU time so far, both in 100 ns units.
#![cfg(windows)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::fmt::Write as _;
use std::os::windows::io::OwnedHandle;
use std::time::{Duration, Instant};

use hex_runtime::windows::sys::{self, Job, Limits, Spawn};

/// How often the job's CPU time is read, and for how long each run: ROOT's probe, as step 14's brief asks it.
const EVERY: Duration = Duration::from_millis(1);
const FOR: Duration = Duration::from_secs(10);
/// The caps, per `hex_core::job::RATE_SCALE` (10,000) of the machine: 10%, 20% and 50%.
const CAPS: [u32; 3] = [1_000, 2_000, 5_000];
/// What the child is told: how many threads to spin.
const SPIN: &str = "HEX_TEST_SPIN_THREADS";

/// Not a check on its own: run in a job by the probe with `HEX_TEST_SPIN_THREADS` set, it spins that many threads
/// until its job ends; otherwise it does nothing.
#[test]
fn spins_when_asked() {
    let Some(threads) = std::env::var_os(SPIN) else {
        return;
    };
    let threads: usize = threads.to_str().unwrap().parse().unwrap();
    // This thread is one of them.
    for _ in 1..threads {
        std::thread::spawn(spin);
    }
    spin();
}

/// Spins until its process ends.
fn spin() {
    let mut n = 0u64;
    loop {
        n = std::hint::black_box(n.wrapping_add(1));
    }
}

/// Spins until `due`: the sampler has a processor of its own, and a timed wait on Windows ends at its timer's
/// resolution (15.6 ms by default), not at a 1 ms sample's deadline.
fn until(due: Instant) {
    while Instant::now() < due {
        std::hint::spin_loop();
    }
}

fn nul() -> OwnedHandle {
    OwnedHandle::from(
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("NUL")
            .unwrap(),
    )
}

/// One run: a job capped at `cap` (none uncapped) running `threads` spinners, its CPU time in 100 ns read every
/// [`EVERY`] for [`FOR`], each with its time since the run's start.
fn run(cap: Option<u32>, threads: u32) -> Vec<(Duration, u64)> {
    let job = Job::create().unwrap();
    job.configure(&Limits {
        memory: None,
        processes: None,
        cpu: cap.map(hex_core::job::Cpu::Cap),
    })
    .unwrap();
    let exe = std::env::current_exe().unwrap();
    let system = std::env::var("SystemRoot").unwrap();
    let mut spawn = Spawn {
        command_line: format!(
            "\"{}\" spins_when_asked --exact --test-threads=1",
            exe.display()
        )
        .encode_utf16()
        .chain([0])
        .collect(),
        environment: hex_core::job::environment(&[
            ("SystemRoot".to_owned(), system),
            (SPIN.to_owned(), threads.to_string()),
        ]),
        cwd: None,
        stdio: [nul(), nul(), nul()],
    };
    let started = sys::create(&mut spawn, &[job.raw()]).unwrap();
    sys::resume(&started.thread).unwrap();
    let cpu = || job.accounting().unwrap().cpu_100ns;
    // The run starts once the spinners are charged: the job's first step.
    let deadline = Instant::now() + FOR;
    while cpu() == 0 {
        assert!(Instant::now() < deadline, "the spinners were never charged");
        until(Instant::now() + EVERY);
    }
    let count = usize::try_from(FOR.as_micros() / EVERY.as_micros()).unwrap();
    let mut samples = Vec::with_capacity(count + 1);
    let start = Instant::now();
    samples.push((Duration::ZERO, cpu()));
    for k in 1..=count {
        until(start + EVERY * u32::try_from(k).unwrap());
        let read = cpu();
        samples.push((start.elapsed(), read));
    }
    job.terminate(hex_core::job::TERMINATED).unwrap();
    assert!(sys::wait(&started.process, u32::MAX).unwrap());
    samples
}

/// The nearest-rank `q`-quantile (per mille) of `values`, and their count and max: ceil(q n / 1000)-th smallest.
fn quantiles(values: &mut [Duration]) -> String {
    values.sort();
    let n = values.len();
    let rank = |q: usize| -> Duration {
        if n == 0 {
            return Duration::ZERO;
        }
        let at = (q * n).div_ceil(1000).max(1) - 1;
        values[at]
    };
    format!(
        "n={n} p50={:?} p99={:?} max={:?}",
        rank(500),
        rank(990),
        values.last().copied().unwrap_or_default()
    )
}

/// The times of the CPU time's steps, and the gaps between them.
fn steps(samples: &[(Duration, u64)]) -> Vec<Duration> {
    samples
        .windows(2)
        .filter(|pair| pair[1].1 > pair[0].1)
        .map(|pair| pair[1].0)
        .collect()
}

/// A capped run's runs, stops and periods: a stop is a gap between two steps longer than `step`, the accounting's own.
fn cycles(samples: &[(Duration, u64)], step: Duration) -> (Vec<Duration>, Vec<Duration>, Vec<Duration>) {
    let at = steps(samples);
    let (mut runs, mut stops, mut periods) = (Vec::new(), Vec::new(), Vec::new());
    let Some(&first) = at.first() else {
        return (runs, stops, periods);
    };
    let (mut began, mut last) = (first, first);
    for &t in &at[1..] {
        let gap = t - last;
        if gap > step {
            runs.push(last - began);
            stops.push(gap);
            periods.push(t - began);
            began = t;
        }
        last = t;
    }
    (runs, stops, periods)
}

fn keep(name: &str, samples: &[(Duration, u64)]) {
    let Some(dir) = std::env::var_os("HEX_CHARGE_WINDOW_OUT") else {
        return;
    };
    let mut csv = String::from("t_100ns,cpu_100ns\n");
    for (t, cpu) in samples {
        writeln!(csv, "{},{cpu}", t.as_nanos() / 100).unwrap();
    }
    std::fs::write(std::path::Path::new(&dir).join(format!("{name}.csv")), csv).unwrap();
}

/// Joint plan step 14 (AC-T7.4.1), C1 on Windows: the interval a job's CPU hard cap holds over, measured. Do run a busy
/// loop uncapped, then capped at 10%, 20% and 50% of the machine, each sampled every 1 ms for 10 s. Expect each capped
/// run held near its cap and stopped past the accounting's own step, its runs, stops and periods printed; the window
/// `WinRuntime::charge_window` derives from is the period's.
#[test]
#[ignore = "ROOT's probe on a quiet Windows host (jupiter): about 40 s of every processor but one"]
fn the_interval_a_jobs_cpu_hard_cap_holds_over() {
    let processors = sys::active_processors();
    let threads = processors.saturating_sub(1).max(1);
    // The CPU the job got over a run, per RATE_SCALE (10,000) of the machine: its CPU time over the run's span on
    // every processor, both in 100 ns.
    let rate = |samples: &[(Duration, u64)]| -> u128 {
        let (Some(first), Some(last)) = (samples.first(), samples.last()) else {
            return 0;
        };
        let span = ((last.0 - first.0).as_nanos() / 100).max(1);
        u128::from(last.1 - first.1) * 10_000 / (span * u128::from(processors))
    };
    let uncapped = run(None, threads);
    keep("uncapped", &uncapped);
    let at = steps(&uncapped);
    let step = at.windows(2).map(|p| p[1] - p[0]).max().unwrap_or_default();
    println!(
        "HEX-CHARGE-WINDOW processors={processors} spinners={threads} uncapped rate={}/10000 accounting_step={step:?} \
         steps={}",
        rate(&uncapped),
        at.len()
    );
    let demand = u64::from(threads) * 10_000 / u64::from(processors);
    for cap in CAPS {
        assert!(
            demand > u64::from(cap),
            "{threads} spinners of {processors} processors ask {demand}/10000, not past the cap {cap}/10000"
        );
        let samples = run(Some(cap), threads);
        keep(&cap.to_string(), &samples);
        let (mut runs, mut stops, mut periods) = cycles(&samples, step);
        println!(
            "HEX-CHARGE-WINDOW cap={cap}/10000 rate={}/10000 runs[{}] stops[{}] periods[{}]",
            rate(&samples),
            quantiles(&mut runs),
            quantiles(&mut stops),
            quantiles(&mut periods)
        );
        assert!(
            !periods.is_empty(),
            "a job capped at {cap}/10000 never stopped past the accounting's step of {step:?}"
        );
    }
}
