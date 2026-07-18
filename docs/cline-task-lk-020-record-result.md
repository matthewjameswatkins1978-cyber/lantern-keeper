# Cline Task - LK-020 Record Result Through the Real System

You are implementing one bounded Lantern Keeper vertical-slice task.

Time limit: 15 minutes total. This task spans service, store, CLI, and focused tests, so 15 minutes is allowed.

Mandatory limits:

- At 7 minutes: stop adding capability and begin validation/reporting.
- At 15 minutes: stop completely.
- No more than two materially identical implementation attempts.
- Abort any command after 90 seconds without useful output.
- One full validation run only; one rerun only after a direct fix.
- Do not work on follow-on tasks after acceptance criteria are met.

## Context

Architecture rules:

- `lighting-core`: pure domain and repository traits.
- `lighting-store-surreal`: SurrealDB and graph implementation only.
- `lighting-service`: application operations, API DTOs, handlers, handoff rendering.
- `lighting-cli`: HTTP client and local file handling only.
- `apps/lighting`: composition and unified CLI parsing only.

Existing proven primitives:

- Source storage has fingerprint-based duplicate detection.
- Episodes are byte ranges into authoritative Sources.
- Episode-to-Project links use the native V3 SurrealDB relation table.
- Project retrieval produces the deterministic `context_package`.
- `lighting project-handoff <project-id>` already prints the server-produced context package.

Read this design first:

```text
docs/lk-019-writeback-design-gate.md
```

## Task

Implement:

```text
lighting project-record-result <project-id> <result-file> --title "..."
```

The command records the result file through the real system:

1. Store the file content as a canonical Source.
2. Create or reuse a full-range Episode for that Source within the target Project.
3. Link the Episode to the Project.
4. Ensure `lighting project-handoff <project-id>` includes the recorded result on the next retrieval.

## Required Shape

Add one endpoint:

```text
POST /api/v1/projects/{project_id}/record-result
```

Request:

```json
{
  "title": "...",
  "kind": "markdown",
  "content": "..."
}
```

Response:

```json
{
  "outcome": "recorded",
  "project_id": "<uuid>",
  "source_id": "<uuid>",
  "episode_id": "<uuid>",
  "start_byte": 0,
  "end_byte": 1234
}
```

Use `"already_recorded"` when a rerun reuses an existing Project-linked full-range Episode.

## Idempotency

Idempotency key:

```text
project_id + canonical source content fingerprint + full source byte range
```

Implementation requirement:

- Add the smallest `MemoryPathRepository` lookup needed to find a Project-linked Episode by `project_id`, `source_id`, `start_byte`, and `end_byte`.
- Implement it in `SurrealMemoryPathRepository` using the native V3 relation table and Episode fields.
- Do not use Project retrieval output or the retrieval hard limit to infer writeback idempotency.

## CLI File Handling

- Reject missing files.
- Reject directories.
- Reject invalid UTF-8 before HTTP.
- Infer `markdown` from `.md` or `.markdown`; otherwise use `plain_text`.
- Default title to file name if `--title` is omitted.
- Preserve content exactly as a Rust `String`: do not trim, rewrite line endings, append a newline, or normalise whitespace.

## Tests

Add focused tests proving:

- CLI command parses and rejects invalid Project UUID before HTTP.
- CLI kind/title inference matches `source add`.
- Service records exact content as a Source and a full-range Episode.
- Rerunning the same Project + same content returns the same Episode ID and does not duplicate Project handoff entries.
- Missing Project returns `project_not_found`.
- Invalid or unsafe inputs do not leak raw content or database errors.
- A Project handoff after record-result contains the recorded Source ID, Episode ID, exact byte range, exact excerpt, and deterministic provenance.

Use live SurrealDB integration tests where needed for repository/service behavior. Keep tests scoped.

## Do Not

- Do not create `Report`, `Task`, or any new broad domain model.
- Do not add AI APIs, embeddings, MCP, cloud, auth, UI, ranking, or importer work.
- Do not change unrelated files.
- Do not alter schema/migrations beyond the smallest index or query support genuinely required for this operation.
- Do not commit, push, reset, checkout, delete, or overwrite unrelated work.

## Validation

Run:

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

If all pass, report:

- Files changed.
- Tests run and result.
- Exact behavior of first run versus rerun.
- Any acceptance criteria not met.
