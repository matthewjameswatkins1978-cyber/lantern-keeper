# Lantern Keeper

Lantern Keeper is a local-first, event-sourced epistemic memory for humans
and AI. It keeps the evidence, the perspective that produced an assertion,
the current reconciled understanding, and the useful-but-not-yet-factual
material separate.

That separation is the point. Ordinary AI memory tends to turn a summary into
a fact, a repeated sentence into consensus, and a correction into a rewrite.
Lantern Keeper keeps a route back to what actually happened.

> Matthew talks. Lucy remembers. Machines assist. Matthew corrects.

## The idea in one diagram

```text
conversation / file / tool result
              |
              v
       Source: exact evidence
              |
              v
       Episode: bounded event
              |
              v
       Claim: what was asserted
       + perspective + provenance
              |
              v
       Belief: current projection
       + lineage + validity + scope

soft Memory Items stay beside this path until they are ready for stronger
interpretation. Context Packs read from these records; they never become
evidence merely because an AI generated them.
```

## Why Lantern is different

### It is event-sourced epistemic memory

Lantern does not store one mutable blob called “memory”. Sources, Episodes,
Claims, and Beliefs have different jobs:

| Record | Meaning |
| --- | --- |
| Source | What happened: exact text or an external artefact |
| Episode | The bounded interaction or work event around that evidence |
| Claim | What someone asserted, including provenance and perspective |
| Belief | The current reconciled projection for a holder, subject, predicate, and scope |
| Memory Item | A soft idea, quote, fragment, lesson, open loop, or other useful possibility |
| Trace / Proposal | Why a candidate or projection exists and what governance decision occurred |

### It preserves exact text before interpretation

The evidence layer can retain the supplied text and byte span before a Claim or
Belief is derived. A generated summary is a view over evidence, not a superior
source. This makes `why` useful: a person or agent can follow a belief back to
the Claim, Episode, and Source that support it.

### It preserves perspective instead of inventing consensus

Matthew, Lucy, shared context, and legacy material are not interchangeable.
Authorship, speaking, transmission, endorsement, and belief ownership are
separate fields. Quoting is not authorship; silence is not endorsement;
assistant repetition is not proof about Matthew. Imported material whose author
cannot be established remains explicitly legacy/shared rather than being
silently attributed to Matthew.

### Corrections preserve history

A correction adds new evidence and a new Claim. It does not rewrite the old
Source or erase the earlier projection. The affected current belief can be
invalidated, dependent projections marked stale, and the prior lineage retained
for inspection. Current retrieval and historical retrieval are intentionally
different questions.

### Soft memory stays soft

Ideas, fragments, quotes, creative seeds, tensions, and open loops can be
remembered without being promoted into factual Claims. The default memory path
is deliberately permissive; factual promotion is narrower and governed.

### AI governance is candidate-only

Dreamer, extractors, semantic matchers, and external agents may propose Claims,
soft memories, associations, contradictions, or correction suggestions. Lucy
is the routine Memory Foreman, but even Foreman review records a bounded,
traceable decision. Candidate generation does not directly establish canonical
Beliefs.

## What works today

There are two states to keep clear:

- The canonical `master` branch contains the recovered Source → Episode →
  Project retrieval and writeback proof.
- The latest verified feature line,
  [`feature/lantern-full-move`](https://github.com/matthewjameswatkins1978-cyber/lantern-keeper/tree/feature/lantern-full-move)
  at checkpoint `345c468`, contains the newer epistemic slice described below.

The verified feature-line work includes:

- Rust 1.98.1 / Edition 2024 and a local-first SurrealDB design with embedded,
  versioned SurrealKV as the normal local path;
- durable Source, Episode, Project, Claim, Belief, soft Memory Item, relation,
  Trace, Proposal, Predicate, and Dimension foundations;
- predicate and scope normalization, explicit unmapped Claims, deterministic
  reconciliation, echo suppression, direct-holder gates, and transitive stale
  propagation;
- current-versus-historical context selection with stable Context Pack IDs,
  typed Matthew/Lucy/shared/legacy sections, bounded budgets, source/episode
  provenance, candidate reasons, and retrieval traces;
- correction recording with immutable evidence, belief lineage, stale
  invalidation, and ambiguity-safe targeting;
- logical export/restore accounting and a deterministic LanternBench suite;
- a local stdio MCP bridge exposing bounded context, remember, search, why,
  correct, status, and Lucy-owned Foreman operations. Soft memories remain the
  default and explicit Claim capture remains governed;
- a repaired Basic Memory snapshot with 61 notes, 496 observations, and 275
  relations accounted for with no unexplained items.

This README does not count the unfinished provenance-complete live factual
capture work, a real Lucy-native client proof, Basic Memory shadow comparison,
or cutover as complete. Those remain acceptance gates for the feature line.
See [`CURRENT.md`](CURRENT.md) for the exact branch/status boundary.

## What is still experimental

- The feature line has not yet been merged to `master`.
- A real connected Lucy-native MCP client proof, full MCP contract coverage,
  and restart qualification remain required.
- Basic Memory remains the migration source and rollback archive until shadow
  comparison, final-delta accounting, and cutover acceptance are green.
- Dreamer operations, exact typed/fused retrieval, richer graph-backed project
  association, and bounded graph expansion are follow-up work.
- The optional remote SurrealDB lane and the Tethers preview are bounded
  integration surfaces, not the memory authority or a hosted service.
- Cloud storage, GUI workflows, universal importers, recommendations, and
  automatic conversation observation are outside the current proof.

## Who this is for

Lantern Keeper is for people who want an AI collaborator to remember useful
context without quietly rewriting history or claiming more certainty than the
evidence supports. It is especially useful for:

- long-running software projects where decisions and corrections must remain
  inspectable;
- a human and an AI sharing context while retaining distinct perspectives;
- local-first workflows that need export, recovery, and an honest degraded
  mode;
- builders of MCP or agent integrations who want bounded, explainable memory
  operations instead of raw database writes.

It is not a replacement for a general-purpose document store, a vector-search
demo, a workflow engine, or an autonomous truth-making agent.

## Try the recovered master proof

The canonical master baseline runs locally and keeps its database and runtime
state out of Git:

```powershell
git clone https://github.com/matthewjameswatkins1978-cyber/lantern-keeper.git
cd lantern-keeper
pwsh -NoProfile -File .\scripts\validate.ps1
```

The first-proof demonstration uses only local public CLI/API paths:

```powershell
.\scripts\start-surreal.ps1
.\scripts\run-first-proof-local.ps1
```

It captures a Markdown Source, records an Episode, links a Project and Marker,
retrieves a deterministic Codex handoff with exact excerpts, records the result
back into the Project, and retrieves the updated handoff. The service binds to
`127.0.0.1:4317` by default.

The newer epistemic commands and MCP bridge are documented on the feature line
until that line is accepted into `master`.

## Documentation map

- [`CURRENT.md`](CURRENT.md) — branch boundary, verified state, and remaining gates
- [`docs/architecture/LANTERN_LIVING_MEMORY_ARCHITECTURE.md`](docs/architecture/LANTERN_LIVING_MEMORY_ARCHITECTURE.md) — canonical memory model
- [`docs/architecture/PERSPECTIVE_AND_PROVENANCE.md`](docs/architecture/PERSPECTIVE_AND_PROVENANCE.md) — ownership, attribution, and evidence routes
- [`docs/architecture/MEMORY_GOVERNANCE.md`](docs/architecture/MEMORY_GOVERNANCE.md) — correction and promotion invariants
- [`docs/architecture/CONTEXT_COMPILER.md`](docs/architecture/CONTEXT_COMPILER.md) — bounded context packs and retrieval rules
- [`docs/architecture/DREAMER_AND_FOREMAN.md`](docs/architecture/DREAMER_AND_FOREMAN.md) — candidate-only AI governance
- [`docs/mcp.md`](docs/mcp.md) — local MCP contract and Lucy proof gate
- [`docs/basic-memory-migration.md`](docs/basic-memory-migration.md) — migration accounting and cutover boundary
- [`docs/recovery.md`](docs/recovery.md) — logical export and recovery procedure
- [`docs/LANTERNBENCH.md`](docs/LANTERNBENCH.md) — deterministic behavioural acceptance suite
- [`docs/architecture/TETHERS_LANTERN_KEEPER_CANONICAL_ARCHITECTURE.md`](docs/architecture/TETHERS_LANTERN_KEEPER_CANONICAL_ARCHITECTURE.md) — historical joint Tethers/Lantern architecture

## Design boundaries

Lantern Keeper is the memory authority. Tethers, when present, coordinates
bounded actions through public capabilities; it does not replace Lantern's
evidence or governance model. The optional Tethers preview is explicitly
read-only with respect to Lantern memory.

Do not commit `.env`, `.lighting-data/`, `.lighting-runtime/`, or
`.private-migration/`. Do not bind the local write service as unauthenticated
remote infrastructure.
