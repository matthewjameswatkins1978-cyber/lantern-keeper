//! Native mapping of the keyed write envelope (LK-HOST-001 v1.1, M1 unification).
//!
//! This adapter accepts a [`WriteEnvelope`] and maps it onto the **existing**
//! staged native calls, in their existing order: Source, Episode, Claim, then the
//! projection transition, then the idempotency receipt.
//!
//! ## Atomicity — retired by Q18, loudly
//!
//! v1.1 retained a staged native risk here (each stage its own committed
//! write: a failure after the Source left the Source committed, and so on).
//! Q18 correction-consistency removed it: this adapter now resolves the
//! dedupe lookups first and applies source, episode, claim, the
//! CAS-guarded projection transition (trace + revisions) and the receipt as
//! ONE transaction — a failure rolls every stage back together, proven by
//! `write_envelope_m1::native_envelope_transaction_rolls_back_every_stage`.
//! `NATIVE_PARTIAL_COMMIT_RISK` now reads RETIRED so old references notice.
//!
//! What this adapter guarantees: the key binding, the payload hash, the
//! version refusal on replay, the revision precondition before the first
//! write, and a single typed refusal for the transaction.

use async_trait::async_trait;
use chrono::Utc;
use lighting_core::write_envelope::sha256_hex;
use lighting_core::{
    Claim, CorrectionIdempotencyRecord, CorrectionWriteBatch, EnvelopeReceiptRecord,
    EpistemicRepository, NativeWriteEnvelope, Source, SourceRepository, WRITE_ENVELOPE_VERSION,
    WriteEnvelope, WriteEnvelopeError, WriteEnvelopeOutcome, WriteEnvelopeReceipt,
};

use crate::connection::SurrealStore;
use crate::epistemic_store::SurrealEpistemicRepository;
use crate::memory_path_store::SurrealMemoryPathRepository;
use crate::source_store::SurrealSourceRepository;

/// RETIRED by Q18 (correction-consistency): the native write set now applies
/// as ONE transaction. The symbol stays so existing references can be
/// checked for the retirement, loudly, instead of implying the old risk.
pub const NATIVE_PARTIAL_COMMIT_RISK: &str = "RETIRED by Q18: stages no longer commit separately — source, episode, claim, \
     projection and the receipt apply as one transaction and roll back together";

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

        // Q18 gap 5: dedupe lookups first, then the whole write set as ONE
        // transaction — this retires NATIVE_PARTIAL_COMMIT_RISK: no stage
        // commits on its own any more, and a failed stage rolls everything
        // back instead of leaving accepted earlier stages behind.
        let mut staged = 0usize;

        let mut source: Option<Source> = None;
        let mut source_exists = false;
        let mut source_previous_id: Option<String> = None;
        if let Some(attempted) = envelope.source.clone() {
            staged += 1;
            let fingerprint = attempted.fingerprint().as_str().to_owned();
            let bytes_hash = sha256_hex(attempted.content().as_bytes());
            if fingerprint != bytes_hash {
                return Err(WriteEnvelopeError::SourceHashMismatch {
                    declared: fingerprint,
                    computed: bytes_hash,
                });
            }
            if let Some(existing_id) = self
                .sources
                .get_by_fingerprint(&fingerprint)
                .await
                .map_err(|error| stage("source.lookup", error))?
            {
                let existing = self
                    .sources
                    .get(&existing_id)
                    .await
                    .map_err(|error| stage("source.lookup", error))?
                    .ok_or_else(|| stage("source.lookup", "deduplicated source missing"))?;
                source = Some(existing);
                source_exists = true;
            } else {
                let title = attempted.title().clone();
                let same_title = self
                    .sources
                    .list_all_by_kind_and_title(attempted.kind(), &title)
                    .await
                    .map_err(|error| stage("source.lookup", error))?;
                source_previous_id = same_title
                    .last()
                    .map(|existing| existing.id().as_str().to_owned());
                source = Some(attempted);
            }
        }

        let mut claim: Option<Claim> = None;
        let mut claim_exists = false;
        if let Some(attempted) = envelope.claim.clone() {
            staged += 1;
            if let Some(existing) = self
                .epistemic
                .get_claim_by_dedupe_key(&attempted.dedupe_key)
                .await
                .map_err(|error| stage("claim.lookup", error))?
            {
                claim = Some(existing);
                claim_exists = true;
            } else {
                claim = Some(attempted);
            }
        }

        let episode = envelope.episode.clone();
        if episode.is_some() {
            staged += 1;
        }
        let has_transition = envelope.current.is_some() || envelope.previous.is_some();
        if has_transition {
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
            .apply_correction_batch(CorrectionWriteBatch {
                source,
                source_exists,
                source_previous_id,
                episode,
                episode_exists: false,
                claim,
                claim_exists,
                previous: envelope.previous.clone(),
                current: envelope.current.clone(),
                revisions: envelope.revisions.clone(),
                trace: envelope.trace.clone(),
                stale_beliefs: envelope.stale_beliefs.clone(),
                expected_token: None,
                receipt: Some(CorrectionIdempotencyRecord {
                    key: key.clone(),
                    payload_hash: payload_hash.clone(),
                    result_json,
                    created_at: Utc::now(),
                }),
                write_trace: has_transition,
            })
            .await
            .map_err(|error| stage("envelope.transaction", error))?;
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
