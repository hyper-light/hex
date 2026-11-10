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
            Arc::new(shards_api::sim::SimGuests::new()),
            super::tests::config(),
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
