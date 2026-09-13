# MCP and Lucy Integration

Status: **feature-line integration; connected Lucy proof still required**

The supported machine-facing boundaries are the local HTTP API used by the
CLI and the `lighting mcp` stdio bridge over that API. JSON responses and the
stable API error shape are the working contract. The optional Tethers preview
is separate and does not write Lantern memory.

Start Lighting locally, then run `lighting mcp` with stdin/stdout connected to a
supported Lucy client. The bridge exposes bounded operations for context,
remember, search, provenance/why, correction, status, and Lucy-owned Foreman
review. It rejects unknown tool arguments and never exposes raw database
mutation.

`lantern_remember` defaults to a soft Memory Item. An explicit Claim packet may
provide a registered predicate, subject, and scope; unknown predicates remain
unmapped and are not silently promoted. The unfinished provenance-complete live
factual capture path must not be described as a completed client capability
until its fresh end-to-end proof passes.

`lantern_why` accepts one explanation target at a time: belief ID, memory ID, or
an unambiguous natural query. Ambiguous queries return an ambiguity result
without choosing one. A correction target must resolve to exactly one active
belief and creates new evidence rather than rewriting the old record.

The real Lucy-native proof must retrieve context, capture a soft memory without
filing instructions, search it, explain provenance, correct a harmless test
belief, and retrieve the corrected state after restart. Until that proof passes,
the bridge is a local development surface, not unauthenticated remote write
infrastructure.
