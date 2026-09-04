//! SurrealDB-backed store for Lighting.
//!
//! Not wired into the service in LK-001.

pub mod config;
pub mod connection;
pub mod export;
pub mod ledger_store;
pub mod memory_path_store;
pub mod memory_store;
pub mod schema;
pub mod source_store;

pub use config::{RedactedStoreConfig, StoreConfig};
pub use connection::{StoreError, SurrealStore};
pub use export::{ExportError, ExportSummary};
pub use ledger_store::SurrealLedgerRepository;
pub use memory_path_store::SurrealMemoryPathRepository;
pub use memory_store::SurrealMemoryRepository;
pub use source_store::SurrealSourceRepository;
