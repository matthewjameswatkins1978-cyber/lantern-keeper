use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use lighting_core::{AuthorityCheck, AuthorityGrantId, AuthorityScope, PrincipalId};
use serde::Deserialize;

pub use lighting_core::AuthorityRequest;

#[derive(Debug, Deserialize)]
pub struct AuthorityCheckRequest {
    pub check: AuthorityCheck,
}

/// Client-controlled authority intent. Security-sensitive grant fields are
/// deliberately absent: the service fills them from the authenticated
/// control session and its own clock/provenance generator.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantIntent {
    pub delegate_principal_id: PrincipalId,
    pub capability_id: String,
    pub capability_version: String,
    pub scope: AuthorityScope,
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub constraints: BTreeMap<String, String>,
}

/// Client-controlled revocation intent. The revocation identity, issuer,
/// session, timestamp, and provenance are all server-owned.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevocationIntent {
    pub grant_id: AuthorityGrantId,
}
