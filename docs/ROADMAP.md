# Lantern Keeper Roadmap

## Release gates — 10 October 2026

The immediate objective is to make an already substantial **source-ready developer preview** reliably installable, maintainable and independently verifiable, while keeping Lantern Warden demo delivery and public hosted-service ambitions separate from core release acceptance.

1. **Reconcile the integration queue.** Resolve PR #12's current conflicts against `master`, preserve its Windows-runtime and stable-SurrealDB work, and prove the result on the actual combined tree before requesting merge. Assess PR #11 and the newer Warden Cloud Run branch independently; a passing demo CI run does not establish a Keeper release.
2. **Harden release truth.** Establish one supported binary/runtime version, package source or binaries with exact hashes and supported-platform labels, verify clean install, backup/export, restore, update, restart and rollback. No formal GitHub Release exists yet.
3. **Expand testing honestly.** CI currently covers Ubuntu Rust checks while setting `LIGHTING_SKIP_INTEGRATION_TESTS=1`. Keep quick checks cheap, then perform real datastore/integration tests, Windows and other claimed-platform tests, and hostile fault/recovery/security campaigns through the existing [Terror Bat](https://github.com/matthewjameswatkins1978-cyber/The-Terror-Bats) framework coordinated by Gary/Pi. Terror Bat worktrees are not security sandboxes; preserve receipts and use true isolation for risky scenarios.
4. **Verify security and user boundaries.** Preserve provenance, corrections, authority denial and write/read separation. For any future hosted service: explicit identity/tenant isolation, protected remote MCP exposure, transport authentication, throttles, quotas, operational kill switch, measured costs and no unbounded free traffic. Self-hosted software and a shared public ChatGPT endpoint have different cost/operational contracts.
5. **Close the ordinary ChatGPT Chat integration gate.** Remote MCP Inspector/Codex/Work success is supportive only. A fresh ordinary Chat must actually discover and call `lantern_status`, `lantern_search`, `lantern_context`, `lantern_why` against live canonical Lantern evidence. Until then, the native Chat connector is **not accepted** and `lantern-git` fallback remains.
6. **Public distribution after explicit review.** Preserve existing `MIT OR Apache-2.0` licence while evaluating optional support/donations, trademark/hosting terms and any deliberate future licensing change. Published plugin hosting, privacy/terms, developer verification and user onboarding are separate decisions requiring human approval.

See [CURRENT.md](../CURRENT.md) for the corresponding dated repository audit. This section records requirements, **not proof that these gates have passed**.

Lantern Keeper's central architecture is now proven well enough for external developers to clone and use from source. The next phase is less about proving the basic idea and more about packaging, generalisation, retrieval quality and broader integration.

## Landed on `master`

### Epistemic memory

- Source / Episode / Claim / Belief separation.
- Soft Memory Items for useful non-factual material.
- Perspective, provenance, predicate and scope handling.
- Durable reconciliation, corrections, immutable revisions and lineage.
- Stale invalidation and current-versus-historical retrieval.
- Bounded deterministic Context Packs and retrieval traces.
- Candidate-only Proposal/Trace governance and bounded Foreman review.
- Logical export/restore and migration accounting.
- LanternBench behavioural acceptance coverage.
- Provenance-complete live factual capture.
- Local stdio MCP with eight bounded tools.
- Real connected-client MCP proof with restart persistence and provenance.

### Authority and effects

- Durable principals, authority grants and revocations.
- Exact capability checks, expiry and fail-closed behaviour.
- Server-owned authority IDs, timestamps and provenance.
- Canonical hash-linked execution receipts.
- Tethers authority integration.
- Verified OpenShell sandbox effect boundary.
- Live Tavily evidence -> Source/Episode -> candidate-only Nemotron path.
- M6 Trust Console showing HEARD -> THOUGHT -> AUTHORISED -> DONE.

## Next

1. **Publish a formal developer release**
   - choose the public version number;
   - create release notes;
   - produce versioned source/binary artifacts where practical;
   - document install and upgrade expectations.

2. **Finish cross-platform productisation**
   - ensure ordinary validation is shell-neutral;
   - remove remaining accidental Windows/PowerShell assumptions;
   - expand CI across supported platforms;
   - keep platform-specific tests behind explicit boundaries.

3. **Generalise remaining user-facing identity defaults**
   - make actor/holder labels deployment-configurable where they are still legacy defaults;
   - preserve historical fixtures when they are useful evidence;
   - ensure public docs and examples use generic roles.

4. **Improve retrieval**
   - exact typed/fused retrieval;
   - richer full-text and graph-backed project association;
   - bounded graph expansion;
   - optional semantic candidate retrieval without creating a second truth path.

5. **Broaden adapters and imports**
   - generic conversation/event importers;
   - additional MCP/client packaging;
   - clearer migration recipes from common memory stores.

## Later

- hosted or synchronised deployments that preserve the local-first trust model;
- multi-user/team administration;
- richer GUI and Trust Console workflows;
- recommendations and automatic observation after governance is independently qualified;
- additional sandbox/executor adapters;
- signed or externally anchored receipt options where cryptographic identity is required.

## Principles that should not change

- Evidence is append-oriented and inspectable.
- Corrections preserve history rather than rewriting it.
- Repetition, quotation or assistant echo does not create consensus.
- Soft memory remains soft until governed promotion.
- AI proposes; the configured governance boundary decides.
- The human/operator remains the final authority for their deployment.
- Retrieval is bounded and explainable.
- Knowledge does not create permission.
- Authority failure fails closed.
- Cross-platform is the default unless a target is explicitly platform-specific.
