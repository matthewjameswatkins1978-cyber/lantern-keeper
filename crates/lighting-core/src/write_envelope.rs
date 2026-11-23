//! Keyed write envelope for the hosted seam (LK-HOST-001 v1.1, M1 unification).
//!
//! The hosted profile must accept **Source bytes, Episode, Claim, projection
//! revision, stale closure, idempotency record and receipt inside one
//! all-or-nothing boundary**. The native store cannot do that today: its capture
//! path commits each stage separately, so a crash or refusal between stages
//! leaves an accepted Claim with no Belief and no receipt linking them.
//!
//! This module adds the envelope *type* and its contract. It deliberately does
//! not change the native path. The native mapping lives in
//! `lighting_store_surreal::NativeStagedEnvelopeWriter`, which records the
//! retained partial-commit risk explicitly instead of pretending atomicity.
//!
//! Idempotency reuses the shape MiMo introduced for keyed corrections in
//! `f76ccf3`: composite key `principal|scope_hash|caller_key`, payload hash over
//! a canonical envelope, and a `CorrectionIdempotencyRecord` receipt. There is
//! exactly one key shape and one receipt record; this module adds no second one.
//! The revision precondition also echoes his contract: the expected revision is
//! the belief's `updated_at` as a JSON string.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{Belief, BeliefRevision, Claim, Episode, Scope, Source, Trace, scope_hash};

/// Version of this envelope contract.
///
/// Follows the `EXPORT_FORMAT` precedent: a public string constant, recorded in
/// every receipt and refused on mismatch when a receipt is replayed. There is no
/// persisted version column until a v2 actually exists.
pub const WRITE_ENVELOPE_VERSION: &str = "lantern-keeper-write-envelope-v1";

/// Separator used by the composite idempotency key, matching keyed corrections.
const KEY_SEPARATOR: char = '|';

/// Build the composite idempotency key: `principal|scope_hash|caller_key`.
///
/// This is the same shape MiMo's keyed `record_correction` uses, so the same
/// caller key under the same principal and scope addresses the same boundary
/// whether it arrives as a correction or as a full envelope.
pub fn write_envelope_key(principal: &str, scope: &Scope, caller_key: &str) -> String {
    format!(
        "{principal}{KEY_SEPARATOR}{}{KEY_SEPARATOR}{caller_key}",
        scope_hash(scope)
    )
}

/// Typed nonacceptance for an envelope.
///
/// Every variant means "nothing was accepted"; none of them means "partially
/// applied". The native adapter additionally documents where it cannot honour
/// that, because it is made of several committed stages.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum WriteEnvelopeError {
    #[error("caller key must not be blank")]
    BlankKey,
    #[error("caller key must not contain the '{KEY_SEPARATOR}' separator")]
    KeySeparator,
    #[error("principal id must not be blank")]
    BlankPrincipal,
    #[error("envelope names no claim and no projection to write")]
    EmptyEnvelope,
    #[error("source bytes hash {declared} does not match the supplied source bytes {computed}")]
    SourceHashMismatch { declared: String, computed: String },
    #[error("idempotency key {key} was already used with a different envelope payload")]
    PayloadConflict { key: String },
    #[error("receipt for {key} was written by envelope version {found}, expected {expected}")]
    VersionMismatch {
        key: String,
        expected: String,
        found: String,
    },
    #[error("expected revision {expected:?} does not match the target revision {actual:?}")]
    RevisionConflict {
        expected: Option<String>,
        actual: Option<String>,
    },
    #[error("native stage {stage} was refused: {detail}")]
    StageRejected { stage: String, detail: String },
}

/// A complete keyed write request.
///
/// `source` and `episode` are optional so the same envelope type covers both a
/// full evidence capture and a pure projection transition, but a hosted adapter
/// must accept all stages that are present in one boundary.
#[derive(Clone, Debug)]
pub struct WriteEnvelope {
    pub principal_id: String,
    pub scope: Scope,
    pub caller_key: String,
    pub expected_revision: Option<String>,
    pub source: Option<Source>,
    pub episode: Option<Episode>,
    pub claim: Option<Claim>,
    pub previous: Option<Belief>,
    pub current: Option<Belief>,
    pub revisions: Vec<BeliefRevision>,
    pub trace: Trace,
    pub stale_beliefs: Vec<Belief>,
}

/// How an envelope was resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteEnvelopeOutcome {
    /// Every stage present in the envelope was accepted.
    Accepted { staged: usize },
    /// The same key and payload were seen before; the original receipt was reused.
    Replayed,
}

/// Receipt returned for an accepted or replayed envelope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteEnvelopeReceipt {
    pub key: String,
    pub payload_hash: String,
    pub version: String,
    pub outcome: WriteEnvelopeOutcome,
}

/// What the adapter records in the idempotency receipt so a replay can be
/// recognised and a version change refused without a schema change.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnvelopeReceiptRecord {
    pub envelope_version: String,
    pub key: String,
    pub payload_hash: String,
    pub staged: usize,
    pub accepted_at: DateTime<Utc>,
}

impl WriteEnvelope {
    /// Composite idempotency key for this envelope.
    pub fn key(&self) -> String {
        write_envelope_key(&self.principal_id, &self.scope, self.caller_key.trim())
    }

    /// SHA-256 over the exact supplied Source bytes, when a source is present.
    pub fn source_bytes_hash(&self) -> Option<String> {
        self.source
            .as_ref()
            .map(|source| sha256_hex(source.content().as_bytes()))
    }

    /// The `Source` fingerprint, when a source is present.
    pub fn declared_source_hash(&self) -> Option<String> {
        self.source
            .as_ref()
            .map(|source| source.fingerprint().as_str().to_owned())
    }

    /// Canonical payload hash binding the version and the whole envelope.
    pub fn payload_hash(&self) -> Result<String, WriteEnvelopeError> {
        let canonical = serde_json::json!({
            "envelope_version": WRITE_ENVELOPE_VERSION,
            "principal": self.principal_id,
            "scope_hash": scope_hash(&self.scope),
            "caller_key": self.caller_key.trim(),
            "expected_revision": self.expected_revision,
            "source": self.source.as_ref().map(|source| serde_json::json!({
                "id": source.id().as_str(),
                "fingerprint": source.fingerprint().as_str(),
                "bytes_hash": self.source_bytes_hash(),
            })),
            "episode": self.episode.as_ref().map(|episode| serde_json::json!({
                "id": episode.id().as_str(),
                "title": episode.title().as_str(),
            })),
            "claim": self.claim,
            "previous": self.previous,
            "current": self.current,
            "revisions": self.revisions,
            "trace": self.trace,
            "stale": self.stale_beliefs,
        });
        let encoded =
            serde_json::to_string(&canonical).map_err(|_| WriteEnvelopeError::EmptyEnvelope)?;
        Ok(sha256_hex(encoded.as_bytes()))
    }

    /// Validate the key, the identity and the source hash before any stage runs.
    pub fn validate(&self) -> Result<(), WriteEnvelopeError> {
        let caller_key = self.caller_key.trim();
        if caller_key.is_empty() {
            return Err(WriteEnvelopeError::BlankKey);
        }
        if caller_key.contains(KEY_SEPARATOR) {
            return Err(WriteEnvelopeError::KeySeparator);
        }
        if self.principal_id.trim().is_empty() {
            return Err(WriteEnvelopeError::BlankPrincipal);
        }
        // An evidence-only envelope (Source + Episode, no Claim) is legitimate:
        // migration and import capture evidence before any assertion exists. Only
        // a boundary with no stage at all is empty.
        if self.source.is_none()
            && self.episode.is_none()
            && self.claim.is_none()
            && self.previous.is_none()
            && self.current.is_none()
        {
            return Err(WriteEnvelopeError::EmptyEnvelope);
        }
        self.verify_source_hash()
    }

    /// Refuse a source whose declared fingerprint does not match its own bytes.
    pub fn verify_source_hash(&self) -> Result<(), WriteEnvelopeError> {
        match (self.source_bytes_hash(), self.declared_source_hash()) {
            (Some(computed), Some(declared)) if computed != declared => {
                Err(WriteEnvelopeError::SourceHashMismatch { declared, computed })
            }
            _ => Ok(()),
        }
    }

    /// The revision the projection currently claims, as the keyed-correction
    /// contract expresses it: `updated_at` serialised to a JSON string.
    pub fn target_revision(&self) -> Option<String> {
        self.previous
            .as_ref()
            .or(self.current.as_ref())
            .and_then(|belief| serde_json::to_value(belief.updated_at).ok())
            .and_then(|value| value.as_str().map(str::to_owned))
    }

    /// Number of stages this envelope asks an adapter to accept.
    pub fn stage_count(&self) -> usize {
        usize::from(self.source.is_some())
            + usize::from(self.episode.is_some())
            + usize::from(self.claim.is_some())
            + usize::from(self.current.is_some() || self.previous.is_some())
    }
}

/// Additive adapter contract for a keyed envelope.
#[async_trait::async_trait]
pub trait NativeWriteEnvelope: Send + Sync {
    /// Accept or replay one envelope.
    async fn apply_write_envelope(
        &self,
        envelope: WriteEnvelope,
    ) -> Result<WriteEnvelopeReceipt, WriteEnvelopeError>;
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EpisodeTitle, SourceRange};

    fn scope() -> Scope {
        Scope::from([("project".to_owned(), "workshop".to_owned())])
    }

    fn source() -> Source {
        Source::create(crate::NewSource {
            kind: crate::SourceKind::PlainText,
            title: crate::SourceTitle::new("envelope note".to_owned()).expect("title"),
            content: crate::SourceContent::new("exact bytes".to_owned()).expect("content"),
        })
    }

    fn envelope() -> WriteEnvelope {
        let source = source();
        let range = SourceRange::new(source.id().clone(), 0, 4, source.content()).expect("range");
        WriteEnvelope {
            principal_id: "matthew".to_owned(),
            scope: scope(),
            caller_key: "capture-1".to_owned(),
            expected_revision: None,
            source: Some(source),
            episode: Some(Episode::new(
                EpisodeTitle::new("envelope episode".to_owned()).expect("title"),
                range,
            )),
            claim: None,
            previous: None,
            current: None,
            revisions: Vec::new(),
            trace: Trace {
                id: crate::TraceId::new("trace-1").expect("trace id"),
                event_type: "envelope.test".to_owned(),
                actor_id: "matthew".to_owned(),
                subject_id: None,
                input_ids: Vec::new(),
                output_ids: Vec::new(),
                details: Default::default(),
                created_at: Utc::now(),
            },
            stale_beliefs: Vec::new(),
        }
    }

    #[test]
    fn key_shape_matches_the_keyed_correction_contract() {
        let envelope = envelope();
        let expected = format!("matthew|{}|capture-1", crate::scope_hash(&scope()));
        assert_eq!(envelope.key(), expected);
        assert_eq!(envelope.key().matches(KEY_SEPARATOR).count(), 2);
    }

    #[test]
    fn validation_refuses_blank_separator_and_empty_bodies() {
        let mut blank = envelope();
        blank.caller_key = "   ".to_owned();
        assert_eq!(blank.validate(), Err(WriteEnvelopeError::BlankKey));

        let mut separated = envelope();
        separated.caller_key = "a|b".to_owned();
        assert_eq!(separated.validate(), Err(WriteEnvelopeError::KeySeparator));

        let mut anonymous = envelope();
        anonymous.principal_id.clear();
        assert_eq!(
            anonymous.validate(),
            Err(WriteEnvelopeError::BlankPrincipal)
        );

        let mut empty = envelope();
        empty.source = None;
        empty.episode = None;
        assert_eq!(empty.validate(), Err(WriteEnvelopeError::EmptyEnvelope));
    }

    #[test]
    fn version_is_part_of_the_payload_hash() {
        let envelope = envelope();
        let hash = envelope.payload_hash().expect("hash");
        assert_eq!(hash.len(), 64);
        assert_eq!(hash, envelope.payload_hash().expect("deterministic"));
        assert!(WRITE_ENVELOPE_VERSION.ends_with("-v1"));
    }

    #[test]
    fn source_hash_check_is_independent_of_the_payload_hash() {
        let envelope = envelope();
        assert_eq!(envelope.validate(), Ok(()));
        assert_eq!(
            envelope.source_bytes_hash(),
            envelope.declared_source_hash(),
            "a Source fingerprint is the SHA-256 of its exact bytes"
        );
    }
}
