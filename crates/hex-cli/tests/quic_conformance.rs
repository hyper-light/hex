//! HEX-QUIC-20 (CP-T7.3; the joint plan's step 4; docs/design/hex-quic.md): the hex-api conformance suite, every
//! case, through the node's QUIC listener. A node in this process serves its `process` boxes over its local socket
//! and over hyper-transport on loopback, through one server ([`hex_node::quic::Quic`]); a client enrolled with the
//! simulator CA reaches it through [`hex_quic::link`] alone, the node agent's guest client over QUIC connections
//! (control, a program's stdio streams); and both ends of their connection report `TLS13_AES_256_GCM_SHA384`. The
//! programs are `hex selftest-box` acts, as the class's own conformance tests run them.
//!
//! Linux needs a delegated cgroup v2 subtree for the class, as `process_box.rs` does, re-running itself once in a
//! `systemd-run --user --scope -p Delegate=yes` where its own isn't one, and recording the skip where neither is.
//! macOS and Windows run their `process` class rootless in this process.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Instant;

use hex_api::conformance::{self, Act, Fixture};
use hex_api::{Bounds, BoxId, BoxSpec, ClassSpec, Program};
use hex_node::quic::{Bounds as QuicBounds, Quic};
use hex_node::serve::{Host, Limits, Native, Server};
use hex_quic::enroll::sim::SimCa;
use hex_quic::{Config, Endpoint, Shape, enroll, peer_id};

struct BinFixture {
    exe: String,
}

impl Fixture for BinFixture {
    fn spec(&self, id: BoxId, bounds: Bounds) -> BoxSpec {
        BoxSpec {
            route: None,
            allocation: None,
            devices: Vec::new(),
            id,
            bounds,
            class: ClassSpec::Process,
            owner: None,
        }
    }

    fn init(&self, id: BoxId, _act: &Act) -> BoxSpec {
        self.spec(id, Bounds::default())
    }

    fn program(&self, act: &Act) -> Program {
        let mut args = vec![self.exe.clone(), "selftest-box".to_owned()];
        match act {
            Act::Say(words) => args.extend(["say".to_owned(), (*words).to_owned()]),
            Act::Fail(code) => args.extend(["fail".to_owned(), code.to_string()]),
            Act::Wait => args.push("wait".to_owned()),
            Act::Hog(bytes) => args.extend(["hog".to_owned(), bytes.to_string()]),
            Act::Spawn(n) => args.extend(["spawn".to_owned(), n.to_string()]),
            Act::Echo => args.push("echo".to_owned()),
        }
        Program::of(&args.iter().map(String::as_str).collect::<Vec<_>>())
    }

    /// Windows' job object counts no member's own refused fork (windows_process_box.rs, Ada's call 2026-11-03).
    fn refused_forks_counted(&self) -> bool {
        !cfg!(windows)
    }

    fn exec_limit(&self) -> Option<hex_api::ExecLimit> {
        #[cfg(target_os = "linux")]
        return Some(hex_runtime::linux::process::exec_limit());
        #[cfg(target_os = "macos")]
        return Some(hex_runtime::macos::exec_limit().expect("kern.argmax"));
        #[cfg(windows)]
        return Some(hex_core::hcs::exec_limit());
    }
}

fn fixture() -> BinFixture {
    BinFixture {
        exe: env!("CARGO_BIN_EXE_hex").to_owned(),
    }
}

/// A runtime served as a node's host: macOS's and Windows' `process` class, in this process.
#[cfg(not(target_os = "linux"))]
mod hosted {
    use hex_api::{
        BackingGrant, BackingKey, BackingOffer, BoxId, BoxInfo, BoxSpec, Capacity, Class, Proc,
        Program, Refusal, Runtime, Signal, Stdio, Usage,
    };
    use hex_core::netns;
    use hex_node::serve::Host;

    /// A runtime served as a node's host, its abandoned programs counted by the runtime's own inspect.
    pub struct Hosted<R>(pub R);

    impl<R: Runtime> Runtime for Hosted<R> {
        fn backing_usage(&self, key: &BackingKey) -> Result<Usage, Refusal> {
            self.0.backing_usage(key)
        }
        fn initialize_backing(&self, grant: BackingGrant) -> Result<BackingOffer, Refusal> {
            self.0.initialize_backing(grant)
        }
        fn release_backing(&self, key: BackingKey) -> Result<BackingOffer, Refusal> {
            self.0.release_backing(key)
        }
        fn classes(&self) -> Vec<Class> {
            self.0.classes()
        }
        fn physical_report(&self) -> Result<hex_api::devices::PhysicalReport, Refusal> {
            self.0.physical_report()
        }
        fn capacity(&self, class: Class) -> Result<Option<Capacity>, Refusal> {
            self.0.capacity(class)
        }
        fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
            self.0.create(spec)
        }
        fn start(&self, id: &BoxId) -> Result<(), Refusal> {
            self.0.start(id)
        }
        fn exec(
            &self,
            id: &BoxId,
            program: Program,
            stdio: Stdio,
        ) -> Result<Box<dyn Proc>, Refusal> {
            self.0.exec(id, program, stdio)
        }
        fn run(
            &self,
            spec: BoxSpec,
            program: Program,
            stdio: Stdio,
        ) -> Result<Box<dyn Proc>, Refusal> {
            self.0.run(spec, program, stdio)
        }
        fn signal(&self, id: &BoxId, signal: Signal) -> Result<(), Refusal> {
            self.0.signal(id, signal)
        }
        fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
            self.0.inspect(id)
        }
        fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
            self.0.usage(id)
        }
        fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
            self.0.list()
        }
        fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
            self.0.delete(id)
        }
    }

    impl<R: Runtime> Host for Hosted<R> {
        fn abandoned(&self, _id: &BoxId) -> Result<(), Refusal> {
            Ok(())
        }
        fn netns(&self) -> Result<netns::Counts, Refusal> {
            Ok(netns::Counts::default())
        }
    }
}

/// The server's bounds for this test: a node's few connections, and as many calls and programs as the suite runs at
/// once.
fn limits() -> Limits {
    Limits {
        connections: 8,
        message_bytes: hex_core::wire::MAX_MESSAGE,
        in_flight: 16,
        programs: 64,
    }
}

fn cache() -> std::path::PathBuf {
    let cache = std::path::PathBuf::from(std::env::var_os("HOME").expect("HOME"))
        .join(".cache/hex-t7/quic");
    std::fs::create_dir_all(&cache).expect("cache dir");
    cache
}

/// HEX-QUIC-20's body over `host`: the node served over QUIC, the suite through it, and the suite on both ends.
fn exercise(host: Arc<dyn Host>) {
    let ca = SimCa::new().unwrap();
    let enrolled = |name: &str| {
        ca.admit(name, name.as_bytes());
        enroll::enroll(&ca, name, name.as_bytes().to_vec()).unwrap()
    };
    let limits = limits();
    let node = Arc::new(
        Endpoint::start(Config {
            credentials: enrolled("node.quic.test"),
            bind: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            listen: true,
            serves: hex_node::quic::ports(),
            shape: Shape {
                connections: limits.connections,
                lanes: u32::try_from(limits.connections).unwrap(),
            },
        })
        .unwrap(),
    );
    let address = node.local_addr();
    let dir = tempfile::Builder::new()
        .prefix("q-")
        .tempdir_in(cache())
        .unwrap();
    let socket = dir.path().join("node.sock");
    let quic = Quic::new(
        Native::bind(&socket).unwrap(),
        Arc::clone(&node),
        QuicBounds::of(&limits),
        Some(dir.path().join("staged")),
    )
    .unwrap();
    let server = Server::with_os(quic, host, limits, limits.in_flight, &socket).unwrap();
    let client_end = Arc::new(
        Endpoint::start(Config {
            credentials: enrolled("client.quic.test"),
            bind: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            listen: false,
            serves: Vec::new(),
            shape: Shape {
                connections: 1,
                lanes: hex_quic::limits::client_lanes(),
            },
        })
        .unwrap(),
    );
    let deadline = Instant::now() + hex_api::conformance::PATIENCE;
    let (client, peer) =
        hex_quic::link::connect(Arc::clone(&client_end), "node.quic.test", address, deadline)
            .unwrap();
    let aes = hex_quic::CipherSuite::TLS13_AES_256_GCM_SHA384;
    assert_eq!(client_end.negotiated(peer).unwrap().unwrap().0, aes);
    assert_eq!(
        node.negotiated(peer_id("client.quic.test"))
            .unwrap()
            .unwrap()
            .0,
        aes
    );
    let failures = conformance::run(&client, &fixture());
    assert!(failures.is_empty(), "through QUIC: {failures:#?}");
    drop(client);
    drop(server);
}

#[cfg(target_os = "macos")]
#[test]
fn hex_quic_20_the_conformance_suite_passes_over_quic() {
    let model = hex_runtime::macos::machine_model().expect("this Mac's model");
    let rates = hex_core::watch::seed(&model);
    let runtime = hex_runtime::macos::MacRuntime::new(
        rates.bytes,
        rates.processes,
        hex_core::watch::lateness_seed(&model),
    )
    .expect("a macOS runtime");
    exercise(Arc::new(hosted::Hosted(runtime)));
}

#[cfg(windows)]
#[test]
fn hex_quic_20_the_conformance_suite_passes_over_quic() {
    exercise(Arc::new(hosted::Hosted(
        hex_runtime::windows::WinRuntime::new(),
    )));
}

#[cfg(target_os = "linux")]
mod linux {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::Arc;

    const TEST: &str = "linux::hex_quic_20_the_conformance_suite_passes_over_quic";

    /// This process's cgroup directory under /sys/fs/cgroup.
    fn own_cgroup() -> PathBuf {
        let own = std::fs::read_to_string("/proc/self/cgroup").expect("/proc/self/cgroup");
        let rel = own
            .lines()
            .find_map(|l| l.strip_prefix("0::"))
            .expect("a 0:: cgroup line")
            .trim_start_matches('/');
        PathBuf::from("/sys/fs/cgroup").join(rel)
    }

    /// This process in a leaf of its own cgroup, which hands every controller down (process_box.rs's).
    fn prepare_cgroup(disk: &Path) -> Result<PathBuf, String> {
        let own = own_cgroup();
        let leaf = own.join("hex-agent");
        std::fs::create_dir_all(&leaf).map_err(|e| format!("making a leaf for the agent: {e}"))?;
        std::fs::write(leaf.join("cgroup.procs"), format!("{}", std::process::id()))
            .map_err(|e| format!("moving self into a leaf: {e}"))?;
        hex_runtime::linux::cgroup::BoxRoot::open(
            &own.join("hex-probe"),
            hex_runtime::linux::disks::under(disk).unwrap_or_default(),
        )
        .map_err(|e| e.to_string())?;
        let _ = std::fs::remove_dir(own.join("hex-probe"));
        Ok(own)
    }

    #[test]
    fn hex_quic_20_the_conformance_suite_passes_over_quic() {
        let cache = super::cache();
        match prepare_cgroup(&cache) {
            Ok(own) => {
                let state = tempfile::Builder::new()
                    .prefix("node-")
                    .tempdir_in(&cache)
                    .unwrap();
                let node =
                    hex_node::Node::open(&own.join("hex-boxes-quic"), state.path(), state.path())
                        .expect("open a node");
                super::exercise(Arc::new(node));
            }
            Err(direct) if std::env::var_os("HEX_DELEGATED").is_none() => {
                let has_systemd = Command::new("systemd-run")
                    .arg("--version")
                    .output()
                    .is_ok_and(|o| o.status.success());
                if !has_systemd {
                    hex_testkit::skip(
                        TEST,
                        "cgroup v2 delegation",
                        &format!(
                            "own cgroup isn't ours to subdivide ({direct}) and no systemd --user"
                        ),
                    )
                    .expect("recording the skip");
                    return;
                }
                let status = Command::new("systemd-run")
                    .args(["--user", "--scope", "--quiet", "-p", "Delegate=yes"])
                    .arg(std::env::current_exe().expect("test binary path"))
                    .args([TEST, "--exact", "--nocapture", "--test-threads=1"])
                    .env("HEX_DELEGATED", "1")
                    .status()
                    .expect("run the test in a delegated scope");
                assert!(status.success(), "the delegated re-run failed");
            }
            Err(direct) => {
                hex_testkit::skip(
                    TEST,
                    "cgroup v2 delegation",
                    &format!(
                        "none in the delegated scope either ({direct}); run on a cgroup-v2 host"
                    ),
                )
                .expect("recording the skip");
            }
        }
    }
}
