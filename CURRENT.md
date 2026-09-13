# Lantern Keeper — Current

## Repository

- Active line: `feature/lantern-full-move`
- Current feature checkpoint: `df956d26a333d042ff325f5c909d6767a4a8a3ed`
- Canonical `master`: `a44554b7050e9b3dd2178eede99b67184d89d12c`
- Feature checkout is clean at the pushed durable-reconciliation checkpoint.
- `master` remains unchanged by the feature work.

## Phase

Foundation modernisation and Basic Memory import accounting are complete.
Spring cleaning is complete. Durable Claim-to-Belief reconciliation and
immutable Belief revision history are now the active implementation phase.

## Works today

- Rust 1.98.1 / Edition 2024 and SurrealDB 3.3.0-beta.4 are pinned.
- Embedded, versioned SurrealKV is the normal local store.
- Sources, Episodes, Projects, Claims, Beliefs, soft Memory Items, relations,
  registries, traces, proposals, export, import accounting, lexical recall,
  context, and the optional Tethers preview are present in the tree.
- Direct Claims can be durably reconciled into current Beliefs. Supersession
  preserves the old projection, records immutable revisions, and retains
  Claim-to-Belief lineage in one transactional store operation.
- Predicate and dimension normalization, explicit unmapped Claims, pure
  reconciliation decisions, echo suppression, direct-holder gates, and
  transitive stale propagation are implemented.
- The repaired Basic Memory snapshot accounts for 61 notes, 496 observations,
  and 275 relations with zero unexplained items.

## Still missing or partial

- Correction workflow, persisted Context Pack traces, and stale-aware reads.
- Fused retrieval, bounded graph projection, expanded LanternBench,
  Foreman/Dreamer operations, restore proof, and a real Lucy-native MCP path.
- Basic Memory shadow comparison, final delta, and cutover.

## Environment

The Rust MSVC target needs the installed Microsoft C++ Build Tools and Windows
SDK libraries, but Lantern does not require the Visual Studio IDE or a
particular editor. Plain `pwsh -NoProfile` validation is the acceptance target.

## Recovery and private state

The original unfinished reconciliation draft remains preserved in the stash named
`preserve unfinished reconciliation draft before spring clean 2026-09-13` and
at `recovery/pre-spring-clean-c2cdfd9`. Private migration material remains in
`.private-migration/` and must not be committed or deleted.

## Next verified step

Implement correction and stale-aware reads, then checkpoint them on
`feature/lantern-full-move`; leave `master` untouched until all cutover gates
are green.
