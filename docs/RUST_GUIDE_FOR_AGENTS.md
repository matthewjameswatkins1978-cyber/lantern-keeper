# Lantern Keeper — Rust Guide for Coding Agents

Read this guide before editing any `.rs` file, `Cargo.toml`, or `Cargo.lock`.

## Version contract

| Item | Value |
| ---- | ----- |
| Declared MSRV (workspace) | Rust **1.89** |
| Edition | **2021** |
| Ambient installed toolchain | rustc/cargo **1.97.1** |
| Dependency resolution | `Cargo.lock` is authoritative |

**Critical rule:** Rust 1.89 is the compatibility ceiling for source choices.
Agents must not use language features, standard-library APIs, Cargo behaviour, or
dependency features introduced after Rust 1.89 merely because the local Rust
1.97 compiler accepts them.

Consult **version-appropriate** official documentation:

- [Rust 1.89 standard library](https://doc.rust-lang.org/1.89.0/std/)
- [The Rust Reference](https://doc.rust-lang.org/reference/)
- [The Cargo Book](https://doc.rust-lang.org/cargo/)
- [The Edition Guide](https://doc.rust-lang.org/edition-guide/)
- [Clippy documentation](https://doc.rust-lang.org/clippy/)

For exact crate APIs, inspect the version recorded in `Cargo.lock` and consult
the matching [docs.rs](https://docs.rs) page. Do not guess APIs from newer
crate releases.

## Workspace boundaries

```
apps/lighting              — binary: Axum HTTP server, wiring, entry point
crates/lighting-core       — library: domain types, traits, no I/O
crates/lighting-service    — library: business logic, ProjectService, orchestrates stores
crates/lighting-store-surreal — library: SurrealDB repository implementations
crates/lighting-cli        — binary: CLI tool (blocking reqwest, admin commands)
```

**Dependency direction:**

```
lighting ─────────────────────────────────────────┐
  └─ lighting-service ─────────────────────┐      │
       ├─ lighting-core                    │      │
       └─ lighting-store-surreal ──────────┤      │
            └─ lighting-core               │      │
lighting-cli ──────────────────────────────┤      │
  └─ lighting-service ─────────────────────┘      │
  └─ lighting-core                                │
```

- `lighting-core` has **no** dependency on service, store, Axum, Tokio, or SurrealDB.
- `lighting-store-surreal` must not import from `lighting-service`.
- Binaries wire everything; libraries define behaviour.

## Conventions

### Serialization
- `Serialize` / `Deserialize` on DTOs (data transfer objects) only.
- Use `serde` derives; keep domain types separate from wire format where practical.

### Error handling
- **stable API error shape:** `{ "code": "...", "message": "..." }`
- `thiserror` in `lighting-service`, `lighting-core`, and `lighting-store-surreal` layers.
- `anyhow` with `.context()` / `bail!` in binaries (`lighting`, `lighting-cli`).
- Do not use `unwrap()` or `expect()` in production integration paths.
- Preserve existing error types and conversions; do not silently replace them.

### Async runtime
- Tokio multi-thread runtime (`rt-multi-thread`).
- Axum async route handlers.
- **Blocking `reqwest`** belongs to `lighting-cli` only; it must not be copied into
  async service paths. Use async `reqwest` (or an appropriate async HTTP client)
  inside Tokio contexts.

### Storage
- SurrealDB access stays behind repository trait boundaries.
  No direct `surrealdb` calls outside `lighting-store-surreal`.
- Do not bypass `ProjectService` or repository traits.

### Formatting
- `rustfmt.toml` enforces **Unix newlines** (`\n`). Preserve this.
- `.cargo/config.toml` sets `AWS_LC_SYS_NO_ASM = "1"` — respect this environment
  variable; do not remove or override it.

### Thread safety
- Do not block Axum/Tokio worker threads with `std::process::Command` or
  synchronous filesystem I/O. Use `tokio::task::spawn_blocking` for CPU-bound or
  blocking work.

### Unsafe
- Do not introduce `unsafe` without an explicit architectural decision recorded
  in task approval.

## Dependency discipline

- **No new dependencies** without explicit task approval.
- **No feature changes** (adding, removing, or altering Cargo features) without
  explicit task authority.
- Inspect `Cargo.lock` for the exact resolved version of every crate you use.
- Consult the **exact version** documentation on docs.rs; do not assume APIs
  from later releases.

## Code discipline

- No `unwrap()` or `expect()` in production integration paths.
- Preserve existing error types and `From` conversions.
- Do not bypass `ProjectService` or repository abstractions.
- Do not put application adapters in `lighting-core` or `lighting-store-surreal`.
- Do not block async worker threads with synchronous I/O.
- Do not use `unsafe` without an explicit architectural decision.

## Validation

The project validation command:

```powershell
pwsh -NoProfile -File .\scripts\validate.ps1
```

This runs, in order:

1. `cargo fmt --check` — verifies formatting
2. `cargo clippy --workspace --all-targets -- -D warnings` — zero-warning lint
3. `cargo test --workspace` — full test suite
4. `cargo run -p lighting -- version` — smoke-test the binary

**Note on integration tests (Windows):** Some integration tests require a live
SurrealDB instance or Docker. The documented skip command for Windows CI is
recorded in project task documentation; do not run or alter it in routine
documentation tasks.

## Tethers integration constraints

Lantern Keeper's initial Tethers integration is preview-only.

- **First event:** `lantern.project_result_preview_requested`
- **Engine path:** configured via `TETHERS_ENGINE_PATH` environment variable
- **Scope:** preview only — no storage writes, no external Effects
- **Boundary:** Tethers never accesses Lantern Keeper storage
- **Future Actions:** `lantern.*` Actions will call public Lantern Keeper
  services (not internal stores)
- **Adapter location:** belongs in `lighting-service`
- **Protocol:** newline-delimited JSON over stdin/stdout — byte-level format
  must remain exact
- **Engine location:** the OCaml engine's local opam switch is path-bound; do
  not move `tethers-0.1/engine-ocaml/` from the Tethers repository

**Future process integration gate:** Running Tethers as a child process requires
a separate architectural decision to either:
- enable appropriate Tokio process/I/O features; or
- isolate synchronous process work safely (e.g. `spawn_blocking`).

Do not choose or change Tokio features in this documentation task.

## Task workflow

1. Read this guide.
2. Read repository-specific task documentation.
3. Inspect `Cargo.toml`, `Cargo.lock`, and relevant source.
4. Make the smallest change that satisfies the task.
5. Run `cargo fmt --all` before the final test.
6. Run the requested validation.
7. Report changed files, test results, and next steps.
8. Do not stage, commit, push, or tag unless the task explicitly authorises it.