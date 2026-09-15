//! Core domain types for Lantern Keeper's Lighting service.
//!
//! This crate intentionally has no storage, HTTP, filesystem, or CLI
//! dependencies. It owns domain meaning: Sources are authoritative original
//! material, and any derived data must point back to them rather than replace
//! them.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod authority;
pub mod dreamer;
pub mod epistemic;
pub mod ledger;
pub mod memory;
pub mod memory_path;
pub mod receipt;
pub mod source;
pub mod source_outline;

pub use authority::{
    AuthenticationClass, AuthorityCheck, AuthorityDecision, AuthorityError, AuthorityGrant,
    AuthorityLedger, AuthorityRequest, AuthorityRevocation, AuthorityScope, DenyReason, Principal,
    PrincipalKind,
};
pub use dreamer::{CandidateKind, DreamerCandidate, DreamerCandidateError, EvidenceReference};
pub use epistemic::{
    Actor, Belief, BeliefLineage, BeliefRevision, BeliefState, Claim, ContextPack, ContextTrace,
    DimensionDefinition, EpistemicError, EpistemicRepository, EpistemicRepositoryError, Frame,
    FrameAction, GraphRelation, MemoryItem, MemoryItemKind, MemoryItemSearch, NewBelief, NewClaim,
    NewGraphRelation, NewMemoryItem, PredicateDefinition, PredicateDefinitionStatus,
    PredicateStatus, Proposal, ReconciliationAction, ReconciliationDecision, RelationKind, Scope,
    Stance, Trace, TrustClass, normalize_registry_key, normalize_scope, propagate_stale_beliefs,
    reconcile_claim, scope_hash, scopes_overlap,
};
pub use ledger::{
    LedgerEvent, LedgerEventError, LedgerEventRepository, LedgerIngestResult,
    LedgerRepositoryError, LedgerRole,
};
pub use memory::{
    Memory, MemoryError, MemoryKind, MemoryRepository, MemoryRepositoryError, MemorySearchQuery,
    MemoryStatus, NewMemory,
};
pub use memory_path::{
    Episode, EpisodeError, EpisodeMarkerLink, EpisodeProjectLink, EpisodeTitle, Marker,
    MarkerError, MemoryPathRepository, MemoryPathRepositoryError, Project, ProjectError,
    ProjectLinkKind, ProjectName, ProjectStatus, SourceRange, SourceRangeError, StoreMarkerResult,
};
pub use receipt::{ExecutionReceipt, ReceiptChain, ReceiptDecision, ReceiptError, ReceiptOutcome};
pub use source::{
    NewSource, Source, SourceContent, SourceError, SourceFingerprint, SourceId, SourceKind,
    SourceRepository, SourceRepositoryError, SourceTitle, StoreSourceResult, find_all_matches,
};
pub use source_outline::{Heading, MarkdownOutline, parse_outline};

/// Lighting service version.
pub const LIGHTING_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Error returned when an identifier value is invalid.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum IdentifierError {
    #[error("identifier cannot be empty")]
    Empty,
}

macro_rules! identifier_type {
    ($name:ident) => {
        /// Strongly typed domain identifier.
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates a new identifier, rejecting empty or whitespace-only values.
            pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(IdentifierError::Empty);
                }

                Ok(Self(value))
            }

            /// Returns the identifier as a string slice.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = IdentifierError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }
    };
}

identifier_type!(ConversationId);
identifier_type!(EpisodeId);
identifier_type!(MarkerId);
identifier_type!(ProjectId);
identifier_type!(TopicId);
identifier_type!(MemoryId);
identifier_type!(ClaimId);
identifier_type!(BeliefId);
identifier_type!(BeliefRevisionId);
identifier_type!(MemoryItemId);
identifier_type!(TraceId);
identifier_type!(ProposalId);
identifier_type!(ContextPackId);
identifier_type!(RelationId);
identifier_type!(ActorId);
identifier_type!(PrincipalId);
identifier_type!(AuthorityGrantId);
identifier_type!(AuthorityRevocationId);
identifier_type!(ActionId);
identifier_type!(ReceiptId);
identifier_type!(TrailId);
identifier_type!(ApprovalId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_reject_empty_values() {
        assert_eq!(ConversationId::new(""), Err(IdentifierError::Empty));
        assert_eq!(EpisodeId::new("   "), Err(IdentifierError::Empty));
        assert_eq!(MarkerId::new("\t"), Err(IdentifierError::Empty));
        assert_eq!(ProjectId::new("\n"), Err(IdentifierError::Empty));
        assert_eq!(TopicId::new(""), Err(IdentifierError::Empty));
    }

    #[test]
    fn identifiers_display_original_values() {
        let id = ConversationId::new("conversation-1").unwrap();

        assert_eq!(id.as_str(), "conversation-1");
        assert_eq!(id.to_string(), "conversation-1");
    }

    #[test]
    fn identifiers_serialize_as_strings() {
        let id = TopicId::new("topic-1").unwrap();

        let json = serde_json::to_string(&id).unwrap();

        assert_eq!(json, "\"topic-1\"");
    }

    #[test]
    fn lighting_version_matches_cargo_version() {
        assert_eq!(LIGHTING_VERSION, env!("CARGO_PKG_VERSION"));
    }
}
