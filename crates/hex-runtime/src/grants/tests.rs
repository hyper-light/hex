use std::net::IpAddr;

use hex_api::{Destination, Grant, Member, MemberKind, Ports, Proto, Refusal, Upstream};
use proptest::prelude::*;
use shards_api::image::{Keep, Narrowing};

use super::*;

fn nothing_kept() -> Narrowing {
    Narrowing {
        egress: Keep::Only(Vec::new()),
        mcp: Keep::Only(Vec::new()),
        dns: false,
    }
}

fn codex() -> Member {
    Member {
        kind: MemberKind::Harness,
        name: "codex".to_owned(),
    }
}

/// Joint plan step 15, T-T7.7.1: do program no grant at all; expect no publish and the image's every grant past the
/// microVM dropped, so its run has no network (`shards api-run`'s `network none`), never the image's own grants that
/// `Narrowing::default()` keeps.
#[test]
fn no_grant_is_programmed_as_no_network() {
    let made = program(&[]).unwrap();
    assert!(made.publish.is_empty());
    assert_eq!(made.egress, nothing_kept());
    assert_ne!(made.egress, Narrowing::default());
}

/// Joint plan step 15: do program a harness's grants as kiln would give them (a provider by name, kiln by address,
/// kiln on the VM's host, a publish); expect each one Shards 482c5d78 can't hold exactly refused, typed, naming the
/// capability it awaits, and a list of publishes alone programmed at the host address and port each names.
#[test]
fn each_grant_482c5d78_cant_hold_is_refused_naming_what_it_awaits() {
    let tcp = |port| Ports {
        proto: Proto::Tcp,
        first: port,
        last: port,
    };
    for (grant, needs) in [
        (
            Grant::Egress {
                member: codex(),
                to: Destination::Name("api.openai.com".to_owned()),
                ports: tcp(443),
            },
            Upstream::MemberGrantsByName,
        ),
        (
            Grant::Egress {
                member: Member {
                    kind: MemberKind::Agent,
                    name: "x".to_owned(),
                },
                to: Destination::Name("mcp.example.com".to_owned()),
                ports: tcp(443),
            },
            Upstream::MemberGrantsByName,
        ),
        (
            Grant::Egress {
                member: codex(),
                to: Destination::Address("10.0.0.5".parse().unwrap()),
                ports: tcp(7070),
            },
            Upstream::MemberGrantsByAddress,
        ),
        (
            Grant::Host {
                member: codex(),
                ports: tcp(7070),
            },
            Upstream::MemberHostService,
        ),
    ] {
        assert_eq!(
            program(std::slice::from_ref(&grant)),
            Err(Refusal::Awaits { needs }),
            "{grant:?}"
        );
    }
    let publishes = [
        Grant::Publish {
            member: codex(),
            proto: Proto::Tcp,
            port: 1455,
            host: "127.0.0.1".parse().unwrap(),
            host_port: None,
        },
        Grant::Publish {
            member: codex(),
            proto: Proto::Udp,
            port: 1455,
            host: "::1".parse().unwrap(),
            host_port: Some(18080),
        },
    ];
    let made = program(&publishes).unwrap();
    assert_eq!(made.egress, nothing_kept());
    assert_eq!(
        made.publish,
        vec![
            shards_api::image::Publish {
                guest_port: 1455,
                proto: shards_api::image::Proto::Tcp,
                host: Some("127.0.0.1".parse().unwrap()),
                host_port: 0,
            },
            shards_api::image::Publish {
                guest_port: 1455,
                proto: shards_api::image::Proto::Udp,
                host: Some("::1".parse().unwrap()),
                host_port: 18080,
            },
        ]
    );
    // A malformed grant is the check's refusal, before anything is programmed.
    assert!(matches!(
        program(&[Grant::Host {
            member: codex(),
            ports: tcp(0),
        }]),
        Err(Refusal::Invalid { what: "grants", .. })
    ));
}

/// The capability a grant 482c5d78 can't hold awaits; none for one it holds.
fn awaited(grant: &Grant) -> Option<Upstream> {
    match grant {
        Grant::Egress {
            to: Destination::Name(_),
            ..
        } => Some(Upstream::MemberGrantsByName),
        Grant::Egress {
            to: Destination::Address(_),
            ..
        } => Some(Upstream::MemberGrantsByAddress),
        Grant::Host { .. } => Some(Upstream::MemberHostService),
        Grant::Publish { .. } => None,
    }
}

fn any_publish() -> impl Strategy<Value = Grant> {
    (
        any_member(),
        any::<bool>(),
        1u16..=u16::MAX,
        prop_oneof![Just("127.0.0.1"), Just("::1"), Just("10.0.0.2")],
        proptest::option::of(1u16..=u16::MAX),
    )
        .prop_map(|(member, udp, port, host, host_port)| Grant::Publish {
            member,
            proto: if udp { Proto::Udp } else { Proto::Tcp },
            port,
            host: host.parse().unwrap(),
            host_port,
        })
}

fn any_member() -> impl Strategy<Value = Member> {
    (any::<bool>(), "[a-z][a-z0-9._-]{0,6}").prop_map(|(agent, name)| Member {
        kind: if agent {
            MemberKind::Agent
        } else {
            MemberKind::Harness
        },
        name,
    })
}

fn any_grant() -> impl Strategy<Value = Grant> {
    let ports = (any::<bool>(), 1u16..=u16::MAX, 0u16..64).prop_map(|(udp, first, span)| Ports {
        proto: if udp { Proto::Udp } else { Proto::Tcp },
        first,
        last: first.saturating_add(span),
    });
    let to = prop_oneof![
        "[a-z]([a-z0-9-]{0,6}[a-z0-9])?\\.[a-z]{2,6}".prop_map(Destination::Name),
        any::<[u8; 4]>().prop_map(|a| Destination::Address(IpAddr::from(a))),
    ];
    // Publishes weigh twice, so lists of publishes alone, which program, are common.
    prop_oneof![
        (any_member(), to, ports.clone()).prop_map(|(member, to, ports)| Grant::Egress {
            member,
            to,
            ports
        }),
        (any_member(), ports).prop_map(|(member, ports)| Grant::Host { member, ports }),
        any_publish(),
        any_publish(),
    ]
    .prop_filter("a well-formed grant", |g| {
        hex_api::check_grants(std::slice::from_ref(g)).is_ok()
    })
}

proptest! {
    /// AC-T7.7.2 (joint plan step 15): do program any well-formed list of grants of every way; expect either a typed
    /// `Awaits` naming the capability of its first grant 482c5d78 can't hold exactly, or a program that keeps none of
    /// the image's grants past the microVM and publishes exactly the list's publish grants, in order, each at the host
    /// address and port it names. So the flows a program allows that no grant names are exactly 0.
    #[test]
    fn a_program_allows_no_flow_its_grants_do_not_name(
        list in proptest::collection::vec(any_grant(), 0..6)
            .prop_filter("each flow named once", |l| hex_api::check_grants(l).is_ok())
    ) {
        match program(&list) {
            Ok(made) => {
                prop_assert!(list.iter().all(|g| awaited(g).is_none()));
                prop_assert_eq!(&made.egress, &nothing_kept());
                prop_assert_eq!(made.publish.len(), list.len());
                for (published, grant) in made.publish.iter().zip(&list) {
                    let Grant::Publish { proto, port, host, host_port, .. } = grant else {
                        return Err(TestCaseError::fail("a published port no publish grant names"));
                    };
                    prop_assert_eq!(published.guest_port, *port);
                    prop_assert_eq!(published.host, Some(*host));
                    prop_assert_eq!(published.host_port, host_port.unwrap_or(0));
                    prop_assert_eq!(
                        published.proto,
                        match proto {
                            Proto::Tcp => shards_api::image::Proto::Tcp,
                            Proto::Udp => shards_api::image::Proto::Udp,
                        }
                    );
                }
            }
            Err(Refusal::Awaits { needs }) => {
                prop_assert_eq!(Some(needs), list.iter().find_map(awaited));
            }
            Err(other) => return Err(TestCaseError::fail(format!("{other:?}"))),
        }
    }
}
