//! The node VM's boot under shards-init (docs/design/node-under-shards-init.md §4, §6), against a Shards of the test's
//! own: its image boot and its run, each call recorded, and its run's readiness or its end as each case says.

use std::sync::{Arc, Mutex};

use shards_api::image::{
    Bound, Channel, MemberEnd, MemberInfo, MemberName, RunEnd, Stats, StatsScope,
};

use super::*;

/// What the test's run does when its readiness is asked: says it, or ends first.
#[derive(Debug, Clone)]
enum Says {
    Ready(Ready),
    Ends(RunEnd),
}

/// Every call the test's Shards took.
#[derive(Debug, Default)]
struct Calls {
    /// Each boot's spec and patience.
    boots: Vec<(ImageSpec, Duration)>,
    /// Each readiness wait's `seen` and timeout.
    waits: Vec<(Option<Ready>, Duration)>,
    stops: usize,
}

struct Shards {
    says: Says,
    calls: Arc<Mutex<Calls>>,
}

impl Images for Shards {
    fn backend(&self) -> Result<shards_api::Backend, ImageRefusal> {
        Ok(shards_api::Backend::Kvm)
    }

    fn boot(&self, spec: &ImageSpec, patience: Duration) -> Result<Box<dyn ImageGuest>, ImageRefusal> {
        self.calls.lock().unwrap().boots.push((spec.clone(), patience));
        Ok(Box::new(Run {
            says: self.says.clone(),
            calls: Arc::clone(&self.calls),
        }))
    }
}

/// The test's run of the node image: no member, its readiness as `says` says.
struct Run {
    says: Says,
    calls: Arc<Mutex<Calls>>,
}

fn unasked() -> ImageRefusal {
    ImageRefusal::Unsupported {
        what: "the test's run",
        why: "the node VM's boot never asks it".to_owned(),
    }
}

impl ImageGuest for Run {
    fn members(&self) -> &[MemberInfo] {
        &[]
    }

    fn published(&self) -> &[Bound] {
        &[]
    }

    fn take_output(&self, _: &MemberName, _: Channel) -> Result<std::io::PipeReader, ImageRefusal> {
        Err(unasked())
    }

    fn take_input(&self, _: &MemberName) -> Result<std::io::PipeWriter, ImageRefusal> {
        Err(unasked())
    }

    fn wait_member(&self, _: &MemberName, _: Duration) -> Result<Option<MemberEnd>, ImageRefusal> {
        Err(unasked())
    }

    fn stats(&self, _: StatsScope, _: Duration) -> Result<Stats, ImageRefusal> {
        Err(unasked())
    }

    fn room(&self, _: Option<i64>, _: Duration) -> Result<Option<u64>, ImageRefusal> {
        Err(unasked())
    }

    fn rate(
        &self,
        _: Option<std::num::NonZeroU64>,
        _: Duration,
    ) -> Result<Option<std::num::NonZeroU64>, ImageRefusal> {
        Err(unasked())
    }

    fn resize(&self, _: &MemberName, _: u16, _: u16, _: Duration) -> Result<(), ImageRefusal> {
        Err(unasked())
    }

    fn wait_ready(&self, seen: Option<Ready>, timeout: Duration) -> Result<Option<Ready>, ImageRefusal> {
        self.calls.lock().unwrap().waits.push((seen, timeout));
        match &self.says {
            Says::Ready(ready) => Ok(Some(*ready)),
            Says::Ends(end) => Err(ImageRefusal::Ended(end.clone())),
        }
    }

    fn plug(&self, _: &MemberName, _: &std::path::Path, _: &str, _: bool, _: Duration) -> Result<(), ImageRefusal> {
        Err(unasked())
    }

    fn unplug(&self, _: &str, _: Duration) -> Result<(), ImageRefusal> {
        Err(unasked())
    }

    fn stop(&self) -> Result<(), ImageRefusal> {
        self.calls.lock().unwrap().stops += 1;
        Ok(())
    }

    fn wait_for(&self, _: Duration) -> Result<Option<RunEnd>, ImageRefusal> {
        Ok(Some(RunEnd::Stopped))
    }
}

/// A node's guest under shards-init, its node image, size and shares as a node configures them: inputs to the boot,
/// not a guest's size or an image (any values do).
fn config() -> HostedConfig {
    HostedConfig {
        binary: PathBuf::from("/opt/shards"),
        image: "hex-node@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned(),
        vcpus: 2,
        memory_mib: 2048,
        shares: vec![
            Share {
                dir: PathBuf::from("/Users"),
                read_only: false,
                at: "/run/hex/shares/0".to_owned(),
            },
            Share {
                dir: PathBuf::from("/opt/tools"),
                read_only: true,
                at: "/run/hex/shares/1".to_owned(),
            },
        ],
        env: vec![(OsString::from("SHARDS_HOME"), OsString::from("/var/hex/shards"))],
    }
}

fn hosted(says: Says) -> (Hosted, Arc<Mutex<Calls>>) {
    let calls = Arc::new(Mutex::new(Calls::default()));
    let shards = Shards {
        says,
        calls: Arc::clone(&calls),
    };
    (Hosted::new(Arc::new(shards), config()), calls)
}

/// AC-T7.4.1, joint plan step 39 (original; docs/design/node-under-shards-init.md §4): the node VM boots under
/// shards-init with Hex's own init the run's own command under the nesting grant. Do: boot the node's guest under
/// shards-init on a Shards whose run says READY. Expect, in the one image boot asked:
/// - the pinned image, its size and binary, and the run host's environment;
/// - `/init` alone as its command, and the nesting grant;
/// - at its cold start, no deadline, the boot following its run host (`Duration::MAX`);
/// - the shares at the paths the direct boot's init mounts them at;
/// - no member, no published port, none of the image's grants past the VM, and no credential.
/// Then READY waited for before the boot returns: none seen yet, following its run, its answer the guest's.
#[test]
fn original_the_node_vm_boots_under_shards_init_with_hexs_own_init_and_waits_for_its_ready() {
    let (hosted, calls) = hosted(Says::Ready(Ready::OnTime));
    let booted = hosted.boot().unwrap();
    assert_eq!(booted.ready, Ready::OnTime);
    let calls = calls.lock().unwrap();
    let [(spec, patience)] = calls.boots.as_slice() else {
        panic!("{:?}", calls.boots);
    };
    assert_eq!(*patience, Duration::MAX, "the boot follows its run host");
    let config = config();
    assert_eq!(spec.image, config.image);
    assert_eq!((spec.vcpus, spec.memory_mib), (2, 2048));
    assert_eq!(spec.command, Some(vec!["/init".to_owned()]));
    assert!(spec.nesting, "Hex's init needs the nesting grant (D234)");
    assert_eq!(spec.ready_by, None, "a cold start gives no deadline: it follows its run");
    assert_eq!(spec.binary, Some(PathBuf::from("/opt/shards")));
    assert_eq!(spec.env, config.env);
    assert_eq!(
        spec.shares,
        vec![
            ImageShare {
                host: PathBuf::from("/Users"),
                at: "/run/hex/shares/0".to_owned(),
                read_only: false,
            },
            ImageShare {
                host: PathBuf::from("/opt/tools"),
                at: "/run/hex/shares/1".to_owned(),
                read_only: true,
            },
        ]
    );
    assert!(spec.members.is_empty() && spec.publish.is_empty() && spec.credentials.is_empty());
    assert_eq!(
        spec.egress,
        Narrowing {
            egress: Keep::Only(Vec::new()),
            mcp: Keep::Only(Vec::new()),
            destinations: Keep::Only(Vec::new()),
            dns: false,
            members: None,
        },
        "default deny: none of the image's grants past the VM"
    );
    assert_eq!(calls.waits, [(None, Duration::MAX)], "READY is waited for, following the run");
    assert_eq!(calls.stops, 0);
}

/// AC-T7.4.1, joint plan step 39 (original; §6): a node VM whose run ends before its init says it is ready is no
/// guest. Do: boot on a Shards whose run ends before READY, by its own exit and failed. Expect: the boot refused,
/// typed, naming the run's end, never a guest returned; and the run stopped, so nothing of it outlives the refusal.
#[test]
fn original_a_node_vm_whose_run_ends_before_ready_is_refused_and_stopped() {
    for end in [RunEnd::Exited(1), RunEnd::Failed("its VM failed".to_owned())] {
        let (hosted, calls) = hosted(Says::Ends(end.clone()));
        match hosted.boot() {
            Err(Refusal::Os { why, .. }) => {
                assert!(why.contains("before its init said it was ready"), "{end:?}: {why}");
            }
            Err(other) => panic!("{end:?}: {other:?}"),
            Ok(_) => panic!("{end:?}: a guest whose run ended was returned"),
        }
        assert_eq!(calls.lock().unwrap().stops, 1, "{end:?}");
    }
}

/// AC-T7.4.1 (§6): READY's deadline passed with none is a run-out. Do: boot on a Shards whose run says its deadline
/// passed. Expect: refused, typed, and its run stopped; READY late, past its deadline, is still READY and its guest.
#[test]
fn a_passed_deadline_is_refused_and_a_late_ready_is_ready() {
    let (hosted_passed, calls) = hosted(Says::Ready(Ready::Passed(7)));
    assert!(matches!(hosted_passed.boot(), Err(Refusal::Os { .. })));
    assert_eq!(calls.lock().unwrap().stops, 1);
    let (hosted_late, calls) = hosted(Says::Ready(Ready::Late(7)));
    assert_eq!(hosted_late.boot().unwrap().ready, Ready::Late(7));
    assert_eq!(calls.lock().unwrap().stops, 0);
}

/// AC-T7.4.1 (§4): Shards' refusals are typed by what they refused. Do: map an image the store lacks, a grant the
/// image doesn't give, no backend, and a run host that failed. Expect: the image's and the grant's as the node image's
/// Invalid, no backend as Platform naming the OS, and the run host's as Os.
#[test]
fn shards_refusals_are_typed_by_what_they_refused() {
    let op = "booting the node VM under shards-init";
    assert!(matches!(
        refused(op, ImageRefusal::Image("not in the store".to_owned())),
        Refusal::Invalid { what: "the node image (HEX_GUEST_IMAGE)", .. }
    ));
    assert!(matches!(
        refused(op, ImageRefusal::Widening("no NESTING".to_owned())),
        Refusal::Invalid { what: "the node image (HEX_GUEST_IMAGE)", .. }
    ));
    assert!(matches!(
        refused(op, ImageRefusal::NoBackend { os: "macos", arch: "x86_64" }),
        Refusal::Platform { platform: "macos", .. }
    ));
    assert!(matches!(
        refused(op, ImageRefusal::Host("it ended".to_owned())),
        Refusal::Os { .. }
    ));
}
