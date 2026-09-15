# Lantern Keeper — Current

## Repository

- Canonical branch: `master`
- Canonical engineering/documentation tip: `8cfd05461f0534c71872137f74acc0b6c959703a`
- `feature/lantern-full-move` has been merged into `master` through PR #3.
- Active hackathon branch: `codex/nebius-authority-foundation`
- This checkout contains the canonical epistemic foundation plus the hackathon
  authority, Dreamer, evidence, receipt, and CI slices.
- This is an engineering checkpoint, not a public release.

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
- `PrincipalId`, `AuthorityGrant`, `AuthorityRevocation`, exact authority
  checks, expiry/revocation handling, and a deterministic authority matrix are
  present. Authority writes are restricted to a trusted bootstrap-created,
  principal-bound control session and CSRF token. Public mutation routes accept
  only intent DTOs; issuer, session ID, timestamps, IDs, and provenance are
  server-owned, and cross-principal revocation is rejected.
- The optional Nebius Dreamer adapter returns strictly validated candidates and
  cannot mutate canonical memory or authority. Invalid model output fails
  closed.
- Execution receipts have canonical JSON, SHA-256 hashes, previous-receipt
  links, and tamper/continuity tests. Hash chaining proves content continuity
  and order, not signer identity.

## Still missing or partial

- Basic Memory shadow comparison against representative real memories.
- Final Basic Memory delta, idempotency/accounting check, private export, and cutover decision.
- Service-level representative retrieval after a fresh restore remains to be accepted.
- Exact typed/fused retrieval, richer graph-backed project associations, bounded graph expansion, and Dreamer operations remain later work.
- Normal ChatGPT/Lucy full read/write custom MCP access is a separate OpenAI product-access limitation. Codex Desktop integration is already proven and must not be confused with that product gate.
- The authority service is currently process-local and is not yet persisted in
  SurrealKV. Its generated `authority-control-*` Episode IDs are provenance
  placeholders, not persisted Lantern Source/Episode records. The Tethers provider/Trail bridge, live Tavily evidence,
  genuine OpenShell enforcement, trust console, hosted demo, and live Nebius
  proof remain incomplete. The new offline CI lane is green on this branch.

## Environment

The Rust MSVC target uses the installed Microsoft C++ Build Tools and Windows SDK libraries. The validated user-level library paths exclude the unusable ATL/MFC-only entry. Lantern does not require the Visual Studio IDE or a particular editor.

## Recovery and private state

Private migration material remains in `.private-migration/` and must not be committed or deleted. Recovery/archive refs remain available for historical safety. Basic Memory remains the migration source and rollback/history archive until cutover is accepted.

## Next verified step

Run the Basic Memory shadow comparison, fix mechanism-level discrepancies only, then perform the final Basic Memory delta and export/restore acceptance. If those gates are green, record the cutover decision. Do not tag or publish a release merely because the engineering foundation is now on `master`.
