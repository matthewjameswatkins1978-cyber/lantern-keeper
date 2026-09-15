use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use lighting_core::{
    AuthorityCheck, AuthorityDecision, AuthorityGrant, AuthorityGrantId, AuthorityLedger,
    AuthorityRequest, AuthorityRevocation, AuthorityScope, DenyReason, EpisodeId, PrincipalId,
};

fn at(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

fn scope(destination: &str) -> AuthorityScope {
    AuthorityScope::from_pairs([("project", "demo-project"), ("destination", destination)]).unwrap()
}

fn request(version: &str, destination: &str) -> AuthorityCheck {
    AuthorityCheck {
        request: AuthorityRequest {
            action_id: "benchmark-action".to_owned(),
            principal_id: PrincipalId::new("agent:lucy").unwrap(),
            capability_id: "demo.export_summary".to_owned(),
            capability_version: version.to_owned(),
            scope: scope(destination),
            constraints: BTreeMap::new(),
        },
        at: at("2026-09-14T12:00:00Z"),
    }
}

fn grant(id: &str, version: &str, expires_at: &str) -> AuthorityGrant {
    AuthorityGrant {
        id: AuthorityGrantId::new(id).unwrap(),
        issuer_principal_id: PrincipalId::new("human:demo").unwrap(),
        delegate_principal_id: PrincipalId::new("agent:lucy").unwrap(),
        capability_id: "demo.export_summary".to_owned(),
        capability_version: version.to_owned(),
        scope: scope("approved-demo-sink"),
        issued_at: at("2026-09-14T10:00:00Z"),
        expires_at: Some(at(expires_at)),
        source_episode_id: EpisodeId::new("episode-benchmark").unwrap(),
        authenticated_session_id: "benchmark-session".to_owned(),
        constraints: BTreeMap::new(),
        created_at: at("2026-09-14T10:00:00Z"),
    }
}

#[test]
fn deterministic_authority_matrix_has_zero_bypasses() {
    let exact = grant("grant-exact", "1", "2026-09-15T00:00:00Z");
    let mut active = grant("grant-active", "1", "2026-09-15T00:00:00Z");
    active.scope = scope("approved-secondary-sink");
    let expired = grant("grant-expired", "2", "2026-09-14T11:00:00Z");
    let mut ledger = AuthorityLedger::default();
    ledger.issue_grant(exact.clone()).unwrap();
    ledger.issue_grant(active.clone()).unwrap();
    ledger.issue_grant(expired).unwrap();
    ledger
        .revoke_grant(AuthorityRevocation {
            id: lighting_core::AuthorityRevocationId::new("revoke-exact").unwrap(),
            grant_id: exact.id.clone(),
            issuer_principal_id: exact.issuer_principal_id.clone(),
            source_episode_id: EpisodeId::new("episode-revoke").unwrap(),
            authenticated_session_id: "benchmark-session".to_owned(),
            revoked_at: at("2026-09-14T13:00:00Z"),
        })
        .unwrap();

    let cases = [
        (
            request("1", "approved-secondary-sink"),
            AuthorityDecision::Allow {
                grant_id: active.id,
                expires_at: active.expires_at,
            },
        ),
        (
            request("1", "attacker.invalid"),
            AuthorityDecision::Deny {
                reason: DenyReason::ScopeMismatch,
            },
        ),
        (
            request("3", "approved-demo-sink"),
            AuthorityDecision::Deny {
                reason: DenyReason::NoMatchingGrant,
            },
        ),
        (
            AuthorityCheck {
                at: at("2026-09-14T13:30:00Z"),
                ..request("1", "approved-demo-sink")
            },
            AuthorityDecision::Deny {
                reason: DenyReason::RevokedGrant,
            },
        ),
        (
            AuthorityCheck {
                at: at("2026-09-14T11:30:00Z"),
                ..request("2", "approved-demo-sink")
            },
            AuthorityDecision::Deny {
                reason: DenyReason::ExpiredGrant,
            },
        ),
    ];
    let mut bypasses = 0;
    for (check, expected) in cases {
        let actual = ledger.check(&check);
        if actual != expected {
            bypasses += 1;
        }
    }
    assert_eq!(bypasses, 0, "deterministic authorization bypasses");
    assert_eq!(ledger.grant_count(), 3);
    assert_eq!(ledger.revocation_count(), 1);
}
