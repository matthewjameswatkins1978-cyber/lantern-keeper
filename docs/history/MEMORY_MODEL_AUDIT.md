# Memory Model Audit

> Historical document. This one-time audit is retained for context and is not
> the current implementation specification.

Status: **PARTIAL — baseline audited; temporal and relation improvements are in progress.**

This audit records the decision about the existing Living Memory prototype before
ambient ingestion work. It is deliberately a small substrate review, not an
attempt to freeze a complete ontology.

## Current boundary

The repository has two useful layers:

- Source / Episode / Marker records are the durable memory-path and source layer.
- `memory` records are derived Living Memory, with explicit confidence, temporal
  fields, provenance references, and supersession state.

The boundary is sound. The current weakness is that several relationships are
arrays of opaque strings, and retrieval is currently lexical rather than fused.

## Decisions

| Area | Decision | Reason |
| --- | --- | --- |
| Stable IDs | **KEEP** | Existing typed IDs and UUID-backed memory records are adequate. |
| Memory content and kind | **KEEP** | The ten useful early kinds are intentionally small and practical. |
| Project association | **KEEP** | Optional project scope is useful without making every memory project-bound. |
| Status | **KEEP / ADJUST** | Active, superseded, and archived are enough initially; queries must treat historical validity separately from current status. |
| Confidence and importance | **KEEP** | Truth confidence and retrieval importance are separate, which is the right direction. |
| Recorded and known timestamps | **KEEP** | They distinguish recording from system knowledge. |
| Observed timestamp | **ADJUST** | Add optional `observed_at`; the system must not invent observation time when an importer cannot supply it. |
| Validity interval | **ADJUST** | Add as-of filtering and acceptance tests for non-overlapping historical claims. |
| Supersession | **KEEP / ADJUST** | Preserve the old record and close its semantic interval; expose explicit evolution relationships. |
| Provenance | **ADJUST** | Keep existing references, but give updates, extensions, derivation, contradiction, support, and supersession explicit names and lineage traversal. |
| Duplicate detection | **DEFER / ADJUST** | Do not silently merge records yet; establish idempotent ledger ingestion first, then add proposals with evidence. |
| Retrieval | **ADJUST** | Preserve exact/lexical/project/currentness behaviour as the baseline; make scoring and traces extensible before adding vectors. |
| Context construction | **ADJUST** | Keep the compact sectioned packet, then add bounded CORE/ACTIVE/RELEVANT/HISTORY roles. |
| Activation | **DEFER** | Retrieval usefulness must be measured before reinforcement changes ranking. |
| Export | **KEEP / ADJUST** | Existing engine-independent NDJSON export is valuable; add validation and restore into a new empty store. |
| Autonomous gardener | **DEFER** | First make evidence, replay, provenance, and proposal boundaries reliable. |
| MCP | **DEFER** | Add a thin surface after domain and HTTP semantics are stable. |

## Explicit non-goals

This pass does not introduce a second graph database, a large ontology, vector
embeddings, or autonomous rewriting of source evidence. Relationship fields and
lineage queries remain part of the existing Memory repository.

## Gate A assessment

The baseline is restorable at `lantern-pre-memory-checkpoint` and the working
tree starts clean at the foundation branch head. Gate A is therefore **passed for
preservation**. The implementation portion of the audit remains **partial** until
the temporal acceptance test, explicit evolution relationships, and replay-safe
ledger ingestion are present.
