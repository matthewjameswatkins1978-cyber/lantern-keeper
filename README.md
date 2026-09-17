# Lantern Keeper

> A local-first trust layer for persistent AI memory and controlled agent action.

Lantern Keeper is infrastructure for AI systems that need to **remember without quietly rewriting history** and **act without quietly inheriting unlimited authority**.

Most AI memory systems flatten very different things into one bucket: what somebody said, what a model inferred, what is currently believed, what might be useful later, and what an agent is allowed to do. Lantern Keeper keeps those things separate on purpose.

That gives it two connected jobs:

1. **Epistemic memory** — preserve evidence, perspective, corrections, current belief, soft memory, and retrieval lineage.
2. **Authority and execution trust** — keep remembered information separate from permission, then record what an authorised agent actually did.

A memory system should be able to answer **“why do you believe this?”**. An agent system should also be able to answer **“why were you allowed to do that?”**. Lantern is built around both questions.

## The idea

```text
KNOWLEDGE

conversation / file / tool result / external evidence
                         |
                         v
                    Source
                 exact evidence
                         |
                         v
                    Episode
                  bounded event
                         |
                         v
                     Claim
            what was actually asserted
          + perspective + provenance
                         |
                         v
                     Belief
             current reconciled view
          + lineage + scope + validity
                         |
                         v
                  Context Pack
             bounded working context

Soft Memory Items sit beside this chain for ideas, fragments, lessons,
creative seeds, tensions and other useful material that is not established fact.

ACTION

Principal -> Authority Grant -> Tethers policy -> bounded executor -> Receipt
                     ^                                      |
                     |                                      v
               revocation / expiry                    actual outcome
```

The two chains deliberately do **not** collapse into one another. A remembered fact cannot grant authority. A model suggestion cannot grant authority. A generated Context Pack cannot become evidence merely because an AI wrote it. Permission has to come through the authority path.

## Why this is useful

Long-lived AI systems accumulate context. Context is useful, but it is also where subtle errors become durable.

A summary can slowly become “fact”. A quotation can lose its speaker. Repetition can look like consensus. A correction can overwrite the thing it corrected. An assistant can quote its own earlier output until the system appears to have evidence that never existed.

Lantern Keeper treats those as modelling problems, not prompt-engineering problems.

Its central design choices are:

- **evidence before interpretation** — retain the source route back to what happened;
- **perspective is explicit** — originator, speaker, transmitter, holder and stance are different things;
- **corrections are additive** — new evidence supersedes current understanding without erasing history;
- **soft memory stays soft** — an idea can remain useful without becoming a factual claim;
- **AI output is candidate material** — Dreamer, extractors and semantic systems may propose, but do not silently establish canonical truth;
- **retrieval is inspectable** — Context Packs carry reasons, provenance, budgets and omissions;
- **authority is separate from knowledge** — knowing something does not imply permission to act on it;
- **effects are receipted** — authorised actions can be tied to an outcome and an auditable receipt chain;
- **local-first by default** — the normal path uses embedded SurrealKV and keeps private state on the operator's machine.

## What can Lantern Keeper be used for?

Lantern began as persistent memory for a long-running human/AI collaboration, but the architecture is intentionally broader than that origin.

### Persistent personal or professional AI memory

Give an assistant long-term memory without turning every remembered sentence into an unquestioned fact. Preferences, decisions, corrections, project knowledge, ideas and open loops can coexist while retaining their different trust levels.

### Long-running project memory

Keep design decisions, requirements, rejected paths, results and corrections attached to evidence. A new agent or session can receive a bounded Context Pack instead of a giant undifferentiated transcript.

### Multi-agent systems

Different agents can share evidence while retaining distinct perspectives and permissions. One agent's output does not automatically become another agent's authority or belief.

### Auditable research and retrieval

External search results can be stored as evidence before interpretation. A model can produce candidate conclusions while the route back to URLs, retrieval metadata, Sources and Episodes remains inspectable.

### Safer autonomous or semi-autonomous agents

Lantern's authority model can be used with Tethers and a sandboxed executor so an agent receives only narrow, explicit capabilities. Grants can expire or be revoked, and execution can fail closed when authority is missing.

### Decision logs and institutional memory

Teams can preserve not only the current decision but why it was made, what evidence existed at the time, what later corrected it, and which historical view is being requested.

### Creative work

Ideas, fragments, jokes, references, patterns, tensions, creative seeds and rejected paths can remain useful without being misclassified as facts. This is especially valuable for creative systems where “interesting” and “true” are different categories.

### Memory migration and portability

Lantern includes logical export/restore and migration accounting so memory is not intended to become an opaque database that can never be inspected or moved.

### Trust-aware RAG and context assembly

Lantern can sit underneath retrieval systems as the layer that decides what kind of thing a record is, where it came from, whether it is current, and why it was selected.

## What works today

The current `master` branch is a **source-ready developer preview**. It is suitable for developers who want to clone it, inspect it, run it locally, build integrations, or experiment with the model now.

Implemented and verified work includes:

- Rust 1.98.1 / Edition 2024 workspace;
- embedded, versioned SurrealKV as the normal local store, with an optional SurrealDB 3.3.0-beta.4 remote lane;
- durable Sources, Episodes, Projects, Claims, Beliefs, soft Memory Items, relations, Traces, Proposals, predicates and dimensions;
- immutable evidence-linked factual capture;
- deterministic Claim-to-Belief reconciliation, supersession, correction lineage and stale propagation;
- current-versus-historical retrieval;
- bounded deterministic Context Packs with provenance, selection reasons and retrieval traces;
- logical export and restore;
- LanternBench behavioural acceptance coverage;
- a local stdio MCP bridge exposing eight bounded tools;
- a real connected Codex Desktop MCP proof covering remember, search, context, why, correction, restart persistence and provenance;
- durable authority grants and revocations with exact capability checks, expiry and fail-closed behaviour;
- server-owned authority IDs, timestamps and provenance rather than client-authored trust records;
- canonical execution receipts with SHA-256 continuity links;
- Tethers authority integration;
- a verified OpenShell sandbox path with denied forbidden paths, denied network egress and no Lantern control credentials exposed to the sandbox;
- an external-evidence cognitive path using Tavily -> Source/Episode -> candidate-only Nemotron output;
- a Trust Console that makes the chain visible as **HEARD -> THOUGHT -> AUTHORISED -> DONE**.

The hackathon authority/execution work is sometimes referred to as **Lantern Warden**. Warden is best understood as a demonstration profile built on the wider Lantern Keeper architecture, not a replacement for the general memory system.

## Download and run from source

There is not yet a packaged GitHub Release or one-click installer. The repository itself is public and ready to clone or download and build from source.

Prerequisites:

- Git
- Rust 1.98.1 toolchain
- the normal native build prerequisites for your platform

```bash
git clone https://github.com/matthewjameswatkins1978-cyber/lantern-keeper.git
cd lantern-keeper
cargo build --workspace
cargo test --workspace
```

Start the local service:

```bash
cargo run -p lighting -- serve
```

The service binds to `127.0.0.1:4317` by default.

In another terminal:

```bash
cargo run -p lighting -- health --json
cargo run -p lighting -- doctor --json
```

Run the stdio MCP bridge:

```bash
cargo run -p lighting -- mcp
```

Platform-specific helper scripts may exist under `scripts/`, but ordinary validation and usage should not depend on PowerShell or any single operating system.

## MCP surface

The MCP bridge intentionally exposes a small capability surface rather than raw database access:

- `lantern_context`
- `lantern_remember`
- `lantern_search`
- `lantern_why`
- `lantern_correct`
- `lantern_status`
- `lantern_foreman_queue`
- `lantern_foreman_review`

See [`docs/mcp.md`](docs/mcp.md).

## Trust boundaries

Lantern Keeper is deliberately conservative about what becomes authoritative.

```text
Evidence answers:       what happened?
Claims answer:          what was asserted?
Beliefs answer:         what is the current reconciled view?
Context Packs answer:   what is useful for this request?
Authority answers:      what may this principal do?
Receipts answer:        what action was actually attempted or completed?
```

Those are different questions and they stay different in the data model.

## Current limitations

The engine is usable now, but it is still a developer-facing project rather than a finished end-user product.

Current follow-up work includes:

- packaged binaries and a formal versioned GitHub Release;
- broader cross-platform packaging and CI polish;
- richer typed/fused retrieval and graph-backed project association;
- more generic user-facing configuration around legacy actor labels that remain in some reference fixtures and defaults;
- optional hosted/cloud workflows without weakening the local-first trust model;
- broader client packaging as product access allows.

Historical reference fixtures may still use the original human/assistant names from the project that produced them. Those names are examples and test history, not requirements of the architecture.

## Documentation

- [`CURRENT.md`](CURRENT.md) — verified current state
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — next engineering and packaging work
- [`docs/architecture/LANTERN_LIVING_MEMORY_ARCHITECTURE.md`](docs/architecture/LANTERN_LIVING_MEMORY_ARCHITECTURE.md) — memory model
- [`docs/architecture/PERSPECTIVE_AND_PROVENANCE.md`](docs/architecture/PERSPECTIVE_AND_PROVENANCE.md) — attribution and evidence lineage
- [`docs/architecture/MEMORY_GOVERNANCE.md`](docs/architecture/MEMORY_GOVERNANCE.md) — promotion, correction and review rules
- [`docs/architecture/CONTEXT_COMPILER.md`](docs/architecture/CONTEXT_COMPILER.md) — bounded context assembly
- [`docs/architecture/DREAMER_AND_FOREMAN.md`](docs/architecture/DREAMER_AND_FOREMAN.md) — candidate-only AI governance
- [`docs/mcp.md`](docs/mcp.md) — MCP integration contract
- [`docs/recovery.md`](docs/recovery.md) — export and recovery
- [`docs/LANTERNBENCH.md`](docs/LANTERNBENCH.md) — behavioural acceptance suite
- [`HACKATHON.md`](HACKATHON.md) and [`docs/hackathon/`](docs/hackathon/) — Lantern Warden trust-chain demonstrations and evidence

## Licence

Lantern Keeper is dual-licensed under MIT or Apache-2.0. See [`LICENSE-MIT`](LICENSE-MIT) and [`LICENSE-APACHE`](LICENSE-APACHE).

## One sentence

**Lantern Keeper is a chain of custody for AI memory, plus a separate chain of authority for AI action.**
