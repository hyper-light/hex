//! AC-T7.4.1: the shared router must reach its selected backend for physical device requests.
//! This is a dispatch regression, not evidence of native GPU attachment.
use super::*;

fn backend(_host: &hex_api::sim::SimRuntime, _class: &ClassSpec) -> Result<Side, Refusal> {
    Err(Refusal::Invalid {
        what: "fixture backend route",
        why: "the actual route was reached".into(),
    })
}

/// A valid claim reaches the native/guest decision on every host, without a platform refusal in
/// the shared wrapper. The destination alone has the configuration/authority to attach it.
#[test]
fn physical_claims_reach_the_configured_backend_route_for_create_and_run() {
    let node = WithGuest {
        process: hex_api::sim::SimRuntime::new(),
        oci: GuestRuntime::new(
            Arc::new(shards_api::sim::SimGuests::new(|_| {
                shards_api::End::PowerOff
            })),
            config(),
        ),
        route: backend,
    };
    let spec = BoxSpec {
        id: BoxId::new("router-selected-device").unwrap(),
        bounds: hex_api::Bounds::default(),
        class: ClassSpec::Process,
        owner: None,
        devices: vec![hex_api::DeviceId {
            kind: hex_api::DeviceKind::Gpu,
            key: 256,
        }],
    };
    spec.check().unwrap();
    let created = node.create(spec.clone());
    let ran = node.run(spec, Program::of(&["unused"]), Stdio::null());
    for refusal in [created.err(), ran.err()] {
        assert!(
            matches!(
                refusal,
                Some(Refusal::Invalid {
                    what: "fixture backend route",
                    ..
                })
            ),
            "actual route was bypassed: {refusal:?}"
        );
    }
    assert!(node.process.list().unwrap().is_empty());
}

struct NativeReporting {
    inner: hex_api::sim::SimRuntime,
    report: hex_api::Capacity,
}

impl Runtime for NativeReporting {
    fn classes(&self) -> Vec<Class> {
        vec![Class::Process, Class::Oci]
    }
    fn capacity(&self, _: Class) -> Result<Option<hex_api::Capacity>, Refusal> {
        Ok(Some(self.report.clone()))
    }
    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        self.inner.create(spec)
    }
    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        self.inner.start(id)
    }
    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        self.inner.exec(id, program, stdio)
    }
    fn run(&self, spec: BoxSpec, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        self.inner.run(spec, program, stdio)
    }
    fn signal(&self, id: &BoxId, signal: Signal) -> Result<(), Refusal> {
        self.inner.signal(id, signal)
    }
    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
        self.inner.inspect(id)
    }
    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        self.inner.usage(id)
    }
    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        self.inner.list()
    }
    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        self.inner.delete(id)
    }
}

/// AC-T7.4.1: configuring the Linux-image guest cannot hide the native Windows OCI device pool.
/// Real HCS attachment is separately validated; these capacity replies are explicit model inputs.
#[test]
fn native_oci_device_capacity_remains_visible_with_a_guest_configured() {
    let native = hex_api::Capacity {
        id: "model-native-pool".into(),
        total: hex_api::CapacityResources::default(),
        free: hex_api::CapacityResources::default(),
        devices: vec![hex_api::DeviceInfo {
            id: hex_api::DeviceId {
                kind: hex_api::DeviceKind::Gpu,
                key: 256,
            },
            model: "fixture-native".into(),
            size: 4096,
            holder: None,
        }],
    };
    let guest = hex_api::Capacity {
        id: "model-guest-pool".into(),
        devices: Vec::new(),
        ..native.clone()
    };
    let node = WithGuest {
        process: NativeReporting {
            inner: hex_api::sim::SimRuntime::new(),
            report: native.clone(),
        },
        oci: capacity_runtime(Some(guest), config()),
        route: |_runtime, _class| Ok(Side::Host),
    };
    assert_eq!(node.capacity(Class::Oci).unwrap(), Some(native));
}

/// AC-T7.4.1: the public seam recognizes the existing KVM/HVF/WHP implementations on every
/// supported host. Hardware capability/boot checks remain with the real backend; Intel Mac excluded.
#[test]
fn every_supported_host_reaches_its_existing_shards_backend() {
    for (os, arch, backend) in [
        ("linux", "x86_64", shards_api::Backend::Kvm),
        ("linux", "aarch64", shards_api::Backend::Kvm),
        ("macos", "aarch64", shards_api::Backend::HypervisorFramework),
        (
            "windows",
            "x86_64",
            shards_api::Backend::WindowsHypervisorPlatform,
        ),
        (
            "windows",
            "aarch64",
            shards_api::Backend::WindowsHypervisorPlatform,
        ),
    ] {
        assert_eq!(shards_api::backend_of(os, arch), Ok(backend), "{os}/{arch}");
    }
    assert!(matches!(
        shards_api::backend_of("macos", "x86_64"),
        Err(shards_api::Refusal::NoBackend { .. })
    ));
}
