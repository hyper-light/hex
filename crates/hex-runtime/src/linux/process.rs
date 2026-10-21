//! The `process` class on Linux: host processes started straight into their box's cgroup, so a
//! program and everything it forks are bound from their first instruction (CP-T7.1).

use std::process::{Command, Stdio as StdStdio};
use std::sync::Arc;

use hex_api::{Program, Refusal, Stdio};

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
    let (first, rest) = program.args.split_first().ok_or(Refusal::Invalid {
        what: "program",
        why: "no arguments".to_owned(),
    })?;
    let mut cmd = Command::new(first);
    cmd.args(rest)
        .env_clear()
        .envs(program.env.iter().map(|(k, v)| (k, v)));
    if let Some(cwd) = &program.cwd {
        cmd.current_dir(cwd);
    }
    cmd.stdin(stdio.stdin.map_or_else(StdStdio::null, StdStdio::from))
        .stdout(stdio.stdout.map_or_else(StdStdio::null, StdStdio::from))
        .stderr(stdio.stderr.map_or_else(StdStdio::null, StdStdio::from));
    join_on_exec(cg, &mut cmd)?;
    let child = cmd.spawn().map_err(|e| Refusal::Os {
        op: "starting a program in a box",
        why: format!("{first}: {e}"),
    })?;
    // The child stays unwaited (dropping a std Child neither waits nor kills), so its pid is ours
    // until the pidfd reaps it.
    PidfdProc::of_child(child.id())
}

/// Makes `cmd`'s child start in the box `cg`: for callers that start processes themselves (kiln's
/// runner keeps its own stdio and process-group handling, CP-T7.1's first consumer).
pub fn join_on_exec(cg: &BoxCgroup, cmd: &mut Command) -> Result<(), Refusal> {
    sys::join_cgroup_on_exec(cmd, Arc::new(cg.procs_for_joining()?));
    Ok(())
}
