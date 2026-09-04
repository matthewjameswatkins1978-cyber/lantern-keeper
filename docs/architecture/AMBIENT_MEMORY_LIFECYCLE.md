# Ambient Memory Lifecycle

The current lifecycle is intentionally conservative:

```text
host turn
  -> LedgerEvent (append-only evidence, replay-safe)
  -> adapter/projection (future: Episode / observation / proposal)
  -> Memory (derived claim with provenance and temporal validity)
  -> recall/context (inspectable selection)
  -> correction/supersession (new claim, old history retained)
```

## Host-neutral lifecycle contract

Adapters should be able to expose these hooks without making Lantern depend on a
particular host:

| Hook | Contract |
| --- | --- |
| `session.open` | Supply stable host/session identity and project hint. No memory mutation is implied. |
| `turn.before` | Request bounded context for the next turn. A missing or conflicting result is valid. |
| `turn.after` | Append the observed user/assistant/tool events. Derived-memory proposals are separate. |
| `session.close` | Flush pending events and progress markers; do not silently discard an uncommitted event. |

The first implemented client surface is the HTTP ledger endpoint and the
`lighting ledger-ingest` CLI command. Codex-specific ambient hooks and an
event-driven worker are deferred until the event/projection boundary has more
qualification.

## Evidence versus understanding

`ledger_event` is source evidence. `memory` is Living Memory. A proposal engine
may suggest a Memory, but only a deliberate projection/acceptance operation may
write it. Corrections create new understanding and preserve the old event and
claim for historical inspection.

## Replay and progress

The current ingestion primitive is idempotent by `idempotency_key` and can be
replayed after restart. Watermarks, out-of-order progress reporting, and a
durable importer cursor are not yet implemented; an importer must therefore
retain its input and rerun safely rather than claiming checkpointed progress.
