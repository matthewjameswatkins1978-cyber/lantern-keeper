# Lantern Keeper

Lantern Keeper is a local-first, event-sourced epistemic memory for humans and AI. It keeps the evidence, the perspective that produced an assertion, the current reconciled understanding, and useful-but-not-yet-factual material separate.

That separation is the point. Ordinary AI memory can turn a summary into a fact, a repeated sentence into consensus, or a correction into a rewrite. Lantern Keeper keeps a route back to what actually happened.

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

Lantern does not store one mutable blob called “memory”. Sources, Episodes, Claims, and Beliefs have different jobs:

| Record | Meaning |
| --- | --- |
| Source | What happened: exact text or an external artefact |
| Episode | The bounded interaction or work event around that evidence |
| Claim | What someone asserted, including provenance and perspective |
| Belief | The current reconciled projection for a holder, subject, predicate, and scope |
| Memory Item | A soft idea, quote, fragment, lesson, open loop, or other useful possibility |
| Trace / Proposal | Why a candidate or projection exists and what governance decision occurred |

### It preserves exact text before interpretation

The evidence layer can retain the supplied text and byte span before a Claim or Belief is derived. A generated summary is a view over evidence, not a superior source. This makes `why` useful: a person or agent can follow a belief back to the Claim, Episode, and Source that support it.

### It preserves perspective instead of inventing consensus

Matthew, Lucy, shared context, and legacy material are not interchangeable. Authorship, speaking, transmission, endorsement, and belief ownership are separate fields. Quoting is not authorship; silence is not endorsement; assistant repetition is not proof about Matthew. Imported material whose author cannot be established remains explicitly legacy/shared rather than being silently attributed to Matthew.

### Corrections preserve history

A correction adds new evidence and a new Claim. It does not rewrite the old Source or erase the earlier projection. The affected current belief can be superseded, dependent projections marked stale, and the prior lineage retained for inspection. Current retrieval and historical retrieval are intentionally different questions.

### Soft memory stays soft

Ideas, fragments, quotes, creative seeds, tensions, and open loops can be remembered without being promoted into factual Claims. The soft-memory path is permissive; factual promotion is narrower and governed.

### AI governance is candidate-only

Dreamer, extractors, semantic matchers, and external agents may propose Claims, soft memories, associations, contradictions, or correction suggestions. Lucy is the routine Memory Foreman, but Foreman review is bounded and traceable. Candidate generation does not directly establish canonical Beliefs.

## The memory trust kernel

Lantern Keeper is the hackathon's memory trust kernel for personal AI. It is
designed to answer three questions about every important remembered statement:

```text
WHY DO YOU BELIEVE THIS?
IS IT STILL TRUE?
IS THIS INFORMATION ALLOWED TO MAKE YOU ACT?
```

The security boundary is intentionally independent of the epistemic model:

```text
Source -> Episode -> Claim -> Belief       (what Lantern knows or believes)
Principal -> AuthorityGrant -> Tethers      (what an agent may do)
```

Memory may influence reasoning, but it cannot create or revoke authority. The
current hackathon status and evidence are in
[`HACKATHON.md`](HACKATHON.md), [`THREAT_MODEL.md`](THREAT_MODEL.md), and
[`docs/hackathon/NEBIUS_2026_BUILD.md`](docs/hackathon/NEBIUS_2026_BUILD.md).

## What works today

The epistemic memory foundation is now merged into canonical `master`.

It includes:

- Rust 1.98.1 / Edition 2024 with embedded, versioned SurrealKV as the normal local store and an exact SurrealDB 3.3.0-beta.4 remote lane;
- durable Source, Episode, Project, Claim, Belief, soft Memory Item, relation, Trace, Proposal, Predicate, and Dimension foundations;
- predicate and scope normalization, explicit unmapped Claims, deterministic reconciliation, echo suppression, direct-holder gates, and transitive stale propagation;
- current-versus-historical Context Packs with stable IDs, typed Matthew/Lucy/shared/legacy sections, bounded budgets, provenance, candidate reasons, and retrieval traces;
- correction recording with immutable evidence, belief lineage, stale invalidation, and ambiguity-safe targeting;
- provenance-complete live factual capture: exact `evidence_text` becomes Source/Episode evidence before the normalized Claim is reconciled;
- logical export/restore accounting and the deterministic LanternBench suite;
- a bounded local stdio MCP bridge exposing `lantern_context`, `lantern_remember`, `lantern_search`, `lantern_why`, `lantern_correct`, `lantern_status`, `lantern_foreman_queue`, and `lantern_foreman_review`;
- a real Codex Desktop MCP proof with all eight tools, remember/search/context/why/correct, current/history separation, provenance, and restart persistence;
- a repaired Basic Memory snapshot with 61 notes, 496 observations, and 275 relations accounted for with no unexplained items.

The Nebius integration is deliberately candidate-only: the optional Nemotron
adapter can propose validated Dreamer candidates, but it has no authority-write
operation. Live provider availability must be proved separately; see
[`docs/hackathon/NEBIUS_INTEGRATION.md`](docs/hackathon/NEBIUS_INTEGRATION.md).

See [`CURRENT.md`](CURRENT.md) for the exact operational state.

## What remains before memory cutover

- Basic Memory shadow comparison against representative real memories;
- final Basic Memory delta and idempotency/accounting check;
- final private export plus representative retrieval after a fresh restore;
- explicit cutover decision and rollback marker.

Normal ChatGPT/Lucy full read/write custom MCP access remains a separate OpenAI product-access limitation. That does not invalidate the working Lantern engine or the proven Codex Desktop client path.

## Who this is for

Lantern Keeper is for people who want an AI collaborator to remember useful context without quietly rewriting history or claiming more certainty than the evidence supports. It is especially useful for:

- long-running projects where decisions and corrections must remain inspectable;
- a human and an AI sharing context while retaining distinct perspectives;
- local-first workflows that need export, recovery, and an honest degraded mode;
- MCP and agent integrations that want bounded, explainable memory operations instead of raw database writes.

It is not a replacement for a general-purpose document store, a vector-search demo, a workflow engine, or an autonomous truth-making agent.

## Build, test, and run

```powershell
git clone https://github.com/matthewjameswatkins1978-cyber/lantern-keeper.git
cd lantern-keeper
pwsh -NoProfile -File .\scripts\validate.ps1
```

The service binds to `127.0.0.1:4317` by default. Embedded SurrealKV is the normal local path; the optional remote SurrealDB lane is documented under `scripts/` and the architecture guides.

The MCP integration is documented in [`docs/mcp.md`](docs/mcp.md).

## Documentation map

- [`CURRENT.md`](CURRENT.md) — verified state and remaining gates
- [`docs/architecture/LANTERN_LIVING_MEMORY_ARCHITECTURE.md`](docs/architecture/LANTERN_LIVING_MEMORY_ARCHITECTURE.md) — canonical memory model
- [`docs/architecture/PERSPECTIVE_AND_PROVENANCE.md`](docs/architecture/PERSPECTIVE_AND_PROVENANCE.md) — ownership, attribution, and evidence routes
- [`docs/architecture/MEMORY_GOVERNANCE.md`](docs/architecture/MEMORY_GOVERNANCE.md) — correction and promotion invariants
- [`docs/architecture/CONTEXT_COMPILER.md`](docs/architecture/CONTEXT_COMPILER.md) — bounded context packs and retrieval rules
- [`docs/architecture/DREAMER_AND_FOREMAN.md`](docs/architecture/DREAMER_AND_FOREMAN.md) — candidate-only AI governance
- [`docs/mcp.md`](docs/mcp.md) — local MCP contract and connected-client boundary
- [`docs/basic-memory-migration.md`](docs/basic-memory-migration.md) — migration accounting and cutover boundary
- [`docs/recovery.md`](docs/recovery.md) — logical export and recovery procedure
- [`docs/LANTERNBENCH.md`](docs/LANTERNBENCH.md) — deterministic behavioural acceptance suite
- [`docs/history/`](docs/history/) — selected historical context

## Design boundaries

Lantern Keeper is the memory authority. Tethers, when present, coordinates bounded actions through public capabilities; it does not replace Lantern's evidence or governance model. The optional Tethers preview is read-only with respect to Lantern memory.

Do not commit `.env`, `.lighting-data/`, `.lighting-runtime/`, or `.private-migration/`. Do not expose the local write service as unauthenticated remote infrastructure.
