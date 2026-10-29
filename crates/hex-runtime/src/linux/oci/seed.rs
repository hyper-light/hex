//! `hex __oci-seed`: a prepared, single-threaded process the node agent keeps and reuses, so that an
//! exec into a container spawns no `hex` process of its own and re-execs no `hex` binary per call
//! (decision 3 of docs/design/runtime-architecture.md, goal 3 plan step 2). The agent hands each exec
//! to a seed over a `SOCK_SEQPACKET` socket: the request's bytes ([`hex_core::seed`]) with the
//! program's stdin, stdout and stderr as descriptors ([`send_request`]). The seed answers with the
//! program's pidfd, and the program is the agent's own child, which the agent's supervisor watches.
//!
//! **One clone, where the seed may choose its children's pid namespace.** A privileged seed sets its
//! children's pid namespace to the container's (`setns(CLONE_NEWPID)`, which governs only the caller's
//! future children) and clones the program with `clone3(CLONE_PARENT | CLONE_INTO_CGROUP |
//! CLONE_PIDFD)`: born in the container's cgroup and pid namespace, and the agent's child from its
//! first instruction, so the seed answers at once. `exit_signal` is 0, the one value `clone3` takes
//! with `CLONE_PARENT` ([`sys::clone_sibling`]). The program then joins the container's other
//! namespaces itself (user first, mount next to last, cgroup last, as `hex oci exec`'s child does),
//! becomes the container's process and execs.
//!
//! **An intermediate, where it may not.** Choosing a pid namespace needs `CAP_SYS_ADMIN` in the
//! caller's own user namespace and in the one that owns the pid namespace (`pidns_install` in
//! kernel/pid_namespace.c), so a rootless seed is refused `EPERM` for a container with a pid namespace
//! of its own. Then the seed forks a short-lived intermediate that joins the container's user
//! namespace first (which grants it those capabilities), then the rest, and clones the program; the
//! intermediate answers the seed over an internal socketpair and exits, and the seed reaps it (so the
//! program, orphaned to the agent, a child subreaper, is the agent's own child) before it forwards the
//! answer, so the agent's `waitid(P_PIDFD)` never meets a program not yet its child. The intermediate is
//! a fork of the seed, no exec, gone before the program runs. So both paths keep "no process but the
//! program" and "no `hex` exec per call".
//!
//! The namespace joins, the clone and the `become_process` are the same ones `hex __oci-exec` makes
//! ([`super::exec`], [`super::init`]); only the plumbing — a socket in place of an inherited request
//! and a waited status — differs.

use std::collections::VecDeque;
use std::ffi::{CString, c_char};
use std::io::{IoSlice, IoSliceMut, Write};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd, RawFd};
use std::path::{Path, PathBuf};

use hex_core::seed::{self, Answer, Failed, Request};
use rustix::net::{
    AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags,
    SendAncillaryBuffer, SendAncillaryMessage, SendFlags, SocketFlags, SocketType,
};
use rustix::pipe::PipeFlags;
use rustix::thread::ThreadNameSpaceType as Ns;

use crate::linux::oci::spec::{Namespace, Process, Spec};
use crate::linux::oci::{Failure, OciError, exec, init, sys};

/// The environment entry naming the socket, inherited from the node agent, the seed serves on.
pub const ENV_SOCKET: &str = "HEX_OCI_SEED_SOCKET";

/// A program that fails before it can start exits as a shell does for a program it can't run (126),
/// and for one that doesn't exist (127), as [`super::exec`] reports the same.
const CANNOT_RUN: u8 = 126;
const NOT_FOUND: u8 = 127;

/// The seed's entry (`hex __oci-seed`): serve execs until the agent closes the socket, then exit 0; a
/// broken socket or a poisoned environment exits non-zero with why on stderr.
pub fn run() -> u8 {
    match serve() {
        Ok(()) => 0,
        Err(e) => {
            let _ = writeln!(std::io::stderr().lock(), "hex __oci-seed: {e}");
            1
        }
    }
}

fn serve() -> Result<(), OciError> {
    let raw: RawFd = env_fd(ENV_SOCKET)?;
    // The agent opened this socket and passed its number here with close-on-exec cleared, so the seed
    // inherited it across the re-exec; the seed is its sole owner. Made close-on-exec again now, so it
    // doesn't survive into a program an intermediate's clone execs (the intermediate is a fork, which
    // keeps it; only the workload's execve, which must not hold the agent's socket, closes it).
    let socket = sys::own_fd(raw)?;
    rustix::io::fcntl_setfd(&socket, rustix::io::FdFlags::CLOEXEC)
        .map_err(|e| OciError::os("setting close-on-exec on the seed's socket", e))?;
    let mut pid_for = PidFor::open()?;
    let mut specs = Specs::default();
    let mut buf = vec![0u8; seed::MAX_REQUEST];
    loop {
        match recv_request(socket.as_fd(), &mut buf)? {
            // The agent closed the socket: nothing more will come, so the seed is done.
            Received::Closed => return Ok(()),
            Received::Message { size, stdio } => {
                let (answer, pidfd) = serve_one(&buf, size, stdio, &mut pid_for, &mut specs);
                send_answer(socket.as_fd(), &answer, pidfd.as_ref().map(AsFd::as_fd))?;
            }
        }
    }
}

/// The pid namespace the seed's children are born in: its own, or the container's the last single
/// clone set. Set explicitly before every clone, so no clone lands in a namespace a past request left.
struct PidFor {
    /// The seed's own pid namespace, opened at its start.
    own: OwnedFd,
    is_own: bool,
}

impl PidFor {
    fn open() -> Result<Self, OciError> {
        let own = rustix::fs::open(
            "/proc/self/ns/pid",
            rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|e| OciError::os("opening the seed's own pid namespace", e))?;
        Ok(Self { own, is_own: true })
    }

    /// The pid namespace of the container init `init` (its pidfd) for the seed's next children.
    /// `EPERM` (an unprivileged seed) is returned for the caller to take the intermediate path;
    /// nothing changed then.
    fn set(&mut self, init: BorrowedFd<'_>) -> rustix::io::Result<()> {
        rustix::thread::move_into_thread_name_spaces(init, Ns::PROCESS_ID)?;
        self.is_own = false;
        Ok(())
    }

    /// The seed's own pid namespace again for its next children, if a single clone changed it.
    fn reset(&mut self) -> Result<(), OciError> {
        if !self.is_own {
            sys::setns(self.own.as_fd(), libc::CLONE_NEWPID)?;
            self.is_own = true;
        }
        Ok(())
    }
}

/// Most parsed `config.json`s a seed keeps ([`Specs`]). A node's seeds meet few bundles at once, and a
/// parsed config is a few KiB, so 64 bound a seed's memory to well under a MiB; past it the oldest is
/// dropped and read again at its next exec. A stated bound (CLAUDE.md §2), not a tuning.
const SPECS: usize = 64;

/// The bundles' parsed `config.json`s, each under the file's identity when it was read (device, inode,
/// modification time and size): an exec into a container whose config the seed has parsed costs one
/// `stat`, not a read and a parse, and a config replaced or rewritten since is read again. At most
/// [`SPECS`], the oldest dropped first.
#[derive(Default)]
pub struct Specs {
    entries: VecDeque<(Identity, Spec)>,
}

/// A file's identity, as `stat` gives it: a rewrite in place changes the time or the size, and a
/// replacement the inode.
#[derive(PartialEq, Eq)]
struct Identity {
    path: PathBuf,
    dev: u64,
    ino: u64,
    mtime: (i128, i128),
    size: i128,
}

impl Specs {
    fn get(&mut self, bundle: &str) -> Result<&Spec, OciError> {
        let path = Path::new(bundle).join("config.json");
        let st = rustix::fs::stat(&path).map_err(|e| OciError::os("reading config.json", e))?;
        let id = Identity {
            path,
            dev: st.st_dev,
            ino: st.st_ino,
            mtime: (i128::from(st.st_mtime), i128::from(st.st_mtime_nsec)),
            size: i128::from(st.st_size),
        };
        let at = match self.entries.iter().position(|(seen, _)| *seen == id) {
            Some(at) => at,
            None => {
                let spec = Spec::parse(
                    &std::fs::read(&id.path).map_err(|e| OciError::os("reading config.json", e))?,
                )?;
                if self.entries.len() >= SPECS {
                    self.entries.pop_front();
                }
                self.entries.push_back((id, spec));
                self.entries.len().saturating_sub(1)
            }
        };
        self.entries
            .get(at)
            .map(|(_, spec)| spec)
            .ok_or_else(|| OciError::Broken("the seed's parsed configs lost one".to_owned()))
    }
}

/// What an exec needs, read and opened by the seed before it clones anything: the container's process
/// with the exec's overrides, its seccomp profile, its cgroup, the init's pidfd and the namespaces of
/// the init's to join by it.
struct Prepared {
    /// The container's process, ready to become.
    becoming: init::Becoming,
    /// The program's execve, made here.
    launch: Launch,
    /// The cgroup's directory, where the program is born (or, where a birth there can't be, whose
    /// `cgroup.procs` it joins by).
    cgroup: Option<OwnedFd>,
    /// The container init's pidfd, from the agent.
    init: OwnedFd,
    /// The kinds of the init's namespaces that aren't the seed's own.
    joins: Ns,
}

/// Applies the exec's overrides to the container's process with `hex __oci-exec`'s own logic (one code
/// path, CLAUDE.md §8). The cgroup is the caller's to give: a seed opens it ([`open_cgroup`]), and the
/// node agent holds it open for an exec straight from its threads.
fn prepare(req: &Request, init: OwnedFd, specs: &mut Specs) -> Result<Prepared, OciError> {
    let spec = specs.get(&req.bundle)?;
    let request = exec::Request {
        pid: req.init_pid,
        bundle: req.bundle.clone(),
        cgroup: req.cgroup.clone(),
        args: req.args.clone(),
        cwd: req.cwd.clone(),
        env: req.env.clone(),
        user: req.user,
        seam: true,
    };
    let process = exec::process_for(spec.process.clone().unwrap_or_default(), &request);
    Ok(Prepared {
        becoming: init::Becoming::of(&process, spec.linux.seccomp.as_ref())?,
        launch: Launch::of(&process)?,
        joins: joins(&spec.linux.namespaces)?,
        cgroup: None,
        init,
    })
}

/// The container's cgroup directory, `dir`, opened (while the opener is still in its own mount
/// namespace) for a clone into it, or for its `cgroup.procs`.
pub fn open_cgroup(dir: Option<&str>) -> Result<Option<OwnedFd>, OciError> {
    dir.map(|dir| {
        rustix::fs::open(
            dir,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|e| OciError::os("opening the container's cgroup", e))
    })
    .transpose()
}

/// The program's `execve`, made before the clone: its arguments and environment as the null-terminated
/// arrays execve(2) takes, the paths to try as execvp(3) tries them, and its working directory. So the
/// clone's child allocates nothing on its way to the program: in a fork every heap write copies a page,
/// which std's `Command` makes many of (the process class's `Spawn` holds the same discipline).
struct Launch {
    /// Owns the strings `argv` points at; moving it moves no string's bytes.
    _args: Vec<CString>,
    /// Owns the strings `envp` points at.
    _env: Vec<CString>,
    argv: Vec<*const c_char>,
    envp: Vec<*const c_char>,
    paths: Vec<CString>,
    cwd: CString,
}

impl Launch {
    /// `process`'s program: its first argument where it names a path, else that name in each folder of
    /// the process's `PATH` (the usual folders when it has none), as [`init::resolve_program`] finds it.
    fn of(process: &Process) -> Result<Self, OciError> {
        let c = |s: &str| {
            CString::new(s).map_err(|_| OciError::Config(format!("{s:?} holds a NUL byte")))
        };
        let program = process
            .args
            .first()
            .ok_or_else(|| OciError::Config("exec needs a program".to_owned()))?;
        let paths = if program.contains('/') {
            vec![c(program)?]
        } else {
            process
                .env
                .iter()
                .find_map(|e| e.strip_prefix("PATH="))
                .unwrap_or("/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")
                .split(':')
                .map(|dir| c(&Path::new(dir).join(program).to_string_lossy()))
                .collect::<Result<_, _>>()?
        };
        let args: Vec<CString> = process
            .args
            .iter()
            .map(|a| c(a))
            .collect::<Result<_, _>>()?;
        // As std's Command takes them: an entry without `=` names no variable.
        let env: Vec<CString> = process
            .env
            .iter()
            .filter(|e| e.contains('='))
            .map(|e| c(e))
            .collect::<Result<_, _>>()?;
        let pointers = |strs: &[CString]| -> Vec<*const c_char> {
            strs.iter()
                .map(|s| s.as_ptr())
                .chain(std::iter::once(std::ptr::null()))
                .collect()
        };
        Ok(Self {
            argv: pointers(&args),
            envp: pointers(&env),
            _args: args,
            _env: env,
            paths,
            cwd: c(&process.cwd)?,
        })
    }

    /// In the clone's child: the program's working directory and signal state, then its execve. Returns
    /// only when it couldn't run: the status a shell gives (127 for a program not there, 126 else) and
    /// what failed.
    fn exec(&self) -> (u8, Failure) {
        if let Err(errno) = rustix::process::chdir(self.cwd.as_c_str()) {
            return (
                CANNOT_RUN,
                Failure::of("the program's working directory")(errno),
            );
        }
        if let Err(errno) = sys::default_signals() {
            return (CANNOT_RUN, Failure::of("the program's signal state")(errno));
        }
        match sys::execve_each(&self.paths, &self.argv, &self.envp) {
            e @ (libc::ENOENT | libc::ENOTDIR) => (
                NOT_FOUND,
                Failure::of("the program isn't in the container")(
                    rustix::io::Errno::from_raw_os_error(e),
                ),
            ),
            e => (
                CANNOT_RUN,
                Failure::of("execve")(rustix::io::Errno::from_raw_os_error(e)),
            ),
        }
    }
}

/// The kinds of namespace an exec joins: those the container's spec gives it, as crun's exec joins
/// them. One it made is its own; one it joined by path may be the seed's own, which is left out, since
/// joining one's own user namespace is refused (`userns_install`) and an unprivileged intermediate may
/// not join the host's others. A kind Hex doesn't make (`time`) isn't joined, as `hex oci exec` doesn't.
fn joins(namespaces: &[Namespace]) -> Result<Ns, OciError> {
    let mut joins = Ns::empty();
    for ns in namespaces {
        let (kind, proc_name) = match ns.kind.as_str() {
            "user" => (Ns::USER, "user"),
            "ipc" => (Ns::INTER_PROCESS_COMMUNICATION, "ipc"),
            "uts" => (Ns::HOST_NAME_AND_NIS_DOMAIN_NAME, "uts"),
            "network" => (Ns::NETWORK, "net"),
            "pid" => (Ns::PROCESS_ID, "pid"),
            "cgroup" => (Ns::CONTROL_GROUP, "cgroup"),
            "mount" => (Ns::MOUNT, "mnt"),
            _ => continue,
        };
        if let Some(path) = &ns.path {
            let theirs = rustix::fs::stat(path.as_str())
                .map_err(|e| OciError::os("reading a namespace the container joined", e))?;
            let ours = rustix::fs::stat(format!("/proc/self/ns/{proc_name}"))
                .map_err(|e| OciError::os("reading the seed's own namespace", e))?;
            if (theirs.st_dev, theirs.st_ino) == (ours.st_dev, ours.st_ino) {
                continue;
            }
        }
        joins |= kind;
    }
    Ok(joins)
}

/// What [`exec_direct`] did.
pub enum Direct {
    /// The program runs, the caller's own child: its pid and pidfd.
    Started(u32, OwnedFd),
    /// Nothing was started: this thread may not choose the container's pid namespace (a rootless node),
    /// or there is no vfork-style clone here (an architecture without the trampoline, a kernel or sandbox
    /// without clone3 into a cgroup), or the container has no cgroup to be born in. The init's pidfd
    /// comes back, for the caller to hand the exec to a seed instead.
    Unable(OwnedFd),
}

/// The pid namespace the calling thread's children are born in is the container's for one clone, then the
/// process's own again: `setns(2)` changes it for the calling thread alone.
fn own_pid_namespace() -> Result<&'static OwnedFd, OciError> {
    static OWN: std::sync::OnceLock<OwnedFd> = std::sync::OnceLock::new();
    if let Some(own) = OWN.get() {
        return Ok(own);
    }
    let own = rustix::fs::open(
        "/proc/self/ns/pid",
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|e| OciError::os("opening the node's own pid namespace", e))?;
    let _ = OWN.set(own);
    OWN.get()
        .ok_or_else(|| OciError::Broken("the node's pid namespace wasn't kept".to_owned()))
}

std::thread_local! {
    /// The stack a vfork-style clone's child runs on, per thread that clones ([`sys::ChildStack`]); and
    /// whether this thread's children's pid namespace couldn't be set back after a clone, which makes it
    /// hand every later exec to a seed rather than clone into a namespace it can't be sure of.
    static STACK: std::cell::RefCell<Option<sys::ChildStack>> = const { std::cell::RefCell::new(None) };
    static UNSURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The calling thread pinned to the CPU it runs on, with the CPUs it may run on kept to give back.
struct Pin {
    mask: rustix::thread::CpuSet,
}

impl Pin {
    /// Pins the calling thread to its CPU; `None` (and nothing changed) where its mask can't be read or
    /// set, as on a CPU a cpuset no longer allows.
    fn here() -> Option<Self> {
        let mask = rustix::thread::sched_getaffinity(None).ok()?;
        let mut here = rustix::thread::CpuSet::new();
        here.set(rustix::thread::sched_getcpu());
        rustix::thread::sched_setaffinity(None, &here).ok()?;
        Some(Self { mask })
    }

    /// Gives the calling thread back the CPUs it had; where the kernel refuses them (a cpuset changed
    /// since), every CPU, which its cpuset then narrows, so no thread or program stays held to one CPU.
    /// A system call alone on memory made before, so a vfork-style clone's child may make it.
    fn release(&self) {
        if rustix::thread::sched_setaffinity(None, &self.mask).is_err() {
            let mut all = rustix::thread::CpuSet::new();
            for cpu in 0..rustix::thread::CpuSet::MAX_CPU {
                all.set(cpu);
            }
            let _ = rustix::thread::sched_setaffinity(None, &all);
        }
    }
}

/// Exec in a container straight from the calling thread (a thread of the node agent), as posix_spawn
/// starts a program: the thread's children's pid namespace set to the container init `init`'s, then one
/// `clone3(CLONE_VM | CLONE_VFORK | CLONE_INTO_CGROUP | CLONE_PIDFD)` ([`sys::clone_vfork`]). The program
/// is born in the container's cgroup and pid namespace as the caller's own child, shares the caller's
/// memory until its execve (so nothing of the caller is copied, and nothing torn down), joins the
/// container's other namespaces, becomes its process and execs, making system calls alone on what
/// [`prepare`] made. `stdio` become its standard streams and `cgroup` (the container's cgroup directory,
/// which the caller holds open) is where it is born. No process but the program, no seed, no `hex`
/// exec. Where it can't, it says so ([`Direct::Unable`]) and starts nothing.
pub fn exec_direct(
    req: &Request,
    stdio: [BorrowedFd<'_>; 3],
    init: OwnedFd,
    cgroup: Option<BorrowedFd<'_>>,
    specs: &mut Specs,
) -> Result<Direct, OciError> {
    if UNSURE.with(std::cell::Cell::get) {
        return Ok(Direct::Unable(init));
    }
    let Some(cgroup) = cgroup else {
        return Ok(Direct::Unable(init));
    };
    let p = prepare(req, init, specs)?;
    let blocked = sys::block_signals()?;
    let set = p.joins.contains(Ns::PROCESS_ID);
    if set {
        match rustix::thread::move_into_thread_name_spaces(p.init.as_fd(), Ns::PROCESS_ID) {
            Ok(()) => {}
            Err(rustix::io::Errno::PERM) => {
                drop(blocked);
                return Ok(Direct::Unable(p.init));
            }
            Err(e) => {
                return Err(OciError::os("choosing the container's pid namespace", e));
            }
        }
    }
    let rest = p.joins.difference(Ns::PROCESS_ID);
    // The child starts on this thread's CPU: the thread is pinned to it for the clone, so the child
    // inherits the pin and runs as soon as the thread blocks in the vfork, rather than wait for an idle
    // CPU the kernel's fork balancing picks to wake. Measured on fedora (bench/competitive/probes/
    // exec_pin.md): the child first runs 18 µs after its fork pinned and 86 µs unpinned (p50s), and
    // exec's p50 falls from 1.186–1.227 ms to 1.123–1.141. The child gives the pin back before anything
    // else, so the program never runs pinned, and the thread does after the clone.
    let pin = Pin::here();
    let child = || {
        if let Some(pin) = &pin {
            pin.release();
        }
        program(
            &p.becoming,
            &p.launch,
            stdio,
            None,
            None,
            (p.init.as_fd(), rest),
        )
    };
    let cloned = STACK.with(|stack| -> Result<_, OciError> {
        let mut stack = stack
            .try_borrow_mut()
            .map_err(|_| OciError::Broken("a thread's child stack is in use".to_owned()))?;
        if stack.is_none() {
            *stack = Some(sys::ChildStack::map()?);
        }
        match stack.as_ref() {
            Some(stack) => sys::clone_vfork(cgroup, stack, &child),
            None => Ok(None),
        }
    });
    if let Some(pin) = &pin {
        pin.release();
    }
    // Whatever the clone did, this thread's children are born in the node's own pid namespace again.
    let reset = if set {
        own_pid_namespace().and_then(|own| sys::setns(own.as_fd(), libc::CLONE_NEWPID))
    } else {
        Ok(())
    };
    drop(blocked);
    if let Err(e) = reset {
        UNSURE.with(|unsure| unsure.set(true));
        // A program started into a namespace this thread can't leave must not run unwatched: end it.
        if let Ok(Some((_, pidfd))) = &cloned {
            let _ = rustix::process::pidfd_send_signal(pidfd, rustix::process::Signal::KILL);
        }
        return Err(e);
    }
    match cloned? {
        Some((pid, pidfd)) => {
            let pid = u32::try_from(pid)
                .map_err(|_| OciError::os("clone3 the program", "a pid out of range"))?;
            Ok(Direct::Started(pid, pidfd))
        }
        None => Ok(Direct::Unable(p.init)),
    }
}

/// Serves one request: prepares it, then starts the program by a single clone where the seed may, or
/// through an intermediate where it may not, and returns the answer with the program's pidfd. A
/// refusal the seed makes is returned in its place, and the seed stays serving.
fn serve_one(
    buf: &[u8],
    size: usize,
    fds: Vec<OwnedFd>,
    pid_for: &mut PidFor,
    specs: &mut Specs,
) -> (Answer, Option<OwnedFd>) {
    let bytes = buf.get(..size.min(buf.len())).unwrap_or_default();
    let req = match seed::read_request(bytes, size) {
        Ok(req) => req,
        Err(fault) => return refused(Failed::Request, fault.to_string()),
    };
    let [stdin, stdout, stderr, init]: [OwnedFd; seed::REQUEST_FDS] = match fds.try_into() {
        Ok(four) => four,
        Err(got) => {
            return refused(
                Failed::Request,
                format!(
                    "an exec carries {} descriptors, not {} (its stdio and the init's pidfd)",
                    got.len(),
                    seed::REQUEST_FDS
                ),
            );
        }
    };
    let stdio = [stdin, stdout, stderr];
    let prepared = match prepare(&req, init, specs).and_then(|mut p| {
        p.cgroup = open_cgroup(req.cgroup.as_deref())?;
        Ok(p)
    }) {
        Ok(prepared) => prepared,
        Err(e) => return refused(Failed::Start, e.to_string()),
    };
    match single_clone(prepared, stdio, pid_for) {
        Ok(Single::Started(pid, pidfd)) => (Answer::Started { pid }, Some(pidfd)),
        Ok(Single::Unable(prepared, stdio)) => through_intermediate(*prepared, stdio, pid_for),
        Err((failed, e)) => refused(failed, e.to_string()),
    }
}

/// What [`single_clone`] did.
enum Single {
    /// The program runs, the agent's child: its pid and pidfd.
    Started(u32, OwnedFd),
    /// Nothing was started: the seed may not choose the container's pid namespace (a rootless seed),
    /// or this kernel has no `clone3` into a cgroup. What it was given comes back for the intermediate.
    Unable(Box<Prepared>, [OwnedFd; 3]),
}

/// Starts the program by one clone of the seed's: its children's pid namespace set to the container's,
/// then `clone3(CLONE_PARENT | CLONE_INTO_CGROUP | CLONE_PIDFD)`. The program joins the container's
/// other namespaces itself, all at once by the init's pidfd.
fn single_clone(
    p: Prepared,
    stdio: [OwnedFd; 3],
    pid_for: &mut PidFor,
) -> Result<Single, (Failed, OciError)> {
    if p.joins.contains(Ns::PROCESS_ID) {
        match pid_for.set(p.init.as_fd()) {
            Ok(()) => {}
            // `pidns_install` wants CAP_SYS_ADMIN in the seed's own user namespace: a rootless seed
            // hasn't it. The intermediate gains it by joining the container's user namespace first.
            Err(rustix::io::Errno::PERM) => return Ok(Single::Unable(Box::new(p), stdio)),
            Err(e) => {
                return Err((
                    Failed::Start,
                    OciError::os("choosing the container's pid namespace", e),
                ));
            }
        }
    } else {
        pid_for.reset().map_err(|e| (Failed::Seed, e))?;
    }
    let cloned =
        sys::clone_sibling(p.cgroup.as_ref().map(AsFd::as_fd)).map_err(|e| (Failed::Start, e))?;
    match cloned {
        None => Ok(Single::Unable(Box::new(p), stdio)),
        Some(sys::Sibling::Child) => {
            // The program. It returns here only if it couldn't join, become the container's process
            // or exec; its status then leaves through its exit, which the agent's pidfd reads. Born in
            // the container's cgroup and pid namespace, it joins the rest, the cgroup namespace with
            // them.
            let Prepared {
                becoming,
                launch,
                cgroup,
                init,
                joins,
            } = p;
            drop(cgroup);
            let rest = joins.difference(Ns::PROCESS_ID);
            let [i, o, e] = &stdio;
            let (code, failed) = program(
                &becoming,
                &launch,
                [i.as_fd(), o.as_fd(), e.as_fd()],
                None,
                None,
                (init.as_fd(), rest),
            );
            let _ = writeln!(
                std::io::stderr().lock(),
                "hex oci exec: {}",
                OciError::from(failed)
            );
            sys::exit_child(code);
        }
        Some(sys::Sibling::Parent { pid, pidfd }) => match u32::try_from(pid) {
            Ok(pid) => Ok(Single::Started(pid, pidfd)),
            Err(_) => {
                // A clone never returns a negative pid; were it to, the program must not run
                // unwatched: end it (the agent's sweep reaps it, a zombie in a box cgroup it doesn't own).
                let _ = rustix::process::pidfd_send_signal(&pidfd, rustix::process::Signal::KILL);
                Err((
                    Failed::Start,
                    OciError::os("clone3 the program", "a pid out of range"),
                ))
            }
        },
    }
}

/// Starts the program through an intermediate (see the module's docs): forks it, lets it answer over
/// an internal socket, reaps it (so the program has reparented to the agent), and returns its answer.
fn through_intermediate(
    p: Prepared,
    stdio: [OwnedFd; 3],
    pid_for: &mut PidFor,
) -> (Answer, Option<OwnedFd>) {
    // The intermediate is born in the seed's own pid namespace; it chooses the container's itself.
    if let Err(e) = pid_for.reset() {
        return refused(Failed::Seed, e.to_string());
    }
    // The internal socket on which the intermediate answers the seed. The seed is not a child
    // subreaper (it never set the attribute), so the program reparents past it to the agent.
    let (seed_end, intermediate_end) = match socketpair() {
        Ok(pair) => pair,
        Err(e) => return refused(Failed::Seed, e.to_string()),
    };
    match sys::fork() {
        Err(e) => refused(Failed::Seed, e.to_string()),
        Ok(sys::Fork::Child) => {
            // The intermediate. It answers the seed (not the agent) and exits; the seed's `waitpid`
            // below turns that exit into the program's adoption by the agent before the agent is told.
            drop(seed_end);
            let (answer, pidfd) = match start_program(p, stdio) {
                Ok((pid, pidfd)) => (Answer::Started { pid }, Some(pidfd)),
                Err((failed, e)) => (
                    Answer::Refused {
                        failed,
                        why: e.to_string(),
                    },
                    None,
                ),
            };
            if let Err(e) = send_answer(
                intermediate_end.as_fd(),
                &answer,
                pidfd.as_ref().map(AsFd::as_fd),
            ) {
                let _ = writeln!(std::io::stderr().lock(), "hex __oci-seed: {e}");
            }
            let code = u8::from(!matches!(answer, Answer::Started { .. }));
            // `_exit`, not `std::process::exit`: a forked child must not run the seed's `atexit`
            // handlers or flush its buffers a second time ([`sys::exit_child`]).
            sys::exit_child(code);
        }
        Ok(sys::Fork::Parent(pid)) => {
            drop((intermediate_end, stdio, p));
            let answered = recv_answer(seed_end.as_fd());
            // Reap the intermediate, so the program has reparented to the agent before it is forwarded.
            let _ = sys::waitpid(pid);
            match answered {
                Ok((answer, pidfd)) => (answer, pidfd),
                Err(e) => refused(
                    Failed::Seed,
                    format!("the exec intermediate ended without answering: {e}"),
                ),
            }
        }
    }
}

/// A refusal the seed makes itself, with no descriptor.
fn refused(failed: Failed, why: String) -> (Answer, Option<OwnedFd>) {
    (Answer::Refused { failed, why }, None)
}

/// The intermediate: joins the container's namespaces by the init's pidfd (the user namespace first,
/// which grants it the rest; the cgroup namespace is left for the program, once it is in the cgroup) and
/// clones the program into them (the same work as [`super::exec`]'s helper), returning the program's
/// pid and the pidfd its clone made. Returns in the intermediate only; the program (the clone's child)
/// execs and never returns here.
fn start_program(p: Prepared, stdio: [OwnedFd; 3]) -> Result<(u32, OwnedFd), (Failed, OciError)> {
    let start = |e| (Failed::Start, e);
    let Prepared {
        becoming,
        launch,
        cgroup,
        init,
        joins,
    } = p;
    rustix::thread::move_into_thread_name_spaces(init.as_fd(), joins.difference(Ns::CONTROL_GROUP))
        .map_err(|e| start(OciError::os("joining the container's namespaces", e)))?;
    // A birth in the cgroup is checked against the credentials the clone runs with, which a joined
    // user namespace has changed; there the program joins the cgroup by migration instead (as the
    // shim does).
    let in_userns = joins.contains(Ns::USER);
    // Born in the cgroup with a pidfd where it can be; otherwise forked, to migrate in and have its
    // pidfd opened from its pid.
    let born = match &cgroup {
        Some(dir) if !in_userns => sys::fork_into_cgroup_pidfd(dir.as_fd()).map_err(start)?,
        _ => None,
    };
    // On the migrate path (rootless, or a kernel without clone-into-cgroup) the program joins the box
    // cgroup itself after the fork, so the intermediate must report it started only once it has — else a
    // delete that races the exec would `cgroup.kill` the box before the program is a member, miss it,
    // and the box would never empty (HEX-RT-7). A program born in the cgroup is a member already, so it
    // needs no such wait. The program signals by one byte on this pipe once it has joined.
    let migrating = born.is_none() && cgroup.is_some();
    let joined_pipe = if migrating {
        Some(
            rustix::pipe::pipe_with(PipeFlags::CLOEXEC)
                .map_err(|e| start(OciError::os("the program's cgroup-join pipe", e)))?,
        )
    } else {
        None
    };
    let (fork, pidfd, procs) = match (born, cgroup) {
        (Some((fork, pidfd)), _) => (fork, pidfd, None),
        (None, cgroup) => {
            // The cgroup.procs the program joins by, opened by its directory before the fork.
            let procs = cgroup
                .map(|dir| {
                    rustix::fs::openat(
                        &dir,
                        "cgroup.procs",
                        rustix::fs::OFlags::WRONLY | rustix::fs::OFlags::CLOEXEC,
                        rustix::fs::Mode::empty(),
                    )
                    .map_err(|e| start(OciError::os("opening the container's cgroup", e)))
                })
                .transpose()?;
            (sys::fork().map_err(start)?, None, procs)
        }
    };
    match fork {
        sys::Fork::Child => {
            // The program. It returns here only if it couldn't become the container's process or
            // exec; its status then leaves through the process's exit, which the agent's pidfd reads.
            let joined = joined_pipe.map(|(_read, write)| write);
            let cgroup_ns = joins.intersection(Ns::CONTROL_GROUP);
            let [i, o, e] = &stdio;
            let (code, failed) = program(
                &becoming,
                &launch,
                [i.as_fd(), o.as_fd(), e.as_fd()],
                procs.as_ref().map(AsFd::as_fd),
                joined.as_ref().map(AsFd::as_fd),
                (init.as_fd(), cgroup_ns),
            );
            let _ = writeln!(
                std::io::stderr().lock(),
                "hex oci exec: {}",
                OciError::from(failed)
            );
            sys::exit_child(code);
        }
        sys::Fork::Parent(pid) => {
            drop((procs, stdio, init));
            // On the migrate path, wait for the program's one byte, meaning it has joined the box
            // cgroup; if the pipe closes with no byte, the program ended first, so reap it and refuse.
            if let Some((read, _write)) = joined_pipe {
                let mut byte = [0u8; 1];
                let joined = loop {
                    match rustix::io::read(&read, &mut byte) {
                        Ok(1) => break true,
                        Ok(_) => break false,
                        Err(rustix::io::Errno::INTR) => continue,
                        Err(_) => break false,
                    }
                };
                if !joined {
                    let _ = sys::waitpid(pid);
                    return Err(start(OciError::os(
                        "starting the program",
                        "it ended before it joined the container's cgroup",
                    )));
                }
            }
            let pidfd = match pidfd {
                Some(pidfd) => pidfd,
                None => {
                    let raw = rustix::process::Pid::from_raw(pid).ok_or_else(|| {
                        start(OciError::os("clone", "the child's pid isn't a pid"))
                    })?;
                    rustix::process::pidfd_open(raw, rustix::process::PidfdFlags::empty())
                        .map_err(|e| start(OciError::os("opening the program's pidfd", e)))?
                }
            };
            let pid = u32::try_from(pid)
                .map_err(|_| start(OciError::os("clone", "a pid out of range")))?;
            Ok((pid, pidfd))
        }
    }
}

/// The program (the clone's child): its stdio set to the ones the caller handed in, migrated into the
/// container's cgroup when it wasn't born there, then into the namespaces `joins` names of the init's
/// (the single clone's all but pid; the intermediate's program's the cgroup namespace alone, once it is
/// in the cgroup), then the container's process become and the program exec'd. System calls alone on
/// what was made before the clone, so it allocates nothing. Returns the status to exit with and what
/// failed only when a step did.
fn program(
    becoming: &init::Becoming,
    launch: &Launch,
    stdio: [BorrowedFd<'_>; 3],
    procs: Option<BorrowedFd<'_>>,
    joined: Option<BorrowedFd<'_>>,
    (init, joins): (BorrowedFd<'_>, Ns),
) -> (u8, Failure) {
    let setup = || -> Result<(), Failure> {
        let [stdin, stdout, stderr] = stdio;
        rustix::stdio::dup2_stdin(stdin).map_err(Failure::of("the program's stdin"))?;
        rustix::stdio::dup2_stdout(stdout).map_err(Failure::of("the program's stdout"))?;
        rustix::stdio::dup2_stderr(stderr).map_err(Failure::of("the program's stderr"))?;
        if let Some(procs) = procs {
            let n = rustix::io::write(procs, b"0")
                .map_err(Failure::of("joining the container's cgroup"))?;
            if n != 1 {
                return Err(Failure::of("joining the container's cgroup")(
                    rustix::io::Errno::IO,
                ));
            }
        }
        // Now a member of the box cgroup (born there, or just migrated in): tell the intermediate, so it
        // reports the program started only once the box's `cgroup.kill` would reach it. Before the
        // program's own setup, so the report can't beat the membership it stands for.
        if let Some(joined) = joined {
            let n = rustix::io::write(joined, b"1")
                .map_err(Failure::of("signalling the container's cgroup was joined"))?;
            if n != 1 {
                return Err(Failure::of("signalling the container's cgroup was joined")(
                    rustix::io::Errno::IO,
                ));
            }
        }
        if !joins.is_empty() {
            rustix::thread::move_into_thread_name_spaces(init, joins)
                .map_err(Failure::of("joining the container's namespaces"))?;
        }
        becoming.apply()
    };
    if let Err(failed) = setup() {
        return (CANNOT_RUN, failed);
    }
    launch.exec()
}

/// What [`recv_request`] read.
pub enum Received {
    /// The peer closed the socket.
    Closed,
    /// A request: its whole size (larger than the buffer when the message was cut), and the
    /// descriptors it carried.
    Message { size: usize, stdio: Vec<OwnedFd> },
}

/// Sends one exec request on `socket`, its descriptors riding with it (`SCM_RIGHTS`): the program's
/// stdin, stdout and stderr, then the container init's pidfd. The agent's end of the agent↔seed socket
/// (`hex-node`'s pool) sends this.
pub fn send_request(
    socket: BorrowedFd<'_>,
    req: &Request,
    fds: [BorrowedFd<'_>; seed::REQUEST_FDS],
) -> Result<(), OciError> {
    let bytes = seed::request(req)
        .map_err(|f| OciError::Broken(format!("encoding an exec request: {f}")))?;
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(seed::REQUEST_FDS))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    if !control.push(SendAncillaryMessage::ScmRights(&fds)) {
        return Err(OciError::os(
            "sending an exec request's descriptors",
            "no room for the descriptors",
        ));
    }
    send(socket, &bytes, &mut control)
}

/// Reads one exec request into `buf` with the descriptors that rode with it, or `Closed` at EOF.
pub fn recv_request(socket: BorrowedFd<'_>, buf: &mut [u8]) -> Result<Received, OciError> {
    loop {
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(seed::REQUEST_FDS))];
        let mut control = RecvAncillaryBuffer::new(&mut space);
        let msg = match rustix::net::recvmsg(
            socket,
            &mut [IoSliceMut::new(buf)],
            &mut control,
            RecvFlags::CMSG_CLOEXEC | RecvFlags::TRUNC,
        ) {
            Ok(msg) => msg,
            Err(rustix::io::Errno::INTR) => continue,
            Err(e) => return Err(OciError::os("reading the seed's socket", e)),
        };
        let stdio = descriptors(&mut control);
        if msg.bytes == 0 && stdio.is_empty() {
            return Ok(Received::Closed);
        }
        // More descriptors than a request carries, or a message larger than the buffer: refused as a
        // bad request, not read as a good one (CLAUDE.md §3). The size is kept so the parser refuses
        // it by type.
        if msg.flags.contains(ReturnFlags::CTRUNC) {
            return Err(OciError::Broken(
                "an exec request carried more descriptors than it may".to_owned(),
            ));
        }
        return Ok(Received::Message {
            size: msg.bytes,
            stdio,
        });
    }
}

/// Sends one answer on `socket`, the program's pidfd riding with it when it started (`SCM_RIGHTS`).
pub fn send_answer(
    socket: BorrowedFd<'_>,
    answer: &Answer,
    pidfd: Option<BorrowedFd<'_>>,
) -> Result<(), OciError> {
    let bytes =
        seed::answer(answer).map_err(|f| OciError::Broken(format!("encoding an answer: {f}")))?;
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(seed::STARTED_FDS))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    let fds: Vec<BorrowedFd<'_>> = pidfd.into_iter().collect();
    if !fds.is_empty() && !control.push(SendAncillaryMessage::ScmRights(&fds)) {
        return Err(OciError::os(
            "sending the program's pidfd",
            "no room for the descriptor",
        ));
    }
    send(socket, &bytes, &mut control)
}

/// Reads one answer from `socket` and the pidfd that may ride with it (`Started` carries one). The
/// agent's end reading a seed, and the seed reading its intermediate.
pub fn recv_answer(socket: BorrowedFd<'_>) -> Result<(Answer, Option<OwnedFd>), OciError> {
    let mut buf = [0u8; seed::MAX_ANSWER];
    loop {
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(seed::STARTED_FDS))];
        let mut control = RecvAncillaryBuffer::new(&mut space);
        let msg = match rustix::net::recvmsg(
            socket,
            &mut [IoSliceMut::new(&mut buf)],
            &mut control,
            RecvFlags::CMSG_CLOEXEC | RecvFlags::TRUNC,
        ) {
            Ok(msg) => msg,
            Err(rustix::io::Errno::INTR) => continue,
            Err(e) => return Err(OciError::os("reading an answer", e)),
        };
        let mut fds = descriptors(&mut control);
        if msg.bytes == 0 && fds.is_empty() {
            return Err(OciError::Broken(
                "the peer closed before answering".to_owned(),
            ));
        }
        if msg.flags.contains(ReturnFlags::CTRUNC) {
            return Err(OciError::Broken(
                "an answer carried more descriptors than it may".to_owned(),
            ));
        }
        let bytes = buf.get(..msg.bytes.min(buf.len())).unwrap_or_default();
        let answer = seed::read_answer(bytes, msg.bytes, fds.len())
            .map_err(|f| OciError::Broken(format!("decoding an answer: {f}")))?;
        return Ok((answer, fds.drain(..).next()));
    }
}

/// The descriptors an ancillary buffer carried.
fn descriptors(control: &mut RecvAncillaryBuffer<'_>) -> Vec<OwnedFd> {
    control
        .drain()
        .filter_map(|m| match m {
            RecvAncillaryMessage::ScmRights(fds) => Some(fds),
            _ => None,
        })
        .flatten()
        .collect()
}

/// Sends `bytes` as one record with `control`'s descriptors, retrying an interrupted call.
fn send(
    socket: BorrowedFd<'_>,
    bytes: &[u8],
    control: &mut SendAncillaryBuffer<'_, '_, '_>,
) -> Result<(), OciError> {
    loop {
        match rustix::net::sendmsg(socket, &[IoSlice::new(bytes)], control, SendFlags::NOSIGNAL) {
            Ok(_) => return Ok(()),
            Err(rustix::io::Errno::INTR) => continue,
            Err(e) => return Err(OciError::os("sending on the seed's socket", e)),
        }
    }
}

/// A socketpair of `SOCK_SEQPACKET`, as the agent makes to reach a seed: one end stays with the agent,
/// the other is given to the seed. Close-on-exec on both; the seed's is cleared before it is passed.
pub fn socketpair() -> Result<(OwnedFd, OwnedFd), OciError> {
    rustix::net::socketpair(
        AddressFamily::UNIX,
        SocketType::SEQPACKET,
        SocketFlags::CLOEXEC,
        None,
    )
    .map_err(|e| OciError::os("making the seed's socket", e))
}

fn env_fd(name: &str) -> Result<RawFd, OciError> {
    std::env::var(name)
        .map_err(|_| OciError::Broken(format!("{name} not set for the seed")))?
        .parse()
        .map_err(|_| OciError::Broken(format!("{name} isn't a file descriptor")))
}

/// Clears close-on-exec on a socket the agent passes to a seed, so the seed inherits it across the
/// re-exec, as the init's inherited pipes are ([`super::init`]).
pub fn clear_cloexec(fd: &OwnedFd) -> Result<(), OciError> {
    rustix::io::fcntl_setfd(fd, rustix::io::FdFlags::empty())
        .map_err(|e| OciError::os("clearing close-on-exec on the seed's socket", e))
}
