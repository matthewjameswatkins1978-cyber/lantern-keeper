# Lantern Keeper

Lantern Keeper is a local-first, AI-first shared memory layer for Matthew and
Lucy. The AI notices useful information, records derived understanding with
provenance, retrieves bounded working context, and preserves corrections as
history. Matthew can correct or protect memory, but ordinary organisation
should not require manual filing.

Lighting is the Rust service in this repository. The first useful loop is:

    Matthew/Lucy observation
    -> derived Living Memory with source references
    -> lexical/project-scoped recall
    -> compact working context
    -> provenance and supersession remain inspectable

Source and ledger records are evidence: what was supplied or happened. Memory
records are derived understanding: what Lantern currently considers useful.
Derived understanding may change; source evidence is not silently rewritten.
The joint Tethers architecture remains documented in
docs/architecture/TETHERS_LANTERN_KEEPER_CANONICAL_ARCHITECTURE.md. Tethers is
out of scope for this foundation pass.

## Current implemented scope

- Rust 1.98.1, edition 2024, repository-pinned by rust-toolchain.toml.
- SurrealDB 3.3.0-beta.4, exactly pinned in Cargo.toml and matched by the
  qualified local server lane.
- Embedded, versioned SurrealKV is the normal local store:
  .lighting-data/surrealkv, with sync=every.
- Remote WebSocket SurrealDB remains an explicit test/development option via
  LIGHTING_STORAGE=remote-surreal.
- Existing Source, Project, Episode, Marker and relation storage.
- Living Memory records with fact, decision, preference, instruction, lesson,
  gotcha, open_loop, workflow, summary and entity kinds.
- Provenance/evolution fields for derived_from, updates, extends, supersedes,
  contradicts and supports, plus bounded multi-hop lineage.
- Semantic temporal fields: recorded_at, known_at, valid_from, valid_until and
  superseded_at, with optional observed_at and as-of retrieval. SurrealKV MVCC
  history is additional physical history, not a replacement for these fields.
- Append-only, replay-safe host-neutral `ledger_event` records, a JSON event
  ingestion endpoint, and `ledger-ingest` CLI support. A representative fixture
  is in fixtures/ledger/matthew-lucy-representative.json.
- Validated Basic Memory snapshot import that preserves every note as
  Markdown Source evidence and records upstream identity/metadata as
  replay-safe ledger events.
- CLI and HTTP operations to remember, recall, build context and supersede.
- Inspectable retrieval traces in recall responses.
- Engine-independent export to manifest.json and NDJSON files.
- Embedded SurrealKV qualification and Living Memory integration tests.
- An executable LanternBench v1 runner in bench/run-lanternbench.ps1.

Retrieval is deliberately a first slice, not a completed cognitive system. It
currently uses phrase matching, project/status/as-of filters, importance,
confidence and recency. BM25, vector search, entity/graph signals, persisted
traces, activation, conflict analysis, proposal/gardening, automatic host
capture and MCP are later work.

## Requirements

- Windows with the MSVC C++ build tools, or an equivalent Rust development
  environment.
- rustup with Rust 1.98.1, rustfmt and Clippy. The repository toolchain file
  selects this automatically.
- SurrealDB CLI/server 3.3.0-beta.4 for the remote integration lane. The
  embedded path does not require a separate server process.
- No SurrealDB process is required for the normal embedded path.

## Build and test

    cargo check --workspace --all-targets --all-features --locked
    cargo test --workspace --locked
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

The embedded storage lanes are:

    cargo test -p lighting-store-surreal --test memory_repository --locked
    cargo test -p lighting-store-surreal --test surrealkv_qualification --locked

The existing remote integration tests are opt-in. Start the pinned
SurrealDB 3.3.0-beta.4 test server with scripts/start-surreal.ps1, set
LIGHTING_STORAGE=remote-surreal, and run the relevant integration test. To
skip those tests explicitly:

    cmd /v /c "set LIGHTING_SKIP_INTEGRATION_TESTS=1&& cargo test --workspace --locked"

For the complete credentialed remote lane, configure the endpoint and root
credentials for the disposable local server before running the same workspace
test command. Run `surreal is-ready` first so the server has completed startup.

Inspect the active toolchain, database connection, server version and schema
state with:

    cargo run -p lighting -- doctor --json

On this Windows repository, the historical source tree contains CRLF files
that the current rustfmt reports as newline-style differences. Formatting
new or modified Rust code is still required; do not convert the legacy tree
wholesale merely to make that diagnostic disappear.

## Local configuration and service

Copy .env.example to .env if you want local overrides:

    Copy-Item .env.example .env

The embedded defaults are sufficient:

    LIGHTING_STORAGE=embedded-surrealkv
    LIGHTING_SURREAL_PATH=.lighting-data/surrealkv

Optional remote settings remain in .env.example for the development server.
Credentials are only used for the remote backend. Do not commit .env.

Start Lighting:

    cargo run -p lighting -- serve

The service binds to 127.0.0.1:4317 by default. Readiness is reported only
after the store and all schema migrations are ready:

    Invoke-RestMethod http://127.0.0.1:4317/health/ready

## Memory operations

Remember a derived memory. Repeat --derived-from, --supports,
--supersedes or --contradicts for multiple references:

    cargo run -p lighting -- remember "Matthew prefers evidence-linked context." --kind preference --importance 0.9 --derived-from source:conversation-001 --json

Recall active memories:

    cargo run -p lighting -- recall --phrase "evidence-linked" --json

Build a bounded working packet:

    cargo run -p lighting -- context --query "Continue with Lantern Keeper" --json

Retain a correction as history:

    cargo run -p lighting -- memory-supersede MEMORY_ID --json

Ingest a host-neutral JSON export (an array or `{ "events": [...] }` object).
Re-running the same file is safe because each event has an idempotency key:

    cargo run -p lighting -- ledger-ingest fixtures/ledger/matthew-lucy-representative.json --json

Import a validated Basic Memory snapshot directory. The importer reads
`manifest.json` plus `notes-*.ndjson`, validates the complete set before
contacting Lighting, stores raw notes as immutable Source evidence, and records
the note, bracketed observation categories, and typed `[[relation]]` links as
replay-safe ledger events:

    cargo run -p lighting -- basic-memory-import .private-migration/snapshot-2026-09-12-repaired --dry-run --json
    cargo run -p lighting -- basic-memory-import .private-migration/snapshot-2026-09-12-repaired --json

The migration command does not silently promote whole notes into canonical
Memory records. Promotion remains a separate reconciliation step so imported
Markdown stays evidence and uncertain observations are not manufactured into
truth.

The equivalent HTTP endpoints are:

    POST /api/v1/memories
    POST /api/v1/memories/recall
    POST /api/v1/memories/context
    POST /api/v1/memories/{memory_id}/supersede
    POST /api/v1/ledger/events

JSON is the machine-facing contract. A no-match recall returns an explicit
abstention message and an empty result set; candidates are not treated as
proof merely because they exist.

## Portable backup

Export logical records without depending on the SurrealKV binary format:

    cargo run -p lighting -- export backup

Stop the Lighting service before running this command on Windows; the active
embedded store holds a file lock. The service can be restarted afterwards.

The export contains:

    backup/manifest.json
    backup/ledger.ndjson
    backup/memories.ndjson
    backup/relations.ndjson
    backup/projects.ndjson

Each NDJSON record includes its logical table name and JSON-converted record.
This is a portable escape hatch; restore validation is still a separate
qualification lane. The original
.lighting-data directory is not removed by export or migration.

## Existing source/project loop

The original exact-source loop remains available:

    cargo run -p lighting -- source add README.md
    cargo run -p lighting -- source show SOURCE_ID
    cargo run -p lighting -- project create "Lantern Keeper"
    cargo run -p lighting -- project-handoff PROJECT_ID

Sources remain authoritative evidence; Episodes and Markers point into them.
The old remote-server demonstration scripts remain available as an explicit
remote test path and are not required for ordinary local memory use.

## Architecture and research notes

- docs/architecture/LANTERN_FOUNDATION_DECISIONS.md records the exact
  modernisation decisions and current boundaries.
- docs/dependency-modernisation-2026-09-12.md records the selected matched
  SurrealDB lane, dependency review and security findings.
- docs/SURREALKV_QUALIFICATION.md records the workload-oriented qualification.
- docs/architecture/MEMORY_MODEL_AUDIT.md records model decisions and gaps.
- docs/architecture/LEDGER_EVENT_MODEL.md defines the evidence boundary.
- docs/architecture/AMBIENT_MEMORY_LIFECYCLE.md defines host-neutral lifecycle
  hooks and replay boundaries.
- bench/lanternbench-v1.json and docs/LANTERNBENCH.md define the benchmark.

Deferred deliberately: GUI, cloud deployment, accounts, broad ontology,
universal ingestion, provider connectors, MCP handlers, vector/BM25 fusion,
automatic gardening and autonomous rewriting of source history.
