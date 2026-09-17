# Lantern Keeper — Current State

Status as of **17 September 2026**.

## Status

Lantern Keeper is now a **source-ready developer preview** on the canonical `master` branch.

The core memory architecture, bounded MCP surface, authority model, execution receipts, Tethers bridge, OpenShell boundary proof, live external-evidence candidate path, and M6 Trust Console have all been merged into `master`.

The project is ready for other developers to **clone, build, inspect, run locally and integrate from source**. It is not yet a packaged binary release or polished one-click end-user application.

## What Lantern Keeper is now

Lantern is no longer just a persistent conversation-memory experiment. It has become a general trust layer with two deliberately separate responsibilities:

1. **Epistemic memory** — evidence, events, claims, beliefs, corrections, soft memory, context selection and provenance.
2. **Authority and effects** — principals, grants, revocations, capability checks, sandboxed execution and receipts.

Knowledge can influence reasoning, but it cannot create permission. AI-generated material can become a candidate, but it cannot silently become canonical truth or authority.

## Verified memory capabilities

- Rust 1.98.1 / Edition 2024 workspace.
- Embedded, versioned SurrealKV normal local store.
- Optional SurrealDB 3.3.0-beta.4 remote lane.
- Durable Source, Episode, Project, Claim, Belief, soft Memory Item, relation, Trace, Proposal, Predicate and Dimension foundations.
- Exact evidence-linked factual capture before reconciliation.
- Claim-to-Belief reconciliation with immutable revisions and lineage.
- Corrections preserve prior evidence and history rather than rewriting it.
- Transitive stale invalidation and current-versus-historical retrieval.
- Predicate and scope normalisation, explicit unmapped Claims, echo suppression and direct-holder gates.
- Deterministic bounded Context Packs with selection reasons, budgets, provenance and persisted retrieval traces.
- Logical export/restore and recovery foundations.
- LanternBench behavioural acceptance coverage.
- Basic Memory migration accounting, including a repaired snapshot with no unexplained imported items.
- A real 1,479-record logical export/restore parity drill has passed.

## Verified client boundary

The local stdio MCP bridge exposes exactly eight bounded tools:

- context
- remember
- search
- why
- correct
- status
- Foreman queue
- Foreman review

A real Codex Desktop client has completed the connected lifecycle, including factual capture, soft memory, context retrieval, provenance explanation, correction and restart persistence.

The bridge rejects unknown arguments and does not expose raw database mutation.

## Verified authority and execution capabilities

- Durable `PrincipalId`, `AuthorityGrant` and `AuthorityRevocation` records.
- Exact capability checks with expiry and revocation handling.
- Trusted, principal-bound control sessions for authority mutation.
- Server-owned IDs, timestamps, issuer identity and provenance.
- Cross-principal revocation rejection.
- Fail-closed behaviour when persistent authority state cannot be read.
- Canonical execution receipts with SHA-256 hashes and previous-receipt continuity links.
- Export/restore preservation for active and revoked authority plus receipts.
- Control sessions and CSRF material remain ephemeral and are not exported as reusable authentication.

## Lantern Warden demonstration profile

The hackathon authority/execution work is referred to as **Lantern Warden**. It demonstrates how the general Lantern Keeper model can govern external evidence, AI candidate reasoning and a real effect boundary.

### M4 — OpenShell effect boundary

Verified locally with OpenShell 0.0.116 under WSL2/Docker:

- approved synthetic write/read succeeded;
- forbidden path access was denied;
- network egress was denied by the sandbox proxy;
- Lantern control credentials and audit tokens were not exposed to the sandbox;
- there is no unsandboxed fallback path.

### M5 — live cognitive plane

Verified path:

```text
Tavily result -> Source -> Episode -> candidate-only Nemotron interpretation
```

External evidence retains query, URL, domain, title, retrieval time, rank, provider metadata and a normalised hash before model interpretation. A live Nebius Nemotron call produced a validated candidate without mutating canonical memory or authority.

Provider failure is typed and fail closed. Model confidence is not treated as proof.

### M6 — Trust Console

The local Trust Console makes the chain visible as four distinct states:

1. **HEARD** — external evidence and provenance.
2. **THOUGHT** — candidate interpretation only.
3. **AUTHORISED** — Tethers policy plus authenticated authority decision.
4. **DONE** — sandbox/effect outcome and receipt evidence.

The console demonstrates denial without authority, grant, authorised replay, wrong-scope denial and revocation denial. Live cognitive lookup is optional; candidate output never becomes authority.

## Ready for external use

The repository is public and can be used now by developers willing to build from source.

The supported starting point is:

```bash
git clone https://github.com/matthewjameswatkins1978-cyber/lantern-keeper.git
cd lantern-keeper
cargo build --workspace
cargo test --workspace
cargo run -p lighting -- serve
```

The normal service address is `127.0.0.1:4317`.

There is currently **no formal GitHub Release or packaged installer**. That is packaging work, not a blocker to cloning and using the current source tree.

## Remaining engineering and product work

The largest remaining items are polish and expansion rather than proof that the central architecture works:

- publish versioned release artifacts and install instructions;
- broaden cross-platform packaging and CI coverage;
- remove or configure remaining legacy actor-name defaults in user-facing surfaces while retaining historical fixtures where useful;
- add richer typed/fused retrieval and graph-backed project associations;
- expand bounded graph retrieval and optional semantic candidate lanes;
- improve generic importers and client adapters;
- explore hosted/cloud sync without weakening local-first trust boundaries;
- package additional client integrations where the client product exposes the required local MCP/plugin surface.

## Important boundary

Some historical fixtures, scripts and migration evidence still contain the original human/assistant names used while the architecture was being built. They are retained where they provide useful test or historical evidence. They should not be interpreted as a requirement that Lantern is tied to those identities.

The public model is generic: humans, assistants, agents, shared holders, legacy holders and principals are roles or identifiers supplied by a deployment.
