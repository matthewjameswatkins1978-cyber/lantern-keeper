use std::{collections::HashMap, sync::Arc};

use chrono::Utc;
use lighting_core::{
    AuthorityCheck, AuthorityDecision, AuthorityError, AuthorityGrant, AuthorityGrantId,
    AuthorityLedger, AuthorityRevocation,
};
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Clone)]
pub struct AuthorityService {
    ledger: Arc<RwLock<AuthorityLedger>>,
    sessions: Arc<RwLock<HashMap<String, String>>>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ControlSession {
    pub session_id: String,
    pub csrf_token: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthorityOperationError {
    #[error("authority service is not available")]
    Unavailable,
    #[error("authenticated control-plane session is missing or invalid")]
    Unauthenticated,
    #[error("control-plane CSRF token is missing or invalid")]
    InvalidCsrf,
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

    pub async fn open_control_session(&self) -> ControlSession {
        let session = ControlSession {
            session_id: format!("session_{}", Uuid::new_v4().simple()),
            csrf_token: format!("csrf_{}", Uuid::new_v4().simple()),
        };
        self.sessions
            .write()
            .await
            .insert(session.session_id.clone(), session.csrf_token.clone());
        session
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
        grant: AuthorityGrant,
    ) -> Result<(), AuthorityOperationError> {
        self.authenticate(session_id, csrf_token).await?;
        self.ledger
            .write()
            .await
            .issue_grant(grant)
            .map_err(map_authority_error)
    }

    pub async fn revoke_from_control_plane(
        &self,
        session_id: &str,
        csrf_token: &str,
        revocation: AuthorityRevocation,
    ) -> Result<(), AuthorityOperationError> {
        self.authenticate(session_id, csrf_token).await?;
        self.ledger
            .write()
            .await
            .revoke_grant(revocation)
            .map_err(map_authority_error)
    }

    async fn authenticate(
        &self,
        session_id: &str,
        csrf_token: &str,
    ) -> Result<(), AuthorityOperationError> {
        if session_id.trim().is_empty() {
            return Err(AuthorityOperationError::Unauthenticated);
        }
        let sessions = self.sessions.read().await;
        let Some(expected_csrf) = sessions.get(session_id) else {
            return Err(AuthorityOperationError::Unauthenticated);
        };
        if expected_csrf != csrf_token {
            return Err(AuthorityOperationError::InvalidCsrf);
        }
        Ok(())
    }

    pub fn now() -> chrono::DateTime<Utc> {
        Utc::now()
    }
}

impl Default for AuthorityService {
    fn default() -> Self {
        Self::new()
    }
}

fn map_authority_error(error: AuthorityError) -> AuthorityOperationError {
    AuthorityOperationError::Invalid(error.to_string())
}
