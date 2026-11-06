//! `hex __box-helper --socket <path> --users <uid,…>`: the macOS box helper daemon (native plan step 2).
//! Run as root — by launchd from the installer, or by hand in the lab — it serves the node over the Unix
//! socket at `--socket`, starting each box's programs as one of the pool `--users` and capping each box's
//! processes (`RLIMIT_NPROC`) at its spec's `pids_max`, else at the platform's per-uid ceiling
//! (`kern.maxprocperuid`, read at start). macOS only; the node reaches it through
//! [`hex_runtime::macos::helper_client`]. It does one job and refuses every other request by name
//! ([`hex_core::helper`]).

use std::io::Write;
use std::path::Path;

use hex_runtime::macos::helper_daemon::Daemon;
use hex_runtime::macos::helper_sys::RealPriv;
use hex_runtime::macos::sys;

/// Runs the daemon until it fails (it serves forever otherwise). Returns a non-zero status on a bad
/// invocation or a socket error.
pub fn main(args: &[String]) -> u8 {
    let mut socket: Option<String> = None;
    let mut users: Vec<u32> = Vec::new();
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--socket" => socket = it.next().cloned(),
            "--users" => {
                users = it.next().map_or_else(Vec::new, |v| {
                    v.split(',').filter_map(|u| u.parse::<u32>().ok()).collect()
                });
            }
            other => {
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "hex __box-helper: unknown argument {other}"
                );
                return 2;
            }
        }
    }
    let Some(socket) = socket else {
        let _ = writeln!(
            std::io::stderr().lock(),
            "hex __box-helper: --socket <path> is required"
        );
        return 2;
    };
    if users.is_empty() {
        let _ = writeln!(
            std::io::stderr().lock(),
            "hex __box-helper: --users <uid,…> is required (the installer's pool)"
        );
        return 2;
    }
    let per_uid = match sys::max_processes_per_uid().map(u32::try_from) {
        Ok(Ok(n)) => n,
        Ok(Err(_)) => u32::MAX,
        Err(e) => {
            let _ = writeln!(
                std::io::stderr().lock(),
                "hex __box-helper: reading kern.maxprocperuid: {e}"
            );
            return 1;
        }
    };
    let ops = match RealPriv::new() {
        Ok(ops) => ops,
        Err(e) => {
            let _ = writeln!(
                std::io::stderr().lock(),
                "hex __box-helper: making the exception port: {e}"
            );
            return 1;
        }
    };
    let daemon = std::sync::Arc::new(Daemon::new(ops, users, per_uid));
    // Rebalance on EXC_RESOURCE crossings on a thread beside the socket (event-driven, no polling).
    let pumper = std::sync::Arc::clone(&daemon);
    std::thread::spawn(move || {
        while let Ok(()) = pumper.pump(500) {}
        let _ = writeln!(
            std::io::stderr().lock(),
            "hex __box-helper: the crossing pump stopped"
        );
    });
    match daemon.listen(Path::new(&socket)) {
        Ok(()) => 0,
        Err(e) => {
            let _ = writeln!(std::io::stderr().lock(), "hex __box-helper: {e}");
            1
        }
    }
}
