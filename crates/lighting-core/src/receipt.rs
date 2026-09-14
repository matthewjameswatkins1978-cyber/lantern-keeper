//! Canonical, hash-linked evidence for capability decisions and outcomes.
//!
//! Hash chaining proves ordering and content continuity. It does not, by
//! itself, authenticate the signer or the execution host.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{AuthorityGrantId, ProjectId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionReceipt {
    pub receipt_id: crate::ReceiptId,
    pub action_id: crate::ActionId,
    pub capability_id: String,
    pub capability_version: String,
    pub principal_id: crate::PrincipalId,
    pub decision: ReceiptDecision,
    pub grant_id: Option<AuthorityGrantId>,
    pub approval_id: Option<crate::ApprovalId>,
    pub scope: crate::AuthorityScope,
    pub project_id: Option<ProjectId>,
    pub trail_id: Option<crate::TrailId>,
    pub requested_at: DateTime<Utc>,
    pub decided_at: DateTime<Utc>,
    pub executed: bool,
    pub outcome: Option<ReceiptOutcome>,
    pub result_ref: Option<String>,
    pub reason_code: Option<String>,
    pub previous_receipt_hash: Option<String>,
    pub receipt_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ReceiptDecision {
    Allow,
    Ask,
    Deny,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ReceiptOutcome {
    Success,
    Failure,
    Uncertain,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptChain {
    receipts: Vec<ExecutionReceipt>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ReceiptError {
    #[error("receipt hash is invalid")]
    InvalidHash,
    #[error("receipt previous hash does not match chain")]
    BrokenLink,
    #[error("receipt hash encoding failed")]
    Encoding,
}

impl ExecutionReceipt {
    pub fn seal(mut self, previous_receipt_hash: Option<String>) -> Result<Self, ReceiptError> {
        self.previous_receipt_hash = previous_receipt_hash;
        self.receipt_hash.clear();
        self.receipt_hash = hash_unsigned(&self)?;
        Ok(self)
    }

    pub fn verify(&self) -> Result<(), ReceiptError> {
        if self.receipt_hash != hash_unsigned(self)? {
            return Err(ReceiptError::InvalidHash);
        }
        Ok(())
    }
}

impl ReceiptChain {
    pub fn append(&mut self, receipt: ExecutionReceipt) -> Result<(), ReceiptError> {
        receipt.verify()?;
        if receipt.previous_receipt_hash.as_deref() != self.last_hash() {
            return Err(ReceiptError::BrokenLink);
        }
        self.receipts.push(receipt);
        Ok(())
    }

    pub fn seal_and_append(&mut self, receipt: ExecutionReceipt) -> Result<String, ReceiptError> {
        let receipt = receipt.seal(self.last_hash().map(str::to_owned))?;
        let hash = receipt.receipt_hash.clone();
        self.receipts.push(receipt);
        Ok(hash)
    }

    pub fn verify(&self) -> Result<(), ReceiptError> {
        let mut previous = None;
        for receipt in &self.receipts {
            receipt.verify()?;
            if receipt.previous_receipt_hash.as_deref() != previous {
                return Err(ReceiptError::BrokenLink);
            }
            previous = Some(receipt.receipt_hash.as_str());
        }
        Ok(())
    }

    pub fn receipts(&self) -> &[ExecutionReceipt] {
        &self.receipts
    }

    fn last_hash(&self) -> Option<&str> {
        self.receipts
            .last()
            .map(|receipt| receipt.receipt_hash.as_str())
    }
}

fn hash_unsigned(receipt: &ExecutionReceipt) -> Result<String, ReceiptError> {
    let mut unsigned = receipt.clone();
    unsigned.receipt_hash.clear();
    let bytes = serde_json::to_vec(&unsigned).map_err(|_| ReceiptError::Encoding)?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn receipt(action: &str, decision: ReceiptDecision) -> ExecutionReceipt {
        ExecutionReceipt {
            receipt_id: crate::ReceiptId::new(format!("receipt-{action}")).unwrap(),
            action_id: crate::ActionId::new(action).unwrap(),
            capability_id: "demo.export_summary".to_owned(),
            capability_version: "1".to_owned(),
            principal_id: crate::PrincipalId::new("agent:lucy").unwrap(),
            decision,
            grant_id: None,
            approval_id: None,
            scope: crate::AuthorityScope(BTreeMap::new()),
            project_id: None,
            trail_id: None,
            requested_at: Utc::now(),
            decided_at: Utc::now(),
            executed: false,
            outcome: None,
            result_ref: None,
            reason_code: Some("NO_MATCHING_GRANT".to_owned()),
            previous_receipt_hash: None,
            receipt_hash: String::new(),
        }
    }

    #[test]
    fn denied_receipt_is_canonical_and_verifiable() {
        let mut chain = ReceiptChain::default();
        chain
            .seal_and_append(receipt("action-1", ReceiptDecision::Deny))
            .unwrap();
        chain.verify().unwrap();
        assert_eq!(chain.receipts()[0].decision, ReceiptDecision::Deny);
        assert!(chain.receipts()[0].receipt_hash.starts_with("sha256:"));
    }

    #[test]
    fn tampering_breaks_hash_chain() {
        let mut chain = ReceiptChain::default();
        chain
            .seal_and_append(receipt("action-1", ReceiptDecision::Deny))
            .unwrap();
        chain.receipts[0].reason_code = Some("ALLOW".to_owned());
        assert_eq!(chain.verify(), Err(ReceiptError::InvalidHash));
    }

    #[test]
    fn second_receipt_links_to_first() {
        let mut chain = ReceiptChain::default();
        chain
            .seal_and_append(receipt("action-1", ReceiptDecision::Deny))
            .unwrap();
        chain
            .seal_and_append(receipt("action-2", ReceiptDecision::Allow))
            .unwrap();
        chain.verify().unwrap();
        assert_eq!(chain.receipts().len(), 2);
        assert_eq!(
            chain.receipts()[1].previous_receipt_hash.as_deref(),
            Some(chain.receipts()[0].receipt_hash.as_str())
        );
    }
}
