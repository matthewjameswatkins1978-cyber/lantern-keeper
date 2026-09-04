# Lantern Keeper Foundation Decisions

## 2026-09 foundation modernisation

- The development toolchain is pinned to Rust 1.98.1 stable with Edition 2024,
  rustfmt, and Clippy.
- The workspace resolver is 3 and the repository URL points to the canonical
  GitHub repository.
- SurrealDB is pinned exactly to `3.3.0-beta.3`, with `kv-surrealkv` and
  `protocol-ws` enabled. The beta is deliberate because it provides the
  embedded/storage and retrieval direction we want to investigate.
- Embedded SurrealKV is the default local backend. It is opened through the
  dynamic SDK engine with `versioned=true` and `sync=every`; the dynamic engine
  keeps the optional remote WebSocket backend behind the same repository
  boundary.
- Remote WebSocket storage is retained for disposable integration tests only.
  Integration test configuration explicitly selects `remote-surreal`, so the
  test suite cannot silently write into the normal embedded store.
- SurrealKV's physical version history is diagnostic infrastructure. Semantic
  temporal fields and provenance belong to Lantern's own memory schema.

## Deferred deliberately

Portable export, abrupt-termination qualification, vector/full-text smoke
tests, and the Memory Ledger/Living Memory schema are separate milestones. No
beta storage claim is allowed to outrun those tests and the escape hatch.
