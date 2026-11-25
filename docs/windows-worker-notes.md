# Windows Development Notes

Use ordinary PowerShell and the repository-pinned Rust toolchain. Validate the
checkout with `pwsh -NoProfile` so editor, IDE, and inherited shell state do
not hide setup problems.

Rust's MSVC target needs Microsoft C++ Build Tools and Windows SDK libraries.
The Visual Studio IDE is not required. If `link.exe`, `cl.exe`, or a Windows
SDK library cannot be found, repair the machine installation; do not add a
project-specific linker path or copy CRT files into the checkout.

Keep `.lighting-data/`, `.lighting-runtime/`, and `.private-migration/` local
and ignored. The migration directory is private recovery material; runtime
state and generated build output are disposable only after confirming no
relevant process is using them.

PowerShell quoting and Windows paths matter. Prefer small commands and inspect
generated files immediately. Stop Lighting before exporting embedded SurrealKV
because its files may be locked. Use the exact-version SurrealDB scripts for
the optional remote lane.

Before a checkpoint inspect branch, status, worktrees, processes, and
`git diff --check`. Preserve unrelated work and never reset or delete unknown
files.
## SurrealDB 4.0 on Windows

The repository pins `surrealdb = 4.0.1` with `surrealkv 1.4.0`. Two differences
from the 3.x lane that have bitten local runs:

- **`file://` is removed.** A 4.0 server refuses to start with "The `file://`
  scheme is no longer supported; use `surrealkv://` instead". Start the local
  server with a SurrealKV path — `scripts/start-surreal.ps1` uses
  `surrealkv:<data-dir>` (the single-colon shorthand is accepted by 4.0.1 and
  has been run successfully). Do not introduce `file://` in scripts or notes.
- **Version gates are exact.** `scripts/start-surreal.ps1` and
  `scripts/check-surreal.ps1` refuse any server that is not the pinned version,
  so they were updated with the dependency bump. A 3.3.0 binary will be
  rejected with a clear message rather than half-working.

An in-place upgrade of an existing 3.x datastore is *not* what these scripts do:
4.0 requires an explicit `experimental_upgrade_from_3x` opt-in for that, and the
qualified path is a fresh 4.x store built from a logical export. Never point the
new engine at the only copy of a datastore.

### Test-thread stack depth (harness-level, measured)

Run the test suite on Windows with `RUST_MIN_STACK=33554432`:

```powershell
$env:RUST_MIN_STACK = '33554432'
cargo test --workspace -- --test-threads=1
```

Without it, 8 of the 31 test binaries abort with `STATUS_STACK_OVERFLOW`
(exit `0xc00000fd`) under surrealdb 4.0.1, measured on 25 November 2026:
`epistemic_reconciliation`, `lanternbench`, `authority_persistence`,
`memory_repository`, `ledger_repository`, `export_restore`,
`export_gitattributes`, `surrealkv_qualification`. These all build a store and
repository graph inside one test thread, so this is a property of the harness
stack on Windows rather than a defect in one test.

Do not paper over it with per-test thread wrappers: an explicit-stack wrapper
protects only the test it is applied to, and nine tests already overflow. The
setting belongs where the suite is invoked (local worker shell or CI job env
for a Windows runner). Linux CI is unaffected; the hosted workflow runs on
`ubuntu-latest`.
