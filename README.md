# Lantern Keeper

Lantern Keeper is a local-first shared memory project for future ChatGPT and
Codex workflows.

The joint architectural contract and build foundation for Lantern Keeper and
Tethers is
[`docs/architecture/TETHERS_LANTERN_KEEPER_CANONICAL_ARCHITECTURE.md`](docs/architecture/TETHERS_LANTERN_KEEPER_CANONICAL_ARCHITECTURE.md).
It defines the accepted target architecture: Lantern Keeper remembers, Tethers
coordinates, AI interprets through explicit capabilities, and Matthew remains
the final authority.

Lighting is the local service inside Lantern Keeper. This repository contains
the smallest runnable proof for the first vertical slice:

```text
Markdown Source
-> Episode
-> Marker + Project links
-> deterministic retrieval with exact Source excerpts
-> Codex handoff
-> recorded result writeback
-> updated Project handoff
```

This is the hackathon proof: Lantern Keeper preserves exact project knowledge,
retrieves the right source-backed context for Codex, and records completed work
back into the same durable Project memory.

The proof is not the complete target memory system. Lantern Keeper remains the
memory system, not the workflow engine; Tethers will coordinate Lantern Keeper
through public Lantern Keeper capabilities rather than embedding Tethers runtime
logic inside Lantern Keeper.

## Current Status

Implemented now:

- Rust Cargo workspace with the intended inward dependency direction.
- `lighting` binary with `version` and `serve` subcommands.
- Localhost-only HTTP service with:
  - `GET /health/live` — returns `{ "alive": true }`
  - `GET /health/ready` — returns 200 only after SurrealDB connection and schema migration
  - `GET /api/v1/version` — returns service name, project, and version
  - `POST /api/v1/sources` — stores a Source; returns 201 (stored) or 200 (duplicate)
  - `GET /api/v1/sources/{source_id}` — returns exact Source by ID
  - `POST /api/v1/projects/{project_id}/record-result` — records a result file as Source-backed Project memory
  - `POST /api/v1/retrieval/projects` — returns a deterministic Codex handoff context package
- `lighting-core` with the `Source` domain, `SourceRepository` trait, and no
  SurrealDB or HTTP dependency.
- `lighting-store-surreal` with `SurrealSourceRepository`, schema migration V1,
  and isolated integration tests against a real SurrealDB engine.
- `lighting-service` with application-layer Source operations, HTTP routes,
  request validation, request-size limiting, and honest readiness.
- Automated tests for service routes that do not need SurrealDB (stub-based).
- Live SurrealDB API integration tests with strict DB isolation.
- CLI commands for Source add/show, marker retrieval, Project handoff, and
  Project result writeback.
- Full-loop local demo using public CLI/API paths only.
- `scripts/validate.ps1` to run formatting, Clippy, tests, and `lighting version`.
- Configuration example in `config/example.toml`.

Not implemented yet:

- Tethers runtime execution through Lantern Keeper capabilities
- Memory proposal, judgement, merging, strengthening, superseding, and archival
  state transitions
- Bounded ranked retrieval with the final context-pack shape
- Embeddings, AI processing, MCP provider work, cloud services, GUI, task
  automation, or importer work
- Automatic conversation ingestion
- Automatic episode detection
- Universal importers, ranking, recommendations, or broad graph expansion

## Prerequisites

- Rust stable toolchain with Cargo, rustfmt, and Clippy.
- Visual Studio Build Tools with the MSVC C++ toolchain (Windows).
- SurrealDB 3.2.1 or later.

Detected local options (on the original workstation):

- `surreal` 3.2.1 for Windows on x86_64 is installed.
- Docker/Podman were not detected.

## Build And Check

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Test

Run all tests with a live SurrealDB 3.2.1 instance on `ws://127.0.0.1:8000`:

```powershell
cargo test --workspace
```

Start SurrealDB locally before running the integration tests:

```powershell
.\scripts\start-surreal.ps1
```

Run tests without a live database:

```powershell
cmd /v /c "set LIGHTING_SKIP_INTEGRATION_TESTS=1&& cargo test --workspace"
```

The `cmd /v /c` form is required on Windows so the environment variable is
set in the same process that runs Cargo.

Run only the source repository integration tests:

```powershell
cargo test --package lighting-store-surreal --test source_repository -- --test-threads=1
```

Run only the live API integration tests:

```powershell
cargo test --package lighting-service --test source_api_integration -- --test-threads=1
```

The integration tests create a fresh database in a `lighting_test` namespace
for each test run and are executed single-threaded so they do not collide.

## Configuration

Copy `config/example.toml` to `config/local.toml` and edit values for your
workstation. Do not commit files containing secrets.

Create a `.env` file from `.env.example`:

```powershell
Copy-Item .env.example .env
```

The `.env` file contains SurrealDB credentials and Lighting bind settings.
Defaults in `.env.example` work out of the box with `scripts/start-surreal.ps1`.

## Start SurrealDB

```powershell
.\scripts\start-surreal.ps1
```

SurrealDB starts on `ws://127.0.0.1:8000` with credentials root/root.

Check SurrealDB status:

```powershell
.\scripts\check-surreal.ps1
```

## Start Lighting

Print the version:

```powershell
cargo run -p lighting -- version
```

Start the local service (requires a running SurrealDB):

```powershell
cargo run -p lighting -- serve
```

Default service address:

```text
127.0.0.1:4317
```

Override with:

```powershell
$env:LIGHTING_HOST = "127.0.0.1"
$env:LIGHTING_PORT = "4317"
```

Lighting rejects non-localhost bind addresses.

## Check The Service

### Readiness

```powershell
Invoke-RestMethod http://127.0.0.1:4317/health/ready
```

Expected response when storage is connected:

```json
{ "ready": true, "reason": "durable Source storage is ready" }
```

When SurrealDB is not connected, the endpoint returns 503 with
`"storage is not configured"`.

### Liveness

```powershell
Invoke-RestMethod http://127.0.0.1:4317/health/live
```

Expected response:

```json
{ "alive": true }
```

### Version

```powershell
Invoke-RestMethod http://127.0.0.1:4317/api/v1/version
```

Expected response:

```json
{
  "service": "Lighting",
  "project": "Lantern Keeper",
  "version": "0.1.0"
}
```

## Marker API

### Create a Marker

```powershell
$body = @{ text = "human network cable" } | ConvertTo-Json
Invoke-RestMethod -Uri http://127.0.0.1:4317/api/v1/markers -Method Post -Body $body -ContentType "application/json"
```

Expected response (new Marker, HTTP 201):

```json
{
  "marker_id": "<uuid>",
  "display_text": "human network cable",
  "lookup_key": "human network cable",
  "created_at": "2026-07-18T13:00:00Z"
}
```

Expected response (normalised duplicate, HTTP 200): same body as above, same ID.

### Get a Marker by ID

```powershell
Invoke-RestMethod http://127.0.0.1:4317/api/v1/markers/<marker_id>
```

Expected response (HTTP 200): same Marker shape as above.

### Lookup a Marker by Phrase

```powershell
Invoke-RestMethod "http://127.0.0.1:4317/api/v1/markers/lookup?text=HUMAN+network+cable"
```

Lookup normalises casing and whitespace. Expected response (HTTP 200): the matching Marker, or 404 if none found.

## Source API

### Post a Small Markdown Source

```powershell
$body = @{
  title   = "Lantern Keeper handbook"
  kind    = "markdown"
  content = "# Lantern Keeper`n`nA local-first shared memory project."
} | ConvertTo-Json

$response = Invoke-RestMethod -Uri http://127.0.0.1:4317/api/v1/sources -Method Post -Body $body -ContentType "application/json"
$response | ConvertTo-Json
```

Expected response (new Source):

```json
{
  "outcome": "stored",
  "source_id": "<uuid>"
}
```

HTTP status: 201.

Expected response (exact duplicate):

```json
{
  "outcome": "duplicate",
  "source_id": "<same uuid>"
}
```

HTTP status: 200.

### Retrieve a Source by ID

```powershell
Invoke-RestMethod http://127.0.0.1:4317/api/v1/sources/<source_id>
```

Expected response:

```json
{
  "source_id": "<uuid>",
  "title": "Lantern Keeper handbook",
  "kind": "markdown",
  "content": "# Lantern Keeper\n\nA local-first shared memory project.",
  "fingerprint": "<sha256 hex>",
  "created_at": "2026-07-18T13:00:00Z"
}
```

### Error Responses

All errors use a stable shape:

```json
{
  "code": "invalid_source",
  "message": "Source title must not be empty"
}
```

Common error codes: `invalid_source`, `invalid_source_id`, `source_not_found`,
`storage_unavailable`, `internal_error`, `payload_too_large`.
`invalid_marker`, `invalid_marker_id`, `invalid_lookup`, `marker_not_found`.
Project retrieval can also return `project_not_found` or `source_unavailable`.

Source content, credentials, and raw database error text are never included in
error responses or logs.

## Project Retrieval And Codex Handoff

Project retrieval turns linked memory records into a compact context package for
an AI coding agent:

```powershell
$body = @{ project_id = "<project_id>" } | ConvertTo-Json
Invoke-RestMethod -Uri http://127.0.0.1:4317/api/v1/retrieval/projects -Method Post -Body $body -ContentType "application/json"
```

Expected response shape:

```json
{
  "project": {
    "project_id": "<uuid>",
    "name": "Lantern Keeper First Proof",
    "status": "active",
    "created_at": "2026-07-18T13:00:00Z"
  },
  "episodes": [
    {
      "episode_id": "<uuid>",
      "title": "The Human Relay Problem",
      "source_id": "<uuid>",
      "start_byte": 30,
      "end_byte": 320,
      "excerpt": "When a person copies context...",
      "why_matched": "Episode is linked to Project \"Lantern Keeper First Proof\"."
    }
  ],
  "context_package": {
    "format": "markdown",
    "audience": "codex",
    "content": "# Codex Handoff Context\n..."
  },
  "warnings": []
}
```

If a linked Episode points at a missing authoritative Source, the endpoint
returns HTTP 409 with `code: "source_unavailable"` instead of silently omitting
the Episode or returning partial context.

The `context_package` is a deterministic Codex-oriented rendering of retrieved
authoritative excerpts and provenance metadata. It is not an AI-generated
summary, recommendation, or narrative.

## CLI Commands

All commands below use the unified `lighting` executable (port 4317 by default).

### Health Check

```powershell
cargo run -p lighting -- health
cargo run -p lighting -- health --json
```

### Add a Source

```powershell
cargo run -p lighting -- source add README.md
cargo run -p lighting -- source add notes.txt --title "My Notes" --kind plain_text
cargo run -p lighting -- source add handbook.md --json
```

### Show a Source

```powershell
cargo run -p lighting -- source show <source_id>
cargo run -p lighting -- source show <source_id> --json
```

## First Proof Demonstration

Run the complete first-proof demonstration after starting SurrealDB and Lighting:

```powershell
.\scripts\start-surreal.ps1
.\scripts\run-first-proof-local.ps1
```

The script seeds a harmless local development demonstration that walks through:

```
fixtures/first-proof.md
-> Source -> Episode -> Project + Marker
-> retrieve -> Codex handoff context
-> local result file -> project-record-result
-> updated Project handoff
```

It is safe to rerun; objects are reused via stored IDs in
`.local/full-loop-demo-v2.json`, and the result writeback is idempotent. No data
is uploaded or deleted.

The four-step proof for the hackathon slice is:

```powershell
.\scripts\start-surreal.ps1
.\scripts\run-first-proof-local.ps1
cargo run -p lighting -- project-handoff <project-id>
cargo run -p lighting -- project-record-result <project-id> .local\full-loop-result.md --title "First Proof Result"
```

## Full Validation

```powershell
.\scripts\validate.ps1
```

Or run each step manually:

```powershell
.\scripts\start-surreal.ps1
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p lighting -- version
```

## Naming

Project: Lantern Keeper

Service: Lighting

Default bind address: `127.0.0.1:4317`

## Engineering Handbook

The `LK handbook.txt` file was not found in or beside the project workspace.
Place it at `docs/Lantern-Keeper-Engineering-Handbook.md` when it becomes
available. Do not invent or reconstruct it.

## Roadmap

See `docs/ROADMAP.md` for the current Done / Next / Later queue. The older
ten-job hackathon MVP sequence is preserved as completed historical context.
