# Basic Memory Migration

Basic Memory is preserved as the migration source and rollback/history archive.
The private snapshot is kept outside Git under `.private-migration/` and is
validated by a manifest and export hash.

The current importer preserves each raw Markdown note as a Lantern Source and
records replay-safe source-ledger metadata. The importer also deterministically
extracts observation and wiki-relation candidates without treating them as
canonical truth. The canonical Claim/Memory Item import path is exposed by the
new local APIs and is the next migration exercise; it must retain unknown
categories and unresolved targets rather than guessing.

Never delete, bulk-edit, or rewrite Basic Memory as part of this work. Before
cutover, rerun the importer against the final delta, verify counts and hashes,
export Lantern, and produce an exception report. If any cutover gate fails,
Basic Memory remains canonical and Lantern remains shadow.
