# Lantern Keeper dependency modernisation — 2026-09-12

## Decision

The supported development baseline is:

| Component | Selected version | Evidence |
| --- | --- | --- |
| Rust | 1.98.1 stable | repository-pinned `rust-toolchain.toml`; local `rustc`/`cargo` verification |
| Edition | 2024 | workspace and package manifests |
| SurrealDB Rust client | 3.3.0-beta.4 | exact Cargo pin and lockfile |
| SurrealDB server | 3.3.0-beta.4 | existing Windows executable upgraded in place with rollback copy; fresh-server qualification |

SurrealDB 3.3.0-beta.4 was selected after the matched client/server lane
passed the complete workspace tests against a fresh in-memory server. The
release is intentionally experimental: its graph and relation improvements
are useful to Lantern Keeper's next memory stages. The [official beta.4
release notes](https://surrealdb.com/releases/3.3) describe the graph,
traversal and relation changes; upgrading this pin requires rerunning the
qualification matrix.

## Direct dependency review

All direct workspace dependencies were inspected with Cargo's resolved tree.
The following manifest or resolved-version changes were made:

| Package | Before | After | Reason |
| --- | --- | --- | --- |
| `surrealdb` | `=3.3.0-beta.3` | `=3.3.0-beta.4` | matched the qualified server and adopted the preferred graph/storage beta |
| `uuid` in `lighting-core` | package-local `1` + `v4` feature | workspace declaration | remove a duplicate declaration; resolved version is `1.26.1` and imported IDs remain unchanged |
| `async-trait` in `lighting-store-surreal` | package-local `0.1` | workspace declaration | keep the direct dependency on the single reviewed workspace pin |

The other direct dependencies were already current enough for the selected
Rust baseline and passed the full verification matrix. Their resolved versions
at this checkpoint are:

```text
anyhow 1.0.104              axum 0.8.9
async-trait 0.1.92          chrono 0.4.45
clap 4.6.6                  dotenvy 0.15.7
reqwest 0.12.28             serde 1.0.229
serde_json 1.0.151          sha2 0.10.9
thiserror 2.0.20            tokio 1.53.1
tower 0.5.3                 tower-http 0.6.11
tracing 0.1.44              tracing-subscriber 0.3.23
urlencoding 2.1.3           uuid 1.26.1
```

No new dependency was added. UUIDv7 was not enabled because existing imported
identity is deterministic and the current record contract does not need an ID
rewrite; that can be evaluated for new records separately.

## Security audit

`cargo audit` against the selected beta.4 lockfile reports:

- `RUSTSEC-2026-0194` and `RUSTSEC-2026-0195`: `quick-xml 0.39.4`, high
  severity. The crate is required by `object_store 0.13.2` inside SurrealDB;
  that dependency requires `quick-xml ^0.39.0`, so the fixed 0.41.0 release
  cannot be selected without an upstream-compatible SurrealDB/object_store
  change.
- `RUSTSEC-2023-0071`: `rsa 0.9.10`, medium severity Marvin timing advisory.
  RustSec reports no fixed upgrade; it is transitive through `jsonwebtoken`
  in SurrealDB's authentication stack.
- `RUSTSEC-2023-0089`: `atomic-polyfill 1.0.3` is reported as unmaintained;
  this is an audit warning rather than an actionable vulnerability in the
  current graph.

The stable 3.2.4 control removed the two quick-xml advisories but retained the
same unfixed RSA advisory. The beta.4 selection therefore remains local
experimental infrastructure and is not a public hostile-input deployment.
The findings are documented, not suppressed; broader exposure remains blocked
until the upstream chains are resolved.

## Qualification matrix

| Lane | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets --all-features --locked` | PASS on beta.4 |
| strict Clippy with all targets/features | PASS |
| complete workspace tests against fresh matched beta.4 server | PASS: 215 passed, 0 failed, 1 ignored |
| embedded SurrealKV qualification | PASS |
| three previously blocked credentialed authentication paths | PASS in the matched beta.4 lane |
| `lighting doctor --json` against fresh migrated beta.4 store | PASS: server 3.3.0-beta.4, schema 7, status OK |
| stable 3.2.4 compile/control | PASS compile; cold parallel live run exposed two startup transaction conflicts and was not selected |
| `cargo audit` | FAIL-as-policy: two quick-xml advisories, one unfixed RSA advisory; one unmaintained warning |
| `lighting version` | PASS: Lighting 0.1.0 (Lantern Keeper) |
| `git diff --check` | PASS after documentation changes |

The first cold parallel remote run on beta.4 also exposed two transient
namespace/database transaction conflicts. After the server readiness gate, a
repeat complete default-concurrency run passed; the same cold symptom occurred
on the 3.2.4 control, so it is recorded as a startup qualification condition
rather than attributed uniquely to beta.4.

## Platform and recovery handling

The only discovered SurrealDB executable was
`C:\Users\Matmus\AppData\Local\SurrealDB\surreal.exe`. The 3.2.1 executable
was preserved at the adjacent rollback path
`surreal.exe.lantern-3.2.1.backup` with SHA-256
`C7BE2555B6D4ABE9BA307E85FD5E80414255C9D00A1E5F686205ACE16B5126E3`.
The selected beta.4 executable was also retained at
`surreal.exe.lantern-3.3.0-beta.4.backup` with SHA-256
`D9D90B8FE5873254AD201656E105321407F0AF4FC857AE500AD6F4ED71E9C80C`.

All compatibility servers used fresh in-memory stores. No new server binary
opened the preserved `.lighting-data` database or Basic Memory snapshot. The
feature branch remained isolated; `master`, recovery branches, tags and
releases were untouched.
