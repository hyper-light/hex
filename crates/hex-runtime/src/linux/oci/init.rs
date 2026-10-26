//! The container init, reached by re-exec (`hex __oci-init`): the shim unshares the namespaces,
//! forks the container and exits, leaving the container to the caller's subreaper as crun's create
//! does (decision 2 of docs/design/runtime-architecture.md). The container sets up its root
//! filesystem, devices, paths, limits, capabilities and user, then blocks on the exec FIFO until
//! `start` and finally `execve`s the workload. Because the process re-exec'd, it is single-threaded
//! when it forks (see [`super::sys`]).
//!
//! It talks to the `create` runtime over two inherited pipes (fd numbers in the environment): a sync
//! pipe the init writes (`U`ser-ns map request, container `P`id, `R`eady, or `E`rror), and a map
//! pipe the runtime writes its acknowledgement on.

use std::fs;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use rustix::fs::{Mode, OFlags};
use rustix::mount::{MountFlags, MountPropagationFlags, UnmountFlags};
use rustix::thread::UnshareFlags;

use crate::linux::oci::spec::{Device, Process, Seccomp, Spec};
use crate::linux::oci::{OciError, caps, seccomp, sys};

/// Environment the `create` runtime sets for `hex __oci-init`.
pub const ENV_SYNC_W: &str = "HEX_OCI_SYNC_W";
pub const ENV_MAP_R: &str = "HEX_OCI_MAP_R";
/// The container's own state directory (holds `state.json` and `exec.fifo`).
pub const ENV_DIR: &str = "HEX_OCI_DIR";
pub const ENV_BUNDLE: &str = "HEX_OCI_BUNDLE";
/// The container's cgroup directory, which `create` has made with its resources in force.
pub const ENV_CGROUP: &str = "HEX_OCI_CGROUP";

/// The default device nodes a runtime makes in `/dev` (OCI spec §Default Devices), as
/// (path, major, minor, mode); all character devices.
const DEFAULT_DEVICES: [(&str, u32, u32, u32); 6] = [
    ("/dev/null", 1, 3, 0o666),
    ("/dev/zero", 1, 5, 0o666),
    ("/dev/full", 1, 7, 0o666),
    ("/dev/random", 1, 8, 0o666),
    ("/dev/urandom", 1, 9, 0o666),
    ("/dev/tty", 5, 0, 0o666),
];

/// The `hex __oci-init` entry. Returns the process exit code; a setup failure is reported to the
/// runtime over the sync pipe before returning non-zero.
pub fn run() -> u8 {
    let sync = match env_fd(ENV_SYNC_W) {
        Ok(fd) => fd,
        Err(_) => return 125,
    };
    match shim(sync.as_fd()) {
        Ok(code) => code,
        Err(e) => {
            msg(sync.as_fd(), &format!("E{e}"));
            127
        }
    }
}

/// The shim: unshares namespaces, forks the container, reports its pid and exits. Nothing waits on the
/// container: `state` finds it stopped once its recorded init no longer runs. The shim stays outside
/// the container's cgroup, so the container's limits bound the container alone.
fn shim(sync: BorrowedFd<'_>) -> Result<u8, OciError> {
    let container_dir = PathBuf::from(env_var(ENV_DIR)?);
    let bundle = env_var(ENV_BUNDLE)?;
    let spec = Spec::parse(
        &fs::read(Path::new(&bundle).join("config.json"))
            .map_err(|e| OciError::os("reading config.json", e))?,
    )?;
    // The cgroup, opened before any namespace changes: its directory, for the container to be born in
    // (`clone3` with `CLONE_INTO_CGROUP`), and its `cgroup.procs`, for it to join by migration where it
    // can't be. The kernel checks a migration against the credentials and cgroup namespace the file was
    // opened with (Linux 5.16, the fix for CVE-2021-4197), so the join works from inside a user
    // namespace too.
    let cgroup = std::env::var_os(ENV_CGROUP)
        .map(|dir| {
            let dir = Path::new(&dir);
            let open = |path: &Path, flags: OFlags| {
                rustix::fs::open(path, flags | OFlags::CLOEXEC, Mode::empty())
                    .map_err(|e| OciError::os("opening the container's cgroup", e))
            };
            Ok::<_, OciError>((
                open(dir, OFlags::RDONLY | OFlags::DIRECTORY)?,
                open(&dir.join("cgroup.procs"), OFlags::WRONLY)?,
            ))
        })
        .transpose()?;
    // A birth in the cgroup is checked against the credentials the clone runs with, and in a user
    // namespace the shim's are no longer the caller's, so there the container joins by migration.
    let born_with_callers_credentials = !spec.linux.namespaces.iter().any(|n| n.kind == "user");

    if spec.makes_namespace("user") {
        sys::unshare(UnshareFlags::NEWUSER)?;
        msg(sync, &format!("U{}", std::process::id()));
        let map = env_fd(ENV_MAP_R)?;
        let mut ack = [0u8; 1];
        rustix::io::read(&map, &mut ack).map_err(|e| OciError::os("reading the map ack", e))?;
        become_userns_root()?;
    }

    join_namespace_paths(&spec)?;
    sys::unshare(namespace_flags(&spec))?;

    // Born in its cgroup where it can be: a migration takes the cgroup threadgroup lock, whose writer can
    // wait out an RCU grace period (docs/design/runtime.md). Otherwise forked, to join by migration.
    let born = match &cgroup {
        Some((dir, _)) if born_with_callers_credentials => sys::fork_into_cgroup(dir.as_fd())?,
        _ => None,
    };
    let (forked, procs) = match (born, cgroup) {
        (Some(forked), _) => (forked, None),
        (None, cgroup) => (sys::fork()?, cgroup.map(|(_, procs)| procs)),
    };
    match forked {
        sys::Fork::Child => Ok(
            match container(&spec, &bundle, &container_dir, sync, procs) {
                Ok(()) => 0,
                Err(e) => {
                    msg(sync, &format!("E{e}"));
                    127
                }
            },
        ),
        sys::Fork::Parent(container_pid) => {
            drop(procs);
            msg(sync, &format!("P{container_pid}"));
            Ok(0)
        }
    }
}

/// The container (PID 1 of its namespaces): join its cgroup through `procs` (when it wasn't born there),
/// set up the world, signal ready, wait for `start`, exec.
fn container(
    spec: &Spec,
    bundle: &str,
    container_dir: &Path,
    sync: BorrowedFd<'_>,
    procs: Option<OwnedFd>,
) -> Result<(), OciError> {
    // First of all, before it allocates or starts anything: into its cgroup, so its limits and
    // accounting cover all it does. Then a fresh cgroup namespace, so the namespace's root is the
    // container's own cgroup (the shim no longer unshares it: it isn't in that cgroup).
    if let Some(procs) = procs {
        let n = rustix::io::write(&procs, b"0")
            .map_err(|e| OciError::os("joining the container's cgroup", e))?;
        if n != 1 {
            return Err(OciError::os(
                "joining the container's cgroup",
                "a short write to cgroup.procs",
            ));
        }
    }
    if spec.makes_namespace("cgroup") {
        sys::unshare(UnshareFlags::NEWCGROUP)?;
    }

    // The exec FIFO lives in the runtime's state dir, outside the rootfs; open its directory now,
    // before pivot_root removes that path from view, and open the FIFO through this fd later.
    let fifo_dir = rustix::fs::open(
        container_dir,
        OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| OciError::os("opening the container directory", e))?;

    let _ = rustix::process::setsid();
    if spec.makes_namespace("mount") {
        setup_rootfs(spec, bundle)?;
    }
    if spec.makes_namespace("uts")
        && let Some(hostname) = &spec.hostname
    {
        rustix::system::sethostname(hostname.as_bytes())
            .map_err(|e| OciError::os("sethostname", e))?;
    }
    apply_sysctls(spec)?;
    // A container may have no process (OCI `process` is optional): it is set up and, once started,
    // simply has nothing to run, so it stops at once.
    let Some(process) = spec.process.clone() else {
        msg(sync, "R");
        let fifo = rustix::fs::openat(&fifo_dir, "exec.fifo", OFlags::WRONLY, Mode::empty())
            .map_err(|e| OciError::os("opening the exec fifo", e))?;
        let _ = rustix::io::write(&fifo, b"0");
        return Ok(());
    };
    become_process(&process, spec.linux.seccomp.as_ref())?;

    // Ready: the runtime may now report `created`. Then block until `start` opens the FIFO.
    msg(sync, "R");
    let fifo = rustix::fs::openat(&fifo_dir, "exec.fifo", OFlags::WRONLY, Mode::empty())
        .map_err(|e| OciError::os("opening the exec fifo", e))?;
    rustix::io::write(&fifo, b"0").map_err(|e| OciError::os("writing the exec fifo", e))?;
    drop(fifo);

    exec(&process)
}

/// Becomes the container's process as its spec gives it: its OOM score, limits, capabilities, user,
/// no_new_privs and umask, and finally the seccomp filter. The container's first process does this
/// before `start`, and each one `hex oci exec` runs does it before it execs.
///
/// The seccomp filter is applied last, so the few syscalls Hex itself makes before this point
/// (setuid, setrlimit, …) run unfiltered, and only the workload's `execve` and what follows are
/// bound by it. The init re-exec'd, so it is single-threaded: [`seccompiler::apply_filter`] binds
/// its one thread, which is the thread that `execve`s (CLAUDE.md §12: no new `unsafe`; seccompiler
/// makes the `seccomp(2)` call).
pub(crate) fn become_process(process: &Process, seccomp: Option<&Seccomp>) -> Result<(), OciError> {
    // oom_score_adj is set while still privileged (lowering it needs CAP_SYS_RESOURCE).
    if let Some(adj) = process.oom_score_adj {
        fs::write("/proc/self/oom_score_adj", adj.to_string())
            .map_err(|e| OciError::os("writing oom_score_adj", e))?;
    }
    apply_rlimits(process)?;
    // Capabilities around the uid change (runc's order): drop the bounding set while still root
    // (needs effective CAP_SETPCAP), keep the permitted set across setuid, then set the sets exactly
    // as the dropped user, so a non-root container still holds the capabilities its spec grants.
    if let Some(c) = &process.capabilities {
        caps::drop_bounding(c)?;
        sys::set_keepcaps(true)?;
        set_user(process)?;
        caps::set_process(c)?;
        sys::set_keepcaps(false)?;
    } else {
        set_user(process)?;
    }
    if process.no_new_privileges {
        rustix::thread::set_no_new_privs(true).map_err(|e| OciError::os("set_no_new_privs", e))?;
    }
    if let Some(mask) = process.user.umask {
        rustix::process::umask(Mode::from_bits_truncate(mask));
    }
    // The seccomp filter last: a profile Hex can't honour is refused here by name (seccomp::compile),
    // never run unfiltered (CLAUDE.md §3). apply_filter also sets PR_SET_NO_NEW_PRIVS, which a
    // seccomp filter needs when the thread isn't privileged.
    if let Some(profile) = seccomp {
        let program = seccomp::compile(profile)?;
        seccompiler::apply_filter(&program)
            .map_err(|e| OciError::os("loading the seccomp filter", e))?;
    }
    Ok(())
}

/// `execve`s the workload, resolving `args[0]` in `PATH` when it has no slash. Only returns on error.
pub(crate) fn exec(process: &Process) -> Result<(), OciError> {
    let prog = process
        .args
        .first()
        .ok_or_else(|| OciError::Config("process.args is empty".to_owned()))?;
    let path = resolve_program(prog, &process.env)?;
    let mut cmd = Command::new(&path);
    cmd.arg0(prog);
    cmd.args(process.args.get(1..).unwrap_or_default());
    cmd.current_dir(&process.cwd);
    cmd.env_clear();
    for entry in &process.env {
        if let Some((k, v)) = entry.split_once('=') {
            cmd.env(k, v);
        }
    }
    // exec() returns only if it failed.
    Err(OciError::os("execve", cmd.exec()))
}

/// Resolves a program name to a path: used as-is when it has a slash, else searched in `PATH`.
pub(crate) fn resolve_program(prog: &str, env: &[String]) -> Result<PathBuf, OciError> {
    if prog.contains('/') {
        return Ok(PathBuf::from(prog));
    }
    let path = env
        .iter()
        .find_map(|e| e.strip_prefix("PATH="))
        .unwrap_or("/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin");
    for dir in path.split(':') {
        let candidate = Path::new(dir).join(prog);
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(OciError::Config(format!(
        "program {prog:?} not found in PATH"
    )))
}

/// Becomes uid/gid 0 within a fresh user namespace (after the runtime wrote the maps).
fn become_userns_root() -> Result<(), OciError> {
    let zero_g = rustix::process::Gid::from_raw(0);
    let zero_u = rustix::process::Uid::from_raw(0);
    let _ = rustix::thread::set_thread_groups(&[]);
    rustix::thread::set_thread_res_gid(zero_g, zero_g, zero_g)
        .map_err(|e| OciError::os("setresgid in userns", e))?;
    rustix::thread::set_thread_res_uid(zero_u, zero_u, zero_u)
        .map_err(|e| OciError::os("setresuid in userns", e))?;
    Ok(())
}

/// The `CLONE_NEW*` flag for a namespace kind.
fn ns_clone_flag(kind: &str) -> Option<i32> {
    let f = match kind {
        "mount" => libc::CLONE_NEWNS,
        "uts" => libc::CLONE_NEWUTS,
        "ipc" => libc::CLONE_NEWIPC,
        "network" => libc::CLONE_NEWNET,
        "cgroup" => libc::CLONE_NEWCGROUP,
        "pid" => libc::CLONE_NEWPID,
        "user" => libc::CLONE_NEWUSER,
        _ => return None,
    };
    Some(f)
}

/// Joins each namespace the spec gives by `path` (as opposed to making a fresh one), checking each
/// is of the named type. User and pid are joined with an eye to order: user first (its privileges
/// gate the rest), pid last (it governs the fork that follows).
fn join_namespace_paths(spec: &Spec) -> Result<(), OciError> {
    let order = |kind: &str| match kind {
        "user" => 0,
        "pid" => 2,
        _ => 1,
    };
    let mut with_path: Vec<&crate::linux::oci::spec::Namespace> = spec
        .linux
        .namespaces
        .iter()
        .filter(|n| n.path.is_some())
        .collect();
    with_path.sort_by_key(|n| order(&n.kind));
    for ns in with_path {
        let Some(path) = &ns.path else { continue };
        let flag = ns_clone_flag(&ns.kind)
            .ok_or_else(|| OciError::Config(format!("unknown namespace type {:?}", ns.kind)))?;
        let fd = rustix::fs::open(path, OFlags::RDONLY | OFlags::CLOEXEC, Mode::empty())
            .map_err(|e| OciError::os("opening a namespace path", e))?;
        sys::setns(fd.as_fd(), flag)?;
    }
    Ok(())
}

/// The namespace flags the shim makes fresh for the container (those without a `path`). A fresh
/// cgroup namespace isn't among them: the container makes it once it has joined its own cgroup.
fn namespace_flags(spec: &Spec) -> UnshareFlags {
    let mut flags = UnshareFlags::empty();
    for (kind, flag) in [
        ("mount", UnshareFlags::NEWNS),
        ("uts", UnshareFlags::NEWUTS),
        ("ipc", UnshareFlags::NEWIPC),
        ("network", UnshareFlags::NEWNET),
        ("pid", UnshareFlags::NEWPID),
    ] {
        if spec.makes_namespace(kind) {
            flags |= flag;
        }
    }
    flags
}

/// Prepares the container's root: private propagation, the spec mounts, default devices and
/// symlinks, masked and readonly paths, then `pivot_root` into it.
fn setup_rootfs(spec: &Spec, bundle: &str) -> Result<(), OciError> {
    let rootfs = {
        let p = Path::new(&spec.root.path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            Path::new(bundle).join(p)
        }
    };
    rustix::mount::mount_change(
        "/",
        MountPropagationFlags::PRIVATE | MountPropagationFlags::REC,
    )
    .map_err(|e| OciError::os("making mounts private", e))?;
    rustix::mount::mount_bind(&rootfs, &rootfs)
        .map_err(|e| OciError::os("binding the rootfs", e))?;

    // In a user namespace mknod is refused (CAP_MKNOD has no effect across the userns boundary), so
    // devices are bind-mounted from the host instead of created.
    let in_userns = spec.makes_namespace("user");
    for op in crate::linux::oci::mount::plan(&spec.mounts, &rootfs)? {
        do_mount(&op)?;
    }
    make_default_devices(&rootfs, in_userns)?;
    for dev in &spec.linux.devices {
        make_device(&rootfs, dev, in_userns)?;
    }
    make_default_symlinks(&rootfs)?;
    for path in &spec.linux.masked_paths {
        mask_path(&rootfs, path)?;
    }
    for path in &spec.linux.readonly_paths {
        readonly_path(&rootfs, path)?;
    }
    if spec.root.readonly {
        rustix::mount::mount_remount(&rootfs, MountFlags::RDONLY | MountFlags::BIND, "")
            .map_err(|e| OciError::os("remounting rootfs ro", e))?;
    }

    rustix::process::chdir(&rootfs).map_err(|e| OciError::os("chdir rootfs", e))?;
    rustix::process::pivot_root(c".", c".").map_err(|e| OciError::os("pivot_root", e))?;
    rustix::mount::unmount(".", UnmountFlags::DETACH)
        .map_err(|e| OciError::os("detaching the old root", e))?;
    rustix::process::chdir("/").map_err(|e| OciError::os("chdir /", e))?;
    // The rootfs propagation the spec asks for (default: left private, as set above).
    if let Some(prop) = spec
        .linux
        .rootfs_propagation
        .as_deref()
        .and_then(propagation)
    {
        rustix::mount::mount_change("/", prop)
            .map_err(|e| OciError::os("setting rootfs propagation", e))?;
    }
    Ok(())
}

/// A rootfs-propagation name (OCI `rootfsPropagation`) as a propagation flag.
fn propagation(name: &str) -> Option<MountPropagationFlags> {
    let f = match name {
        "shared" => MountPropagationFlags::SHARED,
        "slave" => MountPropagationFlags::DOWNSTREAM,
        "private" => MountPropagationFlags::PRIVATE,
        "unbindable" => MountPropagationFlags::UNBINDABLE,
        _ => return None,
    };
    Some(f)
}

fn do_mount(op: &crate::linux::oci::mount::Op) -> Result<(), OciError> {
    ensure_target(op)?;
    if op.is_bind {
        if op.recursive_bind {
            rustix::mount::mount_bind_recursive(&op.kind_source, &op.target)
                .map_err(|e| OciError::os("bind mount", e))?;
        } else {
            rustix::mount::mount_bind(&op.kind_source, &op.target)
                .map_err(|e| OciError::os("bind mount", e))?;
        }
        if op.flags.intersects(MountFlags::RDONLY) {
            rustix::mount::mount_remount(&op.target, op.flags | MountFlags::BIND, op.data.as_str())
                .map_err(|e| OciError::os("remounting bind", e))?;
        }
    } else {
        let data = data_cstr(&op.data)?;
        rustix::mount::mount(
            op.kind_source.as_str(),
            &op.target,
            op.fstype.as_str(),
            op.flags,
            data.as_deref(),
        )
        .map_err(|e| OciError::os("mount", e))?;
    }
    if let Some(prop) = op.propagation {
        rustix::mount::mount_change(&op.target, prop)
            .map_err(|e| OciError::os("mount propagation", e))?;
    }
    Ok(())
}

/// A mount's data string as a C string, or `None` when empty.
fn data_cstr(data: &str) -> Result<Option<std::ffi::CString>, OciError> {
    if data.is_empty() {
        Ok(None)
    } else {
        Ok(Some(std::ffi::CString::new(data.as_bytes()).map_err(
            |_| OciError::Config("mount data has a NUL".to_owned()),
        )?))
    }
}

fn ensure_target(op: &crate::linux::oci::mount::Op) -> Result<(), OciError> {
    if op.is_bind && Path::new(&op.kind_source).is_file() {
        if let Some(parent) = op.target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| OciError::os("making a mount target's parent", e))?;
        }
        if !op.target.exists() {
            fs::File::create(&op.target)
                .map_err(|e| OciError::os("making a mount target file", e))?;
        }
    } else {
        fs::create_dir_all(&op.target).map_err(|e| OciError::os("making a mount target", e))?;
    }
    Ok(())
}

fn make_default_devices(rootfs: &Path, in_userns: bool) -> Result<(), OciError> {
    for (path, major, minor, mode) in DEFAULT_DEVICES {
        let target = crate::linux::oci::mount::under_rootfs(rootfs, path)?;
        if target.exists() {
            continue;
        }
        if in_userns {
            bind_host_device(path, &target)?;
        } else {
            make_node(
                &target,
                rustix::fs::FileType::CharacterDevice,
                mode,
                major,
                minor,
            )?;
        }
    }
    Ok(())
}

fn make_device(rootfs: &Path, dev: &Device, in_userns: bool) -> Result<(), OciError> {
    let target = crate::linux::oci::mount::under_rootfs(rootfs, &dev.path)?;
    if target.exists() {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| OciError::os("making a device's parent", e))?;
    }
    if in_userns {
        return bind_host_device(&dev.path, &target);
    }
    let kind = match dev.kind.as_str() {
        "c" | "u" => rustix::fs::FileType::CharacterDevice,
        "b" => rustix::fs::FileType::BlockDevice,
        "p" => rustix::fs::FileType::Fifo,
        other => {
            return Err(OciError::Config(format!(
                "device type {other:?} isn't c, b, u or p"
            )));
        }
    };
    let major = u32::try_from(dev.major.unwrap_or(0))
        .map_err(|_| OciError::Config("device major out of range".to_owned()))?;
    let minor = u32::try_from(dev.minor.unwrap_or(0))
        .map_err(|_| OciError::Config("device minor out of range".to_owned()))?;
    make_node(&target, kind, dev.file_mode.unwrap_or(0o666), major, minor)?;
    if let (Some(uid), Some(gid)) = (dev.uid, dev.gid) {
        let u = rustix::fs::Uid::from_raw(uid);
        let g = rustix::fs::Gid::from_raw(gid);
        rustix::fs::chownat(
            rustix::fs::CWD,
            &target,
            Some(u),
            Some(g),
            rustix::fs::AtFlags::empty(),
        )
        .map_err(|e| OciError::os("chowning a device", e))?;
    }
    Ok(())
}

/// Makes a device node, then sets its mode exactly (mknod's mode is masked by the umask).
fn make_node(
    target: &Path,
    kind: rustix::fs::FileType,
    mode: u32,
    major: u32,
    minor: u32,
) -> Result<(), OciError> {
    rustix::fs::mknodat(
        rustix::fs::CWD,
        target,
        kind,
        Mode::from_bits_truncate(mode),
        rustix::fs::makedev(major, minor),
    )
    .map_err(|e| OciError::os("making a device", e))?;
    rustix::fs::chmodat(
        rustix::fs::CWD,
        target,
        Mode::from_bits_truncate(mode),
        rustix::fs::AtFlags::empty(),
    )
    .map_err(|e| OciError::os("setting a device's mode", e))?;
    Ok(())
}

/// Binds a host device file into the container (the user-namespace path, where mknod is refused).
fn bind_host_device(host_path: &str, target: &Path) -> Result<(), OciError> {
    if !Path::new(host_path).exists() {
        return Ok(());
    }
    if !target.exists() {
        fs::File::create(target).map_err(|e| OciError::os("making a device bind target", e))?;
    }
    rustix::mount::mount_bind(host_path, target)
        .map_err(|e| OciError::os("binding a host device", e))
}

fn make_default_symlinks(rootfs: &Path) -> Result<(), OciError> {
    let links = [
        ("/proc/self/fd", "/dev/fd"),
        ("/proc/self/fd/0", "/dev/stdin"),
        ("/proc/self/fd/1", "/dev/stdout"),
        ("/proc/self/fd/2", "/dev/stderr"),
        // When a devpts is mounted at /dev/pts, /dev/ptmx is its multiplexer (OCI §Default Devices).
        ("pts/ptmx", "/dev/ptmx"),
    ];
    for (target, link) in links {
        let at = crate::linux::oci::mount::under_rootfs(rootfs, link)?;
        if at.exists() {
            continue;
        }
        if let Some(parent) = at.parent() {
            fs::create_dir_all(parent).map_err(|e| OciError::os("making a symlink's parent", e))?;
        }
        let _ = std::os::unix::fs::symlink(target, &at);
    }
    Ok(())
}

/// A masked path is hidden: a directory gets a read-only empty tmpfs, a file a bind of /dev/null.
fn mask_path(rootfs: &Path, path: &str) -> Result<(), OciError> {
    let target = crate::linux::oci::mount::under_rootfs(rootfs, path)?;
    if !target.exists() {
        return Ok(());
    }
    if target.is_dir() {
        rustix::mount::mount(
            "tmpfs",
            &target,
            "tmpfs",
            MountFlags::RDONLY,
            Some(c"size=0k"),
        )
        .map_err(|e| OciError::os("masking a directory", e))?;
    } else {
        rustix::mount::mount_bind("/dev/null", &target)
            .map_err(|e| OciError::os("masking a file", e))?;
    }
    Ok(())
}

/// A readonly path is bound onto itself and remounted read-only.
fn readonly_path(rootfs: &Path, path: &str) -> Result<(), OciError> {
    let target = crate::linux::oci::mount::under_rootfs(rootfs, path)?;
    if !target.exists() {
        return Ok(());
    }
    rustix::mount::mount_bind(&target, &target)
        .map_err(|e| OciError::os("binding a readonly path", e))?;
    rustix::mount::mount_remount(&target, MountFlags::RDONLY | MountFlags::BIND, "")
        .map_err(|e| OciError::os("remounting a readonly path", e))?;
    Ok(())
}

fn apply_sysctls(spec: &Spec) -> Result<(), OciError> {
    for (key, value) in &spec.linux.sysctl {
        let path = Path::new("/proc/sys").join(key.replace('.', "/"));
        fs::write(&path, value).map_err(|e| OciError::os("writing a sysctl", e))?;
    }
    Ok(())
}

fn apply_rlimits(process: &Process) -> Result<(), OciError> {
    for limit in &process.rlimits {
        let resource = caps::rlimit_resource(&limit.kind)?;
        rustix::process::setrlimit(
            resource,
            rustix::process::Rlimit {
                current: Some(limit.soft),
                maximum: Some(limit.hard),
            },
        )
        .map_err(|e| OciError::os("setrlimit", e))?;
    }
    Ok(())
}

fn set_user(process: &Process) -> Result<(), OciError> {
    let groups: Vec<rustix::process::Gid> = process
        .user
        .additional_gids
        .iter()
        .map(|g| rustix::process::Gid::from_raw(*g))
        .collect();
    let _ = rustix::thread::set_thread_groups(&groups);
    let gid = rustix::process::Gid::from_raw(process.user.gid);
    rustix::thread::set_thread_res_gid(gid, gid, gid).map_err(|e| OciError::os("setgid", e))?;
    let uid = rustix::process::Uid::from_raw(process.user.uid);
    rustix::thread::set_thread_res_uid(uid, uid, uid).map_err(|e| OciError::os("setuid", e))?;
    Ok(())
}

fn env_var(name: &str) -> Result<String, OciError> {
    std::env::var(name)
        .map_err(|_| OciError::Broken(format!("{name} not set for the container init")))
}

/// Takes an inherited fd number from the environment, owning it.
fn env_fd(name: &str) -> Result<OwnedFd, OciError> {
    let n: RawFd = env_var(name)?
        .parse()
        .map_err(|_| OciError::Broken(format!("{name} isn't an fd")))?;
    sys::own_fd(n)
}

/// Writes one newline-terminated sync message; a broken pipe is ignored (the runtime has gone).
fn msg(sync: BorrowedFd<'_>, text: &str) {
    let line = format!("{text}\n");
    let _ = rustix::io::write(sync, line.as_bytes());
}
