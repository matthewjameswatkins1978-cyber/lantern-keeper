# SurrealKV Qualification

## Decision

Lantern Keeper uses embedded, versioned SurrealKV as its normal local storage
path. This is an intentional beta adoption of the exact pinned
`surrealdb = 3.3.0-beta.3` line. The remote WebSocket backend remains available
for disposable integration tests and development diagnostics.

## Configuration

- Rust: 1.98.1 stable, Edition 2024
- SurrealDB Rust SDK: exactly 3.3.0-beta.3
- Embedded engine: `kv-surrealkv`
- Endpoint: `surrealkv://.lighting-data/surrealkv`
- Versioning: enabled with `versioned=true`
- Sync policy: `sync=every`, flushing each transaction commit
- Normal local namespace/database: `lantern_keeper` / `lighting_dev`

## Automated qualification

`crates/lighting-store-surreal/tests/surrealkv_qualification.rs` currently
proves:

- create and update;
- normal close and reopen durability;
- historical `VERSION` reads after a V1 → V2 update;
- concurrent independent writes from multiple Tokio tasks;
- ordinary indexed filtering;
- exact record-count preservation after reopen.

`crates/lighting-store-surreal/tests/ledger_repository.rs` additionally proves
that host-neutral ledger events retain raw payload, survive a clean reopen, and
return `Duplicate` for a replay with the same idempotency key.

Run it with:

```powershell
cargo test -p lighting-store-surreal --test surrealkv_qualification --locked -- --nocapture
```

## Escape hatch

SurrealKV is not the backup format. The \`lighting export\` command writes an
engine-independent export containing manifest metadata, Sources, Projects,
Episodes, relations, and Memory Ledger/Living Memory records. It does not
delete the old remote backend or any existing data directory.

On Windows, export is currently an offline operation: stop the process holding
the embedded store before opening the same path from the export command.

## Known concerns

The beta line has not yet been qualified for abrupt process termination,
vector/full-text index behaviour, or a larger synthetic dataset. Those are
explicit follow-up cases, not silently treated as passed. The export is
available, but restore tooling and a scheduled backup policy do not yet exist.

In a manual Ctrl-C run on Windows, SurrealKV logged cancelled background tasks
while shutting down. The committed data reopened correctly, but clean worker
shutdown is not yet treated as proven and should be revisited in the abrupt
termination lane.

The credentialed legacy WebSocket integration lane also showed transaction
conflicts when parallel API test processes selected separate disposable
databases. The embedded qualification's four-task concurrent-write workload
passed; the remote lane remains optional and needs a bounded retry/serialization
follow-up before it is treated as a release gate.

\`cargo deny check\` also reports transitive advisories in the pinned beta graph:
quick-xml 0.39.4 is constrained by SurrealDB's object_store 0.13.2
dependency, and rsa 0.9.10 has the known Marvin timing advisory with no
upstream patch. The attempted quick-xml 0.41.0 update is rejected by that
dependency constraint. This local-only beta deployment is not presented as a
public hostile-input service; the findings remain release blockers for any
broader exposure.
