//! Durable append-only authority records.

use async_trait::async_trait;
use lighting_core::{AuthorityGrant, AuthorityLedger, AuthorityRevocation};
use serde_json::from_str;
use surrealdb::types::Object;
use thiserror::Error;

use crate::connection::SurrealStore;

#[derive(Debug, Error)]
pub enum SurrealAuthorityError {
    #[error("authority query failed: {0}")]
    Query(#[source] surrealdb::Error),
    #[error("authority payload could not be encoded: {0}")]
    Encode(#[source] serde_json::Error),
    #[error("authority payload could not be decoded: {0}")]
    Decode(#[source] serde_json::Error),
    #[error("authority record is missing its payload")]
    MissingPayload,
    #[error("authority ledger is corrupt: {0}")]
    Corrupt(String),
}

#[derive(Clone)]
pub struct SurrealAuthorityRepository {
    store: SurrealStore,
}

impl SurrealAuthorityRepository {
    pub fn new(store: SurrealStore) -> Self {
        Self { store }
    }

    pub async fn migrate(&self) -> Result<(), SurrealAuthorityError> {
        self.store
            .query(SCHEMA_MIGRATION_V10)
            .await
            .map(|_| ())
            .map_err(SurrealAuthorityError::Query)
    }

    pub async fn load_ledger(&self) -> Result<AuthorityLedger, SurrealAuthorityError> {
        let grants = self.list_grants().await?;
        let revocations = self.list_revocations().await?;
        let mut ledger = AuthorityLedger::default();
        for grant in grants {
            ledger
                .issue_grant(grant)
                .map_err(|error| SurrealAuthorityError::Corrupt(error.to_string()))?;
        }
        for revocation in revocations {
            ledger
                .revoke_grant(revocation)
                .map_err(|error| SurrealAuthorityError::Corrupt(error.to_string()))?;
        }
        Ok(ledger)
    }

    pub async fn list_grants(&self) -> Result<Vec<AuthorityGrant>, SurrealAuthorityError> {
        let records: Vec<Object> = self
            .store
            .query("SELECT * FROM authority_grant ORDER BY issued_at ASC, id ASC")
            .await
            .map_err(SurrealAuthorityError::Query)?
            .take(0)
            .map_err(SurrealAuthorityError::Query)?;
        records
            .into_iter()
            .map(decode_grant)
            .collect::<Result<Vec<_>, _>>()
    }

    pub async fn list_revocations(
        &self,
    ) -> Result<Vec<AuthorityRevocation>, SurrealAuthorityError> {
        let records: Vec<Object> = self
            .store
            .query("SELECT * FROM authority_revocation ORDER BY revoked_at ASC, id ASC")
            .await
            .map_err(SurrealAuthorityError::Query)?
            .take(0)
            .map_err(SurrealAuthorityError::Query)?;
        records
            .into_iter()
            .map(decode_revocation)
            .collect::<Result<Vec<_>, _>>()
    }

    pub async fn issue_grant(&self, grant: &AuthorityGrant) -> Result<(), SurrealAuthorityError> {
        let payload = serde_json::to_string(grant).map_err(SurrealAuthorityError::Encode)?;
        self.store
            .query(
                "CREATE authority_grant CONTENT { id: $id, payload: $payload, issuer_principal_id: $issuer, authenticated_session_id: $auth_session, source_episode_id: $source_episode, issued_at: $issued_at, expires_at: $expires_at };",
            )
            .bind(("id", grant.id.to_string()))
            .bind(("payload", payload))
            .bind(("issuer", grant.issuer_principal_id.to_string()))
            .bind(("auth_session", grant.authenticated_session_id.clone()))
            .bind(("source_episode", grant.source_episode_id.to_string()))
            .bind(("issued_at", grant.issued_at))
            .bind(("expires_at", grant.expires_at))
            .await
            .map(|_| ())
            .map_err(SurrealAuthorityError::Query)
    }

    pub async fn revoke_grant(
        &self,
        revocation: &AuthorityRevocation,
    ) -> Result<(), SurrealAuthorityError> {
        let mut ledger = self.load_ledger().await?;
        ledger
            .revoke_grant(revocation.clone())
            .map_err(|error| SurrealAuthorityError::Corrupt(error.to_string()))?;
        let payload = serde_json::to_string(revocation).map_err(SurrealAuthorityError::Encode)?;
        self.store
            .query(
                "CREATE authority_revocation CONTENT { id: $id, payload: $payload, grant_id: $grant_id, issuer_principal_id: $issuer, authenticated_session_id: $auth_session, source_episode_id: $source_episode, revoked_at: $revoked_at };",
            )
            .bind(("id", revocation.id.to_string()))
            .bind(("payload", payload))
            .bind(("grant_id", revocation.grant_id.to_string()))
            .bind(("issuer", revocation.issuer_principal_id.to_string()))
            .bind(("auth_session", revocation.authenticated_session_id.clone()))
            .bind(("source_episode", revocation.source_episode_id.to_string()))
            .bind(("revoked_at", revocation.revoked_at))
            .await
            .map(|_| ())
            .map_err(SurrealAuthorityError::Query)
    }
}

fn decode_grant(record: Object) -> Result<AuthorityGrant, SurrealAuthorityError> {
    let payload = record
        .get("payload")
        .and_then(|value| value.clone().into_t::<String>().ok())
        .ok_or(SurrealAuthorityError::MissingPayload)?;
    from_str(&payload).map_err(SurrealAuthorityError::Decode)
}

fn decode_revocation(record: Object) -> Result<AuthorityRevocation, SurrealAuthorityError> {
    let payload = record
        .get("payload")
        .and_then(|value| value.clone().into_t::<String>().ok())
        .ok_or(SurrealAuthorityError::MissingPayload)?;
    from_str(&payload).map_err(SurrealAuthorityError::Decode)
}

#[async_trait]
pub trait AuthorityRepository {
    async fn load_ledger(&self) -> Result<AuthorityLedger, SurrealAuthorityError>;
    async fn list_grants(&self) -> Result<Vec<AuthorityGrant>, SurrealAuthorityError>;
    async fn list_revocations(&self) -> Result<Vec<AuthorityRevocation>, SurrealAuthorityError>;
    async fn issue_grant(&self, grant: &AuthorityGrant) -> Result<(), SurrealAuthorityError>;
    async fn revoke_grant(
        &self,
        revocation: &AuthorityRevocation,
    ) -> Result<(), SurrealAuthorityError>;
}

#[async_trait]
impl AuthorityRepository for SurrealAuthorityRepository {
    async fn load_ledger(&self) -> Result<AuthorityLedger, SurrealAuthorityError> {
        self.load_ledger().await
    }

    async fn list_grants(&self) -> Result<Vec<AuthorityGrant>, SurrealAuthorityError> {
        self.list_grants().await
    }

    async fn list_revocations(&self) -> Result<Vec<AuthorityRevocation>, SurrealAuthorityError> {
        self.list_revocations().await
    }

    async fn issue_grant(&self, grant: &AuthorityGrant) -> Result<(), SurrealAuthorityError> {
        self.issue_grant(grant).await
    }

    async fn revoke_grant(
        &self,
        revocation: &AuthorityRevocation,
    ) -> Result<(), SurrealAuthorityError> {
        self.revoke_grant(revocation).await
    }
}

const SCHEMA_MIGRATION_V10: &str = r#"
DEFINE TABLE IF NOT EXISTS authority_grant SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS payload ON authority_grant TYPE string;
DEFINE FIELD IF NOT EXISTS issuer_principal_id ON authority_grant TYPE string;
DEFINE FIELD IF NOT EXISTS authenticated_session_id ON authority_grant TYPE string;
DEFINE FIELD IF NOT EXISTS source_episode_id ON authority_grant TYPE string;
DEFINE FIELD IF NOT EXISTS issued_at ON authority_grant TYPE datetime;
DEFINE FIELD IF NOT EXISTS expires_at ON authority_grant TYPE option<datetime>;
DEFINE INDEX IF NOT EXISTS authority_grant_id ON authority_grant FIELDS id UNIQUE;

DEFINE TABLE IF NOT EXISTS authority_revocation SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS payload ON authority_revocation TYPE string;
DEFINE FIELD IF NOT EXISTS grant_id ON authority_revocation TYPE string;
DEFINE FIELD IF NOT EXISTS issuer_principal_id ON authority_revocation TYPE string;
DEFINE FIELD IF NOT EXISTS authenticated_session_id ON authority_revocation TYPE string;
DEFINE FIELD IF NOT EXISTS source_episode_id ON authority_revocation TYPE string;
DEFINE FIELD IF NOT EXISTS revoked_at ON authority_revocation TYPE datetime;
DEFINE INDEX IF NOT EXISTS authority_revocation_id ON authority_revocation FIELDS id UNIQUE;

UPSERT __lighting_schema:bootstrap CONTENT {
    project: "Lantern Keeper",
    service: "Lighting",
    schema_version: 10,
    updated_at: time::now()
};
"#;
