//! The `process` class on Linux: host processes started straight into their box's cgroup, so a
//! program and everything it forks are bound from their first instruction (CP-T7.1). They are cloned
//! into the cgroup (`clone3` with `CLONE_INTO_CGROUP`) where the kernel can, and otherwise forked and
//! joined to it before they execute.

use std::ffi::CString;
use std::os::fd::{AsFd, OwnedFd};
use std::process::{Command, Stdio as StdStdio};
use std::sync::Arc;

use hex_api::{Program, Refusal, Stdio};
use rustix::fs::{Mode, OFlags};

use super::cgroup::BoxCgroup;
use super::proc::PidfdProc;
use super::sys;

/// Starts `program` in the box `cg` with `stdio`, as the node agent's own user.
pub fn exec(cg: &BoxCgroup, program: &Program, stdio: Stdio) -> Result<PidfdProc, Refusal> {
    program.check()?;
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
    let failed = |e: std::io::Error| Refusal::Os {
        op: "starting a program in a box",
        why: format!("{first}: {e}"),
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
    join_on_exec(cg, &mut cmd)?;
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
/// runner keeps its own stdio and process-group handling, CP-T7.1's first consumer).
pub fn join_on_exec(cg: &BoxCgroup, cmd: &mut Command) -> Result<(), Refusal> {
    sys::join_cgroup_on_exec(cmd, Arc::new(cg.procs_for_joining()?));
    Ok(())
}
