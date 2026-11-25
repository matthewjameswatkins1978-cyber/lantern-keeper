-- Lantern Keeper hosted schema — D1 / SQLite dialect.
-- Migration semantics follow the checked-in Surreal migrations: idempotent
-- DDL (IF NOT EXISTS), a schema bootstrap row recording the version, and
-- additive-only evolution. Work applies this on deployment; nothing here
-- provisions a database.
--
-- Hosted scope excludes authority grants/receipts (LK-HOST-001 v1.1 §4:
-- knowledge must not create permission to execute; the hosted memory
-- surface carries no authority state).
--
-- Version: hosted-1 (maps to the native schema lineage v9..v11 concepts:
-- scope column and index from V10, correction idempotency from V11).
--
-- Timestamps: every TEXT timestamp in this schema MUST be canonical
-- RFC3339 UTC with nanosecond precision and trailing Z
-- (exactly `YYYY-MM-DDTHH:MM:SS.fffffffffZ`, 30 chars). `ORDER BY
-- updated_at` and the keyset cursor are string comparisons: mixed
-- precision or offsets silently break chronological ordering. The batch
-- builder rejects non-canonical `accepted_at`; writers must produce
-- canonical values (service layer, C2).
--
-- Foreign keys are declared as the intended referential contract. The
-- enforced guards are the UNIQUE constraints and the CAS trigger below;
-- FK enforcement additionally requires the connection pragma where the
-- executor supports it.

CREATE TABLE IF NOT EXISTS schema_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

INSERT OR REPLACE INTO schema_meta (key, value) VALUES ('schema_version', 'hosted-1');

-- ---------------------------------------------------------------------------
-- Evidence chain: Source -> Episode -> Claim -> Belief -> (Context Pack).
-- Canonical payloads stay whole-JSON (parity with native) plus indexed
-- projections for the scoped/typed lookup path.
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS source (
    id           TEXT PRIMARY KEY,
    kind         TEXT NOT NULL,
    title        TEXT NOT NULL,
    content      TEXT NOT NULL,
    fingerprint  TEXT NOT NULL,
    created_at   TEXT NOT NULL
);
-- Native dedupes Sources by fingerprint: the hosted index is UNIQUE to
-- preserve that contract.
CREATE UNIQUE INDEX IF NOT EXISTS source_fingerprint ON source (fingerprint);

CREATE TABLE IF NOT EXISTS episode (
    id          TEXT PRIMARY KEY,
    project_id  TEXT,
    title       TEXT NOT NULL,
    source_id   TEXT NOT NULL REFERENCES source (id),
    start_byte  INTEGER NOT NULL,
    end_byte    INTEGER NOT NULL,
    created_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS episode_source ON episode (source_id, start_byte, end_byte);

CREATE TABLE IF NOT EXISTS epistemic_claim (
    id                 TEXT PRIMARY KEY,
    scope_hash         TEXT NOT NULL,
    subject_key        TEXT NOT NULL,
    predicate_key      TEXT,
    predicate_candidate TEXT,
    predicate_status   TEXT NOT NULL,
    value              TEXT NOT NULL,
    polarity           INTEGER NOT NULL,
    speaker_actor_id   TEXT NOT NULL,
    holder_actor_id    TEXT,
    dedupe_key         TEXT NOT NULL,
    episode_id         TEXT REFERENCES episode (id),
    source_id          TEXT REFERENCES source (id),
    payload            TEXT NOT NULL,
    known_at           TEXT NOT NULL,
    created_at         TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS claim_scope ON epistemic_claim (scope_hash);
-- Native carries UNIQUE(dedupe_key): the hosted index is UNIQUE to
-- preserve claim-level replay protection.
CREATE UNIQUE INDEX IF NOT EXISTS claim_dedupe ON epistemic_claim (dedupe_key);
CREATE INDEX IF NOT EXISTS claim_subject_predicate ON epistemic_claim (subject_key, predicate_key);

-- ---------------------------------------------------------------------------
-- Governed projection. `scope` is a materialised column + index so scoped
-- reads bound scan work (v1.1: authorisation before candidate selection,
-- scope constrains scans not just results).
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS belief (
    id                  TEXT PRIMARY KEY,
    scope_hash          TEXT NOT NULL,
    scope               TEXT NOT NULL,
    holder_key          TEXT NOT NULL,
    subject_key         TEXT NOT NULL,
    predicate_key       TEXT NOT NULL,
    current_value       TEXT NOT NULL,
    state               TEXT NOT NULL,
    stale               INTEGER NOT NULL DEFAULT 0,
    stale_since         TEXT,
    stale_reason        TEXT,
    dependency_generation INTEGER NOT NULL DEFAULT 0,
    updated_at          TEXT NOT NULL,
    payload             TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS belief_scope ON belief (scope_hash);
CREATE INDEX IF NOT EXISTS belief_scope_state ON belief (scope_hash, state, stale);
CREATE INDEX IF NOT EXISTS belief_subject_predicate ON belief (subject_key, predicate_key);

-- The revision record is half the CAS guard: UNIQUE(belief_id,
-- from_updated_at) aborts *reuse* of an already-consumed token. It cannot
-- detect a token the database never issued, so the trigger below closes
-- that hole. A conditional UPDATE returning zero rows is deliberately NOT
-- the guard: it cannot abort anything.
CREATE TABLE IF NOT EXISTS belief_revision (
    id              TEXT PRIMARY KEY,
    belief_id       TEXT NOT NULL REFERENCES belief (id),
    from_updated_at TEXT NOT NULL,
    payload         TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    UNIQUE (belief_id, from_updated_at)
);
CREATE INDEX IF NOT EXISTS belief_revision_belief ON belief_revision (belief_id, created_at);

-- Expected-revision CAS, second half: a forged `expected_revision` would
-- otherwise be accepted and move the projection (demonstrated pre-trigger:
-- belief moved rev-1 -> rev-9 on a bogus token). This trigger aborts any
-- projection advance unless a revision row already records a transition
-- *out of* the current token. The legitimate batch inserts exactly that
-- row first, so it passes; a forged token finds no such row and the whole
-- batch aborts with `revision_conflict`.
CREATE TRIGGER IF NOT EXISTS belief_cas_expected
BEFORE UPDATE ON belief FOR EACH ROW
WHEN NEW.updated_at <> OLD.updated_at
 AND NOT EXISTS (SELECT 1 FROM belief_revision
                 WHERE belief_id = NEW.id AND from_updated_at = OLD.updated_at)
BEGIN
    SELECT RAISE(ABORT, 'revision_conflict');
END;

-- ---------------------------------------------------------------------------
-- Audit trace. Hosted READ tools are zero-write (v1.1 §5); traces are only
-- written by accepted mutations.
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS trace (
    id         TEXT PRIMARY KEY,
    payload    TEXT NOT NULL,
    created_at TEXT NOT NULL
);

-- NOTE: native `MemoryItem` carries no scope; the hosted `scope_hash`
-- column is a deliberate semantic divergence (per-scope budgeting and
-- isolation on the hosted path), not parity. Do not treat hosted and
-- native memory items as interchangeable rows.
CREATE TABLE IF NOT EXISTS memory_item (
    id           TEXT PRIMARY KEY,
    scope_hash   TEXT NOT NULL,
    content      TEXT NOT NULL,
    kind         TEXT NOT NULL,
    salience     REAL NOT NULL,
    archived     INTEGER NOT NULL DEFAULT 0,
    payload      TEXT NOT NULL,
    created_at   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS memory_item_scope ON memory_item (scope_hash, archived);

-- ---------------------------------------------------------------------------
-- Idempotency receipts. Key binds principal + scope + caller key
-- (per the LK-HOST-001 decision: binding, not global contention — a
-- cross-principal conflict would itself be an existence oracle).
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS correction_idempotency (
    key           TEXT PRIMARY KEY,
    principal     TEXT NOT NULL,
    scope_hash    TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    payload_hash  TEXT NOT NULL,
    result_json   TEXT NOT NULL,
    created_at    TEXT NOT NULL
);
-- The composite binding is enforced UNIQUE (a cross-principal conflict
-- would be an existence oracle, so binding-not-global is load-bearing).
CREATE UNIQUE INDEX IF NOT EXISTS idempotency_binding ON correction_idempotency (principal, scope_hash, idempotency_key);

-- ---------------------------------------------------------------------------
-- Mutation receipts: one per accepted keyed write, recorded INSIDE the
-- atomic boundary (docs/hosted-m1-d1-atomicity.md). `staged` counts the
-- governed domain writes in this batch (the projection advance = 1);
-- receipts are bookkeeping, not staged. The full-envelope builder (future)
-- passes its own count under the same meaning.
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS mutation_receipt (
    receipt_id   TEXT PRIMARY KEY,
    envelope_version TEXT NOT NULL,
    principal    TEXT NOT NULL,
    scope_hash   TEXT NOT NULL,
    payload_hash TEXT NOT NULL,
    staged       INTEGER NOT NULL,
    accepted_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS mutation_receipt_binding ON mutation_receipt (principal, scope_hash);
