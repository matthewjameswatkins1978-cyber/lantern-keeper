# Threadmoth formatter repair

## Problem

On the preserved `foundation/lantern-pre-memory` baseline at `2431bbb`,
`cargo fmt --all -- --check` failed across 45 Rust files. The failures were
inherited newline debt plus a bounded set of rustfmt style differences. The
repository already had an `.editorconfig` requesting LF, but had no
`.gitattributes` policy and Git reported `core.autocrlf=true`.

## Threadmoth

- Version: `threadmoth 1.3.0`
- Discovery: `threadmoth --version`, `threadmoth --help`,
  `threadmoth capabilities`, `threadmoth suggest .gitattributes`
- File safety: every existing-file mutation carried an expected SHA-256,
  exact relative path, one-file budget, path-prefix budget, and an explicit
  cardinality. Every mutation was previewed first.
- Operations used:
  - `file/create_file` to add `.gitattributes`.
  - `text/replace` with the exact `CRLF` byte sequence replaced by `LF`.
  - `patch/unified_diff` with exact rustfmt-generated patch context.
- Existing files touched: 45. The work used 33 newline operations and 28
  exact formatting patches; the latter overlapped the 16 files needing both.

The actual installed command surface uses `capabilities`; historical
`describe`/`capability list` spellings are not present in this build.

## Classification

Classification was based on the pre-edit byte-level newline profile and the
rustfmt inventory:

| Classification | Files | Meaning |
| --- | ---: | --- |
| `CRLF_ONLY` | 16 | CRLF/newline failure and no rustfmt style diff |
| `FORMAT_DRIFT` | 12 | rustfmt style diff with LF source |
| `MIXED` | 17 | mixed newline profile or both newline and style debt |
| `UNKNOWN` | 0 | none remained unexplained |

The 17 mixed cases include `crates/lighting-service/src/project_ops.rs`,
which had a mixed newline profile, and 16 files requiring both newline and
rustfmt repair.

## Newline policy

Added the minimal repository policy in `.gitattributes`:

```gitattributes
*.rs text eol=lf
*.toml text eol=lf
```

PowerShell, batch, Markdown, and other file classes were not swept into the
policy. `git check-attr eol` reports `lf` for Rust and TOML files.

## Diff quality

The repair diff, excluding this report, contains 29 files: `.gitattributes` plus 28 files
with rustfmt hunks, totalling 119 insertions and 102 deletions. The 17
newline-only files are already LF-normalized in Git's index, so their
attributes-aware content diff is empty after the policy is staged. No
whole-file semantic rewrite, comment loss, BOM change, literal change, or
unrelated file churn was observed. `git diff --check` passed.

`cargo fmt --all` was not used as the editing step. rustfmt was used only to
produce the expected per-file patch content; Threadmoth applied the patches.

## Verification

All commands were run on Windows in PowerShell 7 from the isolated branch.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets --locked --offline` | PASS |
| `cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings` | PASS |
| `cmd /c "set LIGHTING_SKIP_INTEGRATION_TESTS=1&& cargo test --workspace --locked --offline"` | PASS: 215 passed, 0 failed, 1 ignored |
| `git diff --check` | PASS |

## Outcome

Threadmoth successfully fixed the formatter check with a small,
inspectable, evidence-backed diff. The repair did not alter Lantern Keeper
functionality or perform a blind whole-repository formatting rewrite.
