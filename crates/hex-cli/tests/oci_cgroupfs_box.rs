//! Goal 5's extension (AC-T7.1.1, T-T7.1.4): a rootless `oci` box on the host's network must not be
//! able to change another box's cgroup, nor the host cgroup tree, even holding `CAP_SYS_ADMIN` in its
//! own user namespace. Such a box binds the host's `/sys` (it can't mount a sysfs of its own in the
//! host's network namespace), which carries the host's cgroup filesystem beneath the box's own
//! (crates/hex-core/src/bundle.rs, `default_mounts`). A box holding `CAP_SYS_ADMIN` can unmount its
//! own cgroup mounts off; this test is the guard that what it then reaches is read-only.
//!
//! "do X, expect Y" (CLAUDE.md §6): a rootless node renders an attacker box holding `CAP_SYS_ADMIN`
//! (the one capability Docker's defaults leave out, added to a rendered bundle) beside a second box.
//! The attacker's init (which holds the capability):
//!   - unmounts the overmounts over `/sys/fs/cgroup` (it can: they are its own mounts);
//!   - tries to write the second box's `memory.max` and `cgroup.kill` and the parent cgroup's
//!     `memory.max` (to reconfigure or kill another box, or reach the host tree);
//!   - reads the second box's and the parent's cgroup files.
//!
//! Expect: every write is refused, read-only (the fix remounts the carried host cgroup filesystem
//! read-only, which a recursive bind's read-only otherwise leaves only its top), so no box's cgroup
//! can be changed and no process migrated. The reads succeed — the host `/sys` is bound read-only and
//! its cgroup submount is locked in the box's user namespace, so it can't be removed, only made
//! read-only; a box without `CAP_SYS_ADMIN` can't reach it at all (it can't unmount its overmounts,
//! so it sees only its own cgroup subtree). The reads are reported, not asserted; closing them too
//! would mean refusing the box, a policy choice left to the owner.
//!
//! Rootless where the caller isn't root, in a delegated cgroup v2 subtree, as oci_box.rs; where that
//! isn't possible it prints SKIP and why rather than pass falsely. The attacker runs through the
//! bundle path (`ClassSpec::Bundle`) because an `OciSpec` carries no capabilities: its config.json is
//! what the node renders for a rootless host-network box, with `CAP_SYS_ADMIN` added to the three
//! capability sets, so the box holds exactly that one capability more than any box the node makes.
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
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use hex_api::{
    Bounds, BoxId, BoxSpec, ClassSpec, Mount, Network, OciSpec, Program, Rootfs, Runtime,
};
use hex_core::bundle::{Owner, Place, render};
use hex_node::Node;

const TEST: &str = "a_cap_sys_admin_box_cannot_change_another_boxs_cgroup";
/// Bounded wait for a fact (a file the attacker's init writes): a run that never writes it fails
/// rather than hangs (CLAUDE.md §4).
const PATIENCE: Duration = Duration::from_secs(30);

#[test]
fn a_cap_sys_admin_box_cannot_change_another_boxs_cgroup() {
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

fn exercise(own: &Path, cache: &Path, exe: &str) {
    let uid = host_id("-u");
    let gid = host_id("-g");
    eprintln!(
        "oci_cgroupfs_box: {} (uid {uid}), kernel {}",
        if uid != 0 { "rootless" } else { "as root" },
        std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .unwrap_or_default()
            .trim()
    );
    if uid == 0 {
        // As root a box shares the host user namespace and gets a fresh sysfs, not a bind of the
        // host's, so there is no host cgroup filesystem beneath its own: nothing to guard here.
        eprintln!("SKIP: this guard is for a rootless node; as root no host cgroup fs is bound in");
        return;
    }
    if !userns_available() {
        eprintln!("SKIP: unprivileged user namespaces aren't available here");
        return;
    }

    let work = tempfile::Builder::new()
        .prefix("ocicgfs-")
        .tempdir_in(cache)
        .expect("work dir");
    let out = work.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let victim_rootfs = work.path().join("rootfs-victim");
    build_rootfs(&victim_rootfs);
    let attacker_rootfs = work.path().join("rootfs-attacker");
    build_rootfs(&attacker_rootfs);

    let box_root = own.join("hex-oci-cgfs");
    let state = work.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    let node = Node::open(&box_root, &state, &state)
        .expect("open a node")
        .with_oci_helper(PathBuf::from(exe));

    // The victim: a plain rootless host-network box, waiting. Its cgroup is box_root/<id>, with the
    // core interface files (cgroup.procs, cgroup.kill) and, the memory controller delegated,
    // memory.max.
    let victim = BoxId::new("victim").unwrap();
    node.create(BoxSpec {
        id: victim.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Oci(Box::new(OciSpec {
            rootfs: Rootfs::Dir(victim_rootfs.clone()),
            init: Program::of(&["/usr/bin/sleep", "600"]),
            mounts: vec![bind("/usr", "/usr", true)],
            hostname: None,
            network: Network::Host,
            hosts: Vec::new(),
        })),
    })
    .expect("create the victim");
    let _victim_cleanup = Deleting(&node, victim.clone());
    node.start(&victim).expect("start the victim");

    let victim_cgroup = box_root.join(victim.as_str());
    for f in ["cgroup.procs", "cgroup.kill", "memory.max"] {
        assert!(
            victim_cgroup.join(f).exists(),
            "the victim's {f} is there to probe: {}",
            victim_cgroup.join(f).display()
        );
    }
    // The paths the attacker aims at, each as it is reached beneath /sys/fs/cgroup: the victim's own
    // cgroup (a sibling) and the box root (the attacker's parent), both outside its subtree.
    let victim_rel = rel_to_cgroup_root(&victim_cgroup);
    let parent_rel = rel_to_cgroup_root(&box_root);

    // The attacker: the exact bundle the node renders for a rootless host-network box, with
    // CAP_SYS_ADMIN added to the capability sets. Its init is the prober.
    let bundle_dir = work.path().join("attacker-bundle");
    std::fs::create_dir_all(&bundle_dir).unwrap();
    let prober = prober_script(&victim_rel, &parent_rel);
    render_attacker_bundle(
        &bundle_dir,
        &attacker_rootfs,
        &out,
        &prober,
        Owner::Rootless { uid, gid },
    );
    let attacker = BoxId::new("attacker").unwrap();
    node.create(BoxSpec {
        id: attacker.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Bundle(bundle_dir),
    })
    .expect("create the attacker box");
    let _attacker_cleanup = Deleting(&node, attacker.clone());
    node.start(&attacker).expect("start the attacker box");

    // The init writes its report line by line and touches `done` last.
    let done = out.join("done");
    let result = out.join("result");
    wait_for(|| done.exists(), "the attacker's init to finish probing");
    let report = std::fs::read_to_string(&result).unwrap_or_default();
    eprintln!("---- attacker report ----\n{report}\n-------------------------");

    let probes = parse_report(&report);
    let detail = |name: &str| -> &str {
        probes
            .get(name)
            .unwrap_or_else(|| panic!("the attacker reported no {name}: {report}"))
    };

    // Every write to another box's cgroup or to the host tree is refused, read-only: a box holding
    // CAP_SYS_ADMIN can't kill another box (cgroup.kill), reconfigure it (memory.max), or reach the
    // host tree to change it. "read-only" rather than just "refused" shows it is the remount, not an
    // incidental failure.
    for name in [
        "write:victim.memory.max",
        "write:victim.cgroup.kill",
        "write:parent.memory.max",
    ] {
        let rest = detail(name);
        assert!(
            rest.starts_with("REFUSED"),
            "{name} was not refused: a CAP_SYS_ADMIN box changed another box's cgroup or the host \
             tree. Full report:\n{report}"
        );
        assert!(
            rest.to_ascii_lowercase().contains("read-only"),
            "{name} was refused but not by the read-only remount ({rest}); report:\n{report}"
        );
    }
    // The host cgroup filesystem the attacker reached, after unmounting its overmounts, is mounted
    // read-only (its per-mount options hold `ro`).
    let opts = detail("cgfs_top_opts");
    assert!(
        opts.split(',').any(|o| o == "ro"),
        "the host cgroup filesystem the box reached is not read-only ({opts}); report:\n{report}"
    );

    node.delete(&attacker).expect("delete the attacker");
    node.delete(&victim).expect("delete the victim");
}

/// The prober the attacker's init runs, holding CAP_SYS_ADMIN in its own user namespace. It strips
/// the overmounts off `/sys/fs/cgroup`, then tries to write the victim's and the parent's cgroup
/// files and to read them, writing `<name>=OK[..]` or `<name>=REFUSED[..]` for each to `/out/result`
/// and touching `/out/done` last. `$v` and `$p` are the victim's and the parent's paths beneath
/// `/sys/fs/cgroup`.
fn prober_script(victim_rel: &str, parent_rel: &str) -> String {
    format!(
        r#"
set +e
O=/out
R=$O/result
: > "$R"
say() {{ printf '%s\n' "$1" >> "$R"; }}
v=/sys/fs/cgroup/{victim_rel}
p=/sys/fs/cgroup/{parent_rel}

say "caps=$(grep CapEff /proc/self/status | tr -d '\t')"
say "self_cgroup=$(cat /proc/self/cgroup)"
say "mnt_before=$(grep -c ' /sys/fs/cgroup' /proc/self/mountinfo)"

# Strip every overmount the box put over /sys/fs/cgroup (it may: they are its own mounts).
i=0
while [ "$i" -lt 5 ]; do
  if umount /sys/fs/cgroup 2>"$O/e"; then say "umount$i=ok"; else say "umount$i=fail[$(tr -d '\n' <"$O/e")]"; break; fi
  i=$((i+1))
done
say "mnt_after=$(grep -c ' /sys/fs/cgroup' /proc/self/mountinfo)"
# The per-mount options (field 6) of the cgroup filesystem now on top: ro once the fix remounts it.
say "cgfs_top_opts=$(grep ' /sys/fs/cgroup ' /proc/self/mountinfo | tail -1 | cut -d' ' -f6)"

read_probe() {{ if out=$(cat "$2" 2>"$O/e"); then say "read:$1=OK[$(printf '%s' "$out" | tr -d '\n' | cut -c1-48)]"; else say "read:$1=REFUSED[$(tr -d '\n' <"$O/e")]"; fi; }}
read_probe victim.cgroup.procs "$v/cgroup.procs"
read_probe victim.memory.max "$v/memory.max"
read_probe parent.cgroup.procs "$p/cgroup.procs"

# The write's own redirection failure (the file opened on a read-only filesystem) is what shows the
# refusal, so capture the subshell's stderr, where the shell reports it.
write_probe() {{ if ( exec 2>"$O/e"; printf '%s' "$3" > "$2" ); then say "write:$1=OK[]"; else say "write:$1=REFUSED[$(tr -d '\n' <"$O/e")]"; fi; }}
write_probe victim.memory.max "$v/memory.max" max
write_probe parent.memory.max "$p/memory.max" max
write_probe victim.cgroup.kill "$v/cgroup.kill" 1

touch "$O/done"
"#
    )
}

/// The report's `<name>=<value>` lines mapped to their value (`OK[..]`, `REFUSED[..]`, or a plain
/// diagnostic).
fn parse_report(report: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in report.lines() {
        if let Some((name, rest)) = line.split_once('=') {
            out.insert(name.to_owned(), rest.to_owned());
        }
    }
    out
}

/// Renders the rootless host-network bundle the node would render for the attacker's spec, adds
/// CAP_SYS_ADMIN to its capability sets, and writes `config.json` and `hosts` into `bundle_dir`.
fn render_attacker_bundle(
    bundle_dir: &Path,
    rootfs: &Path,
    out: &Path,
    prober: &str,
    owner: Owner,
) {
    let spec = OciSpec {
        rootfs: Rootfs::Dir(rootfs.to_path_buf()),
        init: {
            let mut p = Program::of(&["/bin/sh", "-c", prober]);
            p.env.push((
                "PATH".to_owned(),
                "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_owned(),
            ));
            p
        },
        mounts: vec![bind("/usr", "/usr", true), bind_path(out, "/out", false)],
        hostname: None,
        network: Network::Host,
        hosts: Vec::new(),
    };
    let rendered = render(
        &spec,
        &Place {
            dir: bundle_dir,
            owner,
            masked: &[],
        },
    )
    .expect("render the attacker bundle");
    // Add CAP_SYS_ADMIN to each capability set. The rendered config lists Docker's 14 defaults, which
    // end in CAP_AUDIT_WRITE and leave out CAP_SYS_ADMIN; each of the bounding, effective and
    // permitted sets ends in that one, so extending it there adds the capability to all three.
    let config = String::from_utf8(rendered.config).expect("the rendered config is UTF-8");
    assert_eq!(
        config.matches("\"CAP_AUDIT_WRITE\"").count(),
        3,
        "three capability sets to extend with CAP_SYS_ADMIN: {config}"
    );
    let config = config.replace(
        "\"CAP_AUDIT_WRITE\"",
        "\"CAP_AUDIT_WRITE\", \"CAP_SYS_ADMIN\"",
    );
    std::fs::write(bundle_dir.join("config.json"), config).expect("write config.json");
    std::fs::write(bundle_dir.join("hosts"), &rendered.hosts).expect("write hosts");
}

/// A path's location beneath `/sys/fs/cgroup`, as the attacker reaches it there (no leading slash).
fn rel_to_cgroup_root(cgroup: &Path) -> String {
    cgroup
        .strip_prefix("/sys/fs/cgroup")
        .expect("a cgroup path under /sys/fs/cgroup")
        .to_string_lossy()
        .into_owned()
}

fn bind(source: &str, destination: &str, read_only: bool) -> Mount {
    bind_path(Path::new(source), destination, read_only)
}

fn bind_path(source: &Path, destination: &str, read_only: bool) -> Mount {
    Mount {
        source: source.to_path_buf(),
        destination: destination.to_owned(),
        read_only,
    }
}

/// Deletes its box when dropped, so a check that fails leaves no container running.
struct Deleting<'a>(&'a Node, BoxId);

impl Drop for Deleting<'_> {
    fn drop(&mut self) {
        let _ = self.0.delete(&self.1);
    }
}

fn wait_for(mut cond: impl FnMut() -> bool, what: &str) {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

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

/// A rootfs of the host's `/usr`, with the host's top-level links into it (`/bin`, `/lib64`, …) so
/// the loader and `/bin/sh` resolve as on the host, and the directories the box's mounts land on.
fn build_rootfs(rootfs: &Path) {
    for d in ["proc", "dev", "sys", "tmp", "usr", "out"] {
        std::fs::create_dir_all(rootfs.join(d)).unwrap();
    }
    for link in ["bin", "sbin", "lib", "lib64"] {
        if let Ok(target) = std::fs::read_link(Path::new("/").join(link)) {
            let target = target.to_string_lossy().trim_start_matches('/').to_owned();
            std::os::unix::fs::symlink(target, rootfs.join(link)).unwrap();
        }
    }
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
        PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".cache/hex-t7/ocicgfs");
    std::fs::create_dir_all(&cache).expect("cache dir");
    cache
}
