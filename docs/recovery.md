# Recovery

Create a logical Lantern export before schema changes or cutover. The export
contains a manifest plus newline-delimited records for ledger/evidence,
projects, relations, ordinary memories, and epistemic records. The export is
engine-independent and can be retained with the private migration snapshot.

The current recovery checkpoint is the local commit recorded in
`docs/recovery/LANTERN_KEEPER_RECOVERY_2026-09-12.md`. To resume, check out the
feature branch, confirm the working tree is clean, confirm `.private-migration`
and `.lighting-data` are still ignored private paths, apply schema migrations,
and rerun focused tests before any import or cutover action.

No public push, tag, or release is implied by a successful local build.
