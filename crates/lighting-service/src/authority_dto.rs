use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use lighting_core::{
    AuthorityCheck, AuthorityGrantId, AuthorityScope, PrincipalId, ReceiptDecision, ReceiptOutcome,
    TrailId,
};
use serde::Deserialize;

pub use lighting_core::AuthorityRequest;

#[derive(Debug, Deserialize)]
pub struct AuthorityCheckRequest {
    pub check: AuthorityCheck,
}

/// Versioned, host-authenticated Tethers authority request.  The caller does
/// not provide a clock value: Lantern stamps the check with its own clock.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TethersAuthorityCheckRequest {
    pub wire_version: String,
    pub action_id: String,
    pub principal_id: PrincipalId,
    pub capability_id: String,
    pub capability_version: String,
    pub scope: AuthorityScope,
    #[serde(default)]
    pub constraints: BTreeMap<String, String>,
}

/// Server-owned receipt construction intent from the Tethers host.  Receipt
/// identifiers, timestamps, predecessor links, and hashes are intentionally
/// absent and can never be pre-sealed by a caller.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TethersReceiptIntent {
    pub wire_version: String,
    pub kind: String,
    pub action_id: String,
    pub principal_id: PrincipalId,
    pub capability_id: String,
    pub capability_version: String,
    pub decision: ReceiptDecision,
    #[serde(default)]
    pub grant_id: Option<AuthorityGrantId>,
    #[serde(default)]
    pub approval_id: Option<lighting_core::ApprovalId>,
    pub scope: AuthorityScope,
    pub executed: bool,
    #[serde(default)]
    pub outcome: Option<ReceiptOutcome>,
    #[serde(default)]
    pub result_ref: Option<String>,
    #[serde(default)]
    pub reason_code: Option<String>,
    #[serde(default)]
    pub trail_id: Option<TrailId>,
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
