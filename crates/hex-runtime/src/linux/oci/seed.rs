//! `hex __oci-seed`: a prepared, single-threaded process the node agent keeps and reuses, so that an
//! exec into a container spawns no `hex` process of its own and re-execs no `hex` binary per call
//! (decision 3 of docs/design/runtime-architecture.md, goal 3 plan step 2). The agent hands each exec
//! to a seed over a `SOCK_SEQPACKET` socket: the request's bytes ([`hex_core::seed`]) with the
//! program's stdin, stdout and stderr as descriptors ([`send_request`]). The seed forks a short-lived
//! intermediate that joins the container's namespaces and clones the program into the container's
//! cgroup, hands the program's pidfd back, and exits; the program, orphaned by the intermediate, is
//! adopted by the agent (a child subreaper, `hex-node`'s supervisor), which owns its pidfd. So the only
//! process left beside the program is none, and no `hex` binary was exec'd.
//!
//! **Why an intermediate, not the plan's single `clone3(... | CLONE_PARENT)`.** `clone3` refuses
//! `CLONE_PARENT` on Linux (EINVAL, measured on cachyos 7.2.9 and fedora 6.19; see
//! `docs/research/runtime/seed-clone-parent.md`), and a process can place only its *children* in a pid
//! namespace it joins with `setns`, never itself; a reused seed can't join namespaces itself either (a
//! user-namespace join is irreversible, a pid-namespace one governs only its future children). So a
//! per-exec intermediate joins the namespaces and clones the program, then exits; the program reaches
//! the agent by the same subreaper adoption the `hex __oci-init` shim already uses (decision 2). The
//! intermediate is a fork of the seed, no exec, gone before the program runs, so "the program alone"
//! and "no `hex` exec per call" both hold.
//!
//! **The ordering that makes the adoption race-free.** The intermediate sends its answer to the seed
//! over an internal socketpair, not to the agent. The seed reaps the intermediate (`waitpid`) — so the
//! program has reparented to the agent — *before* it forwards the answer to the agent. The agent
//! therefore learns the program's pidfd only once the program is its own child, so its `waitid(P_PIDFD)`
//! can reap it (it would be `ECHILD` on a program still parented to the intermediate).
//!
//! The namespace joins, the clone and the `become_process` are the same ones `hex __oci-exec` makes
//! ([`super::exec`], [`super::init`]); only the plumbing — a socket in place of an inherited request
//! and a waited status — differs.

use std::io::{IoSlice, IoSliceMut, Write};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd, RawFd};
use std::path::Path;

use hex_core::seed::{self, Answer, Failed, Request};
use rustix::net::{
    AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags,
    SendAncillaryBuffer, SendAncillaryMessage, SendFlags, SocketFlags, SocketType,
};
use rustix::pipe::PipeFlags;

use crate::linux::oci::spec::{Process, Seccomp, Spec};
use crate::linux::oci::{OciError, exec, init, sys};

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
    let mut buf = vec![0u8; seed::MAX_REQUEST];
    loop {
        match recv_request(socket.as_fd(), &mut buf)? {
            // The agent closed the socket: nothing more will come, so the seed is done.
            Received::Closed => return Ok(()),
            Received::Message { size, stdio } => {
                let (answer, pidfd) = serve_one(&buf, size, stdio);
                send_answer(socket.as_fd(), &answer, pidfd.as_ref().map(AsFd::as_fd))?;
            }
        }
    }
}

/// Serves one request: forks the intermediate, which starts the program and answers over the internal
/// socket, waits for it (so the program has reparented to the agent), and returns the answer to forward
/// with the program's pidfd that rides with it. A refusal the seed makes before or around the fork is
/// returned in its place, and the seed stays serving.
fn serve_one(buf: &[u8], size: usize, stdio: Vec<OwnedFd>) -> (Answer, Option<OwnedFd>) {
    let bytes = buf.get(..size.min(buf.len())).unwrap_or_default();
    let req = match seed::read_request(bytes, size) {
        Ok(req) => req,
        Err(fault) => return refused(Failed::Request, fault.to_string()),
    };
    let stdio: [OwnedFd; seed::REQUEST_FDS] = match stdio.try_into() {
        Ok(three) => three,
        Err(got) => {
            return refused(
                Failed::Request,
                format!(
                    "an exec carries {} stdio descriptors, not {}",
                    seed::REQUEST_FDS,
                    got.len()
                ),
            );
        }
    };
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
            let (answer, pidfd) = match start_program(&req, stdio) {
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
            drop((intermediate_end, stdio));
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

/// The intermediate: joins the container's namespaces and cgroup and clones the program into them (the
/// same work as [`super::exec`]'s helper), returning the program's pid and the pidfd its clone made.
/// Returns in the intermediate only; the program (the clone's child) execs and never returns here.
fn start_program(
    req: &Request,
    stdio: [OwnedFd; seed::REQUEST_FDS],
) -> Result<(u32, OwnedFd), (Failed, OciError)> {
    let start = |e| (Failed::Start, e);
    let spec = Spec::parse(
        &std::fs::read(Path::new(&req.bundle).join("config.json"))
            .map_err(|e| start(OciError::os("reading config.json", e)))?,
    )
    .map_err(start)?;
    // Reuse `hex __oci-exec`'s override logic, one code path (CLAUDE.md §8).
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
    let process = exec::process_for(spec.process.unwrap_or_default(), &request);
    let seccomp = spec.linux.seccomp.clone();

    let cgroup = req
        .cgroup
        .as_deref()
        .map(|dir| exec::open_cgroup(Path::new(dir)))
        .transpose()
        .map_err(start)?;
    let joins = exec::namespaces_to_join(req.init_pid).map_err(start)?;
    // A birth in the cgroup is checked against the credentials the clone runs with, which a joined
    // user namespace has changed; there the program joins the cgroup by migration instead (as the
    // shim does), and the cgroup namespace is the child's to join once it is in the cgroup.
    let in_userns = joins.iter().any(|(kind, _, _)| *kind == "user");
    let mut cgroup_ns = None;
    for (kind, flag, fd) in joins {
        if kind == "cgroup" {
            cgroup_ns = Some(fd);
            continue;
        }
        sys::setns(fd.as_fd(), flag).map_err(start)?;
    }

    // Born in the cgroup with a pidfd where it can be; otherwise forked, to migrate in and have its
    // pidfd opened from its pid.
    let born = match &cgroup {
        Some((dir, _)) if !in_userns => sys::fork_into_cgroup_pidfd(dir.as_fd()).map_err(start)?,
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
        (None, cgroup) => (
            sys::fork().map_err(start)?,
            None,
            cgroup.map(|(_, procs)| procs),
        ),
    };
    match fork {
        sys::Fork::Child => {
            // The program. It returns here only if it couldn't become the container's process or
            // exec; its status then leaves through the process's exit, which the agent's pidfd reads.
            let joined = joined_pipe.map(|(_read, write)| write);
            let (code, e) = program(&process, seccomp.as_ref(), procs, cgroup_ns, stdio, joined);
            let _ = writeln!(std::io::stderr().lock(), "hex oci exec: {e}");
            sys::exit_child(code);
        }
        sys::Fork::Parent(pid) => {
            drop((procs, cgroup_ns, stdio));
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
/// container's cgroup when it wasn't born there, into the container's cgroup namespace, then the
/// container's process become and the program exec'd. Returns the status to exit with and why only
/// when one of those failed.
fn program(
    process: &Process,
    seccomp: Option<&Seccomp>,
    procs: Option<OwnedFd>,
    cgroup_ns: Option<OwnedFd>,
    stdio: [OwnedFd; seed::REQUEST_FDS],
    joined: Option<OwnedFd>,
) -> (u8, OciError) {
    let setup = || -> Result<(), OciError> {
        let [stdin, stdout, stderr] = stdio;
        rustix::stdio::dup2_stdin(&stdin).map_err(|e| OciError::os("the program's stdin", e))?;
        rustix::stdio::dup2_stdout(&stdout).map_err(|e| OciError::os("the program's stdout", e))?;
        rustix::stdio::dup2_stderr(&stderr).map_err(|e| OciError::os("the program's stderr", e))?;
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
        // Now a member of the box cgroup (born there, or just migrated in): tell the intermediate, so it
        // reports the program started only once the box's `cgroup.kill` would reach it. Before the
        // program's own setup, so the report can't beat the membership it stands for.
        if let Some(joined) = &joined {
            let n = rustix::io::write(joined, b"1")
                .map_err(|e| OciError::os("signalling the container's cgroup was joined", e))?;
            if n != 1 {
                return Err(OciError::os(
                    "signalling the container's cgroup was joined",
                    "a short write",
                ));
            }
        }
        if let Some(ns) = cgroup_ns {
            sys::setns(ns.as_fd(), libc::CLONE_NEWCGROUP)?;
        }
        init::become_process(process, seccomp)
    };
    if let Err(e) = setup() {
        return (CANNOT_RUN, e);
    }
    let program = match process.args.first() {
        Some(p) => p,
        None => {
            return (
                CANNOT_RUN,
                OciError::Config("exec needs a program".to_owned()),
            );
        }
    };
    match init::resolve_program(program, &process.env) {
        Ok(path) if path.exists() => {}
        Ok(path) => {
            return (
                NOT_FOUND,
                OciError::Config(format!("{} doesn't exist in the container", path.display())),
            );
        }
        Err(e) => return (NOT_FOUND, e),
    }
    match init::exec(process) {
        Err(e) => (CANNOT_RUN, e),
        Ok(()) => (
            CANNOT_RUN,
            OciError::Broken("execve returned without an error".to_owned()),
        ),
    }
}

/// What [`recv_request`] read.
pub enum Received {
    /// The peer closed the socket.
    Closed,
    /// A request: its whole size (larger than the buffer when the message was cut), and the
    /// descriptors it carried.
    Message { size: usize, stdio: Vec<OwnedFd> },
}

/// Sends one exec request on `socket`, the program's `stdio` (stdin, stdout, stderr) riding as
/// descriptors (`SCM_RIGHTS`). The agent's end of the agent↔seed socket (`hex-node`'s pool) sends this.
pub fn send_request(
    socket: BorrowedFd<'_>,
    req: &Request,
    stdio: [BorrowedFd<'_>; seed::REQUEST_FDS],
) -> Result<(), OciError> {
    let bytes = seed::request(req)
        .map_err(|f| OciError::Broken(format!("encoding an exec request: {f}")))?;
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(seed::REQUEST_FDS))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    if !control.push(SendAncillaryMessage::ScmRights(&stdio)) {
        return Err(OciError::os(
            "sending an exec request's stdio",
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
