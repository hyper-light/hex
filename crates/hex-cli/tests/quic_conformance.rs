//! HEX-QUIC-20 (CP-T7.3; the joint plan's step 4; docs/design/hex-quic.md): the hex-api conformance suite, every
//! case, through the node's QUIC listener. A node in this process serves its `process` boxes over its local socket
//! and over hyper-transport on loopback, through one server ([`hex_node::quic::Quic`]); a client enrolled with the
//! simulator CA reaches it through [`hex_quic::link`] alone, the node agent's guest client over QUIC connections
//! (control, a program's stdio streams); and both ends of their connection report `TLS13_AES_256_GCM_SHA384`. The
//! programs are `hex selftest-box` acts, as the class's own conformance tests run them.
//!
//! HEX-QUIC-23 and -24 (Q9): the same through `hex node` itself, a process of its own: configured with `--quic` and
//! the enrollment `hex node enroll` began and the simulator CA answered, it serves the suite over QUIC; without
//! `--quic` it binds no UDP socket, and asked for QUIC without an enrollment it can serve with, it refuses, typed,
//! with nothing bound.
//!
//! Linux needs a delegated cgroup v2 subtree for the class, as `process_box.rs` does, re-running a test once in a
//! `systemd-run --user --scope -p Delegate=yes` where its own isn't one, and recording the skip where neither is.
//! macOS and Windows run their `process` class rootless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::io::{BufRead, BufReader};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio as Std};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use hex_api::conformance::{self, Act, Fixture};
use hex_api::{Bounds, BoxId, BoxSpec, ClassSpec, Program};
use hex_node::quic::{Bounds as QuicBounds, Quic};
use hex_node::serve::{Host, Limits, Native, Server};
use hex_quic::enroll::sim::SimCa;
use hex_quic::enroll::{Ca, store};
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
        fn physical_report(&self) -> Result<hex_api::PhysicalReport, Refusal> {
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

/// A folder of the test's own: under ~/.cache on Unix (the hazards' rule: never /tmp), the system's temp folder on
/// Windows, as Hex's Windows tests take.
fn scratch(prefix: &str) -> tempfile::TempDir {
    #[cfg(unix)]
    {
        let cache =
            PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".cache/hex-t7/quic");
        std::fs::create_dir_all(&cache).expect("cache dir");
        tempfile::Builder::new()
            .prefix(prefix)
            .tempdir_in(&cache)
            .unwrap()
    }
    #[cfg(windows)]
    tempfile::Builder::new().prefix(prefix).tempdir().unwrap()
}

/// HEX-QUIC-20's body over `host`: the node served over QUIC, the suite through it, and the suite on both ends.
fn exercise(host: Arc<dyn Host>) {
    let ca = SimCa::new().unwrap();
    let enrolled = |name: &str| {
        ca.admit(name, name.as_bytes());
        enroll::enroll(&ca, name, name.as_bytes().to_vec()).unwrap()
    };
    let limits = limits();
    // A Linux node stages file trees over QUIC; a macOS or Windows one stages none, on any transport.
    let staging = cfg!(target_os = "linux");
    let node = Arc::new(
        Endpoint::start(Config {
            credentials: enrolled("node.quic.test"),
            bind: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            listen: true,
            serves: hex_node::quic::ports(staging),
            shape: hex_node::quic::shape(&limits, staging),
        })
        .unwrap(),
    );
    let address = node.local_addr();
    let dir = scratch("q-");
    let socket = dir.path().join("node.sock");
    let staged = if staging {
        Some(dir.path().join("staged"))
    } else {
        None
    };
    let quic = Quic::new(
        Native::bind(&socket).unwrap(),
        Arc::clone(&node),
        QuicBounds::of(&limits),
        staged,
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

/// The `hex` binary of this build.
fn hex() -> &'static str {
    env!("CARGO_BIN_EXE_hex")
}

/// A `hex node` process over a folder of the test's, killed when dropped (on Windows with its keeper, which outlives
/// a killed node by design), its stderr kept as it comes.
struct NodeProcess {
    child: Child,
    said: Arc<Mutex<String>>,
    #[cfg(windows)]
    keeping: PathBuf,
}

impl NodeProcess {
    /// `hex node` over `dir`'s socket and state with `more` arguments, its boxes under `box_root` where given (Linux);
    /// and the lines it says on stdout until its `ready`.
    fn start(
        dir: &Path,
        box_root: Option<&Path>,
        more: &[&std::ffi::OsStr],
    ) -> (Self, Vec<String>) {
        let mut command = Command::new(hex());
        command
            .arg("node")
            .arg("--socket")
            .arg(dir.join("node.sock"))
            .arg("--state")
            .arg(dir.join("state"));
        if let Some(root) = box_root {
            command.arg("--box-root").arg(root);
        }
        let mut child = command
            .args(more)
            .stdout(Std::piped())
            .stderr(Std::piped())
            .spawn()
            .expect("start hex node");
        let said = Arc::new(Mutex::new(String::new()));
        let stderr = child.stderr.take().unwrap();
        let sink = Arc::clone(&said);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                sink.lock().unwrap().push_str(&line);
                line.clear();
            }
        });
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            if out.read_line(&mut line).unwrap() == 0 {
                let ended = child.wait().unwrap();
                panic!(
                    "hex node ended {ended} having said {lines:?}: {}",
                    said.lock().unwrap()
                );
            }
            let line = line.trim_end().to_owned();
            let ready = line.starts_with("ready ");
            lines.push(line);
            if ready {
                break;
            }
        }
        // Nothing more comes on stdout; what it says on stderr is kept.
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut out, &mut std::io::sink());
        });
        let node = Self {
            child,
            said,
            #[cfg(windows)]
            keeping: dir.join("state").join("keeper"),
        };
        (node, lines)
    }

    fn said(&self) -> String {
        self.said.lock().unwrap().clone()
    }
}

impl Drop for NodeProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // The keeper its `keeper.owner` names is asked to stop, as a node going down cleanly does (windows_node.rs).
        #[cfg(windows)]
        if let Ok(text) = std::fs::read_to_string(self.keeping.join("keeper.owner")) {
            let mut words = text.split_whitespace();
            if let (Some(Ok(pid)), Some(Ok(created))) = (
                words.next().map(str::parse::<u32>),
                words.next().map(str::parse::<u64>),
            ) && let Ok(keeper) =
                hex_runtime::windows::keeper::NodeKeeper::open(pid, created, &self.keeping)
            {
                keeper.stop();
            }
        }
    }
}

/// The UDP sockets process `pid` holds: on Linux its descriptors' sockets that its namespace's `/proc/<pid>/net/udp`
/// and `udp6` list (proc(5)); on macOS what lsof(8) lists; on Windows what `netstat -ano` lists for it.
fn udp_sockets(pid: u32) -> usize {
    #[cfg(target_os = "linux")]
    {
        let inodes: std::collections::BTreeSet<String> =
            std::fs::read_dir(format!("/proc/{pid}/fd"))
                .unwrap()
                .filter_map(|entry| std::fs::read_link(entry.ok()?.path()).ok())
                .filter_map(|link| {
                    link.to_str()?
                        .strip_prefix("socket:[")?
                        .strip_suffix(']')
                        .map(str::to_owned)
                })
                .collect();
        ["udp", "udp6"]
            .iter()
            .map(|table| {
                std::fs::read_to_string(format!("/proc/{pid}/net/{table}"))
                    .unwrap_or_default()
                    .lines()
                    .skip(1)
                    .filter(|row| {
                        row.split_whitespace()
                            .nth(9)
                            .is_some_and(|inode| inodes.contains(inode))
                    })
                    .count()
            })
            .sum()
    }
    #[cfg(target_os = "macos")]
    {
        let out = Command::new("lsof")
            .args(["-nP", "-a", "-p", &pid.to_string(), "-i", "UDP", "-F", "n"])
            .output()
            .expect("lsof");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|line| line.starts_with('n'))
            .count()
    }
    #[cfg(windows)]
    {
        let pid = pid.to_string();
        ["UDP", "UDPv6"]
            .iter()
            .map(|protocol| {
                let out = Command::new("netstat")
                    .args(["-ano", "-p", protocol])
                    .output()
                    .expect("netstat");
                String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .filter(|row| {
                        row.trim_start().starts_with("UDP")
                            && row.split_whitespace().last() == Some(pid.as_str())
                    })
                    .count()
            })
            .sum()
    }
}

/// HEX-QUIC-23's body: `hex node enroll` begins the node's enrollment, the simulator CA answers its request as kiln's
/// CA would, and `hex node --quic` serves the suite over QUIC, at the address it says before its `ready`, holding the
/// UDP socket it binds, both ends of a client's connection at TLS13_AES_256_GCM_SHA384.
fn configured(box_root: Option<&Path>) {
    let dir = scratch("cli-");
    let credentials = dir.path().join("credentials");
    let begun = Command::new(hex())
        .args(["node", "enroll", "--name", "node.cli.test", "--credentials"])
        .arg(&credentials)
        .output()
        .expect("hex node enroll");
    assert!(
        begun.status.success(),
        "{}",
        String::from_utf8_lossy(&begun.stderr)
    );
    assert_eq!(
        String::from_utf8(begun.stdout).unwrap(),
        format!("request {}\n", credentials.join(store::REQUEST).display())
    );
    let ca = SimCa::new().unwrap();
    ca.admit("node.cli.test", b"grant");
    let issued = ca
        .enroll(&store::request(&credentials, b"grant".to_vec()).unwrap())
        .unwrap();
    store::finish(&credentials, &issued).unwrap();
    let (node, said) = NodeProcess::start(
        dir.path(),
        box_root,
        &[
            "--quic".as_ref(),
            "127.0.0.1:0".as_ref(),
            "--credentials".as_ref(),
            credentials.as_os_str(),
        ],
    );
    let [quic, ready] = said.as_slice() else {
        panic!("hex node said {said:?}");
    };
    let address: SocketAddr = quic
        .strip_prefix("quic ")
        .unwrap_or_else(|| panic!("{quic:?}"))
        .parse()
        .unwrap();
    assert_eq!(
        ready,
        &format!("ready {}", dir.path().join("node.sock").display())
    );
    assert!(
        udp_sockets(node.child.id()) >= 1,
        "a configured hex node holds no UDP socket"
    );
    ca.admit("client.cli.test", b"client");
    let client_end = Arc::new(
        Endpoint::start(Config {
            credentials: enroll::enroll(&ca, "client.cli.test", b"client".to_vec()).unwrap(),
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
        hex_quic::link::connect(Arc::clone(&client_end), "node.cli.test", address, deadline)
            .unwrap();
    assert_eq!(
        client_end.negotiated(peer).unwrap().unwrap().0,
        hex_quic::CipherSuite::TLS13_AES_256_GCM_SHA384
    );
    let failures = conformance::run(&client, &fixture());
    assert!(
        failures.is_empty(),
        "through hex node's QUIC listener: {failures:#?}\n{}",
        node.said()
    );
}

/// HEX-QUIC-24's body: without `--quic`, `hex node` says only its `ready` and holds no UDP socket; asked for QUIC
/// without its enrollment, or with one kiln's CA hasn't answered, it refuses, typed, naming what is missing, before it
/// makes its state folder or binds its socket.
fn unconfigured(box_root: Option<&Path>) {
    let dir = scratch("cli-");
    let (node, said) = NodeProcess::start(dir.path(), box_root, &[]);
    assert_eq!(
        said,
        vec![format!("ready {}", dir.path().join("node.sock").display())]
    );
    assert_eq!(
        udp_sockets(node.child.id()),
        0,
        "an unconfigured hex node holds a UDP socket"
    );
    drop(node);
    let half = scratch("cli-");
    let node = |more: &[&std::ffi::OsStr]| {
        Command::new(hex())
            .arg("node")
            .arg("--socket")
            .arg(half.path().join("node.sock"))
            .arg("--state")
            .arg(half.path().join("state"))
            .args(more)
            .output()
            .expect("hex node")
    };
    let refused = node(&["--quic".as_ref(), "127.0.0.1:0".as_ref()]);
    assert_eq!(refused.status.code(), Some(2));
    let why = String::from_utf8_lossy(&refused.stderr);
    assert!(why.contains("--quic needs --credentials"), "{why}");
    let credentials = half.path().join("credentials");
    let begun = Command::new(hex())
        .args([
            "node",
            "enroll",
            "--name",
            "node.unanswered.test",
            "--credentials",
        ])
        .arg(&credentials)
        .output()
        .expect("hex node enroll");
    assert!(begun.status.success());
    let unanswered = node(&[
        "--quic".as_ref(),
        "127.0.0.1:0".as_ref(),
        "--credentials".as_ref(),
        credentials.as_os_str(),
    ]);
    assert_eq!(unanswered.status.code(), Some(1));
    let why = String::from_utf8_lossy(&unanswered.stderr);
    assert!(
        why.contains(store::CHAIN) && why.contains("hex node enroll"),
        "{why}"
    );
    assert!(
        !half.path().join("node.sock").exists(),
        "a socket was bound"
    );
    assert!(!half.path().join("state").exists(), "the node opened");
}

#[cfg(not(target_os = "linux"))]
#[test]
fn hex_quic_23_a_configured_hex_node_serves_the_suite_over_quic() {
    configured(None);
}

#[cfg(not(target_os = "linux"))]
#[test]
fn hex_quic_24_without_its_enrollment_hex_node_binds_no_quic_and_refuses_typed() {
    unconfigured(None);
}

#[cfg(target_os = "linux")]
mod linux {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::Arc;

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

    /// Runs `body` with this process's cgroup, delegated: here where it is, else once more as `test` alone in a
    /// `systemd-run --user --scope -p Delegate=yes`, else the skip recorded.
    fn delegated(test: &str, body: impl FnOnce(&Path)) {
        let disk = super::scratch("disk-");
        match prepare_cgroup(disk.path()) {
            Ok(own) => body(&own),
            Err(direct) if std::env::var_os("HEX_DELEGATED").is_none() => {
                let has_systemd = Command::new("systemd-run")
                    .arg("--version")
                    .output()
                    .is_ok_and(|o| o.status.success());
                if !has_systemd {
                    hex_testkit::skip(
                        test,
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
                    .args([test, "--exact", "--nocapture", "--test-threads=1"])
                    .env("HEX_DELEGATED", "1")
                    .status()
                    .expect("run the test in a delegated scope");
                assert!(status.success(), "the delegated re-run of {test} failed");
            }
            Err(direct) => {
                hex_testkit::skip(
                    test,
                    "cgroup v2 delegation",
                    &format!(
                        "none in the delegated scope either ({direct}); run on a cgroup-v2 host"
                    ),
                )
                .expect("recording the skip");
            }
        }
    }

    #[test]
    fn hex_quic_20_the_conformance_suite_passes_over_quic() {
        delegated(
            "linux::hex_quic_20_the_conformance_suite_passes_over_quic",
            |own| {
                let state = super::scratch("node-");
                let node =
                    hex_node::Node::open(&own.join("hex-boxes-quic"), state.path(), state.path())
                        .expect("open a node");
                super::exercise(Arc::new(node));
            },
        );
    }

    #[test]
    fn hex_quic_23_a_configured_hex_node_serves_the_suite_over_quic() {
        delegated(
            "linux::hex_quic_23_a_configured_hex_node_serves_the_suite_over_quic",
            |own| super::configured(Some(&own.join("hex-boxes-cli"))),
        );
    }

    #[test]
    fn hex_quic_24_without_its_enrollment_hex_node_binds_no_quic_and_refuses_typed() {
        delegated(
            "linux::hex_quic_24_without_its_enrollment_hex_node_binds_no_quic_and_refuses_typed",
            |own| super::unconfigured(Some(&own.join("hex-boxes-cli-none"))),
        );
    }
}
