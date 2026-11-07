//! What every client of a node agent does whatever carries its messages: a call waits for its answer by
//! its request's id, a program's end goes to its `Proc`, and the connection's end reaches every call and
//! program still waiting. A [`Transport`] moves the bytes: the agent's local `SOCK_SEQPACKET` socket on
//! Linux (`linux.rs`), or a byte stream to the node agent in a Shards guest (`guest.rs`).

use std::collections::BTreeMap;
use std::sync::mpsc::{RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hex_api::{
    BoxId, BoxInfo, BoxSpec, Class, Exit, Proc, Program, Refusal, Runtime, Signal, Stdio, Usage,
};
use hex_core::netns;
use hex_core::wire::{self, Answer, Call, Header, Limits, Request, Streams};

use crate::{ANSWER_PATIENCE, MAX_LIST};

/// Why a send didn't go.
pub enum Sent {
    /// The connection is gone: the reading side says why.
    Gone,
    Refused(Refusal),
}

/// What carries a client's messages to its node agent.
pub trait Transport: Send + Sync + 'static {
    /// What travels with an exec: its stdio as the transport carries it.
    type Stdio: Send;

    /// An exec's stdio for the request `exec`, made ready to travel with it.
    fn stdio(&self, exec: u64, stdio: Stdio) -> Result<Self::Stdio, Refusal>;
    /// Sends one message, with what travels with it.
    fn send(&self, bytes: &[u8], with: Option<Self::Stdio>) -> Result<(), Sent>;
    /// Ends the connection both ways: the reading side sees the end.
    fn shutdown(&self);
}

pub(crate) fn poisoned<T>(_: std::sync::PoisonError<T>) -> Refusal {
    Refusal::Broken("a lock of the node agent's client was poisoned".to_owned())
}

/// What the reading side and the callers share.
pub struct Shared<T> {
    pub(crate) transport: T,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// Calls waiting for their answers, by request id.
    calls: BTreeMap<u64, Waiting>,
    /// Programs whose end a `Proc` waits for, by their exec's id.
    programs: BTreeMap<u64, Arc<Ending>>,
    next: u64,
    /// Why the connection ended, once it has.
    closed: Option<Refusal>,
    /// Why the agent won't serve the connection, when it said (an answer to no request, id 0).
    refused: Option<Refusal>,
}

struct Waiting {
    answer: SyncSender<Result<Answer, Refusal>>,
    /// A list's boxes from the parts that came so far.
    boxes: Vec<BoxInfo>,
}

/// A program's end, once it has come.
#[derive(Default)]
struct Ending {
    exit: Mutex<Option<Result<Exit, Refusal>>>,
    came: Condvar,
}

impl<T> Shared<T> {
    pub(crate) fn new(transport: T) -> Self {
        Self {
            transport,
            state: Mutex::new(State::default()),
        }
    }

    fn state(&self) -> Result<MutexGuard<'_, State>, Refusal> {
        self.state.lock().map_err(poisoned)
    }

    /// Hands one message to whoever waits for it: an answer to its call, a program's end to its
    /// `Proc`. One nobody waits for (a call that gave up waiting, a program released) has no taker.
    pub(crate) fn deliver(&self, header: Header, answer: Answer) -> Result<(), Refusal> {
        let mut state = self.state()?;
        match answer {
            Answer::Exited(exit) => {
                if let Some(ending) = state.programs.get(&header.id) {
                    *ending.exit.lock().map_err(poisoned)? = Some(Ok(exit));
                    ending.came.notify_all();
                }
            }
            Answer::Refused(refusal) if header.id == 0 => state.refused = Some(refusal),
            Answer::Boxes { infos, more: true } => {
                if let Some(waiting) = state.calls.get_mut(&header.id) {
                    if waiting.boxes.len().saturating_add(infos.len()) > MAX_LIST {
                        let waiting = state.calls.remove(&header.id);
                        if let Some(w) = waiting {
                            let _ = w.answer.try_send(Err(Refusal::Exhausted {
                                what: "boxes in one list",
                                bound: MAX_LIST,
                            }));
                        }
                    } else {
                        waiting.boxes.extend(infos);
                    }
                }
            }
            answer => {
                if let Some(mut waiting) = state.calls.remove(&header.id) {
                    let answer = match answer {
                        Answer::Boxes { infos, more } => {
                            waiting.boxes.extend(infos);
                            Answer::Boxes {
                                infos: std::mem::take(&mut waiting.boxes),
                                more,
                            }
                        }
                        other => other,
                    };
                    let _ = waiting.answer.try_send(Ok(answer));
                }
            }
        }
        Ok(())
    }

    /// The connection has ended: every call waiting and every program's `Proc` hear why.
    pub(crate) fn close(&self, why: Refusal) {
        let Ok(mut state) = self.state() else {
            return;
        };
        let why = state.refused.take().unwrap_or(why);
        for (_, waiting) in std::mem::take(&mut state.calls) {
            let _ = waiting.answer.try_send(Err(why.clone()));
        }
        let lost = Refusal::Broken(format!(
            "the connection to the node agent closed before the program's end came: {why}"
        ));
        for ending in state.programs.values() {
            if let Ok(mut exit) = ending.exit.lock()
                && exit.is_none()
            {
                *exit = Some(Err(lost.clone()));
                ending.came.notify_all();
            }
        }
        state.closed = Some(why);
    }
}

/// The connection, held by the client and by each `Proc` it returned: it closes when the last goes.
pub(crate) struct Conn<T: Transport> {
    shared: Arc<Shared<T>>,
    reader: Mutex<Option<JoinHandle<()>>>,
}

impl<T: Transport> Drop for Conn<T> {
    fn drop(&mut self) {
        // The reading side sees the end and goes; the agent sees the hang-up.
        self.shared.transport.shutdown();
        if let Some(reader) = self.reader.lock().ok().and_then(|mut r| r.take()) {
            let _ = reader.join();
        }
    }
}

/// A node agent, reached through a transport.
pub struct Core<T: Transport> {
    conn: Arc<Conn<T>>,
    classes: Vec<Class>,
    limits: Limits,
    /// The wire version this client writes: [`wire::FIRST`] for its hello, then the highest both it
    /// and its agent speak.
    version: u16,
}

impl<T: Transport> Core<T> {
    /// Says hello over `shared`, whose reading side is `reader`, and learns what the agent runs and
    /// its bounds.
    pub(crate) fn hello(shared: Arc<Shared<T>>, reader: JoinHandle<()>) -> Result<Self, Refusal> {
        let mut client = Self {
            conn: Arc::new(Conn {
                shared,
                reader: Mutex::new(Some(reader)),
            }),
            classes: Vec::new(),
            limits: Limits {
                connections: 1,
                message_bytes: wire::MAX_MESSAGE,
                in_flight: 1,
                programs: 0,
            },
            // The hello goes in the first version, which an agent of any version reads.
            version: wire::FIRST,
        };
        match client.call(&Request::Hello, |_| Ok(None), None)?.1 {
            Answer::Hello(hello) if hello.version < wire::FIRST => Err(Refusal::Broken(format!(
                "the node agent says it speaks wire version {}, below the first",
                hello.version
            ))),
            Answer::Hello(hello) => {
                client.classes = hello.classes;
                client.limits = Limits {
                    message_bytes: hello.limits.message_bytes.min(wire::MAX_MESSAGE),
                    ..hello.limits
                };
                client.version = hello.version.min(wire::VERSION);
                Ok(client)
            }
            Answer::Refused(r) => Err(r),
            other => Err(unexpected("hello", &other)),
        }
    }

    /// The agent's bounds on this connection and its socket, as its hello told them.
    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// The agent's network namespace counts (decision 4), as `hex_node::Node::netns` reads them in
    /// process. An agent of wire version 1 has none to give: the call is refused by type before it is
    /// sent, as a kind version 1 doesn't define.
    pub fn netns(&self) -> Result<netns::Counts, Refusal> {
        match self
            .call(&Request::Call(Call::Netns), |_| Ok(None), None)?
            .1
        {
            Answer::Netns(counts) => Ok(counts),
            Answer::Refused(r) => Err(r),
            other => Err(unexpected("netns", &other)),
        }
    }

    /// Sends `request`, with what `with` makes for its id to travel with it, and waits for its
    /// answer. With `program`, a `Proc`'s place for the end of the program an exec starts is made
    /// before the exec goes, so the end can't come first.
    fn call(
        &self,
        request: &Request,
        with: impl FnOnce(u64) -> Result<Option<T::Stdio>, Refusal>,
        program: Option<Arc<Ending>>,
    ) -> Result<(u64, Answer), Refusal> {
        let shared = &self.conn.shared;
        let (tx, rx) = sync_channel(1);
        let id = {
            let mut state = shared.state()?;
            if let Some(why) = &state.closed {
                return Err(why.clone());
            }
            let id = state
                .next
                .checked_add(1)
                .ok_or_else(|| Refusal::Broken("the client's request ids ran out".to_owned()))?;
            state.next = id;
            state.calls.insert(
                id,
                Waiting {
                    answer: tx,
                    boxes: Vec::new(),
                },
            );
            if let Some(program) = program {
                state.programs.insert(id, program);
            }
            id
        };
        let forget = |shared: &Shared<T>| {
            if let Ok(mut state) = shared.state() {
                state.calls.remove(&id);
                state.programs.remove(&id);
            }
        };
        let sent = wire::request(self.version, id, request, self.limits.message_bytes)
            .map_err(|fault| Sent::Refused(fault.into()))
            .and_then(|bytes| {
                let with = with(id).map_err(Sent::Refused)?;
                shared.transport.send(&bytes, with)
            });
        match sent {
            Ok(()) => {}
            // The agent has gone, or closed the connection: why reaches the waiting call through
            // the reading side, which sees the end.
            Err(Sent::Gone) => {}
            Err(Sent::Refused(r)) => {
                forget(shared);
                return Err(r);
            }
        }
        let answer = match rx.recv_timeout(ANSWER_PATIENCE) {
            Ok(answer) => answer,
            Err(RecvTimeoutError::Timeout) => {
                forget(shared);
                return Err(Refusal::Os {
                    op: "waiting for the node agent's answer",
                    why: format!("none came in {ANSWER_PATIENCE:?}"),
                });
            }
            Err(RecvTimeoutError::Disconnected) => {
                Err(shared.state()?.closed.clone().unwrap_or_else(|| {
                    Refusal::Broken("the node agent's answer was lost".to_owned())
                }))
            }
        };
        match answer {
            Ok(answer) => Ok((id, answer)),
            Err(r) => {
                forget(shared);
                Err(r)
            }
        }
    }

    /// A call that carries nothing beside its message.
    fn plain(&self, call: Call) -> Result<Answer, Refusal> {
        Ok(self.call(&Request::Call(call), |_| Ok(None), None)?.1)
    }

    /// A call whose answer is `Done`.
    fn done(&self, call: Call, name: &'static str) -> Result<(), Refusal> {
        match self.plain(call)? {
            Answer::Done => Ok(()),
            Answer::Refused(r) => Err(r),
            other => Err(unexpected(name, &other)),
        }
    }

    /// A call whose answer is a box's info.
    fn info(&self, call: Call, name: &'static str) -> Result<BoxInfo, Refusal> {
        match self.plain(call)? {
            Answer::Info(info) => Ok(info),
            Answer::Refused(r) => Err(r),
            other => Err(unexpected(name, &other)),
        }
    }
}

pub(crate) fn unexpected(call: &str, answer: &Answer) -> Refusal {
    let kind = match answer {
        Answer::Hello(_) => "a hello",
        Answer::Info(_) => "a box's info",
        Answer::Done => "done",
        Answer::Started { .. } => "a started program",
        Answer::Usage(_) => "a usage",
        Answer::Boxes { .. } => "a list",
        Answer::Refused(_) => "a refusal",
        Answer::Exited(_) => "a program's end",
        Answer::Netns(_) => "network namespace counts",
    };
    Refusal::Broken(format!("the node agent answered a {call} with {kind}"))
}

/// The most of a program's arguments and environment that crosses to the node agent: a program can't pass
/// the bound on one wire message (`hex_core::wire::MAX_MESSAGE`), so one that would is refused here, as a
/// program, before it is encoded; the node then holds it to its OS's own exec bound (HEX-RT-14).
pub const WIRE_EXEC_LIMIT: hex_api::ExecLimit = hex_api::ExecLimit {
    block: wire::MAX_MESSAGE,
    counts: hex_api::BlockCount::Execve,
    string: None,
    unit: hex_api::StringUnit::Bytes,
    source: "the node agent's wire (one message, hex_core::wire::MAX_MESSAGE)",
};

impl<T: Transport> Runtime for Core<T> {
    fn classes(&self) -> Vec<Class> {
        self.classes.clone()
    }

    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        if let hex_api::ClassSpec::Oci(oci) = &spec.class {
            oci.init.fits(&WIRE_EXEC_LIMIT)?;
        }
        self.info(Call::Create(spec), "create")
    }

    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        self.done(Call::Start(id.clone()), "start")
    }

    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        program.fits(&WIRE_EXEC_LIMIT)?;
        let streams = Streams {
            stdin: stdio.stdin.is_some(),
            stdout: stdio.stdout.is_some(),
            stderr: stdio.stderr.is_some(),
        };
        let ending = Arc::new(Ending::default());
        let request = Request::Call(Call::Exec {
            id: id.clone(),
            program,
            streams,
        });
        let transport = &self.conn.shared.transport;
        let (exec, answer) = self.call(
            &request,
            |exec| transport.stdio(exec, stdio).map(Some),
            Some(Arc::clone(&ending)),
        )?;
        match answer {
            Answer::Started { pid } => Ok(Box::new(RemoteProc {
                conn: Arc::clone(&self.conn),
                version: self.version,
                exec,
                pid,
                ending,
                exit: None,
            })),
            other => {
                if let Ok(mut state) = self.conn.shared.state() {
                    state.programs.remove(&exec);
                }
                match other {
                    Answer::Refused(r) => Err(r),
                    other => Err(unexpected("exec", &other)),
                }
            }
        }
    }

    fn signal(&self, id: &BoxId, signal: Signal) -> Result<(), Refusal> {
        self.done(
            Call::Signal {
                id: id.clone(),
                signal,
            },
            "signal",
        )
    }

    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
        self.info(Call::Inspect(id.clone()), "inspect")
    }

    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        match self.plain(Call::Usage(id.clone()))? {
            Answer::Usage(usage) => Ok(usage),
            Answer::Refused(r) => Err(r),
            other => Err(unexpected("usage", &other)),
        }
    }

    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        match self.plain(Call::List)? {
            Answer::Boxes { infos, .. } => Ok(infos),
            Answer::Refused(r) => Err(r),
            other => Err(unexpected("list", &other)),
        }
    }

    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        self.done(Call::Delete(id.clone()), "delete")
    }
}

/// A program a node agent started for this client: its end comes as a message.
struct RemoteProc<T: Transport> {
    conn: Arc<Conn<T>>,
    /// The wire version its client writes, which its release goes in.
    version: u16,
    exec: u64,
    pid: u32,
    ending: Arc<Ending>,
    exit: Option<Exit>,
}

impl<T: Transport> RemoteProc<T> {
    /// Its end if it has come, waiting for it at most `timeout` (none: not at all).
    fn wait(&mut self, timeout: Option<Duration>) -> Result<Option<Exit>, Refusal> {
        if let Some(exit) = self.exit {
            return Ok(Some(exit));
        }
        let deadline = timeout.map(|t| Instant::now().checked_add(t));
        let mut came = self.ending.exit.lock().map_err(poisoned)?;
        loop {
            match &*came {
                Some(Ok(exit)) => {
                    self.exit = Some(*exit);
                    return Ok(Some(*exit));
                }
                Some(Err(why)) => return Err(why.clone()),
                None => {}
            }
            let left = match deadline {
                None => return Ok(None),
                // A timeout past what an instant holds waits as long as it can at a time.
                Some(None) => timeout.unwrap_or(Duration::ZERO),
                Some(Some(deadline)) => deadline.saturating_duration_since(Instant::now()),
            };
            if left.is_zero() {
                return Ok(None);
            }
            came = self
                .ending
                .came
                .wait_timeout(came, left)
                .map_err(poisoned)?
                .0;
        }
    }
}

impl<T: Transport> Proc for RemoteProc<T> {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn try_wait(&mut self) -> Result<Option<Exit>, Refusal> {
        self.wait(None)
    }

    fn wait_for(&mut self, timeout: Duration) -> Result<Option<Exit>, Refusal> {
        self.wait(Some(timeout))
    }
}

impl<T: Transport> Drop for RemoteProc<T> {
    /// Let go before its end came: the agent is told, and lets the program run on as dropping an
    /// in-process `Proc` does. A release that can't be sent (the connection gone) needs none: the
    /// agent's disconnect rule has the program.
    fn drop(&mut self) {
        let shared = &self.conn.shared;
        let ended = self.exit.is_some()
            || self
                .ending
                .exit
                .lock()
                .map(|exit| exit.is_some())
                .unwrap_or(true);
        if let Ok(mut state) = shared.state() {
            state.programs.remove(&self.exec);
        }
        if !ended
            && let Ok(bytes) = wire::request(
                self.version,
                self.exec,
                &Request::Release,
                wire::MAX_MESSAGE,
            )
        {
            let _ = shared.transport.send(&bytes, None);
        }
    }
}
