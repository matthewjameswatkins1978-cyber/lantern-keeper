# Lantern Keeper - Current Phase

## Foundation modernisation complete; import accounting checkpoint complete

The authoritative pre-memory Luna baseline is preserved at commit
407c52934de8fe4c583c7ed549e51d1de45c7ce3 under tag
lantern-pre-memory-checkpoint. This implementation runs in the separate
foundation/lantern-pre-memory worktree; the original dirty checkout remains
untouched.

The latest verified import-accounting implementation is commit
bbe26b7bd1fb037a6fe22c8c11de0608b873e3e5 on `feature/lantern-full-move`.

## Delivered in this pass

- Rust 1.98.1 stable, edition 2024, rustfmt and Clippy pinned in
  rust-toolchain.toml.
- Cargo.lock is committed and the supported SurrealDB lane is exactly matched:
  Rust client and local server 3.3.0-beta.4.
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
  the importer to emit observation and relation evidence.
- Typed canonical epistemic records: Actor, Claim, Belief, soft Memory Item,
  Trace, Proposal, Predicate/Dimension definitions, Context Pack, and graph
  relations. Claims retain framing and attribution; stale Beliefs remain
  explicitly separate from truth state.
- Durable SurrealDB stores and HTTP capability routes for Claim capture,
  Belief projection/listing/stale invalidation, soft-memory capture/search,
  and unresolved relation inspection.
- `lighting doctor --json` reports the connected server version, schema version,
  storage configuration and an explicit OK/WARNING status without exposing
  credentials.
- Basic Memory import now creates reusable whole-note Episodes, 156 Claim
  candidates, 299 soft Memory Items, and 191 unresolved relation records in
  the local store from the preserved snapshot. A second pass is replay-safe.
- The repaired Basic Memory snapshot now has a deterministic offline accounting
  report: all 496 observations and 275 relations are accounted for, with zero
  unexplained items. Explicit Source-only, historical, metadata, unresolved and
  unsupported outcomes are retained rather than silently omitted.

## Retrieval boundary

The first retrieval slice is intentionally inspectable: phrase matching,
project/status/as-of filters, importance, confidence and known-at ordering.
Recall returns a trace containing query, channel and candidate/selected IDs. An
empty result is an explicit abstention.

Not yet implemented: clean-database import replay proof, canonical predicate reconciliation/promotion into
Beliefs, BM25/vector/graph fusion, persisted context traces, automated
contradiction resolution, proposal review commands, narrative rebuilds,
native host conversation adapters, restore validation, and MCP. These remain
genuine follow-up gates, not silently implied by the current code.

## Verification

The exact qualification lane passes:

    cargo test -p lighting-store-surreal --test memory_repository --locked
    cargo test -p lighting-store-surreal --test surrealkv_qualification --locked

The workspace compile lane passes:

    cargo check --workspace --all-targets --all-features --locked

The workspace Clippy gate and skip-remote workspace test lane also pass; the
exact packet-level result is recorded in docs/AMBIENT_MEMORY_QUALIFICATION.md.

The new epistemic slice additionally passes core unit tests, workspace check,
workspace all-features Clippy, live embedded capture/retrieval/stale checks,
and two Basic Memory importer passes. The final logical export contains the
new epistemic and relation tables. Cutover has not been performed.

The modernisation qualification passes the complete credentialed remote suite
against a fresh SurrealDB 3.3.0-beta.4 in-memory server: 215 passed, 0 failed,
1 ignored. The embedded SurrealKV qualification also passes. A clean 3.2.4
fallback compile/control was exercised; beta.4 was selected because its
matched live lane passed and its graph/relation capabilities serve the next
product stages. The dependency audit findings and their rationale are in
`docs/dependency-modernisation-2026-09-12.md`.
