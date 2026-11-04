//! The guest client's tests (docs/design/shards-guest.md §3, HEX-GUEST-4 and HEX-GUEST-5; native plan
//! step 5, serving HEX-RT-1..13 through the seam): against a stand-in agent on loopback, the client says
//! hello in the first version and calls in the highest both speak, framed; an exec's stdin and stdout
//! ride connections of their own, named for it, and its end comes back; and an agent that sends bytes
//! that aren't a message, or closes mid-message, refuses every waiting call by type.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use super::*;
use hex_api::{BoxId, Class, Exit, Program, Runtime};
use hex_core::wire::{Answer, Call, Header, Limits, Request};

/// The guest's ports, as two loopback listeners.
struct Loopback {
    control: SocketAddr,
    stdio: SocketAddr,
}

impl Link for Loopback {
    fn dial(&self, port: u32) -> Result<Box<dyn Duplex>, Refusal> {
        let addr = match port {
            stream::CONTROL_PORT => self.control,
            stream::STDIO_PORT => self.stdio,
            other => panic!("dialed port {other:#x}"),
        };
        Ok(Box::new(TcpStream::connect(addr).unwrap()))
    }
}

fn listeners() -> (TcpListener, TcpListener, Arc<dyn Link>) {
    let control = TcpListener::bind("127.0.0.1:0").unwrap();
    let stdio = TcpListener::bind("127.0.0.1:0").unwrap();
    let link = Arc::new(Loopback {
        control: control.local_addr().unwrap(),
        stdio: stdio.local_addr().unwrap(),
    });
    (control, stdio, link)
}

/// The next framed request on `conn`.
fn next_request(conn: &mut TcpStream) -> (Header, Request) {
    conn.set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let mut d = Deframer::new(wire::MAX_MESSAGE);
    let mut byte = [0u8; 1];
    loop {
        conn.read_exact(&mut byte).unwrap();
        if let (_, Some(m)) = d.take(&byte).unwrap() {
            return wire::read_request_apart(&m, wire::MAX_MESSAGE).unwrap();
        }
    }
}

fn send_answer(conn: &mut TcpStream, version: u16, id: u64, answer: &Answer) {
    let bytes = wire::answer(version, id, answer, wire::MAX_MESSAGE).unwrap();
    conn.write_all(&stream::frame(&bytes, wire::MAX_MESSAGE).unwrap())
        .unwrap();
}

/// The relay's token for every stand-in connection.
const TOKEN: u64 = 0x7465_7374;

fn hello(conn: &mut TcpStream) -> Header {
    conn.write_all(&TOKEN.to_le_bytes()).unwrap();
    let (h, request) = next_request(conn);
    assert_eq!(request, Request::Hello);
    send_answer(
        conn,
        wire::FIRST,
        h.id,
        &Answer::Hello(wire::Hello {
            version: wire::VERSION,
            classes: vec![Class::Oci],
            limits: Limits {
                connections: 1,
                message_bytes: wire::MAX_MESSAGE,
                in_flight: 4,
                programs: 4,
            },
        }),
    );
    h
}

/// HEX-GUEST-4: connect, then inspect a box the agent doesn't hold: expect the hello in version 1 and
/// the inspect in the highest version both speak, each framed, the classes the agent named, and its
/// refusal returned typed.
#[test]
fn a_call_goes_framed_and_its_refusal_comes_back_typed() {
    let (control, _stdio, link) = listeners();
    let agent = std::thread::spawn(move || {
        let (mut conn, _) = control.accept().unwrap();
        let first = hello(&mut conn);
        let (h, request) = next_request(&mut conn);
        let Request::Call(Call::Inspect(id)) = request else {
            panic!("expected an inspect, got {request:?}");
        };
        send_answer(
            &mut conn,
            h.version,
            h.id,
            &Answer::Refused(Refusal::NoSuchBox(id)),
        );
        [(first.version, first.kind), (h.version, h.kind)]
    });
    let client = Client::connect(link).unwrap();
    assert_eq!(client.classes(), vec![Class::Oci]);
    let b = BoxId::new("b").unwrap();
    assert_eq!(client.inspect(&b), Err(Refusal::NoSuchBox(b)));
    assert_eq!(
        agent.join().unwrap(),
        [(wire::FIRST, 0x0001), (wire::VERSION, 0x0006)]
    );
}

/// HEX-GUEST-4: exec a program with stdin and stdout whose agent echoes one into the other: expect two
/// stdio connections named for the exec's id, the bytes written to stdin read back from stdout to its
/// end, and the program's end as the agent sent it.
#[test]
fn an_execs_stdio_rides_named_connections_and_its_end_comes_back() {
    let (control, stdio, link) = listeners();
    let agent = std::thread::spawn(move || {
        let (mut conn, _) = control.accept().unwrap();
        hello(&mut conn);
        let (h, request) = next_request(&mut conn);
        let Request::Call(Call::Exec { streams, .. }) = request else {
            panic!("expected an exec, got {request:?}");
        };
        assert!(streams.stdin && streams.stdout && !streams.stderr);
        let mut named = std::collections::BTreeMap::new();
        for _ in 0..2 {
            let (mut s, _) = stdio.accept().unwrap();
            let mut name = [0u8; stream::NAME_BYTES];
            s.read_exact(&mut name).unwrap();
            let n = stream::read_name(&name).unwrap();
            assert_eq!(
                n.exec,
                Exec {
                    conn: TOKEN,
                    id: h.id
                },
                "a stream names its connection's token and its exec's request id"
            );
            named.insert(n.stream, s);
        }
        send_answer(&mut conn, h.version, h.id, &Answer::Started { pid: 42 });
        let mut stdin = named.remove(&stream::Stdio::In).unwrap();
        let mut stdout = named.remove(&stream::Stdio::Out).unwrap();
        std::io::copy(&mut stdin, &mut stdout).unwrap();
        drop(stdout);
        send_answer(&mut conn, h.version, h.id, &Answer::Exited(Exit::Code(0)));
        // Held open until the client has its end, as an agent's connection is.
        let _ = next_request(&mut conn);
    });
    let client = Client::connect(link).unwrap();
    let (in_r, mut in_w) = std::io::pipe().unwrap();
    let (mut out_r, out_w) = std::io::pipe().unwrap();
    let mut proc = client
        .exec(
            &BoxId::new("b").unwrap(),
            Program::of(&["cat"]),
            Stdio {
                stdin: Some(in_r),
                stdout: Some(out_w),
                stderr: None,
            },
        )
        .unwrap();
    assert_eq!(proc.pid(), 42);
    in_w.write_all(b"hello through the guest\n").unwrap();
    drop(in_w);
    let mut out = String::new();
    out_r.read_to_string(&mut out).unwrap();
    assert_eq!(out, "hello through the guest\n");
    assert_eq!(
        proc.wait_for(Duration::from_secs(30)).unwrap(),
        Some(Exit::Code(0))
    );
    drop(proc);
    let _ = client.inspect(&BoxId::new("b").unwrap());
    agent.join().unwrap();
}

/// HEX-GUEST-5: have the agent announce a frame past the wire's bound while a call waits: expect the
/// call refused by type, naming bytes that aren't a message, before any body is read.
#[test]
fn an_agent_announcing_an_oversized_frame_is_refused() {
    let (control, _stdio, link) = listeners();
    let agent = std::thread::spawn(move || {
        let (mut conn, _) = control.accept().unwrap();
        hello(&mut conn);
        let _ = next_request(&mut conn);
        let past = u32::try_from(wire::MAX_MESSAGE + 1).unwrap();
        conn.write_all(&past.to_be_bytes()).unwrap();
        conn
    });
    let client = Client::connect(link).unwrap();
    let got = client.inspect(&BoxId::new("b").unwrap());
    let Err(Refusal::Broken(why)) = got else {
        panic!("expected a broken connection, got {got:?}");
    };
    assert!(why.contains("aren't a message"), "{why}");
    drop(agent.join().unwrap());
}

/// HEX-GUEST-5: have the agent close the connection partway through an answer: expect the waiting call
/// refused by type, naming the cut message.
#[test]
fn an_agent_closing_mid_message_is_refused() {
    let (control, _stdio, link) = listeners();
    let agent = std::thread::spawn(move || {
        let (mut conn, _) = control.accept().unwrap();
        hello(&mut conn);
        let (h, _) = next_request(&mut conn);
        let bytes = wire::answer(h.version, h.id, &Answer::Done, wire::MAX_MESSAGE).unwrap();
        let framed = stream::frame(&bytes, wire::MAX_MESSAGE).unwrap();
        conn.write_all(&framed[..framed.len() - 1]).unwrap();
    });
    let client = Client::connect(link).unwrap();
    let got = client.start(&BoxId::new("b").unwrap());
    agent.join().unwrap();
    let Err(Refusal::Broken(why)) = got else {
        panic!("expected a broken connection, got {got:?}");
    };
    assert!(why.contains("middle of a message"), "{why}");
}
