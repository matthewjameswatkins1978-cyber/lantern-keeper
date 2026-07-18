# Lantern Keeper — Current Phase

## Phase

1 — source storage

## Current Task

LK-003: SurrealDB-backed Source repository with integration tests

## Acceptance Criteria

- `SourceRepository` trait and `Source` repository result/error types live in
  `lighting-core` without SurrealDB or HTTP dependencies.
- `SurrealSourceRepository` in `lighting-store-surreal` implements `store` and
  `get` for `Source`, returning `StoreSourceResult::Stored` or
  `StoreSourceResult::Duplicate`.
- Schema migration V1 defines a `source` table with a unique fingerprint index
  and a `__lighting_schema` version record.
- Duplicate detection returns the existing `SourceId` without creating a second
  record.
- Exact bytes/characters of `SourceContent` round-trip through the database,
  including mixed line endings and trailing whitespace.
- Real integration tests run against a live SurrealDB 3.2.1 engine in a
  disposable test namespace/database, and can be skipped with
  `LIGHTING_SKIP_INTEGRATION_TESTS=1`.
- `cargo fmt --check` passes.
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- `cargo test --workspace` passes with SurrealDB running.
- Documentation tells a human how to run the integration tests and how to skip
  them.
- No episode, marker, project, retrieval, context, embeddings, AI processing,
  MCP, cloud, GUI, or importer work is implemented yet.

## Next Verified Step

LK-004: In-memory Source repository and service wiring
