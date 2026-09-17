# Lantern Keeper — Worker Guide

Lantern Keeper is a local-first trust layer for persistent AI memory and controlled agent action.

Sources preserve what happened. Episodes bound evidence into events. Claims preserve what was asserted. Beliefs represent current reconciled understanding. Soft Memory Items retain useful material that is not established fact. Authority grants are a separate trust path that determines what a principal may do.

The service is evidence-linked, bounded, correction-friendly and fail-closed at trust boundaries.

## Current boundaries

- `lighting-core` contains domain types and repository traits; it has no I/O.
- `lighting-service` contains application behaviour and HTTP routes.
- `lighting-store-surreal` is the SurrealDB/SurrealKV adapter.
- `lighting-cli` is the machine-readable command-line client.
- `apps/lighting` wires the service, CLI, MCP bridge and local console surface.
- Tethers is an authority/policy integration, not Lantern's memory authority.
- OpenShell is an effect boundary, not a source of permission.
- External models and search providers produce evidence or candidates; they do not directly establish canonical memory or authority.

Current architecture is described in:

- `README.md` — public overview, use cases and source quick start
- `CURRENT.md` — verified operational state
- `docs/ROADMAP.md` — remaining engineering and packaging work
- `docs/architecture/LANTERN_LIVING_MEMORY_ARCHITECTURE.md`
- `docs/architecture/MEMORY_GOVERNANCE.md`
- `docs/architecture/PERSPECTIVE_AND_PROVENANCE.md`
- `docs/architecture/CONTEXT_COMPILER.md`
- `docs/architecture/DREAMER_AND_FOREMAN.md`
- `docs/mcp.md`, `docs/recovery.md`, and `docs/LANTERNBENCH.md`

Historical qualification and recovery notes are labelled as such. Do not treat them as current implementation instructions.

## Build and validation

The repository pins Rust 1.98.1 and Edition 2024.

The default engineering assumption is **cross-platform**. Ordinary code and tests must not hard-code PowerShell, Windows drive letters, Windows path semantics, shell-specific quoting, or other platform behaviour unless the feature under test is explicitly platform-specific.

Portable baseline:

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Platform-specific helper scripts may exist under `scripts/`, but they are helpers, not the semantic authority for ordinary cross-platform validation.

Embedded SurrealKV is the normal local path. The optional remote lane uses SurrealDB client/server 3.3.0-beta.4.

## Engineering rules

- Consequential truth must be explicit.
- Prefer one semantic authority for each concept.
- Keep reasoning bounded and inspectable.
- Put hard constraints below the model rather than relying on prompts.
- AI or provider output is candidate material until the appropriate deterministic/governed boundary accepts it.
- Evidence and permission are separate systems.
- Fail closed on authority or provenance ambiguity where proceeding would create trust.
- Preserve cross-platform behaviour unless a platform-specific target is explicitly stated.
- Tests should prove mechanisms rather than special-case one fixture or query.

## Repository hygiene

- Preserve unrelated changes and inspect a dirty tree before acting.
- Never commit `.env`, `.lighting-data/`, `.lighting-runtime/`, or `.private-migration/`.
- Keep generated output and local databases out of Git.
- Before substantial changes, inspect the current branch and relevant tests.
- Do not reset, silently discard, or delete unknown work.
- At meaningful verified checkpoints, commit and push the feature branch.
- Merge only after the intended acceptance gates pass.
- Never force or overwrite unknown remote work.
- Do not add editor, agent or machine-specific setup as a general project requirement.
