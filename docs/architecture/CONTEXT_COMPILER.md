# Context Compiler

Status: **CANONICAL CURRENT — deterministic first slice implemented**

The canonical compiler now combines deterministic typed Belief candidates and
lexical soft-memory candidates into a bounded Context Pack. Packs have stable
IDs, typed Matthew/Lucy/shared/legacy sections, explicit stale exclusions,
current-versus-historical belief sections, source and episode references from
selected evidence, candidate scores, and a persisted retrieval trace.
The service and `lighting context-pack` CLI expose this first slice.

The remaining retrieval lanes are deliberate follow-up work: exact typed
predicate lookup, full-text and project filtering, richer history queries,
bounded graph expansion, and optional semantic candidates. They must extend
this compiler rather than create a second context path.

Context Packs are disposable read projections with traceable selected and
omitted IDs. They are not evidence and must not be fed back as self-citation.
