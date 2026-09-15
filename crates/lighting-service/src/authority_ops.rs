use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, Duration, Utc};
use lighting_core::{
    AuthorityCheck, AuthorityDecision, AuthorityError, AuthorityGrant, AuthorityGrantId,
    AuthorityLedger, AuthorityRevocation, AuthorityRevocationId, EpisodeId, PrincipalId,
};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::authority_dto::{GrantIntent, RevocationIntent};

#[derive(Clone)]
pub struct AuthorityService {
    ledger: Arc<RwLock<AuthorityLedger>>,
    sessions: Arc<RwLock<HashMap<String, AuthenticatedControlSession>>>,
}

/// The only session record accepted by an authority mutation. It is created
/// by trusted server/bootstrap code, never deserialized from an agent request.
#[derive(Clone, Debug)]
pub struct AuthenticatedControlSession {
    pub session_id: String,
    pub csrf_token: String,
    pub principal_id: PrincipalId,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ControlSession {
    pub session_id: String,
    pub csrf_token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthorityOperationError {
    #[error("authority service is not available")]
    Unavailable,
    #[error("authenticated control-plane session is missing or invalid")]
    Unauthenticated,
    #[error("control-plane CSRF token is missing or invalid")]
    InvalidCsrf,
    #[error("authenticated control-plane session has expired")]
    ExpiredSession,
    #[error("authority operation is invalid: {0}")]
    Invalid(String),
}

impl AuthorityService {
    pub fn new() -> Self {
        Self {
            ledger: Arc::new(RwLock::new(AuthorityLedger::default())),
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Trusted bootstrap seam for a human/demo control session. This method
    /// is intentionally not mounted as an HTTP or Dreamer route.
    pub async fn open_control_session(&self, principal_id: PrincipalId) -> ControlSession {
        self.open_control_session_with_expiry(principal_id, Utc::now() + Duration::hours(1))
            .await
    }

    /// Trusted bootstrap/test seam allowing an explicit expiry for lifecycle
    /// tests. Callers must not take principal identity from an untrusted body.
    pub async fn open_control_session_with_expiry(
        &self,
        principal_id: PrincipalId,
        expires_at: DateTime<Utc>,
    ) -> ControlSession {
        let issued_at = Utc::now();
        let authenticated = AuthenticatedControlSession {
            session_id: format!("session_{}", Uuid::new_v4().simple()),
            csrf_token: format!("csrf_{}", Uuid::new_v4().simple()),
            principal_id,
            issued_at,
            expires_at,
        };
        self.sessions
            .write()
            .await
            .insert(authenticated.session_id.clone(), authenticated.clone());
        ControlSession {
            session_id: authenticated.session_id,
            csrf_token: authenticated.csrf_token,
            expires_at: authenticated.expires_at,
        }
    }

    pub async fn list_grants(&self) -> Vec<AuthorityGrant> {
        self.ledger.read().await.grants().to_vec()
    }

    pub async fn list_revocations(&self) -> Vec<AuthorityRevocation> {
        self.ledger.read().await.revocations().to_vec()
    }

    pub async fn check(&self, check: &AuthorityCheck) -> AuthorityDecision {
        self.ledger.read().await.check(check)
    }

    pub async fn explain(
        &self,
        grant_id: &AuthorityGrantId,
    ) -> Option<(AuthorityGrant, Option<AuthorityRevocation>)> {
        let ledger = self.ledger.read().await;
        let grant = ledger
            .grants()
            .iter()
            .find(|grant| &grant.id == grant_id)?
            .clone();
        let revocation = ledger
            .revocations()
            .iter()
            .find(|revocation| &revocation.grant_id == grant_id)
            .cloned();
        Some((grant, revocation))
    }

    pub async fn issue_from_control_plane(
        &self,
        session_id: &str,
        csrf_token: &str,
        intent: GrantIntent,
    ) -> Result<AuthorityGrant, AuthorityOperationError> {
        let session = self.authenticate(session_id, csrf_token).await?;
        if intent.capability_id.trim().is_empty() || intent.capability_version.trim().is_empty() {
            return Err(AuthorityOperationError::Invalid(
                "capability_id and capability_version cannot be blank".to_owned(),
            ));
        }
        let issued_at = Utc::now();
        let grant = AuthorityGrant {
            id: AuthorityGrantId::new(format!("grant_{}", Uuid::new_v4().simple()))
                .map_err(|error| AuthorityOperationError::Invalid(error.to_string()))?,
            issuer_principal_id: session.principal_id,
            delegate_principal_id: intent.delegate_principal_id,
            capability_id: intent.capability_id,
            capability_version: intent.capability_version,
            scope: intent.scope,
            issued_at,
            expires_at: intent.expires_at,
            source_episode_id: service_owned_provenance_id(),
            authenticated_session_id: session.session_id,
            constraints: intent.constraints,
            created_at: issued_at,
        };
        self.ledger
            .write()
            .await
            .issue_grant(grant.clone())
            .map_err(map_authority_error)?;
        Ok(grant)
    }

    pub async fn revoke_from_control_plane(
        &self,
        session_id: &str,
        csrf_token: &str,
        intent: RevocationIntent,
    ) -> Result<AuthorityRevocation, AuthorityOperationError> {
        let session = self.authenticate(session_id, csrf_token).await?;
        let revocation = AuthorityRevocation {
            id: AuthorityRevocationId::new(format!("revocation_{}", Uuid::new_v4().simple()))
                .map_err(|error| AuthorityOperationError::Invalid(error.to_string()))?,
            grant_id: intent.grant_id,
            issuer_principal_id: session.principal_id,
            source_episode_id: service_owned_provenance_id(),
            authenticated_session_id: session.session_id,
            revoked_at: Utc::now(),
        };
        self.ledger
            .write()
            .await
            .revoke_grant(revocation.clone())
            .map_err(map_authority_error)?;
        Ok(revocation)
    }

    async fn authenticate(
        &self,
        session_id: &str,
        csrf_token: &str,
    ) -> Result<AuthenticatedControlSession, AuthorityOperationError> {
        if session_id.trim().is_empty() {
            return Err(AuthorityOperationError::Unauthenticated);
        }
        let sessions = self.sessions.read().await;
        let Some(session) = sessions.get(session_id) else {
            return Err(AuthorityOperationError::Unauthenticated);
        };
        if session.csrf_token != csrf_token {
            return Err(AuthorityOperationError::InvalidCsrf);
        }
        if session.expires_at <= Utc::now() {
            return Err(AuthorityOperationError::ExpiredSession);
        }
        Ok(session.clone())
    }

    pub fn now() -> chrono::DateTime<Utc> {
        Utc::now()
    }
}

fn service_owned_provenance_id() -> EpisodeId {
    EpisodeId::new(format!("authority-control-{}", Uuid::new_v4().simple()))
        .expect("generated authority provenance identifier is non-empty")
}

impl Default for AuthorityService {
    fn default() -> Self {
        Self::new()
    }
}

fn map_authority_error(error: AuthorityError) -> AuthorityOperationError {
    AuthorityOperationError::Invalid(error.to_string())
}
