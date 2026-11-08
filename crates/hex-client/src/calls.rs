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

/// What takes a call's answer in the event form ([`Core::begin`]), with its request's id (an exec's names
/// its program): run once, on the thread that reads the connection, after the client's lock is let go.
pub type Then = Box<dyn FnOnce(u64, Result<Answer, Refusal>) + Send>;
/// What takes an exec'd program's end in the event form: run once, as [`Then`] is.
pub type Ended = Box<dyn FnOnce(Result<Exit, Refusal>) + Send>;

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
    /// Whether it makes the streams an exec asks for itself ([`Transport::asks`]), rather than asking the
    /// agent, which gives their ends with its answer ([`Answer::Given`]).
    const MAKES_ASKED: bool = false;
    /// The streams `ask` names for exec `exec`, made here, its program's ends the agent's to find (a
    /// guest's stdio connections, named for it): only where [`Transport::MAKES_ASKED`].
    fn asks(&self, exec: u64, ask: hex_api::Ask) -> Result<hex_api::Given, Refusal> {
        let _ = (exec, ask);
        Err(Refusal::Broken(
            "this transport asks the agent for an exec's streams".to_owned(),
        ))
    }
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
    /// Programs whose end something waits for, by their exec's id.
    programs: BTreeMap<u64, Watching>,
    next: u64,
    /// Why the connection ended, once it has.
    closed: Option<Refusal>,
    /// Why the agent won't serve the connection, when it said (an answer to no request, id 0).
    refused: Option<Refusal>,
    /// The streams a given answer's descriptors are, by its request's id, until its exec takes them.
    given: BTreeMap<u64, hex_api::Given>,
}

struct Waiting {
    answer: Taker,
    /// A list's boxes from the parts that came so far.
    boxes: Vec<BoxInfo>,
}

/// What takes a call's answer: a caller waiting on it, or a completion ([`Then`]).
enum Taker {
    Wait(SyncSender<Result<Answer, Refusal>>),
    Then(Then),
}

/// What waits for a program's end: its `Proc`, or a completion ([`Ended`]).
enum Watching {
    Proc(Arc<Ending>),
    Then(Ended),
}

/// A completion to run once the client's lock is let go.
type Due = Box<dyn FnOnce() + Send>;

impl Taker {
    /// Gives `answer` to its taker: at once to a waiting caller, or as a completion run later.
    fn give(self, id: u64, answer: Result<Answer, Refusal>, due: &mut Vec<Due>) {
        match self {
            Self::Wait(tx) => {
                let _ = tx.try_send(answer);
            }
            Self::Then(then) => due.push(Box::new(move || then(id, answer))),
        }
    }
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
    /// `Proc` or completion. One nobody waits for (a call that gave up waiting, a program released) has no
    /// taker. Completions run after the lock is let go, so one may call the client again.
    pub(crate) fn deliver(&self, header: Header, answer: Answer) -> Result<(), Refusal> {
        self.deliver_with(header, answer, Vec::new())
    }

    /// [`Shared::deliver`], with the descriptors that came with the message: a given answer's, one for each
    /// stream it names, in order, kept for its exec.
    #[cfg(unix)]
    pub(crate) fn deliver_with(
        &self,
        header: Header,
        answer: Answer,
        fds: Vec<std::os::fd::OwnedFd>,
    ) -> Result<(), Refusal> {
        if let Answer::Given { given, .. } = &answer {
            let mut fds = fds.into_iter();
            let mut take = |on: bool| if on { fds.next() } else { None };
            let ends = hex_api::Given {
                stdin: take(given.stdin),
                stdout: take(given.stdout),
                stderr: take(given.stderr),
            };
            self.state()?.given.insert(header.id, ends);
        }
        self.deliver_inner(header, answer)
    }

    #[cfg(not(unix))]
    pub(crate) fn deliver_with(
        &self,
        header: Header,
        answer: Answer,
        _fds: Vec<()>,
    ) -> Result<(), Refusal> {
        self.deliver_inner(header, answer)
    }

    fn deliver_inner(&self, header: Header, answer: Answer) -> Result<(), Refusal> {
        let mut due: Vec<Due> = Vec::new();
        {
            let mut state = self.state()?;
            match answer {
                Answer::Exited(exit) => match state.programs.remove(&header.id) {
                    Some(Watching::Proc(ending)) => {
                        *ending.exit.lock().map_err(poisoned)? = Some(Ok(exit));
                        ending.came.notify_all();
                        // Its Proc forgets it when it is dropped or has its end.
                        state.programs.insert(header.id, Watching::Proc(ending));
                    }
                    Some(Watching::Then(ended)) => due.push(Box::new(move || ended(Ok(exit)))),
                    None => {}
                },
                Answer::Refused(refusal) if header.id == 0 => state.refused = Some(refusal),
                Answer::Boxes { infos, more: true } => {
                    if let Some(waiting) = state.calls.get_mut(&header.id) {
                        if waiting.boxes.len().saturating_add(infos.len()) > MAX_LIST {
                            if let Some(w) = state.calls.remove(&header.id) {
                                w.answer.give(
                                    header.id,
                                    Err(Refusal::Exhausted {
                                        what: "boxes in one list",
                                        bound: MAX_LIST,
                                    }),
                                    &mut due,
                                );
                            }
                        } else {
                            waiting.boxes.extend(infos);
                        }
                    }
                }
                answer => {
                    // An exec that started no program has no end to come: a completion waiting
                    // for one is let go.
                    if !matches!(answer, Answer::Started { .. } | Answer::Given { .. })
                        && matches!(state.programs.get(&header.id), Some(Watching::Then(_)))
                    {
                        state.programs.remove(&header.id);
                    }
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
                        waiting.answer.give(header.id, Ok(answer), &mut due);
                    }
                }
            }
        }
        for run in due {
            run();
        }
        Ok(())
    }

    /// The connection has ended: every call waiting and every program's `Proc` or completion hear why.
    pub(crate) fn close(&self, why: Refusal) {
        let mut due: Vec<Due> = Vec::new();
        {
            let Ok(mut state) = self.state() else {
                return;
            };
            let why = state.refused.take().unwrap_or(why);
            for (id, waiting) in std::mem::take(&mut state.calls) {
                waiting.answer.give(id, Err(why.clone()), &mut due);
            }
            let lost = Refusal::Broken(format!(
                "the connection to the node agent closed before the program's end came: {why}"
            ));
            for (exec, watching) in std::mem::take(&mut state.programs) {
                match watching {
                    Watching::Proc(ending) => {
                        if let Ok(mut exit) = ending.exit.lock()
                            && exit.is_none()
                        {
                            *exit = Some(Err(lost.clone()));
                            ending.came.notify_all();
                        }
                        state.programs.insert(exec, Watching::Proc(ending));
                    }
                    Watching::Then(ended) => {
                        let lost = lost.clone();
                        due.push(Box::new(move || ended(Err(lost))));
                    }
                }
            }
            state.closed = Some(why);
        }
        for run in due {
            run();
        }
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

impl<T: Transport> Conn<T> {
    /// Sends `request`, with what `with` makes for its id to travel with it, its answer to go to
    /// `answer` and an exec's program's end to `program`, whose place is made before the exec goes, so
    /// the end can't come first. Its id, once sent (or gone with the connection, whose end then reaches
    /// the taker); a request that can't be sent is refused and forgotten.
    fn send(
        &self,
        version: u16,
        bound: usize,
        request: &Request,
        with: impl FnOnce(u64) -> Result<Option<T::Stdio>, Refusal>,
        answer: Taker,
        program: Option<Watching>,
    ) -> Result<u64, Refusal> {
        let shared = &self.shared;
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
                    answer,
                    boxes: Vec::new(),
                },
            );
            if let Some(program) = program {
                state.programs.insert(id, program);
            }
            id
        };
        let sent = wire::request(version, id, request, bound)
            .map_err(|fault| Sent::Refused(fault.into()))
            .and_then(|bytes| {
                let with = with(id).map_err(Sent::Refused)?;
                shared.transport.send(&bytes, with)
            });
        match sent {
            // The agent has gone, or closed the connection: why reaches the taker through the
            // reading side, which sees the end.
            Ok(()) | Err(Sent::Gone) => Ok(id),
            Err(Sent::Refused(r)) => {
                self.forget(id);
                Err(r)
            }
        }
    }

    /// Forgets call `id` and its program's place: no answer or end of it is taken from now.
    fn forget(&self, id: u64) {
        if let Ok(mut state) = self.shared.state() {
            state.calls.remove(&id);
            state.programs.remove(&id);
        }
    }

    /// Sends `request` and waits for its answer, as [`Conn::send`] does with a caller waiting.
    fn call(
        &self,
        version: u16,
        bound: usize,
        request: &Request,
        with: impl FnOnce(u64) -> Result<Option<T::Stdio>, Refusal>,
        program: Option<Arc<Ending>>,
    ) -> Result<(u64, Answer), Refusal> {
        let (tx, rx) = sync_channel(1);
        let id = self.send(
            version,
            bound,
            request,
            with,
            Taker::Wait(tx),
            program.map(Watching::Proc),
        )?;
        let answer = match rx.recv_timeout(ANSWER_PATIENCE) {
            Ok(answer) => answer,
            Err(RecvTimeoutError::Timeout) => {
                self.forget(id);
                return Err(Refusal::Os {
                    op: "waiting for the node agent's answer",
                    why: format!("none came in {ANSWER_PATIENCE:?}"),
                });
            }
            Err(RecvTimeoutError::Disconnected) => {
                Err(self.shared.state()?.closed.clone().unwrap_or_else(|| {
                    Refusal::Broken("the node agent's answer was lost".to_owned())
                }))
            }
        };
        match answer {
            Ok(answer) => Ok((id, answer)),
            Err(r) => {
                self.forget(id);
                Err(r)
            }
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
        let client = Self::unknown(shared, Some(reader));
        let answer = client.call(&Request::Hello, |_| Ok(None), None)?.1;
        client.greeted(answer)
    }

    /// Whether its connection has ended: no call it sends is answered.
    pub fn closed(&self) -> bool {
        !self
            .conn
            .shared
            .state()
            .is_ok_and(|state| state.closed.is_none())
    }

    /// Says hello over `shared` with no reading side of its own: `read` reads what has come on the
    /// connection, waiting at most a bounded time, until the hello's answer is among it.
    pub(crate) fn hello_inline(
        shared: Arc<Shared<T>>,
        mut read: impl FnMut(&Shared<T>) -> Result<(), Refusal>,
    ) -> Result<Self, Refusal> {
        let client = Self::unknown(Arc::clone(&shared), None);
        let (tx, rx) = sync_channel(1);
        client.conn.send(
            wire::FIRST,
            wire::MAX_MESSAGE,
            &Request::Hello,
            |_| Ok(None),
            Taker::Wait(tx),
            None,
        )?;
        let answer = loop {
            match rx.try_recv() {
                Ok(answer) => break answer?,
                Err(std::sync::mpsc::TryRecvError::Empty) => read(&shared)?,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err(shared.state()?.closed.clone().unwrap_or_else(|| {
                        Refusal::Broken("the node agent's hello was lost".to_owned())
                    }));
                }
            }
        };
        client.greeted(answer)
    }

    /// A client over `shared` that hasn't said hello: what the agent runs and its bounds are learned
    /// from the hello's answer ([`Core::greeted`]). Its hello goes in the first version, which an agent of
    /// any version reads.
    pub(crate) fn unknown(shared: Arc<Shared<T>>, reader: Option<JoinHandle<()>>) -> Self {
        Self {
            conn: Arc::new(Conn {
                shared,
                reader: Mutex::new(reader),
            }),
            classes: Vec::new(),
            limits: Limits {
                connections: 1,
                message_bytes: wire::MAX_MESSAGE,
                in_flight: 1,
                programs: 0,
            },
            version: wire::FIRST,
        }
    }

    /// This client, told by the hello's `answer` what its agent runs, its bounds and its version.
    pub(crate) fn greeted(mut self, answer: Answer) -> Result<Self, Refusal> {
        match answer {
            Answer::Hello(hello) if hello.version < wire::FIRST => Err(Refusal::Broken(format!(
                "the node agent says it speaks wire version {}, below the first",
                hello.version
            ))),
            Answer::Hello(hello) => {
                self.classes = hello.classes;
                self.limits = Limits {
                    message_bytes: hello.limits.message_bytes.min(wire::MAX_MESSAGE),
                    ..hello.limits
                };
                self.version = hello.version.min(wire::VERSION);
                Ok(self)
            }
            Answer::Refused(r) => Err(r),
            other => Err(unexpected("hello", &other)),
        }
    }

    /// The event form of a call: sends `call`, an exec's with `stdio`, and returns its request's id at
    /// once. `then` gets its answer on the thread that reads the connection, and an exec's program's end
    /// goes to `ended` once its start has come. A call that can't be sent is refused here, and `then`
    /// never runs.
    pub fn begin(
        &self,
        call: Call,
        stdio: Stdio,
        then: Then,
        ended: Ended,
    ) -> Result<u64, Refusal> {
        let transport = &self.conn.shared.transport;
        let (request, program) = match call {
            Call::Exec { id, program, .. } => {
                let streams = Streams {
                    stdin: stdio.stdin.is_some(),
                    stdout: stdio.stdout.is_some(),
                    stderr: stdio.stderr.is_some(),
                };
                let request = Request::Call(Call::Exec {
                    id,
                    program,
                    streams,
                });
                return self.conn.send(
                    self.version,
                    self.limits.message_bytes,
                    &request,
                    move |exec| transport.stdio(exec, stdio).map(Some),
                    Taker::Then(then),
                    Some(Watching::Then(ended)),
                );
            }
            call => (Request::Call(call), None),
        };
        self.conn.send(
            self.version,
            self.limits.message_bytes,
            &request,
            |_| Ok(None),
            Taker::Then(then),
            program,
        )
    }

    /// The event form's release of exec `exec`'s program: its end is no longer taken, and the agent lets
    /// it run on, as dropping an in-process `Proc` does. One the connection can't carry needs none: the
    /// agent's disconnect rule has it.
    pub fn release(&self, exec: u64) {
        let shared = &self.conn.shared;
        if let Ok(mut state) = shared.state() {
            state.programs.remove(&exec);
        }
        if let Ok(bytes) = wire::request(
            self.version,
            exec,
            &Request::Release,
            self.limits.message_bytes,
        ) {
            let _ = shared.transport.send(&bytes, None);
        }
    }

    /// The event form's kill of exec `exec`'s program: `then` hears whether the agent sent it.
    pub fn kill_then(&self, exec: u64, then: Then) -> Result<u64, Refusal> {
        self.begin(Call::Kill { exec }, Stdio::null(), then, Box::new(|_| {}))
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

    /// Sends `request` on the connection, with what `with` makes for its id, and waits for its answer.
    fn call(
        &self,
        request: &Request,
        with: impl FnOnce(u64) -> Result<Option<T::Stdio>, Refusal>,
        program: Option<Arc<Ending>>,
    ) -> Result<(u64, Answer), Refusal> {
        self.conn.call(
            self.version,
            self.limits.message_bytes,
            request,
            with,
            program,
        )
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
        Answer::Given { .. } => "a started program with streams",
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

    /// Streams asked for are made by the transport where it can (a guest's, no copy) or asked of the
    /// agent, which gives their ends with its answer (wire version 3's [`Call::ExecStreams`]).
    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        program.fits(&WIRE_EXEC_LIMIT)?;
        stdio.check()?;
        let ask = stdio.ask;
        let asking = ask != hex_api::Ask::default();
        let mut streams = Streams {
            stdin: stdio.stdin.is_some(),
            stdout: stdio.stdout.is_some(),
            stderr: stdio.stderr.is_some(),
        };
        let ending = Arc::new(Ending::default());
        let transport = &self.conn.shared.transport;
        // Made here, the asked streams ride beside the exec as its own do; asked of the agent, they come
        // back with its answer.
        let made = std::cell::Cell::new(None);
        let ask_streams = Streams {
            stdin: ask.stdin,
            stdout: ask.stdout,
            stderr: ask.stderr,
        };
        let request = |exec_streams: Streams, agent_makes: bool| {
            if agent_makes {
                Request::Call(Call::ExecStreams {
                    id: id.clone(),
                    program: program.clone(),
                    streams: exec_streams,
                    ask: ask_streams,
                })
            } else {
                Request::Call(Call::Exec {
                    id: id.clone(),
                    program: program.clone(),
                    streams: exec_streams,
                })
            }
        };
        // Whether the transport makes them is known only once the exec's id is: asked with it.
        let (exec, answer) = if asking {
            let local = T::MAKES_ASKED;
            if local {
                streams.stdin |= ask.stdin;
                streams.stdout |= ask.stdout;
                streams.stderr |= ask.stderr;
            }
            let req = request(streams, !local);
            self.call(
                &req,
                |exec| {
                    if local {
                        made.set(Some(transport.asks(exec, ask)?));
                    }
                    transport.stdio(exec, stdio).map(Some)
                },
                Some(Arc::clone(&ending)),
            )?
        } else {
            let req = request(streams, false);
            self.call(
                &req,
                |exec| transport.stdio(exec, stdio).map(Some),
                Some(Arc::clone(&ending)),
            )?
        };
        let given = match &answer {
            Answer::Given { .. } => self.conn.shared.state()?.given.remove(&exec),
            _ => made.take(),
        };
        match answer {
            Answer::Started { pid } | Answer::Given { pid, .. } => Ok(Box::new(RemoteProc {
                conn: Arc::clone(&self.conn),
                version: self.version,
                bound: self.limits.message_bytes,
                exec,
                pid,
                ending,
                exit: None,
                given,
            })),
            other => {
                if let Ok(mut state) = self.conn.shared.state() {
                    state.programs.remove(&exec);
                    state.given.remove(&exec);
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
    /// The wire version its client writes, which its release and kill go in, and the agent's message bound.
    version: u16,
    bound: usize,
    exec: u64,
    pid: u32,
    ending: Arc<Ending>,
    exit: Option<Exit>,
    /// The caller's ends of the streams its exec asked for, until taken.
    given: Option<hex_api::Given>,
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

    /// A kill request naming its exec, answered once the agent has sent KILL; its end then comes as any
    /// end does. An agent of wire version 2 or earlier has no kill: refused by type before it is sent.
    fn kill(&mut self) -> Result<(), Refusal> {
        if self.wait(None)?.is_some() {
            return Ok(());
        }
        let kill = Request::Call(Call::Kill { exec: self.exec });
        match self
            .conn
            .call(self.version, self.bound, &kill, |_| Ok(None), None)?
            .1
        {
            Answer::Done => Ok(()),
            Answer::Refused(r) => Err(r),
            other => Err(unexpected("kill", &other)),
        }
    }

    fn streams(&mut self) -> Result<hex_api::Given, Refusal> {
        Ok(self.given.take().unwrap_or_default())
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
