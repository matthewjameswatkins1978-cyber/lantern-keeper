# Lantern Keeper — Current State

## Repository status — 10 October 2026 (post-integration update)

- **Default branch:** `master` at `657affbe31440a66b925266668dc0879aa80e5e5` (merge of PR #17). SurrealDB 3.3.0 stable and the Windows runtime consolidation are now **on master**; PR #11 (Warden M7 demo) merged earlier as `cb8d77d`. PR #12 was closed as superseded by #17 with its history preserved — do not treat it as an open integration task.
- **CI scope:** `.github/workflows/ci.yml` runs Rust checks/tests on `ubuntu-latest` with `LIGHTING_SKIP_INTEGRATION_TESTS=1`, plus a `lantern_git_export_test.py` unittest step and a `public-image` Docker build job. Still no Windows CI lane, and a green badge is still **not** a persistence, recovery or hostile-test certification.
- The pre-integration audit below is preserved for provenance; its statements about branch state refer to `master` at `8f506da`.

### Pre-integration audit — 10 October 2026 (`master` at `8f506da`)

This is a **dated GitHub audit**, not a claim that the same branch/release state will remain current. Consult the live repository and CI before a new consequential decision.

- **Default branch:** `master` at `8f506dadf7802c3b4ce40d1049d22ffadc8c5b3c`. Its most recent observed [CI run](https://github.com/matthewjameswatkins1978-cyber/lantern-keeper/actions/runs/37149292477) passed. Current master still pins `surrealdb = "=3.3.0-beta.4"`; do not present SurrealDB 3.3.0 stable as landed on master.
- **Open PR #12:** [Windows runtime consolidation and SurrealDB 3.3.0 stable](https://github.com/matthewjameswatkins1978-cyber/lantern-keeper/pull/12), head `7b199476`. Its PR CI passed at that head on 3 October, but GitHub currently reports **merge conflicts** (`mergeable_state: dirty`). It requires independent reconciliation, conflict resolution and verification; do not merge automatically.
- **Open PR #11:** [Lantern Warden M7 replay-only public demo](https://github.com/matthewjameswatkins1978-cyber/lantern-keeper/pull/11), head `4c03abda`; GitHub reports mergeable with green CI, but this is a distinct demonstration profile and requires an explicit merge decision. The newer `codex/lantern-warden-cloud-run` branch (`0cce3ca5`) also has a passing 9 October [CI run](https://github.com/matthewjameswatkins1978-cyber/lantern-keeper/actions/runs/37988282966), not evidence that its changes landed on `master`.
- **Release/distribution:** no published GitHub Releases returned as of this audit. The repository currently declares `MIT OR Apache-2.0` with both license files; no relicensing decision has been made. Twenty named branches exist, including feature, recovery and archive references; do not prune them without ownership/provenance review.
- **CI scope:** `.github/workflows/ci.yml` currently runs Rust checks/tests on `ubuntu-latest` and sets `LIGHTING_SKIP_INTEGRATION_TESTS=1`. A green CI badge is therefore **not** an end-to-end persistence, Windows, recovery or hostile-test certification. The inspected default branch reports `protected: false`; consider branch safeguards before public release, without changing repository settings implicitly.
- **ChatGPT integration:** as of 10 October, a fresh **ordinary ChatGPT Chat** could load the private Lantern Keeper V2 skill but no native `lantern_*` tools were registered (empty registry result). The local/read-only Streamable HTTP transport passed separate tests; this does **not** establish Chat tool mounting or authenticated production deployment. The OpenAI Plugin Creator and `Auth unsupported` investigation remains open; preserve the `lantern-git` fallback.
- **Next release gates:** reconcile PR #12, clarify Warden's independent delivery path, rerun integration/recovery and cross-platform tests, obtain documented install/upgrade/rollback proof, run isolated Terror Bat adversarial campaigns coordinated by Gary/Pi, verify security/privacy and bounded hosting costs, and independently accept native MCP tool execution in *ordinary Chat* before claiming that integration. See [the roadmap](docs/ROADMAP.md#release-gates--10-october-2026).

### Earlier verified engineering checkpoint

The material below is a snapshot recorded on **17 September 2026**. It is preserved for provenance; descriptions of what is "current" or "verified" below apply to that checkpoint unless refreshed against newer code/tests.

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
- Optional SurrealDB 3.3.0 remote lane.
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
