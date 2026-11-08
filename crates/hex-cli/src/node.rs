//! `hex node`: the node agent as a process of its own, serving the `Runtime` seam on its local socket
//! to callers that don't link Hex (decision 3 of docs/design/runtime-architecture.md):
//!
//!   hex node --socket <path> --state <dir> [--box-root <cgroup dir>]
//!
//! It opens the node, its boxes in cgroups under `--box-root` (else in a subtree beside its own
//! cgroup), their records under `--state` and their containers' state on tmpfs under the runtime
//! directory (`/run/hex-node` as root, `$XDG_RUNTIME_DIR/hex-node` rootless; decision 1), takes back
//! or cleans up what an agent before it left, saying on stderr each box it removed and why
//! (T-T7.1.2), raises its soft descriptor limit to the hard one, says on stderr whether it parks procs with
//! its inits (decision 8, parked procs: a root node's, never a rootless one's), binds the
//! socket, says `ready <path>` on stdout, and serves until it is signalled; if serving fails it says why on
//! stderr and exits 1. On SIGTERM it writes one line of its parked procs' counts on stderr (made, hits, misses,
//! dry, capped, alive) and exits 143 with nothing else done: its boxes, connections and socket's file are left as
//! a SIGKILL leaves them, for the next agent's recovery. A socket an ended agent left at the path is replaced; one a live agent serves is
//! refused. A root node whose network-namespace pool was refused for a capability it lacks says so on stderr as it
//! opens (decision 4). A debug build also takes `--crash-at <cgroup-made|bundle-rendered|record-first-write>`,
//! which arms the node's crash point there (`hex_node::CrashPoint`) for T-T7.1.2's crash test: from
//! the agent's own arguments alone, so no container can arm it.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use hex_api::Refusal;
use hex_core::netns::Counts;
#[cfg(debug_assertions)]
use hex_node::CrashPoint;
use hex_node::Node;
use hex_node::serve::{Server, limits};
use rustix::process::Resource;

/// Runs `hex node`'s arguments (everything after `node`); returns the exit status.
pub fn main(args: &[String]) -> u8 {
    if args == ["--descriptors"] {
        return descriptors();
    }
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

const USAGE: &str = "usage: hex node --socket <path> --state <dir> [--box-root <cgroup dir>] | hex node --descriptors";

/// `hex node --descriptors`: what a node of this process's owner needs of its soft descriptor limit on this host, at a
/// first start's bounds, its own and every kept namespace's files (`FilesBound::needed`), said as `own N files M need
/// T`: what an installer gives the node's service and the install lab checks, never a figure copied from one host's
/// (goal 6 step 6's HEX-INST-5). What this process holds now stands for what the node holds beside its bounds.
fn descriptors() -> u8 {
    use hex_node::netns::{FilesBound, Pools, open_descriptors};
    let needed = hex_node::sizes::Sizes::now().and_then(|sizes| {
        let pools = if rustix::process::geteuid().is_root() {
            Pools::root(sizes)
        } else {
            Pools::rootless(sizes)
        };
        let limit = rustix::process::getrlimit(Resource::Nofile);
        Ok(FilesBound::needed(pools, open_descriptors(&limit)?))
    });
    match needed {
        Ok(n) => {
            let line = format!("own {} files {} need {}\n", n.own, n.files, n.total());
            match std::io::stdout().lock().write_all(line.as_bytes()) {
                Ok(()) => 0,
                Err(_) => 1,
            }
        }
        Err(e) => {
            let _ = std::io::stderr()
                .lock()
                .write_all(format!("hex node: {e}\n").as_bytes());
            1
        }
    }
}

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
    // The waits census, where the probe asks for it (bench/competitive/probes/waits_census.md); nothing otherwise.
    hex_node::census::install_from_env().map_err(failed)?;
    // SIGTERM is read from a signalfd, blocked before the node starts a thread so every thread inherits the block;
    // each process the node starts has its mask emptied before it execs (the seeds' and execs' `default_signals`,
    // std's Command), so none inherits it.
    let term =
        hex_runtime::linux::oci::sys::forward_signals(&[rustix::process::Signal::TERM.as_raw()])
            .map_err(|e| (1, format!("reading SIGTERM: {e}")))?;
    raise_descriptor_limit().map_err(failed)?;
    // The state's disk is what the boxes' io bounds apply to, so it exists before the node looks.
    std::fs::create_dir_all(&args.state)
        .map_err(|e| (1, format!("making {}: {e}", args.state.display())))?;
    let node = match &args.box_root {
        Some(root) => Node::open(root, &args.state, &args.state),
        None => Node::beside_self(&args.state, &args.state),
    }
    .map_err(failed)?;
    // What a label whose module the host's kernel doesn't run does: its owner's word, else skipped with a
    // warning.
    let node = node
        .with_lsm(hex_runtime::linux::oci::lsm::Policy::from_env().map_err(|e| failed(e.into()))?);
    #[cfg(debug_assertions)]
    let node = match args.crash_at {
        Some(point) => node.with_crash_point(point),
        None => node,
    };
    if let Some(line) = refused_pool(&node.netns().map_err(failed)?) {
        let _ = std::io::stderr().lock().write_all(line.as_bytes());
    }
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
        // The kept namespaces' held files fit in the descriptor limit the node was given, never raised:
        // say how many may hold them, so a node at a default limit isn't read as holding any.
        if let Ok(Some(kept)) = node.netns_reuse()
            && let Some(files) = kept.files_bound
        {
            let soft = files
                .soft
                .map_or_else(|| "unlimited".to_owned(), |n| n.to_string());
            let _ = writeln!(
                err,
                "hex node: kept network namespaces hold their /proc/net files for at most {} of them: \
                 the soft descriptor limit is {soft}, the node's own at its bounds {}{}",
                files.namespaces,
                files.own,
                if files.namespaces == 0 {
                    "; each check enters its namespace instead, counted (a soft limit past the node's own \
                     lets them hold files)"
                } else {
                    ""
                }
            );
        }
    }
    {
        let said = if node.parked_procs() {
            format!("on, at most {} alive", node.sizes().procs_alive())
        } else {
            "off: a rootless node parks none".to_owned()
        };
        let _ = writeln!(std::io::stderr().lock(), "hex node: parked procs: {said}");
    }
    let node = Arc::new(node);
    let (ended, end) = std::sync::mpsc::channel();
    end_on_sigterm(term, Arc::clone(&node), ended.clone()).map_err(failed)?;
    let sizes = node.sizes();
    let mut server =
        Server::bind(&args.socket, node, limits(&sizes), sizes.workers).map_err(failed)?;
    let mut out = std::io::stdout().lock();
    out.write_all(format!("ready {}\n", args.socket.display()).as_bytes())
        .and_then(|()| out.flush())
        .map_err(|e| (1, format!("saying it is ready: {e}")))?;
    drop(out);
    std::thread::Builder::new()
        .name("hex-serve".to_owned())
        .spawn(move || {
            let why = server
                .wait()
                .unwrap_or_else(|| "its socket stopped".to_owned());
            let _ = ended.send((1, why));
        })
        .map_err(|e| (1, format!("starting the server's thread: {e}")))?;
    Err(end
        .recv()
        .unwrap_or_else(|_| (1, "its ending threads stopped unsaid".to_owned())))
}

/// What ends the node: a thread reading SIGTERM sends its exit status and last words here, and the thread that owns
/// the server sends why it stopped. [`serve`] returns the first that comes, so `main` exits with the server still
/// owned by a thread blocked in its wait: no destructor runs, and the server, its connections and its boxes are left
/// as a SIGKILL leaves them, for the next agent's recovery (T-T7.1.2).
type Ended = std::sync::mpsc::Sender<(u8, String)>;

/// On SIGTERM, from a thread of its own: one line of the node's parked procs' counts, status 143 (128 + SIGTERM, as a
/// shell reports a SIGTERM's end). The signalfd reads only SIGTERM; a read that fails ends the node saying why. A count
/// the node can't read is said as unread, never left out.
fn end_on_sigterm(
    term: hex_runtime::linux::oci::sys::ForwardedSignals,
    node: Arc<Node>,
    ended: Ended,
) -> Result<(), Refusal> {
    std::thread::Builder::new()
        .name("hex-sigterm".to_owned())
        .spawn(move || {
            let end = match term.next() {
                Ok(_) => (
                    143,
                    format!("parked procs at exit: {}", parked_counts(&node)),
                ),
                Err(e) => (1, format!("reading SIGTERM: {e}")),
            };
            let _ = ended.send(end);
        })
        .map(drop)
        .map_err(|e| Refusal::os("starting the SIGTERM thread", e))
}

fn parked_counts(node: &Node) -> String {
    match node.cgroup_reuse() {
        Ok(c) => {
            let p = c.parked;
            format!(
                "made {}, hits {}, misses {}, dry {}, capped {}, alive {}",
                p.procs_made, p.proc_hits, p.proc_misses, p.proc_dry, p.procs_capped, p.procs_alive
            )
        }
        Err(e) => format!("unread: {e}"),
    }
}

/// What `hex node` says on stderr when its network-namespace pool was refused: what the node lacks,
/// and that its creates make their namespaces on the path, which its counts say as `refused_on_path`.
fn refused_pool(counts: &Counts) -> Option<String> {
    counts.refused.map(|missing| {
        format!(
            "hex node: network-namespace pool refused: this node lacks {missing}, which making one \
             ahead takes; each create makes its container's on its path (counted as refused_on_path)\n"
        )
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(more: &[&str]) -> Vec<String> {
        ["--socket", "/s", "--state", "/d"]
            .iter()
            .chain(more)
            .map(|s| (*s).to_owned())
            .collect()
    }

    /// Parked procs have no off switch (decision 8, parked procs; Ada's 2026-11-06 rule, once
    /// bench/competitive/probes/parked_procs_ab.md decided them on; AC-T7.1.3). Do: parse `--parked-procs` with `on`
    /// and with `off`. Expect: each refused as an unknown argument, naming it.
    #[test]
    fn parked_procs_have_no_flag() {
        for value in ["on", "off"] {
            let why = parse(&args(&["--parked-procs", value])).err().unwrap();
            assert!(
                why.starts_with("unknown argument \"--parked-procs\""),
                "{value}: {why}"
            );
        }
    }

    use hex_core::netns::Missing;

    /// HEX-NETNS-13: a node whose pool was refused says so on stderr, naming what it lacks; one whose
    /// pool wasn't says nothing. Do: render both. Expect: the line naming CAP_SYS_ADMIN, and none.
    #[test]
    fn a_refused_pool_is_said_by_name() {
        let refused = Counts {
            refused: Missing::of(false, true),
            ..Counts::default()
        };
        assert_eq!(
            refused_pool(&refused).unwrap(),
            "hex node: network-namespace pool refused: this node lacks CAP_SYS_ADMIN, which making \
             one ahead takes; each create makes its container's on its path (counted as \
             refused_on_path)\n"
        );
        assert_eq!(refused_pool(&Counts::default()), None);
    }
}
