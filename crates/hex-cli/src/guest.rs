//! `hex guest`: what runs in a Shards guest (docs/design/shards-guest.md §3 and §4).
//!
//!   hex guest init [--state-disk <device>] [--shares <n>]
//!   hex guest relay --node <socket> (--vsock | --unix <dir>) [--staged <dir>]
//!
//! `init` is the guest's PID 1, the `hex` binary itself as its initramfs's `/init`, the kernel's command
//! line ending `-- guest init`. It mounts what a Linux node needs (`/proc`, `/sys`, cgroup2, `/dev`, a
//! tmpfs `/run` and `/tmp`, and the node's state: the ext4 `--state-disk` where given, else a tmpfs),
//! starts `hex node` and then `hex guest relay --vsock`, says `hex guest ready` on the console, and reaps
//! every process that ends, as PID 1 must. With `--shares <n>` it mounts the host's shares, Shards' virtio-fs
//! tags `shards0` to `shards<n-1>`, at `/run/hex/shares/<i>` (docs/design/shards-guest.md §4a). When the node or the relay ends, it says why and powers the
//! guest off, so the host sees the guest end.
//!
//! With `--vsock` it listens on the guest's vsock ports ([`hex_core::stream::CONTROL_PORT`],
//! [`hex_core::stream::STDIO_PORT`] and [`hex_core::stream::files::FILES_PORT`], any CID); with `--unix` on
//! `control.sock`, `stdio.sock` and `files.sock` in `<dir>`, staging under `--staged` (default
//! `/run/hex/staged`, made if absent),
//! which tests dial as the host would. Once both listen it says `ready` on stdout, and it serves until a
//! listener fails, when it says why on stderr and exits 1.

use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hex_client::guest::Duplex;
use hex_core::stream;
use hex_node::relay::{LIMITS, Listen, Relay, STAGED};
use socket2::{Domain, SockAddr, Socket, Type};

/// Runs `hex guest`'s arguments (everything after `guest`); returns the exit status.
pub fn main(args: &[String]) -> u8 {
    let run = match args.first().map(String::as_str) {
        Some("init") => init(args.get(1..).unwrap_or_default()),
        _ => relay(args),
    };
    match run {
        Ok(()) => 0,
        Err((status, why)) => {
            let _ = std::io::stderr()
                .lock()
                .write_all(format!("hex guest: {why}\n").as_bytes());
            status
        }
    }
}

const USAGE: &str = "usage: hex guest init [--state-disk <device>] [--shares <n>]\n       hex guest relay --node <socket> (--vsock | --unix <dir>) [--staged <dir>]";

/// Where the relay listens.
enum On {
    Vsock,
    Unix(PathBuf),
}

/// What `hex guest relay` was given.
struct Args {
    node: PathBuf,
    on: On,
    staged: PathBuf,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut rest = args.iter();
    if rest.next().map(String::as_str) != Some("relay") {
        return Err(USAGE.to_owned());
    }
    let (mut node, mut on, mut staged) = (None, None, None);
    while let Some(flag) = rest.next() {
        let mut value = || {
            rest.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value\n{USAGE}"))
        };
        let given = match flag.as_str() {
            "--node" => node.replace(PathBuf::from(value()?)).is_some(),
            "--vsock" => on.replace(On::Vsock).is_some(),
            "--unix" => on.replace(On::Unix(PathBuf::from(value()?))).is_some(),
            "--staged" => staged.replace(PathBuf::from(value()?)).is_some(),
            other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
        };
        if given {
            return Err(format!(
                "{flag} is given twice, or with another place\n{USAGE}"
            ));
        }
    }
    Ok(Args {
        node: node.ok_or_else(|| format!("--node is needed\n{USAGE}"))?,
        on: on.ok_or_else(|| format!("--vsock or --unix is needed\n{USAGE}"))?,
        staged: staged.unwrap_or_else(|| PathBuf::from(STAGED)),
    })
}

fn relay(args: &[String]) -> Result<(), (u8, String)> {
    let Args { node, on, staged } = parse(args).map_err(|why| (2, why))?;
    std::fs::create_dir_all(&staged)
        .map_err(|e| (1, format!("making {}: {e}", staged.display())))?;
    type Three = (Box<dyn Listen>, Box<dyn Listen>, Box<dyn Listen>);
    let (control, stdio, files): Three = match &on {
        On::Vsock => (
            Box::new(vsock(stream::CONTROL_PORT).map_err(|e| (1, e))?),
            Box::new(vsock(stream::STDIO_PORT).map_err(|e| (1, e))?),
            Box::new(vsock(stream::files::FILES_PORT).map_err(|e| (1, e))?),
        ),
        On::Unix(dir) => (
            Box::new(unix(&dir.join("control.sock")).map_err(|e| (1, e))?),
            Box::new(unix(&dir.join("stdio.sock")).map_err(|e| (1, e))?),
            Box::new(unix(&dir.join("files.sock")).map_err(|e| (1, e))?),
        ),
    };
    let mut out = std::io::stdout().lock();
    out.write_all(b"ready\n")
        .and_then(|()| out.flush())
        .map_err(|e| (1, format!("saying it is ready: {e}")))?;
    drop(out);
    Relay::new(&node, &staged, LIMITS)
        .serve(control, stdio, files)
        .map_err(|why| (1, why.to_string()))
}

fn unix(path: &Path) -> Result<UnixListener, String> {
    UnixListener::bind(path).map_err(|e| format!("listening on {}: {e}", path.display()))
}

/// The guest's listener on vsock `port`, from any CID (the host's is the only other on its device).
fn vsock(port: u32) -> Result<VsockListener, String> {
    let fail = |e: io::Error| format!("listening on vsock port {port:#x}: {e}");
    let socket = Socket::new(Domain::VSOCK, Type::STREAM.cloexec(), None).map_err(fail)?;
    // VMADDR_CID_ANY (vsock(7)).
    socket
        .bind(&SockAddr::vsock(u32::MAX, port))
        .map_err(fail)?;
    socket.listen(128).map_err(fail)?;
    Ok(VsockListener(socket))
}

struct VsockListener(Socket);

impl Listen for VsockListener {
    fn accept(&self) -> io::Result<Box<dyn Duplex>> {
        Ok(Box::new(Vsock(self.0.accept()?.0)))
    }
}

/// A vsock connection, as the relay's connections are.
struct Vsock(Socket);

impl Read for Vsock {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for Vsock {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl Duplex for Vsock {
    fn try_clone(&self) -> io::Result<Box<dyn Duplex>> {
        Ok(Box::new(Self(self.0.try_clone()?)))
    }

    fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        self.0.shutdown(how)
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.0.set_write_timeout(timeout)
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.0.set_read_timeout(timeout)
    }
}

/// Where the guest keeps the node's socket, state and boxes.
const RUN: &str = "/run/hex";
const STATE: &str = "/var/lib/hex";
const BOX_ROOT: &str = "/sys/fs/cgroup/hex";
/// The controllers the node's boxes are bounded by (`docs/design/runtime.md`), enabled for the root's
/// children.
const CONTROLLERS: &str = "+cpu +io +memory +pids";

fn init(args: &[String]) -> Result<(), (u8, String)> {
    let (mut state_disk, mut shares) = (None, None);
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().ok_or_else(|| (2, USAGE.to_owned()))?;
        let given = match flag.as_str() {
            "--state-disk" => state_disk.replace(PathBuf::from(value)).is_some(),
            "--shares" => shares
                .replace(value.parse::<u32>().map_err(|e| (2, format!("--shares {value:?}: {e}\n{USAGE}")))?)
                .is_some(),
            _ => return Err((2, USAGE.to_owned())),
        };
        if given {
            return Err((2, format!("{flag} is given twice\n{USAGE}")));
        }
    }
    let fail = |why: String| (1, why);
    mount_all(state_disk.as_deref()).map_err(fail)?;
    mount_shares(shares.unwrap_or(0)).map_err(fail)?;
    std::fs::write("/sys/fs/cgroup/cgroup.subtree_control", CONTROLLERS).map_err(|e| {
        fail(format!(
            "enabling the cgroup controllers {CONTROLLERS}: {e}"
        ))
    })?;
    let exe = std::env::current_exe().map_err(|e| fail(format!("finding /init: {e}")))?;
    let socket = Path::new(RUN).join("node.sock");
    let state = Path::new(STATE).join("state");
    let node = start(
        &exe,
        &[
            "node".as_ref(),
            "--socket".as_ref(),
            socket.as_os_str(),
            "--state".as_ref(),
            state.as_os_str(),
            "--box-root".as_ref(),
            BOX_ROOT.as_ref(),
        ],
    )
    .map_err(fail)?;
    let relay = start(
        &exe,
        &[
            "guest".as_ref(),
            "relay".as_ref(),
            "--node".as_ref(),
            socket.as_os_str(),
            "--vsock".as_ref(),
        ],
    )
    .map_err(fail)?;
    let mut out = std::io::stdout().lock();
    let _ = out
        .write_all(b"hex guest ready\n")
        .and_then(|()| out.flush());
    drop(out);
    let why = reap(node, relay);
    let _ = std::io::stderr()
        .lock()
        .write_all(format!("hex guest: {why}; powering off\n").as_bytes());
    rustix::system::reboot(rustix::system::RebootCommand::PowerOff)
        .map_err(|e| fail(format!("powering off: {e}")))
}

/// Mounts what the node needs, each on a directory made for it.
fn mount_all(state_disk: Option<&Path>) -> Result<(), String> {
    use rustix::mount::{MountFlags, mount};
    let quiet = MountFlags::NOSUID | MountFlags::NOEXEC | MountFlags::NODEV;
    let state_fs = if state_disk.is_some() {
        "ext4"
    } else {
        "tmpfs"
    };
    let state_source = state_disk.unwrap_or(Path::new("tmpfs"));
    let mounts: [(&Path, &str, &str, MountFlags); 7] = [
        (Path::new("proc"), "/proc", "proc", quiet),
        (Path::new("sysfs"), "/sys", "sysfs", quiet),
        (Path::new("cgroup2"), "/sys/fs/cgroup", "cgroup2", quiet),
        (
            Path::new("devtmpfs"),
            "/dev",
            "devtmpfs",
            MountFlags::NOSUID,
        ),
        (
            Path::new("tmpfs"),
            "/run",
            "tmpfs",
            MountFlags::NOSUID | MountFlags::NODEV,
        ),
        (
            Path::new("tmpfs"),
            "/tmp",
            "tmpfs",
            MountFlags::NOSUID | MountFlags::NODEV,
        ),
        (
            state_source,
            STATE,
            state_fs,
            MountFlags::NOSUID | MountFlags::NODEV,
        ),
    ];
    for (source, target, fs, flags) in mounts {
        std::fs::create_dir_all(target).map_err(|e| format!("making {target}: {e}"))?;
        mount(source, target, fs, flags, None)
            .map_err(|e| format!("mounting {fs} on {target}: {e}"))?;
    }
    for dir in [RUN, BOX_ROOT] {
        std::fs::create_dir_all(dir).map_err(|e| format!("making {dir}: {e}"))?;
    }
    Ok(())
}

/// Where the host's shares are mounted, `<i>` under it for Shards' tag `shards<i>`.
const SHARES: &str = "/run/hex/shares";
/// The most shares a guest mounts: the host's own bound on the roots a node shares
/// (`hex_client::guest::shares::MAX_SHARES`, its default lists five on macOS and two on Windows), so the
/// loop over the command line's number is bounded (CLAUDE.md §2).
const MAX_SHARES: u32 = 64;

/// Mounts each of the host's `n` shares, virtio-fs tag `shards<i>`, at `/run/hex/shares/<i>`.
fn mount_shares(n: u32) -> Result<(), String> {
    use rustix::mount::{MountFlags, mount};
    if n > MAX_SHARES {
        return Err(format!("--shares {n} is over the {MAX_SHARES} a guest mounts"));
    }
    for i in 0..n {
        let target = Path::new(SHARES).join(i.to_string());
        std::fs::create_dir_all(&target)
            .map_err(|e| format!("making {}: {e}", target.display()))?;
        let tag = format!("shards{i}");
        mount(tag.as_str(), &target, "virtiofs", MountFlags::NODEV, None)
            .map_err(|e| format!("mounting share {tag} on {}: {e}", target.display()))?;
    }
    Ok(())
}

/// Starts `/init` with `args` and waits for its `ready` line.
fn start(exe: &Path, args: &[&std::ffi::OsStr]) -> Result<u32, String> {
    let what = args
        .iter()
        .take(2)
        .map(|a| a.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    let mut child = std::process::Command::new(exe)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("starting hex {what}: {e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("hex {what}'s stdout wasn't kept"))?;
    let mut line = String::new();
    io::BufRead::read_line(&mut io::BufReader::new(stdout), &mut line)
        .map_err(|e| format!("reading hex {what}'s first line: {e}"))?;
    if !line.starts_with("ready") {
        return Err(format!("hex {what} said {line:?}, not ready"));
    }
    Ok(child.id())
}

/// Reaps every process that ends until the node or the relay does, and says which and how.
fn reap(node: u32, relay: u32) -> String {
    use rustix::process::{WaitOptions, wait};
    loop {
        match wait(WaitOptions::empty()) {
            Ok(Some((pid, status))) => {
                let pid = pid.as_raw_nonzero().get().unsigned_abs();
                let name = if pid == node {
                    "the node agent"
                } else if pid == relay {
                    "the relay"
                } else {
                    continue;
                };
                return format!("{name} ended ({status:?})");
            }
            Ok(None) => {}
            Err(rustix::io::Errno::INTR) => {}
            Err(e) => return format!("waiting for the guest's processes: {e}"),
        }
    }
}
