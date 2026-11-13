use std::fmt::Write as _;

use super::*;
use crate::{MemberKind, Ports};

fn member(kind: MemberKind, name: &str) -> Member {
    Member {
        kind,
        name: name.to_owned(),
    }
}

fn claude() -> Member {
    member(MemberKind::Harness, "claude")
}

fn egress(member: Member, name: &str, proto: Proto) -> Grant {
    Grant::Egress {
        member,
        to: Destination::Name(name.to_owned()),
        ports: Ports {
            proto,
            first: 443,
            last: 443,
        },
    }
}

/// The placeholder kiln's broker would give here for the credential `reference` names (Shards' D242): its visible
/// marker, then hex, one for each reference, so that grants of one credential share it and two credentials never do.
fn placeholder(reference: &str) -> String {
    reference
        .bytes()
        .fold("kiln-placeholder-".to_owned(), |mut placeholder, b| {
            write!(placeholder, "{b:02x}").unwrap();
            placeholder
        })
}

fn credential(member: Member, reference: &str, toward: &[&str]) -> CredentialGrant {
    CredentialGrant {
        member,
        reference: reference.to_owned(),
        toward: toward.iter().map(|h| (*h).to_owned()).collect(),
        placeholder: placeholder(reference),
    }
}

/// Claude Code's harness as kiln would grant it (joint plan step 33's example): its providers by name, by TCP, and
/// its account's credential toward both.
fn harness() -> (Vec<Grant>, Vec<CredentialGrant>) {
    (
        vec![
            egress(claude(), "api.anthropic.com", Proto::Tcp),
            egress(claude(), "platform.claude.com", Proto::Tcp),
        ],
        vec![credential(
            claude(),
            "anthropic/ada",
            &["api.anthropic.com", "platform.claude.com"],
        )],
    )
}

fn refused(credentials: &[CredentialGrant], grants: &[Grant]) -> String {
    match check_credentials(credentials, grants) {
        Err(Refusal::Invalid {
            what: "credentials",
            why,
        }) => why,
        other => panic!("not refused as invalid credentials: {other:?}"),
    }
}

/// AC-T7.7.2, joint plan step 33 (C2): do check a harness's credential toward the providers its own egress grants
/// reach by name, by TCP, and a box granted none. Expect both taken.
#[test]
fn w33_a_credential_toward_its_members_own_egress_hosts_is_taken() {
    let (grants, credentials) = harness();
    assert_eq!(check_credentials(&credentials, &grants), Ok(()));
    assert_eq!(check_credentials(&[], &grants), Ok(()));
    assert_eq!(check_credentials(&[], &[]), Ok(()));
}

/// AC-T7.7.2 (C2): do check a credential with each field malformed in turn. Expect each refused as invalid
/// credentials naming its field: a member's name no Agentfile takes, a reference no store could hold (empty, past
/// REFERENCE_MAX, a space), no host, a host named twice.
#[test]
fn w33_each_malformed_field_is_refused_by_name() {
    let (grants, _) = harness();
    let at_most = "r".repeat(REFERENCE_MAX);
    assert_eq!(
        check_credentials(
            &[credential(claude(), &at_most, &["api.anthropic.com"])],
            &grants
        ),
        Ok(())
    );
    let past = "r".repeat(REFERENCE_MAX + 1);
    let cases = [
        (
            credential(
                member(MemberKind::Harness, "Claude"),
                "anthropic/ada",
                &["api.anthropic.com"],
            ),
            "credentials[0].member.name",
        ),
        (
            credential(claude(), "", &["api.anthropic.com"]),
            "credentials[0].reference",
        ),
        (
            credential(claude(), &past, &["api.anthropic.com"]),
            "credentials[0].reference",
        ),
        (
            credential(claude(), "anthropic ada", &["api.anthropic.com"]),
            "credentials[0].reference",
        ),
        (credential(claude(), "anthropic/ada", &[]), "toward no host"),
        (
            credential(
                claude(),
                "anthropic/ada",
                &["api.anthropic.com", "api.anthropic.com"],
            ),
            "credentials[0].toward names \"api.anthropic.com\" twice",
        ),
    ];
    for (grant, says) in cases {
        let why = refused(&[grant], &grants);
        assert!(why.contains(says), "{says}: {why}");
    }
}

/// AC-T7.7.2, Shards' D242 (C2): a credential grant grants no egress. Do grant a credential toward a host its member
/// reaches by no egress grant, by UDP alone, by address alone, by another member's grant alone, or written in another
/// case than its grant's. Expect each refused, naming the host its member's egress doesn't reach by name, by TCP.
#[test]
fn w33_a_credential_toward_a_host_its_members_egress_does_not_reach_by_name_is_refused() {
    let agent = member(MemberKind::Agent, "helper");
    let unreached: [(Vec<Grant>, &str); 5] = [
        (Vec::new(), "api.anthropic.com"),
        (
            vec![egress(claude(), "api.anthropic.com", Proto::Udp)],
            "api.anthropic.com",
        ),
        (
            vec![Grant::Egress {
                member: claude(),
                to: Destination::Address([203, 0, 113, 7].into()),
                ports: Ports {
                    proto: Proto::Tcp,
                    first: 443,
                    last: 443,
                },
            }],
            "203.0.113.7",
        ),
        (
            vec![egress(agent, "api.anthropic.com", Proto::Tcp)],
            "api.anthropic.com",
        ),
        (
            vec![egress(claude(), "api.anthropic.com", Proto::Tcp)],
            "API.anthropic.com",
        ),
    ];
    for (grants, host) in unreached {
        let why = refused(&[credential(claude(), "anthropic/ada", &[host])], &grants);
        assert!(
            why.contains(&format!(
                "{host:?} is no host an egress grant of harness claude reaches"
            )),
            "{host} over {grants:?}: {why}"
        );
    }
}

/// AC-T7.7.2, Shards' D242 (C2): do grant one member a credential twice. Expect it refused: a member's credential is
/// one grant's.
#[test]
fn w33_a_credential_granted_twice_is_refused() {
    let (grants, _) = harness();
    let twice = [
        credential(claude(), "anthropic/ada", &["api.anthropic.com"]),
        credential(claude(), "anthropic/ada", &["platform.claude.com"]),
    ];
    let why = refused(&twice, &grants);
    assert!(
        why.contains("credentials[0] and credentials[1] both grant harness claude anthropic/ada"),
        "{why}"
    );
}

/// AC-T7.7.2, Shards' D242's step 3 (C2): Shards' injector fills a value in only where a grant's own placeholder stands,
/// so a host is no longer one credential's alone. Do grant one member two credentials toward one host, and two members
/// each their own credential toward one host, each credential by its own placeholder. Expect both taken.
#[test]
fn w33_two_credentials_toward_one_host_are_taken_each_by_its_own_placeholder() {
    let (mut grants, _) = harness();
    let two = [
        credential(claude(), "anthropic/ada", &["api.anthropic.com"]),
        credential(claude(), "anthropic/team", &["api.anthropic.com"]),
    ];
    assert_eq!(check_credentials(&two, &grants), Ok(()));
    let agent = member(MemberKind::Agent, "helper");
    grants.push(egress(agent.clone(), "api.anthropic.com", Proto::Tcp));
    let each = [
        credential(claude(), "anthropic/ada", &["api.anthropic.com"]),
        credential(agent, "anthropic/team", &["api.anthropic.com"]),
    ];
    assert_eq!(check_credentials(&each, &grants), Ok(()));
}

/// AC-T7.7.2, Shards' D242's step 3 (C2; RFC 6750 §2.1): a member sends its placeholder where the credential goes, as
/// a bearer token is sent. Do grant a credential with a placeholder that is no `b64token` in turn: none, one with a
/// space, a colon, an `=` before its end, a byte past ASCII. Expect each refused naming its placeholder's field, never
/// its bytes. Do grant it with tokens: kiln's, and one of every character a `b64token` takes, its `=` at the end.
/// Expect each taken.
#[test]
fn w33_a_placeholder_no_request_could_carry_is_refused_by_name() {
    let (grants, _) = harness();
    let with = |placeholder: &str| {
        let mut grant = credential(claude(), "anthropic/ada", &["api.anthropic.com"]);
        grant.placeholder = placeholder.to_owned();
        [grant]
    };
    for placeholder in [
        "",
        "kiln placeholder",
        "kiln:placeholder",
        "kiln=placeholder",
        "kiln-placeholder-\u{e9}",
    ] {
        let why = refused(&with(placeholder), &grants);
        assert!(
            why.contains("credentials[0].placeholder") && why.contains("RFC 6750"),
            "{placeholder:?}: {why}"
        );
        assert!(
            placeholder.is_empty() || !why.contains(placeholder),
            "the refusal printed its placeholder: {why}"
        );
    }
    for placeholder in [
        "kiln-placeholder-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "Az09-._~+/==",
    ] {
        assert_eq!(
            check_credentials(&with(placeholder), &grants),
            Ok(()),
            "{placeholder:?}"
        );
    }
}

/// AC-T7.7.2, Shards' D242's step 3 (C2): one placeholder could stand for neither of two credentials alone, as the
/// injector could not tell which a request carrying it asks. Do give two credentials one placeholder, one member's and
/// two members'. Expect each refused naming both grants and neither placeholder. Do give two members' grants of one
/// credential one placeholder. Expect it taken: it stands for that credential alone.
#[test]
fn w33_one_placeholder_for_two_credentials_is_refused() {
    let agent = member(MemberKind::Agent, "helper");
    let (mut grants, _) = harness();
    grants.push(egress(agent.clone(), "api.anthropic.com", Proto::Tcp));
    let shared = |grant: CredentialGrant| CredentialGrant {
        placeholder: placeholder("anthropic/ada"),
        ..grant
    };
    for two in [
        [
            credential(claude(), "anthropic/ada", &["api.anthropic.com"]),
            shared(credential(
                claude(),
                "anthropic/team",
                &["platform.claude.com"],
            )),
            credential(agent.clone(), "anthropic/ada", &["api.anthropic.com"]),
        ],
        [
            credential(claude(), "anthropic/ada", &["api.anthropic.com"]),
            credential(agent.clone(), "anthropic/ada", &["api.anthropic.com"]),
            shared(credential(
                agent.clone(),
                "anthropic/team",
                &["api.anthropic.com"],
            )),
        ],
    ] {
        let why = refused(&two, &grants);
        assert!(
            why.contains("credentials[1].placeholder")
                || why.contains("credentials[2].placeholder"),
            "{why}"
        );
        assert!(
            why.contains("anthropic/ada") && why.contains("anthropic/team"),
            "{why}"
        );
        assert!(!why.contains(&placeholder("anthropic/ada")), "{why}");
    }
    let one = [
        credential(claude(), "anthropic/ada", &["api.anthropic.com"]),
        credential(agent, "anthropic/ada", &["api.anthropic.com"]),
    ];
    assert_eq!(check_credentials(&one, &grants), Ok(()));
}

/// AC-T7.7.2, Shards' D242's "Who" (C2): Shards' injector ends the TLS of every flow to a credential's host. Do let a
/// second member reach a credential's host by its own egress grant, ungranted the credential, then granted it too.
/// Expect the first refused, naming the member and the host, and the second taken; a member reaching that host by UDP
/// alone (no TLS) is no such member.
#[test]
fn w33_every_member_reaching_a_credentials_host_holds_it() {
    let agent = member(MemberKind::Agent, "helper");
    let (mut grants, mut credentials) = harness();
    grants.push(egress(agent.clone(), "api.anthropic.com", Proto::Tcp));
    let why = refused(&credentials, &grants);
    assert!(
        why.contains("grants[2] lets agent helper reach api.anthropic.com, toward which credential anthropic/ada"),
        "{why}"
    );
    credentials.push(credential(
        agent.clone(),
        "anthropic/ada",
        &["api.anthropic.com"],
    ));
    assert_eq!(check_credentials(&credentials, &grants), Ok(()));
    let (mut grants, credentials) = harness();
    grants.push(egress(agent, "api.anthropic.com", Proto::Udp));
    assert_eq!(check_credentials(&credentials, &grants), Ok(()));
}

/// Joint plan step 33 (every credential grant in Hex's record of the VM): do write a credential grant as the record's
/// JSON and read it back, and read one a record written before placeholders holds. Expect it whole, each field by its
/// name, its placeholder last; the older one read with none.
#[test]
fn w33_a_credential_grant_round_trips_through_the_records_json() {
    let (_, credentials) = harness();
    let json = serde_json::to_string(&credentials).unwrap();
    assert_eq!(
        json,
        r#"[{"member":{"kind":"harness","name":"claude"},"reference":"anthropic/ada","toward":["api.anthropic.com","platform.claude.com"],"placeholder":"kiln-placeholder-616e7468726f7069632f616461"}]"#
    );
    let back: Vec<CredentialGrant> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, credentials);
    let older: CredentialGrant = serde_json::from_str(
        r#"{"member":{"kind":"harness","name":"claude"},"reference":"anthropic/ada","toward":["api.anthropic.com"]}"#,
    )
    .unwrap();
    assert_eq!(older.placeholder, "");
}

/// Shards' D242 (holding a placeholder is the capability): do format a credential grant with `{:?}`. Expect its
/// member, reference and hosts, and of its placeholder its length alone.
#[test]
fn w33_a_credential_grants_debug_shows_its_placeholders_length_alone() {
    let (_, credentials) = harness();
    let shown = format!("{credentials:?}");
    assert!(!shown.contains(&credentials[0].placeholder), "{shown}");
    assert!(shown.contains("placeholder: 43 bytes"), "{shown}");
    assert!(
        shown.contains("reference: \"anthropic/ada\"") && shown.contains("platform.claude.com"),
        "{shown}"
    );
}
