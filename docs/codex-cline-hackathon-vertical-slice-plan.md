# Lantern Keeper - Hackathon Vertical-Slice Plan

## Mission

For the next few days, we are proving one complete, believable Lantern Keeper loop:

```text
Capture knowledge
-> store it as canonical, source-backed memory
-> retrieve the right project context
-> give that context to Codex
-> complete work
-> write the result back as canonical memory
-> retrieve again and show the updated handoff
```

We are proving the concept, not shrinking the vision: every shortcut must preserve the path to the full Lantern Keeper.

The current system has already proved:

```text
Source -> Episode -> Project / Marker links -> deterministic retrieval
-> exact authoritative excerpts -> Codex-oriented context_package
```

The remaining vertical-slice work is to make the final half real:

```text
Codex handoff -> result file -> recorded back into the Project -> updated handoff
```

## Architecture rules

- Keep the existing layered architecture.
- `lighting-core`: pure domain and repository traits.
- `lighting-store-surreal`: SurrealDB and graph implementation only.
- `lighting-service`: application operations, API DTOs, handlers, handoff rendering.
- `lighting-cli`: HTTP client and local file handling only.
- `apps/lighting`: composition and unified CLI parsing only.
- Source content remains authoritative.
- An Episode remains a byte range into a Source; do not store copied excerpts.
- The existing V3 native relation tables remain the active graph storage.
- No new database or domain model unless the existing primitives genuinely cannot express the workflow.
- Prefer composing the existing Source, Episode, Project, Marker, association, retrieval, and context-package primitives.
- Any adapter or script must stay isolated at the edge and be safe to replace later.

## Scope rules for this sprint

Build only what directly completes or demonstrates the loop.

Defer:

- embeddings or semantic search;
- AI-generated summaries;
- automatic OpenAI/Codex API calls;
- MCP server work;
- cloud sync, accounts, authentication, multi-user support;
- UI;
- universal importers;
- memory decay/compression modes;
- Tasks, Decisions, Reports, ranking, recommendations, or broad graph expansion;
- speculative abstractions for imagined future needs.

No demo hacks in core code. No fake data path that bypasses canonical storage or retrieval.

## Work-sharing rules

### Codex owns

- Architecture decisions and scope control.
- Reading Cline's report critically before accepting a task.
- Writing one complete, copyable Cline task at a time.
- Ensuring every Cline prompt contains the relevant current architecture and exact acceptance criteria.
- Rejecting scope drift, silent test weakening, raw database leakage, duplicated source text, or unproven claims.
- Deciding whether a task is accepted, needs a small corrective task, or should be deferred.
- Human-facing integration checks: reviewing the final demo, README story, and Git checkpoint timing.

Codex should not hand architectural discovery to Cline. Cline implements a bounded task; Codex decides what the task is.

### Cline owns

- Inspecting only the files needed for the assigned task.
- Implementing one bounded change.
- Adding focused tests.
- Running the exact validation requested.
- Returning a concise report with evidence.

Cline must not:

- start a new feature because it looks related;
- refactor broad areas "while here";
- change architecture;
- alter migrations or schema unless explicitly asked;
- commit, push, reset, checkout, delete, or overwrite unrelated work;
- weaken, skip, or delete tests to make validation pass;
- repeatedly retry the same failing approach.

## Cline time limits - mandatory

Default hard limit: **10 minutes total per task**.

Use 15 minutes only when Codex explicitly says why the task genuinely needs it. Never use an open-ended limit.

Every Cline task must include:

- At 7 minutes: stop adding capability and begin validation/reporting.
- At 10 minutes: stop completely.
- At most two materially identical implementation attempts.
- Abort any command with no useful output after 90 seconds.
- One full validation run only, with one rerun only if a validation failure was fixed.
- If blocked: report the exact blocker and the smallest next repair. Do not keep digging.
- No follow-on work after acceptance criteria are met.

If Cline breaches these limits, Codex should treat the code separately from the process: assess whether it works, but record the breach and make the next task smaller.

## Planned task sequence

### Checkpoint 1 - Current handoff work

Before more feature work:

- Ensure LK-017 / current handoff changes pass the full workspace suite.
- Run the local proof once.
- Matthew makes a Git/GitHub checkpoint.

Do not bundle unrelated evidence folders or local state into commits.

### LK-018 - Codex Handoff CLI

Current next task, if not already complete:

```text
lighting project-handoff <project-id>
lighting project-handoff <project-id> --json
```

Normal output prints only the server-produced `context_package`, unchanged and ready to paste into Codex.

This is a thin CLI wrapper over existing project retrieval. No new service, database, or domain work.

Time limit: 10 minutes.

### LK-019 - Writeback Design Gate

Codex first decides the smallest permanent writeback shape before giving Cline code.

Target user experience:

```text
lighting project-record-result <project-id> <result-file> --title "..."
```

The result file is recorded as a canonical Source. A full-range Episode points into it and is linked to the Project.

The existing Project handoff must then include that recorded result on the next retrieval.

Before implementation, Codex must settle and state:

- the idempotency rule for recording the same result file twice;
- whether the CLI orchestrates existing HTTP calls or requires one small service operation;
- how file contents are read without altering UTF-8, whitespace, or line endings;
- how a missing or invalid Project is handled;
- how the CLI avoids creating duplicate Project-linked Episodes on a normal rerun.

Do not create a new `Report` or `Task` domain model merely to satisfy this proof.

This is a design/review task only unless the answer is obvious from existing primitives.

Time limit: 10 minutes.

### LK-020 - Record Result Through the Real System

After Codex accepts the LK-019 design, give Cline one implementation task.

Definition of done:

1. A user can give one local Markdown or text result file to the CLI.
2. Its content is preserved exactly as a Source.
3. It is represented by a full-range Episode.
4. That Episode is linked to the specified Project.
5. A repeat is safe according to the accepted idempotency rule.
6. `lighting project-handoff <project-id>` subsequently includes the result with:

- Source ID,
- Episode ID,
- exact byte range,
- exact excerpt,
- deterministic provenance.

7. Focused tests prove exact preservation, project linkage, rerun behaviour, and safe errors.

Time limit: 15 minutes only if Codex judges that the accepted design spans service + CLI + live integration testing. Otherwise split it into two 10-minute tasks.

### LK-021 - Demonstrate the Full Loop

Create or extend a harmless local demonstration that proves:

```text
1. Seed a Project with source-backed knowledge.
2. Produce a Codex handoff.
3. Create a small result file from the handoff.
4. Record that result through the real CLI/API path.
5. Retrieve the Project again.
6. Show that the handoff now contains both earlier knowledge and the new recorded result.
```

Requirements:

- Use isolated or clearly harmless local demo data.
- Never delete user data.
- Never upload data.
- Start and stop only a service process that the script itself started.
- Avoid port conflicts and executable locks.
- State files remain local and Git-ignored.
- The demo must use real public CLI/API paths, not direct database writes.
- Matthew runs the final demo manually once and confirms the output.

Time limit: 10 minutes for script work. If the work is larger, split it rather than extending the limit.

### Checkpoint 2 - Hackathon proof

After the full loop works:

- Run full validation.
- Run the live demonstration.
- Make a Git/GitHub checkpoint.
- Update README with the exact four-step proof.
- Write the hackathon description around the working loop, not unbuilt future features.

## Definition of hackathon success

A viewer can understand and verify this in under two minutes:

```text
Lantern Keeper takes exact project knowledge,
preserves its provenance,
retrieves the relevant context,
formats it for Codex,
and lets the completed work be written back into the same durable memory.
```

That is enough. Everything else is the future system, not this sprint.
