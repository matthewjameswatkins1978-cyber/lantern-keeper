# Dreamer and Foreman

Status: **canonical architecture; candidate path implemented, broader automation intentionally bounded**

Dreamer and extractors may produce candidate Claims, soft-memory candidates, associations, contradictions and correction suggestions. They cannot mutate canonical Beliefs directly.

The **Foreman** is a configurable bounded review role, not a specific named assistant. Foreman review operates over a Proposal queue and records an append-oriented Trace for the decision.

Possible outcomes include:

- accept;
- reject;
- modify;
- defer.

None of those outcomes should erase the evidence that led to the proposal.

## Candidate-only rule

The distinction between candidate generation and canonical state is deliberate.

A language model may be useful at noticing patterns, suggesting associations or interpreting external evidence, but model output is not proof. Candidate confidence is not evidence confidence, and repetition by multiple models is not automatic consensus.

## Live provider example

The Lantern Warden M5 path demonstrates this rule with real external providers:

```text
Tavily result -> Source/Episode evidence -> Nemotron candidate
```

The result is persisted as external evidence before model interpretation. The model can return a validated candidate, but that path cannot directly mutate canonical memory or authority.

## Automation boundary

Automatic work remains bounded and lazy after eager stale invalidation. There is no requirement for an autonomous swarm or hidden second curator.

Any future autonomous promotion path must independently qualify:

- its inputs;
- review policy;
- replay behaviour;
- failure handling;
- provenance;
- non-amplification against hostile or repeated content.
