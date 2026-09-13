# Lantern Keeper — Current

## Repository

- Active line: `feature/lantern-full-move`
- Current feature checkpoint: LanternBench behavioural checkpoint (`83e1a9f`)
- Canonical `master`: `a44554b7050e9b3dd2178eede99b67184d89d12c`
- Feature checkout is clean at the pushed correction-search checkpoint.
- `master` remains unchanged by the feature work.

## Phase

Foundation modernisation and Basic Memory import accounting are complete.
Spring cleaning is complete. Durable Claim-to-Belief reconciliation,
correction, stale invalidation, deterministic Context Packs, bounded Foreman
review, logical export/restore, and the current LanternBench behavioural suite
are now implemented on the feature line.

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
  source/episode provenance, candidate scores and reasons, retrieval lanes,
  item/token budget enforcement, persistence, and compiler traces. Ordinary
  current queries do not inject superseded beliefs; history wording or a named
  historical value can request the historical projection.
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
- Exact typed/fused retrieval, graph-backed project associations, bounded graph
  projection, Dreamer operations, and a real Lucy-native MCP path remain.
- Restore has passed a real 1,479-record export/restore parity drill, but a
  service-level retrieval-after-restore and real Lucy restart proof remain.
- Full MCP contract coverage, real Lucy-native proof, Basic Memory shadow
  comparison, final delta, and cutover.

## Environment

The Rust MSVC target needs the installed Microsoft C++ Build Tools and Windows
SDK libraries. The user-level `LIB` and `LIBPATH` values now point to the
validated VC and Windows SDK x64 libraries, excluding the unusable ATL/MFC
entry. Lantern does not require the Visual Studio IDE or a particular editor;
plain `pwsh -NoProfile` validation passes after opening a new shell so it
inherits the updated user environment.

## Recovery and private state

The original unfinished reconciliation draft remains preserved in the stash named
`preserve unfinished reconciliation draft before spring clean 2026-09-13` and
at `recovery/pre-spring-clean-c2cdfd9`. Private migration material remains in
`.private-migration/` and must not be committed or deleted.

## Next verified step

Run the LanternBench checkpoint, then prove the real Lucy-facing MCP sequence
across a Lantern restart. Keep `master` untouched until every cutover gate is
green.
