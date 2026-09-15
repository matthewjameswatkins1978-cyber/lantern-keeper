//! Durable execution receipt and hash-chain persistence.

use lighting_core::{ExecutionReceipt, ReceiptChain};
use surrealdb::types::Object;
use thiserror::Error;

use crate::connection::SurrealStore;

#[derive(Debug, Error)]
pub enum SurrealReceiptError {
    #[error("receipt query failed: {0}")]
    Query(#[source] surrealdb::Error),
    #[error("receipt payload could not be encoded: {0}")]
    Encode(#[source] serde_json::Error),
    #[error("receipt payload could not be decoded: {0}")]
    Decode(#[source] serde_json::Error),
    #[error("receipt is invalid: {0}")]
    Invalid(String),
    #[error("receipt payload is missing")]
    MissingPayload,
}

#[derive(Clone)]
pub struct SurrealReceiptRepository {
    store: SurrealStore,
}

impl SurrealReceiptRepository {
    pub fn new(store: SurrealStore) -> Self {
        Self { store }
    }

    pub async fn migrate(&self) -> Result<(), SurrealReceiptError> {
        self.store
            .query(SCHEMA_MIGRATION_RECEIPTS)
            .await
            .map(|_| ())
            .map_err(SurrealReceiptError::Query)
    }

    pub async fn append(
        &self,
        receipt: ExecutionReceipt,
    ) -> Result<ExecutionReceipt, SurrealReceiptError> {
        receipt
            .verify()
            .map_err(|error| SurrealReceiptError::Invalid(error.to_string()))?;
        let existing: Option<Object> = self
            .store
            .query("SELECT * FROM receipt WHERE id = type::record('receipt', $id) LIMIT 1")
            .bind(("id", receipt.receipt_id.to_string()))
            .await
            .map_err(SurrealReceiptError::Query)?
            .take(0)
            .map_err(SurrealReceiptError::Query)?;
        if existing.is_some() {
            return Err(SurrealReceiptError::Invalid(
                "receipt identifier is duplicated".to_owned(),
            ));
        }

        let previous = receipt.previous_receipt_hash.clone();
        let count_records: Vec<Object> = self
            .store
            .query("SELECT count() FROM receipt GROUP ALL")
            .await
            .map_err(SurrealReceiptError::Query)?
            .take(0)
            .map_err(SurrealReceiptError::Query)?;
        let previous_count = count_records
            .into_iter()
            .next()
            .and_then(|object: Object| {
                object
                    .get("count")
                    .and_then(|value| value.clone().into_t::<i64>().ok())
            })
            .unwrap_or_default();
        if previous.is_none() && previous_count != 0 {
            return Err(SurrealReceiptError::Invalid(
                "first receipt cannot follow an existing chain".to_owned(),
            ));
        }
        if let Some(previous) = &previous {
            let predecessor: Option<Object> = self
                .store
                .query("SELECT id FROM receipt WHERE receipt_hash = $hash LIMIT 1")
                .bind(("hash", previous.clone()))
                .await
                .map_err(SurrealReceiptError::Query)?
                .take(0)
                .map_err(SurrealReceiptError::Query)?;
            if predecessor.is_none() {
                return Err(SurrealReceiptError::Invalid(
                    "receipt predecessor is not persisted".to_owned(),
                ));
            }
        }

        let payload = serde_json::to_string(&receipt).map_err(SurrealReceiptError::Encode)?;
        self.store
            .query(
                "CREATE receipt CONTENT { id: $id, payload: $payload, receipt_hash: $hash, previous_receipt_hash: $previous, decided_at: $decided_at };",
            )
            .bind(("id", receipt.receipt_id.to_string()))
            .bind(("payload", payload))
            .bind(("hash", receipt.receipt_hash.clone()))
            .bind(("previous", previous))
            .bind(("decided_at", receipt.decided_at))
            .await
            .map_err(SurrealReceiptError::Query)?;
        Ok(receipt)
    }

    pub async fn store(
        &self,
        receipt: ExecutionReceipt,
    ) -> Result<ExecutionReceipt, SurrealReceiptError> {
        self.append(receipt).await
    }

    pub async fn list(&self) -> Result<Vec<ExecutionReceipt>, SurrealReceiptError> {
        let records: Vec<Object> = self
            .store
            .query("SELECT * FROM receipt")
            .await
            .map_err(SurrealReceiptError::Query)?
            .take(0)
            .map_err(SurrealReceiptError::Query)?;
        records
            .into_iter()
            .map(|record| {
                let payload = record
                    .get("payload")
                    .and_then(|value| value.clone().into_t::<String>().ok())
                    .ok_or(SurrealReceiptError::MissingPayload)?;
                serde_json::from_str(&payload).map_err(SurrealReceiptError::Decode)
            })
            .collect()
    }

    pub async fn load_chain(&self) -> Result<ReceiptChain, SurrealReceiptError> {
        let mut remaining = self.list().await?;
        let mut chain = ReceiptChain::default();
        while !remaining.is_empty() {
            let previous = chain
                .receipts()
                .last()
                .map(|receipt| receipt.receipt_hash.as_str());
            let index = remaining
                .iter()
                .position(|receipt| receipt.previous_receipt_hash.as_deref() == previous);
            let Some(index) = index else {
                return Err(SurrealReceiptError::Invalid(
                    "persisted receipt chain is broken".to_owned(),
                ));
            };
            chain
                .append(remaining.swap_remove(index))
                .map_err(|error| SurrealReceiptError::Invalid(error.to_string()))?;
        }
        Ok(chain)
    }
}

const SCHEMA_MIGRATION_RECEIPTS: &str = r#"
DEFINE TABLE IF NOT EXISTS receipt SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS payload ON receipt TYPE string;
DEFINE FIELD IF NOT EXISTS receipt_hash ON receipt TYPE string;
DEFINE FIELD IF NOT EXISTS previous_receipt_hash ON receipt TYPE option<string>;
DEFINE FIELD IF NOT EXISTS decided_at ON receipt TYPE datetime;
DEFINE INDEX IF NOT EXISTS receipt_id ON receipt FIELDS id UNIQUE;
DEFINE INDEX IF NOT EXISTS receipt_hash_index ON receipt FIELDS receipt_hash UNIQUE;
UPSERT __lighting_schema:bootstrap CONTENT {
    project: "Lantern Keeper",
    service: "Lighting",
    schema_version: 10,
    updated_at: time::now()
};
"#;
