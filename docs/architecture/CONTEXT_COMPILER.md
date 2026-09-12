# Context Compiler

Status: **CANONICAL CURRENT — implementation partial**

The current service keeps the existing deterministic Memory context path. The
next canonical compiler will combine direct Belief reads, soft-memory recall,
episode/source evidence, graph relations, perspective, and explicit stale
markers into a bounded Context Pack. A stale Belief is omitted when
unnecessary and labelled unresolved when it is necessary; it is never silently
repaired by a model.

Context Packs are disposable read projections with traceable selected and
omitted IDs. They are not evidence and must not be fed back as self-citation.
