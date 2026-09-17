# MCP Integration

Lantern Keeper exposes a bounded local MCP surface through the `lighting mcp` stdio bridge. The bridge delegates to the same local HTTP service used by the CLI.

The design goal is deliberately narrow: an AI client gets useful memory operations without receiving raw database mutation authority.

## Start the service

```bash
cargo run -p lighting -- serve
```

Then configure the client to launch:

```bash
cargo run -p lighting -- mcp
```

The default service endpoint is `http://127.0.0.1:4317`.

## Exposed tools

The bridge exposes exactly eight bounded tools:

- `lantern_context`
- `lantern_remember`
- `lantern_search`
- `lantern_why`
- `lantern_correct`
- `lantern_status`
- `lantern_foreman_queue`
- `lantern_foreman_review`

Unknown tool arguments are rejected. The MCP client does not receive a generic SQL/database tool.

## Remembering factual and soft material

`lantern_remember` accepts either a factual Claim or a supported soft Memory Item.

Supported soft kinds include:

`quote`, `idea`, `fragment`, `impression`, `anecdote`, `creative_seed`, `pattern_candidate`, `strength_observation`, `lesson`, `open_loop`, `tension`, `rejected_path`, `negative_constraint`, `reference`, `humour`, and `other`.

There is no generic `memory` kind because Lantern deliberately distinguishes types of remembered material.

For `kind: "claim"`, `predicate_key` and `evidence_text` are required.

- `content` is the normalised Claim value.
- `evidence_text` is preserved as immutable Source content.
- Lantern creates or reuses the Source and whole-text Episode.
- The Claim receives the Source/Episode provenance and UTF-8 byte span.
- Reconciliation then produces or updates the current Belief projection.

Clients do not supply trusted Source or Episode IDs for this path. Unknown predicates remain explicitly unmapped rather than being silently promoted into a made-up schema.

## Asking why

`lantern_why` accepts exactly one of:

- `belief_id`
- `memory_id`
- `query`

A belief ID returns its Claim and revision lineage. A memory ID returns its recorded provenance. A natural query uses deterministic belief search and explains a result only when there is one unambiguous match. Multiple matches are returned as ambiguity rather than having the model quietly choose one.

## Corrections

Corrections are additive evidence events.

The command-line equivalent is:

```bash
cargo run -p lighting -- correction record <belief-id> <correction-text> <replacement-value>
```

The MCP correction tool can also accept a target query when it resolves to exactly one active belief. Empty or ambiguous queries are rejected without mutation.

The previous Source, Episode, Claim and Belief history remains inspectable after correction.

## Connected-client proof

A real Codex Desktop client has completed the full connected MCP lifecycle, including:

- factual capture;
- soft-memory capture;
- search and bounded context retrieval;
- provenance explanation;
- correction;
- current/history separation;
- restart persistence.

That proves the bridge itself with a real MCP client. Other clients can use the same stdio contract where their product exposes a compatible local MCP/process integration surface.

## Security boundary

The MCP bridge is designed for local use. Do not expose the local write service as unauthenticated remote infrastructure.

MCP memory access also does not create authority to execute arbitrary external effects. Lantern's authority/grant path remains a separate boundary.
