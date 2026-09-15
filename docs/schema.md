# Lantern Schema

The durable model is one graph with several record types:

| Plane | Records | Meaning |
| --- | --- | --- |
| Evidence | Source, Episode, Claim | What happened and what was asserted |
| Projection | Belief | Current reconciled understanding |
| Soft memory | Memory Item | Ideas, quotes, fragments, lessons, and other possibilities |
| Governance | Trace, Proposal | Why a projection or candidate exists |
| Organisation | Project, typed relations | Useful scope and graph structure |

The core types live in `lighting-core` and have no storage dependency. The
SurrealDB adapter lives in `lighting-store-surreal`; its current epistemic
migration is schema version 10. Records carry stable IDs, timestamps, and deterministic keys where
replay matters. Sources, Episodes, and Claims have no ordinary update/delete
repository operations.

Belief identity is `holder + subject + predicate + canonical scope`, not merely
subject plus predicate. `scope_hash` is calculated from sorted key/value pairs.
Stale is independent of belief state and increases dependency generation.
Belief projections retain Claim IDs and prior Belief IDs in their lineage;
value/state changes are also written to immutable `belief_revision` records.
Context Packs are bounded, stable-ID read projections persisted with their
typed selected records and retrieval trace. Foreman decisions update only
Proposal state and append a Trace; they do not promote canonical Beliefs.
