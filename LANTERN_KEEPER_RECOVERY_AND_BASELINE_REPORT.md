# Lantern Keeper Recovery and Baseline Report

Date: 12 September 2026  
Repository: `matthewjameswatkins1978-cyber/lantern-keeper`  
Target branch: `master`

## Executive verdict

The visible Lantern Keeper foundation has been recovered onto a clean feature
branch based on remote `master` and extended with the first canonical-memory,
reconciliation, migration and AI-facing capability slice. The branch is ready
for review and workstation validation. It is **not yet safe to merge into
`master` as the canonical Basic Memory replacement** because this environment
cannot run the live SurrealDB migration/recovery drill or inspect the requested
Windows-only checkout and Tethers 0.7 runtime.

Basic Memory was read-only during this work. No Basic Memory note was changed,
deleted or made into a second write target.

## Recovered canonical state

- Starting remote `master`: `a44554b7050e9b3dd2178eede99b67184d89d12c`
- Starting tree: the merged repository foundation on `master`.
- Advanced visible historical branch: `agent/lighting-source-api` at
  `407c52934de8fe4c583c7ed549e51d1de45c7ce3`.
- Working branch: `feature/lantern-full-move`.
- The requested `D:\The Next Thing\lantern-keeper-foundation` checkout was not
  mounted here; any local-only commits, worktrees or databases there remain an
  explicit workstation archaeology gate.
- Recovery refs for visible remote heads were preserved in the local recovery
  clone before implementation.

## Implemented on the feature branch

- Rust 1.98.1 pin and workspace `rust-version` update.
- Canonical `Memory` domain model with scope, kind, current/history state,
  valid-time, provenance, reinforcement, supersession, conflict, derivation,
  sensitivity, revision and checksum fields.
- Deterministic reconciliation outcomes for new, reinforce, supersede,
  historical, conflict, unresolved, ask and ignore candidates.
- SurrealDB V4 schema for memories, relation records and mutation events.
- Typed relations with unresolved target retention and second-pass import.
- HTTP capability routes and a small JSON-RPC MCP adapter, including search,
  context, remember, relations, history, forget, audit and doctor.
- Reusable `lighting import-basic-memory` snapshot validator/importer with
  deterministic manifest, source-first import, recognized observations,
  typed relations, historical-status handling and stable retry identities.
- Separate `apps/mastra` working-memory package using
  `@surrealdb/mastra-ai`; Mastra proposes candidates to Lantern and does not
  write canonical tables.
- Current architecture, migration, roadmap, Tethers boundary and status docs.

## Basic Memory snapshot

The live `Lantern` project was inventoried and preserved outside the repository
as a private migration input. The deterministic manifest is:

| Measure | Result |
| --- | ---: |
| Markdown records | 61 |
| Directories | 14 |
| UTF-8 bytes | 210,186 |
| Recognized observations | 455 |
| Typed relations | 209 |
| Unresolved relation targets | 7 |
| Manifest SHA-256 | `f723a5ee0309679d4ff9198d8ec567b0dbe0f8431e9cc897a7c78b347fe1def2` |

The `--verify --json` importer run reproduced this manifest. Private source
contents are not committed to the public repository.

## Toolchain and database findings

- `rustc 1.98.1 (48a229cea 2026-09-01)`
- `cargo 1.98.1 (797e8a9bc 2026-08-05)`
- Production dependency remains SurrealDB 3.2.1 via the locked 3.2 line.
- A separate full-workspace compile check passed with
  `surrealdb 3.3.0-beta.4`; it was not promoted to production and no live
  beta database was exercised.
- `cargo audit` still reports transitive `rkyv 0.7.46` and `rsa 0.9.10`
  advisories, plus maintenance/yanked warnings. These require a deliberate
  dependency/security decision before a production cutover.

## Verification

Passed on the isolated feature branch:

- `cargo fmt --all -- --check`
- `cargo check --workspace --all-targets --all-features`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `LIGHTING_SKIP_INTEGRATION_TESTS=1 cargo test --workspace` — 34 CLI, 50
  core, 60 service, and existing fixture/in-memory integration tests passed;
  one historical Tethers engine test remains ignored.
- `cargo build --workspace --release`
- `npm run typecheck && npm run build` in `apps/mastra`
- Basic Memory importer `--verify --json`
- `git diff --check`

Not yet proven in this environment:

- live SurrealDB schema/mutation/restart tests;
- backup/restore and interrupted-write recovery;
- actual snapshot import and retry against a live database;
- Basic Memory parity, incremental activity bridge and shadow trial;
- authenticated remote MCP operation;
- current Tethers 0.7 host/capability integration;
- live Mastra storage and observational-memory behaviour.

## Tethers state

The repository still contains the historical preview-only Tethers 0.1 adapter.
The current Tethers 0.7 boundary and verification gate are documented in
`docs/integrations/TETHERS_0_7_LANTERN_BOUNDARY.md`. No old Tethers runtime was
silently extended, and no Tethers operation currently owns Lantern storage.

## Recommended next implementation point

Start from the feature branch tip after confirming the Windows checkout has no
unexplained local-only work. Run the V4 schema, backup/restore, importer
idempotency and restart tests against an isolated SurrealDB service. Then add
atomic multi-record reconciliation, Basic Memory parity/incremental mirroring,
the reviewed Tethers 0.7 binding and authenticated MCP deployment. Only after
those gates pass should the final Basic Memory delta and reversible `master`
cutover be performed.

## Git and rollback

The feature branch is intended to be published for review; remote `master`
must remain at its known-good starting SHA until the acceptance gates above
pass. The rollback source is the untouched Basic Memory `Lantern` project plus
the private snapshot manifest/archive. Lantern test imports must use an isolated
database or a pre-import SurrealDB backup; no destructive migration operation
was performed here.
