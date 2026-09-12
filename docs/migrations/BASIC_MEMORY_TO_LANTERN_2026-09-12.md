# Basic Memory → Lantern migration checkpoint

Date: 12 September 2026  
Source project: Basic Memory Cloud `Lantern`  
Source project ID: `ff4f6328-dd77-4b0b-8e5a-485e73554d97`

## Snapshot inventory

The live project was enumerated through the Basic Memory Cloud connector. The
snapshot contains 61 Markdown records in 14 directories, 210,186 UTF-8 bytes,
455 recognised observations and 209 typed wiki-link relations. Seven relation
targets do not resolve to another snapshot record. The current manifest
SHA-256 is:

```text
f723a5ee0309679d4ff9198d8ec567b0dbe0f8431e9cc897a7c78b347fe1def2
```

The snapshot preserves each record's path, title, note type, permalink,
external ID, update timestamp and complete Markdown content. The private
snapshot is deliberately outside the public Git repository.

## Import behaviour

`lighting import-basic-memory <snapshot> --verify --json` validates the
project identity, rejects blank records, calculates per-record SHA-256 hashes,
and produces the deterministic manifest. Without `--verify` or `--dry-run`,
the importer submits every record to the Lantern Source API and then submits a
linked canonical-memory candidate. The importer also promotes recognised
`- [category] proposition` observations and creates typed relation records in
a second pass. Unresolved relation targets retain their original target text
and provenance. Source fingerprints and stable memory identities make
repeated runs safe; the original note content remains the evidence.

The importer does not infer unsupported semantics from arbitrary prose. It
preserves unknown observation categories, treats the note itself as a durable
source-backed memory, and records parsed observations as derived candidates.
Explicit `historical`, `archive`, `archived`, `superseded` and
`historical-*` frontmatter statuses are promoted outside the current view.
Unknown statuses remain reviewable candidates. No source, observation or
relation is silently discarded.

## Current cutover state

`Basic Memory = untouched, live reference and canonical external memory for
the moment.` Lantern is the candidate replacement implementation, not yet the
authoritative external memory service, because this recovery environment has
no mounted Windows checkout, no existing Lantern database, and no running
SurrealDB server.

## Required next gates

1. Run the snapshot importer against an isolated SurrealDB backup/restore
   environment.
2. Verify source, relation and observation counts and rerun the importer to
   prove idempotency.
3. Run representative Basic Memory versus Lantern retrieval parity tests.
4. Complete MCP, Mastra, Tethers, audit and recovery tests.
5. Perform a final Basic Memory delta snapshot before any read-only fallback
   cutover.

## Rollback

Keep the Basic Memory `Lantern` project intact. To roll back a Lantern test
import, restore the pre-import SurrealDB backup or discard the isolated test
namespace; do not delete or rewrite the Basic Memory project.
