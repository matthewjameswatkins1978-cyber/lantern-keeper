# Ambient Memory Foundation Qualification

Date: 2026-09-04  
Branch: `foundation/lantern-pre-memory`

## Evidence

- Preservation checkpoint: `lantern-pre-memory-checkpoint` at
  `407c52934de8fe4c583c7ed549e51d1de45c7ce3`.
- Working-tree foundation head before this packet: `1ee1e8b76148dcd49c5a4694d414aa27e5e5395f`.
- Current packet commits are intentionally incremental; no force-push or reset
  was used.
- Rust `1.98.1`, edition 2024, exact SurrealDB `3.3.0-beta.3`.
- Normal local storage is embedded versioned SurrealKV with `sync=every`.
- Portable export remains logical JSON/NDJSON and includes ledger events.

## Qualification status

| Capability | Status | Evidence / boundary |
| --- | --- | --- |
| Memory Ledger | **IMPLEMENTED** | Existing source/episode path plus append-only `ledger_event`. |
| Assertions | **IMPLEMENTED** | `memory` records with confidence, status, and temporal validity. |
| Provenance | **IMPLEMENTED** | Source references, explicit evolution fields, bounded lineage. |
| Temporal state | **IMPLEMENTED** | Optional observation time, valid intervals, as-of query test. |
| Living Memory | **IMPLEMENTED** | Small practical MemoryKind vocabulary. |
| Retrieval | **PARTIALLY IMPLEMENTED** | Lexical/project/currentness/as-of baseline; no BM25/vector fusion. |
| Context builder | **IMPLEMENTED (first slice)** | Compact deterministic sectioned context packet. |
| Conflicts / abstention | **PARTIALLY IMPLEMENTED** | Empty recall abstains; contradiction storage exists, automatic conflict analysis does not. |
| Activation | **DEFERRED** | No usefulness counters or ranking reinforcement yet. |
| Retrieval traces | **PARTIALLY IMPLEMENTED** | Returned trace exists; durable trace records are deferred. |
| Gardener | **DEFERRED** | No autonomous derived-memory maintenance yet. |
| LanternBench | **IMPLEMENTED (initial)** | PowerShell runner with global and per-category metrics. |
| MCP | **DEFERRED** | HTTP/CLI semantics come first; no thin MCP wrapper yet. |
| Host importer | **PARTIALLY IMPLEMENTED** | Generic JSON event importer and representative fixture; no native Codex export adapter or watermark cursor. |

## Tests

Passing gates observed during this packet:

```text
cargo check --workspace --all-targets --locked                         PASS
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings  PASS (offline)
cmd /c "set LIGHTING_SKIP_INTEGRATION_TESTS=1&& cargo test --workspace --locked --offline"  PASS: 197 passed, 0 failed, 1 ignored
cargo test -p lighting-store-surreal --test ledger_repository --locked      PASS
cargo test -p lighting-store-surreal --test memory_repository --locked      PASS: 2 passed
```

The normal remote integration lane remains separately qualified. Earlier
credentialed remote runs exposed parallel transaction conflicts in legacy
episode tests; that is documented in `SURREALKV_QUALIFICATION.md` and remains a
known concern rather than being hidden by the embedded default.

`cargo fmt --all -- --check` still reports pre-existing CRLF/style debt in the
legacy tree. New code was formatted/compiled without wholesale reformatting.

## What is proven

The embedded tests prove:

1. Memory V1/V2 semantic validity can be queried historically without deleting
   the old claim.
2. Multi-hop memory lineage follows memory-backed provenance/evolution links.
3. Ledger events preserve raw payload, survive close/reopen, and replay as
   `Duplicate` rather than creating a second event.
4. Export includes the new ledger table in the portable ledger stream.

## What is not yet proven

Abrupt child-process termination, restore into a new empty store, durable
retrieval traces, concurrent idempotency races, BM25/vector search, semantic
proposal quality, and native host lifecycle wiring remain open qualification
work. The current branch is therefore **PARTIAL**, not a claim that the full
ambient-memory mission is complete.

## Recommended next job

Implement a restart-safe importer cursor plus export validation/restore into a
new empty embedded store, then run it against a real Matthew↔Lucy transcript
export before adding autonomous proposals.
