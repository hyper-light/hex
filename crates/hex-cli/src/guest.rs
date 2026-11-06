//! `hex guest`: what runs in a Shards guest (docs/design/shards-guest.md §3 and §4).
//!
//!   hex guest relay --node <socket> (--vsock | --unix <dir>) [--staged <dir>]
//!
//! The guest's init is the `hex` binary itself as its initramfs's `/init`: run as PID 1, `hex` is the
//! guest's init whatever words the kernel passes it ([`init_main`]), as Shards appends its own to a
//! command line (` quiet` where no one reads the console, `crates/vmm/src/vm/mod.rs:1007-1018` at
//! `a0cced87`), and words after a `--` are init's. It first moves itself off the kernel's initramfs onto a tmpfs root, as
//! switch_root does, since a container's start pivots its root and pivot_root(2) refuses to leave the
//! initramfs ("EINVAL The current root directory is on the rootfs"). It then mounts what a Linux node
//! needs (`/proc`, `/sys`, cgroup2, `/dev`, a tmpfs `/run` and `/tmp`; the node's state is under `/run`,
//! as the guest has no disk: docs/design/shards-guest.md §4), mounts the Shards shares the kernel's
//! command line names (`hex.shares=rw,ro,…`, §4a) at `/run/hex/shares/<i>`, starts `hex node` and then
//! `hex guest relay --vsock`, says `hex guest ready` on the console, and reaps every process that ends,
//! as PID 1 must. When the node or the relay ends, it says why and powers the guest off, so the host sees
//! the guest end.
//!
//! With `--vsock` it listens on the guest's vsock ports ([`hex_core::stream::CONTROL_PORT`],
//! [`hex_core::stream::STDIO_PORT`] and [`hex_core::stream::files::FILES_PORT`], any CID); with `--unix` on
//! `control.sock`, `stdio.sock` and `files.sock` in `<dir>`, which tests dial as the host would. It stages
//! boxes' roots under `--staged` (default `/run/hex/staged`, made if absent; §4a). Once all listen it says
//! `ready` on stdout, and it serves until a listener fails, when it says why on stderr and exits 1.

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
    match relay(args) {
        Ok(()) => 0,
        Err((status, why)) => {
            let _ = std::io::stderr()
                .lock()
                .write_all(format!("hex guest: {why}\n").as_bytes());
            status
        }
    }
}

const USAGE: &str =
    "usage: hex guest relay --node <socket> (--vsock | --unix <dir>) [--staged <dir>]";

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
const STATE: &str = "/run/hex/state";
const BOX_ROOT: &str = "/sys/fs/cgroup/hex";
/// The controllers the node's boxes are bounded by (`docs/design/runtime.md`), enabled for the root's
/// children.
const CONTROLLERS: &str = "+cpu +io +memory +pids";

/// How the kernel names the initramfs's first process: `rdinit`'s default, passed as its `argv[0]`
/// (init/main.c, `ramdisk_execute_command` and `run_init_process`).
const KERNEL_INIT: &str = "/init";

/// Whether `hex` runs as the guest's init: PID 1, and the kernel's first process, whatever other words it
/// is given. PID 1 alone isn't enough: a container's first program is PID 1 of its own PID namespace, and
/// `/hex selftest-box` or the runtime's `__oci-init` there must run as themselves, so its `argv[0]` tells them
/// apart. A container that names its own program `/init` and is `hex` fails init's first privileged step
/// or stays inside its namespaces: it reaches nothing a container can't.
pub fn runs_as_init(pid: u32, argv0: Option<&std::ffi::OsStr>) -> bool {
    pid == 1 && argv0 == Some(std::ffi::OsStr::new(KERNEL_INIT))
}

/// Runs as the guest's init (§4); returns only if it fails, with the exit status.
pub fn init_main() -> u8 {
    let why = match init() {
        Ok(()) => "powering off returned".to_owned(),
        Err((_, why)) => why,
    };
    let _ = std::io::stderr()
        .lock()
        .write_all(format!("hex guest: {why}\n").as_bytes());
    1
}

fn init() -> Result<(), (u8, String)> {
    let fail = |why: String| (1, why);
    leave_rootfs().map_err(fail)?;
    mount_all().map_err(fail)?;
    mount_shares().map_err(fail)?;
    std::fs::write("/sys/fs/cgroup/cgroup.subtree_control", CONTROLLERS).map_err(|e| {
        fail(format!(
            "enabling the cgroup controllers {CONTROLLERS}: {e}"
        ))
    })?;
    let exe = std::env::current_exe().map_err(|e| fail(format!("finding /init: {e}")))?;
    let socket = Path::new(RUN).join("node.sock");
    let state = Path::new(STATE);
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

/// One file system `init` mounts: its source, target, type and flags.
struct Mount {
    source: &'static str,
    target: &'static str,
    fs: &'static str,
    flags: rustix::mount::MountFlags,
}

/// What `init` mounts, in order.
fn mounts() -> [Mount; 6] {
    use rustix::mount::MountFlags;
    let quiet = MountFlags::NOSUID | MountFlags::NOEXEC | MountFlags::NODEV;
    let mount = |source, target, fs, flags| Mount {
        source,
        target,
        fs,
        flags,
    };
    [
        mount("proc", "/proc", "proc", quiet),
        mount("sysfs", "/sys", "sysfs", quiet),
        mount("cgroup2", "/sys/fs/cgroup", "cgroup2", quiet),
        mount("devtmpfs", "/dev", "devtmpfs", MountFlags::NOSUID),
        mount(
            "tmpfs",
            "/run",
            "tmpfs",
            MountFlags::NOSUID | MountFlags::NODEV,
        ),
        mount(
            "tmpfs",
            "/tmp",
            "tmpfs",
            MountFlags::NOSUID | MountFlags::NODEV,
        ),
    ]
}

/// The most of `/proc/self/mountinfo` read: a fresh guest's lists a handful of mounts.
const MOUNTINFO_BOUND: u64 = 64 * 1024;

/// Where `init` mounts the tmpfs that becomes the root.
const NEW_ROOT: &str = "/hex-root";

/// Moves `init` off the kernel's initramfs, whose `rootfs` pivot_root(2) can't leave, onto a tmpfs
/// mounted over `/` (switch_root's way: `MS_MOVE` and `chroot`), and runs it again from there. Returns
/// only when the root is already not the initramfs.
fn leave_rootfs() -> Result<(), String> {
    use rustix::mount::{MountFlags, UnmountFlags, mount, mount_move, unmount};
    let quiet = MountFlags::NOSUID | MountFlags::NOEXEC | MountFlags::NODEV;
    std::fs::create_dir_all("/proc").map_err(|e| format!("making /proc: {e}"))?;
    mount("proc", "/proc", "proc", quiet, None).map_err(|e| format!("mounting /proc: {e}"))?;
    let mut info = String::new();
    let read = std::fs::File::open("/proc/self/mountinfo")
        .and_then(|f| f.take(MOUNTINFO_BOUND).read_to_string(&mut info));
    unmount("/proc", UnmountFlags::empty()).map_err(|e| format!("unmounting /proc: {e}"))?;
    read.map_err(|e| format!("reading /proc/self/mountinfo: {e}"))?;
    if root_fs(&info) != Some("rootfs") {
        return Ok(());
    }
    std::fs::create_dir_all(NEW_ROOT).map_err(|e| format!("making {NEW_ROOT}: {e}"))?;
    mount(
        "tmpfs",
        NEW_ROOT,
        "tmpfs",
        MountFlags::empty(),
        c"mode=0755",
    )
    .map_err(|e| format!("mounting the root's tmpfs: {e}"))?;
    let init = Path::new(NEW_ROOT).join("init");
    std::fs::copy("/init", &init).map_err(|e| format!("copying /init: {e}"))?;
    // The initramfs is unreachable once the tmpfs covers it, so its copy goes now; this process keeps
    // its pages until the exec below.
    std::fs::remove_file("/init").map_err(|e| format!("removing the initramfs's /init: {e}"))?;
    std::env::set_current_dir(NEW_ROOT).map_err(|e| format!("entering {NEW_ROOT}: {e}"))?;
    mount_move(".", "/").map_err(|e| format!("moving {NEW_ROOT} to /: {e}"))?;
    rustix::process::chroot(".").map_err(|e| format!("entering the new root: {e}"))?;
    std::env::set_current_dir("/").map_err(|e| format!("entering /: {e}"))?;
    let e = std::os::unix::process::CommandExt::exec(&mut std::process::Command::new("/init"));
    Err(format!("running /init from the new root: {e}"))
}

/// The file system mounted at `/` in `mountinfo` (proc(5)): the last mount on `/`, as each covers the
/// one before; `None` where no line names `/`.
fn root_fs(mountinfo: &str) -> Option<&str> {
    mountinfo
        .lines()
        .filter_map(|line| {
            let (mount, fs) = line.split_once(" - ")?;
            (mount.split(' ').nth(4)? == "/").then(|| fs.split(' ').next())?
        })
        .next_back()
}

/// One of Shards' shares (docs/design/shards-guest.md §4a): whether the guest mounts it read-only.
#[derive(Debug, PartialEq, Eq)]
struct Share {
    read_only: bool,
}

/// The most shares a guest mounts: a bound of Hex's own, as the node shares a handful of roots (by
/// default one, `/`); a longer list is refused by name.
const MAX_SHARES: usize = 32;

/// The parameter on the kernel's command line that names the guest's shares: `hex.shares=` and a mode
/// per share, `rw` or `ro`, joined by commas, in the order of their tags (`shards0`, `shards1`, …).
/// Absent, the guest has none.
const SHARES_PARAMETER: &str = "hex.shares=";

/// Where share `i` is mounted: `/run/hex/shares/<i>`.
const SHARES: &str = "/run/hex/shares";

/// The most read of `/proc/cmdline`: the kernel's own bound on a command line is smaller (`COMMAND_LINE_SIZE`,
/// 2048 on x86_64 and arm64).
const CMDLINE_BOUND: u64 = 4096;

/// The shares the kernel's command line names, in tag order. Only the kernel's own parameters count:
/// what follows `--` is init's arguments.
fn shares(cmdline: &str) -> Result<Vec<Share>, String> {
    let mut named = cmdline
        .split_whitespace()
        .take_while(|word| *word != "--")
        .filter_map(|word| word.strip_prefix(SHARES_PARAMETER));
    let Some(modes) = named.next() else {
        return Ok(Vec::new());
    };
    if named.next().is_some() {
        return Err(format!("{SHARES_PARAMETER} is given twice"));
    }
    let mut out = Vec::new();
    for mode in modes.split(',') {
        if out.len() == MAX_SHARES {
            return Err(format!(
                "{SHARES_PARAMETER} names more than {MAX_SHARES} shares"
            ));
        }
        out.push(Share {
            read_only: match mode {
                "rw" => false,
                "ro" => true,
                other => {
                    return Err(format!(
                        "{SHARES_PARAMETER} names a share {other:?}, not rw or ro"
                    ));
                }
            },
        });
    }
    Ok(out)
}

/// Mounts each share the kernel's command line names, as `virtiofs` by its tag, at `/run/hex/shares/<i>`.
fn mount_shares() -> Result<(), String> {
    use rustix::mount::{MountFlags, mount};
    let mut cmdline = String::new();
    std::fs::File::open("/proc/cmdline")
        .and_then(|f| f.take(CMDLINE_BOUND).read_to_string(&mut cmdline))
        .map_err(|e| format!("reading /proc/cmdline: {e}"))?;
    for (i, share) in shares(&cmdline)?.iter().enumerate() {
        let (tag, target) = (format!("shards{i}"), Path::new(SHARES).join(i.to_string()));
        let mut flags = MountFlags::NOSUID | MountFlags::NODEV;
        if share.read_only {
            flags |= MountFlags::RDONLY;
        }
        std::fs::create_dir_all(&target)
            .map_err(|e| format!("making {}: {e}", target.display()))?;
        mount(&tag, &target, "virtiofs", flags, None)
            .map_err(|e| format!("mounting share {tag} on {}: {e}", target.display()))?;
    }
    Ok(())
}

/// Mounts what the node needs, each on a directory made for it.
fn mount_all() -> Result<(), String> {
    for m in mounts() {
        let (target, fs) = (m.target, m.fs);
        std::fs::create_dir_all(target).map_err(|e| format!("making {target}: {e}"))?;
        rustix::mount::mount(m.source, target, fs, m.flags, None)
            .map_err(|e| format!("mounting {fs} on {target}: {e}"))?;
    }
    for dir in [RUN, STATE, BOX_ROOT] {
        std::fs::create_dir_all(dir).map_err(|e| format!("making {dir}: {e}"))?;
    }
    Ok(())
}

/// The most read of a started process's first line: `ready` and its socket's path.
const LINE_BOUND: u64 = 4096;

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
    io::BufRead::read_line(&mut io::BufReader::new(stdout.take(LINE_BOUND)), &mut line)
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

#[cfg(test)]
mod tests {
    use super::*;
    use rustix::mount::MountFlags;

    /// HEX-GUEST-9 (docs/design/shards-guest.md §4, amended 2026-11-06): `init` mounts `/proc`, `/sys`,
    /// cgroup2, `devtmpfs` and a tmpfs `/run`, and the node's state lives on that tmpfs, never a disk;
    /// `/run` may run programs, since the staged bundles under it are containers' roots.
    #[test]
    fn init_mounts_section_4s_file_systems_and_keeps_state_on_tmpfs() {
        let all = mounts();
        for (target, fs) in [
            ("/proc", "proc"),
            ("/sys", "sysfs"),
            ("/sys/fs/cgroup", "cgroup2"),
            ("/dev", "devtmpfs"),
            ("/run", "tmpfs"),
        ] {
            assert!(
                all.iter().any(|m| m.target == target && m.fs == fs),
                "{fs} on {target} is missing"
            );
        }
        let run = all.iter().find(|m| m.target == "/run").unwrap();
        assert!(!run.flags.contains(MountFlags::NOEXEC));
        for m in &all {
            assert!(
                ["proc", "sysfs", "cgroup2", "devtmpfs", "tmpfs"].contains(&m.fs),
                "{} on {} is a disk's file system",
                m.fs,
                m.target
            );
        }
        assert!(Path::new(STATE).starts_with(RUN), "the state is at {STATE}");
    }

    /// HEX-GUEST-10 (§4): `init` reads the root's file system from mountinfo, so it leaves the
    /// initramfs (`rootfs`) once and never again; hostile or short lines name no root.
    #[test]
    fn the_root_file_system_is_read_from_mountinfo() {
        let initramfs = "1 1 0:2 / / rw - rootfs rootfs rw,size=1000k\n";
        assert_eq!(root_fs(initramfs), Some("rootfs"));
        let moved = "1 1 0:2 / / rw - rootfs rootfs rw\n\
                     20 1 0:20 / / rw,relatime - tmpfs tmpfs rw,mode=755\n\
                     21 20 0:21 / /proc rw - proc proc rw\n";
        assert_eq!(root_fs(moved), Some("tmpfs"));
        for hostile in [
            "",
            "\n\n",
            "1 1 0:2 / /\n",
            "1 1 0:2 / /run rw - tmpfs tmpfs rw\n",
            " - - - - - \n",
            "1 1 0:2 / / rw -rootfs\n",
        ] {
            assert_eq!(root_fs(hostile), None, "{hostile:?}");
        }
        assert_eq!(root_fs("1 1 0:2 / / rw - \n"), Some(""));
    }

    /// HEX-GUEST-11 (§4a): `hex.shares=` on the kernel's command line names Shards' shares in tag order,
    /// `rw` or `ro` each, which init mounts at /run/hex/shares/<i>; none named is none; a bad mode, an
    /// empty one, the parameter twice or past its bound is refused by name; init's own arguments after
    /// `--` are not the kernel's.
    #[test]
    fn shares_are_read_from_the_kernel_command_line() {
        let (rw, ro) = (Share { read_only: false }, Share { read_only: true });
        assert_eq!(shares("console=ttyS0 panic=-1 -- guest init"), Ok(vec![]));
        assert_eq!(
            shares("console=ttyS0 hex.shares=rw,ro,rw -- guest init\n"),
            Ok(vec![rw, ro, Share { read_only: false }])
        );
        assert_eq!(shares("hex.shares=ro"), Ok(vec![Share { read_only: true }]));
        assert_eq!(shares("-- hex.shares=rw"), Ok(vec![]));
        let past = vec!["rw"; MAX_SHARES + 1].join(",");
        let at = vec!["rw"; MAX_SHARES].join(",");
        assert_eq!(
            shares(&format!("hex.shares={at}")).map(|s| s.len()),
            Ok(MAX_SHARES)
        );
        for bad in [
            "hex.shares=".to_owned(),
            "hex.shares=rw,,ro".to_owned(),
            "hex.shares=rx".to_owned(),
            "hex.shares=RW".to_owned(),
            "hex.shares=rw hex.shares=ro".to_owned(),
            format!("hex.shares={past}"),
        ] {
            let why = shares(&bad).unwrap_err();
            assert!(why.contains("hex.shares"), "{bad:?}: {why}");
        }
    }

    /// HEX-GUEST-12 (§4): as PID 1, `hex` is the guest's init, whatever words the kernel passes it
    /// (Shards appends `quiet` to a run with no console, which once reached init as an argument and
    /// ended the guest: a boot on adas-mac-studio-2, 2026-11-06, `Attempted to kill init!
    /// exitcode=0x00000200`); as any other process it is not, nor as PID 1 of a container's namespace.
    #[test]
    fn hex_is_the_guests_init_as_pid_1_alone() {
        let init = Some(std::ffi::OsStr::new("/init"));
        assert!(runs_as_init(1, init));
        for pid in [0, 2, 1000, u32::MAX] {
            assert!(!runs_as_init(pid, init), "{pid}");
        }
        // A container's first program is PID 1 of its namespace, and runs as itself.
        for argv0 in ["/hex", "/proc/self/exe", "hex", "/usr/bin/hex"] {
            assert!(
                !runs_as_init(1, Some(std::ffi::OsStr::new(argv0))),
                "{argv0}"
            );
        }
        assert!(!runs_as_init(1, None));
    }
}
