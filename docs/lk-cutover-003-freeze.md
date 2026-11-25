# LK-CUTOVER-003 — write freeze and high-water mark for migration handover

Scope: the authoritative boundary that lets a Basic Memory → Lantern migration
capture a delta **without dual uncontrolled writes** (LK-HOST-001 v1.1 M5).
This document is method only. It provisions nothing, moves no data and never
touches live Basic Memory — Work owns the private export and the switch.

Baseline revision: `992a355` on `cutover/d1-seam` (re-pin on every adoption
push; a freeze method that cannot be tied to a revision is not evidence).

## 1. The two supported boundary modes

Both modes produce the same artefact: a **fenced logical export** whose
manifest records `"fenced": true` and a caller-supplied `fence_id`. Only a
fenced export is an authoritative high-water mark (HWM).

### Mode A — write freeze (preferred when the window can be short)

1. Operator announces the freeze window and names the fence
   (e.g. `freeze-2026-11-25T0300Z`).
2. All writers stop: hosted MCP credentials are paused or withdrawn by Work,
   local `lighting serve` instances are stopped, agent lanes report idle.
   A freeze is an **operator action**, not a library call — nothing in
   Lantern can silently refuse writes on its own yet.
3. Verify quiescence: no in-flight sessions (client status), service log
   quiet, last-write timestamps older than the freeze start.
4. Take the fenced export (§3), record fence id, export hash, captured_at.
5. Capture the Basic Memory snapshot (Work) and its `export_sha256`.
6. Run the reconciliation commands (§4).
7. Unfreeze writers only after step 4's export is verified on independent
   storage.
8. Any write accepted after the freeze start is **post-HWM delta**: capture
   it in a second fenced export (`freeze-<n>-delta`) before cutover, or
   reject it until the window reopens. There is no silent "close enough".

### Mode B — high-water mark without a full freeze

Used when a freeze window is unacceptable (e.g. longer import verification):

1. Note the authoritative HWM inputs **at one instant**: fenced export
   (fence id + hash + captured_at) *and* the Basic Memory snapshot
   (`captured_at` + `export_sha256`). The pair, not either alone, is the HWM.
2. Treat every service mutation whose receipt/`updated_at` is newer than the
   export's `captured_at` as delta. Delta is enumerated from mutation
   receipts and ledger events, never from a re-read of current state.
3. Delta capture itself is fenced (Mode A steps 1–4) so the delta window
   cannot grow while it is being read.
4. Mode B is only as sound as clock discipline: both sides record UTC, and
   the reconciliation report (§4) records both timestamps. If either clock
   is suspect, fall back to Mode A.

Mode B never claims completeness it has not proven: the reconcile verdict
stays `fail` until a fenced export covers every post-HWM write.

## 2. What makes a high-water mark authoritative

- The export manifest must contain `"fenced": true` and a non-empty
  `fence_id`. An unfenced export records **no** `fenced` key at all — a
  report that cites an unfenced export as the HWM is invalid by definition.
- The fence id appears verbatim in the freeze record (this doc's run log),
  the export manifest and the reconciliation report, so the three can be
  cross-checked byte-for-byte.
- The export hash covers the manifest; a mismatch invalidates the HWM.
- Provider snapshots (D1 Time Travel, VM snapshots) are supplementary
  recovery aids, never the HWM itself.

## 3. Taking the fenced export (mechanics)

The store API already provides this boundary:

- `SurrealStore::export_fenced_to(directory, fence_id)` — writes the export
  and stamps `manifest.json` with `"fenced": true` + `"fence_id"`
  (`crates/lighting-store-surreal/src/export.rs`, fence support introduced
  on `cutover/d1-seam`).
- Plain `export_to(directory)` is **unfenced** and records no `fenced` key;
  it must never be cited as a HWM.

Procedure:

1. Hold the fence (Mode A) or fix the HWM instant (Mode B).
2. `export_fenced_to("<independent-storage>/lantern-hwm-<fence_id>", "<fence_id>")`.
3. Verify immediately: manifest contains `fenced == true`, the expected
   `fence_id`, `export_hash` matches a recomputed hash, and a fresh-schema
   restore of this export returns representative results with identical
   IDs/hashes/lineage (v1.1 M4 recovery gate).
4. Keep two verified, encrypted generations off the hosting provider.
5. Record in the run log: fence id, export path, export hash, captured_at,
   operator, unfreeze time.

## 4. Reconciliation at the boundary

With both sides captured, run the read-only reconciliation (LK-CUTOVER-001
C3) — it writes nothing:

```text
lighting basic-memory-reconcile --snapshot <bm-export-dir> \
  --output <report-dir>/reconcile.json --json
```

- Counts: notes present, Basic Memory-originated claims, imported relations.
- Content: exact per-note byte equality against the hosted Source.
- Relationships: imported relation coverage (unresolved lane).
- Anything REST cannot enumerate (soft memory items, ledger events,
  resolved relations, episodes) is listed under
  `not_enumerable_over_rest` and is **never counted as reconciled** — a
  `pass` verdict plus an unlisted gap is a broken report, and the command
  fails closed on any missing note, content mismatch or count delta.

Boundary acceptance: reconcile `verdict == pass` **and** the report's
snapshot timestamps fall inside the fence window recorded in §3.

## 5. Failure modes and their handling

| Failure | Detection | Handling |
|---|---|---|
| Writer not actually frozen | New mutation receipt newer than fence start | Freeze invalid; restart Mode A, do not proceed on Mode B |
| Unfenced export cited as HWM | Manifest lacks `fenced` | Reject the report; re-export under fence |
| Fence id drift between records | String compare across manifest / run log / report | Reject; ids are evidence, not labels |
| Export hash mismatch | Recomputed hash differs | Discard export; investigate before any restore |
| Clock skew across the pair | `captured_at` deltas unexplained | Fall back to Mode A; record skew |
| Reconcile gap hidden as pass | `not_enumerable_over_rest` emptied or missing items | Command treats that as malformed; never edit the gap list by hand |
| Post-HWM writes during delta | Receipts newer than delta fence | Second delta pass; cutover cannot proceed on a growing window |

## 6. Explicit non-goals

- No live Basic Memory export/import here (Work owns it; M5 data gate).
- No provisioning, deployment or credential changes.
- No claim that a freeze exists without an operator action behind it.
- No use of provider snapshots as the sole HWM.

## 7. Run log template

```text
fence_id:
mode: A (freeze) | B (high-water)
freeze_start_utc:            unfreeze_utc:
lantern_export_path:         lantern_export_hash:
manifest.fenced: true        manifest.fence_id: <must equal fence_id>
bm_snapshot_captured_at:     bm_export_sha256:
reconcile_report_path:       verdict:
operator:
notes:
```
