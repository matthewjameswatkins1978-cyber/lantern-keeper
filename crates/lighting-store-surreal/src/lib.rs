//! SurrealDB-backed store for Lighting.
//!
//! Not wired into the service in LK-001.

pub mod authority_store;
pub mod config;
pub mod connection;
pub mod epistemic_store;
pub mod export;
pub mod ledger_store;
pub mod memory_path_store;
pub mod memory_store;
pub mod receipt_store;
pub mod schema;
pub mod source_store;

pub use authority_store::{AuthorityRepository, SurrealAuthorityError, SurrealAuthorityRepository};
pub use config::{RedactedStoreConfig, StoreConfig};
pub use connection::{StoreError, SurrealStore};
pub use epistemic_store::{SurrealEpistemicError, SurrealEpistemicRepository};
pub use export::{ExportError, ExportSummary, RestoreSummary};
pub use ledger_store::SurrealLedgerRepository;
pub use memory_path_store::SurrealMemoryPathRepository;
pub use memory_store::SurrealMemoryRepository;
pub use receipt_store::{SurrealReceiptError, SurrealReceiptRepository};
pub use source_store::SurrealSourceRepository;
