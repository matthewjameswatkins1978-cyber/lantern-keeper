# Dreamer and Foreman

Lucy is the sole routine Memory Foreman. Dreamer and extractors may produce
candidate Claims, soft-memory candidates, associations, contradictions, and
correction suggestions. They cannot mutate canonical Beliefs directly.

The durable Proposal record and append-only Trace provide the queue/audit
boundary. Automatic work should remain bounded and lazy after eager stale
invalidation. No autonomous swarm or second curator identity is required.
