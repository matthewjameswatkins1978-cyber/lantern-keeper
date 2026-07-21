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

## Next

Build Lantern Keeper's minimum memory foundation for the later joint runtime
slice. The first implementation should stay inside Lantern Keeper's memory
responsibility and expose capabilities to Tethers later, rather than embedding
Tethers runtime logic.

1. Inspect the actual repository, migrations and service boundaries against the
   five durable concepts: Project, Source, Episode, Memory and Link.
2. Add or adapt the smallest durable `Memory` model needed for:
   - kinds: `fact`, `decision`, `constraint`, `preference`, `idea`, `task`,
     `finding`, `experience`;
   - states: `active`, `superseded`, `archived`;
   - confidence, importance, provenance, reinforcement and supersession data.
3. Define proposal handling responsibilities for new, reinforcing,
   correcting, superseding, conflicting and insufficient candidates.
4. Define the first public service/capability surface:
   `lantern.context.retrieve`, `lantern.episode.record`,
   `lantern.memory.propose`, `lantern.memory.get`, and
   `lantern.memory.search`.
5. Keep retrieval bounded and explainable: project/state filters, exact IDs and
   terms, full-text search, recent Project items, direct graph links,
   deterministic ranking, stable tie-breaking, and fixed context-pack sections.
6. Treat AI as optional bounded judgement or compression over structured inputs
   and candidate sets. AI never owns storage rules, permissions, provenance or
   state transitions.

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
