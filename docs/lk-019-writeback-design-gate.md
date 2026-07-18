# LK-019 - Writeback Design Gate

## Decision

Implement result writeback as one narrow service operation plus one CLI command:

```text
lighting project-record-result <project-id> <result-file> --title "..."
```

The CLI reads one local Markdown or text file and sends the exact content to the service. The service records that content as a canonical Source, creates or reuses a full-range Episode for that Source within the target Project, links the Episode to the Project, and returns the recorded IDs.

This is permanent application behavior, not a demo shortcut. It composes existing Source, Episode, Project, Project-link, and Project-retrieval primitives. It does not introduce a Report, Task, AI summary, embedding, MCP, cloud, UI, or new graph concept.

## Idempotency Rule

Idempotency is scoped to:

```text
project_id + canonical source content fingerprint + full source byte range
```

Normal rerun behavior:

1. The same file content stores as the same existing Source through the existing Source fingerprint duplicate rule.
2. The service checks whether the target Project already has a linked Episode whose Source ID is that canonical Source ID and whose byte range is exactly `0..source_content.len()`.
3. If such an Episode exists, the operation returns that existing Episode and does not create another Episode.
4. If no such linked Episode exists, the service creates a new full-range Episode and links it to the Project as `primary`.

Changing only `--title` on a rerun with identical content and Project does not create a new Episode. Source content and Project membership define the durable memory fact; title is metadata for the first recording. If a different title is genuinely needed, that is a later edit/update capability, not part of this sprint.

## Service Shape

Add one endpoint:

```text
POST /api/v1/projects/{project_id}/record-result
```

Request:

```json
{
  "title": "Implemented project handoff CLI",
  "kind": "markdown",
  "content": "# Result\n\n..."
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

Use `"already_recorded"` for the idempotent rerun outcome.

The operation belongs in `lighting-service` because it must atomically coordinate existing repositories enough to avoid duplicate Project-linked Episodes. The CLI should not reconstruct idempotency from handoff output or direct database state.

## Repository Support

Add the smallest repository capability needed to avoid duplicate Episodes:

```rust
find_project_episode_by_source_range(project_id, source_id, start_byte, end_byte)
```

This belongs on `MemoryPathRepository` because it is graph/storage lookup behavior. It does not change the core domain model. The SurrealDB implementation should query the native V3 relation table and the linked Episode fields.

The existing `list_project_episode_links` hard limit is retrieval behavior and should not be reused as the writeback idempotency mechanism.

## File Handling

The CLI reads the file with Rust text APIs that preserve UTF-8 content, whitespace, and line endings in the resulting `String`.

Rules:

- Reject missing paths.
- Reject directories.
- Reject invalid UTF-8 with a clear CLI error before making an HTTP request.
- Infer `markdown` from `.md` or `.markdown`; otherwise use `plain_text`, matching `source add`.
- Default title to the file name when `--title` is omitted.
- Do not trim, normalise, append a newline, or rewrite file content.

## Project Errors

Invalid Project UUIDs are rejected locally by the CLI before HTTP.

Missing Projects are rejected by the service with the existing safe API error shape:

```json
{
  "code": "project_not_found",
  "message": "No Project exists with the given ID"
}
```

No raw database errors, file content, or partial handoff content should appear in error responses or CLI error messages.

## Demonstration Consequence

After a successful record-result operation, this existing command must show the result in the Project handoff:

```text
lighting project-handoff <project-id>
```

The handoff must include the recorded result's Source ID, Episode ID, exact byte range, exact excerpt, and deterministic provenance through the normal Project retrieval path.
