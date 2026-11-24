# LK-CUTOVER-001 — synthetic smoke instructions (hosted candidate)

Scope: prove the hosted candidate on real workshop clients with **synthetic
data only**. No live Basic Memory, no production traffic, no secrets in this
document (names only). Work runs provisioning/deploy; these steps assume the
candidate is deployed and bound to the empty D1 database.

Baseline revision: `e9a4d4823c93758d8e76749706dfc0e1e0db637c` on
`cutover/d1-seam` (re-pin on every adoption push; a smoke doc that cannot
be tied to a revision is not evidence).

## 0. Preconditions

- Worker bound to the `lantern-keeper` D1 database (Work).
- Schema applied from `crates/lighting-store-d1/migrations/` (Work, checked-in
  migration semantics: idempotent DDL + `schema_version` row).
- Secrets **names** (values from Work's secret store, never in chat or Git):
  `LIGHTING_MCP_OAUTH_AUDIENCE`, `LIGHTING_MCP_OAUTH_JWKS_URL`,
  `LIGHTING_MCP_OAUTH_SUBJECT`, `LIGHTING_MCP_OAUTH_RESOURCE`.
  Provenance (honest): these names are defined by the reviewed-but-unmerged
  OAuth adapter on branch `codex/lantern-chatgpt-read-safety`
  (`apps/lighting/src/mcp_auth.rs`). **No code on `cutover/d1-seam` reads
  them yet.** The hosted worker (C2) will either adopt these names as its
  contract or record its own names here; until then, no reader should look
  for their definition on this branch.
  Until the Auth0 tenant is verified (separate dependency), smoke runs use a
  static-Bearer dev route **explicitly labelled non-production**; that route
  proves transport and scoping, never production auth.
- Client: any workshop MCP host (Pi / Codex / ChatGPT) pointed at the hosted
  endpoint with its own authenticated identity.

## 1. Durable write (accepted path)

**Scoped to corrections**: keyed idempotency landed only for the correction
path (`f76ccf3`). A `lantern_remember` with a caller key does not exist yet;
do not smoke what is not built.

1. Seed one belief in scope `project=smoke` with content `synthetic-1`
   (seeded fixture, then read its current revision R via `lantern_why`).
2. `lantern_correct` with idempotency key `K1`, `expected_revision = R`,
   new content `synthetic-1-fixed`.
3. Expect a mutation receipt (id, `lantern-keeper-write-envelope-v1`,
   accepted_at).
4. `lantern_search` for `synthetic-1-fixed` in the same scope: must appear.
5. Verify D1 persistence: a fresh client session (or restart) must still see
   `synthetic-1-fixed`. **Persistence check is mandatory** — a write that only
   survives in the original session is a failure.

## 2. Failure path (nothing persisted)

1. `lantern_correct` with key `K2` and an **invalid** envelope (e.g. blank
   content): expect a typed `invalid` rejection.
2. `lantern_search` scoped to `project=smoke`: must NOT show any `K2` record.
3. Row-count check (D1 `meta` or export counts) must be unchanged.

## 3. Contested correction (revision precondition)

1. `lantern_why` to read the belief's current revision R.
2. Client A: `lantern_correct` with `expected_revision = R`, key `K3` —
   expect accepted, new revision R'.
3. Client B (simultaneously, same target): `lantern_correct` with
   `expected_revision = R`, key `K4` — expect **typed `revision_conflict`**
   and **nothing written** (row counts unchanged).
4. `lantern_why` must show exactly one accepted correction lineage; the
   rejected one must leave no partial evidence.

**Expected HTTP mapping (to be proven at C2 worker deploy, not yet
demonstrated)**: typed `invalid` → 400, `revision_conflict` and
`idempotency_conflict` → 409. No error→HTTP code in this doc may be cited
as proven until the worker serves it.

## 4. Retry / idempotent replay

1. Replay client A's exact request (same key `K3`, identical payload):
   expect the **original** receipt (same id), no new records.
2. Replay with same key `K3` but a **changed** payload: expect
   `idempotency_conflict` (→ 409 when the worker mapping lands), nothing written.
3. A different principal replaying `K3` under their own identity: expect a
   **distinct** write (binding, not global contention — by design).

## 5. Zero-write reads

1. Record D1 write-row counters (or export counts) before.
2. Run `lantern_status`, `lantern_search`, `lantern_context`, `lantern_why`
   a dozen times across scopes.
3. Write-row counters must be **unchanged**. Hosted reads persist nothing.

## 6. Cross-scope isolation

1. Scope `project=smoke` holds records; scope `project=other` must not see
   them (exact normalized-scope equality — deliberately fails closed).
2. A large inaccessible scope must not influence the small authorised
   scope's ranking, counts, or scanned work.

## 7. Budget and failure behaviour

- Candidate fetch is capped (128 total) by a LIMIT-clamped statement; this
  crate carries no truncation flag, so an over-cap query is visible as
  "fewer rows than requested", never as an unbounded scan. If the service
  layer (C2) adds an explicit truncation flag, this step will assert it.
- Forced quota/limit error (if reproducible safely): expect a typed
  `quota`/`unavailable` error, no silent partial success, no retry storm.

## Pass criteria

Every step's expected result observed, with the persistence and
nothing-written checks actually run (not assumed). Report pass/fail per
step with the candidate SHA. Any failure: report the exact step, the typed
error actually returned, and whether anything was written.
