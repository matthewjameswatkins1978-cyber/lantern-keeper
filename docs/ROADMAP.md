# Lantern Keeper Roadmap

Lantern Keeper is a local-first shared memory layer for Matthew and Lucy.
Sources preserve evidence; Claims preserve assertions; Beliefs preserve
current reconciled understanding; Memory Items preserve useful possibilities.

The foundation, storage baseline, typed epistemic vocabulary, registry work,
import accounting, and first deterministic retrieval slice are complete.

## Next implementation order

1. Persist Claim-to-Belief reconciliation, history, lineage, and traces.
2. Complete correction handling and eager stale invalidation with lazy repair.
3. Build the bounded Context Compiler and provenance explanation path.
4. Add deterministic retrieval lanes and a bounded associative graph projection.
5. Expand LanternBench around attribution, scope, history, correction, and
   false-positive resistance.
6. Complete the Lucy-owned Foreman queue and restrained Dreamer proposals.
7. Prove export/restore/restart parity and the local Lucy-native MCP contract.
8. Run Basic Memory shadow comparison and the final delta before reversible
   cutover.

## Later, optional

Embeddings, richer graph ranking, automatic host capture, narrative views,
additional connectors, GUI, cloud deployment, accounts, multi-user support,
and a broader ontology remain optional. None is required for the current
local Matthew/Lucy product path.

## Constraints

- SurrealDB remains canonical; derived indexes must be rebuildable.
- Lucy is the routine Memory Foreman, not a second source of truth.
- Inference cannot silently become Matthew's belief.
- Retrieval may abstain when evidence is absent or conflicting.
- Basic Memory remains a migration source and rollback archive until cutover.
