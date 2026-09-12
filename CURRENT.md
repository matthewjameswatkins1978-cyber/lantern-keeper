# Lantern Keeper - Current Phase

**Baseline reset:** 12 September 2026

**Canonical trunk:** `master`

**Recovered implementation:** `407c529` (`agent/lighting-source-api`)

## Phase

Joint Tethers/Lantern Keeper foundation: preserve the completed
source-backed memory loop and prepare Lantern Keeper's minimum memory
foundation for later Tethers runtime integration.

## Current Task

Install the canonical joint architecture, keep the existing proof honest, and
prepare the next Lantern Keeper task: a minimal Project/Source/Episode/Memory/
Link foundation and first small capability surface for the later Tethers
runtime slice.

Canonical architecture:
[`docs/architecture/TETHERS_LANTERN_KEEPER_CANONICAL_ARCHITECTURE.md`](docs/architecture/TETHERS_LANTERN_KEEPER_CANONICAL_ARCHITECTURE.md)

## Verified Workflow

Lantern Keeper now proves one complete loop through the real system:

```text
Capture project knowledge
-> store it as canonical Source-backed memory
-> retrieve Project context
-> format a Codex handoff
-> record the completed result as canonical memory
-> retrieve the updated Project handoff
```

## Implemented Now

- Exact Source storage and retrieval with fingerprint duplicate detection.
- Episodes as UTF-8-safe byte ranges into authoritative Sources.
- Projects and Markers stored through the memory-path repository.
- Episode-to-Project and Episode-to-Marker links through native SurrealDB relation tables.
- Marker-led retrieval with exact excerpts and deterministic provenance.
- Project-led retrieval with a deterministic Codex `context_package`.
- `lighting project-handoff <project-id>` for paste-ready Codex context.
- `lighting project-record-result <project-id> <result-file> --title "..."`
  for writing completed work back into the same Project.
- A full local proof script that demonstrates the loop using public CLI/API paths.

## Validation

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
