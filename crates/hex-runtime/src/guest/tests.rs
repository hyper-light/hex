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
/// bind: expect its root named by the staged copy and staged from the host path, each bind's source
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

    let layered = rewrite(
        oci(
            Rootfs::Layers(vec![
                PathBuf::from("/Users/l0.tar"),
                PathBuf::from("/Users/l1.tar"),
            ]),
            vec![],
        ),
        &shared,
        STAGED,
    )
    .unwrap();
    let ClassSpec::Oci(spec) = &layered.spec.class else {
        panic!("not oci");
    };
    assert_eq!(
        spec.rootfs,
        Rootfs::Layers(vec![
            PathBuf::from("/run/hex/staged/box-1/layer0"),
            PathBuf::from("/run/hex/staged/box-1/layer1"),
        ])
    );

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
        vcpus: 1,
        memory_mib: 256,
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
