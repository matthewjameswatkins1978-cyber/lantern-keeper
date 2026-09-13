# Lantern Keeper — Current

## Repository state

- Canonical branch: `master`
- Canonical master SHA: `a44554b`
- Latest verified implementation line:
  `feature/lantern-full-move` at `345c468`
- The feature line is ahead of master and has not yet passed its final
  connected-client, shadow-comparison, and cutover gates.

This status page intentionally distinguishes the shipped master baseline from
the latest verified feature work. Uncommitted provenance-complete live-capture
changes are not treated as a completed capability.

## What master proves

The canonical master branch contains the recovered local Source-backed loop:

```text
capture project knowledge
-> store exact Source evidence
-> record an Episode and Project links
-> retrieve deterministic excerpts
-> produce a Codex handoff
-> record the completed result
-> retrieve the updated Project handoff
```

It uses a Rust workspace, a localhost-only Lighting service, and a SurrealDB
adapter. The Tethers endpoint is preview-only and does not execute actions or
write memory.

## What the verified feature line adds

The feature line contains the current epistemic slice:

- Source / Episode / Claim / Belief separation with soft Memory Items beside
  the factual path;
- provenance and perspective fields that keep originator, speaker, transmitter,
  holder, stance, and transformation distinct;
- durable Claim-to-Belief reconciliation, immutable revisions, lineage,
  correction evidence, stale invalidation, and ambiguity-safe targeting;
- deterministic Context Packs with current-versus-historical sections,
  Matthew/Lucy/shared/legacy partitions, bounded budgets, provenance, and
  retrieval traces;
- bounded Lucy-owned Foreman review and candidate-only Proposal/Trace
  governance;
- logical export/restore, Basic Memory import accounting, LanternBench, and the
  local stdio MCP bridge.

The latest committed MCP checkpoint proves the bridge surface and restart
persistence in the feature-line development record. It does not prove a real
Lucy-native client integration or final cutover.

## Remaining gates

- complete the MCP contract and connected Lucy proof;
- prove the provenance-complete live factual capture path, including exact
  Source/Episode evidence and correction history;
- run Basic Memory shadow comparison and final-delta accounting;
- complete service-level restore/retrieval and real restart qualification;
- accept the feature line before describing it as shipped on `master`.

Until those gates pass, Basic Memory remains the canonical migration source and
rollback archive, and Lantern remains a local feature-line candidate rather
than a completed cutover.

## Safety invariants

- Sources and Episodes preserve evidence; Claims preserve assertions; Beliefs
  are current projections.
- No inferred consensus, self-citation laundering, or silent attribution of
  legacy material to Matthew.
- Corrections add evidence, preserve history, invalidate affected current
  projections, and mark dependent projections stale.
- Soft memory remains soft until explicit governed promotion.
- Dreamer and external agents propose; Foreman decisions are bounded and
  traced; candidate generation never directly mutates canonical Beliefs.
- Context Packs are disposable, bounded read projections and never evidence.
- Local storage, export, and recovery are preferred over an implicit cloud
  dependency.
