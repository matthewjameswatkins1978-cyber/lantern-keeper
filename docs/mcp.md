# MCP and Lucy Integration

The current supported machine-facing boundaries are the local HTTP API used by
the CLI and the `lighting mcp` stdio bridge over that same API. JSON responses
and the stable API error shape are the working contract. The optional Tethers
preview is separate and does not write Lantern memory.

Start Lighting locally, then run `lighting mcp` with stdin/stdout connected to
the Lucy client. The bridge exposes only `lantern_context`, `lantern_remember`,
`lantern_search`, `lantern_why`, `lantern_correct`, `lantern_status`,
`lantern_foreman_queue`, and `lantern_foreman_review`. It rejects unknown tool
arguments and never exposes raw database mutation.

`lantern_remember` accepts either a factual Claim or a supported soft Memory
Item. The supported kinds are `claim`, `quote`, `idea`, `fragment`,
`impression`, `anecdote`, `creative_seed`, `pattern_candidate`,
`strength_observation`, `lesson`, `open_loop`, `tension`, `rejected_path`,
`negative_constraint`, `reference`, `humour`, and `other`. There is no
`memory` kind. Unsupported kinds are rejected before any service call.

For `kind: "claim"`, `predicate_key` and `evidence_text` are required.
`content` is the normalized Claim value; `evidence_text` is preserved exactly
as the immutable Source content. Lantern automatically creates or reuses the
whole-text Source and Episode, links their IDs and UTF-8 byte span to the
Claim, and reconciles it into a Belief. Clients do not supply Source or
Episode IDs. Unknown predicates remain unmapped and are not silently promoted.

`lantern_why` accepts exactly one of `belief_id`, `memory_id`, or `query`. A
belief ID returns its recorded Claim and revision lineage. A memory ID returns
the memory item and its recorded source/episode/actor provenance. A natural
query uses deterministic belief search and returns an explanation only when it
has one unambiguous match; multiple matches are returned as an ambiguity result
without choosing one.

The command-line equivalent for recording a correction is
`lighting correction record <belief-id> <correction-text> <replacement-value>`.
The MCP correction tool also accepts `target_query` when it resolves to exactly
one active belief; ambiguous and empty queries are rejected without mutation.

A real Codex Desktop client has now completed the connected MCP lifecycle,
including factual capture, correction, restart persistence, and provenance.
The bridge is local and delegates to the existing service, so it must not be
bound as unauthenticated remote write infrastructure. This Codex proof is
distinct from the still-separate ChatGPT/Lucy product-access gate.

Do not claim Lucy integration is complete until a real supported client has
retrieved context, captured a soft memory without filing instructions, searched
it, explained provenance, corrected a harmless test belief, and retrieved the
corrected state after restart.
