# Lantern Keeper Roadmap

A local-first shared memory layer for future ChatGPT and Codex workflows.

Canonical joint architecture:
[`architecture/TETHERS_LANTERN_KEEPER_CANONICAL_ARCHITECTURE.md`](architecture/TETHERS_LANTERN_KEEPER_CANONICAL_ARCHITECTURE.md)

That report is the accepted target architecture and build order for Lantern
Keeper plus Tethers. This roadmap records Lantern Keeper's project-local
Done / Next / Later view.

## Done

The original hackathon MVP sequence is complete as the working
source-backed memory loop:

1. Repository and runnable Lighting skeleton.
2. Source domain and validation.
3. SurrealDB source storage, duplicates and restart persistence.
4. Source HTTP API and CLI.
5. Projects, Episodes and Markers.
6. Markdown-heading Episode creation and manual Marker association.
7. Deterministic retrieval, explanation and source-range return.
8. CLI retrieval and a small evaluation dataset.
9. Basic Markdown/JSON ContextPackage for a coding handoff.
10. End-to-end demonstration, failure checks, baseline, and phase report.

This proof preserves authoritative Sources, creates bounded Episodes, links
them to Projects and Markers, retrieves deterministic Codex handoff context,
and records completed results back into the Project.

## Immediate recovery and restart gates

Before Memory implementation, complete and record:

1. Windows local checkout archaeology and preservation of all local-only work.
2. Rust 1.98.1 formatting, compilation, Clippy and test validation.
3. Separate SurrealDB 3.3.0-beta.4 compatibility experiment, with fallback
   to the locked 3.2.1 client if required.
4. README/CURRENT/ROADMAP reconciliation and a clean `master` baseline.

The repository baseline is now understood for the visible remote history, Rust
1.98.1, the locked SurrealDB 3.2.1 client and the Basic Memory snapshot. The
unavailable Windows checkout and live database remain explicit external gates.
The detailed takeover packet defines the later LK-N1 through LK-N18 sequence,
including recovery, reconciliation, retrieval, MCP, Basic Memory replication,
shadow use and reversible cutover.

## Current implementation slice

The first canonical-memory slice is implemented on the recovery feature branch.
It stays inside Lantern Keeper's memory responsibility and exposes capabilities
to Tethers later, rather than embedding Tethers runtime logic.

1. The existing service and migrations cover the five durable concepts:
   Project, Source, Episode, Memory and Link.
2. The durable `Memory` model covers:
   - kinds: `fact`, `decision`, `constraint`, `preference`, `idea`, `task`,
     `finding`, `experience`;
   - states: `active`, `superseded`, `archived`;
   - confidence, importance, provenance, reinforcement and supersession data.
3. Deterministic proposal handling covers new, reinforcing, superseding,
   historical, conflicting and ignored candidates; correction remains a
   reviewed follow-up hardening item.
4. The first HTTP/JSON-RPC capability surface provides search, context,
   remember, relations, history, forget, audit and doctor.
5. Keep retrieval bounded and explainable: project/state filters, exact IDs and
   terms, full-text search, recent Project items, direct graph links,
   deterministic ranking, stable tie-breaking, and fixed context-pack sections.
6. Treat AI as optional bounded judgement or compression over structured inputs
   and candidate sets. AI never owns storage rules, permissions, provenance or
   state transitions.

## Next gates

1. Run V4 migration, backup/restore, importer idempotency and restart checks
   against an isolated live SurrealDB service.
2. Make multi-record reconciliation atomic and add mutation recovery tests.
3. Run Basic Memory retrieval parity and incremental activity mirroring.
4. Replace the historical Tethers 0.1 preview adapter with the reviewed 0.7
   capability binding and add authenticated remote MCP deployment.
5. Complete Mastra live storage checks and the golden takeover evaluation.

## Principles

- Local-first.
- Source material is authoritative and must never be logged casually.
- Lantern Keeper is the memory system, not the workflow engine.
- Tethers coordinates Lantern Keeper through public capabilities exposed by
  Lantern Keeper.
- The durable concepts are Project, Source, Episode, Memory and Link.
- Minimalist, Living Memory and Archivist are configuration profiles over one
  pipeline, not separate implementations.
- Retrieval is concrete machinery, not "AI finds the right memories."
- `lighting-core` must not depend on SurrealDB or HTTP frameworks.
- Build one narrow end-to-end capability, test it, freeze it, then improve the weakest real behaviour.

## Later

- Automatic conversation ingestion
- Automatic episode detection
- Embeddings and vector search
- ChatGPT/Codex/MCP connectors beyond the first deliberately integrated route
- Cloud services and synchronisation
- Graphical user interface
- Task automation, decisions, and report ingestion
- Multi-user collaboration
- Broad graph expansion or ontology generation
- Separate implementations for retention profiles
