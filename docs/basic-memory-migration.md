# Basic Memory Migration

Basic Memory is preserved as the migration source and rollback/history archive.
It is not a permanent Lantern runtime dependency. The private snapshot remains
outside Git under `.private-migration/` and must never be committed, bulk-edited,
or deleted as part of migration work.

The importer preserves each raw Markdown note as a Lantern Source and records
replay-safe source-ledger metadata. Observation and wiki-relation candidates are
accounted for without treating them as canonical truth. Unknown categories and
unresolved targets remain explicit rather than being guessed into Claims.

## Verified accounting checkpoint

The repaired snapshot used by the feature line accounts for:

- 61 notes;
- 496 observations, all accounted for;
- 275 relations, all accounted for;
- Claim, soft-memory, historical-memory, metadata, and intentional Source-only
  classifications with unresolved and unsupported cases reported explicitly.

This is source-corpus accounting. It does not claim predicate promotion,
belief reconciliation, replay into a clean database, shadow equivalence, or
cutover.

## Cutover boundary

Before cutover, rerun the importer against the final Basic Memory delta, verify
counts and hashes, export Lantern, and produce an exception report. If any gate
fails, Basic Memory remains canonical and Lantern remains shadow. A successful
import or local build is not permission to delete the source archive.
