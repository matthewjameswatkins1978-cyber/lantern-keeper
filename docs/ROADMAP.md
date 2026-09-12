# Lantern Keeper Roadmap

Lantern Keeper is a local-first, AI-first shared memory layer. The immediate
product target is the Matthew and Lucy loop; other agents, connectors and
cloud services come later.

## Done

- Source-backed Project/Episode/Marker storage and deterministic retrieval.
- Exact source preservation, provenance-bearing excerpts and result writeback.
- Rust 1.98.1 / edition 2024 repository pin.
- Exact SurrealDB 3.3.0-beta.4 dependency with embedded versioned SurrealKV
  as the normal local backend.
- Portable logical export and workload-oriented SurrealKV qualification.
- First durable Living Memory model with temporal/as-of queries, explicit
  evolution relationships and bounded lineage.
- Append-only host-neutral ledger events, raw payload retention, replay-safe
  JSON ingestion, and executable LanternBench v1 runner.
- CLI/HTTP remember, recall, context and supersede operations.
- Inspectable first retrieval trace and LanternBench v1 fixture.
- Lossless Basic Memory snapshot capture and replay-safe Source/ledger import
  of the complete Lantern project at note level.

## Foundation modernisation record

The foundation modernisation pass completed and recorded:

1. Windows local checkout archaeology and preservation of all local-only work.
2. Rust 1.98.1 formatting, compilation, Clippy and test validation.
3. A matched SurrealDB 3.3.0-beta.4 client/server qualification, with a
   3.2.4 fallback control.
4. README/CURRENT/ROADMAP reconciliation and an untouched `master` baseline.

The repository must not begin the canonical Memory model until these gates are
understood. The detailed takeover packet defines the later LK-N1 through LK-N18
sequence, including recovery, reconciliation, retrieval, MCP, Basic Memory
replication, shadow use and reversible cutover.

## Next implementation

Exercise and verify the checkpointed observation/relation second pass, then
promote the captured Matthew/Lucy Basic Memory source set through a real,
reviewable reconciliation pass and qualify restart-safe progress. The next
slice should preserve the current source/derived boundary and add:

1. conversation and turn records in the Memory Ledger;
2. persisted retrieval traces and activation metadata;
3. conflict/supersession workflows with explicit evidence;
4. a small real-history LanternBench corpus with expected evidence;
5. thin MCP exposure over the existing service operations.

## Later

- BM25/vector/entity/graph retrieval fusion with inspectable scoring.
- Conservative AI gardener and reflection/digestion jobs.
- Automatic conversation capture and source-to-memory proposals.
- Additional Codex/Luna/Cline/provider connectors.
- GUI, cloud deployment, accounts, multi-user permissions and broad ontology.

## Principles

- Source evidence is append-oriented and never silently rewritten.
- Derived memory may evolve, but every correction retains provenance.
- Physical SurrealKV version history is diagnostic; semantic time lives in the
  Lantern data model.
- Retrieval may abstain when evidence is absent or conflicting.
- Keep the AI-facing surface thin, predictable and machine-readable.
- Do not dispatch to or modify GARY in this phase.
