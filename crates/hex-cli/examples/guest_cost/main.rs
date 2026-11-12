//! AC-T7.4.1 producer of a VM class's `RuntimeCost` and its base evidence, measured on the host it runs on:
//! what a node reads as `HEX_GUEST_COST_FILE`, `HEX_GUEST_COST_SHA256` and `HEX_GUEST_COST_EVIDENCE`.
//!
//!   cargo run --release --locked -p hex-cli --example guest_cost -- --class shards --out <dir>
//!
//! with the class's guest configured as its node configures it (Shards: `SHARDS_BIN`, `HEX_GUEST_KERNEL`,
//! `HEX_GUEST_INITRD`, `HEX_GUEST_CPUS`, `HEX_GUEST_MEMORY_MIB`). On a Mac, `macos_cpu_response` then adds the
//! host class's CPU-response row to the same pair.
//!
//! A class is a driver on one seam ([`seam::Driver`]: boot, hold, report, tear down); the census, the
//! evidence and the output are shared ([`run`]). No input is chosen; each is derived, and the evidence says
//! how:
//! - floor = ceiling = the configured guest envelope (what `GuestRuntime::with_runtime_cost` requires);
//! - quantum = each dimension's unit, read here: the API's milli-CPU, the host's page size, and the block
//!   size of the filesystem the VMM's own files go on;
//! - overhead = the VMM's cost outside the envelope: its CPU over an idle hold, its peak memory beyond the
//!   guest's RAM, and the bytes it wrote; guest_overhead = the kernel's and relay's use inside it, the
//!   envelope less what the guest agent reports free. Each is the maximum over SEEDABLE guests (decision
//!   9's census rule, bench/competitive/probes/waits_census.py), each guest booted within the class's
//!   recorded patience, doubled at each run-out, and held idle as long as its own boot took.
// A measurement example, not production code (CLAUDE.md): the test crates' opt-outs.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

// The shared half reads its host's units through rustix, and the drivers so far are macOS's and Linux's.
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod run;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod seam;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod shards;

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: guest_cost --class shards --out <dir>";

/// The run's arguments: which class's driver, and where its output goes. Exactly these, each once.
struct Args {
    class: String,
    out: PathBuf,
}

impl Args {
    fn parse(words: &[String]) -> Result<Self, String> {
        let (mut class, mut out) = (None, None);
        let mut words = words.iter();
        while let Some(name) = words.next() {
            let slot = match name.as_str() {
                "--class" => &mut class,
                "--out" => &mut out,
                other => return Err(format!("unknown argument {other}\n{USAGE}")),
            };
            let value = words
                .next()
                .ok_or_else(|| format!("{name} needs a value\n{USAGE}"))?;
            if slot.replace(value.as_str()).is_some() {
                return Err(format!("{name} is given twice\n{USAGE}"));
            }
        }
        let missing = |name: &str| format!("{name} is required\n{USAGE}");
        Ok(Self {
            class: class.ok_or_else(|| missing("--class"))?.to_owned(),
            out: PathBuf::from(out.ok_or_else(|| missing("--out"))?),
        })
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn main() -> ExitCode {
    eprintln!("guest_cost has no class driver on {}", std::env::consts::OS);
    ExitCode::FAILURE
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn main() -> ExitCode {
    let words: Result<Vec<String>, String> = std::env::args_os()
        .skip(1)
        .map(|word| {
            word.into_string()
                .map_err(|_| "arguments must be UTF-8".to_owned())
        })
        .collect();
    match words.and_then(|words| produce(&Args::parse(&words)?)) {
        Ok(made) => {
            println!("{}", made.summary);
            println!("HEX_GUEST_COST_FILE={}", made.cost_file.display());
            println!("HEX_GUEST_COST_SHA256={}", run::hex(&made.cost_sha256));
            println!("HEX_GUEST_COST_EVIDENCE={}", made.evidence_file.display());
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("guest_cost: {why}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn produce(args: &Args) -> Result<run::Made, String> {
    let plan = run::Plan::derived()?;
    match args.class.as_str() {
        "shards" => {
            let mut driver = shards::Shards::from_env(&args.out)?;
            run::produce(&mut driver, &plan, &args.out)
        }
        other => Err(format!(
            "no driver for class {other} on {}\n{USAGE}",
            std::env::consts::OS
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC-T7.4.1: the producer takes exactly its class and output folder: expect a run's parameters, a
    /// repeated or unknown argument, refused.
    #[test]
    fn the_producer_takes_its_class_and_folder_alone() {
        let words = |list: &[&str]| list.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
        let args = Args::parse(&words(&["--class", "shards", "--out", "/tmp/x"])).unwrap();
        assert_eq!(
            (args.class.as_str(), args.out),
            ("shards", PathBuf::from("/tmp/x"))
        );
        assert!(Args::parse(&words(&["--class", "shards"])).is_err());
        assert!(Args::parse(&words(&["--class", "a", "--class", "b", "--out", "o"])).is_err());
        assert!(
            Args::parse(&words(&[
                "--samples",
                "3",
                "--class",
                "shards",
                "--out",
                "o"
            ]))
            .is_err()
        );
    }
}
