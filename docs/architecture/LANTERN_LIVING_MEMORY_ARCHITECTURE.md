# Lantern Living Memory Architecture

Status: **canonical current architecture**

Lantern Keeper is a local-first memory and trust layer for humans and AI. Sources preserve what happened; Claims preserve what was asserted; Beliefs preserve current reconciled understanding; and soft Memory Items preserve useful material that is not ready to become factual truth.

The architecture is generic. Historical fixtures may contain the identities used while Lantern was being developed, but human, assistant, agent, shared and legacy holders are deployment roles rather than hard requirements of the model.

## The epistemic chain

```text
Source  ->  Episode  ->  Claim  ->  Belief
 evidence    event       assertion   current projection

                    +-> Memory Item
                    |   soft possibility
                    +-> Trace / Proposal
                        governance explanation
```

Sources and Episodes are evidence. Claims are immutable assertions that point back to evidence and retain perspective. Beliefs are projections that may be reconciled, superseded, invalidated or marked stale without rewriting the evidence. Memory Items are deliberately softer and need not become Claims.

## Governing constitution

> Evidence first. Interpretation second. Authority separately.
>
> Remember generously. Assert cautiously. Act only with explicit authority.
>
> Authorship, transmission, endorsement and belief ownership are separate.

The architecture does not infer consensus from repetition, silence, quotation or model output. A generated narrative or Context Pack is a disposable view and never becomes evidence merely because it was generated.

## Perspective

A retained assertion may distinguish:

- originator;
- speaker;
- transmitter;
- holder;
- stance;
- transformation/framing path.

This is what prevents a quoted sentence, assistant repetition or migrated note from silently becoming “the user's belief”.

## Correction model

Corrections are new evidence, not edits to history.

A correction can:

1. add a new Source/Episode;
2. add a new Claim;
3. supersede or invalidate the current Belief projection;
4. mark dependent projections stale;
5. retain the prior lineage for historical inspection.

Current retrieval and historical retrieval are therefore intentionally different questions.

## Soft memory

Ideas, fragments, creative seeds, patterns, lessons, jokes, tensions, references and open loops may be worth remembering without being factual Claims.

This is an important architectural boundary. A useful thought does not need to be promoted into truth merely to remain retrievable.

## Context Compiler

Context Packs are bounded read projections over memory and evidence. They can carry:

- stable IDs;
- holder/perspective sections;
- current/historical separation;
- Source and Episode references;
- candidate scores and selection reasons;
- selected and omitted IDs;
- item/token budgets;
- persisted retrieval traces.

A Context Pack is not evidence and must not be fed back into Lantern as self-citation.

## Governance and candidates

Extractors, Dreamer, semantic matchers and external models may produce candidate Claims, soft memories, associations, contradictions or corrections. They do not directly mutate canonical Beliefs.

A configured Foreman/review boundary can accept, reject, modify or defer proposals while recording the decision in a Trace.

## Authority is a separate chain

Lantern Keeper also contains an independent authority model:

```text
Principal -> AuthorityGrant -> policy check -> effect boundary -> Receipt
```

This chain answers a different question from memory. A Belief can influence reasoning but cannot create or revoke permission.

The Lantern Warden demonstration profile combines Lantern authority, Tethers policy and OpenShell execution to prove this separation at a real effect boundary.

## Storage and recovery

The domain model remains independent of SurrealDB. The normal local adapter uses embedded, versioned SurrealKV and keeps an engine-independent logical export for recovery and migration.

The service binds locally by default. Remote access, hosted memory and raw database mutation are not implicit parts of the architecture.
