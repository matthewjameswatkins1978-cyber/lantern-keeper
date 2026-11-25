# Hosted Seam — M1 Design Note

**Mission:** LK-HOST-001 · slice M1 (design only)
**Author:** Pi worker [pi:01a206e6-4a40] · 23 November 2026
**Basis:** read-only audit of worktree `D:/Projects/lantern-m1-seam`, branch `feat/m1-hosted-storage-seam` at `ac619cca225d2aa44816e54e2802749391ffa015` (HEAD verified, worktree clean before writing). No code changes, no provisioning, no tests run in this slice. All line references are to that revision. Switchboard references are to `D:/Projects/switchboard` at `12bd4b6`, read-only.

**Status:** design input for Muse. Decisions marked *proposed* require the M0 feasibility proof and Muse's confirmation.

---

## 1. `lighting-service` function map — portable vs store-bound

### Tier A — portable now (`lighting-core`, no I/O)

Pure domain logic and types, no repository or runtime dependencies:

- Reconciliation/projection: `reconcile_claim` (`epistemic.rs:789`), `propagate_stale_beliefs` (`:855`), `Belief::mark_stale(at)` (`:338` — already takes injected time).
- Scope/registry: `scope_hash` (`:718`), `normalize_scope` (`:741`), `scopes_overlap` (`:763`), `normalize_registry_key` (`:906`).
- Domain constructors: `Claim::new` (`:197`), `Belief::new` (`:308`), `MemoryItem::new` (`:410`), `Actor::new` (`:152`), `NewGraphRelation::new` (`:572`).
- Evidence primitives: `SourceFingerprint::for_content` (SHA-256, pure) and the injection precedent `Source::reconstitute` (`source.rs:249-265` — id/time supplied by caller).
- `authority.rs`, `receipt.rs`, `ledger.rs`, `memory_path.rs`, `source_outline.rs`, `bridge_intent.rs` domain types are structurally portable, with the clock/ID caveats in §2.

### Tier B — pure but stranded in `lighting-service` (extraction candidates)

Private free functions and deterministic phases in `epistemic_ops.rs`:

- `context_tokens` (`:1553`), `belief_relevance_details` (`:1641`), `memory_relevance_details` (`:1683`), `render_context` (`:1729`), `insert_historical_beliefs` (`:1769`), `push_unique` (`:1830`).
- Inside `compile_context` (`:956`): candidate scoring/filter/sort loops (`:979-1051`), budget selection (`:1052-1083`), holder partitioning and pack assembly (`:1114-1213`). All deterministic given inputs.
- **Proposed seam cut:** extract a pure `compile_select(...)` taking `(query, scope, intent, project_hints, budgets, belief_candidates, memory_candidates, lineage_refs, now, pack_id)` → `ContextPack`. The service keeps bounded fetching and Trace persistence; `Utc::now()` (`:1192`) and `Uuid::new_v4()` (`:1084`, `:1217`) become injected parameters (reconstitute-style, per Muse's seam input).

### Tier C — store-bound orchestration (the hosted adapter implements these)

- Every service struct holds `Arc<dyn …Repository>` over traits declared in `lighting-core`: `EpistemicService` (`epistemic_ops.rs:92-108`), `SourceService` (`source_ops.rs:46`), `MemoryService` (`memory_ops.rs:45`), `ProjectService` (`project_ops.rs:48`), plus marker/episode/association/ledger/bridge-intent/retrieval services. **These traits are the storage seam**; a D1 adapter implements them, native Surreal keeps implementing them.
- **Direct Surreal coupling to contain:**
  - `authority_ops.rs:11-30` and `:85-94` — `AuthorityService` holds concrete `SurrealAuthorityRepository` / `SurrealSourceRepository` / `SurrealMemoryPathRepository` / `SurrealReceiptRepository` and `SurrealStore`, not dyn traits. *Proposed:* authority stays local per packet §4 (no authority grants on the hosted surface), so the smallest cut is excluding `authority_ops` from the hosted build rather than extracting traits now.
  - `cognitive_ops.rs:140` Surreal import is test-only (`#[cfg(test)]` at `:134`).
  - `lighting-service/Cargo.toml` carries a path dependency on `lighting-store-surreal`; the hosted build must drop or feature-gate it (wiring belongs in `apps/lighting`).
- **Unbounded work inventory inside `compile_context` (must be fixed at the seam):**
  1. `list_beliefs(true)` (`:964-968`) — store query has no `LIMIT` (`epistemic_store.rs:133-137`).
  2. `search_memory_items(limit: 0)` (`:971-977`) — store appends `LIMIT` only when `limit > 0` (`epistemic_store.rs:389-397`, verified).
  3. `list_claims(false)` (`:1143-1147`) — full-table claim fetch for lineage resolution, no `LIMIT` (`epistemic_store.rs:80-87`). **This is a third unbounded fetch beyond the two `limit:0` sites; lineage must resolve via batched point lookups on selected claim IDs only.**
- **`limit == 0` semantics (verified):** `epistemic_store.rs:389-397` (memory search) and `:443-450` (proposals) only emit `LIMIT` when `limit > 0` — zero means *explicitly unbounded*, not a default page size. See §4 for the hosted contract choice.

### Tier D — native-only (not in the hosted Worker path)

`app.rs`/`*_routes.rs` (axum), tokio runtime, and outbound HTTP: `tavily.rs`, `dreamer_ops.rs` `NebiusDreamer::from_env` (`:122`), `tethers_engine_client.rs`. These stay native; the hosted profile exposes MCP/HTTP handlers over the same service API (pattern precedent: `docs/mcp.md` — both existing adapters call the service API, neither opens a store).

---

## 2. Wasm dependency audit (`lighting-core`)

### Production clock/RNG sites (exhaustive for this crate)

`Uuid::new_v4()` — **9 production sites** (matches the known count):

| File | Lines | Constructor |
|---|---|---|
| `epistemic.rs` | 223, 312, 415, 580 | `Claim` / `Belief` / `MemoryItem` / `Relation` |
| `memory.rs` | 163 | `Memory` |
| `memory_path.rs` | 193, 260, 316 | `Project::new` / `Episode` / `Marker` |
| `source.rs` | 23 (`SourceId::new`, called from `Source::create:243`) | `Source` |

`Utc::now()` — **4 production sites**: `source.rs:244` (`Source::create`), `memory_path.rs:196`, `:263`, `:319` (same constructors as the ID sites above — they generate *both* ID and time internally).

**Remaining 12 `Utc::now()` sites are test-only — verified** by scanning from each `#[cfg(test)]` anchor: `authority.rs:318/:451` (anchor `:287`), `dreamer.rs:113` (`:96`), `epistemic.rs:1057/:1059/:1077/:1134/:1173/:1209/:1231` (`:967`), `ledger.rs:120` (`:104`), `receipt.rs:149/:150` (`:132`). Production authority/dreamer/epistemic/ledger/receipt flows already take time as a parameter (e.g. `Belief::mark_stale(at)`), so the injection pattern exists in production code — Muse's claim confirmed.

**Other OS-clock/RNG paths in `lighting-core`: none.** Grep for `rand|thread_rng|getrandom|OsRng|tempfile|std::fs|std::thread|std::process|std::env|SystemTime|Instant::now` returns no matches. The `rand`/`getrandom` entries in `Cargo.lock` belong to native-only graph members (`surrealdb-*`, `rmcp`, `jsonwebtoken`, …), not to `lighting-core`, whose dependencies are exactly: `async-trait`, `chrono`, `serde`, `serde_json`, `sha2`, `thiserror`, `uuid`.

### What actually breaks on `wasm32-unknown-unknown` (verified against crate sources)

- **uuid 1.26.1 — hard compile failure today.** `rng.rs:95-101`: `compile_error!("to use uuid on wasm32-unknown-unknown, specify a source of randomness using one of the js, rng-getrandom, or rng-rand features")`. Workspace enables only `v4` (`Cargo.toml:44`, `v4 → rng → dep:getrandom`, and the `getrandom` dependency is `cfg(not(wasm32))`). With `js` (=`["dep:wasm-bindgen","dep:js-sys"]`), the wasm backend is WebCrypto via `wasm-bindgen` (`rng.rs:167-215`, `webcrypto::fill` → `crypto.getRandomValues`, panics if unavailable). Native builds are unaffected by adding `js` to a wasm-only target table.
- **chrono 0.4.45 — compiles but panics at runtime.** Workspace uses `default-features = false, features = ["clock","serde","std"]` (`Cargo.toml:30`); `wasmbind` is not enabled. Without it, `Utc::now()` on wasm32 uses `SystemTime::now()` (`utc.rs:90-101`) → runtime panic on `wasm32-unknown-unknown` ("time not implemented"). With `wasmbind` (`= ["wasm-bindgen", "js-sys"]`), it uses `js_sys::Date::new_0()` (`utc.rs:103-111`). Only `Utc` is used by core; `Local`/`iana-time-zone` on wasm is out of scope.

### Exact proposed target-cfg change

```toml
# crates/lighting-core/Cargo.toml — appended; no source edits required for the compile gate
[target.'cfg(target_arch = "wasm32")'.dependencies]
chrono = { workspace = true, features = ["wasmbind"] }
uuid   = { workspace = true, features = ["js"] }
```

Resolver 3 unifies features per target: native builds keep exactly today's graph; wasm builds gain `js`/`wasmbind`.

**Proposed division of labour (per Muse's seam direction):**

1. Prefer **reconstitute-style host-supplied time/ID** for everything crossing the seam (`Source::reconstitute` precedent; `compile_select` in §1 Tier B). This gives deterministic, fixture-paritable outputs independent of any clock.
2. The target-cfg features above cover the remaining *in-constructor* call sites (`Source::create`, `Project/Episode/Marker::new`, `Claim/Belief/MemoryItem/Relation::new`) when they run on the Worker. `js_sys::Date` time is host-observable, so hosted records must still be routed through governed `now` handling, not treated as semantic authority.
3. Service-side sites stay native unless extraction moves them: `compile_context` pack assembly (`epistemic_ops.rs:1084`, `:1192`) plus ~40 other `Utc::now`/`new_v4` uses across ops (`authority_ops.rs`, `dreamer_ops.rs`, `memory_ops.rs`, …).

**Unverified in this slice:** no wasm build was run (read-only design). The M0 time-boxed proof must run `cargo build --target wasm32-unknown-unknown -p lighting-core` with the snippet, measure wasm-bindgen/js-sys size and Worker CPU impact, and run native/wasm fixture parity with fixed inputs (reconstitute path) before this is called a passing feasibility result. Chrono-on-Wasm behaviour above is from crate source, not from a build — report actual toolchain constraints, not a premature commitment.

---

## 3. D1 vs Durable Object — decision inputs from the Switchboard side

Read-only inspection of `D:/Projects/switchboard` @ `12bd4b6` (branch `feat/swb-005-antigravity-plug`):

1. **D1 is the established canonical lane.** `wrangler.toml` declares `[[d1_databases]]` binding `DB` with `migrations_dir = "src/db/migrations"`, `nodejs_compat`, hourly cron. Schema/migration precedent: `src/db/migrations/0001_initial_schema.sql`; adapter interface precedent: `DatabaseSession` (`src/db/database.ts`) with `D1DatabaseLike` prepare/batch/exec (`src/db/d1-adapter.ts:1-18`).
2. **No Durable Objects exist in this checkout.** Grep for `DurableObject|durable_objects|SQLiteStorage` across `src/` and `tests/` returns zero hits, and `wrangler.toml` has no `durable_objects` block. OAuth grant/refresh state here is a *local `node:sqlite` file*: `SqliteClientStorage` (`switchboard-client.ts:138`), path `state-${endpointId}.sqlite` (`config.ts:34`). **Packet discrepancy:** the packet's "separately serialised SQLite Durable Object OAuth grant/refresh state" is not present at this revision — recheck the packet pin `401d2bc` (Switchboard PR #3) before citing DO precedent either way.
3. **Switchboard's D1 "transaction" is application-staged, not server-side.** `D1TransactionSession` stages statements and visibility buffers; `commit()` = `d1.batch(stagedStatements)` (`d1-adapter.ts:162-164`); `rollback()` = local discard (`:172`). There is no interactive `BEGIN`/`COMMIT`, and a conditional statement inside a batch returning zero changes does not abort the batch. Read-your-write is emulated per query shape (`:24-90`, three hard-coded shapes). The local lane, by contrast, uses real `SAVEPOINT` nesting (`sqlite-adapter.ts:64`) — Switchboard's native and D1 lanes are *not* semantically identical, and the emulation layer is the price paid.
4. **Implications for Lantern's packet invariant** ("a conditional UPDATE returning zero alone is not rollback"):
   - Do **not** inherit Switchboard's staged-batch pattern as sufficient for canonical receipt/correction writes. Lantern needs database-enforced compare-and-set/preconditions where the *complete logical write* is one atomic batch that either applies fully or is rejected, proven by the packet's injected mid-write failure test.
   - Precedent supports: D1 canonical store, versioned SQL migrations, typed adapter interface, migration discipline, primary reads.
   - If the atomicity proof cannot be met with bounded SQL, the fallback is the single-writer DO variant (packet §3) — evaluate then; do not add both as canonical stores.
   - Reuse patterns and tests only; never share DB, tokens, grants, or the production Worker (packet §2/§4 rule).
5. **FTS5:** no full-text index exists in Switchboard's schema — Lantern's FTS5 lane has no in-workshop precedent and needs its own qualification (consistent with foundation decisions deferring FTS smoke tests).

---

## 4. Candidate-cap reconciliation (500 vs 128) + measurement plan

### Inputs (all verified)

- Existing local remote MCP caps (`docs/mcp.md`): 500 candidate beliefs/memories, 50 results per search, 50 revisions/50 lineage claims for `lantern_why`, 256 KiB response ceiling, packs/traces not persisted on the remote route.
- Packet §5: candidate sets ≤128 **per authorised lane** plus a total cap; item budget ≤32 items / ~8,000 estimated tokens suggested maximum; 500,000 rows/day scanned budget; p95 ≤5 ms hosted target; "`LIMIT` alone does not bound scan work".
- Store reality: `limit == 0` = no `LIMIT` clause (§1); `compile_context` has three unbounded fetches (§1).

### Proposed hosted contract (explicit and versioned; native semantics untouched in M1)

**Bounded default when `limit` is 0/unspecified, plus mandatory cursor paging for bulk classes:**

- Hosted route receiving `limit: 0`/omitted → server default (start at **64 per lane**, candidate for reduction after measurement) and the versioned response reports `truncated` + `omitted_count`. Unbounded results are never representable on the hosted surface.
- Bulk classes (export, import, large history) use server-owned opaque cursors with stable ID keyset paging — no cursorless deep paging, no automatic full-corpus pulls (packet §4).
- Native/local `limit: 0` keeps its current meaning in M1; the hosted divergence is introduced as contract version 1 with the response metadata above (packet allows versioned hosted contracts; forbids *silent* semantic change).
- Lineage resolves through batched point lookups on selected claim IDs only (fixes the third fetch), so it does not paginate.

### Measurement plan (feeds M0 sizing and M2 acceptance)

1. **Golden corpus:** ≥50 intent/answer examples with known targets (M2 deliverable; defined now so cap selection is recall-driven).
2. **Cap sweep, native:** synthetic corpora at 1k/10k/50k records × caps {64, 128, 256, 500} × lanes {belief, memory}. Record: candidates generated, recall@10 on known-target intents, p50/p95 latency, items/tokens per pack. Choose the **smallest cap meeting recall@10 ≥90%** (packet goal; do not reclassify failures to pass).
3. **Row-scan budget arithmetic:** 500k scanned rows/day ÷ 400 representative reads ≈ **1.25k rows/read average**. Each candidate design must show its actual scanned rows (instrumented native counts; `EXPLAIN QUERY PLAN` + D1 `meta.rows_read` on the local wrangler lane) stay within this at 10k/50k corpora — returned rows ≠ scanned rows.
4. **Cap interaction check:** 128/lane × 2 lanes → ≤256 candidates → ≤32 selected items; serialization must remain under the 256 KiB response ceiling already proven in `mcp.md`. If 500-candidate responses currently approach it, that is evidence for the lower cap.
5. **Hosted verification (M4, synthetic only, within free-tier budget):** repeat the chosen points on local wrangler D1; reject any query where `LIMIT` is the only bound (scan cost still consumes allowance).
6. **Reconciliation decision recorded, not inherited:** publish the per-lane/total/response-byte cap table as hosted contract v1 and explicitly decide whether `docs/mcp.md`'s local 500s stay as-is or align — a deliberate, versioned choice for Lucy's review, never a silent local change.

---

## 5. Open items for commissioning

1. **Wasm build proof** (M0 time-box): apply §2 snippet, build `lighting-core` for `wasm32-unknown-unknown`, fixture parity with fixed inputs, measure size/CPU — report actual constraints.
2. **Switchboard DO discrepancy** (§3.2): recheck packet pin `401d2bc` / PR #3 for the DO OAuth state claim.
3. **`compile_select` extraction cut** (§1 Tier B): Muse to confirm the signature boundary before code moves.
4. **Authority coupling** (§1 Tier C): confirm hosted build excludes `authority_ops` rather than extracting Surreal-bound traits.
5. **Fetch-all fix scope:** three unbounded fetches in `compile_context`, not two — lineage point-lookup work belongs in the M1 storage-seam slice, not later.

*Read-only statements for this slice: no repository writes outside this note, no Switchboard modifications, no provisioning, no tests run.*
