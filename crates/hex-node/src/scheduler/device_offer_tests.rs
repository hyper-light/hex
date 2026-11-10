//! AC-T7.4.1: Service's real capacity/device bridge with pure Facts/Capacity inputs.
//! These are synthetic measurements in production types, never hardware/native evidence.

use super::*;
use hex_api::{Capacity, CapacityResources, DeviceId, DeviceInfo, DeviceKind as NativeKind};
use hex_sim::facts::{Cpu, Facts, Gpu};
use hex_sim::model::{Class as Route, Device, DeviceKind, GpuLoad, Os, bind};

const NAMESPACE: &str = "00112233445566778899aabbccddeeff";
const FOREIGN: &str = "ffeeddccbbaa99887766554433221100";

type Reports = Vec<(Route, Option<Capacity>)>;

fn inputs() -> (Facts, Reports) {
    let memory = hex_api::MIN_MEMORY;
    let facts = Facts {
        os: Os::Linux,
        cpu: Cpu {
            threads: 1,
            ..Cpu::default()
        },
        classes: Classes::NONE.with(Route::Oci),
        gpus: vec![
            Gpu {
                vendor: "nvidia".into(),
                model: "NVIDIA GeForce RTX 5060 Ti".into(),
                memory,
                pci: Some("12345678:01:02.3".into()),
                load: GpuLoad {
                    busy_per_mille: 1000 / 4,
                    memory_used: memory / 4,
                },
                ..Gpu::default()
            },
            Gpu {
                vendor: "amd".into(),
                model: "AMD Radeon 610M".into(),
                memory: memory * 2,
                pci: Some("00000000:01:02.3".into()),
                load: GpuLoad {
                    busy_per_mille: 1000 / 2,
                    memory_used: memory,
                },
                ..Gpu::default()
            },
        ],
        ..Facts::default()
    };
    let devices = vec![
        // Fixed PCI domain32:bus8:device5:function3 fixtures; same BDF in distinct domains.
        DeviceInfo {
            id: DeviceId {
                kind: NativeKind::Gpu,
                key: 0x1234_5678_0113,
            },
            model: "nvidia_geforce-rtx-5060-ti".into(),
            size: memory,
            holder: None,
        },
        DeviceInfo {
            id: DeviceId {
                kind: NativeKind::Gpu,
                key: 0x0113,
            },
            model: "AMD Radeon 610M".into(),
            size: memory * 2,
            holder: None,
        },
    ];
    (facts, vec![(Route::Oci, Some(report(devices)))])
}

fn report(devices: Vec<DeviceInfo>) -> Capacity {
    let room = CapacityResources {
        cpu_milli: u64::from(hex_api::MIN_CPU_MILLI),
        mem_bytes: hex_api::MIN_MEMORY,
        disk_bytes: 0,
    };
    Capacity {
        id: "fixture-native-pool".into(),
        total: room,
        free: room,
        devices,
    }
}

/// Mirror Service::open/refresh: Facts::node interns measured GPU models before offers runs.
fn service_cluster(facts: &Facts) -> Cluster {
    let mut cluster = Cluster::single_owner(Vec::new(), Vec::new());
    cluster.model("existing-model").unwrap();
    let local = facts.node(NodeId(0), &mut cluster).unwrap();
    cluster.nodes.push(local);
    cluster
}

fn offered(reports: &Reports, facts: &Facts, cluster: &mut Cluster) -> Vec<Device> {
    capacity::devices::offers(reports, facts, cluster, NAMESPACE).unwrap()
}

#[test]
fn exact_measured_pci_metadata_load_and_report_order_reach_offers() {
    let (mut facts, mut reports) = inputs();
    facts.gpus.push(Gpu {
        model: "Telemetry Only".into(),
        memory: hex_api::MIN_MEMORY,
        pci: Some("0000:02:00.0".into()),
        ..Gpu::default()
    });
    let devices = &mut reports[0].1.as_mut().unwrap().devices;
    devices.reverse(); // Native order, not telemetry order, is the scheduler's device slot order.
    let before_inputs = (facts.clone(), reports.clone());
    let mut cluster = service_cluster(&facts);
    let before_cluster = cluster.clone();
    let amd = cluster.model(&facts.gpus[1].model).unwrap();
    let nvidia = cluster.model(&facts.gpus[0].model).unwrap();
    let actual = offered(&reports, &facts, &mut cluster);
    assert_eq!(
        actual,
        vec![
            Device {
                kind: DeviceKind::Gpu,
                key: Some(0x0113),
                available: true,
                model: amd,
                size: facts.gpus[1].memory,
                load: facts.gpus[1].load
            },
            Device {
                kind: DeviceKind::Gpu,
                key: Some(0x1234_5678_0113),
                available: true,
                model: nvidia,
                size: facts.gpus[0].memory,
                load: facts.gpus[0].load
            },
        ]
    );
    assert_eq!(
        actual[0].memory_free(),
        facts.gpus[1].memory - facts.gpus[1].load.memory_used
    );
    assert_eq!((facts, reports), before_inputs);
    assert_eq!(cluster, before_cluster);
    eprintln!(
        "DEVICE_OFFER exact actual={actual:?}; inputs/cluster unchanged; telemetry-only GPU omitted"
    );
}

#[test]
fn complete_telemetry_without_backend_capability_offers_no_devices() {
    let (facts, _) = inputs();
    for reports in [
        Vec::new(),
        vec![(Route::Oci, None)],
        vec![(Route::Oci, Some(report(Vec::new())))],
    ] {
        let before_inputs = (facts.clone(), reports.clone());
        let mut cluster = service_cluster(&facts);
        let before_cluster = cluster.clone();
        assert!(
            !cluster.nodes[0].devices.is_empty(),
            "positive measured telemetry control"
        );
        assert!(offered(&reports, &facts, &mut cluster).is_empty());
        assert_eq!((facts.clone(), reports), before_inputs);
        assert_eq!(cluster, before_cluster);
    }
    eprintln!(
        "DEVICE_OFFER absent backend/unknown capacity/empty capability: 3 exact empty offers"
    );
}

#[derive(Debug, Clone, Copy)]
enum BadInput {
    MissingPci,
    MalformedPci,
    WrongDomain,
    MissingGpu,
    Model,
    Memory,
    Duplicate,
    PoolRoom,
}

#[test]
fn mismatched_identity_model_memory_and_capacity_refuse_without_mutating_inputs() {
    let cases = [
        BadInput::MissingPci,
        BadInput::MalformedPci,
        BadInput::WrongDomain,
        BadInput::MissingGpu,
        BadInput::Model,
        BadInput::Memory,
        BadInput::Duplicate,
        BadInput::PoolRoom,
    ];
    for case in cases {
        let (mut facts, mut reports) = inputs();
        let report = reports[0].1.as_mut().unwrap();
        match case {
            BadInput::MissingPci => facts.gpus[0].pci = None,
            BadInput::MalformedPci => facts.gpus[0].pci = Some("12345678:01:20.3".into()),
            BadInput::WrongDomain => facts.gpus[0].pci = Some("12345679:01:02.3".into()),
            BadInput::MissingGpu => {
                facts.gpus.remove(0);
            }
            BadInput::Model => report.devices[0].model = "other-model".into(),
            BadInput::Memory => report.devices[0].size += 1,
            BadInput::Duplicate => report.devices.push(report.devices[0].clone()),
            BadInput::PoolRoom => report.free.cpu_milli = report.total.cpu_milli + 1,
        }
        let before_inputs = (facts.clone(), reports.clone());
        let mut cluster = service_cluster(&facts);
        let before_cluster = cluster.clone();
        let refused =
            capacity::devices::offers(&reports, &facts, &mut cluster, NAMESPACE).unwrap_err();
        eprintln!(
            "DEVICE_OFFER refusal {case:?}: {refused:?}; facts={facts:?}; reports={reports:?}; cluster_before={before_cluster:?}; cluster_after={cluster:?}"
        );
        assert!(matches!(refused, Refusal::Invalid { .. }));
        assert_eq!((facts, reports), before_inputs);
        assert_eq!(cluster, before_cluster);
        let (repaired_facts, repaired_reports) = inputs();
        let mut repaired_cluster = service_cluster(&repaired_facts);
        assert_eq!(
            offered(&repaired_reports, &repaired_facts, &mut repaired_cluster).len(),
            2
        );
    }
    eprintln!(
        "DEVICE_OFFER 8 typed refusals/input+cluster snapshots preserved; 8 repaired positive controls"
    );
}

#[test]
fn foreign_holders_are_unavailable_but_current_namespace_survives_for_recovery() {
    let key = Key {
        owner: 0,
        epoch: 1,
        seq: 0,
    };
    let own = crate::placement::physical_id(NAMESPACE, key).unwrap();
    let foreign = crate::placement::physical_id(FOREIGN, key).unwrap();
    let near = BoxId::new(&format!("hex-place-{NAMESPACE}0-0-1-0")).unwrap();
    let cases = [
        (None, true),
        (Some(own), true),
        (Some(foreign), false),
        (Some(near), false),
        (Some(BoxId::new("caller-id").unwrap()), false),
    ];
    for (holder, available) in cases {
        let (facts, mut reports) = inputs();
        let report = reports[0].1.as_mut().unwrap();
        report.devices.truncate(1);
        report.devices[0].holder = holder.clone();
        let before_inputs = (facts.clone(), reports.clone());
        let mut cluster = service_cluster(&facts);
        let before_cluster = cluster.clone();
        let actual = offered(&reports, &facts, &mut cluster);
        assert_eq!(actual.len(), 1);
        assert_eq!(actual[0].available, available);
        assert_eq!(actual[0].key, Some(report_key(&reports)));
        let choice = bind(&actual, |index| actual[index].available, 1, &Ask::default());
        assert_eq!(
            choice.is_some(),
            available,
            "existing binder sees bridge availability"
        );
        assert_eq!((facts, reports), before_inputs);
        assert_eq!(cluster, before_cluster);
        eprintln!("DEVICE_OFFER holder={holder:?}; available={available}; bound={choice:?}");
    }
}

fn report_key(reports: &Reports) -> u64 {
    reports[0].1.as_ref().unwrap().devices[0].id.key
}

#[test]
fn initialized_native_oci_routes_devices_and_unrelated_class_reports_refuse() {
    for os in [Os::Linux, Os::Windows] {
        let (mut facts, reports) = inputs();
        facts.os = os;
        let mut cluster = service_cluster(&facts);
        let actual = offered(&reports, &facts, &mut cluster);
        assert_eq!(actual.len(), 2);
        assert!(actual.iter().all(|device| device.kind == DeviceKind::Gpu));
    }
    // These are misrouted native backend reports, not a claim that a valid VM/device request
    // may remain unsupported. ROOT owns the actual guest and macOS attachment paths.
    for route in [Route::Firecracker, Route::Shards] {
        let (facts, mut reports) = inputs();
        reports[0].0 = route;
        let before_inputs = (facts.clone(), reports.clone());
        let mut cluster = service_cluster(&facts);
        let before_cluster = cluster.clone();
        let refused =
            capacity::devices::offers(&reports, &facts, &mut cluster, NAMESPACE).unwrap_err();
        assert!(matches!(refused, Refusal::Invalid { .. }));
        assert_eq!((facts, reports), before_inputs);
        assert_eq!(cluster, before_cluster);
    }
    eprintln!(
        "DEVICE_OFFER Linux/Windows OCI inputs accepted; 2 misrouted native reports typed-refused; no hardware/platform completion claim"
    );
}

#[test]
fn late_refusal_does_not_publish_partial_model_interning() {
    let (mut facts, _) = inputs();
    facts.gpus.truncate(1);
    let block = DeviceInfo {
        id: DeviceId {
            kind: NativeKind::Block,
            key: (1u64 << 32) | 3,
        },
        model: "new-block-model".into(),
        size: hex_api::MIN_MEMORY,
        holder: None,
    };
    let (_, mut gpu_reports) = inputs();
    let mut gpu = gpu_reports[0].1.as_mut().unwrap().devices.remove(0);
    gpu.size += 1; // One later mismatching metadata field after a valid named native device.
    let reports = vec![(Route::Oci, Some(report(vec![block, gpu])))];
    let before_inputs = (facts.clone(), reports.clone());
    let mut cluster = service_cluster(&facts);
    let before_cluster = cluster.clone();
    let refused = capacity::devices::offers(&reports, &facts, &mut cluster, NAMESPACE).unwrap_err();
    let after_cluster = cluster.clone();
    eprintln!(
        "DEVICE_OFFER candidate late-refusal={refused:?}; facts={facts:?}; reports={reports:?}; cluster_before={before_cluster:?}; cluster_after={after_cluster:?}"
    );
    assert!(matches!(refused, Refusal::Invalid { .. }));
    assert_eq!((facts.clone(), reports.clone()), before_inputs);
    // Preserve the failed snapshots intact. A separate repaired working copy proves that the
    // same ordinary bridge still accepts the intended devices; it does not erase a candidate red.
    let mut repaired_reports = reports.clone();
    repaired_reports[0].1.as_mut().unwrap().devices[1].size = facts.gpus[0].memory;
    let mut repaired_cluster = before_cluster.clone();
    let repaired = offered(&repaired_reports, &facts, &mut repaired_cluster);
    assert_eq!(repaired.len(), 2);
    assert_eq!(repaired[0].kind, DeviceKind::Block);
    assert_eq!(repaired[1].kind, DeviceKind::Gpu);
    eprintln!(
        "DEVICE_OFFER candidate repair accepted={repaired:?}; repaired_models={:?}; failed snapshot retained",
        repaired_cluster.models
    );
    assert_eq!(
        after_cluster, before_cluster,
        "requested candidate invariant: an error publishes no partial model table; ROOT classifies/fixes separately after actual result"
    );
}
