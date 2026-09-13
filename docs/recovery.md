# Recovery

Lantern recovery is logical and local-first. Create an engine-independent
export before schema changes, migration, or cutover. The export contains a
manifest plus newline-delimited records for ledger/evidence, projects,
relations, ordinary memories, and epistemic records.

## Recovery rules

1. Stop the Lighting service before exporting embedded storage.
2. Confirm `.lighting-data/`, `.lighting-runtime/`, and `.private-migration/`
   remain ignored private paths.
3. Record the export manifest and hashes before changing schema or importing.
4. Restore into an isolated local store and compare counts, IDs, hashes, and
   representative retrieval results.
5. Keep the original export and migration snapshot until the final acceptance
   decision is recorded.

The feature line has a real logical export/restore parity drill, but service-
level retrieval-after-restore and a real Lucy restart proof remain acceptance
gates. Do not infer recovery success from a process exit alone.

The canonical master baseline remains a source-backed local proof. No public
push, tag, or release is implied by a successful local recovery run.
