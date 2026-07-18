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

## Next Verified Step

Make a checkpoint commit/PR that excludes local runtime state and unrelated
evidence folders, then write the hackathon description around this working loop.
