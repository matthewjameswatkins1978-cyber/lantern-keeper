# Lantern Keeper Roadmap

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
