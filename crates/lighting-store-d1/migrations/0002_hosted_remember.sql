-- LK-Q17 hosted capture: ordinary remember path (additive, no 0001 changes).
-- Applied by Work after 0001_hosted_schema.sql. Fresh/re-seeded DBs only;
-- no data migration (existing rows are synthetic smoke data).
--
-- Design: ordinary notes live in memory_item (scoped), NEVER in
-- epistemic_claim — a remembered note must not silently become a verified
-- claim. memory_item.source_id links the note to its byte-exact source row
-- so exact get stays scope-confined through a join (source itself carries
-- no scope column).

ALTER TABLE memory_item ADD COLUMN source_id TEXT REFERENCES source (id);

CREATE TABLE IF NOT EXISTS remember_idempotency (
    key           TEXT PRIMARY KEY,
    principal     TEXT NOT NULL,
    scope_hash    TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    payload_hash  TEXT NOT NULL,
    result_json   TEXT NOT NULL,
    created_at    TEXT NOT NULL
);

-- Same binding-not-global rule as corrections: enforced UNIQUE.
CREATE UNIQUE INDEX IF NOT EXISTS remember_binding ON remember_idempotency (principal, scope_hash, idempotency_key);

-- The deployed 0001 baseline reports hosted-1; this migration moves the
-- database to hosted-2 so lantern_status never claims an old schema.
INSERT OR REPLACE INTO schema_meta (key, value) VALUES ('schema_version', 'hosted-2');
