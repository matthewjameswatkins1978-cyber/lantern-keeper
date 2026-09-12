# Lantern Keeper — Rust Guide

This guide covers the repository's Rust conventions. It is tool-neutral and
does not require a particular editor or AI assistant.

## Version contract

| Item | Value |
| --- | --- |
| Rust | 1.98.1 |
| Edition | 2024 |
| Dependency resolution | `Cargo.lock` is authoritative |
| SurrealDB | exactly 3.3.0-beta.4 |

Keep source choices compatible with the pinned toolchain. Inspect the lockfile
and matching crate documentation before relying on an API.

## Crate boundaries

    apps/lighting                 server binary and wiring
    crates/lighting-core          domain types and repository traits; no I/O
    crates/lighting-service       application behaviour and HTTP routes
    crates/lighting-store-surreal SurrealDB repository implementations
    crates/lighting-cli           machine-facing CLI client

Storage access stays behind repository traits. Do not put Axum, Tokio, or
SurrealDB in `lighting-core`, and do not bypass service operations from a
client or route.

## Conventions

- Keep domain types separate from wire DTOs where practical.
- Use the stable API error shape `{ "code": "...", "message": "..." }`.
- Use `thiserror` in library layers and `anyhow` at binary boundaries.
- Do not use `unwrap()` or `expect()` in production integration paths.
- Do not block Tokio workers with synchronous process or file I/O.
- Keep source and evidence append-oriented; preserve provenance on derived
  changes and corrections.
- Do not introduce `unsafe` without a recorded architectural decision.
- Do not add dependencies or alter dependency features without explicit scope.

## Native build prerequisites

Windows builds target `x86_64-pc-windows-msvc`, so Microsoft C++ Build Tools
and the Windows SDK are normal platform prerequisites. Lantern does not depend
on the Visual Studio IDE or an inherited IDE environment. Do not add a
repository-specific linker path or copy runtime libraries into the project.

The repository's `.cargo/config.toml` sets
`AWS_LC_SYS_PREBUILT_NASM=1`. This is required by the pinned transitive
`aws-lc-sys 0.45.0` build on a normal Windows shell because NASM is not a
supported repository prerequisite. Keep this setting in sync with that
dependency; do not replace it with a different AWS-LC workaround.

## Validation

Run the single supported validation path from a plain PowerShell session:

    pwsh -NoProfile -File .\scripts\validate.ps1

Equivalent checks are:

    cargo fmt --all -- --check
    cargo check --locked --workspace --all-targets --all-features
    cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
    cargo test --locked --workspace
    cargo run --locked -p lighting -- version

Remote integration tests are opt-in and require the exact SurrealDB version
documented in `README.md`. Embedded tests should remain the default fast lane.
