use lighting_core::{AuthorityCheck, AuthorityGrant, AuthorityRevocation};
use serde::Deserialize;

pub use lighting_core::{AuthorityGrantId, AuthorityRequest, AuthorityScope};

#[derive(Debug, Deserialize)]
pub struct AuthorityCheckRequest {
    pub check: AuthorityCheck,
}

#[derive(Debug, Deserialize)]
pub struct GrantRequest {
    pub grant: AuthorityGrant,
}

#[derive(Debug, Deserialize)]
pub struct RevocationRequest {
    pub revocation: AuthorityRevocation,
}
