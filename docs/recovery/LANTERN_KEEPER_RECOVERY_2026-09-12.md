# Lantern Keeper recovery baseline — 2026-09-12

## Scope

This report records the state found while taking over the stalled Lantern
Recovery Baseline / Lantern Keeper job. The previous ChatGPT Work run was
treated as untrusted. No completion state from that run was used.

## Repository baseline

- Repository: D:/The Next Thing/lantern-keeper-foundation
- Remote: https://github.com/matthewjameswatkins1978-cyber/lantern-keeper.git
- Checkout at inspection: spike/ambient-lucy-connection
- Checkout HEAD: e68e6390de533428d65d0b7ee5a6355764eed131
- master: ca4f86a (origin/master)
- Advanced local implementation head: e68e639
- July source/API worktree: agent/lighting-source-api at 407c529
- Recovery worktree: recovery/lantern-keeper-0.1 at 1637f33
- Rust: rustc 1.98.1, cargo 1.98.1
- Workspace edition and declared Rust version: 2024 / 1.98.1
- SurrealDB dependency: exactly pinned 3.3.0-beta.3, embedded SurrealKV enabled
- Existing .lighting-data directory at inspection: absent; no local database
  migration was required

The checkout was clean before this report and source snapshot work. The only
tracked working-tree change after recovery began is the ignored
.private-migration/ rule in .gitignore. The other worktree
D:/The Next Thing/lantern-keeper has four unrelated untracked scratch files:
_outline_test_file_.rs, _p.py, d, and source_outline_tests.rs. They were
preserved and not touched.

## Worktrees and process state

Git reported three worktrees: the advanced agent/lighting-source-api checkout,
this spike/ambient-lucy-connection checkout, and the clean
recovery/lantern-keeper-0.1 checkout. No detached or unreferenced commit
containing later Lantern work was visible in the inspected refs or reflog.

No cargo, rustc, surreal, lantern, or Mastra process was running. The visible
codex.exe processes were the desktop host, not a repository worker. There was
therefore no stale Lantern process to terminate.

## What the stalled run had accomplished

No commit, dirty source file, database change, or active build attributable to
the stalled September 12 run was found. The newest repository commits predate
that run and are the existing local modernization sequence:

1. 348581b — embedded SurrealKV modernization
2. 1ee1e8b — first Matthew/Lucy memory loop
3. 4cf1cce — temporal memory and durable ledger events
4. f23d74c — ambient ledger ingestion
5. 7ef2c4e — SurrealKV qualification ledger
6. 2431bbb — Rust formatting repair
7. e68e639 — final formatting normalization

These commits were preserved in place. No reset, clean, rebase, merge, push or
tag operation was performed.

## Baseline verification

- cargo fmt --check: passed.
- cargo clippy --workspace --all-targets --all-features --locked -- -D warnings:
  passed.
- cargo check --workspace --all-targets --all-features --locked: passed.
- LIGHTING_SKIP_INTEGRATION_TESTS=1 cargo test --workspace --locked: passed.
- cargo run -p lighting --locked -- version: passed and reported
  Lighting 0.1.0 (Lantern Keeper).
- The repository validation script without the documented remote-test skip
  failed only because three legacy tests attempted the unavailable
  ws://127.0.0.1:8000 server. The intended skip lane passed.

## Basic Memory source snapshot

The live Basic Memory Cloud Lantern project was reachable and enumerated before
migration work:

- project id: ff4f6328-dd77-4b0b-8e5a-485e73554d97
- discovered directories: 14
- discovered Markdown files: 61
- source content bytes: 210,186 UTF-8 bytes
- bracketed observation lines: 496
- wiki-link relation lines: 275
- repaired snapshot export SHA-256:
  d36c2768c63d2bf9dbd69960a52991503330f168eec9cef0afcdd58df83d1335

The first connector capture was invalid because request concurrency caused 53
empty bodies; it is marked invalid in
.private-migration/snapshot-2026-09-12/STATUS.md. A throttled second capture
was completed and validated at
.private-migration/snapshot-2026-09-12-repaired/manifest.json. The raw source
is ignored because it contains private personal memory. Basic Memory was not
deleted or modified.

## Recovery continuation

The recovered checkout contains a deterministic basic-memory-import command.
It validates the snapshot before contacting the service, imports each raw note
as a Markdown Source titled by its stable source path, and records upstream
identity and metadata as replay-safe basic-memory-cloud ledger evidence. The
checkpoint also extends the command to emit separate events for each bracketed
observation and typed relation; that extension is intentionally left
unexercised for the next packet rather than reported as verified.

The command was run against the local embedded store:

- first pass (verified before the checkpoint extension): 61 Sources and 61
  note events stored
- second pass: 61 Source duplicates and 61 note-event duplicates
- logical export: 122 records
- exported Sources: 61
- exported ledger events: 61
- source content hash mismatches: 0

The import deliberately did not turn complete notes or bracketed observations
into active canonical Memory records. That promotion requires a separate
reviewable reconciliation pass; silently treating every imported observation
as truth would violate the Source/evidence boundary.

## Recovery decision

Continue from e68e639 as the recovered implementation head, keep the
Basic Memory snapshot as immutable migration input, and make the next changes
on this checkout without publishing. The canonical branch remains master;
no merge into master is claimed until the importer, backend checks, and
post-import verification are complete.

## Full-packet continuation checkpoint

The new packet was continued from the recovery commit on the isolated branch
`feature/lantern-full-move`; the packet's expected master SHA
`a44554b7050e9b3dd2178eede99b67184d89d12c` is not present in this clone, so it
could not be verified as a local ancestor. Actual `master` remains
`ca4f86a2504d34c585f8984b9d93fabf4fe49142` and was not changed.

The typed epistemic model and local SurrealDB adapter now preserve Claims,
Beliefs, soft Memory Items, Traces, Proposals, and graph relations. The
importer creates exact whole-note Episodes and routes truth-bearing
observations to conservative Claim candidates while creative, historical, and
unknown observations become soft Memory Items. Imported wiki relations retain
their original labels and unresolved target keys.

Against the repaired private snapshot, the local run observed 61 duplicate
Sources, 61 reusable Episodes, 156 Claim candidates, 299 soft Memory Items,
and 191 unresolved relations. The identical second pass stored no duplicate
ledger events, observations, relations, Claims, Memory Items, or Episodes.
The local export at
`.private-migration/lantern-export-feature-2026-09-12-d` contains 1,479
logical records and includes `memory_relation` in its epistemic export.

Verification for this continuation: `cargo fmt --all`, workspace all-targets
check, workspace all-features Clippy with warnings denied, 49 core unit tests,
live embedded HTTP capture/search/stale checks, and two complete importer
passes. The packet remains pre-cutover: predicate reconciliation, richer
retrieval/context compilation, correction cascade tests, native Lucy client
proof, full restore validation, and final delta migration remain open.

## Foundation modernisation continuation

The expected packet checkpoint was subsequently present exactly at feature
HEAD `d58ffc9445b542ea32428ab13d2496bd7936cc20`; `origin/master` remained
`a44554b7050e9b3dd2178eede99b67184d89d12c`. Private paths remained ignored and
the recovery/archive branches were not modified.

The Rust client was advanced from SurrealDB 3.3.0-beta.3 to exactly
3.3.0-beta.4. The existing Windows server executable was upgraded from 3.2.1
to 3.3.0-beta.4 with the old executable retained as a SHA-256-verified
rollback copy. Qualification used fresh in-memory server stores only; the
preserved `.lighting-data` and `.private-migration` paths were not opened by
the new server binary.

The final matched beta.4 remote lane passed 215 tests with 0 failures and 1
ignored test, plus the embedded SurrealKV qualification. The stable 3.2.4
control compiled successfully and removed the quick-xml advisories, but beta.4
was selected under the packet's rule because its complete matched lane passed
and its graph/relation improvements are directly relevant. `cargo audit` still
reports the upstream-constrained quick-xml advisories on beta.4 and the
unfixed transitive rsa advisory; these are documented rather than suppressed.
