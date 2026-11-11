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
        cost: None,
    }
}

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
fn unread_native_counters_never_become_a_fresh_idle_free_offer() {
    let (mut facts, reports) = inputs();
    let inventory = facts
        .gpus
        .iter()
        .map(|gpu| hex_gpu::Gpu {
            vendor: hex_gpu::Vendor::Nvidia,
            model: gpu.model.clone(),
            memory_total_bytes: gpu.memory,
            identity: hex_gpu::Identity::Pci(
                hex_gpu::PciAddress::parse(gpu.pci.as_deref().unwrap()).unwrap(),
            ),
            driver_version: "pure-fixture".into(),
        })
        .collect::<Vec<_>>();
    let native = [
        hex_gpu::Load {
            busy_percent: Err(hex_gpu::Unread::Unsupported),
            memory_used_bytes: Ok(hex_api::MIN_MEMORY / 4),
        },
        hex_gpu::Load {
            busy_percent: Ok(50),
            memory_used_bytes: Err(hex_gpu::Unread::NoSample),
        },
    ];
    facts.gpus = crate::facts::gpu_facts(&inventory, &native);
    assert_eq!(facts.gpus.len(), 2);
    assert_eq!(
        facts.gpus[0].key,
        Some(reports[0].1.as_ref().unwrap().devices[0].id.key)
    );
    assert_eq!(facts.gpus[0].load.memory_used, hex_api::MIN_MEMORY / 4);
    assert_eq!(facts.gpus[1].load.busy_per_mille, 500);
    let mut cluster = service_cluster(&facts);
    let offers = offered(&reports, &facts, &mut cluster);
    let selected = bind(
        &offers,
        |index| offers.get(index).is_some_and(|device| device.available),
        1,
        &hex_sim::model::Ask::default(),
    );
    eprintln!("pure unread Facts/offers selected={selected:?}; hardware_effect=false");
    assert!(
        selected.is_none(),
        "Unread native counters must not become a fresh known-idle/free offer"
    );
}
