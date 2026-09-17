# Context Compiler

Status: **canonical current architecture; deterministic implementation merged to `master`**

The Context Compiler turns a request into a bounded, inspectable Context Pack. It is a read projection over evidence and memory; it is not a new source of truth.

## Current behaviour

The implementation combines deterministic typed Belief candidates and lexical soft-memory candidates.

Packs include:

- stable IDs and typed holder/perspective sections, such as human, assistant, shared and legacy;
- current and historical belief separation;
- explicit stale and superseded exclusions for ordinary current queries;
- Source and Episode references from selected evidence;
- candidate scores and reasons;
- selected and omitted IDs;
- persisted retrieval Traces;
- item and token budgets with deterministic tie-breaking;
- hardened lexical matching that avoids generic holder identity and substring false positives.

History wording can request the historical projection. Project hints are accepted and traced, but remain deterministic lexical seeds until a graph-backed project index is deliberately added.

## Why bounded context matters

Long transcripts and giant memory dumps are easy to produce and hard to trust. A Context Pack should make it possible to inspect not only what was included, but why it was included and what was left out.

The compiler therefore prefers a smaller explainable result over an apparently complete but untraceable context blob.

## Non-negotiable boundaries

- Context Packs are disposable projections, not evidence.
- A pack must not be fed back as self-citation.
- Weak evidence should produce less context or visible uncertainty, not invented remembered context.
- Retrieval failure must be visible.
- Semantic retrieval may propose candidates, but it must not create a second ungoverned truth path.

## Follow-up work

Exact typed predicate lookup, richer full-text filtering, graph-backed project associations, bounded graph expansion and optional semantic candidates remain follow-up lanes. They should extend this compiler rather than create parallel context semantics.
