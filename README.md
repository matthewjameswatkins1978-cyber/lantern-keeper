# Lantern Keeper

Lantern Keeper is a local-first shared memory layer for Matthew and Lucy. It
preserves not only information, but provenance, perspective, changing beliefs,
and softer fragments that may matter later.

The central boundary is simple:

    Sources preserve what happened.
    Claims preserve what was asserted.
    Beliefs preserve current reconciled understanding.
    Memory Items preserve what may matter without claiming it as fact.

Lantern is designed so Matthew can talk normally. Lucy can notice, remember,
retrieve, explain, and correct memory without making Matthew become the filing
clerk. Historical evidence is retained when derived understanding changes.

## What works today

- Rust 1.98.1 and Edition 2024 are pinned.
- SurrealDB 3.3.0-beta.4 is the supported client/server lane.
- Embedded, versioned SurrealKV is the normal local store.
- Source, Episode, Project, Claim, Belief, soft Memory Item, relation, Trace,
  Proposal, Predicate, Dimension, export, import-accounting, recall, and
  context foundations are present.
- Predicate and scope normalization, explicit unmapped Claims, pure
  reconciliation decisions, echo suppression, direct-holder gates, and
  transitive stale propagation are implemented.
- The repaired Basic Memory snapshot accounts for 61 notes, 496 observations,
  and 275 relations with zero unexplained items.

The current feature line is still a foundation, not the finished cutover
product. Durable Claim-to-Belief transitions, correction-aware Context
Compiler reads, restore proof, Lucy-native MCP, shadow comparison, and
cutover remain open. See `CURRENT.md` for the short operational truth.

## Prerequisites

1. Rustup with Rust 1.98.1, rustfmt, and Clippy.
2. Microsoft C++ Build Tools and Windows SDK for Rust's MSVC target on
   Windows. The Visual Studio IDE is not required.
3. SurrealDB 3.3.0-beta.4 only when running the optional remote integration
   lane. Embedded SurrealKV needs no separate server.
4. PowerShell on Windows.

## Build, test, and validate

    git clone https://github.com/matthewjameswatkins1978-cyber/lantern-keeper.git
    cd lantern-keeper
    pwsh -NoProfile -File .\scripts\validate.ps1

The validation script is the canonical workflow. Its underlying checks are:

    cargo fmt --all -- --check
    cargo check --locked --workspace --all-targets --all-features
    cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
    cargo test --locked --workspace -- --test-threads=1
    cargo run --locked -p lighting -- version

## Run Lantern

The default store is `.lighting-data/surrealkv`, which is local and ignored.
Optional local overrides can be copied from `.env.example` to `.env`.

    cargo run --locked -p lighting -- serve
    cargo run --locked -p lighting -- doctor --json
    cargo run --locked -p lighting -- recall --phrase "evidence-linked" --json
    cargo run --locked -p lighting -- context --query "Continue with Lantern Keeper" --json

For the optional remote lane, use the exact-version scripts in `scripts/`:

    .\scripts\start-surreal.ps1
    $env:LIGHTING_STORAGE = "remote-surreal"
    $env:LIGHTING_SURREAL_USERNAME = "root"
    $env:LIGHTING_SURREAL_PASSWORD = "root"
    cargo test --locked --workspace -- --test-threads=1
    .\scripts\stop-surreal.ps1

Do not commit `.env`, local databases, runtime state, or private migration
material. Stop the Lighting service before exporting embedded storage.

## Documentation map

- `CURRENT.md` — current phase, verified state, gaps, and next step
- `AGENTS.md` — tool-neutral worker landing page
- `docs/architecture/` — current architecture and governance
- `docs/basic-memory-migration.md` — migration accounting and boundaries
- `docs/recovery.md` — export and recovery procedure
- `docs/LANTERNBENCH.md` — behavioural benchmark
- `docs/windows-worker-notes.md` — Windows-specific development notes
- `docs/mcp.md` — current API boundary and future Lucy-native contract
- `docs/history/` — selected historical context only

The optional Tethers preview is explicitly bounded and does not define
Lantern's memory authority. Legacy implementation archaeology remains in Git
history or the small labelled history directory, not in the normal setup path.
