//! The `process` class on Linux: host processes started straight into their box's cgroup, so a
//! program and everything it forks are bound from their first instruction (CP-T7.1). They are cloned
//! into the cgroup (`clone3` with `CLONE_INTO_CGROUP`) where the kernel can, and otherwise forked and
//! joined to it before they execute.

use std::ffi::CString;
use std::os::fd::{AsFd, OwnedFd};
use std::process::{Command, Stdio as StdStdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use hex_api::{ExecLimit, Program, Refusal, Stdio};
use rustix::fs::{Mode, OFlags};

use super::cgroup::BoxCgroup;
use super::proc::PidfdProc;
use super::sys;

/// The node's own RLIMIT_STACK soft limit, read once (0 until then; `u64::MAX` for none) and again only when
/// the kernel answers E2BIG past the bound Hex held a program to (learned).
static NODE_STACK: AtomicU64 = AtomicU64::new(0);
/// How many times the node's stack limit was read, so a test can tell no exec reads it.
static STACK_READS: AtomicU64 = AtomicU64::new(0);

fn read_stack() -> u64 {
    STACK_READS.fetch_add(1, Ordering::Relaxed);
    rustix::process::getrlimit(rustix::process::Resource::Stack)
        .current
        .unwrap_or(u64::MAX)
        .max(1)
}

/// The node's RLIMIT_STACK soft limit: read on first use, then kept.
pub fn node_stack() -> u64 {
    match NODE_STACK.load(Ordering::Relaxed) {
        0 => {
            let stack = read_stack();
            NODE_STACK.store(stack, Ordering::Relaxed);
            stack
        }
        stack => stack,
    }
}

/// Reads the node's stack limit again, after the kernel refused what its last reading admitted.
pub fn relearn_stack() {
    NODE_STACK.store(read_stack(), Ordering::Relaxed);
}

/// How many times the node's stack limit has been read.
pub fn stack_reads() -> u64 {
    STACK_READS.load(Ordering::Relaxed)
}

/// The bound a box's programs are held to: [`exec_limit_for`] the node's own stack limit, read once.
pub fn exec_limit() -> ExecLimit {
    exec_limit_for(node_stack())
}

/// Holds a spec's process (its arguments and `KEY=VALUE` environment, as execve takes them) to the bound
/// of a box whose stack limit is `stack`.
pub fn fits_process(args: &[String], env: &[String], stack: u64) -> Result<(), Refusal> {
    let program = Program {
        args: args.to_vec(),
        env: env
            .iter()
            .map(|e| {
                let (k, v) = e.split_once('=').unwrap_or((e.as_str(), ""));
                (k.to_owned(), v.to_owned())
            })
            .collect(),
        ..Program::default()
    };
    program.fits(&exec_limit_for(stack))
}

/// The bound Linux's execve puts on a program's arguments and environment in a process whose RLIMIT_STACK
/// soft limit is `stack`, kind (a) (docs/research/runtime/exec-size-bounds.md): the strings may take a
/// quarter of the stack, capped at three quarters of `_STK_LIM` and never under `ARG_MAX`
/// (`fs/exec.c:410-416`, v6.19), and any one string `MAX_ARG_STRLEN`, 32 pages
/// (`include/uapi/linux/binfmts.h:15`). glibc's `sysconf(_SC_ARG_MAX)` skips the kernel's cap, so the
/// kernel's own formula is used. No system call: the page size is the auxiliary vector's.
pub fn exec_limit_for(stack: u64) -> ExecLimit {
    // `_STK_LIM`, include/uapi/linux/resource.h:66, and `ARG_MAX`, include/uapi/linux/limits.h:8 (v6.19).
    const STK_LIM: u64 = 8 << 20;
    const ARG_MAX: u64 = 131_072;
    // `MAX_ARG_STRLEN` is this many pages, include/uapi/linux/binfmts.h:15.
    const MAX_ARG_STRLEN_PAGES: usize = 32;
    let quarter = stack.checked_div(4).unwrap_or(0);
    let cap = STK_LIM.checked_div(4).unwrap_or(0).saturating_mul(3);
    let block = cap.min(quarter).max(ARG_MAX);
    ExecLimit {
        block: usize::try_from(block).unwrap_or(usize::MAX),
        counts: hex_api::BlockCount::Execve,
        string: Some(rustix::param::page_size().saturating_mul(MAX_ARG_STRLEN_PAGES)),
        unit: hex_api::StringUnit::Bytes,
        source: "Linux's execve (a quarter of the box's RLIMIT_STACK, fs/exec.c)",
    }
}

/// Starts `program` in the box `cg` with `stdio`, as the node agent's own user.
pub fn exec(cg: &BoxCgroup, program: &Program, stdio: Stdio) -> Result<PidfdProc, Refusal> {
    program.check()?;
    program.fits(&exec_limit())?;
    if program.user.is_some() {
        return Err(Refusal::Invalid {
            what: "program",
            why: "a process box runs its programs as the node agent's user; a user is the oci class's".to_owned(),
        });
    }
    let first = program.args.first().ok_or(Refusal::Invalid {
        what: "program",
        why: "no arguments".to_owned(),
    })?;
    let failed = |e: std::io::Error| {
        if e.raw_os_error() == Some(rustix::io::Errno::TOOBIG.raw_os_error()) {
            // The kernel refused what the bound admitted: its stack limit changed. Learn it again.
            relearn_stack();
            return Refusal::Invalid {
                what: "program",
                why: format!(
                    "{first}: the kernel refused its arguments and environment (E2BIG); the node's stack limit is read again"
                ),
            };
        }
        Refusal::Os {
            op: "starting a program in a box",
            why: format!("{first}: {e}"),
        }
    };
    let stdio = [
        stdio
            .stdin
            .map_or_else(|| null(OFlags::RDONLY), |p| Ok(OwnedFd::from(p)))?,
        stdio
            .stdout
            .map_or_else(|| null(OFlags::WRONLY), |p| Ok(OwnedFd::from(p)))?,
        stdio
            .stderr
            .map_or_else(|| null(OFlags::WRONLY), |p| Ok(OwnedFd::from(p)))?,
    ];
    let spawn = sys::Spawn::new(
        paths(program)?,
        program
            .args
            .iter()
            .map(|a| c_string(a))
            .collect::<Result<_, _>>()?,
        program
            .env
            .iter()
            .map(|(k, v)| c_string(&format!("{k}={v}")))
            .collect::<Result<_, _>>()?,
        program.cwd.as_deref().map(c_string).transpose()?,
        stdio,
        super::default_nofile(),
    )
    .map_err(failed)?;
    let dir = cg.open_dir()?;
    match sys::clone_into_cgroup(dir.as_fd(), spawn).map_err(failed)? {
        sys::Cloned::Started { pid, pidfd } => Ok(PidfdProc::of_clone(pid, pidfd)),
        sys::Cloned::Unsupported(spawn) => exec_forked(cg, program, spawn.into_stdio()),
    }
}

/// Where the kernel or a seccomp profile refuses a clone into a cgroup: forked by std's `Command`,
/// which then joins the cgroup by migration in the child before it executes.
fn exec_forked(
    cg: &BoxCgroup,
    program: &Program,
    stdio: [OwnedFd; 3],
) -> Result<PidfdProc, Refusal> {
    let (first, rest) = program.args.split_first().ok_or(Refusal::Invalid {
        what: "program",
        why: "no arguments".to_owned(),
    })?;
    let [stdin, stdout, stderr] = stdio;
    let mut cmd = Command::new(first);
    cmd.args(rest)
        .env_clear()
        .envs(program.env.iter().map(|(k, v)| (k, v)))
        .stdin(StdStdio::from(stdin))
        .stdout(StdStdio::from(stdout))
        .stderr(StdStdio::from(stderr));
    if let Some(cwd) = &program.cwd {
        cmd.current_dir(cwd);
    }
    // A box the node starts gets the default limit, not the node's own (HEX-INST-6).
    sys::join_cgroup_on_exec(
        &mut cmd,
        Arc::new(cg.procs_for_joining()?),
        Some(super::default_nofile()),
    );
    let child = cmd.spawn().map_err(|e| Refusal::Os {
        op: "starting a program in a box",
        why: format!("{first}: {e}"),
    })?;
    // The child stays unwaited (dropping a std Child neither waits nor kills), so its pid is ours
    // until the pidfd reaps it.
    PidfdProc::of_child(child.id())
}

/// The paths a program is executed from, in order, as execvp(3) tries them: its name when it holds a
/// `/`, else that name in each folder of its own `PATH`, or of glibc's default `/bin:/usr/bin` when it
/// sets none (an empty entry is the working folder). The same as std's `Command`, which hands execvp
/// the child's environment.
fn paths(program: &Program) -> Result<Vec<CString>, Refusal> {
    let first = program.args.first().ok_or(Refusal::Invalid {
        what: "program",
        why: "no arguments".to_owned(),
    })?;
    if first.contains('/') {
        return Ok(vec![c_string(first)?]);
    }
    let path = program
        .env
        .iter()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.as_str())
        .unwrap_or("/bin:/usr/bin");
    path.split(':')
        .map(|dir| {
            c_string(&format!(
                "{}/{first}",
                if dir.is_empty() { "." } else { dir }
            ))
        })
        .collect()
}

fn c_string(s: &str) -> Result<CString, Refusal> {
    CString::new(s).map_err(|_| Refusal::Invalid {
        what: "program",
        why: format!("{s:?} holds a NUL byte"),
    })
}

/// The null device, for a stream the caller gave none.
fn null(flags: OFlags) -> Result<OwnedFd, Refusal> {
    rustix::fs::open("/dev/null", flags | OFlags::CLOEXEC, Mode::empty())
        .map_err(|e| Refusal::os("opening /dev/null", e))
}

/// Makes `cmd`'s child start in the box `cg`: for callers that start processes themselves (kiln's
/// runner keeps its own stdio and process-group handling, CP-T7.1's first consumer, and `hex box run`).
/// The program keeps its caller's descriptor limit: such a caller wraps a program for itself, and isn't
/// the node, whose raised limit the default for the node's own boxes keeps them from (HEX-INST-6).
pub fn join_on_exec(cg: &BoxCgroup, cmd: &mut Command) -> Result<(), Refusal> {
    sys::join_cgroup_on_exec(cmd, Arc::new(cg.procs_for_joining()?), None);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HEX-RT-14: the bound a box's exec is held to comes from the box's own RLIMIT_STACK, and reading it
    /// takes no system call: the node reads its own limit once. Do: the bound for a box whose spec sets a
    /// 1 MiB stack, a hundred times, after the node's bound has been read. Expect a quarter of it (256
    /// KiB, above Linux's 128 KiB floor and under the node's default), and no getrlimit among the hundred.
    #[test]
    fn a_box_with_a_smaller_stack_gets_the_smaller_bound_and_no_read_per_exec() {
        let node = exec_limit();
        let reads = stack_reads();
        for _ in 0..100 {
            let bound = exec_limit_for(1 << 20);
            assert_eq!(bound.block, 256 << 10);
            let _ = exec_limit();
        }
        assert_eq!(stack_reads(), reads, "a getrlimit per exec");
        assert!(node.block >= 256 << 10, "{node:?}");
    }
}
