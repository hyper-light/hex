//! [`SimRuntime`]: boxes in memory, running built-in programs that do what the conformance suite
//! asks for (say words, fail, wait, use memory, start processes, echo stdin), with the bounds
//! enforced as the kernel would: past `memory_max` a program is killed and the kill counted; past
//! `pids_max` a fork is refused and counted. An `oci` box has an init, made at create and waiting
//! until start. From a [`ClassSpec::Oci`] it runs the spec's init program, so `fail N` ends it with N
//! at start and `say` with 0; any other init, and a bundle's, runs until it is signalled or the box
//! is deleted. Once the init has ended the box is empty and inspect reports how it ended, as the node
//! agent does for a container whose init it reaped.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::{
    BoxId, BoxInfo, BoxSpec, BoxState, Class, ClassSpec, Exit, Proc, Program, Refusal, Runtime,
    Signal, Stdio, Usage,
};

/// Most boxes a [`SimRuntime`] holds; past it `create` is refused with [`Refusal::Full`].
pub const SIM_BOXES: usize = 10_000;
/// Most programs a simulated box holds, running or ended; past it `exec` is refused.
pub const SIM_PROCS: usize = 1_000;
/// Most bytes the `echo` program copies; past it, it stops and exits 1.
pub const SIM_ECHO_BYTES: u64 = 1 << 20;

/// A runtime in memory, of both classes.
#[derive(Default)]
pub struct SimRuntime {
    boxes: Mutex<BTreeMap<BoxId, SimBox>>,
}

struct SimBox {
    spec: BoxSpec,
    usage: Usage,
    procs: Vec<Arc<SimProc>>,
    /// An `oci` box's init.
    init: Option<Arc<SimProc>>,
    /// Whether its init has been started.
    started: bool,
    /// How its init ends once started, from its program; `None` runs until signalled.
    init_ends: Option<Exit>,
}

#[derive(Default)]
struct SimProc {
    exit: Mutex<Option<Exit>>,
    ended: Condvar,
}

impl SimProc {
    fn end(&self, exit: Exit) -> Result<(), Refusal> {
        let mut e = self.exit.lock().map_err(poisoned)?;
        if e.is_none() {
            *e = Some(exit);
        }
        self.ended.notify_all();
        Ok(())
    }

    fn exit(&self) -> Result<Option<Exit>, Refusal> {
        Ok(*self.exit.lock().map_err(poisoned)?)
    }
}

fn poisoned<T>(_: std::sync::PoisonError<T>) -> Refusal {
    Refusal::Broken("a simulator lock was poisoned".to_owned())
}

impl SimRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    fn boxes(&self) -> Result<MutexGuard<'_, BTreeMap<BoxId, SimBox>>, Refusal> {
        self.boxes.lock().map_err(poisoned)
    }

    fn info(b: &SimBox) -> Result<BoxInfo, Refusal> {
        let mut running = false;
        for p in b.procs.iter().chain(&b.init) {
            running |= p.exit()?.is_none();
        }
        let waiting = match &b.init {
            Some(init) if !b.started => init.exit()?.is_none(),
            _ => false,
        };
        // An oci box is its init: once that has ended the box is empty, whatever else still runs.
        let exit = match &b.init {
            Some(init) => init.exit()?,
            None => None,
        };
        Ok(BoxInfo {
            id: b.spec.id.clone(),
            class: b.spec.class.class(),
            state: if waiting {
                BoxState::Created
            } else if exit.is_some() {
                BoxState::Empty
            } else if running {
                BoxState::Running
            } else {
                BoxState::Empty
            },
            bounds: b.spec.bounds.clone(),
            adopted: false,
            exit,
        })
    }
}

/// How a simulated init ends once started: `fail N` with N, `say` with 0, anything else (and a
/// bundle, whose program the simulator can't read) not until it is signalled.
fn init_ends(class: &ClassSpec) -> Result<Option<Exit>, Refusal> {
    let ClassSpec::Oci(spec) = class else {
        return Ok(None);
    };
    match spec.init.args.first().map(String::as_str) {
        Some("fail") => Ok(Some(Exit::Code(arg(&spec.init, 1)?))),
        Some("say") => Ok(Some(Exit::Code(0))),
        _ => Ok(None),
    }
}

fn arg<T: std::str::FromStr>(p: &Program, i: usize) -> Result<T, Refusal> {
    p.args
        .get(i)
        .and_then(|a| a.parse().ok())
        .ok_or(Refusal::Invalid {
            what: "program",
            why: format!("{:?} needs a number as argument {i}", p.args.first()),
        })
}

fn write(to: Option<&mut std::io::PipeWriter>, bytes: &[u8]) {
    // A reader that has gone is the caller's choice; the program runs on as a real one would.
    if let Some(w) = to {
        let _ = w.write_all(bytes);
    }
}

impl Runtime for SimRuntime {
    fn classes(&self) -> Vec<Class> {
        vec![Class::Process, Class::Oci]
    }

    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        spec.bounds.check()?;
        let mut boxes = self.boxes()?;
        if boxes.contains_key(&spec.id) {
            return Err(Refusal::Exists(spec.id));
        }
        if boxes.len() >= SIM_BOXES {
            return Err(Refusal::Full(SIM_BOXES));
        }
        let init = (spec.class.class() == Class::Oci).then(|| Arc::new(SimProc::default()));
        let init_ends = init_ends(&spec.class)?;
        let b = SimBox {
            spec,
            usage: Usage::default(),
            procs: Vec::new(),
            init,
            started: false,
            init_ends,
        };
        let info = Self::info(&b)?;
        boxes.insert(b.spec.id.clone(), b);
        Ok(info)
    }

    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        let mut boxes = self.boxes()?;
        let b = boxes
            .get_mut(id)
            .ok_or_else(|| Refusal::NoSuchBox(id.clone()))?;
        let Some(init) = &b.init else {
            return Err(Refusal::Invalid {
                what: "start",
                why: format!("box {id} is a process box, which has no init to start"),
            });
        };
        if b.started {
            return Err(Refusal::Invalid {
                what: "start",
                why: format!("box {id} has started already"),
            });
        }
        if init.exit()?.is_some() {
            return Err(Refusal::NotRunning(id.clone(), "start"));
        }
        if let Some(exit) = b.init_ends {
            init.end(exit)?;
        }
        b.started = true;
        Ok(())
    }

    fn exec(
        &self,
        id: &BoxId,
        program: Program,
        mut stdio: Stdio,
    ) -> Result<Box<dyn Proc>, Refusal> {
        program.check()?;
        let mut boxes = self.boxes()?;
        let b = boxes
            .get_mut(id)
            .ok_or_else(|| Refusal::NoSuchBox(id.clone()))?;
        if b.procs.len() >= SIM_PROCS {
            return Err(Refusal::Full(SIM_PROCS));
        }
        let proc = Arc::new(SimProc::default());
        let name = program.args.first().map(String::as_str).unwrap_or_default();
        let bounds = &b.spec.bounds;
        match name {
            "say" => {
                let words = program.args.get(1..).unwrap_or_default().join(" ");
                write(stdio.stdout.as_mut(), format!("{words}\n").as_bytes());
                proc.end(Exit::Code(0))?;
            }
            "fail" => {
                let code: i32 = arg(&program, 1)?;
                write(stdio.stderr.as_mut(), b"failing\n");
                proc.end(Exit::Code(code))?;
            }
            "wait" => {}
            "hog" => {
                let bytes: u64 = arg(&program, 1)?;
                if bounds.memory_max.is_some_and(|m| bytes > m) {
                    b.usage.oom_kills = b.usage.oom_kills.saturating_add(1);
                    proc.end(Exit::Signal(Signal::KILL.number()))?;
                } else {
                    b.usage.memory_peak_bytes = b.usage.memory_peak_bytes.max(bytes);
                    proc.end(Exit::Code(0))?;
                }
            }
            "spawn" => {
                let n: u64 = arg(&program, 1)?;
                // The program is a process itself, so n more need n + 1 within the bound.
                if bounds.pids_max.is_some_and(|m| n >= u64::from(m)) {
                    b.usage.pids_refused = b.usage.pids_refused.saturating_add(1);
                    write(stdio.stderr.as_mut(), b"spawn: can't fork\n");
                    proc.end(Exit::Code(2))?;
                } else {
                    proc.end(Exit::Code(0))?;
                }
            }
            "pwd" => {
                let cwd = program.cwd.clone().unwrap_or_else(|| "/".to_owned());
                write(stdio.stdout.as_mut(), format!("{cwd}\n").as_bytes());
                proc.end(Exit::Code(0))?;
            }
            "echo" => {
                let p = Arc::clone(&proc);
                let (stdin, stdout) = (stdio.stdin.take(), stdio.stdout.take());
                std::thread::Builder::new()
                    .name("hex-sim-echo".to_owned())
                    .spawn(move || {
                        let code = match stdin {
                            None => 0,
                            Some(r) => {
                                let mut r = r.take(SIM_ECHO_BYTES);
                                let copied = match stdout {
                                    Some(mut w) => std::io::copy(&mut r, &mut w),
                                    None => std::io::copy(&mut r, &mut std::io::sink()),
                                };
                                if copied.is_ok_and(|n| n < SIM_ECHO_BYTES) {
                                    0
                                } else {
                                    1
                                }
                            }
                        };
                        let _ = p.end(Exit::Code(code));
                    })
                    .map_err(|e| Refusal::os("starting the echo program", e))?;
            }
            other => {
                return Err(Refusal::Invalid {
                    what: "program",
                    why: format!(
                        "{other:?}: the simulator runs say, fail, wait, hog, spawn, pwd and echo"
                    ),
                });
            }
        }
        b.procs.push(Arc::clone(&proc));
        Ok(Box::new(SimHandle(proc)))
    }

    fn signal(&self, id: &BoxId, signal: Signal) -> Result<(), Refusal> {
        let boxes = self.boxes()?;
        let b = boxes
            .get(id)
            .ok_or_else(|| Refusal::NoSuchBox(id.clone()))?;
        let mut any = false;
        for p in b.procs.iter().chain(&b.init) {
            if p.exit()?.is_none() {
                p.end(Exit::Signal(signal.number()))?;
                any = true;
            }
        }
        if any {
            Ok(())
        } else {
            Err(Refusal::NotRunning(id.clone(), "signal"))
        }
    }

    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
        let boxes = self.boxes()?;
        Self::info(
            boxes
                .get(id)
                .ok_or_else(|| Refusal::NoSuchBox(id.clone()))?,
        )
    }

    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        let boxes = self.boxes()?;
        let b = boxes
            .get(id)
            .ok_or_else(|| Refusal::NoSuchBox(id.clone()))?;
        let mut u = b.usage;
        let mut running = 0u64;
        for p in b.procs.iter().chain(&b.init) {
            if p.exit()?.is_none() {
                running = running.saturating_add(1);
            }
        }
        u.pids = running;
        Ok(u)
    }

    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        self.boxes()?.values().map(Self::info).collect()
    }

    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        let b = self
            .boxes()?
            .remove(id)
            .ok_or_else(|| Refusal::NoSuchBox(id.clone()))?;
        for p in b.procs.iter().chain(&b.init) {
            p.end(Exit::Signal(Signal::KILL.number()))?;
        }
        Ok(())
    }
}

struct SimHandle(Arc<SimProc>);

impl Proc for SimHandle {
    fn pid(&self) -> u32 {
        0
    }

    fn try_wait(&mut self) -> Result<Option<Exit>, Refusal> {
        self.0.exit()
    }

    fn wait_for(&mut self, timeout: Duration) -> Result<Option<Exit>, Refusal> {
        let deadline = Instant::now().checked_add(timeout);
        let mut exit = self.0.exit.lock().map_err(poisoned)?;
        loop {
            if exit.is_some() {
                return Ok(*exit);
            }
            let left = match deadline {
                Some(d) => d.saturating_duration_since(Instant::now()),
                None => timeout,
            };
            if left.is_zero() {
                return Ok(None);
            }
            exit = self.0.ended.wait_timeout(exit, left).map_err(poisoned)?.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conformance::{self, Act, Fixture};
    use crate::{Bounds, Network, OciSpec, Rootfs};

    /// The simulator's programs, in boxes of either class.
    struct SimFixture(Class);

    impl Fixture for SimFixture {
        fn spec(&self, id: BoxId, bounds: Bounds) -> BoxSpec {
            BoxSpec {
                id,
                bounds,
                class: match self.0 {
                    Class::Process => ClassSpec::Process,
                    Class::Oci => ClassSpec::Bundle("/bundle".into()),
                },
            }
        }
        fn init(&self, id: BoxId, act: &Act) -> BoxSpec {
            let class = match self.0 {
                Class::Process => ClassSpec::Process,
                Class::Oci => ClassSpec::Oci(Box::new(OciSpec {
                    rootfs: Rootfs::Dir("/rootfs".into()),
                    init: self.program(act),
                    mounts: Vec::new(),
                    hostname: None,
                    network: Network::Private,
                    hosts: Vec::new(),
                })),
            };
            BoxSpec {
                id,
                bounds: Bounds::default(),
                class,
            }
        }
        fn program(&self, act: &Act) -> Program {
            match act {
                Act::Say(words) => Program::of(&["say", words]),
                Act::Fail(code) => Program::of(&["fail", &code.to_string()]),
                Act::Wait => Program::of(&["wait"]),
                Act::Hog(bytes) => Program::of(&["hog", &bytes.to_string()]),
                Act::Spawn(n) => Program::of(&["spawn", &n.to_string()]),
                Act::Echo => Program::of(&["echo"]),
            }
        }
    }

    #[test]
    fn the_simulator_passes_the_conformance_suite() {
        for class in [Class::Process, Class::Oci] {
            let failures = conformance::run(&SimRuntime::new(), &SimFixture(class));
            assert!(failures.is_empty(), "{class:?}: {failures:#?}");
        }
    }

    #[test]
    fn the_simulator_refuses_past_its_box_bound() {
        let rt = SimRuntime::new();
        for i in 0..SIM_BOXES {
            let id = BoxId::new(&format!("b{i}")).unwrap();
            rt.create(SimFixture(Class::Process).spec(id, Bounds::default()))
                .unwrap();
        }
        let id = BoxId::new("one-more").unwrap();
        assert_eq!(
            rt.create(SimFixture(Class::Process).spec(id, Bounds::default())),
            Err(Refusal::Full(SIM_BOXES))
        );
    }
}
