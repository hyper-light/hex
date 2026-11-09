//! A node over any OS's runtime (native plan step 8): each box's record kept durably in the node's state
//! directory beside the runtime that makes the box ([`crate::store`], the records Linux's node keeps), and the
//! programs the socket's disconnect rule ends counted into it ([`crate::abandoned`]), so a node served on
//! any OS's socket ([`crate::serve`]) answers as Linux's does. Linux's node ([`crate::Node`]) keeps the same
//! records and counts through the same code, with its cgroups and containers beside them; macOS's node is
//! this over its runtime (the `process` class, and Linux images in its Shards guest).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use hex_api::{
    Ask, BoxId, BoxInfo, BoxSpec, Class, Proc, Program, Refusal, Runtime, Signal, Stdio, Usage,
};
use hex_core::netns;
use hex_core::record::{BoxRecord, Phase, ProcId};
use hex_core::recovery::{self, Decision, Parts, Seen, Why};

use crate::abandoned::Abandoned;
use crate::serve::{self, Forward, Forwarded, Host, Loop, Reply};
use crate::store::Store;
use hex_core::wire::{Answer, Call};

/// What a recovery did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoverySummary {
    /// Boxes taken back as they were.
    pub adopted: Vec<BoxId>,
    /// Boxes removed, each with why.
    pub removed: Vec<(BoxId, String)>,
    /// Slots removed that no box's record named: idle when the agent stopped, or a create's caught
    /// before its record (goal 5 extended, step 6). Linux's node's alone: no other OS's hands a box's
    /// cgroup on.
    pub slots_removed: Vec<u64>,
}

/// Why a box was removed, for its summary.
pub(crate) fn why_text(why: &Why) -> String {
    match why {
        Why::MidCreate => "the node agent stopped while it was being made".to_owned(),
        Why::MidDelete => "the node agent stopped while it was being removed".to_owned(),
        Why::InitGone => "its container's init had ended".to_owned(),
        Why::NoCgroup => "its record had no cgroup".to_owned(),
        Why::NoRecord(parts) => {
            let left: Vec<&str> = [
                (parts.cgroup, "its cgroup"),
                (parts.state_dir, "its container's state"),
                (parts.bundle, "its bundle"),
            ]
            .into_iter()
            .filter(|(found, _)| *found)
            .map(|(_, what)| what)
            .collect();
            format!(
                "no record named it, made only in part: {} left",
                left.join(", ")
            )
        }
        Why::Unreadable(w) => format!("its record couldn't be read: {w}"),
        Why::StartInterrupted => {
            "the node agent stopped while it started the box, in its startContainer hooks"
                .to_owned()
        }
    }
}

/// What a node needs of its runtime to take its boxes back after a crash (T-T7.1.2).
pub trait Recover: Runtime {
    /// What names box `id`'s processes after the node is gone, recorded with the box once it has any (its
    /// first member started): a process and its start time, which tells it from one given its pid since.
    fn trace(&self, id: &BoxId) -> Result<Option<ProcId>, Refusal>;
    /// Takes back box `spec`, its processes the ones `trace` names where they still run.
    fn adopt(&self, spec: &BoxSpec, trace: Option<ProcId>) -> Result<(), Refusal>;
    /// Ends what still runs of what `trace` names, for a box recovery removes.
    fn end(&self, trace: Option<ProcId>) -> Result<(), Refusal>;
    /// For an `oci` box: whether its init still runs, so recovery takes it back; `None` for a class with
    /// no init. By default an `oci` box's init went with the node.
    fn alive(&self, spec: &BoxSpec) -> Result<Option<bool>, Refusal> {
        Ok(match spec.class.class() {
            Class::Oci => Some(false),
            Class::Process => None,
        })
    }
    /// Removes what is left of box `spec` that `trace` doesn't name, for a box recovery removes. By
    /// default there is nothing.
    fn discard(&self, spec: &BoxSpec) -> Result<(), Refusal> {
        let _ = spec;
        Ok(())
    }
    /// The core's loop, for connections of its own ([`Host::attach`]). By default it has none.
    fn attach(&self, lp: Arc<dyn Loop>) -> Result<(), Refusal> {
        let _ = lp;
        Ok(())
    }
    /// A call it answers later ([`Host::forward`]). By default every call runs on the worker.
    fn forward(
        &self,
        call: Call,
        stdio: Stdio,
        ends: bool,
        reply: Reply,
        ended: serve::Ended,
    ) -> Forward {
        let _ = (ends, reply, ended);
        Forward::Back(Box::new((call, stdio)))
    }
}

/// A runtime whose boxes' records the node keeps.
pub struct Recorded<R> {
    runtime: R,
    books: Arc<Books>,
}

/// The node's records of its boxes, shared with the completions of calls its guest answers.
struct Books {
    store: Store,
    abandoned: Abandoned,
    /// Boxes a recovery took back, which inspect reports as adopted.
    adopted: Mutex<BTreeSet<BoxId>>,
}

impl<R: Recover> Recorded<R> {
    /// `runtime`'s boxes, their records in `state_dir`. Does not recover; call [`Recorded::recover`]. The records
    /// that needn't outlive the host ([`crate::store`]) are kept beside the rest, in `@records` under it (`@` is in
    /// no box id, so it is never read as a box's record): these OSes give the node no tmpfs runtime directory.
    pub fn open(runtime: R, state_dir: &Path) -> Result<Self, Refusal> {
        Ok(Self {
            runtime,
            books: Arc::new(Books {
                store: Store::open(state_dir, &state_dir.join("@records"))?,
                abandoned: Abandoned::default(),
                adopted: Mutex::new(BTreeSet::new()),
            }),
        })
    }

    /// After a restart: takes each box back or cleans it up as its record says (T-T7.1.2), by
    /// `hex_core::recovery`'s decisions, Linux's node's. A box is a record and the runtime's processes it
    /// names: a record made is the box, whatever runs in it now, so it is taken back; one being made or
    /// removed when the node stopped is removed, with what its record names ended. Safe to call again after
    /// a crash during it: ending a box tolerates its processes being gone already.
    pub fn recover(&self) -> Result<RecoverySummary, Refusal> {
        // What a crash at a record write left: a temp file never renamed into place.
        self.books.store.remove_strays()?;
        let mut summary = RecoverySummary::default();
        let records = self.books.store.all()?;
        let mut seen: BTreeMap<BoxId, Seen> = BTreeMap::new();
        for (id, record) in &records {
            let alive = match record {
                Ok(r) => self.runtime.alive(&r.spec)?,
                Err(_) => None,
            };
            seen.insert(
                id.clone(),
                Seen {
                    record: Some(record.clone()),
                    // A record is the box here, whatever runs in it now: its runtime's part stands for the
                    // cgroup Linux's node finds.
                    parts: Parts {
                        cgroup: true,
                        ..Parts::default()
                    },
                    init_alive: alive,
                    start_interrupted: false,
                },
            );
        }
        for (id, decision) in recovery::plan(&seen) {
            let record = records.get(&id).and_then(|r| r.as_ref().ok());
            let trace = record.and_then(|r| r.init);
            match decision {
                Decision::Adopt => {
                    if let Some(r) = record {
                        self.runtime.adopt(&r.spec, trace)?;
                    }
                    self.books
                        .adopted
                        .lock()
                        .map_err(|_| poisoned())?
                        .insert(id.clone());
                    summary.adopted.push(id);
                }
                Decision::Remove(why) => {
                    self.runtime.end(trace)?;
                    if let Some(r) = record {
                        self.runtime.discard(&r.spec)?;
                    }
                    self.books.store.remove(&id)?;
                    summary.removed.push((id, why_text(&why)));
                }
            }
        }
        Ok(summary)
    }

    fn counted(&self, info: BoxInfo) -> Result<BoxInfo, Refusal> {
        self.books.counted(info)
    }
}

impl Books {
    /// `info` with the abandoned programs counted for its box, recorded and not yet, and whether a recovery
    /// took it back.
    fn counted(&self, mut info: BoxInfo) -> Result<BoxInfo, Refusal> {
        let recorded = match self.store.get(&info.id)? {
            Some(Ok(record)) => record.abandoned,
            _ => 0,
        };
        info.abandoned = recorded.saturating_add(self.abandoned.unrecorded(&info.id)?);
        info.adopted |= self
            .adopted
            .lock()
            .map_err(|_| poisoned())?
            .contains(&info.id);
        Ok(info)
    }
}

fn poisoned() -> Refusal {
    Refusal::Broken("the node's adopted boxes' lock was poisoned".to_owned())
}

impl<R: Recover> Runtime for Recorded<R> {
    fn classes(&self) -> Vec<Class> {
        self.runtime.classes()
    }

    /// Records the box as being made before anything is made, so a crash leaves a half-made box known as
    /// one (T-T7.1.2), then as made once the runtime has made it; a refused create leaves no record.
    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        let mut record = BoxRecord::new(spec.clone(), Phase::Creating, Vec::new());
        self.books.store.put(&record)?;
        let info = match self.runtime.create(spec) {
            Ok(info) => info,
            Err(why) => {
                let _ = self.books.store.remove(&record.spec.id);
                return Err(why);
            }
        };
        record.phase = Phase::Ready;
        self.books.store.put(&record)?;
        self.counted(info)
    }

    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        self.runtime.start(id)
    }

    /// Runs `program`, and once the box has processes, records what names them after a crash.
    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        let proc = self.runtime.exec(id, program, stdio)?;
        if let Some(Ok(record)) = self.books.store.get(id)?
            && record.init.is_none()
            && let Some(trace) = self.runtime.trace(id)?
        {
            self.books.store.update(id, |r| {
                r.init = Some(trace);
                true
            })?;
        }
        Ok(proc)
    }

    fn signal(&self, id: &BoxId, signal: Signal) -> Result<(), Refusal> {
        self.runtime.signal(id, signal)
    }

    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
        self.counted(self.runtime.inspect(id)?)
    }

    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        self.runtime.usage(id)
    }

    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        self.runtime
            .list()?
            .into_iter()
            .map(|info| self.counted(info))
            .collect()
    }

    /// Records the box as being deleted, deletes it, then removes its record: a crash between leaves a
    /// record recovery finishes.
    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        if let Some(Ok(mut record)) = self.books.store.get(id)? {
            record.phase = Phase::Deleting;
            self.books.store.put(&record)?;
        }
        self.runtime.delete(id)?;
        self.books.store.remove(id)?;
        self.books.abandoned.forget(id);
        if let Ok(mut adopted) = self.books.adopted.lock() {
            adopted.remove(id);
        }
        Ok(())
    }
}

impl<R: Recover> Host for Recorded<R> {
    fn attach(&self, lp: Arc<dyn Loop>) -> Result<(), Refusal> {
        self.runtime.attach(lp)
    }

    /// The calls its runtime answers later go there: an inspect's answer counted as any inspect's is
    /// ([`Books::counted`]). A create or a delete, which write the box's record and stage or remove its
    /// root, runs on the worker.
    fn forward(
        &self,
        call: Call,
        stdio: Stdio,
        ends: bool,
        reply: Reply,
        ended: serve::Ended,
    ) -> Forward {
        match call {
            Call::Inspect(_) => {
                let books = Arc::clone(&self.books);
                let counted: Reply = Box::new(move |got| {
                    reply(match got {
                        Ok(Forwarded::Answer(Answer::Info(info))) => books
                            .counted(info)
                            .map(|info| Forwarded::Answer(Answer::Info(info))),
                        other => other,
                    })
                });
                self.runtime.forward(call, stdio, ends, counted, ended)
            }
            Call::Start(_)
            | Call::Signal { .. }
            | Call::Usage(_)
            | Call::Exec { .. }
            | Call::ExecStreams { .. } => self.runtime.forward(call, stdio, ends, reply, ended),
            call => Forward::Back(Box::new((call, stdio))),
        }
    }

    fn abandoned(&self, id: &BoxId) -> Result<(), Refusal> {
        self.books.abandoned.count(&self.books.store, id)
    }

    /// Network namespaces are Linux's: a node elsewhere has none to count.
    fn netns(&self) -> Result<netns::Counts, Refusal> {
        Err(Refusal::Platform {
            what: "the node's network namespace counts",
            platform: std::env::consts::OS,
            why: "network namespaces are Linux's; this node makes none".to_owned(),
        })
    }
}

/// The macOS `process` class: a box's processes are its process group, named by its leader's pid and start
/// time (`proc_pid_rusage`'s Mach absolute time, in [`ProcId::start_ticks`]).
#[cfg(target_os = "macos")]
impl Recover for hex_runtime::macos::MacRuntime {
    fn trace(&self, id: &BoxId) -> Result<Option<ProcId>, Refusal> {
        Ok(self.group(id)?.and_then(|(pgid, started)| {
            Some(ProcId {
                pid: u32::try_from(pgid).ok()?,
                start_ticks: started,
            })
        }))
    }

    fn adopt(&self, spec: &BoxSpec, trace: Option<ProcId>) -> Result<(), Refusal> {
        hex_runtime::macos::MacRuntime::adopt(self, spec, group(trace))
    }

    fn end(&self, trace: Option<ProcId>) -> Result<(), Refusal> {
        hex_runtime::macos::MacRuntime::end(group(trace))
    }
}

#[cfg(target_os = "macos")]
fn group(trace: Option<ProcId>) -> Option<(i32, u64)> {
    trace.and_then(|t| Some((i32::try_from(t.pid).ok()?, t.start_ticks)))
}

/// A node whose Linux images are its Shards guest's: its `process` boxes are taken back through the host's
/// runtime. Its `oci` boxes live in its guest, which outlives the node's crash and which the node takes back
/// when it starts (`GuestRuntime::recorded_at`): one the guest holds is taken back as it is, and one it
/// doesn't (the guest was gone, and the node booted another) is removed (`hex_core::recovery`: its init has
/// ended). Neither has host processes to end; a box recovery removes that the guest still holds is deleted
/// there.
impl<P: Recover> Recover for hex_runtime::guest::WithGuest<P> {
    fn trace(&self, id: &BoxId) -> Result<Option<ProcId>, Refusal> {
        match self.host().inspect(id) {
            Ok(_) => self.host().trace(id),
            Err(Refusal::NoSuchBox(_)) => Ok(None),
            Err(why) => Err(why),
        }
    }

    fn adopt(&self, spec: &BoxSpec, trace: Option<ProcId>) -> Result<(), Refusal> {
        match spec.class.class() {
            // Its guest holds it already.
            Class::Oci => Ok(()),
            Class::Process => self.host().adopt(spec, trace),
        }
    }

    fn end(&self, trace: Option<ProcId>) -> Result<(), Refusal> {
        self.host().end(trace)
    }

    fn alive(&self, spec: &BoxSpec) -> Result<Option<bool>, Refusal> {
        match spec.class.class() {
            Class::Oci => match self.guest().inspect(&spec.id) {
                Ok(_) => Ok(Some(true)),
                Err(Refusal::NoSuchBox(_)) => Ok(Some(false)),
                Err(why) => Err(why),
            },
            Class::Process => self.host().alive(spec),
        }
    }

    fn discard(&self, spec: &BoxSpec) -> Result<(), Refusal> {
        match spec.class.class() {
            Class::Oci => match self.guest().delete(&spec.id) {
                Ok(()) | Err(Refusal::NoSuchBox(_)) => Ok(()),
                Err(why) => Err(why),
            },
            Class::Process => self.host().discard(spec),
        }
    }

    /// Its guest's control connection, in the event form, joins the core's loop, and so does every guest's
    /// it boots or takes back after: a call through it holds no thread while the guest answers (native plan
    /// step 8 (1)).
    fn attach(&self, lp: Arc<dyn Loop>) -> Result<(), Refusal> {
        self.guest().join_loop(Arc::new(LoopJoins(lp)))
    }

    /// A call about a box its guest holds goes there through the event form's client and is answered
    /// from the loop; one about a host box runs on the worker. With no guest running the box is no box; a
    /// guest whose connection can't be joined to the loop refuses the call, naming why.
    ///
    /// A guest program can't take the caller's own pipes, and the node copies no byte between them (native
    /// plan step 8): each stream an exec carries or asks for is the guest's own stdio connection, whose end
    /// goes to the caller's client with the answer, and the caller's pipes are let go here. Its client
    /// copies between those it carried and their ends; one of a wire version that takes no ends with an
    /// answer is refused before anything is sent.
    fn forward(
        &self,
        call: Call,
        stdio: Stdio,
        ends: bool,
        reply: Reply,
        ended: serve::Ended,
    ) -> Forward {
        let id = match &call {
            Call::Inspect(id) | Call::Start(id) | Call::Usage(id) => id,
            Call::Signal { id, .. } | Call::Exec { id, .. } | Call::ExecStreams { id, .. } => id,
            _ => return Forward::Back(Box::new((call, stdio))),
        };
        match self.host().inspect(id) {
            Err(Refusal::NoSuchBox(_)) => {}
            Ok(_) | Err(_) => return Forward::Back(Box::new((call, stdio))),
        }
        let client = match self.guest().evented() {
            Ok(Some(client)) => client,
            Ok(None) => return Forward::Refused(Refusal::NoSuchBox(id.clone())),
            Err(why) => return Forward::Refused(why),
        };
        let (call, stdio) = match call {
            Call::Exec { id, program, .. } | Call::ExecStreams { id, program, .. } => {
                if let Err(why) = stdio.check() {
                    return Forward::Refused(why);
                }
                let ask = Ask {
                    stdin: stdio.stdin.is_some() || stdio.ask.stdin,
                    stdout: stdio.stdout.is_some() || stdio.ask.stdout,
                    stderr: stdio.stderr.is_some() || stdio.ask.stderr,
                };
                if ask != Ask::default() && !ends {
                    return Forward::Refused(Refusal::Invalid {
                        what: "stdio",
                        why: "a program in the node's guest takes its streams as the guest's own connections, whose ends ride its answer from wire version 3 on; this client's exec is of an earlier version".to_owned(),
                    });
                }
                // The caller's own pipes, let go here: its client keeps its own.
                drop(stdio);
                let streams = hex_core::wire::Streams::default();
                let exec = Call::Exec {
                    id,
                    program,
                    streams,
                };
                let asked = Stdio {
                    ask,
                    ..Stdio::null()
                };
                (exec, asked)
            }
            call => (call, stdio),
        };
        let started = Arc::clone(&client);
        let then: hex_client::Then = Box::new(move |exec, got| {
            reply(match got {
                Ok(Answer::Started { pid }) => Ok(Forwarded::Started {
                    pid,
                    given: started.given(exec),
                    program: Box::new(GuestProgram {
                        client: started,
                        exec,
                    }),
                }),
                Ok(Answer::Refused(why)) | Err(why) => Err(why),
                Ok(answer) => Ok(Forwarded::Answer(answer)),
            })
        });
        // Refused before it was sent: `then` is dropped unrun, and the caller hears why.
        match client.begin(call, stdio, then, ended) {
            Ok(_) => Forward::Taken,
            Err(why) => Forward::Refused(why),
        }
    }
}

/// The core's loop as the guest's runtime joins its connections to it.
struct LoopJoins(Arc<dyn Loop>);

impl hex_runtime::guest::Joins for LoopJoins {
    fn reserve(&self) -> Result<(u64, hex_runtime::guest::Room), Refusal> {
        let token = self.0.token()?;
        let lp = Arc::clone(&self.0);
        Ok((token, Arc::new(move || lp.room(token))))
    }

    fn join(&self, token: u64, pump: hex_client::guest::Pump) -> Result<(), Refusal> {
        self.0.add(token, Box::new(Pumped(pump)))
    }
}

/// The guest's control connection's reading side, as a source of the core's loop.
struct Pumped(hex_client::guest::Pump);

impl serve::Source for Pumped {
    fn handle(&self) -> serve::Handle<'_> {
        self.0.handle()
    }

    fn ready(&mut self) -> serve::Want {
        match self.0.ready() {
            hex_client::guest::Pumped::Read => serve::Want::Read,
            hex_client::guest::Pumped::ReadWrite => serve::Want::ReadWrite,
            hex_client::guest::Pumped::Ended => serve::Want::Done,
        }
    }
}

/// A program in the guest, by its exec's id on the event form's connection.
struct GuestProgram {
    client: Arc<hex_client::guest::Client>,
    exec: u64,
}

impl serve::Remote for GuestProgram {
    fn kill(&self, done: serve::Killed) {
        let done = Arc::new(Mutex::new(Some(done)));
        let answered = Arc::clone(&done);
        let then: hex_client::Then = Box::new(move |_, got| {
            let sent = match got {
                Ok(Answer::Done) => Ok(()),
                Ok(Answer::Refused(why)) | Err(why) => Err(why),
                Ok(_) => Err(Refusal::Broken(
                    "the guest's node answered a kill with something but done".to_owned(),
                )),
            };
            if let Some(done) = answered.lock().ok().and_then(|mut d| d.take()) {
                done(sent);
            }
        });
        if let Err(why) = self.client.kill_then(self.exec, then)
            && let Some(done) = done.lock().ok().and_then(|mut d| d.take())
        {
            done(Err(why));
        }
    }

    fn release(&self) {
        self.client.release(self.exec);
    }
}
