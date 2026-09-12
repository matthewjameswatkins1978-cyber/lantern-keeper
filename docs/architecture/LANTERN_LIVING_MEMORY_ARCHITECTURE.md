# Lantern Living Memory Architecture

Status: **CANONICAL CURRENT**

Lantern Keeper is a local-first shared memory for humans and AI. Sources
preserve what happened; Claims preserve what was asserted; Beliefs preserve
current reconciled understanding; and Memory Items preserve useful material
that is not ready to become truth. Lucy is the routine Memory Foreman and
Matthew remains the ultimate authority about Matthew.

## Current vertical slice

The repository now contains the typed epistemic vocabulary and durable
SurrealDB-backed stores for Claims, Beliefs, soft Memory Items, Traces, and
Proposals. The HTTP surface exposes capture, belief projection/listing and
explicit stale invalidation, plus soft-memory capture/search. The existing
Source, Episode, project, ledger, retrieval, export, and Tethers-preview
surfaces remain intact.

Claims are append-only. Beliefs are read projections and may be invalidated
without changing their truth state. Staleness is therefore separate from
`active`, `disputed`, `superseded`, and `archived` state. Imported Basic Memory
material remains evidence until a later reconciliation step assigns an
appropriate trust class.

The experimental SurrealDB adapter stores an engine-independent JSON payload
with indexed operational fields. This keeps the domain model independent of
SurrealDB while allowing deterministic deduplication and safe recovery export.

## Governing constitution

> Matthew talks. Lucy remembers. Machines assist. Matthew corrects.

> Remember generously. Assert cautiously.

> Authorship, transmission, endorsement and belief ownership are separate.

The full cutover remains gated on migration verification, retrieval behaviour,
correction tests, and a real Lucy-native client path.
