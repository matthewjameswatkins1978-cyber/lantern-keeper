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

A real Lucy-native client proof remains a cutover gate. The bridge is local and
delegates to the existing service, so it must not be bound as unauthenticated
remote write infrastructure.

Do not claim Lucy integration is complete until a real supported client has
retrieved context, captured a soft memory without filing instructions, searched
it, explained provenance, corrected a harmless test belief, and retrieved the
corrected state after restart.
