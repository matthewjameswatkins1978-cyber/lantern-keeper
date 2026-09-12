# Lantern Keeper — Worker Guide

Lantern Keeper is a local-first shared memory service for Matthew and Lucy.
Sources preserve what happened, Claims preserve what was asserted, Beliefs are
derived current understanding, and Memory Items preserve useful material that
is not established fact. The service is evidence-linked, bounded, and
correction-friendly.

## Current boundaries

- `lighting-core` contains domain types and repository traits; it has no I/O.
- `lighting-service` contains application behaviour and HTTP routes.
- `lighting-store-surreal` is the only SurrealDB adapter.
- `lighting-cli` is the machine-readable command-line client.
- `apps/lighting` wires the service and CLI.
- The optional Tethers preview is an adapter, not Lantern's memory authority.

Current architecture is described in:

- `README.md` — purpose, setup, and supported commands
- `CURRENT.md` — short operational status
- `docs/architecture/LANTERN_LIVING_MEMORY_ARCHITECTURE.md`
- `docs/architecture/MEMORY_GOVERNANCE.md`
- `docs/architecture/PERSPECTIVE_AND_PROVENANCE.md`
- `docs/architecture/CONTEXT_COMPILER.md`
- `docs/architecture/DREAMER_AND_FOREMAN.md`
- `docs/basic-memory-migration.md`, `docs/recovery.md`, and `docs/LANTERNBENCH.md`

Historical qualification and recovery notes are labelled as such. Do not treat
them as current implementation instructions.

## Build and validation

The repository pins Rust 1.98.1 and Edition 2024. The normal Windows shell is
PowerShell. The MSVC Build Tools and Windows SDK required by Rust's
`x86_64-pc-windows-msvc` target are platform prerequisites; the Visual Studio
IDE is not a project dependency.

```powershell
pwsh -NoProfile -File .\scripts\validate.ps1
```

Embedded SurrealKV is the normal local path. The optional remote lane uses
SurrealDB client/server 3.3.0-beta.4 and the scripts under `scripts/`.

## Repository hygiene

- Preserve unrelated changes and inspect a dirty tree before acting.
- Never commit `.env`, `.lighting-data/`, `.lighting-runtime/`, or
  `.private-migration/`; the latter contains private recovery material.
- Keep generated output and local databases out of Git.
- At meaningful verified checkpoints, commit and push the feature branch.
- When a feature is complete and acceptance gates pass, merge to the canonical
  branch when appropriate and push it. Never force or overwrite unknown remote
  work.
- Do not add editor, agent, or machine-specific setup to the repository.

Before a substantial change, inspect `git status`, the current branch, and
relevant tests. Do not reset, silently discard, or delete unknown work.
