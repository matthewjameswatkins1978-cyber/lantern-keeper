# Lantern Keeper — Current

## Repository

- Canonical branch: `master`
- Canonical engineering merge: `4e40e48f4b336cebce216338c9a4bdbc02e26ba6`
- `feature/lantern-full-move` has been merged into `master` through PR #3.
- The merge reconciled the later master documentation checkpoint without force-pushing either line.
- This is an engineering checkpoint, not the final Basic Memory cutover or a public release.

## Phase

The epistemic memory foundation is now canonical on `master`. Foundation modernisation, spring cleaning, Basic Memory import accounting, durable Claim-to-Belief reconciliation, correction, stale invalidation, deterministic Context Packs, bounded Foreman review, logical export/restore foundations, LanternBench, and the bounded MCP bridge are merged.

## Works today

- Rust 1.98.1 / Edition 2024 and SurrealDB 3.3.0-beta.4 are pinned.
- Embedded, versioned SurrealKV is the normal local store.
- Sources, Episodes, Projects, Claims, Beliefs, soft Memory Items, relations, registries, traces, proposals, export, import accounting, recall, context, and the optional Tethers preview are present.
- Direct Claims can be durably reconciled into current Beliefs. Supersession preserves the old projection, records immutable revisions, and retains Claim-to-Belief lineage.
- Direct Matthew corrections are preserved as Source/Episode evidence and can be linked to the Context Pack that influenced the correction.
- Deterministic Context Packs provide typed Matthew/Lucy/shared/legacy sections, current/historical separation, bounded selection, stale exclusions, provenance, candidate reasons, budgets, persistence, and compiler traces.
- The stdio MCP bridge exposes exactly eight bounded tools: context, remember, search, why, correct, status, Foreman queue, and Foreman review.
- Factual `kind: "claim"` capture requires exact `evidence_text`; Lantern creates or reuses the Source and whole-text Episode, links the UTF-8 byte span, and reconciles the normalized value.
- Codex Desktop is connected with all eight tools. Remember/search/context/why/correct, current/history separation, initial provenance, correction provenance, and restart persistence are proven.
- Context matching has been hardened against holder-only and lexical false positives.
- Predicate/dimension normalization, explicit unmapped Claims, echo suppression, direct-holder gates, and transitive stale propagation are implemented.
- The repaired Basic Memory snapshot accounts for 61 notes, 496 observations, and 275 relations with zero unexplained items.
- A real 1,479-record logical export/restore parity drill has passed.

## Still missing or partial

- Basic Memory shadow comparison against representative real memories.
- Final Basic Memory delta, idempotency/accounting check, private export, and cutover decision.
- Service-level representative retrieval after a fresh restore remains to be accepted.
- Exact typed/fused retrieval, richer graph-backed project associations, bounded graph expansion, and Dreamer operations remain later work.
- Normal ChatGPT/Lucy full read/write custom MCP access is a separate OpenAI product-access limitation. Codex Desktop integration is already proven and must not be confused with that product gate.

## Environment

The Rust MSVC target uses the installed Microsoft C++ Build Tools and Windows SDK libraries. The validated user-level library paths exclude the unusable ATL/MFC-only entry. Lantern does not require the Visual Studio IDE or a particular editor.

## Recovery and private state

Private migration material remains in `.private-migration/` and must not be committed or deleted. Recovery/archive refs remain available for historical safety. Basic Memory remains the migration source and rollback/history archive until cutover is accepted.

## Next verified step

Run the Basic Memory shadow comparison, fix mechanism-level discrepancies only, then perform the final Basic Memory delta and export/restore acceptance. If those gates are green, record the cutover decision. Do not tag or publish a release merely because the engineering foundation is now on `master`.
