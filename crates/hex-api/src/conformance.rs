//! The [`crate::Runtime`] contract as cases every implementation passes: the simulator in this
//! crate's tests, each real class in its own (CLAUDE.md §9). A [`Fixture`] says how to make a box of
//! the class under test and supplies programs by what they do, so each implementation brings its
//! own programs. Each case reads "do X, expect Y"; [`run`] returns the cases that failed and why.

use std::io::{Read, Write};
use std::time::Duration;

use crate::{Bounds, BoxId, BoxSpec, BoxState, Exit, Program, Refusal, Runtime, Signal, Stdio};

/// What a fixture's program does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// Writes these words and a newline to stdout, then exits 0.
    Say(&'static str),
    /// Exits with this status.
    Fail(i32),
    /// Runs until it is signalled.
    Wait,
    /// Uses this many bytes of memory, then exits 0.
    Hog(u64),
    /// Starts this many processes at once, then exits 0; exits non-zero when one can't start.
    Spawn(u32),
    /// Copies stdin to stdout until stdin closes, then exits 0.
    Echo,
}

/// How to drive the implementation under test.
pub trait Fixture {
    /// A box of the class under test, with `bounds`.
    fn spec(&self, id: BoxId, bounds: Bounds) -> BoxSpec;
    /// A program that does `act`.
    fn program(&self, act: &Act) -> Program;
}

/// One part of the contract.
pub struct Case {
    pub id: &'static str,
    pub says: &'static str,
    run: fn(&dyn Runtime, &dyn Fixture) -> Result<(), String>,
}

/// A case that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub case: &'static str,
    pub why: String,
}

/// Longest a case waits for a program it expects to end: a bound on a wait for a fact, so a
/// runtime that never reports an end fails the case instead of hanging the suite.
pub const PATIENCE: Duration = Duration::from_secs(60);
/// How long a case watches a program it expects to keep running.
pub const STILL_RUNNING: Duration = Duration::from_millis(200);

/// Every case, in order.
pub const CASES: &[Case] = &[
    Case {
        id: "HEX-RT-1",
        says: "create makes a box that list and inspect show empty; a second create of its id is refused as Exists",
        run: create_and_exists,
    },
    Case {
        id: "HEX-RT-2",
        says: "exec of a program that says words: they reach its stdout and it exits 0",
        run: say,
    },
    Case {
        id: "HEX-RT-3",
        says: "exec of a program that fails with 3 ends Code(3)",
        run: fail,
    },
    Case {
        id: "HEX-RT-4",
        says: "a waiting program keeps running and the box shows Running; signal TERM ends it with Signal(15)",
        run: signal,
    },
    Case {
        id: "HEX-RT-5",
        says: "a box bounded to 64 MiB: a program using 512 MiB is killed and the kill counted",
        run: memory_bound,
    },
    Case {
        id: "HEX-RT-6",
        says: "a box bounded to 8 processes: a program starting 32 exits non-zero and the refused fork is counted",
        run: pids_bound,
    },
    Case {
        id: "HEX-RT-7",
        says: "delete ends what runs in the box and removes it; then every call on it is NoSuchBox",
        run: delete,
    },
    Case {
        id: "HEX-RT-8",
        says: "create with an invalid bound is refused as Invalid and makes nothing",
        run: invalid_bounds,
    },
    Case {
        id: "HEX-RT-9",
        says: "stdin reaches the program: what is written comes back on stdout",
        run: stdin,
    },
];

/// Runs every case on fresh boxes; returns the failures.
pub fn run(rt: &dyn Runtime, fx: &dyn Fixture) -> Vec<Failure> {
    CASES
        .iter()
        .filter_map(|c| (c.run)(rt, fx).err().map(|why| Failure { case: c.id, why }))
        .collect()
}

fn id(name: &str) -> Result<BoxId, String> {
    BoxId::new(&format!("hex-conformance-{name}")).map_err(|e| e.to_string())
}

fn ensure(ok: bool, why: impl FnOnce() -> String) -> Result<(), String> {
    if ok { Ok(()) } else { Err(why()) }
}

fn made(rt: &dyn Runtime, fx: &dyn Fixture, name: &str, bounds: Bounds) -> Result<BoxId, String> {
    let id = id(name)?;
    // A box left by an earlier run of the suite is removed first; absent is the usual case.
    let _ = rt.delete(&id);
    rt.create(fx.spec(id.clone(), bounds))
        .map_err(|e| format!("create: {e}"))?;
    Ok(id)
}

fn ended(p: &mut Box<dyn crate::Proc>) -> Result<Exit, String> {
    p.wait_for(PATIENCE)
        .map_err(|e| format!("wait: {e}"))?
        .ok_or_else(|| format!("still running after {PATIENCE:?}"))
}

fn piped() -> Result<(std::io::PipeReader, std::io::PipeWriter), String> {
    std::io::pipe().map_err(|e| format!("pipe: {e}"))
}

fn create_and_exists(rt: &dyn Runtime, fx: &dyn Fixture) -> Result<(), String> {
    let id = made(rt, fx, "exists", Bounds::default())?;
    let listed = rt.list().map_err(|e| e.to_string())?;
    ensure(listed.iter().any(|b| b.id == id), || {
        format!("{id} isn't in {listed:?}")
    })?;
    let info = rt.inspect(&id).map_err(|e| e.to_string())?;
    let class = fx.spec(id.clone(), Bounds::default()).class.class();
    ensure(info.class == class, || {
        format!("inspect says class {:?}", info.class)
    })?;
    // An oci box's init runs from create on; a process box is empty until exec.
    if class == crate::Class::Process {
        ensure(info.state == BoxState::Empty, || {
            format!("a new box is {:?}", info.state)
        })?;
    }
    let again = rt.create(fx.spec(id.clone(), Bounds::default()));
    ensure(again == Err(Refusal::Exists(id.clone())), || {
        format!("second create: {again:?}")
    })?;
    rt.delete(&id).map_err(|e| e.to_string())
}

fn say(rt: &dyn Runtime, fx: &dyn Fixture) -> Result<(), String> {
    let id = made(rt, fx, "say", Bounds::default())?;
    let (mut r, w) = piped()?;
    let mut p = rt
        .exec(
            &id,
            fx.program(&Act::Say("hello from a box")),
            Stdio {
                stdout: Some(w),
                ..Stdio::null()
            },
        )
        .map_err(|e| format!("exec: {e}"))?;
    let exit = ended(&mut p)?;
    let mut out = String::new();
    r.read_to_string(&mut out)
        .map_err(|e| format!("read: {e}"))?;
    rt.delete(&id).map_err(|e| e.to_string())?;
    ensure(exit == Exit::Code(0), || format!("exit {exit:?}"))?;
    ensure(out == "hello from a box\n", || format!("stdout {out:?}"))
}

fn fail(rt: &dyn Runtime, fx: &dyn Fixture) -> Result<(), String> {
    let id = made(rt, fx, "fail", Bounds::default())?;
    let mut p = rt
        .exec(&id, fx.program(&Act::Fail(3)), Stdio::null())
        .map_err(|e| format!("exec: {e}"))?;
    let exit = ended(&mut p)?;
    rt.delete(&id).map_err(|e| e.to_string())?;
    ensure(exit == Exit::Code(3), || format!("exit {exit:?}"))
}

fn signal(rt: &dyn Runtime, fx: &dyn Fixture) -> Result<(), String> {
    let id = made(rt, fx, "signal", Bounds::default())?;
    let mut p = rt
        .exec(&id, fx.program(&Act::Wait), Stdio::null())
        .map_err(|e| format!("exec: {e}"))?;
    let early = p.wait_for(STILL_RUNNING).map_err(|e| e.to_string())?;
    ensure(early.is_none(), || {
        format!("a waiting program ended: {early:?}")
    })?;
    let state = rt.inspect(&id).map_err(|e| e.to_string())?.state;
    ensure(state == BoxState::Running, || {
        format!("box {state:?} while a program waits")
    })?;
    rt.signal(&id, Signal::TERM)
        .map_err(|e| format!("signal: {e}"))?;
    let exit = ended(&mut p)?;
    rt.delete(&id).map_err(|e| e.to_string())?;
    ensure(exit == Exit::Signal(15), || format!("exit {exit:?}"))
}

fn memory_bound(rt: &dyn Runtime, fx: &dyn Fixture) -> Result<(), String> {
    let bounds = Bounds {
        memory_max: Some(64 << 20),
        ..Bounds::default()
    };
    let id = made(rt, fx, "memory", bounds)?;
    let mut p = rt
        .exec(&id, fx.program(&Act::Hog(512 << 20)), Stdio::null())
        .map_err(|e| format!("exec: {e}"))?;
    let exit = ended(&mut p)?;
    let usage = rt.usage(&id).map_err(|e| e.to_string())?;
    rt.delete(&id).map_err(|e| e.to_string())?;
    ensure(exit != Exit::Code(0), || {
        "a program over the memory bound exited 0".to_owned()
    })?;
    ensure(usage.oom_kills >= 1, || {
        format!("no kill counted: {usage:?}")
    })
}

fn pids_bound(rt: &dyn Runtime, fx: &dyn Fixture) -> Result<(), String> {
    let bounds = Bounds {
        pids_max: Some(8),
        ..Bounds::default()
    };
    let id = made(rt, fx, "pids", bounds)?;
    let mut p = rt
        .exec(&id, fx.program(&Act::Spawn(32)), Stdio::null())
        .map_err(|e| format!("exec: {e}"))?;
    let exit = ended(&mut p)?;
    let usage = rt.usage(&id).map_err(|e| e.to_string())?;
    rt.delete(&id).map_err(|e| e.to_string())?;
    ensure(exit != Exit::Code(0), || {
        "a program over the process bound exited 0".to_owned()
    })?;
    ensure(usage.pids_refused >= 1, || {
        format!("no refused fork counted: {usage:?}")
    })
}

fn delete(rt: &dyn Runtime, fx: &dyn Fixture) -> Result<(), String> {
    let id = made(rt, fx, "delete", Bounds::default())?;
    let mut p = rt
        .exec(&id, fx.program(&Act::Wait), Stdio::null())
        .map_err(|e| format!("exec: {e}"))?;
    rt.delete(&id).map_err(|e| format!("delete: {e}"))?;
    let exit = ended(&mut p)?;
    ensure(matches!(exit, Exit::Signal(_)), || {
        format!("deleted box's program: {exit:?}")
    })?;
    let gone = Refusal::NoSuchBox(id.clone());
    let calls = [
        ("inspect", rt.inspect(&id).err()),
        ("usage", rt.usage(&id).err()),
        ("signal", rt.signal(&id, Signal::KILL).err()),
        (
            "exec",
            rt.exec(&id, fx.program(&Act::Wait), Stdio::null()).err(),
        ),
        ("delete", rt.delete(&id).err()),
    ];
    for (call, got) in calls {
        ensure(got.as_ref() == Some(&gone), || {
            format!("{call} after delete: {got:?}")
        })?;
    }
    let listed = rt.list().map_err(|e| e.to_string())?;
    ensure(!listed.iter().any(|b| b.id == id), || {
        format!("{id} still listed")
    })
}

fn invalid_bounds(rt: &dyn Runtime, fx: &dyn Fixture) -> Result<(), String> {
    let id = id("invalid")?;
    let _ = rt.delete(&id);
    let bad = Bounds {
        cpu_weight: Some(0),
        ..Bounds::default()
    };
    let got = rt.create(fx.spec(id.clone(), bad));
    ensure(
        matches!(
            got,
            Err(Refusal::Invalid {
                what: "cpu_weight",
                ..
            })
        ),
        || format!("create: {got:?}"),
    )?;
    let got = rt.inspect(&id);
    ensure(got == Err(Refusal::NoSuchBox(id.clone())), || {
        format!("a refused create made {got:?}")
    })
}

fn stdin(rt: &dyn Runtime, fx: &dyn Fixture) -> Result<(), String> {
    let id = made(rt, fx, "stdin", Bounds::default())?;
    let (in_r, mut in_w) = piped()?;
    let (mut out_r, out_w) = piped()?;
    let mut p = rt
        .exec(
            &id,
            fx.program(&Act::Echo),
            Stdio {
                stdin: Some(in_r),
                stdout: Some(out_w),
                stderr: None,
            },
        )
        .map_err(|e| format!("exec: {e}"))?;
    in_w.write_all(b"round trip\n")
        .map_err(|e| format!("write: {e}"))?;
    drop(in_w);
    let mut out = String::new();
    out_r
        .read_to_string(&mut out)
        .map_err(|e| format!("read: {e}"))?;
    let exit = ended(&mut p)?;
    rt.delete(&id).map_err(|e| e.to_string())?;
    ensure(exit == Exit::Code(0), || format!("exit {exit:?}"))?;
    ensure(out == "round trip\n", || format!("stdout {out:?}"))
}
