//! The node's QUIC listener in this process (CP-T7.3; docs/design/hex-quic.md Q4).

use super::*;

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Instant;

use hex_core::patience::Patience;
use hex_quic::enroll::{self, sim::SimCa};
use hex_quic::{Config, Shape};

use crate::serve::Native;

/// The log's reader registered by the node: no recorded operation makes it, so measuring, for the fact alone.
static READER: Patience = Patience::measuring("test.hex_node.quic.log_reader");
/// A line the stopped reader missed: made by the reader's lane filling, which no record times.
static MISSED: Patience = Patience::measuring("test.hex_node.quic.log_missed");

fn limits() -> Limits {
    Limits {
        connections: 4,
        message_bytes: wire::MAX_MESSAGE,
        in_flight: 4,
        programs: 4,
    }
}

fn enrolled(ca: &SimCa, name: &str) -> hex_quic::tls::Credentials {
    ca.admit(name, name.as_bytes());
    enroll::enroll(ca, name, name.as_bytes().to_vec()).unwrap()
}

fn read_line(conn: &hex_quic::Conn, pending: &mut Vec<u8>) -> String {
    let deadline = Instant::now() + hex_api::conformance::PATIENCE;
    let mut buf = vec![0u8; hex_core::lanes::DATA_BOUND];
    loop {
        if let Some(at) = pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = pending.drain(..=at).collect();
            return String::from_utf8(line).unwrap();
        }
        let n = conn.read(&mut buf, Some(deadline)).unwrap();
        assert_ne!(n, 0, "the log ended");
        pending.extend_from_slice(&buf[..n]);
    }
}

/// HEX-QUIC-16 (Q4): a connection to the node's log port is sent each line the node says, in order; one whose reader
/// stops misses the lines its lane can't take, counted, never holding the node, and once it reads again it is told
/// how many it missed before the next line.
#[test]
fn hex_quic_16_the_log_lane_sends_lines_and_tells_a_slow_reader_what_it_missed() {
    let ca = SimCa::new().unwrap();
    let limits = limits();
    let node = Arc::new(
        Endpoint::start(Config {
            credentials: enrolled(&ca, "node.log.test"),
            bind: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            listen: true,
            serves: ports(),
            shape: Shape {
                connections: limits.connections,
                lanes: 8,
            },
        })
        .unwrap(),
    );
    let cache =
        std::path::PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cache/hex-t7/quic");
    std::fs::create_dir_all(&cache).unwrap();
    let dir = tempfile::Builder::new()
        .prefix("l-")
        .tempdir_in(&cache)
        .unwrap();
    let quic = Quic::new(
        Native::bind(&dir.path().join("node.sock")).unwrap(),
        Arc::clone(&node),
        Bounds::of(&limits),
        None,
    )
    .unwrap();
    let log = quic.log();
    let client = Endpoint::start(Config {
        credentials: enrolled(&ca, "client.log.test"),
        bind: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        listen: false,
        serves: Vec::new(),
        shape: Shape {
            connections: 1,
            lanes: 8,
        },
    })
    .unwrap();
    let deadline = Instant::now() + hex_api::conformance::PATIENCE;
    let peer = client
        .dial("node.log.test", node.local_addr(), deadline)
        .unwrap();
    let reader = client.open(peer, hex_core::lanes::LOG_PORT).unwrap();
    hex_testkit::waits::until(&READER, "the log's reader", || log.readers() == 1).unwrap();
    let mut pending = Vec::new();
    log.say("hex.node event=first\n");
    assert_eq!(read_line(&reader, &mut pending), "hex.node event=first\n");
    // The reader stops: lines of a frame each fill its lane and QUIC's window behind it, and the node goes on.
    let big = format!("{}\n", "x".repeat(hex_core::lanes::DATA_BOUND - 1));
    let mut said = 0u64;
    hex_testkit::waits::until(&MISSED, "a missed line", || {
        log.say(&big);
        said += 1;
        log.missed() > 0
    })
    .unwrap();
    let missed = log.missed();
    // Reading again: every line that went, then the count of those missed, then the next line.
    let mut lines = 0u64;
    loop {
        let line = read_line(&reader, &mut pending);
        if line == big {
            lines += 1;
            if lines + missed == said {
                break;
            }
            continue;
        }
        panic!("a line out of order: {} bytes", line.len());
    }
    hex_testkit::waits::until(&READER, "the reader's room", || {
        log.say("hex.node event=after\n");
        log.missed() == 0
    })
    .unwrap();
    // The count comes before the next line that went (more than once where a later line missed too), then that line.
    let mut counted = 0u64;
    loop {
        let line = read_line(&reader, &mut pending);
        if line == "hex.node event=after\n" {
            break;
        }
        let n = line
            .strip_prefix("hex.log event=missed lines=")
            .and_then(|n| n.trim_end().parse::<u64>().ok());
        let Some(n) = n else {
            panic!("a line out of order: {line:?}");
        };
        counted += n;
    }
    assert!(counted >= missed, "told {counted} of {missed} missed");
}
