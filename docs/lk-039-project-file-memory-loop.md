# LK-039 — Project-File Memory Loop

This page documents Lantern Keeper's working workflow for capturing a file, attaching it to a Project, revising and re-capturing it, inspecting revision history, and producing a Project handoff.

Status: implemented proof workflow. The joint canonical architecture in
[`architecture/TETHERS_LANTERN_KEEPER_CANONICAL_ARCHITECTURE.md`](architecture/TETHERS_LANTERN_KEEPER_CANONICAL_ARCHITECTURE.md)
keeps this Source/Episode/Project proof as foundation work, but the forward
durable memory model is Project, Source, Episode, Memory and Link. Existing
Markers are a proof-era retrieval aid; they are not a separate required durable
concept for the next memory-foundation milestone.

## Prerequisites

SurrealDB 3.2.1+ running on `ws://127.0.0.1:8000`, plus Lighting serving on `http://127.0.0.1:4317`:

```powershell
.\scripts\start-surreal.ps1
cargo run -p lighting -- serve
```

Verify readiness:

```powershell
cargo run -p lighting -- health
```

## Walkthrough

### 1. Create or choose a Project

```powershell
cargo run -p lighting -- project-create "My Demo Project"
```

Output shows the Project ID, name, and status. Human output:

```
Project ID: <uuid>
Name      : My Demo Project
Status    : active
```

To list Projects and find their IDs at any time:

```powershell
cargo run -p lighting -- project-list
```

Output is one compact row per Project: `[<uuid>] <status> — <name>`. An empty list prints `(no Projects)`. Capture the ID for later steps:

```powershell
$projectId = "<uuid-from-output>"
```

The rest of the walkthrough uses `$projectId` wherever `<project-id>` appears.

### 2. Capture a file as a Source

```powershell
cargo run -p lighting -- source-add my-notes.md
```

Output shows the Source ID, title (defaults to the normalised absolute path), and capture outcome (`first capture` or `new revision`). Duplicate content returns `unchanged` with the existing Source ID.

### 3. Attach the Source to the Project

```powershell
cargo run -p lighting -- project-add-file $projectId my-notes.md
```

This captures the file content as a Source (with fingerprint dedup), creates or reuses a full-range Episode, and links it to the Project as `primary`. All three steps are idempotent via logical Source identity `(kind, title)`.

### 4. Revise and re-capture the same file

Edit `my-notes.md`, then re-run exactly the same command:

```powershell
cargo run -p lighting -- project-add-file $projectId my-notes.md
```

The output reports `new revision` with a `previous_source_id` pointing to the prior capture. The existing Project-Episode link is reused — no duplicate linkage is created. The Project handoff context will now reference the latest revision.

### 5. Inspect revision history

```powershell
cargo run -p lighting -- source-history my-notes.md
```

Lists every revision in chronological order, oldest to newest, with a `(current)` marker on the latest revision. Each entry shows the Source ID, optional previous Source ID, and capture timestamp.

### 6. Produce a Project handoff

```powershell
cargo run -p lighting -- project-handoff $projectId
```

Outputs the deterministic Codex-oriented context package containing all linked Episodes with exact byte ranges and authoritative excerpts. The handoff respects provenance: historical Sources are retained, and the current Source content is used only when the excerpt can be safely rebased to the latest revision.

### 7. Record a result back into the Project

```powershell
cargo run -p lighting -- project-record-result $projectId result.md --title "Completed Work"
```

Captures the result file as a Source, creates or reuses a full-range Episode, and links it to the Project. Re-running with identical content is idempotent (`already_recorded`). The updated Project handoff now includes the recorded result.

## Run the Demo

A repeatable end-to-end demo script exercises the full workflow:

```powershell
.\scripts\demo-lk049-project-memory.ps1
```

The script creates a uniquely-named Project, captures a Markdown file, revises and re-captures it, inspects the revision history through `source-history`, produces a deterministic Project handoff, records a result back via `project-record-result`, and verifies the result appears in a final updated handoff. Every command uses `--json` to prove the machine-readable contract. Temporary files are created outside the repository and cleaned up automatically.

## Project Show

Inspect a Project and its linked Episodes anytime through the read path:

```powershell
cargo run -p lighting -- project-show $projectId
```

This prints the Project's ID, name, status, and a numbered list of linked Episodes with their Source identity, byte range, and link kind. Use `--json` for the full machine-readable view, which includes all Episode details plus an optional `latest_source_id` per Episode:

- **`source_id`** — the historical Source ID that was current when the Episode was linked to the Project. It is **never** rewritten to a newer revision; you can count on it being stable.
- **`latest_source_id`** — present only when a newer revision of the same logical Source exists. The historical `source_id` remains intact, and the presence of `latest_source_id` tells you there is an update to inspect.

An empty Project returns `"episodes": []`. An unknown Project ID returns the documented `project_not_found` 404.

## Provenance Behaviour

- Each file capture creates a Source record with a content fingerprint. Identical content returns the existing Source ID (no duplicate storage).
- When a file is revised and re-captured, a new Source is stored with a `previous_source_id` chain linking back to the prior revision. Both revisions remain retrievable.
- A Project keeps one visible Episode per logical file `(kind, title)`. The Project handoff always shows the Episode pointing at the historical Source ID that was current when the Episode was linked, with a `latest_source_id` field flagging the most recent revision.
- When retrieval can uniquely and safely rebase the excerpt to the current revision, it does so; when it cannot (multi-branch or ambiguous), it keeps the historical bytes and flags the existence of a newer revision.
- `project-record-result` creates new Episodes for each distinct result — it does not overwrite the original file Episode. Results and source files are separate entries in the handoff.

## What This Proves

This workflow demonstrates the full working memory loop that Lantern Keeper delivers for the hackathon vertical slice: a local file is captured as canonical Source-backed memory, linked to a durable Project, revised and re-captured with full revision history, retrieved with deterministic provenance through the Project handoff, and then a result is written back into the same Project — all through public CLI commands that compose the existing Source, Episode, Project, and retrieval primitives. No AI, embeddings, MCP, cloud, or GUI is required.
