# Lantern Keeper Roadmap

Lantern Keeper is a local-first epistemic memory. The canonical architecture and safety boundaries are described in the current guides linked from the [README](../README.md).

## Canonical foundation now on master

- Rust workspace and layered core, store, service, CLI, and app boundaries.
- Exact Source storage, duplicate detection, UTF-8-safe Episode ranges, and Project/Marker links.
- Source / Episode / Claim / Belief separation plus soft Memory Items.
- Perspective, provenance, predicate, and scope handling.
- Durable reconciliation, corrections, immutable revisions, lineage, stale invalidation, and echo suppression.
- Bounded deterministic Context Packs and retrieval traces.
- Candidate-only Proposal/Trace governance and bounded Foreman review.
- Logical export/restore, Basic Memory accounting, and LanternBench.
- Provenance-complete live factual capture.
- Local stdio MCP with eight bounded tools.
- Real Codex Desktop connected-client proof with restart persistence and provenance.

## Next

1. Run the Basic Memory shadow comparison against representative real memories.
2. Fix mechanism-level discrepancies rather than query-specific cheats.
3. Run final Basic Memory delta/idempotency/accounting and exception review.
4. Produce the final private export and prove representative retrieval after a fresh restore.
5. Record the cutover/rollback decision.

Normal ChatGPT/Lucy full read/write custom MCP remains a separate product-access gate rather than a Lantern-engine defect.

## Later

- exact typed/fused retrieval and richer graph-backed project associations;
- bounded graph expansion and optional semantic candidates;
- Dreamer operations after candidate, review, and replay behaviour are independently qualified;
- hosted/cloud sync, GUI workflows, universal importers, recommendations, automatic conversation observation, and multi-user collaboration;
- public ChatGPT/plugin packaging once the product-access path can be exercised properly.

## Principles

- Evidence is append-oriented and inspectable.
- Corrections preserve history rather than rewriting it.
- No inferred consensus, self-citation laundering, or silent attribution.
- Soft memory remains soft until explicit governed promotion.
- AI proposes; bounded Foreman governance decides; Matthew remains the final human authority.
- Retrieval is bounded, deterministic, explainable, and local-first.
