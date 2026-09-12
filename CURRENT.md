# Lantern Keeper - Current Phase

## Foundation modernisation and first useful memory loop

The authoritative pre-memory Luna baseline is preserved at commit
407c52934de8fe4c583c7ed549e51d1de45c7ce3 under tag
lantern-pre-memory-checkpoint. This implementation runs in the separate
foundation/lantern-pre-memory worktree; the original dirty checkout remains
untouched.

## Delivered in this pass

- Rust 1.98.1 stable, edition 2024, rustfmt and Clippy pinned in
  rust-toolchain.toml.
- Direct and transitive dependencies refreshed with Cargo.lock committed in
  the modernisation commit, with SurrealDB exactly pinned to 3.3.0-beta.3.
- Embedded versioned SurrealKV is the default local backend with sync=every.
  Remote WebSocket storage remains opt-in for existing integration tests.
- Existing Source/Project/Episode/Marker data remains logically addressable;
  no .lighting-data store existed in the preserved baseline, so there was no
  meaningful local database migration to perform.
- A portable export command writes manifest.json plus ledger, memory,
  relation and project NDJSON files.
- A small Memory Ledger/Living Memory substrate supports derived content,
  provenance references, optional observation time, as-of queries, explicit
  evolution relationships and bounded multi-hop lineage.
- Host-neutral append-only ledger events with raw-payload retention, HTTP
  ingestion and replay-safe `ledger-ingest` CLI support.
- CLI and HTTP capture, recall, context and supersede operations.
- Initial embedded qualification and memory repository integration tests.
- Executable LanternBench v1 runner, representative event fixture, and
  architecture/qualification documentation.
- A validated 2026-09-12 Basic Memory Cloud snapshot containing 61 notes,
  plus a replay-safe `basic-memory-import` command. The checkpoint extends
  the importer to emit observation and relation evidence, but that extension
  has not yet been rerun against the local store.

## Retrieval boundary

The first retrieval slice is intentionally inspectable: phrase matching,
project/status/as-of filters, importance, confidence and known-at ordering.
Recall returns a trace containing query, channel and candidate/selected IDs. An
empty result is an explicit abstention.

Not yet implemented: note-level reconciliation/promotion, BM25/vector/graph
fusion, persisted retrieval traces, activation metadata, automated
contradiction resolution, proposals/gardening, native host conversation
adapters, watermark cursors, restore validation, and MCP. These are deliberate
follow-up work, not silently implied by the current code.

## Verification

The exact qualification lane passes:

    cargo test -p lighting-store-surreal --test memory_repository --locked
    cargo test -p lighting-store-surreal --test surrealkv_qualification --locked

The workspace compile lane passes:

    cargo check --workspace --all-targets --all-features --locked

The workspace Clippy gate and skip-remote workspace test lane also pass; the
exact packet-level result is recorded in docs/AMBIENT_MEMORY_QUALIFICATION.md.
