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
  provenance references, temporal fields, confidence, importance and explicit
  supersession.
- CLI and HTTP capture, recall, context and supersede operations.
- Initial embedded qualification and memory repository integration tests.
- LanternBench v1 fixture and architecture/qualification documentation.

## Retrieval boundary

The first retrieval slice is intentionally inspectable: phrase matching,
project/status filters, importance, confidence and known-at ordering. Recall
returns a trace containing query, channel and candidate/selected IDs. An empty
result is an explicit abstention.

Not yet implemented: BM25/vector/graph fusion, persisted retrieval traces,
activation metadata, automated contradiction resolution, the gardener,
automatic conversation ingestion, MCP, and a benchmark runner. These are
deliberate follow-up work, not silently implied by the current code.

## Verification

The exact qualification lane passes:

    cargo test -p lighting-store-surreal --test memory_repository --locked
    cargo test -p lighting-store-surreal --test surrealkv_qualification --locked

The workspace compile lane passes:

    cargo check --workspace --all-targets --all-features --locked

Full test and Clippy results belong in the final acceptance report after the
remaining cleanup pass.
