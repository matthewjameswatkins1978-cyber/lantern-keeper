# Dreamer and Foreman

Status: **canonical current architecture — implementation partial**

Dreamer and extractors may produce candidate Claims, soft-memory candidates,
associations, contradictions, and correction suggestions. They cannot mutate
canonical Beliefs directly.

Lucy is the routine Memory Foreman. Foreman review is a bounded governance
operation over a Proposal queue, with an append-only Trace for the decision.
Possible outcomes include accept, reject, modify, and defer; none should erase
the evidence that led to the proposal.

Automatic work remains bounded and lazy after eager stale invalidation. There is
no autonomous swarm, hidden second curator, or inferred consensus authority.
The Dreamer lane is deliberately candidate-only until its inputs, review rules,
and replay behaviour are separately qualified.
