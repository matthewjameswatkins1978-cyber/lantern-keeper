# Lantern Warden — Current

## Repository

- Canonical branch: `master`
- Canonical engineering/documentation tip: `79da8f83c1f41b8ba7ad7e92cb89fdfc9f14c4bf`
- `feature/lantern-full-move` has been merged into `master` through PR #3.
- The hackathon authority/evidence work was merged through PR #4 at
  `79da8f83c1f41b8ba7ad7e92cb89fdfc9f14c4bf`; its verified pre-merge head was
  `194c7c1d1c014d9dbf574569101642746fe7d142`.
- Active development branch: `master`
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
- Authority grants, revocations, and execution receipts are persisted in
  SurrealKV and reconstructed after restart. Logical export/restore preserves
  active and revoked authority plus receipts and their Source/Episode audit
  provenance. Control sessions and CSRF tokens remain deliberately ephemeral
  and are not exported or persisted as reusable authentication material.
- Authority mutation provenance is established by a service-created canonical
  PlainText Source and full-range Episode before the durable grant or
  revocation is written. Source evidence remains explanatory only; it cannot
  semantically create or revoke authority. Persistent read failures return an
  explicit unavailable state and fail closed.

## Milestone 4 closeout checkpoint

The companion Tethers M4 branch now contains a capability-specific OpenShell
executor and a real opt-in boundary test. On the verified Windows WSL2/Docker
workstation, OpenShell 0.0.116 created a hard-Landlock sandbox: the approved
synthetic summary write/read succeeded, the forbidden path was denied, HTTPS
egress returned the sandbox proxy's 403, and no Lantern control credential or
audit token was present in the sandbox environment. The executor uses the
existing Tethers supervised-child owner and has no unsandboxed fallback or
retry.

The local effect boundary is verified, and the cross-repository run connecting
a live Lantern authority decision receipt and outcome receipt to the OpenShell
effect has now passed locally. The run used a local authenticated development
service. This working checkpoint does not claim Nebius deployment, a controlled
hostile website, or a public demo.

## Milestone 5 working checkpoint

The live cognitive plane is now verified on the M5 branch. Tavily basic search
results are retained as explicitly external evidence with query, URL, domain,
title, retrieval time, rank, provider metadata, and a normalized hash. The
bounded orchestration path persists each result as a Source and Episode before
passing it to the candidate-only Nemotron Dreamer. A live Nebius Token Factory
call using `nvidia/nemotron-3-super-120b-a12b` produced a validated candidate;
the combined live test confirmed that evidence provenance survived persistence
and that neither canonical memory nor authority changed.

Provider keys remain environment-only. Provider failure is typed and fail
closed, model metadata is server-owned, and model confidence is not proof.

## Still missing or partial

- Basic Memory shadow comparison against representative real memories.
- Final Basic Memory delta, idempotency/accounting check, private export, and cutover decision.
- Service-level representative retrieval after a fresh restore remains to be accepted.
- Exact typed/fused retrieval, richer graph-backed project associations, bounded graph expansion, and Dreamer operations remain later work.
- Normal ChatGPT/Lucy full read/write custom MCP access is a separate OpenAI product-access limitation. Codex Desktop integration is already proven and must not be confused with that product gate.
- The trust console, hosted demo, and Nebius/Tavily public presentation remain
  incomplete. The OpenShell effect boundary and live cross-repository Lantern
  receipt run are locally verified on the M4/M5 working branches. The offline CI
  lane is green on this branch; the local full workspace test command remains
  subject to intermittent Windows linker resource exhaustion.

## Environment

The Rust MSVC target uses the installed Microsoft C++ Build Tools and Windows SDK libraries. The validated user-level library paths exclude the unusable ATL/MFC-only entry. Lantern does not require the Visual Studio IDE or a particular editor.

## Recovery and private state

Private migration material remains in `.private-migration/` and must not be committed or deleted. Recovery/archive refs remain available for historical safety. Basic Memory remains the migration source and rollback/history archive until cutover is accepted.

## Next verified step

Run the Basic Memory shadow comparison, fix mechanism-level discrepancies only, then perform the final Basic Memory delta and export/restore acceptance. If those gates are green, record the cutover decision. Do not tag or publish a release merely because the engineering foundation is now on `master`.
