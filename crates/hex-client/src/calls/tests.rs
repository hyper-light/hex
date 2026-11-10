//! HEX-RT-17: a caller can reserve a guest exec before opening its named stdio connections, then commit or cancel.

use super::*;

struct Memory {
    handle: std::net::UdpSocket,
    outbound: Mutex<Vec<Vec<u8>>>,
    inbound: Mutex<VecDeque<Vec<u8>>>,
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
        let mut came = false;
        while let Some(bytes) = self.inbound.lock().unwrap().pop_front() {
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
        Ok(true)
    }

    fn shutdown(&self) {}
}

fn client() -> Core<Memory> {
    Core {
        conn: Arc::new(Conn {
            shared: Arc::new(Shared::new(Memory {
                handle: std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap(),
                outbound: Mutex::new(Vec::new()),
                inbound: Mutex::new(VecDeque::new()),
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
    }
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
    let mut deframer = hex_core::stream::Deframer::new(wire::MAX_MESSAGE);
    let mut rest = sent[0].as_slice();
    let mut message = None;
    while !rest.is_empty() {
        let (used, got) = deframer.take(rest).unwrap();
        assert!(used > 0);
        rest = &rest[used..];
        if let Some(got) = got {
            assert!(message.replace(got).is_none());
        }
    }
    let (header, request) = wire::read_request_apart(&message.unwrap(), wire::MAX_MESSAGE).unwrap();
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
