# lantern-git

`lantern-git` is an optional, read-only transport mirror for clients whose native Lantern MCP tools are unavailable. Lantern remains canonical. The mirror is disposable, timestamped, and never accepts writes back into Lantern.

## Layout

The exporter reads the existing local Lighting HTTP read endpoints and creates a complete staged generation under `mirror/`:

- `manifest.json` records export format/version, UTC generation time, source Lantern software revision, optional operator-supplied source-store identity, per-type counts, per-record SHA-256 and Git blob hashes, the UUID-to-path rule, and exclusions.
- `index/active.jsonl` and `index/archived.jsonl` are compact catalogs for connector search.
- `records/{memory-item|claim|belief}/{active|archived}/{uuid-prefix}/{uuid-prefix}/{uuid}.md` contains one deterministic, searchable Markdown record with its allowlisted JSON representation.

Soft memories, claims and beliefs remain distinct record types. UUIDs and source, episode, originator, transmitter, holder and lineage identifiers are preserved when the canonical endpoint supplies them. The exporter does not fetch or export raw Source/Episode content, authority grants, credentials, local database files or unknown response fields. Credential-shaped values and oversized records fail the generation closed. Secret scanning is a guardrail, not a guarantee that arbitrary text contains no sensitive information; use a private repository and review the diff before a first export.

The first design intentionally omits generated project context packs: the current project-retrieval endpoint includes excerpts from raw Sources, which the mirror is explicitly designed to exclude. A future pack needs a curated Lantern endpoint that preserves semantic links while filtering source blobs.

## Export and publish

Use a local clone of the private repository `matthewjameswatkins1978-cyber/lantern-git` as `--output`. The default service address is loopback-only (`127.0.0.1:4317`). No credentials are embedded or read by the exporter.

```powershell
python scripts/lantern_git_export.py --output D:\Projects\lantern-git --require-marker pi-crossclient-20261001-094804 --require-uuid ed7e2de6-2d4d-464a-903d-f41df1b990f3
git -C D:\Projects\lantern-git diff -- mirror
git -C D:\Projects\lantern-git add mirror
git -C D:\Projects\lantern-git commit -m "Refresh Lantern read-only mirror"
git -C D:\Projects\lantern-git push origin main
```

Review the staged diff before pushing, especially on the first export. Export failure leaves the previous mirror intact. A push failure cannot affect Lantern writes. Schedule the same reviewed command through a user-managed task only if recurring publication is wanted; automation is deliberately not installed or enabled by this feature.

The manifest omits machine paths and service URLs. `LANTERN_GIT_SOURCE_ID` may be set to a non-secret stable label to distinguish source stores; without it the manifest states that identity is unknown. The software revision is the Git commit of the exporter checkout, not a database revision.

## ChatGPT fallback

The Lantern Keeper V2 skill uses native read tools first. If they are missing, it fetches `mirror/manifest.json` from the private GitHub connector and searches only `matthewjameswatkins1978-cyber/lantern-git`. GitHub connector search uses the repository default branch and may lag while a new repository is indexed; fetch-by-path can read a known file/ref directly. The manifest timestamp is the snapshot time, not a claim of live freshness. Treat exports older than 24 hours as stale for current-state answers. Every fallback answer must identify GitHub as transport and retain record IDs and provenance.

GitHub is not a second memory authority. A mirror record is evidence copied from Lantern at a stated time; a missing or stale record is not proof of absence in Lantern. The Git blob hash returned by connector fetch can be compared with the manifest's `git_blob_sha`; the exporter separately checks SHA-256 before replacing its local generation.
