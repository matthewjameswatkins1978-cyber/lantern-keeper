# Lantern Keeper — Agent Guidance

Read this file before making changes to this repository.

## Before Rust work

Read `docs/RUST_GUIDE_FOR_AGENTS.md` for the MSRV, convention, dependency,
and code-discipline rules that govern every Rust change.

## Crate boundaries

```
lighting-core        — no I/O, no Axum, no SurrealDB
lighting-service     — business logic, ProjectService, orchestrates stores
lighting-store-surreal — SurrealDB repository impls only
lighting-cli         — CLI binary (blocking reqwest OK)
lighting              — Axum server binary
```

Do not import across boundaries in the wrong direction. Do not bypass
`ProjectService` or repository traits. Do not put application adapters in
`lighting-core` or `lighting-store-surreal`.

## Repository hygiene

- Repository-specific and source-specific instructions override generic agent
  advice.
- Preserve unrelated changes in the working tree; do not touch untracked files
  the task does not name.
- Stage and commit only when the task explicitly authorises it.
- Never push or tag unless explicitly requested.
- `Cargo.lock` is authoritative for dependency resolution.

## Documentation pointers

- `docs/RUST_GUIDE_FOR_AGENTS.md` — Rust rules, MSRV, conventions, validation
- `docs/ROADMAP.md` — project roadmap
- `.clinerules` — Cline task guardrails
- Task-specific documentation lives in `docs/` and task files.

Do not duplicate project documentation here. Read the relevant document for
the current task.