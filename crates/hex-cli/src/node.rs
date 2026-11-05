//! `hex node`: the node agent as a process of its own, serving the `Runtime` seam on its local socket
//! to callers that don't link Hex (decision 3 of docs/design/runtime-architecture.md):
//!
//!   hex node --socket <path> --state <dir> [--box-root <cgroup dir>]
//!
//! It opens the node, its boxes in cgroups under `--box-root` (else in a subtree beside its own
//! cgroup), their records under `--state` and their containers' state on tmpfs under the runtime
//! directory (`/run/hex-node` as root, `$XDG_RUNTIME_DIR/hex-node` rootless; decision 1), takes back
//! or cleans up what an agent before it left, saying on stderr each box it removed and why
//! (T-T7.1.2), raises its soft descriptor limit to the hard one, binds the socket, says
//! `ready <path>` on stdout, and serves until it is signalled; if serving fails it says why on stderr
//! and exits 1. A socket an ended agent left at the path is replaced; one a live agent serves is
//! refused. A debug build also takes `--crash-at <cgroup-made|bundle-rendered|record-first-write>`,
//! which arms the node's crash point there (`hex_node::CrashPoint`) for T-T7.1.2's crash test: from
//! the agent's own arguments alone, so no container can arm it.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use hex_api::Refusal;
#[cfg(debug_assertions)]
use hex_node::CrashPoint;
use hex_node::Node;
use hex_node::serve::{LIMITS, Server, WORKERS};
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
    #[cfg(debug_assertions)]
    crash_at: Option<CrashPoint>,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let (mut socket, mut state, mut box_root) = (None, None, None);
    #[cfg(debug_assertions)]
    let mut crash_at = None;
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        #[cfg(debug_assertions)]
        if flag == "--crash-at" {
            let name = rest
                .next()
                .ok_or_else(|| format!("{flag} needs a value\n{USAGE}"))?;
            let point = CrashPoint::named(name)
                .ok_or_else(|| format!("{flag} names no crash point: {name:?}"))?;
            if crash_at.replace(point).is_some() {
                return Err(format!("{flag} is given twice\n{USAGE}"));
            }
            continue;
        }
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
        #[cfg(debug_assertions)]
        crash_at,
    })
}

fn serve(args: &[String]) -> Result<(), (u8, String)> {
    let args = parse(args).map_err(|why| (2, why))?;
    let failed = |e: Refusal| (1, e.to_string());
    raise_descriptor_limit().map_err(failed)?;
    // The state's disk is what the boxes' io bounds apply to, so it exists before the node looks.
    std::fs::create_dir_all(&args.state)
        .map_err(|e| (1, format!("making {}: {e}", args.state.display())))?;
    let node = match &args.box_root {
        Some(root) => Node::open(root, &args.state, &args.state),
        None => Node::beside_self(&args.state, &args.state),
    }
    .map_err(failed)?;
    // The way a pruned create's mount namespace is made, where its owner asks for one (decision 6's
    // update); unset, the node's default.
    let node = match std::env::var(hex_runtime::linux::oci::mntns::ENV_WAY) {
        Ok(way) => node.with_mount_namespace(&way),
        Err(_) => node,
    };
    #[cfg(debug_assertions)]
    let node = match args.crash_at {
        Some(point) => node.with_crash_point(point),
        None => node,
    };
    // Recovery removes a box it can't take back; say which and why, so nothing is dropped unseen
    // (CLAUDE.md §3), before the socket opens.
    let recovered = node.recover().map_err(failed)?;
    {
        let mut err = std::io::stderr().lock();
        for n in &recovered.slots_removed {
            let _ = writeln!(
                err,
                "hex node: recovered: removed idle cgroup {}: no box's record named it",
                hex_core::reuse::name(*n)
            );
        }
        for (id, why) in &recovered.removed {
            let _ = writeln!(err, "hex node: recovered: removed {id}: {why}");
        }
    }
    let mut server = Server::bind(&args.socket, Arc::new(node), LIMITS, WORKERS).map_err(failed)?;
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
