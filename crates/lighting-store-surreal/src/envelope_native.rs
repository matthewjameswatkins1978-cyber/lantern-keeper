//! Native mapping of the keyed write envelope (LK-HOST-001 v1.1, M1 unification).
//!
//! This adapter accepts a [`WriteEnvelope`] and maps it onto the **existing**
//! staged native calls, in their existing order: Source, Episode, Claim, then the
//! projection transition, then the idempotency receipt.
//!
//! ## Retained native risk — stated, not hidden
//!
//! Each of those stages is its own committed write in the native store. This
//! adapter therefore cannot offer the hosted contract's all-or-nothing boundary,
//! and it does not pretend to:
//!
//! * a failure after the Source is stored leaves the Source committed;
//! * a failure after the Claim is stored leaves a Claim with no Belief;
//! * the receipt is written last, so a crash before it leaves accepted stages
//!   with no receipt linking them, and a retry re-stages what it can.
//!
//! That is the exact partial-commit risk v1.1 names, deliberately retained
//! natively and recorded here (see `NATIVE_PARTIAL_COMMIT_RISK`) rather than
//! silently changed. The D1 adapter must close it with one transaction and a
//! database-enforced precondition; the requirement note lives in
//! `docs/hosted-m1-d1-atomicity.md`.
//!
//! What this adapter *does* guarantee: the key binding, the payload hash, the
//! version refusal on replay, the revision precondition before the first write,
//! and a typed refusal for every stage.

use async_trait::async_trait;
use chrono::Utc;
use lighting_core::write_envelope::sha256_hex;
use lighting_core::{
    CorrectionIdempotencyRecord, EnvelopeReceiptRecord, EpistemicRepository, MemoryPathRepository,
    NativeWriteEnvelope, SourceRepository, WRITE_ENVELOPE_VERSION, WriteEnvelope,
    WriteEnvelopeError, WriteEnvelopeOutcome, WriteEnvelopeReceipt,
};

use crate::connection::SurrealStore;
use crate::epistemic_store::SurrealEpistemicRepository;
use crate::memory_path_store::SurrealMemoryPathRepository;
use crate::source_store::SurrealSourceRepository;

/// Recorded verbatim so a reviewer of this adapter cannot miss it.
pub const NATIVE_PARTIAL_COMMIT_RISK: &str = "stages are separately committed in native order; a failure or crash between them leaves \
     earlier stages accepted with no receipt, and no rollback is attempted";

/// Keys a whole write boundary across the native repositories.
///
/// Not `Clone`: two staged repositories are not cloneable, and a writer should be
/// a single owned handle rather than an accidentally duplicated one.
pub struct NativeStagedEnvelopeWriter {
    sources: SurrealSourceRepository,
    memory_path: SurrealMemoryPathRepository,
    epistemic: SurrealEpistemicRepository,
}

impl NativeStagedEnvelopeWriter {
    pub fn new(store: SurrealStore) -> Self {
        Self {
            sources: SurrealSourceRepository::new(store.clone()),
            memory_path: SurrealMemoryPathRepository::new(store.clone()),
            epistemic: SurrealEpistemicRepository::new(store),
        }
    }

    /// Create the tables each staged repository needs.
    pub async fn migrate(&self) -> Result<(), WriteEnvelopeError> {
        self.sources
            .migrate()
            .await
            .map_err(|error| stage("source.migrate", error))?;
        self.memory_path
            .migrate()
            .await
            .map_err(|error| stage("episode.migrate", error))?;
        self.epistemic
            .migrate()
            .await
            .map_err(|error| stage("claim.migrate", error))?;
        Ok(())
    }
}

#[async_trait]
impl NativeWriteEnvelope for NativeStagedEnvelopeWriter {
    async fn apply_write_envelope(
        &self,
        envelope: WriteEnvelope,
    ) -> Result<WriteEnvelopeReceipt, WriteEnvelopeError> {
        envelope.validate()?;
        let key = envelope.key();
        let payload_hash = envelope
            .payload_hash()
            .map_err(|_| WriteEnvelopeError::EmptyEnvelope)?;

        // Replay detection reuses the keyed-correction receipt store: one key
        // shape, one receipt record, no second mechanism.
        if let Some(record) = self
            .epistemic
            .get_correction_idempotency(&key)
            .await
            .map_err(|error| stage("receipt.read", error))?
        {
            let stored: EnvelopeReceiptRecord =
                serde_json::from_str(&record.result_json).map_err(|_| {
                    WriteEnvelopeError::VersionMismatch {
                        key: key.clone(),
                        expected: WRITE_ENVELOPE_VERSION.to_owned(),
                        found: "<unreadable receipt>".to_owned(),
                    }
                })?;
            if stored.envelope_version != WRITE_ENVELOPE_VERSION {
                return Err(WriteEnvelopeError::VersionMismatch {
                    key,
                    expected: WRITE_ENVELOPE_VERSION.to_owned(),
                    found: stored.envelope_version,
                });
            }
            if stored.payload_hash != payload_hash || record.payload_hash != payload_hash {
                return Err(WriteEnvelopeError::PayloadConflict { key });
            }
            return Ok(WriteEnvelopeReceipt {
                key,
                payload_hash,
                version: WRITE_ENVELOPE_VERSION.to_owned(),
                outcome: WriteEnvelopeOutcome::Replayed,
            });
        }

        // Revision precondition strictly before the first write, so a mismatch
        // persists nothing at all.
        if let Some(expected) = envelope.expected_revision.as_deref() {
            let actual = envelope.target_revision();
            if actual.as_deref() != Some(expected) {
                return Err(WriteEnvelopeError::RevisionConflict {
                    expected: Some(expected.to_owned()),
                    actual,
                });
            }
        }

        let mut staged = 0usize;

        if let Some(source) = envelope.source.clone() {
            let fingerprint = source.fingerprint().as_str().to_owned();
            let bytes_hash = sha256_hex(source.content().as_bytes());
            if fingerprint != bytes_hash {
                return Err(WriteEnvelopeError::SourceHashMismatch {
                    declared: fingerprint,
                    computed: bytes_hash,
                });
            }
            self.sources
                .store(source)
                .await
                .map_err(|error| stage("source.store", error))?;
            staged += 1;
        }

        if let Some(episode) = envelope.episode.clone() {
            self.memory_path
                .create_episode(episode)
                .await
                .map_err(|error| stage("episode.create", error))?;
            staged += 1;
        }

        if let Some(claim) = envelope.claim.clone() {
            self.epistemic
                .store_claim(claim)
                .await
                .map_err(|error| stage("claim.store", error))?;
            staged += 1;
        }

        if envelope.current.is_some() || envelope.previous.is_some() {
            self.epistemic
                .apply_belief_transition(
                    envelope.previous.clone(),
                    envelope.current.clone(),
                    envelope.revisions.clone(),
                    envelope.trace.clone(),
                    envelope.stale_beliefs.clone(),
                )
                .await
                .map_err(|error| stage("projection.transition", error))?;
            staged += 1;
        }

        let receipt = EnvelopeReceiptRecord {
            envelope_version: WRITE_ENVELOPE_VERSION.to_owned(),
            key: key.clone(),
            payload_hash: payload_hash.clone(),
            staged,
            accepted_at: Utc::now(),
        };
        let result_json =
            serde_json::to_string(&receipt).map_err(|error| stage("receipt.encode", error))?;
        self.epistemic
            .store_correction_idempotency(CorrectionIdempotencyRecord {
                key: key.clone(),
                payload_hash: payload_hash.clone(),
                result_json,
                created_at: Utc::now(),
            })
            .await
            .map_err(|error| stage("receipt.store", error))?;

        Ok(WriteEnvelopeReceipt {
            key,
            payload_hash,
            version: WRITE_ENVELOPE_VERSION.to_owned(),
            outcome: WriteEnvelopeOutcome::Accepted { staged },
        })
    }
}

fn stage(stage: &str, error: impl std::fmt::Display) -> WriteEnvelopeError {
    WriteEnvelopeError::StageRejected {
        stage: stage.to_owned(),
        detail: error.to_string(),
    }
}
