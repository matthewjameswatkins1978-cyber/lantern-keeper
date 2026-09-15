use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, Duration, Utc};
use lighting_core::{
    AuthorityCheck, AuthorityDecision, AuthorityError, AuthorityGrant, AuthorityGrantId,
    AuthorityLedger, AuthorityRevocation, AuthorityRevocationId, Episode, EpisodeId, EpisodeTitle,
    MemoryPathRepository, NewSource, PrincipalId, Source, SourceContent, SourceKind, SourceRange,
    SourceRepository, SourceTitle, StoreSourceResult,
};
use lighting_store_surreal::{
    SurrealAuthorityError, SurrealAuthorityRepository, SurrealMemoryPathRepository,
    SurrealSourceRepository, SurrealStore,
};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::authority_dto::{GrantIntent, RevocationIntent};

#[derive(Clone)]
pub struct AuthorityService {
    ledger: Arc<RwLock<AuthorityLedger>>,
    sessions: Arc<RwLock<HashMap<String, AuthenticatedControlSession>>>,
    persistence: Option<Arc<SurrealAuthorityRepository>>,
    source_repository: Option<Arc<SurrealSourceRepository>>,
    memory_path_repository: Option<Arc<SurrealMemoryPathRepository>>,
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
    #[error("persistent authority state is unavailable: {0}")]
    Persistence(String),
}

impl AuthorityService {
    pub fn new() -> Self {
        Self {
            ledger: Arc::new(RwLock::new(AuthorityLedger::default())),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            persistence: None,
            source_repository: None,
            memory_path_repository: None,
        }
    }

    /// Constructs the production authority service over Lantern's persistent
    /// store. Persistent reads are used for every decision; no empty in-memory
    /// fallback is possible after a storage error.
    pub fn with_store(store: SurrealStore) -> Self {
        Self {
            ledger: Arc::new(RwLock::new(AuthorityLedger::default())),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            persistence: Some(Arc::new(SurrealAuthorityRepository::new(store.clone()))),
            source_repository: Some(Arc::new(SurrealSourceRepository::new(store.clone()))),
            memory_path_repository: Some(Arc::new(SurrealMemoryPathRepository::new(store))),
        }
    }

    pub async fn migrate(&self) -> Result<(), AuthorityOperationError> {
        if let Some(repository) = &self.persistence {
            repository.migrate().await.map_err(map_persistence_error)?;
        }
        Ok(())
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

    pub async fn list_grants(&self) -> Result<Vec<AuthorityGrant>, AuthorityOperationError> {
        Ok(self.read_ledger().await?.grants().to_vec())
    }

    pub async fn list_revocations(
        &self,
    ) -> Result<Vec<AuthorityRevocation>, AuthorityOperationError> {
        Ok(self.read_ledger().await?.revocations().to_vec())
    }

    pub async fn check(
        &self,
        check: &AuthorityCheck,
    ) -> Result<AuthorityDecision, AuthorityOperationError> {
        Ok(self.read_ledger().await?.check(check))
    }

    pub async fn explain(
        &self,
        grant_id: &AuthorityGrantId,
    ) -> Result<Option<(AuthorityGrant, Option<AuthorityRevocation>)>, AuthorityOperationError>
    {
        let ledger = self.read_ledger().await?;
        let grant = ledger
            .grants()
            .iter()
            .find(|grant| &grant.id == grant_id)
            .cloned();
        let Some(grant) = grant else {
            return Ok(None);
        };
        let revocation = ledger
            .revocations()
            .iter()
            .find(|revocation| &revocation.grant_id == grant_id)
            .cloned();
        Ok(Some((grant, revocation)))
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
            source_episode_id: placeholder_provenance_id(),
            authenticated_session_id: session.session_id,
            constraints: intent.constraints,
            created_at: issued_at,
        };
        let mut grant = grant;
        if let Some(repository) = self.persistence.as_ref() {
            grant.source_episode_id = self.persist_provenance(&grant, "authority_grant").await?;
            repository
                .issue_grant(&grant)
                .await
                .map_err(map_persistence_error)?;
        } else {
            self.ledger
                .write()
                .await
                .issue_grant(grant.clone())
                .map_err(map_authority_error)?;
        }
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
            source_episode_id: placeholder_provenance_id(),
            authenticated_session_id: session.session_id,
            revoked_at: Utc::now(),
        };
        if let Some(repository) = &self.persistence {
            let mut ledger = repository
                .load_ledger()
                .await
                .map_err(map_persistence_error)?;
            ledger
                .revoke_grant(revocation.clone())
                .map_err(map_authority_error)?;
            let mut revocation = revocation;
            revocation.source_episode_id = self
                .persist_provenance(&revocation, "authority_revocation")
                .await?;
            repository
                .revoke_grant(&revocation)
                .await
                .map_err(map_persistence_error)?;
            return Ok(revocation);
        }
        self.ledger
            .write()
            .await
            .revoke_grant(revocation.clone())
            .map_err(map_authority_error)?;
        Ok(revocation)
    }

    async fn read_ledger(&self) -> Result<AuthorityLedger, AuthorityOperationError> {
        if let Some(repository) = &self.persistence {
            return repository
                .load_ledger()
                .await
                .map_err(map_persistence_error);
        }
        Ok(self.ledger.read().await.clone())
    }

    async fn persist_provenance<T>(
        &self,
        record: &T,
        operation: &str,
    ) -> Result<EpisodeId, AuthorityOperationError>
    where
        T: serde::Serialize,
    {
        let source_repository = self.source_repository.as_ref().ok_or_else(|| {
            AuthorityOperationError::Persistence("Source repository is missing".to_owned())
        })?;
        let memory_path_repository = self.memory_path_repository.as_ref().ok_or_else(|| {
            AuthorityOperationError::Persistence("Episode repository is missing".to_owned())
        })?;
        let mut canonical = serde_json::to_value(record)
            .map_err(|error| AuthorityOperationError::Persistence(error.to_string()))?;
        if let serde_json::Value::Object(fields) = &mut canonical {
            fields.remove("source_episode_id");
        }
        let content = serde_json::to_string(&canonical)
            .map_err(|error| AuthorityOperationError::Persistence(error.to_string()))?;
        let content =
            format!("Lantern authority operation: {operation}. Canonical record: {content}");
        let source = Source::create(NewSource {
            kind: SourceKind::PlainText,
            title: SourceTitle::new(format!("Authority {operation} evidence"))
                .map_err(|error| AuthorityOperationError::Persistence(error.to_string()))?,
            content: SourceContent::new(content.clone())
                .map_err(|error| AuthorityOperationError::Persistence(error.to_string()))?,
        });
        let source = match source_repository
            .store(source)
            .await
            .map_err(map_source_error)?
        {
            StoreSourceResult::Stored(source) => source,
            StoreSourceResult::Duplicate { existing_id, .. } => source_repository
                .get(&existing_id)
                .await
                .map_err(map_source_error)?
                .ok_or_else(|| {
                    AuthorityOperationError::Persistence(
                        "stored provenance Source disappeared".to_owned(),
                    )
                })?,
        };
        let range = SourceRange::new(
            source.id().clone(),
            0,
            source.content().as_bytes().len(),
            source.content(),
        )
        .map_err(|error| AuthorityOperationError::Persistence(error.to_string()))?;
        let episode = Episode::new(
            EpisodeTitle::new(format!("Authority {operation}"))
                .map_err(|error| AuthorityOperationError::Persistence(error.to_string()))?,
            range,
        );
        let episode = memory_path_repository
            .create_episode(episode)
            .await
            .map_err(|error| AuthorityOperationError::Persistence(error.to_string()))?;
        Ok(episode.id().clone())
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

fn placeholder_provenance_id() -> EpisodeId {
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

fn map_persistence_error(error: SurrealAuthorityError) -> AuthorityOperationError {
    AuthorityOperationError::Persistence(error.to_string())
}

fn map_source_error(error: impl std::error::Error) -> AuthorityOperationError {
    AuthorityOperationError::Persistence(error.to_string())
}
