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

The clean restart validation now passes in the recovery environment with Rust
1.98.1 and SurrealDB integration deliberately skipped:

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
LIGHTING_SKIP_INTEGRATION_TESTS=1 cargo test --workspace
```

All available tests pass; the live Tethers-engine test remains intentionally
ignored until a Tethers engine binary is configured. Windows live SurrealDB
validation remains a workstation check.

## Task Log — LK-027 through LK-038

LK-027 through LK-038 implement the file-capture, revision-history, revision-safe retrieval, and project-file-linking features that complete the project-file memory loop.

### Commits

- `bb619b7` — Add project file linking (`project-add-file`)
- `b0f2bee` — Add Cline task guardrails (`.clinerules`)

### Test Validation

The recovered baseline has been revalidated with Rust 1.98.1: all available non-live workspace tests pass. The external Tethers-engine test remains intentionally ignored, and live Windows/SurrealDB validation remains a workstation check.

### Workflow Delivered

The `lighting source-add`, `lighting source-history`, `lighting project-add-file`, `lighting project-handoff`, and `lighting project-record-result` CLI commands compose the complete project-file memory loop documented in `docs/lk-039-project-file-memory-loop.md`.

## Next Phase

Minimum memory foundation and first capability surface. Lantern Keeper should
not implement Tethers runtime logic; it should expose a small set of public
capabilities that the Tethers runtime can later plan and call.

Immediate Lantern Keeper work:

1. Inspect the existing SurrealDB schema and repository traits against the five
   durable concepts: Project, Source, Episode, Memory, and Link.
2. Define the smallest durable `Memory` representation and state model needed
   for `active`, `superseded`, and `archived` memory outcomes, with provenance
   back to Source/Episode evidence.
3. Define the first public capability/API surface around
   `lantern.context.retrieve`, `lantern.episode.record`,
   `lantern.memory.propose`, `lantern.memory.get`, and
   `lantern.memory.search`, without exposing raw database writes.
4. Keep retrieval bounded and mechanical: project/state filters, exact IDs and
   terms, full-text/recent/graph candidates, deterministic ranking, stable
   tie-breaks, and a fixed context-pack shape before any optional AI reranking
   or embeddings.
5. Keep Minimalist, Living Memory, and Archivist as configuration profiles over
   one pipeline, not separate implementations.

## Tethers Preview Integration (Preview-Only)

- `lighting_service::tethers_preview` — typed request/response DTOs matching
  the frozen Tethers 0.1 JSON protocol.
- `lighting_service::tethers_engine_client` — async client that spawns the
  OCaml Tethers engine, sends a newline-delimited JSON request, and returns a
  typed `TethersResponse`.
- The connection is **preview-only**: it evaluates a `lantern.project_result_preview_requested`
  event and returns a plan, but does **not** execute Actions, access storage, or
  write to Lantern Keeper.
- Engine binary is configured via `TETHERS_ENGINE_PATH`; evaluation times out
  after 10 seconds by default with child-process termination and reaping.
- **Preview endpoint:** `POST /api/v1/projects/{project_id}/tethers/preview`
  accepts `{task, changed_files, evaluation_id, event_id}`, validates the
  Project via ProjectService, interacts with the Tethers engine, and returns
  the complete typed response (matched, not_matched, or error) — without
  executing any Actions.
- `TETHERS_ENGINE_PATH` is optional until the endpoint is used; Lantern Keeper
  starts normally without it.

## Next Verified Step

Confirm the Windows checkout contains no newer local-only work, then begin the
canonical Memory model and reconciliation slice. Do not treat the existing
`memory_path` module as the canonical Memory entity.
    cargo check --workspace --all-targets --all-features --locked

Full test and Clippy results belong in the final acceptance report after the
remaining cleanup pass.
