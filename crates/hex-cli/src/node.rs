//! `hex node`: the node agent as a process of its own, serving the `Runtime` seam on its local socket
//! to callers that don't link Hex (decision 3 of docs/design/runtime-architecture.md):
//!
//!   hex node --socket <path> --state <dir> [--box-root <cgroup dir>]
//!
//! It opens the node, its boxes in cgroups under `--box-root` (else in a subtree beside its own
//! cgroup) and their records under `--state`, takes back or cleans up what an agent before it left
//! (T-T7.1.2), raises its soft descriptor limit to the hard one, binds the socket, says
//! `ready <path>` on stdout, and serves until it is signalled; if serving fails it says why on stderr
//! and exits 1. A socket an ended agent left at the path is replaced; one a live agent serves is
//! refused.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use hex_api::Refusal;
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
    let node = match &args.box_root {
        Some(root) => Node::open(root, &args.state, &args.state),
        None => Node::beside_self(&args.state, &args.state),
    }
    .map_err(failed)?;
    node.recover().map_err(failed)?;
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
