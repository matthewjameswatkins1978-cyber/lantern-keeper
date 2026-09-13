# Context Compiler

Status: **CANONICAL CURRENT — deterministic first slice implemented**

The canonical compiler now combines deterministic typed Belief candidates and
lexical soft-memory candidates into a bounded Context Pack. Packs have stable
IDs, typed Matthew/Lucy/shared/legacy sections, explicit stale exclusions,
current-versus-historical belief sections, source and episode references from
selected evidence, candidate scores and reasons, item and token budgets, and a
persisted retrieval trace. Ordinary current queries exclude superseded beliefs;
history wording or a directly named historical value opts the historical
projection back in. The service, MCP bridge, and `lighting context-pack` CLI
expose this first slice.

The remaining retrieval lanes are deliberate follow-up work: exact typed
predicate lookup, full-text filtering, richer project/association queries,
bounded graph expansion, and optional semantic candidates. Project hints are
accepted and traced, but are currently used as additional deterministic lexical
seeds rather than as a separate graph-backed project index. They must extend
this compiler rather than create a second context path.

Context Packs are disposable read projections with traceable selected and
omitted IDs. They are not evidence and must not be fed back as self-citation.
