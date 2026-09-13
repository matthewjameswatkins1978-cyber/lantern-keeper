# Lantern Keeper — Current

## Repository

- Active line: `feature/lantern-full-move`
- Current feature checkpoint: `b2b7aa7`
- Canonical `master`: `a44554b7050e9b3dd2178eede99b67184d89d12c`
- Feature checkout is clean at the pushed MCP and correction-search checkpoint.
- `master` remains unchanged by the feature work.

## Phase

Foundation modernisation and Basic Memory import accounting are complete.
Spring cleaning is complete. Durable Claim-to-Belief reconciliation,
correction, stale invalidation, deterministic Context Packs, and bounded
Foreman review are now implemented on the feature line.

## Works today

- Rust 1.98.1 / Edition 2024 and SurrealDB 3.3.0-beta.4 are pinned.
- Embedded, versioned SurrealKV is the normal local store.
- Sources, Episodes, Projects, Claims, Beliefs, soft Memory Items, relations,
  registries, traces, proposals, export, import accounting, lexical recall,
  context, and the optional Tethers preview are present in the tree.
- Direct Claims can be durably reconciled into current Beliefs. Supersession
  preserves the old projection, records immutable revisions, and retains
  Claim-to-Belief lineage in one transactional store operation.
- Direct Matthew corrections are preserved as Source/Episode evidence and can
  be linked to the exact Context Pack that influenced the correction.
- Deterministic Context Packs have stable IDs, typed Matthew/Lucy/shared/legacy
  sections, separate historical beliefs, bounded selection, stale exclusions,
  source/episode provenance, candidate scores, retrieval lanes, persistence,
  and compiler traces.
- Belief inspection supports get, lexical search, immutable history,
  provenance explanation, and stale listing. Foreman supports a bounded queue
  and traced accept/reject/modify/defer decisions without direct belief
  promotion.
- A local stdio MCP bridge now exposes bounded context, remember, search,
  provenance, correction, status, and Lucy-owned Foreman tools over the
  existing HTTP service. `lantern_why` supports belief IDs, memory IDs, and
  ambiguity-safe natural queries. A real supported Lucy client proof is still
  required.
- Predicate and dimension normalization, explicit unmapped Claims, pure
  reconciliation decisions, echo suppression, direct-holder gates, and
  transitive stale propagation are implemented.
- The repaired Basic Memory snapshot accounts for 61 notes, 496 observations,
  and 275 relations with zero unexplained items.

## Still missing or partial

- Natural-language correction targeting is now ambiguity-safe but requires one
  uniquely matching active belief; full response-context bridge linkage remains
  partial.
- Exact typed/fused retrieval, bounded graph projection, expanded LanternBench, Dreamer
  operations, restore proof, and a real Lucy-native MCP path remain.
- Full MCP contract coverage, real Lucy-native proof, Basic Memory shadow
  comparison, final delta, and cutover.

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

Run the complete workspace lane, then expand Context Compiler retrieval and
LanternBench behaviour before beginning MCP or cutover work. Keep
`master` untouched until every cutover gate
are green.
