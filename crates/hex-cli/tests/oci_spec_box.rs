//! Runtime goal 3, plan step 5 (decision 3; AC-T7.1.1, T-T7.1.2): a box made from the structured
//! `OciSpec` runs from the bundle the node renders for it. "do X, expect Y" (§6):
//! - a box from layers [base, app] whose init is `sleep infinity`, with one read-only bind mount, a
//!   hostname, a hosts line and a private network: its root is an overlay; `/usr` is the bind and
//!   read-only; its hostname, `/etc/hosts` line and network namespace are the spec's, the network
//!   holding loopback alone, up; what it writes lands in its own writable layer and the layers are
//!   untouched; after delete nothing of it is left: no bundle (so no writable layer), no container
//!   state, no cgroup, no record, no mount;
//! - a box from a directory on the host's network: its writes land in the directory, in place, its
//!   network namespace is the node's, and after delete nothing of it is left but those writes;
//! - the hex-api conformance suite (HEX-RT-1 to 13) on boxes made from OciSpecs;
//! - a spec this node can't render (a root or a mount source that isn't there, a user a rootless
//!   node can't map) is refused by the field's name, and nothing is made;
//! - a crash mid-create leaves a rendered bundle that recovery removes, however its layer was left.
//!
//! Rootless where the caller isn't root (overlayfs mounts unprivileged since Linux 5.11), as root in a
//! privileged container otherwise (bench/oci-validation's setting); in a delegated cgroup v2 subtree,
//! as oci_box.rs. Where none of that is possible it prints SKIP and why rather than pass falsely.
#![cfg(target_os = "linux")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use hex_api::conformance::{self, Act, Fixture};
use hex_api::{
    Bounds, BoxId, BoxSpec, BoxState, ClassSpec, Exit, Mount, Network, OciSpec, Program, Refusal,
    Rootfs, Runtime, Stdio, User,
};
use hex_core::record::{BoxRecord, Phase};
use hex_node::Node;
use hex_node::store::Store;

const TEST: &str = "oci_spec_boxes_run_from_the_bundles_the_node_renders";

#[test]
fn oci_spec_boxes_run_from_the_bundles_the_node_renders() {
    let exe = env!("CARGO_BIN_EXE_hex").to_owned();
    let cache = cache_dir();
    match prepare_cgroup(&cache) {
        Ok(own) => exercise(&own, &cache, &exe),
        Err(direct) if std::env::var_os("HEX_DELEGATED").is_none() => {
            let has_systemd = Command::new("systemd-run")
                .arg("--version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !has_systemd {
                eprintln!(
                    "SKIP: own cgroup isn't ours to subdivide ({direct}) and no systemd --user for a private scope"
                );
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
            eprintln!("SKIP: no usable cgroup v2 delegation ({direct}); run on a cgroup-v2 host")
        }
    }
}

/// One run's layers, directories and nodes.
struct Lab {
    exe: String,
    work: tempfile::TempDir,
    base: PathBuf,
    app: PathBuf,
    rootless: bool,
}

impl Lab {
    fn new(cache: &Path, exe: &str) -> Self {
        let work = tempfile::Builder::new()
            .prefix("ocispec-")
            .tempdir_in(cache)
            .expect("work dir");
        let base = work.path().join("layers/base");
        build_base(&base);
        let app = work.path().join("layers/app");
        std::fs::create_dir_all(app.join("app")).unwrap();
        std::fs::write(app.join("app/hello"), "hello from the app layer\n").unwrap();
        Self {
            exe: exe.to_owned(),
            work,
            base,
            app,
            rootless: host_id("-u") != 0,
        }
    }

    /// A node over its own box root and state.
    fn node(&self, own: &Path, name: &str) -> (Node, PathBuf, PathBuf) {
        let box_root = own.join(format!("hex-ocispec-{name}"));
        let state = self.work.path().join(format!("state-{name}"));
        std::fs::create_dir_all(&state).unwrap();
        let node = Node::open(&box_root, &state, &state)
            .expect("open a node")
            .with_oci_helper(PathBuf::from(&self.exe));
        (node, box_root, state)
    }

    /// The conformance suite's container: the base layer alone, the host's `/usr` and the `hex`
    /// binary bound in read-only, a private network.
    fn conformance_spec(&self, args: &[String]) -> OciSpec {
        OciSpec {
            rootfs: Rootfs::Layers(vec![self.base.clone()]),
            init: Program::of(&args.iter().map(String::as_str).collect::<Vec<_>>()),
            mounts: vec![usr(), bind(&self.exe, "/hex")],
            hostname: None,
            network: Network::Private,
            hosts: Vec::new(),
        }
    }

    /// The `hex selftest-box` program for `act`, as the container sees the binary (`/hex`).
    fn act(act: &Act) -> Vec<String> {
        let mut args = vec!["/hex".to_owned(), "selftest-box".to_owned()];
        match act {
            Act::Say(words) => args.extend(["say".to_owned(), (*words).to_owned()]),
            Act::Fail(code) => args.extend(["fail".to_owned(), code.to_string()]),
            Act::Wait => args.push("wait".to_owned()),
            Act::Hog(bytes) => args.extend(["hog".to_owned(), bytes.to_string()]),
            Act::Spawn(n) => args.extend(["spawn".to_owned(), n.to_string()]),
            Act::Echo => args.push("echo".to_owned()),
        }
        args
    }
}

impl Fixture for Lab {
    fn spec(&self, id: BoxId, bounds: Bounds) -> BoxSpec {
        BoxSpec {
            id,
            bounds,
            class: ClassSpec::Oci(Box::new(self.conformance_spec(&Self::act(&Act::Wait)))),
        }
    }

    fn init(&self, id: BoxId, act: &Act) -> BoxSpec {
        BoxSpec {
            id,
            bounds: Bounds::default(),
            class: ClassSpec::Oci(Box::new(self.conformance_spec(&Self::act(act)))),
        }
    }

    fn program(&self, act: &Act) -> Program {
        let args = Self::act(act);
        Program::of(&args.iter().map(String::as_str).collect::<Vec<_>>())
    }
}

fn exercise(own: &Path, cache: &Path, exe: &str) {
    let lab = Lab::new(cache, exe);
    eprintln!(
        "oci_spec_box: {} (uid {}), kernel {}",
        if lab.rootless { "rootless" } else { "as root" },
        host_id("-u"),
        std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .unwrap_or_default()
            .trim()
    );
    // A rootless container needs unprivileged user namespaces; a host without them can't run one,
    // which is a reason to skip, said. Past that, any failure is the lab's to report.
    if lab.rootless && !userns_available() {
        eprintln!("SKIP: unprivileged user namespaces aren't available here");
        return;
    }
    layered_box(&lab, own);
    in_place_box(&lab, own);
    links_in_a_layer_stay_in_the_box(&lab, own);
    conformance_on_specs(&lab, own);
    refusals(&lab, own);
    recovery_removes_a_half_made_bundle(&lab, own);
}

/// The plan step's example: layers [base, app], `sleep infinity`, one read-only bind, a private
/// network (with a hostname and a hosts line).
fn layered_box(lab: &Lab, own: &Path) {
    let (node, box_root, state) = lab.node(own, "layers");
    let before = (snapshot(&lab.base), snapshot(&lab.app));
    let id = BoxId::new("layered").unwrap();
    let spec = OciSpec {
        rootfs: Rootfs::Layers(vec![lab.base.clone(), lab.app.clone()]),
        init: Program::of(&["sleep", "infinity"]),
        mounts: vec![usr()],
        hostname: Some("spec-box".to_owned()),
        network: Network::Private,
        hosts: vec![("db_main".to_owned(), "10.1.2.3".to_owned())],
    };
    let made = node.create(BoxSpec {
        id: id.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Oci(Box::new(spec)),
    });
    assert_eq!(made.expect("create").state, BoxState::Created);
    let _cleanup = Deleting(&node, id.clone());
    node.start(&id).expect("start");
    assert_eq!(node.inspect(&id).unwrap().state, BoxState::Running);
    let layer = state.join("bundles/layered");

    // Its root is the overlay, over the box's own writable layer; /usr is the bind, read-only.
    let mounts = mountinfo(&run(&node, &id, "cat /proc/self/mountinfo"));
    let root = mounts
        .iter()
        .rev()
        .find(|m| m.point == "/")
        .expect("a root");
    assert_eq!(root.fstype, "overlay", "the root: {root:?}");
    assert!(
        root.super_options
            .contains(&format!("upperdir={}", layer.join("upper").display())),
        "the overlay's writable layer is the box's own: {root:?}"
    );
    let bound = mounts
        .iter()
        .rev()
        .find(|m| m.point == "/usr")
        .expect("/usr");
    assert!(
        bound.options.split(',').any(|o| o == "ro"),
        "/usr is read-only: {bound:?}"
    );
    assert_ne!(
        exit_of(&node, &id, "touch /usr/hex-probe"),
        Exit::Code(0),
        "a write to the read-only bind is refused"
    );
    // Its hostname, hosts line and network are the spec's: its own network namespace, holding
    // loopback alone, up (IFF_UP, 0x1).
    assert_eq!(
        run(&node, &id, "cat /proc/sys/kernel/hostname").trim(),
        "spec-box"
    );
    let hosts = run(&node, &id, "cat /etc/hosts");
    assert!(
        hosts.starts_with("127.0.0.1\tlocalhost\n") && hosts.ends_with("10.1.2.3\tdb_main\n"),
        "/etc/hosts: {hosts:?}"
    );
    assert_ne!(
        run(&node, &id, "readlink /proc/self/ns/net").trim(),
        own_ns("net"),
        "a private network is a namespace of the box's own"
    );
    assert_eq!(run(&node, &id, "ls /sys/class/net").trim(), "lo");
    let flags = run(&node, &id, "cat /sys/class/net/lo/flags");
    let flags = u32::from_str_radix(flags.trim().trim_start_matches("0x"), 16).unwrap();
    assert_eq!(flags & 0x1, 0x1, "loopback is up: flags {flags:#x}");

    // Writes land in the box's own layer: a new file, a layer's file changed and one removed; the
    // layers are untouched.
    assert_eq!(
        exit_of(
            &node,
            &id,
            "echo written > /written && echo changed > /app/hello && rm /base-file"
        ),
        Exit::Code(0)
    );
    let upper = layer.join("upper");
    assert_eq!(
        std::fs::read_to_string(upper.join("written")).unwrap(),
        "written\n"
    );
    assert_eq!(
        std::fs::read_to_string(upper.join("app/hello")).unwrap(),
        "changed\n"
    );
    let whiteout = std::fs::symlink_metadata(upper.join("base-file")).unwrap();
    assert!(
        whiteout.file_type().is_char_device() && whiteout.rdev() == 0,
        "a removed layer file is a whiteout in the box's layer"
    );
    assert_eq!(
        (snapshot(&lab.base), snapshot(&lab.app)),
        before,
        "the layers are untouched"
    );

    node.delete(&id).expect("delete");
    nothing_left(&node, &id, &box_root, &state);
    assert_eq!(
        (snapshot(&lab.base), snapshot(&lab.app)),
        before,
        "the layers are untouched by delete"
    );
}

/// A directory root is used in place: the box's writes land in it, and after delete they're all
/// that's left of the box. On the host's network, its network namespace is the node's.
fn in_place_box(lab: &Lab, own: &Path) {
    let (node, box_root, state) = lab.node(own, "dir");
    let dir = lab.work.path().join("in-place");
    build_base(&dir);
    let id = BoxId::new("in-place").unwrap();
    node.create(BoxSpec {
        id: id.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Oci(Box::new(OciSpec {
            rootfs: Rootfs::Dir(dir.clone()),
            init: Program::of(&["sleep", "infinity"]),
            mounts: vec![usr()],
            hostname: None,
            network: Network::Host,
            hosts: Vec::new(),
        })),
    })
    .expect("create");
    let _cleanup = Deleting(&node, id.clone());
    node.start(&id).expect("start");
    assert_eq!(
        exit_of(&node, &id, "echo in-place > /written-in-place"),
        Exit::Code(0)
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("written-in-place")).unwrap(),
        "in-place\n"
    );
    assert_eq!(
        run(&node, &id, "readlink /proc/self/ns/net").trim(),
        own_ns("net"),
        "the host's network is the node's namespace"
    );
    assert_eq!(
        run(&node, &id, "cat /proc/self/cgroup").trim(),
        "0::/",
        "the box's cgroup is the root of its cgroup namespace"
    );
    node.delete(&id).expect("delete");
    nothing_left(&node, &id, &box_root, &state);
    assert!(
        dir.join("written-in-place").exists(),
        "a directory root keeps what was written in it"
    );
}

/// An image's links stay in its box. A layer whose `/etc` is an absolute link to a host directory:
/// the node writes nothing there, refuses the box by the path that leads out, and leaves nothing of
/// it. A layer whose `/etc` is an absolute link to a directory in the image: followed there, inside
/// the box's root, so the box's `/etc/hosts` is made in its own layer and the box sees it.
fn links_in_a_layer_stay_in_the_box(lab: &Lab, own: &Path) {
    let probe = lab.work.path().join("escape-probe");
    std::fs::create_dir_all(&probe).unwrap();
    let out = lab.work.path().join("layers/link-out");
    std::fs::create_dir_all(&out).unwrap();
    std::os::unix::fs::symlink(&probe, out.join("etc")).unwrap();
    let inside = lab.work.path().join("layers/link-inside");
    std::fs::create_dir_all(inside.join("real-etc")).unwrap();
    std::os::unix::fs::symlink("/real-etc", inside.join("etc")).unwrap();
    let (node, box_root, state) = lab.node(own, "links");
    let spec = |layer: &Path| {
        ClassSpec::Oci(Box::new(OciSpec {
            rootfs: Rootfs::Layers(vec![lab.base.clone(), layer.to_path_buf()]),
            init: Program::of(&["sleep", "infinity"]),
            mounts: vec![usr()],
            hostname: None,
            network: Network::Private,
            hosts: vec![("db".to_owned(), "10.1.2.4".to_owned())],
        }))
    };
    let wrote = |probe: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(probe)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect()
    };

    let id = BoxId::new("link-out").unwrap();
    let made = node.create(BoxSpec {
        id: id.clone(),
        bounds: Bounds::default(),
        class: spec(&out),
    });
    let _cleanup = Deleting(&node, id.clone());
    assert_eq!(
        wrote(&probe),
        Vec::<PathBuf>::new(),
        "the node wrote through a layer's link onto the host"
    );
    assert!(
        matches!(&made, Err(Refusal::Os { why, .. })
            if why.contains("/etc/hosts: etc in the container's root is a link to nothing inside it")),
        "{made:?}"
    );
    nothing_left(&node, &id, &box_root, &state);

    let id = BoxId::new("link-inside").unwrap();
    node.create(BoxSpec {
        id: id.clone(),
        bounds: Bounds::default(),
        class: spec(&inside),
    })
    .expect("create");
    let _cleanup = Deleting(&node, id.clone());
    node.start(&id).expect("start");
    let hosts = run(&node, &id, "cat /etc/hosts");
    assert!(hosts.ends_with("10.1.2.4\tdb\n"), "/etc/hosts: {hosts:?}");
    assert!(
        state
            .join("bundles/link-inside/upper/real-etc/hosts")
            .is_file(),
        "the bind's target is made in the box's own layer, where the link leads"
    );
    node.delete(&id).expect("delete");
    nothing_left(&node, &id, &box_root, &state);
    assert_eq!(wrote(&probe), Vec::<PathBuf>::new());
}

/// AC-T7.1.1: the hex-api conformance suite holds for boxes made from OciSpecs, HEX-RT-13 (an
/// unrenderable spec refused by name) included.
fn conformance_on_specs(lab: &Lab, own: &Path) {
    let (node, _, _) = lab.node(own, "conformance");
    let failures = conformance::run(&node, lab);
    assert!(failures.is_empty(), "conformance failures: {failures:#?}");
}

/// What this node can't render is refused by the field's name, and nothing is made: a root or a
/// mount source that isn't there, a layer that isn't a directory, and, rootless, a user the node
/// can't map into the container.
fn refusals(lab: &Lab, own: &Path) {
    let (node, box_root, state) = lab.node(own, "refusals");
    let id = BoxId::new("refused").unwrap();
    let missing = lab.work.path().join("missing");
    let mut cases: Vec<(&str, OciSpec)> = Vec::new();
    let mut s = lab.conformance_spec(&Lab::act(&Act::Wait));
    s.rootfs = Rootfs::Dir(missing.clone());
    cases.push(("rootfs.dir", s));
    let mut s = lab.conformance_spec(&Lab::act(&Act::Wait));
    s.rootfs = Rootfs::Layers(vec![lab.base.clone(), lab.app.join("app/hello")]);
    cases.push(("rootfs.layers[1]", s));
    let mut s = lab.conformance_spec(&Lab::act(&Act::Wait));
    s.mounts.push(bind(&missing.to_string_lossy(), "/missing"));
    cases.push(("mounts[2].source", s));
    if lab.rootless {
        let mut s = lab.conformance_spec(&Lab::act(&Act::Wait));
        s.init.user = Some(User {
            uid: 1000,
            gid: 1000,
            groups: Vec::new(),
        });
        cases.push(("init.user", s));
    }
    for (field, spec) in cases {
        let got = node.create(BoxSpec {
            id: id.clone(),
            bounds: Bounds::default(),
            class: ClassSpec::Oci(Box::new(spec)),
        });
        assert!(
            matches!(&got, Err(Refusal::Invalid { what: "oci spec", why }) if why.contains(field)),
            "{field}: {got:?}"
        );
        nothing_left(&node, &id, &box_root, &state);
    }
}

/// T-T7.1.2: the node stopped mid-create, after the record and the rendered bundle and before the
/// cgroup; its layer already holds a directory of mode 0 and a deep tree, as a container can leave
/// one. Recovery removes the box, its bundle with it.
fn recovery_removes_a_half_made_bundle(lab: &Lab, own: &Path) {
    let (node, box_root, state) = lab.node(own, "recovery");
    drop(node);
    let id = BoxId::new("half-made").unwrap();
    let bundle = state.join("bundles/half-made");
    let mut deep = bundle.join("upper");
    for _ in 0..200 {
        deep.push("d");
    }
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::create_dir_all(bundle.join("work/work")).unwrap();
    std::fs::set_permissions(
        bundle.join("work/work"),
        std::fs::Permissions::from_mode(0o000),
    )
    .unwrap();
    let spec = BoxSpec {
        id: id.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Oci(Box::new(lab.conformance_spec(&Lab::act(&Act::Wait)))),
    };
    let record = BoxRecord::new(
        spec,
        Phase::Creating,
        vec![state.join("oci/half-made"), bundle.clone()],
    );
    Store::open(&state).unwrap().put(&record).unwrap();
    let (restarted, _, _) = lab.node(own, "recovery");
    let summary = restarted.recover().expect("recover");
    assert!(
        summary.removed.iter().any(|(r, _)| *r == id),
        "not removed: {summary:?}"
    );
    assert!(!bundle.exists(), "the half-made bundle is left");
    nothing_left(&restarted, &id, &box_root, &state);
}

/// Deletes its box when dropped, so a check that fails leaves no container running.
struct Deleting<'a>(&'a Node, BoxId);

impl Drop for Deleting<'_> {
    fn drop(&mut self) {
        let _ = self.0.delete(&self.1);
    }
}

/// Nothing of box `id` is left on the node: no bundle, no container state, no cgroup, no record, and
/// no mount anywhere naming its bundle.
fn nothing_left(node: &Node, id: &BoxId, box_root: &Path, state: &Path) {
    let bundle = state.join("bundles").join(id.as_str());
    assert!(!bundle.exists(), "{id}'s bundle is left");
    assert!(
        !state.join("oci").join(id.as_str()).exists(),
        "{id}'s container state is left"
    );
    assert!(
        !box_root.join(id.as_str()).exists(),
        "{id}'s cgroup is left"
    );
    assert!(
        !state.join(format!("{id}.json")).exists(),
        "{id}'s record is left"
    );
    assert_eq!(node.inspect(id), Err(Refusal::NoSuchBox(id.clone())));
    let mounts = std::fs::read_to_string("/proc/self/mountinfo").unwrap();
    assert!(
        !mounts.contains(&bundle.to_string_lossy().into_owned()),
        "a mount names {id}'s bundle"
    );
}

/// `sh -c script` in the box; its stdout, once it has exited 0.
fn run(node: &Node, id: &BoxId, script: &str) -> String {
    let (exit, out) = exec(node, id, script);
    assert_eq!(exit, Exit::Code(0), "{script}: {out}");
    out
}

fn exit_of(node: &Node, id: &BoxId, script: &str) -> Exit {
    exec(node, id, script).0
}

fn exec(node: &Node, id: &BoxId, script: &str) -> (Exit, String) {
    let (mut r, w) = std::io::pipe().unwrap();
    let mut proc = node
        .exec(
            id,
            Program::of(&["/bin/sh", "-c", script]),
            Stdio {
                stdout: Some(w),
                ..Stdio::null()
            },
        )
        .expect("exec");
    let mut out = String::new();
    r.read_to_string(&mut out).unwrap();
    let exit = proc
        .wait_for(conformance::PATIENCE)
        .unwrap()
        .expect("the program ends");
    (exit, out)
}

/// One `/proc/<pid>/mountinfo` line (proc_pid_mountinfo(5)).
#[derive(Debug)]
struct MountLine {
    point: String,
    options: String,
    fstype: String,
    super_options: String,
}

fn mountinfo(text: &str) -> Vec<MountLine> {
    text.lines()
        .map(|line| {
            let (left, right) = line.split_once(" - ").unwrap();
            let left: Vec<&str> = left.split(' ').collect();
            let right: Vec<&str> = right.split(' ').collect();
            MountLine {
                point: left[4].to_owned(),
                options: left[5].to_owned(),
                fstype: right[0].to_owned(),
                super_options: right[2].to_owned(),
            }
        })
        .collect()
}

/// Every entry under `dir`, by path: its kind and mode, and a file's bytes or a link's target. Walked
/// with a stack.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, (String, u32, Vec<u8>)> {
    let mut out = BTreeMap::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for entry in std::fs::read_dir(&d).unwrap() {
            let path = entry.unwrap().path();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            let (kind, bytes) = if meta.is_dir() {
                todo.push(path.clone());
                ("dir", Vec::new())
            } else if meta.file_type().is_symlink() {
                (
                    "link",
                    std::fs::read_link(&path)
                        .unwrap()
                        .into_os_string()
                        .into_encoded_bytes(),
                )
            } else {
                ("file", std::fs::read(&path).unwrap())
            };
            out.insert(
                path.strip_prefix(dir).unwrap().to_path_buf(),
                (kind.to_owned(), meta.permissions().mode(), bytes),
            );
        }
    }
    out
}

/// A layer that's a root of its own: the directories a container's mounts land on, a file and a
/// directory of its own, and the host's top-level links into `/usr` (`/bin`, `/lib64`, …), so the
/// loader and `/bin/sh` resolve, through the `/usr` bind, as on the host.
fn build_base(dir: &Path) {
    for d in ["proc", "dev", "sys", "tmp", "usr", "etc", "app"] {
        std::fs::create_dir_all(dir.join(d)).unwrap();
    }
    std::fs::write(dir.join("base-file"), "from the base layer\n").unwrap();
    for link in ["bin", "sbin", "lib", "lib64"] {
        if let Ok(target) = std::fs::read_link(Path::new("/").join(link)) {
            let target = target.to_string_lossy().trim_start_matches('/').to_owned();
            std::os::unix::fs::symlink(target, dir.join(link)).unwrap();
        }
    }
}

fn usr() -> Mount {
    bind("/usr", "/usr")
}

fn bind(source: &str, destination: &str) -> Mount {
    Mount {
        source: PathBuf::from(source),
        destination: destination.to_owned(),
        read_only: true,
    }
}

/// This process's namespace of `kind`, as `readlink /proc/self/ns/<kind>` prints it.
fn own_ns(kind: &str) -> String {
    std::fs::read_link(format!("/proc/self/ns/{kind}"))
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

/// Whether this process may make a user namespace: `unshare -U` of a program that does nothing.
fn userns_available() -> bool {
    Command::new("unshare")
        .args(["-U", "true"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn host_id(flag: &str) -> u32 {
    let out = Command::new("id").arg(flag).output().expect("id");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("id is a number")
}

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

/// Moves this process into a leaf of its own cgroup and confirms every controller is delegated, as
/// oci_box.rs does; returns the own cgroup dir, or why it isn't ours to subdivide.
fn prepare_cgroup(disk: &Path) -> Result<PathBuf, String> {
    let own = own_cgroup();
    let leaf = own.join("hex-agent");
    std::fs::create_dir_all(&leaf).map_err(|e| format!("making a leaf for the agent: {e}"))?;
    std::fs::write(leaf.join("cgroup.procs"), format!("{}", std::process::id()))
        .map_err(|e| format!("moving self into a leaf: {e}"))?;
    let probe = hex_runtime::linux::cgroup::BoxRoot::open(
        &own.join("hex-probe"),
        hex_runtime::linux::disks::under(disk).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;
    let missing: Vec<&str> = hex_core::cgroup::CONTROLLERS
        .into_iter()
        .filter(|c| !probe.controllers().contains(*c))
        .collect();
    let _ = std::fs::remove_dir(own.join("hex-probe"));
    if missing.is_empty() {
        Ok(own)
    } else {
        Err(format!(
            "{} can't hand down the {} controllers",
            own.display(),
            missing.join(", ")
        ))
    }
}

fn cache_dir() -> PathBuf {
    let cache =
        PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".cache/hex-t7/ocispec");
    std::fs::create_dir_all(&cache).expect("cache dir");
    cache
}
