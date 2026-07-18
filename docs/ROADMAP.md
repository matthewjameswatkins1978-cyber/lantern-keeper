# Lantern Keeper MVP Roadmap

A local-first shared memory layer for future ChatGPT and Codex workflows.

## Build Order

1. Repository and runnable Lighting skeleton
2. Source domain and validation
3. SurrealDB source storage, duplicates and restart persistence
4. Source HTTP API and CLI
5. Projects, episodes and markers
6. Markdown-heading episode creation and manual marker association
7. Deterministic retrieval, explanation and source-range return
8. CLI retrieval and a small evaluation dataset
9. Basic Markdown/JSON ContextPackage for a coding handoff
10. End-to-end demonstration, failure checks, baseline, and phase report

## Principles

- Local-first.
- Source material is authoritative and must never be logged casually.
- `lighting-core` must not depend on SurrealDB or HTTP frameworks.
- Build one narrow end-to-end capability, test it, freeze it, then improve the weakest real behaviour.

## Out of Scope for the MVP

- Automatic conversation ingestion
- Automatic episode detection
- Embeddings and vector search
- ChatGPT/Codex/MCP connectors
- Cloud services and synchronisation
- Graphical user interface
- Task automation, decisions, and report ingestion
- Multi-user collaboration
