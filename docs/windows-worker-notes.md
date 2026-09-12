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
