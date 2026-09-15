//! The independent authority domain.
//!
//! Epistemic records explain what the system has heard or currently believes.
//! This module deliberately does not consume those records when deciding
//! whether an effectful capability is permitted. Authority is issued and
//! revoked by an authenticated control plane and checked deterministically.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ActorId, EpisodeId};

/// An authenticated security principal. This is not an epistemic actor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    pub id: crate::PrincipalId,
    pub kind: PrincipalKind,
    pub display_name: String,
    pub authn_class: AuthenticationClass,
    /// Optional explicit provenance mapping; never used as authentication.
    pub actor_id: Option<ActorId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrincipalKind {
    HumanSession,
    Agent,
    Service,
    DemoUser,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthenticationClass {
    PasswordlessSession,
    ApiCredential,
    ServiceCredential,
    SyntheticDemo,
}

/// A typed capability scope. Matching is exact: an action cannot smuggle an
/// additional or different resource dimension past a grant.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AuthorityScope(pub BTreeMap<String, String>);

impl AuthorityScope {
    pub fn new(entries: BTreeMap<String, String>) -> Result<Self, AuthorityError> {
        if entries
            .iter()
            .any(|(key, value)| key.trim().is_empty() || value.trim().is_empty())
        {
            return Err(AuthorityError::BlankScopeEntry);
        }
        Ok(Self(entries))
    }

    pub fn from_pairs(
        entries: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Result<Self, AuthorityError> {
        Self::new(
            entries
                .into_iter()
                .map(|(key, value)| (key.into(), value.into()))
                .collect(),
        )
    }

    pub fn as_map(&self) -> &BTreeMap<String, String> {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityGrant {
    pub id: crate::AuthorityGrantId,
    pub issuer_principal_id: crate::PrincipalId,
    pub delegate_principal_id: crate::PrincipalId,
    pub capability_id: String,
    pub capability_version: String,
    pub scope: AuthorityScope,
    pub issued_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub source_episode_id: EpisodeId,
    pub authenticated_session_id: String,
    pub constraints: BTreeMap<String, String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityRevocation {
    pub id: crate::AuthorityRevocationId,
    pub grant_id: crate::AuthorityGrantId,
    pub issuer_principal_id: crate::PrincipalId,
    pub source_episode_id: EpisodeId,
    pub authenticated_session_id: String,
    pub revoked_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityRequest {
    pub action_id: String,
    pub principal_id: crate::PrincipalId,
    pub capability_id: String,
    pub capability_version: String,
    pub scope: AuthorityScope,
    pub constraints: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityCheck {
    pub request: AuthorityRequest,
    pub at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorityDecision {
    Allow {
        grant_id: crate::AuthorityGrantId,
        expires_at: Option<DateTime<Utc>>,
    },
    Deny {
        reason: DenyReason,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DenyReason {
    NoMatchingGrant,
    ExpiredGrant,
    RevokedGrant,
    PrincipalMismatch,
    CapabilityMismatch,
    ScopeMismatch,
    ConstraintMismatch,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum AuthorityError {
    #[error("authority identifier is duplicated")]
    DuplicateIdentifier,
    #[error("scope contains a blank key or value")]
    BlankScopeEntry,
    #[error("grant expiry must be after issue time")]
    InvalidExpiry,
    #[error("revocation references an unknown grant")]
    UnknownGrant,
    #[error("revocation issuer does not match grant issuer")]
    RevocationIssuerMismatch,
}

/// Append-only authority state. No epistemic type appears in the decision
/// algorithm; the source episode is retained only for audit provenance.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityLedger {
    grants: Vec<AuthorityGrant>,
    revocations: Vec<AuthorityRevocation>,
}

impl AuthorityLedger {
    pub fn issue_grant(&mut self, grant: AuthorityGrant) -> Result<(), AuthorityError> {
        if self.grants.iter().any(|item| item.id == grant.id)
            || grant
                .expires_at
                .is_some_and(|expires_at| expires_at <= grant.issued_at)
        {
            return if self.grants.iter().any(|item| item.id == grant.id) {
                Err(AuthorityError::DuplicateIdentifier)
            } else {
                Err(AuthorityError::InvalidExpiry)
            };
        }
        AuthorityScope::new(grant.scope.0.clone())?;
        self.grants.push(grant);
        Ok(())
    }

    pub fn revoke_grant(&mut self, revocation: AuthorityRevocation) -> Result<(), AuthorityError> {
        if self.revocations.iter().any(|item| item.id == revocation.id) {
            return Err(AuthorityError::DuplicateIdentifier);
        }
        let grant = self
            .grants
            .iter()
            .find(|item| item.id == revocation.grant_id)
            .ok_or(AuthorityError::UnknownGrant)?;
        if grant.issuer_principal_id != revocation.issuer_principal_id {
            return Err(AuthorityError::RevocationIssuerMismatch);
        }
        self.revocations.push(revocation);
        Ok(())
    }

    pub fn check(&self, check: &AuthorityCheck) -> AuthorityDecision {
        let candidates = self.grants.iter().filter(|grant| {
            grant.delegate_principal_id == check.request.principal_id
                && grant.capability_id == check.request.capability_id
                && grant.capability_version == check.request.capability_version
        });
        let candidates = candidates.collect::<Vec<_>>();
        if candidates.is_empty() {
            return AuthorityDecision::Deny {
                reason: DenyReason::NoMatchingGrant,
            };
        }
        let scoped = candidates
            .iter()
            .copied()
            .filter(|grant| grant.scope == check.request.scope)
            .collect::<Vec<_>>();
        if scoped.is_empty() {
            return AuthorityDecision::Deny {
                reason: DenyReason::ScopeMismatch,
            };
        }
        let constrained = scoped
            .iter()
            .copied()
            .filter(|grant| grant.constraints == check.request.constraints)
            .collect::<Vec<_>>();
        if constrained.is_empty() {
            return AuthorityDecision::Deny {
                reason: DenyReason::ConstraintMismatch,
            };
        }
        let issued = constrained
            .iter()
            .copied()
            .filter(|grant| grant.issued_at <= check.at)
            .collect::<Vec<_>>();
        if issued.is_empty() {
            return AuthorityDecision::Deny {
                reason: DenyReason::NoMatchingGrant,
            };
        }

        for grant in &issued {
            let revoked = self.revocations.iter().any(|revocation| {
                revocation.grant_id == grant.id && revocation.revoked_at <= check.at
            });
            let expired = grant
                .expires_at
                .is_some_and(|expires_at| expires_at <= check.at);
            if !revoked && !expired {
                return AuthorityDecision::Allow {
                    grant_id: grant.id.clone(),
                    expires_at: grant.expires_at,
                };
            }
        }

        if issued.iter().all(|grant| {
            self.revocations.iter().any(|revocation| {
                revocation.grant_id == grant.id && revocation.revoked_at <= check.at
            })
        }) {
            AuthorityDecision::Deny {
                reason: DenyReason::RevokedGrant,
            }
        } else {
            AuthorityDecision::Deny {
                reason: DenyReason::ExpiredGrant,
            }
        }
    }

    pub fn grants(&self) -> &[AuthorityGrant] {
        &self.grants
    }

    pub fn revocations(&self) -> &[AuthorityRevocation] {
        &self.revocations
    }

    pub fn grant_count(&self) -> usize {
        self.grants.len()
    }

    pub fn revocation_count(&self) -> usize {
        self.revocations.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActorId, EpisodeId};

    fn principal(value: &str) -> crate::PrincipalId {
        crate::PrincipalId::new(value).unwrap()
    }

    fn grant() -> AuthorityGrant {
        AuthorityGrant {
            id: crate::AuthorityGrantId::new("grant-1").unwrap(),
            issuer_principal_id: principal("human:matthew"),
            delegate_principal_id: principal("agent:lucy"),
            capability_id: "demo.export_summary".to_owned(),
            capability_version: "1".to_owned(),
            scope: AuthorityScope::from_pairs([
                ("project", "lantern-demo"),
                ("destination", "approved-demo-sink"),
            ])
            .unwrap(),
            issued_at: DateTime::parse_from_rfc3339("2026-09-14T10:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            expires_at: Some(
                DateTime::parse_from_rfc3339("2026-09-15T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            source_episode_id: EpisodeId::new("episode-1").unwrap(),
            authenticated_session_id: "session-opaque".to_owned(),
            constraints: BTreeMap::new(),
            created_at: Utc::now(),
        }
    }

    fn check(scope: AuthorityScope, at: &str) -> AuthorityCheck {
        AuthorityCheck {
            request: AuthorityRequest {
                action_id: "action-1".to_owned(),
                principal_id: principal("agent:lucy"),
                capability_id: "demo.export_summary".to_owned(),
                capability_version: "1".to_owned(),
                scope,
                constraints: BTreeMap::new(),
            },
            at: DateTime::parse_from_rfc3339(at)
                .unwrap()
                .with_timezone(&Utc),
        }
    }

    #[test]
    fn exact_scope_is_required_for_allow() {
        let mut ledger = AuthorityLedger::default();
        ledger.issue_grant(grant()).unwrap();

        assert!(matches!(
            ledger.check(&check(
                AuthorityScope::from_pairs([("project", "lantern-demo")]).unwrap(),
                "2026-09-14T12:00:00Z"
            )),
            AuthorityDecision::Deny {
                reason: DenyReason::ScopeMismatch
            }
        ));
        assert!(matches!(
            ledger.check(&check(
                AuthorityScope::from_pairs([
                    ("project", "lantern-demo"),
                    ("destination", "approved-demo-sink")
                ])
                .unwrap(),
                "2026-09-14T12:00:00Z"
            )),
            AuthorityDecision::Allow { .. }
        ));
    }

    #[test]
    fn expiry_and_revocation_are_deterministic() {
        let mut ledger = AuthorityLedger::default();
        let grant = grant();
        ledger.issue_grant(grant.clone()).unwrap();
        assert!(matches!(
            ledger.check(&check(grant.scope.clone(), "2026-09-15T00:00:00Z")),
            AuthorityDecision::Deny {
                reason: DenyReason::ExpiredGrant
            }
        ));

        ledger
            .revoke_grant(AuthorityRevocation {
                id: crate::AuthorityRevocationId::new("revoke-1").unwrap(),
                grant_id: grant.id,
                issuer_principal_id: principal("human:matthew"),
                source_episode_id: EpisodeId::new("episode-2").unwrap(),
                authenticated_session_id: "session-opaque".to_owned(),
                revoked_at: DateTime::parse_from_rfc3339("2026-09-14T12:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            })
            .unwrap();
        assert!(matches!(
            ledger.check(&check(
                AuthorityScope::from_pairs([
                    ("project", "lantern-demo"),
                    ("destination", "approved-demo-sink")
                ])
                .unwrap(),
                "2026-09-14T13:00:00Z"
            )),
            AuthorityDecision::Deny {
                reason: DenyReason::RevokedGrant
            }
        ));
    }

    #[test]
    fn an_active_duplicate_scope_survives_a_revoked_grant() {
        let mut ledger = AuthorityLedger::default();
        let mut revoked = grant();
        revoked.id = crate::AuthorityGrantId::new("grant-revoked").unwrap();
        ledger.issue_grant(revoked.clone()).unwrap();
        ledger
            .revoke_grant(AuthorityRevocation {
                id: crate::AuthorityRevocationId::new("revoke-1").unwrap(),
                grant_id: revoked.id,
                issuer_principal_id: revoked.issuer_principal_id,
                source_episode_id: EpisodeId::new("episode-revoke").unwrap(),
                authenticated_session_id: "session-opaque".to_owned(),
                revoked_at: DateTime::parse_from_rfc3339("2026-09-14T11:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            })
            .unwrap();
        let mut active = grant();
        active.id = crate::AuthorityGrantId::new("grant-active").unwrap();
        ledger.issue_grant(active.clone()).unwrap();

        assert_eq!(
            ledger.check(&check(active.scope.clone(), "2026-09-14T12:00:00Z")),
            AuthorityDecision::Allow {
                grant_id: active.id,
                expires_at: active.expires_at,
            }
        );
    }

    #[test]
    fn actor_id_is_not_a_principal() {
        let actor = ActorId::new("matthew").unwrap();
        let principal = principal("human:matthew");
        assert_ne!(actor.to_string(), principal.to_string());
    }

    #[test]
    fn invalid_revocation_cannot_change_ledger() {
        let mut ledger = AuthorityLedger::default();
        let result = ledger.revoke_grant(AuthorityRevocation {
            id: crate::AuthorityRevocationId::new("revoke-1").unwrap(),
            grant_id: crate::AuthorityGrantId::new("missing").unwrap(),
            issuer_principal_id: principal("human:matthew"),
            source_episode_id: EpisodeId::new("episode-2").unwrap(),
            authenticated_session_id: "session-opaque".to_owned(),
            revoked_at: Utc::now(),
        });
        assert_eq!(result, Err(AuthorityError::UnknownGrant));
        assert_eq!(ledger.grant_count(), 0);
        assert_eq!(ledger.revocation_count(), 0);
    }
}
