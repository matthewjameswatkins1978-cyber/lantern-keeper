# Lantern Inquiry v0.1

Status: experimental design for `exp/inquiry-lite`.

## Objective

Add a bounded inquiry plane without replacing Lantern's evidence, memory, governance, or authority planes. Inquiry records explain how interpretations change; they do not become canonical Beliefs.

## Reusable infrastructure

- Sources preserve original bytes and immutable revision chains.
- Episodes and Claims retain evidence spans and perspective/provenance.
- Beliefs and BeliefRevision records already implement current projections plus additive history.
- GraphRelation links, Trace records, Proposal/Foreman review, and bounded Context Packs provide lineage, audit, and retrieval primitives.
- Dreamer output is already candidate-only and rejects authority-shaped responses.

## Narrow typed addition

Add `Inquiry` and `InquiryRevision` domain records.

An Inquiry contains the question, competing Hypotheses, linked EvidenceLinks, current unresolved questions, and the current revision identity. A Hypothesis is explicitly separate from a Claim or Belief and has a kind (`empirical`, `provisional`, or `creative_heuristic`) and an ambiguity-safe status (`active`, `challenged`, `supported`, `refuted`, `rejected`, or `superseded`). It has no numerical confidence.

An EvidenceLink stores Source/Claim identifiers, a role (`inspires`, `supports`, `contradicts`, or `corroborates`), and a provenance class. Only distinct original Source identities count as independent corroboration. A repeated source, derived restatement, or model interpretation is retained but never counted as independent evidence.

An InquiryRevision is append-only. It records the previous revision, the changed hypothesis state, linked evidence, lessons, and a Trace identity. Updating an interpretation creates a revision; it never deletes the old hypothesis or source lineage.

## Bounded operation

The service exposes create, observe, retrieve-history, and next-question operations.

Observation ingestion captures exact supplied text as Source/Episode evidence before appending an InquiryRevision. Linking an existing Claim is reserved for a follow-up route so claim identity validation remains explicit.

Next-question generation is deterministic and bounded. It returns:

- unknowns and unresolved assumptions;
- the most relevant competing explanations;
- a candidate observation that would distinguish them;
- the smallest useful test or investigation;
- a framing warning when the question may be ill-posed.

Suggestions are data, not commands. No operation executes a proposed test.

## Dreamer and Foreman boundary

A Dreamer candidate may be wrapped as an `inquiry_hypothesis` Proposal after validation against the supplied evidence. Foreman acceptance appends a hypothesis revision only; it cannot create or change a canonical Belief or authority record. Rejection remains retrievable as negative knowledge. Existing candidate-only and authority-separation invariants remain unchanged.

## Persistence and compatibility

Add `inquiry` and `inquiry_revision` tables through an additive `IF NOT EXISTS` migration. Store typed payloads as JSON, use a transaction for projection + revision + trace, and include both tables in logical export/restore. Existing tables, routes, and production data are not migrated or rewritten.

## First experiment

The freeze case starts with CPU exhaustion and lock contention hypotheses. A low-CPU observation challenges CPU exhaustion without deleting it. The next-question result proposes thread dumps as the cheapest distinguishing observation. A later observation can support, refute, or qualify either explanation.

A cross-domain fixture uses competing explanations for a delayed plant growth observation. The same invariants must hold outside debugging.

## Rejected alternatives

- MemoryItem-only storage: useful for ideas, but lacks typed hypothesis lifecycle and revision atomicity.
- Proposal payload as the durable inquiry store: governance metadata would be present, but ordinary retrieval and historical lineage would be opaque.
- New autonomous agents: existing Dreamer/Foreman boundaries are sufficient.
- Numerical confidence or fixed evidence budgets: they would create unsupported precision and obscure provenance.
- A parallel graph/database: duplicates existing semantic authority and weakens export/recovery.

## Acceptance evidence

Deterministic Rust tests must prove source preservation, competing hypotheses, contradiction without deletion, revision history, independent-evidence deduplication, hostile text remaining data, and no canonical/authority mutation. Integration tests must prove persistence, restart retrieval, export inclusion, and Foreman candidate handling. A small identical-fixture comparison must report retrieval-only versus inquiry-enabled behaviour, token/latency/memory observations, and limitations without treating unit tests as proof of better reasoning.
