# Lantern Living Memory Architecture

Status: **canonical current architecture; epistemic foundation merged to `master`**

Lantern Keeper is a local-first shared memory for humans and AI. Sources preserve what happened; Claims preserve what was asserted; Beliefs preserve current reconciled understanding; and Memory Items preserve useful material that is not ready to become truth.

## The four-layer boundary

```text
Source  ->  Episode  ->  Claim  ->  Belief
 evidence    event       assertion   current projection

                    +-> Memory Item
                    |   soft possibility
                    +-> Trace / Proposal
                        governance explanation
```

Sources and Episodes are evidence. Claims are immutable assertions that point back to evidence and retain perspective. Beliefs are projections that may be reconciled, superseded, invalidated, or marked stale without rewriting the evidence. Memory Items are deliberately softer and need not become Claims.

## Governing constitution

> Matthew talks. Lucy remembers. Machines assist. Matthew corrects.
> Remember generously. Assert cautiously.
> Authorship, transmission, endorsement, and belief ownership are separate.

The architecture does not infer consensus from repetition, silence, quotation, or assistant output. A generated narrative or Context Pack is a disposable view and never becomes evidence merely because it was generated.

## Current vertical slice

Canonical `master` now contains typed epistemic records and durable stores for Claims, Beliefs, immutable Belief revisions, soft Memory Items, Traces, and Proposals alongside Source, Episode, Project, ledger, retrieval, export, and bounded Tethers-preview surfaces.

It supports durable Claim-to-Belief reconciliation, belief inspection, correction evidence, stale invalidation, bounded Foreman review, deterministic Context Packs, logical export/restore, and a local stdio MCP bridge. Ordinary live factual capture records exact Source/Episode evidence before reconciliation. A real Codex Desktop client has proved the eight-tool lifecycle, current/history separation, provenance, and restart persistence.

Basic Memory shadow comparison, final delta/accounting, representative retrieval after fresh restore, and the explicit cutover decision remain gates. Normal ChatGPT/Lucy full read/write MCP is separately limited by current product access.

## Storage and recovery

The domain model remains independent of SurrealDB. The local adapter uses embedded, versioned SurrealKV for the normal path and keeps an engine-independent logical export for recovery and migration. The service binds locally by default; remote access, hosted memory, and raw database mutation are not implicit parts of the architecture.
