use super::*;
use hex_api::{Bounds, ClassSpec};
use hex_core::record::Phase;

fn catalog() -> Catalog {
    Catalog::parse(b"hex-devices-1\ngpu\t256\tfixture-a\t4096\tcdi\t/private/fixture.json\tfixture.test/gpu=a\ngpu\t512\tfixture-b\t8192\thcs\tPCIROOT(0)#PCI(0200)\t\n").unwrap()
}

fn spec(name: &str, key: u64) -> BoxSpec {
    BoxSpec {
        id: BoxId::new(name).unwrap(),
        bounds: Bounds::default(),
        class: ClassSpec::Oci(Box::new(hex_api::OciSpec {
            rootfs: hex_api::Rootfs::Dir("/fixture/root".into()),
            init: hex_api::Program::of(&["true"]),
            mounts: Vec::new(),
            hostname: None,
            network: hex_api::Network::Private,
            hosts: Vec::new(),
        })),
        owner: None,
        devices: vec![DeviceId {
            kind: DeviceKind::Gpu,
            key,
        }],
    }
}

/// AC-T7.4.1: a request selects the configured physical resource, never its neighbor's edit plan.
#[test]
fn selected_identity_returns_only_its_native_binding() {
    let catalog = catalog();
    let selected = catalog.prepare(&spec("one", 512), &Records::new()).unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].info.id.key, 512);
    assert!(
        matches!(&selected[0].binding, Binding::Hcs { location } if location == "PCIROOT(0)#PCI(0200)")
    );
    let selected = catalog.prepare(&spec("two", 256), &Records::new()).unwrap();
    assert!(
        matches!(&selected[0].binding, Binding::Cdi { file, qualified } if file == "/private/fixture.json" && qualified == "fixture.test/gpu=a")
    );
    assert!(
        catalog
            .prepare(&spec("unknown", 768), &Records::new())
            .is_err()
    );
}

/// AC-T7.4.1/T-T7.1.2: pending creation and deletion retain exactly the same exclusive resource.
#[test]
fn every_recorded_phase_retains_the_device_until_physical_removal() {
    let catalog = catalog();
    let held = spec("held", 256);
    for phase in [Phase::Creating, Phase::Ready, Phase::Deleting] {
        let record = BoxRecord::new(held.clone(), phase, Vec::new());
        let bytes = record.encode().unwrap();
        let records = BTreeMap::from([(held.id.clone(), Ok(record))]);
        assert_eq!(
            catalog.holder(held.devices[0], &records).unwrap(),
            Some(&held.id)
        );
        assert!(catalog.prepare(&spec("next", 256), &records).is_err());
        assert!(catalog.prepare(&spec("neighbor", 512), &records).is_ok());
        assert_eq!(
            records
                .get(&held.id)
                .unwrap()
                .as_ref()
                .unwrap()
                .encode()
                .unwrap(),
            bytes
        );
    }
    assert!(
        catalog
            .prepare(&spec("after-removal", 256), &Records::new())
            .is_ok()
    );
}

/// AC-T7.4.1: recovery uncertainty never becomes invented free capacity, and reads change no rows.
#[test]
fn malformed_records_and_multiple_holders_refuse_without_repairing_them() {
    let catalog = catalog();
    let first = spec("one", 256);
    let second = spec("two", 256);
    let records = BTreeMap::from([
        (
            first.id.clone(),
            Ok(BoxRecord::new(first, Phase::Ready, Vec::new())),
        ),
        (
            second.id.clone(),
            Ok(BoxRecord::new(second, Phase::Deleting, Vec::new())),
        ),
    ]);
    assert!(matches!(
        catalog.prepare(&spec("next", 256), &records),
        Err(Refusal::Broken(_))
    ));
    assert_eq!(records.len(), 2);
    let broken = BTreeMap::from([(
        BoxId::new("damaged").unwrap(),
        Err("private corrupt content".into()),
    )]);
    let error = catalog.prepare(&spec("next", 512), &broken).err().unwrap();
    assert!(matches!(error, Refusal::Broken(_)));
    assert!(!error.to_string().contains("private corrupt content"));
    assert_eq!(broken.len(), 1);
}

/// AC-T7.4.1: malformed administrator data returns only a field/cause, never private bindings.
#[test]
fn invalid_catalogs_refuse_without_echoing_private_configuration() {
    for bytes in [
        b"".as_slice(),
        b"hex-devices-2\n",
        b"hex-devices-1\ngpu\tbad\tprivate-model\t4096\tcdi\t/private/secret\tsecret-name\n",
        b"hex-devices-1\ngpu\t256\tprivate-model\tbad\tcdi\t/private/secret\tsecret-name\n",
        b"hex-devices-1\nnet\t0\tprivate-model\t0\tcdi\t/private/secret\tsecret-name\n",
        b"hex-devices-1\ngpu\t256\tprivate-model\t0\thcs\tprivate-location\tnot-empty\n",
        b"hex-devices-1\ngpu\t256\tprivate-model\t0\tcdi\trelative-secret\tsecret-name\n",
        b"hex-devices-1\ngpu\t256\tprivate-model\t0\tcdi\t/private/secret\tsecret-name\textra\n",
    ] {
        let error = Catalog::parse(bytes).err().unwrap();
        let rendered = format!("{error:?} {error}");
        for private in [
            "private-model",
            "private-location",
            "/private/secret",
            "secret-name",
            "relative-secret",
        ] {
            assert!(!rendered.contains(private), "{rendered}");
        }
    }
    let duplicate = b"hex-devices-1\ngpu\t256\ta\t1\tcdi\t/fixture\tfixture.test/gpu=a\ngpu\t256\tb\t2\tcdi\t/other\tfixture.test/gpu=b\n";
    assert!(Catalog::parse(duplicate).is_err());
}

/// AC-T7.4.1: absence gives no grants; an oversized/growing input cannot be silently truncated.
#[test]
fn absent_and_oversized_catalogs_are_distinguished() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("devices");
    assert!(Catalog::open(&path).unwrap().entries().is_empty());
    std::fs::write(&path, []).unwrap();
    assert!(Catalog::open(&path).is_err());
    let bytes = vec![b'x'; hex_core::bundle::MAX_CONFIG_BYTES + 1];
    std::fs::write(&path, bytes).unwrap();
    assert!(Catalog::open(&path).is_err());
    std::fs::write(&path, b"hex-devices-1\n").unwrap();
    assert!(Catalog::open(&path).unwrap().entries().is_empty());
}
