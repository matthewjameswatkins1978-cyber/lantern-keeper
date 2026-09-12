# Ledger Event Model

Status: **SUPPORTING CURRENT — implementation partial**

Status: **implemented foundation; adapter and derived-memory projection remain partial**.

`LedgerEvent` is the host-neutral source boundary for ambient memory. It records
what an adapter received or observed. It is not an assertion and must not be
silently rewritten when a later Memory changes.

## Event shape

| Field | Meaning |
| --- | --- |
| `event_id` | Stable Lantern identity for the event. |
| `source` | Adapter/host that supplied it (`lucy`, `codex`, etc.). |
| `external_id` | Upstream identity when available. |
| `session_id`, `conversation_id`, `turn_id` | Optional host-neutral grouping keys. |
| `actor`, `role` | Who produced the content and their coarse role. |
| `content` | Exact normalized text presented to the ledger. |
| `observed_at` | Source-observed time, if supplied. It is not invented. |
| `received_at` | When Lantern accepted the event. |
| `reply_to` | Optional event identity for conversation threading. |
| `project_hint` | Non-authoritative routing hint for deterministic project resolution. |
| `idempotency_key` | Stable replay key. Duplicate ingestion returns the existing event. |
| `raw_payload` | Exact source payload when the adapter can preserve it. |
| `metadata` | Small adapter metadata map, serialized opaquely. |

## Ingestion contract

1. Validate event identity, source, actor, content, and idempotency key.
2. Look up the idempotency key before writing.
3. Store the event append-only in `ledger_event`.
4. Return `Stored` or `Duplicate`; never create a second evidence record for a
   replay.
5. Project events into Episodes, Assertions, or Living Memory only in a later
   adapter/projection step, retaining the event ID as provenance.

The current durable implementation is `SurrealLedgerRepository`. Its unique
idempotency index is a storage guard, while the preflight lookup gives callers a
useful duplicate result. A future concurrent adapter should additionally test
and handle a unique-index race as a duplicate.

## Ordering and restart

Events may arrive out of order. `received_at` is operational ordering only;
`observed_at` and the session/turn/reply fields carry semantic context. Listing
is deterministic by `received_at`, then `event_id`. Replay after a process restart
is covered by the integration test and preserves the raw payload.

## Deliberate boundary

The ledger is evidence. Derived Memory owns claims, confidence, validity,
supersession, contradiction, and derivation relationships. No gardener or
importer may rewrite the original ledger event to make a later interpretation
look like the original source.
