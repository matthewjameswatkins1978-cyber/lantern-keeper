# Context Compiler

Status: **canonical current architecture; deterministic first slice merged to `master`**

The Context Compiler turns a request into a bounded, inspectable Context Pack. It is a read projection over evidence and memory; it is not a new source of truth.

## Current behaviour

The canonical implementation combines deterministic typed Belief candidates and lexical soft-memory candidates. Packs have:

- stable IDs and typed Matthew, Lucy, shared, and legacy sections;
- current and historical belief separation;
- explicit stale and superseded exclusions for ordinary current queries;
- Source and Episode references from selected evidence;
- candidate scores, reasons, selected/omitted IDs, and a persisted retrieval Trace;
- item and token budgets with deterministic tie-breaking;
- hardened lexical matching that avoids generic holder identity and substring false positives.

History wording can request the historical projection. Project hints are accepted and traced, but remain deterministic lexical seeds until a graph-backed project index is deliberately added.

## Non-negotiable boundaries

- Context Packs are disposable projections, not evidence.
- A pack must not be fed back as self-citation.
- The compiler returns less context with uncertainty when evidence is weak; it does not invent a missing answer.
- Retrieval failure must be visible and must not silently become fabricated remembered context.

Exact typed predicate lookup, richer full-text filtering, graph-backed project associations, bounded graph expansion, and optional semantic candidates remain follow-up lanes. They must extend this compiler rather than create a second context path.
