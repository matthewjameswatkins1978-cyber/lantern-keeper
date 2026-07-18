# Lantern Keeper

Lantern Keeper is a local-first shared memory project for future ChatGPT and
Codex workflows.

Lighting is the local service inside Lantern Keeper. This repository is only the
foundation: workspace structure, configuration, a localhost HTTP service, a
SurrealDB connection, and first build/test gates.

## Current Status

Implemented now:

- Rust Cargo workspace
- Strongly typed placeholder identifiers in `lighting-core`
- SurrealDB configuration, connection, schema bootstrap, and health check
- Axum app with `GET /health` and `GET /version`
- `lighting-cli` development commands
- Docker Compose for local SurrealDB
- Unit and integration test structure

Not implemented yet:

- memory records or graph relations
- conversations, episodes, markers, projects, topics, decisions, tasks, handoffs
- search, vector search, embeddings, ingestion
- ChatGPT, Codex, MCP, authentication, cloud services, or a GUI

## Prerequisites

- Rust stable toolchain with Cargo
- Docker Desktop or another Docker Compose compatible runtime, for local
  SurrealDB

## Start SurrealDB

Copy the example environment file first:

```powershell
Copy-Item .env.example .env
```

Start the local development database:

```powershell
docker compose up -d surrealdb
```

SurrealDB listens only on `127.0.0.1:8000`.

## Build

```powershell
cargo build --workspace
```

## Test

Run the complete workspace test suite:

```powershell
cargo test --workspace
```

The integration tests use disposable database names beginning with
`lighting_test_`. They never use the `lighting_dev` database directly.

If SurrealDB is not running, integration tests fail with a message explaining how
to start it. To deliberately skip those tests:

```powershell
$env:LIGHTING_SKIP_INTEGRATION_TESTS="1"
cargo test --workspace
```

## Quality Gates

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

## Run Lighting

```powershell
cargo run -p lighting
```

Default service address:

```text
127.0.0.1:4317
```

Override with:

```powershell
$env:LIGHTING_HOST="127.0.0.1"
$env:LIGHTING_PORT="4317"
```

Lighting currently rejects non-localhost bind addresses.

## Check Health

```powershell
Invoke-RestMethod http://127.0.0.1:4317/health
```

Expected healthy response:

```json
{
  "service": "Lighting",
  "status": "ok",
  "database": "connected"
}
```

Version:

```powershell
Invoke-RestMethod http://127.0.0.1:4317/version
```

## lighting-cli

```powershell
cargo run -p lighting-cli -- health
cargo run -p lighting-cli -- init-db
cargo run -p lighting-cli -- print-config
```

`print-config` redacts the password completely.

## Stop the Database

```powershell
docker compose down
```

To remove local development database files too:

```powershell
docker compose down
Remove-Item -Recurse -Force surreal-data
```

## Naming

Project: Lantern Keeper

Service: Lighting

Database namespace: `lantern_keeper`

Development database: `lighting_dev`

## Known Current Limitations

This is a foundation only. It intentionally avoids the first memory proof,
ingestion, integrations, authentication, search, and UI work.
