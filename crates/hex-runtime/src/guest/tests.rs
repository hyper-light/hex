//! The guest runtime's tests (docs/design/shards-guest.md §1, §4a, §5; native plan step 5): the shared
//! roots read from `HEX_GUEST_SHARES` or this OS's default, never `/`; a box's spec rewritten to the
//! guest's paths, its root staged and each bind source under its share, one under none refused by
//! name (HEX-GUEST-16); and where Shards has no backend, the class refused by name before anything
//! boots, and one guest booted for every create after (HEX-GUEST-17).

use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::rewrite::rewrite;
use super::shares::{self, Share};
use super::*;
use hex_api::{Bounds, Mount, Network, OciSpec, Rootfs};
use hex_client::guest::stage::Source;

/// The `i`th share, `dir`, where the guest's init mounts it.
fn share(i: usize, dir: &str, read_only: bool) -> Share {
    Share {
        dir: PathBuf::from(dir),
        read_only,
        at: format!("/run/hex/shares/{i}"),
    }
}

const STAGED: &str = "/run/hex/staged";

/// HEX-GUEST-16: read share lists as an operator writes them: expect each root, `:ro` or a following
/// `ro` making it read-only; a relative root, an empty list, `ro` first and a list past the bound
/// refused by name.
#[cfg(unix)]
#[test]
fn shared_roots_read_as_the_operator_writes_them() {
    let read = |s: &str| shares::parse(&OsString::from(s));
    assert_eq!(
        read("/Users:/data:ro:/srv").unwrap(),
        vec![
            share(0, "/Users", false),
            share(1, "/data", true),
            share(2, "/srv", false)
        ]
    );
    for bad in ["", "relative", "ro:/a"] {
        assert!(
            matches!(
                read(bad),
                Err(Refusal::Invalid {
                    what: shares::ENV,
                    ..
                })
            ),
            "{bad:?}"
        );
    }
    let many: Vec<String> = (0..=shares::MAX_SHARES).map(|i| format!("/r{i}")).collect();
    assert!(read(&many.join(":")).is_err());
}

/// HEX-GUEST-16: read this OS's default roots: expect Docker Desktop's list on macOS, the users and
/// temp folders on Windows, nothing on Linux, every one read-write and none the host's root.
#[test]
fn the_default_roots_are_docker_desktops_never_the_hosts_root() {
    let defaults = shares::defaults().unwrap();
    let dirs: Vec<&Path> = defaults.iter().map(|s| s.dir.as_path()).collect();
    if cfg!(target_os = "macos") {
        let want = ["/Users", "/Volumes", "/private", "/tmp", "/var/folders"];
        assert_eq!(dirs, want.map(Path::new));
    } else if cfg!(target_os = "windows") {
        // The users folder holds this user's profile; the temp folder is the one TMP or TEMP names.
        let profile = PathBuf::from(std::env::var_os("USERPROFILE").unwrap());
        let temp = PathBuf::from(
            std::env::var_os("TMP")
                .or_else(|| std::env::var_os("TEMP"))
                .unwrap(),
        );
        assert_eq!(dirs, [profile.parent().unwrap(), temp.as_path()]);
    } else {
        assert!(dirs.is_empty());
    }
    assert!(defaults.iter().all(|s| !s.read_only));
    assert!(
        dirs.iter().all(|d| d.parent().is_some()),
        "a default shares a root"
    );
}

/// HEX-GUEST-16: map sources under nested shares: expect each under the deepest share that holds it,
/// and a source escaping by `..`, or under no share, under none.
#[test]
fn a_source_maps_under_the_deepest_share_that_holds_it() {
    let shared = [share(0, "/Users", false), share(1, "/Users/ada/data", true)];
    assert_eq!(
        shares::in_guest(&shared, Path::new("/Users/bo/x")).as_deref(),
        Some("/run/hex/shares/0/bo/x")
    );
    assert_eq!(
        shares::in_guest(&shared, Path::new("/Users/ada/data/set")).as_deref(),
        Some("/run/hex/shares/1/set")
    );
    assert_eq!(
        shares::in_guest(&shared, Path::new("/Users")).as_deref(),
        Some("/run/hex/shares/0")
    );
    assert_eq!(shares::in_guest(&shared, Path::new("/Users/../etc")), None);
    assert_eq!(shares::in_guest(&shared, Path::new("/etc")), None);
}

fn oci(rootfs: Rootfs, mounts: Vec<Mount>) -> BoxSpec {
    BoxSpec {
        id: BoxId::new("box-1").unwrap(),
        bounds: Bounds::default(),
        class: ClassSpec::Oci(Box::new(OciSpec {
            rootfs,
            init: Program::of(&["sleep", "1"]),
            mounts,
            hostname: None,
            network: Network::Private,
            hosts: Vec::new(),
        })),
    }
}

fn mount(source: &str, destination: &str, read_only: bool) -> Mount {
    Mount {
        source: PathBuf::from(source),
        destination: destination.to_owned(),
        read_only,
    }
}

/// HEX-GUEST-16: rewrite an OciSpec with a root folder and with layers, and a writable and a read-only
/// bind: expect its root named by the staged copy and staged from the host path (an image's layer blobs
/// named by the digests their names give, never read, a layer tarball named otherwise refused by name, and
/// tarballs mixed with directories refused), each bind's source
/// its path under its share (writable binds pass), and a bind under no share refused naming it and the
/// shared roots.
#[test]
fn an_oci_spec_is_rewritten_to_the_guests_paths() {
    let shared = [share(0, "/Users", false), share(1, "/etc", false)];
    let got = rewrite(
        oci(
            Rootfs::Dir(PathBuf::from("/Users/ada/rootfs.tar")),
            vec![
                mount("/etc", "/host-etc", false),
                mount("/Users/ada/src", "/src", true),
            ],
        ),
        &shared,
        STAGED,
    )
    .unwrap();
    let ClassSpec::Oci(spec) = &got.spec.class else {
        panic!("not oci");
    };
    assert_eq!(
        spec.rootfs,
        Rootfs::Dir(PathBuf::from("/run/hex/staged/box-1/root"))
    );
    assert_eq!(spec.mounts[0].source, PathBuf::from("/run/hex/shares/1"));
    assert!(!spec.mounts[0].read_only);
    assert_eq!(
        spec.mounts[1].source,
        PathBuf::from("/run/hex/shares/0/ada/src")
    );
    assert_eq!(got.items.len(), 1);
    assert_eq!(got.items[0].name, "root");
    assert_eq!(
        got.items[0].source,
        Source::Tar(PathBuf::from("/Users/ada/rootfs.tar"))
    );

    // Two layers as an image keeps them, each blob named by its digest (an OCI image layout's
    // blobs/sha256/<hex>, as containerd's content store's). Neither file exists: a layer's digest is its
    // name, never read to learn it (§4a; the lead's rule, 2026-11-06: a create never hashes a layer).
    let digests: Vec<[u8; 32]> = [[0xa0u8; 32], [0x1bu8; 32]].to_vec();
    let blobs: Vec<PathBuf> = digests
        .iter()
        .map(|d| {
            PathBuf::from(format!(
                "/Users/ada/image/blobs/sha256/{}",
                hex_core::stream::files::digest_hex(d)
            ))
        })
        .collect();
    let layered = rewrite(oci(Rootfs::Layers(blobs.clone()), vec![]), &shared, STAGED).unwrap();
    let ClassSpec::Oci(spec) = &layered.spec.class else {
        panic!("not oci");
    };
    // Each is a lower layer of the box's overlay, its folder in the guest's layer store, lowest first.
    assert_eq!(
        spec.rootfs,
        Rootfs::Layers(
            digests
                .iter()
                .map(|d| PathBuf::from(hex_client::guest::stage::layer_path(STAGED, d)))
                .collect()
        )
    );
    assert_eq!(
        layered.items[0].source,
        Source::Layers(
            blobs
                .iter()
                .zip(&digests)
                .map(|(path, digest)| hex_client::guest::stage::Layer {
                    path: path.clone(),
                    digest: *digest,
                })
                .collect()
        )
    );
    // A layer tarball not named by its digest is refused by name, never hashed to name it.
    let Err(Refusal::Invalid { why, .. }) = rewrite(
        oci(
            Rootfs::Layers(vec![
                blobs[0].clone(),
                PathBuf::from("/Users/ada/l1.tar.gz"),
            ]),
            vec![],
        ),
        &shared,
        STAGED,
    ) else {
        panic!("expected a layer tarball not named by its digest refused");
    };
    assert!(
        why.contains("rootfs.layers[1] /Users/ada/l1.tar.gz") && why.contains("blobs/sha256/"),
        "{why}"
    );
    let Err(Refusal::Invalid { why, .. }) = rewrite(
        oci(
            Rootfs::Layers(vec![
                PathBuf::from("/Users/l0.tar"),
                PathBuf::from("/Users/l1"),
            ]),
            vec![],
        ),
        &shared,
        STAGED,
    ) else {
        panic!("expected tarballs and directories together refused");
    };
    assert!(why.contains("mixes tarballs and directories"), "{why}");

    let Err(Refusal::Platform { why, .. }) = rewrite(
        oci(
            Rootfs::Dir(PathBuf::from("/Users/r.tar")),
            vec![mount("/opt/x", "/x", true)],
        ),
        &shared,
        STAGED,
    ) else {
        panic!("expected the unshared bind refused");
    };
    assert!(
        why.contains("/x's source /opt/x") && why.contains("[/Users, /etc]"),
        "{why}"
    );
}

/// HEX-GUEST-16: rewrite a bundle whose config names a relative root, a relative and an absolute
/// writable bind (both kinds of bind option) and a proc mount: expect the bundle the staged folder,
/// its root staged from the bundle's, its config's root `root`, each bind's source its share path and
/// the proc mount untouched; and a bind under no share refused.
#[cfg(unix)]
#[test]
fn a_bundles_config_is_rewritten_to_the_guests_paths() {
    let dir = tempfile::tempdir().unwrap();
    let bundle = dir.path().join("bundle");
    std::fs::create_dir_all(bundle.join("rootfs")).unwrap();
    let base = dir.path().to_str().unwrap().to_owned();
    let config = serde_json::json!({
        "ociVersion": "1.2.0",
        "root": {"path": "rootfs"},
        "mounts": [
            {"destination": "/proc", "type": "proc", "source": "proc"},
            {"destination": "/etc-host", "type": "bind", "source": "/etc", "options": ["rbind", "rw"]},
            {"destination": "/data", "source": "data", "options": ["bind"]},
        ],
    });
    std::fs::write(bundle.join("config.json"), config.to_string()).unwrap();
    let shared = [share(0, "/etc", false), share(1, &base, false)];
    let spec = BoxSpec {
        id: BoxId::new("box-1").unwrap(),
        bounds: Bounds::default(),
        class: ClassSpec::Bundle(bundle.clone()),
    };
    let got = rewrite(spec.clone(), &shared, STAGED).unwrap();
    assert_eq!(
        got.spec.class,
        ClassSpec::Bundle(PathBuf::from("/run/hex/staged/box-1"))
    );
    assert_eq!(got.items[0].source, Source::Path(bundle.join("rootfs")));
    let Source::Bytes(bytes) = &got.items[1].source else {
        panic!("config.json isn't bytes");
    };
    assert_eq!(got.items[1].name, "config.json");
    let rewritten: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(rewritten["root"]["path"], "root");
    assert_eq!(rewritten["mounts"][0]["source"], "proc");
    assert_eq!(rewritten["mounts"][1]["source"], "/run/hex/shares/0");
    assert_eq!(
        rewritten["mounts"][2]["source"],
        "/run/hex/shares/1/bundle/data"
    );
    assert!(matches!(
        rewrite(spec, &[share(0, "/etc", false)], STAGED),
        Err(Refusal::Platform { .. })
    ));
}

/// A stand-in for Shards: a backend or the refusal of one, counting boots, each refused as a boot
/// failure (no guest is needed to show none was asked for).
struct NoGuest {
    backend: Result<shards_api::Backend, shards_api::Refusal>,
    boots: AtomicUsize,
}

impl shards_api::Guests for NoGuest {
    fn backend(&self) -> Result<shards_api::Backend, shards_api::Refusal> {
        self.backend.clone()
    }

    fn boot(
        &self,
        _spec: &shards_api::GuestSpec,
    ) -> Result<Box<dyn shards_api::Guest>, shards_api::Refusal> {
        self.boots.fetch_add(1, Ordering::SeqCst);
        Err(shards_api::Refusal::Boot(
            "a stand-in boots nothing".to_owned(),
        ))
    }
}

fn config() -> GuestConfig {
    GuestConfig {
        binary: PathBuf::from("/shards"),
        kernel: PathBuf::from("/k"),
        initrd: PathBuf::from("/i"),
        // Shards' least guest: one vCPU and its architecture's least memory (`shards_api::check`).
        vcpus: 1,
        memory_mib: shards_api::min_memory_mib(std::env::consts::ARCH),
        shares: vec![share(0, "/", false)],
        staged: STAGED.to_owned(),
    }
}

/// HEX-GUEST-17: create an oci box, its root an absolute path on this host, where Shards has no backend
/// (macOS x86_64's refusal): expect the class refused as the platform's, naming the OS, arch and Shards, and no boot asked for; a process
/// box refused as the host's; and a call about a box before any boot NoSuchBox, with nothing booted.
#[test]
fn where_shards_has_no_backend_the_class_is_refused_before_anything_boots() {
    let guests = Arc::new(NoGuest {
        backend: Err(shards_api::Refusal::NoBackend {
            os: "macos",
            arch: "x86_64",
        }),
        boots: AtomicUsize::new(0),
    });
    let runtime = GuestRuntime::new(guests.clone(), config());
    // An absolute path on this host (a drive's on Windows): the spec's own check passes it, so the
    // refusal that comes is the platform's.
    let host = tempfile::tempdir().unwrap();
    let got = runtime.create(oci(Rootfs::Dir(host.path().join("rootfs.tar")), vec![]));
    let Err(Refusal::Platform { platform, why, .. }) = got else {
        panic!("expected the platform's refusal, got {got:?}");
    };
    assert_eq!(platform, "macos");
    assert!(why.contains("x86_64") && why.contains("Shards"), "{why}");
    assert_eq!(guests.boots.load(Ordering::SeqCst), 0);
    let process = BoxSpec {
        id: BoxId::new("p").unwrap(),
        bounds: Bounds::default(),
        class: ClassSpec::Process,
    };
    assert!(matches!(
        runtime.create(process),
        Err(Refusal::Invalid { .. })
    ));
    let id = BoxId::new("box-1").unwrap();
    assert_eq!(runtime.inspect(&id), Err(Refusal::NoSuchBox(id.clone())));
    assert_eq!(runtime.list(), Ok(Vec::new()));
    assert_eq!(guests.boots.load(Ordering::SeqCst), 0);
}

/// HEX-GUEST-17: read the guest's spec from its config: expect the initramfs as its init, the init's
/// command line naming the shares, the channel's three ports alone, and each share as configured.
#[test]
fn the_guests_spec_names_its_init_ports_and_shares() {
    let spec = config().spec();
    assert_eq!(spec.cmdline, "console=ttyS0 panic=-1 hex.shares=rw");
    let none = GuestConfig {
        shares: Vec::new(),
        ..config()
    };
    assert_eq!(none.spec().cmdline, "console=ttyS0 panic=-1");
    assert_eq!(
        spec.ports,
        [stream::CONTROL_PORT, stream::STDIO_PORT, files::FILES_PORT]
    );
    assert_eq!(spec.shares.len(), 1);
    assert!(spec.disks.is_empty());
}

/// HEX-GUEST-17: read the guest from variables with Shards' binary, the kernel and the initramfs
/// missing, then with every one missing: expect one refusal naming each missing one, read before
/// anything boots; then with all given: expect the guest they name, booted with that binary.
#[test]
fn every_missing_guest_input_is_named_in_one_refusal() {
    use std::collections::BTreeMap;
    let read = |given: &[(&str, &str)]| {
        let vars: BTreeMap<String, OsString> = given
            .iter()
            .map(|(k, v)| ((*k).to_owned(), OsString::from(v)))
            .collect();
        GuestConfig::from_vars(move |name| vars.get(name).cloned())
    };
    // Inputs to the parse, not a guest's size: any numbers do.
    let sized = [("HEX_GUEST_CPUS", "2"), ("HEX_GUEST_MEMORY_MIB", "1024")];
    let Err(Refusal::Invalid { why, .. }) = read(&sized) else {
        panic!("expected a refusal");
    };
    assert!(
        why.starts_with("SHARDS_BIN, HEX_GUEST_KERNEL, HEX_GUEST_INITRD unset"),
        "{why}"
    );
    let Err(Refusal::Invalid { why, .. }) = read(&[("SHARDS_BIN", "")]) else {
        panic!("expected a refusal");
    };
    assert!(
        why.starts_with(
            "SHARDS_BIN, HEX_GUEST_KERNEL, HEX_GUEST_INITRD, HEX_GUEST_CPUS, HEX_GUEST_MEMORY_MIB unset"
        ),
        "{why}"
    );
    let mut all = sized.to_vec();
    all.extend([
        ("SHARDS_BIN", "/opt/shards"),
        ("HEX_GUEST_KERNEL", "/k"),
        ("HEX_GUEST_INITRD", "/i"),
        (
            "HEX_GUEST_SHARES",
            if cfg!(windows) { r"C:\data" } else { "/data" },
        ),
    ]);
    let config = read(&all).unwrap();
    assert_eq!((config.vcpus, config.memory_mib), (2, 1024));
    assert_eq!(config.spec().binary, Some(PathBuf::from("/opt/shards")));
    assert_eq!(config.shares.len(), 1);
}

/// One value of a spec broken, as HEX-RT-13 breaks them.
type Break = fn(&mut OciSpec);

/// HEX-RT-13 through the guest (found by HEX-GUEST-19 on studio-2): create oci boxes whose specs break
/// one value each (HEX-RT-13's), their bind source under no share: expect each refused as the spec's
/// own Invalid naming the field, as on any node, before its sources are mapped to shares and before
/// anything boots.
#[test]
fn an_unrenderable_spec_is_refused_by_its_field_before_its_shares_or_a_boot() {
    let guests = Arc::new(NoGuest {
        backend: Ok(shards_api::Backend::Kvm),
        boots: AtomicUsize::new(0),
    });
    let runtime = GuestRuntime::new(guests.clone(), config());
    let unshared = GuestConfig {
        shares: vec![share(0, "/Users", false)],
        ..config()
    };
    let unshared = GuestRuntime::new(guests.clone(), unshared);
    let broken: [(&str, Break); 3] = [
        ("mounts[0].destination", |s| {
            s.mounts[0].destination = "data".to_owned();
        }),
        ("hostname", |s| s.hostname = Some("two words".to_owned())),
        ("init.cwd", |s| s.init.cwd = Some("relative".to_owned())),
    ];
    for (field, break_it) in broken {
        let mut spec = oci(
            Rootfs::Dir(PathBuf::from("/hex-conformance/rootfs")),
            vec![mount("/hex-conformance/data", "/data", true)],
        );
        let ClassSpec::Oci(o) = &mut spec.class else {
            panic!("not oci");
        };
        break_it(o);
        for rt in [&runtime, &unshared] {
            let got = rt.create(spec.clone());
            assert!(
                matches!(&got, Err(Refusal::Invalid { what: "oci spec", why }) if why.contains(field)),
                "{field}: {got:?}"
            );
        }
    }
    assert_eq!(guests.boots.load(Ordering::SeqCst), 0);
}

/// HEX-GUEST-16 (§4a): rewrite an OciSpec whose root is a tarball, and one whose root is a directory: expect
/// the tarball staged from its own entries everywhere, and the directory staged as it is on macOS and
/// Linux but refused by name on Windows, whose NTFS keeps neither its modes nor its links' texts.
#[test]
fn a_tarball_root_stages_everywhere_and_a_directory_root_is_refused_on_windows() {
    let host = tempfile::tempdir().unwrap();
    let tarball = host.path().join("rootfs.tar.gz");
    let got = rewrite(oci(Rootfs::Dir(tarball.clone()), vec![]), &[], STAGED).unwrap();
    assert_eq!(got.items[0].source, Source::Tar(tarball));
    let dir = host.path().join("rootfs");
    let got = rewrite(oci(Rootfs::Dir(dir.clone()), vec![]), &[], STAGED);
    if cfg!(windows) {
        let Err(Refusal::Platform { why, .. }) = got else {
            panic!("expected a Windows directory root refused, got {got:?}");
        };
        assert!(why.contains(".tar or .tar.gz"), "{why}");
    } else {
        assert_eq!(got.unwrap().items[0].source, Source::Path(dir));
    }
}

/// How a stand-in guest answers its first dial.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Boots {
    /// Its relay answers: the token, then the node's hello.
    Ready,
    /// Alive, its relay never listening: the dial's patience runs out.
    Hung,
}

/// A stand-in for Shards whose guests boot as `plan` says, one entry a boot, recording each first
/// dial's patience and each stop.
struct Planned {
    plan: Mutex<Vec<Boots>>,
    patience: Arc<Mutex<Vec<Duration>>>,
    stops: Arc<AtomicUsize>,
    boots: AtomicUsize,
}

impl Planned {
    fn new(plan: &[Boots]) -> Self {
        Self {
            plan: Mutex::new(plan.iter().rev().copied().collect()),
            patience: Arc::new(Mutex::new(Vec::new())),
            stops: Arc::new(AtomicUsize::new(0)),
            boots: AtomicUsize::new(0),
        }
    }
}

struct PlannedGuest {
    boots: Boots,
    patience: Arc<Mutex<Vec<Duration>>>,
    stops: Arc<AtomicUsize>,
}

/// A loopback stream as Shards' guest stream.
struct Looped(std::net::TcpStream);

impl std::io::Read for Looped {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl std::io::Write for Looped {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

impl shards_api::Stream for Looped {
    fn try_clone(&self) -> std::io::Result<Box<dyn shards_api::Stream>> {
        Ok(Box::new(Looped(self.0.try_clone()?)))
    }

    fn shutdown(&self, how: std::net::Shutdown) -> std::io::Result<()> {
        self.0.shutdown(how)
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.0.set_read_timeout(timeout)
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.0.set_write_timeout(timeout)
    }
}

/// A relay and node agent in one: the connection's token, then the answer to its hello.
fn ready_relay() -> std::net::TcpStream {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        conn.write_all(&7u64.to_le_bytes()).unwrap();
        let mut d = stream::Deframer::new(hex_core::wire::MAX_MESSAGE);
        let mut byte = [0u8; 1];
        let message = loop {
            conn.read_exact(&mut byte).unwrap();
            if let (_, Some(m)) = d.take(&byte).unwrap() {
                break m;
            }
        };
        let (h, _) =
            hex_core::wire::read_request_apart(&message, hex_core::wire::MAX_MESSAGE).unwrap();
        let hello = hex_core::wire::Answer::Hello(hex_core::wire::Hello {
            version: hex_core::wire::VERSION,
            classes: vec![hex_api::Class::Oci],
            limits: hex_core::wire::Limits {
                connections: 1,
                message_bytes: hex_core::wire::MAX_MESSAGE,
                in_flight: 1,
                programs: 1,
            },
        });
        let bytes = hex_core::wire::answer(
            hex_core::wire::FIRST,
            h.id,
            &hello,
            hex_core::wire::MAX_MESSAGE,
        )
        .unwrap();
        conn.write_all(&stream::frame(&bytes, hex_core::wire::MAX_MESSAGE).unwrap())
            .unwrap();
        // Held until the client goes, as a relay's connection is.
        let _ = conn.read(&mut byte);
    });
    std::net::TcpStream::connect(addr).unwrap()
}

impl shards_api::Guest for PlannedGuest {
    fn connect(
        &self,
        port: u32,
        patience: Duration,
    ) -> Result<Box<dyn shards_api::Stream>, shards_api::Refusal> {
        self.patience.lock().unwrap().push(patience);
        match self.boots {
            Boots::Ready => Ok(Box::new(Looped(ready_relay()))),
            Boots::Hung => Err(shards_api::Refusal::Channel {
                port,
                why: format!("unanswered in {patience:?}"),
            }),
        }
    }

    fn stop(&self) -> Result<(), shards_api::Refusal> {
        self.stops.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn wait_for(&self, _: Duration) -> Result<Option<shards_api::End>, shards_api::Refusal> {
        Ok(None)
    }
}

impl shards_api::Guests for Planned {
    fn backend(&self) -> Result<shards_api::Backend, shards_api::Refusal> {
        shards_api::backend()
    }

    fn boot(
        &self,
        _spec: &shards_api::GuestSpec,
    ) -> Result<Box<dyn shards_api::Guest>, shards_api::Refusal> {
        self.boots.fetch_add(1, Ordering::SeqCst);
        let boots = self.plan.lock().unwrap().pop().unwrap();
        Ok(Box::new(PlannedGuest {
            boots,
            patience: Arc::clone(&self.patience),
            stops: Arc::clone(&self.stops),
        }))
    }
}

/// HEX-GUEST-34 (the lead's rule for learned waits): boot a guest before any boot is seen: expect its
/// first dial bounded by twice the record's slowest boot to ready, never unbounded.
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn the_first_boot_is_bounded_by_the_record() {
    if shards_api::backend().is_err() {
        return;
    }
    let guests = Arc::new(Planned::new(&[Boots::Ready]));
    let rt = GuestRuntime::new(guests.clone(), config());
    rt.guest().unwrap();
    let given = guests.patience.lock().unwrap()[0];
    assert!(
        given <= 2 * recorded_boot().unwrap() && given > recorded_boot().unwrap(),
        "{given:?}"
    );
}

/// HEX-GUEST-34: tell the runtime of a boot slower than the record, then a faster one, and boot: expect
/// the first dial bounded by twice the slowest seen, the bound only rising.
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn the_bound_rises_with_a_slow_boot_and_never_falls() {
    if shards_api::backend().is_err() {
        return;
    }
    let guests = Arc::new(Planned::new(&[Boots::Ready]));
    let rt = GuestRuntime::new(guests.clone(), config());
    let slow = 3 * recorded_boot().unwrap();
    rt.learn(slow);
    rt.learn(recorded_boot().unwrap() / 2);
    assert_eq!(rt.first_dial_bound().unwrap(), 2 * slow);
    rt.guest().unwrap();
    let given = guests.patience.lock().unwrap()[0];
    assert!(given <= 2 * slow && given > slow, "{given:?}");
}

/// HEX-GUEST-34 (no caller fails where an alternative exists): boot a guest that stays alive and never
/// listens, then one that does: expect the first stopped and counted, the second booted and used.
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn a_hung_guest_is_retired_and_booted_again() {
    if shards_api::backend().is_err() {
        return;
    }
    let guests = Arc::new(Planned::new(&[Boots::Hung, Boots::Ready]));
    let rt = GuestRuntime::new(guests.clone(), config());
    rt.guest().unwrap();
    assert_eq!(guests.boots.load(Ordering::SeqCst), 2);
    assert_eq!(guests.stops.load(Ordering::SeqCst), 1);
    assert_eq!(rt.retired(), 1);
}

/// HEX-GUEST-34: boot two guests that both hang: expect the caller refused by type, naming the boot and
/// its bound, after exactly two boots, both stopped.
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn a_second_hang_is_refused_by_type() {
    if shards_api::backend().is_err() {
        return;
    }
    let guests = Arc::new(Planned::new(&[Boots::Hung, Boots::Hung]));
    let rt = GuestRuntime::new(guests.clone(), config());
    let Err(Refusal::Os { op, why }) = rt.guest().map(|_| ()) else {
        panic!("expected the boot refused");
    };
    assert_eq!(op, "booting the node's guest");
    assert!(why.contains("ready"), "{why}");
    assert_eq!(guests.boots.load(Ordering::SeqCst), 2);
    assert_eq!(guests.stops.load(Ordering::SeqCst), 2);
    assert_eq!(rt.retired(), 2);
}

/// HEX-GUEST-34 (the parent's rule: a host class without a record doesn't seed its bound): on Linux (KVM),
/// whose boot-to-ready record is pending (records.tsv holds HVF's and WHP's rows alone), ask for a guest:
/// expect it refused by type, naming the pending record, before anything boots, never a bound borrowed from
/// another class.
#[cfg(target_os = "linux")]
#[test]
fn a_host_class_without_a_record_refuses_by_name() {
    let guests = Arc::new(Planned::new(&[Boots::Ready, Boots::Hung]));
    let rt = GuestRuntime::new(guests.clone(), config());
    let got = rt.guest().map(|_| ());
    let Err(Refusal::Platform { why, .. }) = got else {
        panic!("expected the pending record named, got {got:?}");
    };
    assert!(why.contains("records.tsv") && why.contains(CLASS), "{why}");
    assert_eq!(guests.boots.load(Ordering::SeqCst), 0);
}

/// HEX-GUEST-34 (the lead: the record is the input that turns a class on): read a records table with
/// HVF's rows alone, then with a KVM row added: expect KVM to have no seed from the first (HVF's never
/// borrowed) and its own from the second, with no code between them.
#[test]
fn a_records_row_is_what_turns_a_class_on() {
    let hvf = "# measure\tclass\tslowest_us\trecord\nboot_ready\tmacOS-HVF\t265000\tr.md\n";
    assert_eq!(
        slowest_in(hvf, "boot_ready", "macOS-HVF"),
        Some(Duration::from_millis(265))
    );
    assert_eq!(slowest_in(hvf, "boot_ready", "Linux-KVM"), None);
    let kvm = format!("{hvf}boot_ready\tLinux-KVM\t180000\tr.md\n");
    assert_eq!(
        slowest_in(&kvm, "boot_ready", "Linux-KVM"),
        Some(Duration::from_millis(180))
    );
    assert_eq!(slowest_in(&kvm, "stop", "Linux-KVM"), None);
}
