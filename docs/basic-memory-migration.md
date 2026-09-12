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

## Import accounting checkpoint — 2026-09-12

The repaired 61-note snapshot is accounted for by the offline CLI command:

```text
lighting-cli basic-memory-accounting <snapshot-directory> --output <report.json>
```

The deterministic report records the exact source path, line number, raw value,
classification and reason for every imported observation/relation candidate.
The current snapshot result is:

- 496/496 observations accounted for; unexplained observations: 0;
- 275/275 relations accounted for; unexplained relations: 0;
- 156 Claim candidates, 76 soft-memory candidates, 13 historical-memory
  candidates, 66 metadata entries and 185 intentional Source-only entries;
- 171 resolved relations, 74 metadata-only relations, 21 historical relations,
  7 unresolved targets and 2 explicitly unsupported multi-target lines.

The report is private migration evidence under `.private-migration/` and is
ignored by Git. This checkpoint accounts for the source corpus; it does not yet
claim predicate promotion, belief reconciliation, replay into a clean database,
or cutover.

Never delete, bulk-edit, or rewrite Basic Memory as part of this work. Before
cutover, rerun the importer against the final delta, verify counts and hashes,
export Lantern, and produce an exception report. If any cutover gate fails,
Basic Memory remains canonical and Lantern remains shadow.
