# Windows Worker Notes

Use the pinned Rust 1.98.1 toolchain and PowerShell. Keep `.lighting-data/`
and `.private-migration/` local and ignored. Stop the local service before
exporting embedded SurrealKV because the database file is single-writer. The
SurrealDB beta may emit background-task cancellation warnings on Ctrl-C; keep
them visible and treat them separately from query/test failures.

Before a packet checkpoint, inspect branch, status, worktrees, processes,
export health, and line-ending changes. Preserve unrelated worktrees and never
push, tag, or release without explicit instruction.
