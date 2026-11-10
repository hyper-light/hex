//! HEX-RT-17: a caller can reserve a guest exec before opening its named stdio connections, then commit or cancel.

use super::*;
use std::io::{Read, Write};

type Opening = Box<dyn FnOnce(&wire::StreamOffer) + Send>;

struct Memory {
    handle: std::net::UdpSocket,
    outbound: Mutex<Vec<Vec<u8>>>,
    inbound: Mutex<VecDeque<Vec<u8>>>,
    offer: Mutex<Option<Result<(), Refusal>>>,
    opened: Mutex<Vec<wire::StreamOffer>>,
    program_stdio: Mutex<Option<Stdio>>,
    opening: Mutex<Option<Opening>>,
}

impl Memory {
    fn queue(&self, id: u64, answer: Answer) {
        self.inbound
            .lock()
            .unwrap()
            .push_back(wire::answer(id, &answer, wire::MAX_MESSAGE).unwrap());
        self.handle.send(&[1]).unwrap();
    }
}

impl Transport for Memory {
    type Reading = ();
    const MAKES_ASKED: bool = true;

    fn answers() -> &'static [Patience; KINDS] {
        static ANSWERS: [Patience; KINDS] =
            [const { Patience::measuring("test.deferred.answer") }; KINDS];
        &ANSWERS
    }

    fn reading(&self) {}

    fn handle(&self) -> Handle<'_> {
        #[cfg(unix)]
        return std::os::fd::AsFd::as_fd(&self.handle);
        #[cfg(windows)]
        std::os::windows::io::AsSocket::as_socket(&self.handle)
    }

    fn frame(message: Vec<u8>) -> Result<Vec<u8>, Refusal> {
        hex_core::stream::frame(&message, wire::MAX_MESSAGE).map_err(Refusal::from)
    }

    fn recv(&self, _: &mut (), take: &mut Take<'_>) -> Result<bool, Refusal> {
        while self.handle.recv(&mut [0u8; 1]).is_ok() {}
        let mut came = false;
        loop {
            let bytes = self.inbound.lock().unwrap().pop_front();
            let Some(bytes) = bytes else { break };
            take(
                &Received {
                    size: bytes.len(),
                    bytes: &bytes,
                    descriptors: 0,
                    descriptors_cut: false,
                },
                Vec::new(),
            )?;
            came = true;
        }
        Ok(came)
    }

    fn try_send(&self, bytes: &[u8], at: &mut usize, lent: &[Lent<'_>]) -> Result<bool, Sent> {
        assert!(lent.is_empty(), "a deferred guest call lends no handles");
        self.outbound.lock().unwrap().push(bytes.to_vec());
        *at = bytes.len();
        if self.offer.lock().unwrap().is_some() {
            let (header, request) = sent_request(bytes);
            match request {
                Request::Call(Call::Exec { streams, .. }) => {
                    let mut offer = offered(header.id);
                    offer.streams = streams;
                    self.queue(header.id, Answer::Offered(offer));
                }
                Request::StreamsReady => {
                    self.queue(header.id, Answer::Started { pid: 7 });
                    self.queue(header.id, Answer::Exited(Exit::Code(0)));
                }
                _ => {}
            }
        }
        Ok(true)
    }

    fn open_offer(&self, offer: &wire::StreamOffer) -> Result<hex_api::Given, Refusal> {
        self.opened.lock().unwrap().push(offer.clone());
        let opening = self.opening.lock().unwrap().take();
        if let Some(opening) = opening {
            opening(offer);
        }
        let mut stdio = Stdio {
            ask: hex_api::Ask {
                stdin: offer.streams.stdin,
                stdout: offer.streams.stdout,
                stderr: offer.streams.stderr,
            },
            ..Stdio::null()
        };
        let given = stdio.make_asked()?;
        *self.program_stdio.lock().unwrap() = Some(stdio);
        match self.offer.lock().unwrap().as_ref().unwrap() {
            Ok(()) => Ok(given),
            Err(why) => Err(why.clone()),
        }
    }

    fn shutdown(&self) {}
}

fn client() -> Core<Memory> {
    let handle = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    handle.connect(handle.local_addr().unwrap()).unwrap();
    handle.set_nonblocking(true).unwrap();
    let client = Core {
        conn: Arc::new(Conn {
            shared: Arc::new(Shared::new(Memory {
                handle,
                outbound: Mutex::new(Vec::new()),
                inbound: Mutex::new(VecDeque::new()),
                offer: Mutex::new(None),
                opened: Mutex::new(Vec::new()),
                program_stdio: Mutex::new(None),
                opening: Mutex::new(None),
            })),
        }),
        classes: Vec::new(),
        limits: Limits {
            connections: 1,
            message_bytes: wire::MAX_MESSAGE,
            in_flight: 2,
            programs: 2,
        },
        budgets: wire::Budgets {
            in_flight: 2,
            programs: 2,
        },
    };
    // This stand-in has already negotiated its hello, including the programs bound on acknowledgements.
    client.conn.shared.owed.lock().unwrap().bound = client.budgets.programs;
    client
}

fn offered(request: u64) -> wire::StreamOffer {
    wire::StreamOffer {
        pid: 11,
        started: 13,
        vsock: std::path::PathBuf::from("mock-vsock"),
        token: 17,
        // A dedicated guest control's exec id need not equal the outer socket request's id.
        exec: request.checked_add(1).unwrap(),
        streams: Streams {
            stdin: true,
            stdout: true,
            stderr: false,
        },
    }
}

fn sent_request(bytes: &[u8]) -> (Header, Request) {
    let mut deframer = hex_core::stream::Deframer::new(wire::MAX_MESSAGE);
    let mut rest = bytes;
    let mut message = None;
    while !rest.is_empty() {
        let (used, got) = deframer.take(rest).unwrap();
        assert!(used > 0);
        rest = &rest[used..];
        if let Some(got) = got {
            assert!(message.replace(got).is_none());
        }
    }
    wire::read_request_apart(&message.unwrap(), wire::MAX_MESSAGE).unwrap()
}

fn exec() -> Call {
    Call::Exec {
        id: BoxId::new("deferred").unwrap(),
        program: Program::of(&["cat"]),
        streams: Streams {
            stdin: true,
            stdout: true,
            stderr: false,
        },
    }
}

/// HEX-RT-17: cancel one unsent exec; expect no outbound request or completions, and only its reservation removed.
#[test]
fn dropping_a_deferred_call_cancels_only_its_reservation_and_sends_nothing() {
    let client = client();
    let (answer, answers) = std::sync::mpsc::channel();
    let (end, ends) = std::sync::mpsc::channel();
    let first = client
        .defer(
            exec(),
            Box::new(move |id, got| answer.send((id, got)).unwrap()),
            Box::new(move |got| end.send(got).unwrap()),
        )
        .unwrap();
    let second = client
        .defer(exec(), Box::new(|_, _| {}), Box::new(|_| {}))
        .unwrap();
    let (first_id, second_id) = (first.id(), second.id());
    let shared = &client.conn.shared;
    {
        let mut state = shared.state().unwrap();
        assert_eq!(state.calls.len(), 2);
        assert_eq!(state.programs.len(), 2);
        // Any ends later prepared under the reservation must be cancelled by the same ownership guard.
        state.given.insert(first_id, hex_api::Given::default());
        state.given.insert(second_id, hex_api::Given::default());
    }
    drop(first);
    {
        let state = shared.state().unwrap();
        assert_eq!(state.calls.keys().copied().collect::<Vec<_>>(), [second_id]);
        assert_eq!(
            state.programs.keys().copied().collect::<Vec<_>>(),
            [second_id]
        );
        assert_eq!(state.given.keys().copied().collect::<Vec<_>>(), [second_id]);
    }
    assert!(shared.transport.outbound.lock().unwrap().is_empty());
    assert_eq!(
        answers.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    );
    assert_eq!(
        ends.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    );
    drop(second);
    let state = shared.state().unwrap();
    assert!(state.calls.is_empty());
    assert!(state.programs.is_empty());
    assert!(state.given.is_empty());
}

/// HEX-RT-17: send an exec after reserving its id; expect its exact explicit streams, then its answer and end
/// through the existing receive/delivery core. A guest transport's asks hook must never be needed.
#[test]
fn sending_a_deferred_call_keeps_its_id_and_uses_the_same_answer_and_end_core() {
    let client = client();
    let shared = &client.conn.shared;
    let (answer, answers) = std::sync::mpsc::channel();
    let (end, ends) = std::sync::mpsc::channel();
    let pending = client
        .defer(
            exec(),
            Box::new(move |id, got| answer.send((id, got)).unwrap()),
            Box::new(move |got| end.send(got).unwrap()),
        )
        .unwrap();
    let id = pending.id();
    assert!(shared.transport.outbound.lock().unwrap().is_empty());
    pending.send().unwrap();
    let sent = shared.transport.outbound.lock().unwrap();
    assert_eq!(sent.len(), 1);
    let (header, request) = sent_request(&sent[0]);
    assert_eq!(header.id, id);
    assert_eq!(request, Request::Call(exec()));
    drop(sent);
    {
        let mut inbound = shared.transport.inbound.lock().unwrap();
        inbound
            .push_back(wire::answer(id, &Answer::Started { pid: 7 }, wire::MAX_MESSAGE).unwrap());
        inbound.push_back(
            wire::answer(id, &Answer::Exited(Exit::Code(0)), wire::MAX_MESSAGE).unwrap(),
        );
    }
    assert!(shared.recv().unwrap());
    assert_eq!(
        answers.try_recv().unwrap(),
        (id, Ok(Answer::Started { pid: 7 }))
    );
    assert_eq!(ends.try_recv().unwrap(), Ok(Exit::Code(0)));
    let state = shared.state().unwrap();
    assert!(state.calls.is_empty());
    assert!(state.programs.is_empty());
    assert!(state.given.is_empty());
}

fn close_before_start(deferred: bool) {
    let client = client();
    let (answer, answers) = std::sync::mpsc::channel();
    let (end, ends) = std::sync::mpsc::channel();
    let then: Then = Box::new(move |id, got| answer.send((id, got)).unwrap());
    let ended: Ended = Box::new(move |got| end.send(got).unwrap());
    let (id, pending) = if deferred {
        let pending = client.defer(exec(), then, ended).unwrap();
        (pending.id(), Some(pending))
    } else {
        (
            client.begin(exec(), Stdio::null(), then, ended).unwrap(),
            None,
        )
    };
    let shared = &client.conn.shared;
    let why = Refusal::Broken("the connection ended before start".to_owned());
    shared.close(why.clone());
    shared.close(Refusal::Broken(
        "a later close must not notify twice".to_owned(),
    ));
    assert_eq!(answers.try_recv().unwrap(), (id, Err(why)));
    assert_eq!(
        answers.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    );
    assert_eq!(
        ends.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected),
        "an unstarted program has no end callback"
    );
    assert_eq!(
        shared.transport.outbound.lock().unwrap().len(),
        usize::from(!deferred)
    );
    drop(pending);
    let state = shared.state().unwrap();
    assert!(state.calls.is_empty());
    assert!(state.programs.is_empty());
}

/// HEX-RT-17: an event-form exec whose start never came gets its call refusal only, exactly once.
#[test]
fn closing_a_pending_exec_refuses_the_call_without_reporting_a_program_end() {
    close_before_start(false);
}

/// HEX-RT-17: an unsent deferred exec gets its call refusal only, never the end of a program it did not start.
#[test]
fn closing_an_unsent_deferred_exec_refuses_the_call_without_reporting_a_program_end() {
    close_before_start(true);
}

/// HEX-RT-17: once Started came through the core, closing reports its program's lost end exactly once.
#[test]
fn closing_after_started_reports_the_programs_lost_end_once() {
    let client = client();
    let (answer, answers) = std::sync::mpsc::channel();
    let (end, ends) = std::sync::mpsc::channel();
    let id = client
        .begin(
            exec(),
            Stdio::null(),
            Box::new(move |id, got| answer.send((id, got)).unwrap()),
            Box::new(move |got| end.send(got).unwrap()),
        )
        .unwrap();
    let shared = &client.conn.shared;
    shared
        .transport
        .inbound
        .lock()
        .unwrap()
        .push_back(wire::answer(id, &Answer::Started { pid: 7 }, wire::MAX_MESSAGE).unwrap());
    assert!(shared.recv().unwrap());
    assert_eq!(
        answers.try_recv().unwrap(),
        (id, Ok(Answer::Started { pid: 7 }))
    );
    assert_eq!(ends.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
    let why = Refusal::Broken("the connection ended after start".to_owned());
    shared.close(why.clone());
    shared.close(why.clone());
    assert_eq!(
        ends.try_recv().unwrap(),
        Err(Refusal::Broken(format!(
            "the connection to the node agent closed before the program's end came: {why}"
        )))
    );
    assert_eq!(
        ends.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    );
    assert_eq!(
        answers.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    );
    let state = shared.state().unwrap();
    assert!(state.calls.is_empty());
    assert!(state.programs.is_empty());
}

/// HEX-RT-17: a late offer for an expired call is cancelled by encoded Release, while another pending call and
/// a started program still receive their own replies. The read queues its debt instead of writing recursively.
#[test]
fn a_late_offer_is_released_without_closing_other_calls_or_programs() {
    let client = client();
    let shared = &client.conn.shared;
    let mut expired = Caller::new(shared, Some(Arc::new(Ending::default()))).unwrap();
    let expired_id = expired.id;
    let bytes = Memory::frame(
        wire::request(expired_id, &Request::Call(exec()), wire::MAX_MESSAGE).unwrap(),
    )
    .unwrap();
    assert!(expired.send(&bytes, NONE, None, None).is_ok());
    assert!(expired.wait(Some(Instant::now())).is_none());
    drop(expired);

    let missing = BoxId::new("still-pending").unwrap();
    let (answer, answers) = std::sync::mpsc::channel();
    let pending = client
        .defer(
            Call::Inspect(missing.clone()),
            Box::new(move |id, got| answer.send((id, got)).unwrap()),
            Box::new(|_| panic!("an inspect has no program end")),
        )
        .unwrap();
    let pending_id = pending.id();
    pending.send().unwrap();
    let (start, starts) = std::sync::mpsc::channel();
    let (end, ends) = std::sync::mpsc::channel();
    let active = client
        .defer(
            exec(),
            Box::new(move |id, got| start.send((id, got)).unwrap()),
            Box::new(move |got| end.send(got).unwrap()),
        )
        .unwrap();
    let active_id = active.id();
    active.send().unwrap();
    shared
        .transport
        .queue(active_id, Answer::Started { pid: 7 });
    assert!(shared.recv().unwrap());
    assert_eq!(
        starts.try_recv().unwrap(),
        (active_id, Ok(Answer::Started { pid: 7 }))
    );
    let before = shared.transport.outbound.lock().unwrap().len();
    shared
        .transport
        .queue(expired_id, Answer::Offered(offered(expired_id)));
    assert!(shared.recv().unwrap());
    assert!(!client.closed());
    assert!(shared.transport.opened.lock().unwrap().is_empty());
    assert_eq!(
        answers.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    );
    assert_eq!(ends.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
    assert!(shared.owing());
    assert_eq!(shared.transport.outbound.lock().unwrap().len(), before);
    shared.send_owed_now();
    let outbound = shared.transport.outbound.lock().unwrap();
    assert_eq!(outbound.len(), before + 1);
    let (header, request) = sent_request(outbound.last().unwrap());
    assert_eq!(header.id, expired_id);
    assert_eq!(request, Request::Release);
    drop(outbound);

    shared.transport.queue(
        pending_id,
        Answer::Refused(Refusal::NoSuchBox(missing.clone())),
    );
    shared
        .transport
        .queue(active_id, Answer::Exited(Exit::Code(3)));
    assert!(shared.recv().unwrap());
    assert_eq!(
        answers.try_recv().unwrap(),
        (pending_id, Ok(Answer::Refused(Refusal::NoSuchBox(missing))))
    );
    assert_eq!(ends.try_recv().unwrap(), Ok(Exit::Code(3)));
    assert!(!client.closed());
    let state = shared.state().unwrap();
    assert!(state.calls.is_empty());
    assert!(state.programs.is_empty());
}

/// HEX-RT-17: Offered keeps the original call pending. Opening its ends then sends same-id Ready, and only
/// that acknowledgement lets the mock return Started/Exited through the existing synchronous core reader.
#[test]
fn an_offer_is_intermediate_until_same_id_ready_and_the_programs_real_answer() {
    let client = client();
    let shared = &client.conn.shared;
    *shared.transport.offer.lock().unwrap() = Some(Ok(()));
    let weak = Arc::downgrade(shared);
    *shared.transport.opening.lock().unwrap() = Some(Box::new(move |offer| {
        let shared = weak.upgrade().unwrap();
        let state = shared
            .state
            .try_lock()
            .expect("open_offer must run outside State's lock");
        assert!(!state.reading, "the dial must not retain the reader's turn");
        let outbound = shared.transport.outbound.lock().unwrap();
        assert_eq!(outbound.len(), 1, "Ready must follow opening");
        let (header, request) = sent_request(&outbound[0]);
        assert_ne!(header.id, offer.exec);
        assert!(
            state.calls.contains_key(&header.id),
            "the offer must not remove the waiting call"
        );
        assert!(state.programs.contains_key(&header.id));
        assert!(!state.calls.get(&header.id).unwrap().reads);
        assert_eq!(request, Request::Call(exec()));
    }));
    let ending = Arc::new(Ending::default());
    let (id, answer) = client
        .conn
        .call(
            wire::MAX_MESSAGE,
            &Request::Call(exec()),
            |_| Ok(NONE),
            Some(Arc::clone(&ending)),
        )
        .unwrap();
    assert_eq!(answer, Answer::Started { pid: 7 });
    assert_eq!(
        shared.transport.opened.lock().unwrap().as_slice(),
        [offered(id)]
    );
    let outbound = shared.transport.outbound.lock().unwrap();
    assert_eq!(outbound.len(), 2);
    let (ready, request) = sent_request(&outbound[1]);
    assert_eq!(ready.id, id);
    assert_eq!(request, Request::StreamsReady);
    drop(outbound);
    assert_eq!(ending.state.lock().unwrap().exit, Some(Ok(Exit::Code(0))));

    let given = client
        .given(id)
        .expect("the opened ends must survive Started");
    let mut program = shared
        .transport
        .program_stdio
        .lock()
        .unwrap()
        .take()
        .unwrap();
    let mut stdin = hex_api::OwnedStream::from(given.stdin.unwrap());
    stdin.write_all(b"caller").unwrap();
    drop(stdin);
    let mut input = String::new();
    program
        .stdin
        .take()
        .unwrap()
        .read_to_string(&mut input)
        .unwrap();
    assert_eq!(input, "caller");
    program
        .stdout
        .take()
        .unwrap()
        .write_all(b"program")
        .unwrap();
    let mut output = String::new();
    hex_api::OwnedStream::from(given.stdout.unwrap())
        .read_to_string(&mut output)
        .unwrap();
    assert_eq!(output, "program");
    client.release(id);
    assert!(shared.state().unwrap().calls.is_empty());
    assert!(shared.state().unwrap().programs.is_empty());
}

/// HEX-RT-17: a mocked partial stream-open failure closes its made ends, sends Release rather than Ready, and
/// forgets the local waiting call, streams and program reservation while leaving the connection usable.
#[test]
fn a_failed_offer_open_releases_the_remote_preparation_and_local_reservation() {
    let client = client();
    let shared = &client.conn.shared;
    let why = Refusal::Broken("the caller could not finish opening its offered streams".to_owned());
    *shared.transport.offer.lock().unwrap() = Some(Err(why.clone()));
    let got = client.conn.call(
        wire::MAX_MESSAGE,
        &Request::Call(exec()),
        |_| Ok(NONE),
        Some(Arc::new(Ending::default())),
    );
    assert_eq!(got, Err(why));
    let opened = shared.transport.opened.lock().unwrap();
    assert_eq!(opened.len(), 1);
    let outbound = shared.transport.outbound.lock().unwrap();
    assert_eq!(outbound.len(), 2);
    let id = sent_request(&outbound[0]).0.id;
    assert_ne!(id, opened[0].exec);
    let (header, request) = sent_request(&outbound[1]);
    assert_eq!(header.id, id);
    assert_eq!(request, Request::Release);
    let mut program = shared
        .transport
        .program_stdio
        .lock()
        .unwrap()
        .take()
        .unwrap();
    assert_eq!(
        program.stdin.take().unwrap().read(&mut [0u8; 1]).unwrap(),
        0
    );
    assert!(!client.closed());
    let state = shared.state().unwrap();
    assert!(state.calls.is_empty());
    assert!(state.programs.is_empty());
    assert!(state.given.is_empty());
}
