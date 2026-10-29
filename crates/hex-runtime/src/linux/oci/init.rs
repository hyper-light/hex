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

use crate::linux::oci::inroot::{self, Make};
use crate::linux::oci::spec::{Device, Process, Seccomp, Spec};
use crate::linux::oci::{Failure, OciError, caps, net, seccomp, sys};

/// Environment the `create` runtime sets for `hex __oci-init`.
pub const ENV_SYNC_W: &str = "HEX_OCI_SYNC_W";
pub const ENV_MAP_R: &str = "HEX_OCI_MAP_R";
/// The container's own state directory (holds `state.json` and `exec.fifo`).
pub const ENV_DIR: &str = "HEX_OCI_DIR";
pub const ENV_BUNDLE: &str = "HEX_OCI_BUNDLE";
/// The container's cgroup directory, which `create` has made with its resources in force.
pub const ENV_CGROUP: &str = "HEX_OCI_CGROUP";
/// A network namespace made ahead, its loopback up (decision 4's pool), by the path of the runtime's
/// descriptor for it (`/proc/<pid>/fd/<n>`), which stays close-on-exec: the shim opens it, and joins
/// it in place of making the container's.
pub const ENV_NETWORK: &str = "HEX_OCI_NETWORK";

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
    let sync = match inherited_fd(ENV_SYNC_W) {
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
    // A network namespace made ahead (decision 4), opened by its path before any namespace changes,
    // while this process's /proc is the runtime's. It belongs to the runtime's user namespace, so it
    // serves only a container that makes its own network namespace there.
    let network = match std::env::var_os(ENV_NETWORK) {
        Some(path) => {
            if !spec.makes_namespace("network") || !born_with_callers_credentials {
                return Err(OciError::Config(
                    "a network namespace made ahead is for a container that makes its own network \
                     namespace and has no user namespace"
                        .to_owned(),
                ));
            }
            Some(
                rustix::fs::open(
                    Path::new(&path),
                    OFlags::RDONLY | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(|e| OciError::os("opening the network namespace made ahead", e))?,
            )
        }
        None => None,
    };

    // The map pipe's end is read only for a user namespace's maps, and is the shim's to close either
    // way, before the container is forked.
    let map = inherited_fd(ENV_MAP_R)?;
    if spec.makes_namespace("user") {
        sys::unshare(UnshareFlags::NEWUSER)?;
        msg(sync, &format!("U{}", std::process::id()));
        let mut ack = [0u8; 1];
        rustix::io::read(&map, &mut ack).map_err(|e| OciError::os("reading the map ack", e))?;
        become_userns_root()?;
    }
    drop(map);

    join_namespace_paths(&spec)?;
    // The network namespace made ahead is joined in place of a fresh one, and the shim's copy closed
    // before the fork, so the container holds it by being in it and by nothing else.
    let mut flags = namespace_flags(&spec);
    let made_ahead = match network {
        Some(network) => {
            sys::setns(network.as_fd(), libc::CLONE_NEWNET)?;
            flags.remove(UnshareFlags::NEWNET);
            true
        }
        None => false,
    };
    sys::unshare(flags)?;

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
            match container(&spec, &bundle, &container_dir, sync, procs, made_ahead) {
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
/// set up the world, signal ready, wait for `start`, exec. `network_made_ahead`: its network namespace
/// came from the node's pool, its loopback already up.
fn container(
    spec: &Spec,
    bundle: &str,
    container_dir: &Path,
    sync: BorrowedFd<'_>,
    procs: Option<OwnedFd>,
    network_made_ahead: bool,
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
    // A network namespace of the container's own holds loopback, down until it's brought up, as
    // runc and crun bring it up; one made ahead had it brought up when it was made.
    if spec.makes_namespace("network") && !network_made_ahead {
        net::loopback_up()?;
    }
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

/// Becomes the container's process as its spec gives it: its OOM score, limits, seccomp filter,
/// capabilities, user, no_new_privs and umask. The container's first process does this before
/// `start`, and each one `hex oci exec` runs does it before it execs ([`Becoming`]).
pub(crate) fn become_process(process: &Process, seccomp: Option<&Seccomp>) -> Result<(), OciError> {
    Ok(Becoming::of(process, seccomp)?.apply()?)
}

/// The container's process, made ready to become before a clone (goal 3 plan step 2): its seccomp filter
/// compiled to the kernel's instructions, its limits, groups and capability sets resolved, its OOM score's
/// text made. [`Becoming::apply`] then makes system calls alone on what was made here, so the child of a
/// vfork-style clone, which shares its parent's memory and may not allocate, can become the process.
///
/// no_new_privs is exactly the spec's `noNewPrivileges`, so the seccomp filter goes on where that
/// allows, as runc puts it on (libcontainer/standard_init_linux.go). Without no_new_privs, loading a
/// filter needs CAP_SYS_ADMIN (seccomp(2)), so it goes on before the user and capabilities change,
/// and the profile also binds Hex's own calls from there to `execve`. With no_new_privs, it goes on
/// last, so as few of Hex's own calls as possible run under it.
pub(crate) struct Becoming {
    oom: Option<Vec<u8>>,
    rlimits: Vec<(rustix::process::Resource, rustix::process::Rlimit)>,
    filter: Option<Vec<libc::sock_filter>>,
    no_new_privileges: bool,
    caps: Option<caps::Ready>,
    uid: rustix::process::Uid,
    gid: rustix::process::Gid,
    groups: Vec<rustix::process::Gid>,
    umask: Option<Mode>,
}

impl Becoming {
    /// `process` made ready. A profile Hex can't honour is refused by name here, before anything about
    /// the process changes (seccomp::compile), never run unfiltered (CLAUDE.md §3).
    pub(crate) fn of(process: &Process, seccomp: Option<&Seccomp>) -> Result<Self, OciError> {
        let filter = seccomp.map(seccomp::compile).transpose()?.map(|program| {
            program
                .iter()
                .map(|f| libc::sock_filter {
                    code: f.code,
                    jt: f.jt,
                    jf: f.jf,
                    k: f.k,
                })
                .collect::<Vec<_>>()
        });
        if filter
            .as_ref()
            .is_some_and(|f| u16::try_from(f.len()).is_err())
        {
            return Err(OciError::Config(
                "the seccomp filter has more instructions than a filter holds".to_owned(),
            ));
        }
        let rlimits = process
            .rlimits
            .iter()
            .map(|limit| {
                Ok((
                    caps::rlimit_resource(&limit.kind)?,
                    rustix::process::Rlimit {
                        current: Some(limit.soft),
                        maximum: Some(limit.hard),
                    },
                ))
            })
            .collect::<Result<_, OciError>>()?;
        Ok(Self {
            oom: process
                .oom_score_adj
                .map(|adj| adj.to_string().into_bytes()),
            rlimits,
            filter,
            no_new_privileges: process.no_new_privileges,
            caps: process.capabilities.as_ref().map(caps::ready),
            uid: rustix::process::Uid::from_raw(process.user.uid),
            gid: rustix::process::Gid::from_raw(process.user.gid),
            groups: process
                .user
                .additional_gids
                .iter()
                .map(|g| rustix::process::Gid::from_raw(*g))
                .collect(),
            umask: process.user.umask.map(Mode::from_bits_truncate),
        })
    }

    /// Becomes the process: system calls alone, in the order the type's docs give.
    pub(crate) fn apply(&self) -> Result<(), Failure> {
        // oom_score_adj is set while still privileged (lowering it needs CAP_SYS_RESOURCE).
        if let Some(text) = &self.oom {
            let fd = rustix::fs::open(
                c"/proc/self/oom_score_adj",
                OFlags::WRONLY | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(Failure::of("opening oom_score_adj"))?;
            rustix::io::write(&fd, text).map_err(Failure::of("writing oom_score_adj"))?;
        }
        for (resource, limit) in &self.rlimits {
            rustix::process::setrlimit(*resource, *limit).map_err(Failure::of("setrlimit"))?;
        }
        if !self.no_new_privileges
            && let Some(filter) = &self.filter
        {
            sys::load_seccomp_filter(filter).map_err(Failure::of("loading the seccomp filter"))?;
        }
        // Capabilities around the uid change (runc's order): drop the bounding set while still root
        // (needs effective CAP_SETPCAP), keep the permitted set across setuid, then set the sets exactly
        // as the dropped user, so a non-root container still holds the capabilities its spec grants.
        match &self.caps {
            Some(c) => {
                c.drop_bounding()?;
                sys::set_keepcaps(true).map_err(Failure::of("PR_SET_KEEPCAPS"))?;
                self.set_user()?;
                c.set_process()?;
                sys::set_keepcaps(false).map_err(Failure::of("PR_SET_KEEPCAPS"))?;
            }
            None => self.set_user()?,
        }
        if self.no_new_privileges {
            rustix::thread::set_no_new_privs(true).map_err(Failure::of("set_no_new_privs"))?;
        }
        if let Some(mask) = self.umask {
            rustix::process::umask(mask);
        }
        // With no_new_privs now set, the filter needs no privilege.
        if self.no_new_privileges
            && let Some(filter) = &self.filter
        {
            sys::load_seccomp_filter(filter).map_err(Failure::of("loading the seccomp filter"))?;
        }
        Ok(())
    }

    fn set_user(&self) -> Result<(), Failure> {
        // A user namespace with setgroups denied (the rootless one) refuses this, and the process keeps
        // the groups it has: none beyond its gid there.
        let _ = rustix::thread::set_thread_groups(&self.groups);
        rustix::thread::set_thread_res_gid(self.gid, self.gid, self.gid)
            .map_err(Failure::of("setgid"))?;
        rustix::thread::set_thread_res_uid(self.uid, self.uid, self.uid)
            .map_err(Failure::of("setuid"))?;
        Ok(())
    }
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
/// symlinks, masked and readonly paths, then `pivot_root` into it. Every path in the root is resolved
/// inside it ([`inroot`]): until the pivot `/` is the host's, and an image's links may point there.
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
    for op in crate::linux::oci::mount::plan(&spec.mounts)? {
        do_mount(&rootfs, &op)?;
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

fn do_mount(rootfs: &Path, op: &crate::linux::oci::mount::Op) -> Result<(), OciError> {
    // Which mount failed, and of what, with why.
    let failed = |e: rustix::io::Errno| {
        format!(
            "{} at {}: {e}",
            if op.is_bind {
                op.kind_source.as_str()
            } else {
                op.fstype.as_str()
            },
            op.destination
        )
    };
    let make = if op.is_bind && Path::new(&op.kind_source).is_file() {
        Make::File
    } else {
        Make::Dir
    };
    let target_fd = target(rootfs, &op.destination, make)?;
    let at = inroot::fd_path(&target_fd);
    if op.is_bind {
        if op.recursive_bind {
            rustix::mount::mount_bind_recursive(&op.kind_source, &at)
                .map_err(|e| OciError::os("bind mount", failed(e)))?;
        } else {
            rustix::mount::mount_bind(&op.kind_source, &at)
                .map_err(|e| OciError::os("bind mount", failed(e)))?;
        }
        if op.flags.intersects(MountFlags::RDONLY) {
            // Opened again, the destination is the bind just made on top of the target.
            let top_fd = target(rootfs, &op.destination, Make::Nothing)?;
            let top = inroot::fd_path(&top_fd);
            rustix::mount::mount_remount(&top, op.flags | MountFlags::BIND, op.data.as_str())
                .map_err(|e| OciError::os("remounting bind", failed(e)))?;
        }
    } else {
        let data = data_cstr(&op.data)?;
        rustix::mount::mount(
            op.kind_source.as_str(),
            &at,
            op.fstype.as_str(),
            op.flags,
            data.as_deref(),
        )
        .map_err(|e| OciError::os("mount", failed(e)))?;
    }
    if let Some(prop) = op.propagation {
        let top_fd = target(rootfs, &op.destination, Make::Nothing)?;
        rustix::mount::mount_change(inroot::fd_path(&top_fd), prop)
            .map_err(|e| OciError::os("mount propagation", failed(e)))?;
    }
    Ok(())
}

/// `dest` opened in the container's root, made as `make` says when it's missing.
fn target(rootfs: &Path, dest: &str, make: Make) -> Result<OwnedFd, OciError> {
    inroot::open(rootfs, dest, make)?
        .ok_or_else(|| OciError::Config(format!("{dest} isn't in the container's root")))
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

fn make_default_devices(rootfs: &Path, in_userns: bool) -> Result<(), OciError> {
    for (path, major, minor, mode) in DEFAULT_DEVICES {
        if inroot::open(rootfs, path, Make::Nothing)?.is_some() {
            continue;
        }
        if in_userns {
            bind_host_device(rootfs, path, path)?;
        } else {
            make_node(
                rootfs,
                path,
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
    if inroot::open(rootfs, &dev.path, Make::Nothing)?.is_some() {
        return Ok(());
    }
    if in_userns {
        return bind_host_device(rootfs, &dev.path, &dev.path);
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
    make_node(
        rootfs,
        &dev.path,
        kind,
        dev.file_mode.unwrap_or(0o666),
        major,
        minor,
    )?;
    if let (Some(uid), Some(gid)) = (dev.uid, dev.gid) {
        let (dir, name) = inroot::parent(rootfs, &dev.path)?;
        rustix::fs::chownat(
            &dir,
            name.as_str(),
            Some(rustix::fs::Uid::from_raw(uid)),
            Some(rustix::fs::Gid::from_raw(gid)),
            rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
        )
        .map_err(|e| OciError::os("chowning a device", e))?;
    }
    Ok(())
}

/// Makes a device node at `dest` in the root, then sets its mode exactly (mknod's mode is masked by
/// the umask).
fn make_node(
    rootfs: &Path,
    dest: &str,
    kind: rustix::fs::FileType,
    mode: u32,
    major: u32,
    minor: u32,
) -> Result<(), OciError> {
    let (dir, name) = inroot::parent(rootfs, dest)?;
    rustix::fs::mknodat(
        &dir,
        name.as_str(),
        kind,
        Mode::from_bits_truncate(mode),
        rustix::fs::makedev(major, minor),
    )
    .map_err(|e| OciError::os("making a device", e))?;
    rustix::fs::chmodat(
        &dir,
        name.as_str(),
        Mode::from_bits_truncate(mode),
        rustix::fs::AtFlags::empty(),
    )
    .map_err(|e| OciError::os("setting a device's mode", e))?;
    Ok(())
}

/// Binds the host's device file `host_path` at `dest` in the container (the user-namespace path,
/// where mknod is refused).
fn bind_host_device(rootfs: &Path, host_path: &str, dest: &str) -> Result<(), OciError> {
    if !Path::new(host_path).exists() {
        return Ok(());
    }
    let target_fd = target(rootfs, dest, Make::File)?;
    rustix::mount::mount_bind(host_path, inroot::fd_path(&target_fd))
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
        if inroot::open(rootfs, link, Make::Nothing)?.is_some() {
            continue;
        }
        let (dir, name) = inroot::parent(rootfs, link)?;
        match rustix::fs::symlinkat(target, &dir, name.as_str()) {
            // A link there already, to what isn't mounted yet.
            Ok(()) | Err(rustix::io::Errno::EXIST) => {}
            Err(e) => return Err(OciError::os("making a /dev link", format!("{link}: {e}"))),
        }
    }
    Ok(())
}

/// A masked path is hidden: a directory gets a read-only empty tmpfs, a file a bind of /dev/null.
fn mask_path(rootfs: &Path, path: &str) -> Result<(), OciError> {
    let Some(target) = inroot::open(rootfs, path, Make::Nothing)? else {
        return Ok(());
    };
    let dir = rustix::fs::fstat(&target)
        .map(|st| rustix::fs::FileType::from_raw_mode(st.st_mode).is_dir())
        .map_err(|e| OciError::os("masking a path", format!("{path}: {e}")))?;
    let at = inroot::fd_path(&target);
    if dir {
        rustix::mount::mount("tmpfs", &at, "tmpfs", MountFlags::RDONLY, Some(c"size=0k"))
            .map_err(|e| OciError::os("masking a directory", format!("{path}: {e}")))?;
    } else {
        rustix::mount::mount_bind("/dev/null", &at)
            .map_err(|e| OciError::os("masking a file", format!("{path}: {e}")))?;
    }
    Ok(())
}

/// A readonly path is bound onto itself and remounted read-only.
fn readonly_path(rootfs: &Path, path: &str) -> Result<(), OciError> {
    let Some(target) = inroot::open(rootfs, path, Make::Nothing)? else {
        return Ok(());
    };
    let at = inroot::fd_path(&target);
    rustix::mount::mount_bind(&at, &at)
        .map_err(|e| OciError::os("binding a readonly path", format!("{path}: {e}")))?;
    // Opened again, the path is the bind just made on top of it.
    let top_fd = self::target(rootfs, path, Make::Nothing)?;
    rustix::mount::mount_remount(
        inroot::fd_path(&top_fd),
        MountFlags::RDONLY | MountFlags::BIND,
        "",
    )
    .map_err(|e| OciError::os("remounting a readonly path", format!("{path}: {e}")))?;
    Ok(())
}

fn apply_sysctls(spec: &Spec) -> Result<(), OciError> {
    for (key, value) in &spec.linux.sysctl {
        let path = Path::new("/proc/sys").join(key.replace('.', "/"));
        fs::write(&path, value).map_err(|e| OciError::os("writing a sysctl", e))?;
    }
    Ok(())
}

fn env_var(name: &str) -> Result<String, OciError> {
    std::env::var(name)
        .map_err(|_| OciError::Broken(format!("{name} not set for the container init")))
}

/// A pipe end the runtime passed across its exec of the shim, named by `name`. It goes no further: it's
/// made close-on-exec again, so the program the container execs never holds it (crun's holds only its
/// stdio, and so does Hex's).
fn inherited_fd(name: &str) -> Result<OwnedFd, OciError> {
    let n: RawFd = env_var(name)?
        .parse()
        .map_err(|_| OciError::Broken(format!("{name} isn't an fd")))?;
    let fd = sys::own_fd(n)?;
    rustix::io::fcntl_setfd(&fd, rustix::io::FdFlags::CLOEXEC)
        .map_err(|e| OciError::os("setting close-on-exec", e))?;
    Ok(fd)
}

/// Writes one newline-terminated sync message; a broken pipe is ignored (the runtime has gone).
fn msg(sync: BorrowedFd<'_>, text: &str) {
    let line = format!("{text}\n");
    let _ = rustix::io::write(sync, line.as_bytes());
}
