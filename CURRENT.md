# Lantern Keeper - Current Phase

## Phase

Hackathon vertical slice: complete source-backed shared-memory loop.

## Current Task

Checkpoint 2: freeze the working proof and prepare a Git/GitHub checkpoint.

## Verified Workflow

Lantern Keeper now proves one complete loop through the real system:

```text
Capture project knowledge
-> store it as canonical Source-backed memory
-> retrieve Project context
-> format a Codex handoff
-> record the completed result as canonical memory
-> retrieve the updated Project handoff
```

## Implemented Now

- Exact Source storage and retrieval with fingerprint duplicate detection.
- Episodes as UTF-8-safe byte ranges into authoritative Sources.
- Projects and Markers stored through the memory-path repository.
- Episode-to-Project and Episode-to-Marker links through native SurrealDB relation tables.
- Marker-led retrieval with exact excerpts and deterministic provenance.
- Project-led retrieval with a deterministic Codex `context_package`.
- `lighting project-handoff <project-id>` for paste-ready Codex context.
- `lighting project-record-result <project-id> <result-file> --title "..."`
  for writing completed work back into the same Project.
- A full local proof script that demonstrates the loop using public CLI/API paths.

## Validation

Last verified locally:

```powershell
.\scripts\validate.ps1
.\scripts\run-first-proof-local.ps1
```

Both commands passed with live local SurrealDB.

## Task Log — LK-027 through LK-038

LK-027 through LK-038 implement the file-capture, revision-history, revision-safe retrieval, and project-file-linking features that complete the project-file memory loop.

### Commits

- `bb619b7` — Add project file linking (`project-add-file`)
- `b0f2bee` — Add Cline task guardrails (`.clinerules`)

### Test Validation

141 tests pass across the full workspace (25 CLI, 32 core, 26 service, plus integration tests for episodes, markers, projects, retrieval, sources, store-surreal, and memory-path repository).

### Workflow Delivered

The `lighting source-add`, `lighting source-history`, `lighting project-add-file`, `lighting project-handoff`, and `lighting project-record-result` CLI commands compose the complete project-file memory loop documented in `docs/lk-039-project-file-memory-loop.md`.

## Next Phase

Real-use / hackathon presentation evidence. Run the documented workflow end-to-end, capture the output, and produce concrete demonstration material that proves Lantern Keeper's working memory loop for the hackathon vertical slice.

## Next Verified Step

Make a checkpoint commit/PR that excludes local runtime state and unrelated
evidence folders, then write the hackathon description around this working loop.