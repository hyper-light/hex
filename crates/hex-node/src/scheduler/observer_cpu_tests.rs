//! One original real-Service refresh control. Synthetic runtimes only; no GPU/VMM/provider.
use super::*;
use hex_api::{
    BackingKey, BackingOffer, BackingPhase, Bounds, Capacity, CapacityResources, ClassSpec,
    DeviceId, DeviceKind, Execution, GpuEligibility, ImageArch, ImageOs, ImagePlatform,
    LifecycleCause, PhysicalReport, Proc, Program, RouteKey, RouteOffer, RuntimeCost, Signal,
    Stdio, Usage,
};
use hex_sim::model::{Ask, Demand, Kind, Os, Resources};
use hex_sim::peer::Budget;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Endpoint {
    sim: Arc<hex_api::sim::SimRuntime>,
    observer: bool,
    failed: Arc<AtomicBool>,
    calls: AtomicUsize,
    key: BackingKey,
    capacity: Capacity,
}

impl Endpoint {
    fn observation(&self) -> Result<(), Refusal> {
        if self.observer && self.failed.load(Ordering::SeqCst) {
            self.calls.fetch_add(1, Ordering::SeqCst);
            return Err(Refusal::Backing {
                key: self.key,
                cause: LifecycleCause::Uncertain,
            });
        }
        Ok(())
    }
}

impl Runtime for Endpoint {
    fn classes(&self) -> Vec<hex_api::Class> {
        vec![if self.observer {
            hex_api::Class::Firecracker
        } else {
            hex_api::Class::Process
        }]
    }
    fn capacity(&self, _: hex_api::Class) -> Result<Option<Capacity>, Refusal> {
        self.observation()?;
        Ok(Some(self.capacity.try_clone()?))
    }
    fn physical_report(&self) -> Result<PhysicalReport, Refusal> {
        self.observation()?;
        self.sim.physical_report()
    }
    fn create(&self, spec: BoxSpec) -> Result<hex_api::BoxInfo, Refusal> {
        self.sim.create(spec)
    }
    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        self.sim.start(id)
    }
    fn exec(&self, id: &BoxId, p: Program, s: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        self.sim.exec(id, p, s)
    }
    fn run(&self, b: BoxSpec, p: Program, s: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        self.sim.run(b, p, s)
    }
    fn signal(&self, id: &BoxId, s: Signal) -> Result<(), Refusal> {
        self.sim.signal(id, s)
    }
    fn inspect(&self, id: &BoxId) -> Result<hex_api::BoxInfo, Refusal> {
        self.sim.inspect(id)
    }
    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        self.sim.usage(id)
    }
    fn list(&self) -> Result<Vec<hex_api::BoxInfo>, Refusal> {
        self.sim.list()
    }
    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        self.sim.delete(id)
    }
}

/// AC-T7.4.1: one failed configured GPU observation cannot fabricate fresh GPU facts or disable
/// an independently valid CPU plane. This is SOURCE FIRST; no production isolation remedy.
#[test]
fn failed_firecracker_observation_keeps_real_service_cpu_progress_without_gpu_facts() {
    let dir = tempfile::tempdir().unwrap();
    let room = CapacityResources {
        cpu_milli: 1000,
        mem_bytes: 512 << 20,
        disk_bytes: 0,
    };
    let image = ImagePlatform {
        os: ImageOs::Linux,
        arch: match std::env::consts::ARCH {
            "x86_64" => ImageArch::Amd64,
            "aarch64" => ImageArch::Arm64,
            other => panic!("unsupported fixture architecture {other}"),
        },
    };
    let key = BackingKey {
        pool: [81; 32],
        generation: [82; 16],
    };
    let mut native = Capacity {
        id: "one native physical CPU plane".into(),
        total: room,
        free: room,
        devices: Vec::new(),
        cost: None,
        routes: Vec::new(),
        device_routes: Vec::new(),
        backings: Vec::new(),
    };
    hex_runtime::devices::native_routes(&mut native, hex_api::Class::Process, image).unwrap();
    let cost = RuntimeCost {
        id: [83; 32],
        floor: CapacityResources {
            cpu_milli: 1000,
            mem_bytes: 128 << 20,
            disk_bytes: 0,
        },
        ceiling: CapacityResources {
            cpu_milli: 1000,
            mem_bytes: 128 << 20,
            disk_bytes: 0,
        },
        quantum: CapacityResources {
            cpu_milli: 1000,
            mem_bytes: 1,
            disk_bytes: 1,
        },
        guest_overhead: CapacityResources::default(),
        overhead: CapacityResources::default(),
    };
    let observer_report = Capacity {
        id: native.id.clone(),
        total: room,
        free: room,
        devices: Vec::new(),
        cost: None,
        routes: vec![RouteOffer {
            key: RouteKey {
                class: hex_api::Class::Firecracker,
                image,
                execution: Execution::Firecracker,
                gpu: GpuEligibility::Required,
            },
            cost: Some(cost),
            backing: Some(key),
        }],
        device_routes: Vec::new(),
        backings: vec![BackingOffer {
            key,
            cost,
            phase: BackingPhase::Prepared,
            // Configuration authority only. No model, memory, load or measured DeviceInfo.
            devices: vec![DeviceId {
                kind: DeviceKind::Gpu,
                key: 7,
            }],
        }],
    };
    observer_report.check().unwrap();
    let failed = Arc::new(AtomicBool::new(false));
    let sim = Arc::new(hex_api::sim::SimRuntime::for_backend(
        hex_api::Class::Process,
    ));
    let native = Arc::new(Endpoint {
        sim: sim.clone(),
        observer: false,
        failed: failed.clone(),
        calls: AtomicUsize::new(0),
        key,
        capacity: native,
    });
    let observer = Arc::new(Endpoint {
        sim: sim.clone(),
        observer: true,
        failed: failed.clone(),
        calls: AtomicUsize::new(0),
        key,
        capacity: observer_report,
    });
    let runtimes = Runtimes::new([
        (hex_sim::model::Class::Oci, native as Arc<dyn Runtime>),
        (
            hex_sim::model::Class::Firecracker,
            observer.clone() as Arc<dyn Runtime>,
        ),
    ])
    .unwrap();
    let mut local = NodeSpec::new(
        NodeId(0),
        Resources {
            cpu_milli: room.cpu_milli,
            mem_bytes: room.mem_bytes,
            disk_bytes: 0,
            gpus: 0,
        },
    );
    local.classes = Classes::NONE
        .with(hex_sim::model::Class::Oci)
        .with(hex_sim::model::Class::Firecracker);
    let mut remote = NodeSpec::new(NodeId(1), Resources::default());
    remote.classes = Classes::NONE;
    let cluster = Cluster::single_owner(vec![local, remote], Vec::new());
    let (send, _receive) = channel();
    let service = super::tests::advertise_service_with_budget(
        NodeId(0),
        dir.path(),
        cluster,
        runtimes,
        send,
        Budget { owned: 4, ran: 4 },
    );
    service
        .refresh_snapshot()
        .expect("healthy configured native CPU plane must normalize first");
    failed.store(true, Ordering::SeqCst);
    let refreshed = service.refresh_snapshot();
    let facts = runtime_facts(&service.facts_dir, service.initialized, &service.runtimes);
    eprintln!(
        "observer CPU isolation original: refresh={:?}; facts={:?}; callbacks={}; native_effect=false",
        refreshed.as_ref().map(|_| ()),
        facts.as_ref().map(|(facts, authority)| (
            facts.gpus.len(),
            facts.no_gpus.is_some(),
            authority.as_deref()
        )),
        observer.calls.load(Ordering::SeqCst)
    );
    assert!(
        observer.calls.load(Ordering::SeqCst) > 0,
        "failure must reach the real configured endpoint"
    );
    assert!(
        refreshed.is_ok(),
        "failed GPU observer must not block a valid native CPU refresh"
    );
    let (facts, authority) = facts.unwrap();
    assert!(
        facts.gpus.is_empty(),
        "missing observations cannot become fabricated GPU facts"
    );
    assert!(
        facts.no_gpus.is_some(),
        "unavailable GPU observation must remain diagnostic"
    );
    assert_eq!(authority.as_deref(), Some("simulator:cpu"));
    let demand = Demand {
        class: hex_sim::model::Class::Oci,
        kind: Kind::Plain,
        need: Resources {
            cpu_milli: u64::from(hex_api::MIN_CPU_MILLI),
            mem_bytes: hex_api::MIN_MEMORY,
            ..Resources::default()
        },
        ask: Ask {
            runtime_class: Some(hex_api::Class::Process),
            os: Some(Os::Linux),
            ..Ask::default()
        },
    };
    let (workload, _) = service
        .driver
        .submit(
            demand,
            Payload {
                spec: BoxSpec {
                    id: BoxId::new("cpu-during-observer-loss").unwrap(),
                    class: ClassSpec::Process,
                    owner: None,
                    devices: Vec::new(),
                    route: None,
                    allocation: None,
                    bounds: Bounds {
                        cpu_max_milli: Some(hex_api::MIN_CPU_MILLI),
                        memory_max: Some(hex_api::MIN_MEMORY),
                        ..Bounds::default()
                    },
                },
                start: false,
            },
        )
        .unwrap();
    let status = service.driver.status(workload).unwrap().unwrap();
    assert!(
        status.created,
        "CPU demand must reach physical create, not remain tracked"
    );
    assert_eq!(
        sim.inspect(&status.physical).unwrap().class,
        hex_api::Class::Process
    );
    service.driver.delete(workload).unwrap();
    assert!(sim.list().unwrap().is_empty());
}
