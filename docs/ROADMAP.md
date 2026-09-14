# Lantern Keeper Roadmap

Lantern Keeper is a local-first epistemic memory. The canonical architecture
and safety boundaries are described in the current guides linked from the
[README](../README.md).

## Done on the canonical master baseline

- Rust workspace and layered `lighting-core`, store, service, CLI, and app
  boundaries.
- Exact Source storage, duplicate detection, UTF-8-safe Episode ranges, and
  Project/Marker links.
- Deterministic Source-backed retrieval with exact excerpts and a Codex
  handoff context package.
- Idempotent result writeback into the same Project memory.
- Localhost-only service behaviour, validation, integration tests, and the
  complete first-proof demonstration.
- Preview-only Tethers endpoint with no Lantern memory mutation.

## Verified on the current feature line

The `feature/lantern-full-move` line at `345c468` adds the current epistemic
slice without changing the authority boundary:

- Source / Episode / Claim / Belief separation and soft Memory Items;
- perspective, provenance, predicate, and scope handling;
- durable reconciliation, corrections, immutable revisions, lineage, and stale
  invalidation;
- bounded deterministic Context Packs and retrieval traces;
- candidate-only Proposal/Trace governance and bounded Foreman review;
- logical export/restore, Basic Memory accounting, LanternBench, and local MCP.

That line is not yet accepted into `master`. The connected Lucy proof,
provenance-complete live capture, shadow comparison, final delta, and cutover
remain gates. See [`CURRENT.md`](../CURRENT.md).

## Next

1. Complete the MCP contract and real Lucy-native client proof.
2. Prove exact Source/Episode provenance for ordinary factual live capture and
   its correction path.
3. Run Basic Memory shadow comparison, final-delta accounting, and exception
   review.
4. Qualify restore/restart retrieval and accept the feature line before
   describing it as shipped on `master`.

## Later

- exact typed/fused retrieval and richer graph-backed project associations;
- bounded graph expansion and optional semantic candidates;
- Dreamer operations after candidate, review, and replay behaviour are
  independently qualified;
- hosted/cloud sync, GUI workflows, universal importers, recommendations,
  automatic conversation observation, and multi-user collaboration.

## Principles

- Evidence is append-oriented and inspectable.
- Corrections preserve history rather than rewriting it.
- No inferred consensus, self-citation laundering, or silent attribution.
- Soft memory remains soft until explicit governed promotion.
- AI proposes; bounded Foreman governance decides; Matthew remains the final
  human authority.
- Retrieval is bounded, deterministic, explainable, and local-first.
