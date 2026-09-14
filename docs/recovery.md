# Recovery

Lantern recovery is logical and local-first. Create an engine-independent export before schema changes, migration, or cutover. The export contains a manifest plus newline-delimited records for ledger/evidence, projects, relations, ordinary memories, and epistemic records.

## Recovery rules

1. Stop the Lighting service before exporting embedded storage.
2. Confirm `.lighting-data/`, `.lighting-runtime/`, and `.private-migration/` remain ignored private paths.
3. Record the export manifest and hashes before changing schema or importing.
4. Restore into an isolated local store and compare counts, IDs, hashes, and representative retrieval results.
5. Keep the original export and migration snapshot until the final acceptance decision is recorded.

Canonical `master` has passed a real 1,479-record logical export/restore parity drill. Representative service-level retrieval after a fresh restore remains an acceptance gate. Codex Desktop restart persistence is separately proven for the live MCP path.

Basic Memory remains the rollback/history archive until cutover is explicitly accepted. No tag or public release is implied by successful recovery tests.
