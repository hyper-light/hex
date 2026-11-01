//! The macOS `process` class's own path, measured (native plan step 1, AC-T7.1.5-style accounting): how
//! far a box's footprint passes its 256 MiB memory bound before its watcher ends it, with 1, 4, 8, 14 and
//! 28 members allocating at full speed, and the watcher's CPU for a box held at 95% of its bound. Both go
//! through [`hex_runtime::macos::MacRuntime`] and real `hex selftest-box` members, so the figure is the
//! class's, not a standalone probe's (bench/native/macos/box_probe.c measures the raw interfaces).
//!
//!   macos_box [passes]           both measurements, `passes` times each (default 3)
//!
//! The record is `bench/native/macos/RESULT.md`; run it on adas-mac-studio-3 and capture its rows.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros,
    clippy::print_stdout,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("SKIP: the macOS box bench is macOS-only");
}

#[cfg(target_os = "macos")]
fn main() {
    macos::main();
}

#[cfg(target_os = "macos")]
mod macos {
    use std::num::NonZeroU64;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use hex_api::{Bounds, BoxId, BoxSpec, ClassSpec, Program, Runtime, Stdio};
    use hex_runtime::macos::MacRuntime;
    use hex_runtime::macos::sys;

    const MIB: u64 = 1 << 20;
    const GIB: u64 = 1 << 30;
    const BOUND: u64 = 256 * MIB;

    /// The hex binary whose `selftest-box` acts are the box's members: `HEX_BIN`, else the `hex` beside
    /// this bench's own executable (cargo puts both under the same profile directory).
    fn hex_binary() -> PathBuf {
        if let Some(p) = std::env::var_os("HEX_BIN") {
            return PathBuf::from(p);
        }
        let me = std::env::current_exe().expect("current exe");
        // target/<profile>/deps/macos_box-<hash> -> target/<profile>/hex
        let hex = me
            .parent()
            .and_then(|deps| deps.parent())
            .map(|profile| profile.join("hex"))
            .expect("a profile directory above the bench");
        assert!(
            hex.exists(),
            "no hex binary at {} (set HEX_BIN)",
            hex.display()
        );
        hex
    }

    /// A runtime reckoning 256 GiB/s (RESULT.md) and a high process rate, so the bound is read at its
    /// 250 µs floor near it.
    fn runtime() -> MacRuntime {
        MacRuntime::new(
            NonZeroU64::new(256 * GIB).unwrap(),
            NonZeroU64::new(1_000_000).unwrap(),
        )
        .expect("a macOS runtime")
    }

    fn member(hex: &Path, act: &[&str]) -> Program {
        let mut args = vec![
            hex.to_string_lossy().into_owned(),
            "selftest-box".to_owned(),
        ];
        args.extend(act.iter().map(|a| (*a).to_owned()));
        Program::of(&args.iter().map(String::as_str).collect::<Vec<_>>())
    }

    /// One overshoot run: a 256 MiB box with `members` hogs allocating at full speed; returns how far past
    /// the bound the box's footprint reached when the watcher ended it, in bytes.
    fn overshoot(hex: &Path, members: usize) -> u64 {
        let rt = runtime();
        let id = BoxId::new(&format!("mac-bench-over-{members}")).unwrap();
        let _ = rt.delete(&id);
        rt.create(BoxSpec {
            id: id.clone(),
            bounds: Bounds {
                memory_max: Some(BOUND),
                ..Bounds::default()
            },
            class: ClassSpec::Process,
        })
        .expect("create");
        // Each member allocates far past the whole bound, so the box's summed footprint races up until the
        // watcher kills it; the members never finish allocating.
        let per = 4 * GIB;
        let mut procs = Vec::new();
        for _ in 0..members {
            procs.push(
                rt.exec(&id, member(hex, &["hog", &per.to_string()]), Stdio::null())
                    .expect("exec hog"),
            );
        }
        // Poll usage until the watcher reports it ended the box for memory, then read the peak it saw.
        let deadline = Instant::now() + Duration::from_secs(30);
        let peak = loop {
            let u = rt.usage(&id).expect("usage");
            if u.oom_kills >= 1 {
                break u.memory_peak_bytes;
            }
            assert!(Instant::now() < deadline, "the box never ended for memory");
            std::thread::yield_now();
        };
        rt.delete(&id).expect("delete");
        drop(procs);
        peak.saturating_sub(BOUND)
    }

    /// The watcher's CPU for a box held at 95% of its bound for `secs` seconds: the bench process's own CPU
    /// over that window (nothing else in it runs — the main thread parks), as a fraction of one core.
    fn watcher_cpu(hex: &Path, secs: u64) -> f64 {
        let rt = runtime();
        let id = BoxId::new("mac-bench-hold").unwrap();
        let _ = rt.delete(&id);
        rt.create(BoxSpec {
            id: id.clone(),
            bounds: Bounds {
                memory_max: Some(BOUND),
                ..Bounds::default()
            },
            class: ClassSpec::Process,
        })
        .expect("create");
        let hold = BOUND / 100 * 95;
        let millis = secs * 1000 + 4000;
        let _p = rt
            .exec(
                &id,
                member(hex, &["hold", &hold.to_string(), &millis.to_string()]),
                Stdio::null(),
            )
            .expect("exec hold");
        // Wait for it to reach ~90% of the bound, so the watcher is reading at its floor; then measure with
        // the main thread parked so the only CPU the process spends is the watcher's.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let u = rt.usage(&id).expect("usage");
            if u.memory_bytes >= BOUND / 100 * 90 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the hold member never reached 90%"
            );
            std::thread::yield_now();
        }
        let c0 = sys::process_cpu().expect("process cpu");
        std::thread::park_timeout(Duration::from_secs(secs));
        let cpu = sys::process_cpu().expect("process cpu").saturating_sub(c0);
        rt.delete(&id).expect("delete");
        cpu.as_secs_f64() / secs as f64 * 100.0
    }

    pub fn main() {
        let passes: usize = std::env::args_os()
            .nth(1)
            .and_then(|a| a.into_string().ok())
            .filter(|a| a != "--bench")
            .and_then(|a| a.parse().ok())
            .unwrap_or(3);
        let hex = hex_binary();
        let counts = [1usize, 4, 8, 14, 28];
        println!("macos_box bound_mib={} passes={passes}", BOUND / MIB);
        for pass in 1..=passes {
            for &n in &counts {
                let over = overshoot(&hex, n);
                println!(
                    "pass={pass} members={n} overshoot_mib={:.1} load1={:.2}",
                    over as f64 / MIB as f64,
                    sys::loadavg().unwrap_or(-1.0),
                );
            }
        }
        for pass in 1..=passes {
            let pct = watcher_cpu(&hex, 2);
            println!(
                "pass={pass} hold_seconds=2 watcher_cpu_pct_of_core={pct:.2} load1={:.2}",
                sys::loadavg().unwrap_or(-1.0)
            );
        }
    }
}
