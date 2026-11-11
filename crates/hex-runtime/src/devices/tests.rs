use super::*;
use hex_api::{Bounds, ClassSpec};
use hex_core::record::Phase;

fn catalog() -> Catalog {
    Catalog::parse(b"hex-devices-1\ngpu\t256\tfixture-a\t4096\tcdi\t/private/fixture.json\tfixture.test/gpu=a\ngpu\t512\tfixture-b\t8192\thcs\tPCIROOT(0)#PCI(0200)\t\n").unwrap()
}

fn spec(name: &str, key: u64) -> BoxSpec {
    BoxSpec {
        route: None,
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
        allocation: None,
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
        matches!(selected[0].binding(Route::Hcs), Some(Binding::Hcs { location }) if location == "PCIROOT(0)#PCI(0200)")
    );
    let selected = catalog.prepare(&spec("two", 256), &Records::new()).unwrap();
    assert!(
        matches!(selected[0].binding(Route::Cdi), Some(Binding::Cdi { file, qualified }) if file == "/private/fixture.json" && qualified == "fixture.test/gpu=a")
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

/// AC-T7.4.1: OCI and microVM routes for one physical GPU share one slot and retained holder.
#[test]
fn explicit_consistent_backend_aliases_keep_one_physical_entry_and_holder() {
    let catalog = Catalog::parse(b"hex-devices-1\ngpu\t512\tother\t8192\thcs\tPCIROOT(0)#PCI(0200)\t\ngpu\t256\tsame\t4096\tshards-gpu\tpci:0000:01:00.0\t\ngpu\t256\tsame\t4096\tcdi\t/private/gpu.cdi\tfixture.test/gpu=a\ngpu\t256\tsame\t4096\tvfio\t0000:01:00.0\t7\n").unwrap();
    assert_eq!(catalog.entries().len(), 2);
    assert_eq!(catalog.entries()[0].info.id.key, 512);
    assert_eq!(catalog.entries()[1].info.id.key, 256);
    for route in [Route::Cdi, Route::ShardsGpu, Route::Vfio] {
        let entries = catalog.entries_for(route).collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].info.id.key, 256);
        assert_eq!(entries[0].bindings().len(), 3);
    }
    assert_eq!(catalog.entries_for(Route::Hcs).count(), 1);
    let held = spec("retained-alias", 256);
    let records = BTreeMap::from([(
        held.id.clone(),
        Ok(BoxRecord::new(held.clone(), Phase::Creating, Vec::new())),
    )]);
    assert_eq!(
        catalog.holder(held.devices[0], &records).unwrap(),
        Some(&held.id)
    );
    assert!(
        catalog
            .prepare(&spec("another-backend", 256), &records)
            .is_err()
    );
    assert!(catalog.prepare(&spec("independent", 512), &records).is_ok());
    assert_eq!(catalog.conflict_ids(&held.devices).unwrap(), held.devices);
}

/// AC-T7.4.1: aliases cannot change physical metadata or make one backend choose between recipes.
#[test]
fn conflicting_alias_metadata_and_duplicate_routes_refuse() {
    let first = "hex-devices-1\ngpu\t256\tsame\t4096\tcdi\t/private/a\tfixture.test/gpu=a\n";
    for alias in [
        "gpu\t256\tchanged\t4096\tshards-gpu\tpci:0000:01:00.0\t\n",
        "gpu\t256\tsame\t8192\tshards-gpu\tpci:0000:01:00.0\t\n",
        "gpu\t256\tsame\t4096\tcdi\t/private/a\tfixture.test/gpu=a\n",
        "gpu\t256\tsame\t4096\tcdi\t/private/other\tfixture.test/gpu=b\n",
    ] {
        assert!(Catalog::parse(format!("{first}{alias}").as_bytes()).is_err());
    }
}

/// AC-T7.4.1: a current-boot LUID declaration does not rename the stable identity or prove a join.
#[test]
fn luid_route_retains_stable_device_identity_without_claiming_native_correspondence() {
    let catalog = Catalog::parse(
        b"hex-devices-1\ngpu\t256\tsame\t4096\tshards-gpu\tluid:123456789abcdef0\t\n",
    )
    .unwrap();
    let entry = catalog.entries().first().unwrap();
    assert_eq!(entry.info.id.key, 256);
    assert!(matches!(
        entry.binding(Route::ShardsGpu),
        Some(Binding::ShardsGpu {
            physical: shards_api::PhysicalGpu::Luid(0x123456789abcdef0)
        })
    ));
}

/// AC-T7.4.1: a catalog label cannot grant trusted guest render authority or bypass host CDI.
#[test]
fn guest_recipe_is_separate_and_requires_exact_boot_owner_and_full_set() {
    let catalog = Catalog::parse(b"hex-devices-1\ngpu\t256\tshards-renderer\t0\tguest-gpu\t/private/guest.cdi\thex.shards/gpu=256\n").unwrap();
    assert_eq!(catalog.entries_for(Route::GuestGpu).count(), 1);
    assert_eq!(catalog.entries_for(Route::Cdi).count(), 0);
    let selected = spec("dedicated", 256);
    assert!(catalog.cdi(&selected.devices).is_err());
    let foreign = crate::guest::gpu::Authority {
        owner: BoxId::new("foreign").unwrap(),
        devices: selected.devices.clone(),
    };
    assert!(catalog.guest_cdi(&selected, &foreign).is_err());
    let subset = crate::guest::gpu::Authority {
        owner: selected.id.clone(),
        devices: vec![
            selected.devices[0],
            DeviceId {
                kind: DeviceKind::Gpu,
                key: 512,
            },
        ],
    };
    assert!(catalog.guest_cdi(&selected, &subset).is_err());
    for row in [
        "gpu\t256\tshards-renderer\t0\tguest-gpu\t/private/g\thex.shards/gpu=512",
        "gpu\t256\tshards-renderer\t4096\tguest-gpu\t/private/g\thex.shards/gpu=256",
        "gpu\t256\tinvented-measured-model\t0\tguest-gpu\t/private/g\thex.shards/gpu=256",
    ] {
        assert!(Catalog::parse(format!("hex-devices-1\n{row}\n").as_bytes()).is_err());
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("guest.cdi");
    std::fs::write(&path, br#"{"cdiVersion":"1.1.0","kind":"hex.shards/gpu","devices":[{"name":"256","containerEdits":{"deviceNodes":[{"path":"/dev/dri/renderD128","type":"c","major":226,"minor":128,"permissions":"rw"}]}}]}"#).unwrap();
    let catalog = Catalog::parse(
        format!(
            "hex-devices-1\ngpu\t256\tshards-renderer\t0\tguest-gpu\t{}\thex.shards/gpu=256\n",
            path.display()
        )
        .as_bytes(),
    )
    .unwrap();
    let authority = crate::guest::gpu::Authority {
        owner: selected.id.clone(),
        devices: selected.devices.clone(),
    };
    let plan = catalog.guest_cdi(&selected, &authority).unwrap();
    assert_eq!(plan.edits().unwrap().device_nodes.len(), 1);
    assert!(catalog.cdi(&selected.devices).is_err());
}
