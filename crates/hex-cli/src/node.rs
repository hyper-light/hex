//! `hex node`: the node agent as a process of its own, serving the `Runtime` seam on its local socket
//! to callers that don't link Hex (decision 3 of docs/design/runtime-architecture.md):
//!
//!   hex node --socket <path> --state <dir> [--box-root <cgroup dir>]
//!
//! On macOS (native plan step 8b) the node is the macOS runtime's `process` class with its boxes'
//! records kept in `--state` ([`hex_node::recorded::Recorded`]), and, where a Shards guest is configured
//! (`SHARDS_BIN`, `HEX_GUEST_KERNEL`, `HEX_GUEST_INITRD`, ...,
//! [`hex_runtime::guest::GuestConfig::from_env`]), Linux images in that guest, booted before the node
//! accepts a box (HEX-GUEST-39); `--box-root` (cgroups) is Linux's. On Linux:
//!
//! It opens the node, its boxes in cgroups under `--box-root` (else in a subtree beside its own
//! cgroup), their records under `--state` and their containers' state on tmpfs under the runtime
//! directory (`/run/hex-node` as root, `$XDG_RUNTIME_DIR/hex-node` rootless; decision 1), takes back
//! or cleans up what an agent before it left
//! (T-T7.1.2), raises its soft descriptor limit to the hard one, binds the socket, says
//! `ready <path>` on stdout, and serves until it is signalled; if serving fails it says why on stderr
//! and exits 1. A socket an ended agent left at the path is replaced; one a live agent serves is
//! refused.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use hex_api::Refusal;
use hex_node::serve::{Host, LIMITS, Server, WORKERS};
use rustix::process::Resource;

/// Runs `hex node`'s arguments (everything after `node`); returns the exit status.
pub fn main(args: &[String]) -> u8 {
    match serve(args) {
        Ok(()) => 0,
        Err((status, why)) => {
            let _ = std::io::stderr()
                .lock()
                .write_all(format!("hex node: {why}\n").as_bytes());
            status
        }
    }
}

const USAGE: &str = "usage: hex node --socket <path> --state <dir> [--box-root <cgroup dir>]";

struct Args {
    socket: PathBuf,
    state: PathBuf,
    box_root: Option<PathBuf>,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let (mut socket, mut state, mut box_root) = (None, None, None);
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let slot = match flag.as_str() {
            "--socket" => &mut socket,
            "--state" => &mut state,
            "--box-root" => &mut box_root,
            other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
        };
        let value = rest
            .next()
            .ok_or_else(|| format!("{flag} needs a value\n{USAGE}"))?;
        if slot.replace(PathBuf::from(value)).is_some() {
            return Err(format!("{flag} is given twice\n{USAGE}"));
        }
    }
    Ok(Args {
        socket: socket.ok_or_else(|| format!("--socket is needed\n{USAGE}"))?,
        state: state.ok_or_else(|| format!("--state is needed\n{USAGE}"))?,
        box_root,
    })
}

fn serve(args: &[String]) -> Result<(), (u8, String)> {
    let args = parse(args).map_err(|why| (2, why))?;
    let failed = |e: Refusal| (1, e.to_string());
    raise_descriptor_limit().map_err(failed)?;
    // The state's disk is what the boxes' io bounds apply to, so it exists before the node looks.
    std::fs::create_dir_all(&args.state)
        .map_err(|e| (1, format!("making {}: {e}", args.state.display())))?;
    let node = host(&args)?;
    let mut server = Server::bind(&args.socket, node, LIMITS, WORKERS).map_err(failed)?;
    let mut out = std::io::stdout().lock();
    out.write_all(format!("ready {}\n", args.socket.display()).as_bytes())
        .and_then(|()| out.flush())
        .map_err(|e| (1, format!("saying it is ready: {e}")))?;
    drop(out);
    let why = server
        .wait()
        .unwrap_or_else(|| "its socket stopped".to_owned());
    Err((1, why))
}

/// The Linux node: its boxes in cgroups under `--box-root` (else beside its own cgroup), recovered.
#[cfg(target_os = "linux")]
fn host(args: &Args) -> Result<Arc<dyn Host>, (u8, String)> {
    let failed = |e: Refusal| (1, e.to_string());
    let node = match &args.box_root {
        Some(root) => hex_node::Node::open(root, &args.state, &args.state),
        None => hex_node::Node::beside_self(&args.state, &args.state),
    }
    .map_err(failed)?;
    node.recover().map_err(failed)?;
    Ok(Arc::new(node))
}

/// The macOS node: the `process` class, and Linux images in a Shards guest where one is configured.
#[cfg(target_os = "macos")]
fn host(args: &Args) -> Result<Arc<dyn Host>, (u8, String)> {
    use hex_node::recorded::Recorded;
    use hex_runtime::guest::{GuestConfig, GuestRuntime, WithGuest};
    let failed = |e: Refusal| (1, e.to_string());
    if args.box_root.is_some() {
        return Err((
            2,
            format!("--box-root names a cgroup directory, which is Linux's\n{USAGE}"),
        ));
    }
    // The Mac Studio's measured allocation rate (bench/native/macos/RESULT.md), 256 GiB/s, the pace the
    // footprint bound is reckoned against; the process rate set high, so the process bound reads at its
    // 250 us floor, as the class's conformance test does (tests/macos_process_box.rs).
    let rate_bytes =
        std::num::NonZeroU64::new(256 << 30).ok_or((1, "a zero allocation rate".to_owned()))?;
    let rate_processes =
        std::num::NonZeroU64::new(1_000_000).ok_or((1, "a zero process rate".to_owned()))?;
    let process =
        hex_runtime::macos::MacRuntime::new(rate_bytes, rate_processes).map_err(failed)?;
    let guest = [
        hex_runtime::guest::ENV_SHARDS_BIN,
        hex_runtime::guest::ENV_KERNEL,
        hex_runtime::guest::ENV_INITRD,
    ]
    .iter()
    .any(|v| std::env::var_os(v).is_some());
    if !guest {
        let node = Recorded::open(process, &args.state).map_err(failed)?;
        node.recover().map_err(failed)?;
        return Ok(Arc::new(node));
    }
    let config = GuestConfig::from_env().map_err(failed)?;
    let node = WithGuest::new(
        process,
        // `@` is in no box id, so neither the record nor its temporary is read as a box's.
        GuestRuntime::new(Arc::new(shards_api::Shards::new()), config)
            .recorded_at(args.state.join("@guest")),
    )
    .map_err(failed)?;
    let node = Recorded::open(node, &args.state).map_err(failed)?;
    node.recover().map_err(failed)?;
    Ok(Arc::new(node))
}

/// Raises the soft limit on open descriptors to the hard one: each connection and watched program
/// holds descriptors, and a service's soft limit stays 1,024 for `select(2)`'s sake while its hard
/// limit is 524,288 (systemd-system.conf(5), `DefaultLimitNOFILE=`), which a program that needs
/// more raises its soft limit to.
fn raise_descriptor_limit() -> Result<(), Refusal> {
    let limit = rustix::process::getrlimit(Resource::Nofile);
    if limit.current < limit.maximum {
        rustix::process::setrlimit(
            Resource::Nofile,
            rustix::process::Rlimit {
                current: limit.maximum,
                maximum: limit.maximum,
            },
        )
        .map_err(|e| Refusal::os("raising the descriptor limit", e))?;
    }
    Ok(())
}
