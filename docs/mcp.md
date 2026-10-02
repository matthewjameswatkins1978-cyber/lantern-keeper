# MCP Integration

Lantern Keeper has two MCP transports over the same local HTTP service. Trusted local clients use `lighting mcp` over stdio and retain the existing eight-tool surface. Remote clients use `lighting mcp-http`, which exposes only four read tools through MCP Streamable HTTP. Both adapters call the existing service API; neither opens a database connection or creates a second store.

The remote transport uses the official [Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk), crate `rmcp` **3.5.0**, with Cargo features `server` and `transport-streamable-http-server`. The SDK is Apache-2.0 licensed, maintained in `modelcontextprotocol/rust-sdk`, and provides the current Streamable HTTP implementation as a Tower service that mounts directly in Lantern's existing Axum router. No legacy HTTP+SSE transport or additional web framework is used.

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

## Read-only remote MCP

Start the normal service in one terminal:

```bash
cargo run -p lighting -- serve
```

Start the separate MCP adapter in another:

```bash
cargo run -p lighting -- mcp-http
```

It listens at `http://127.0.0.1:4318/mcp`. The bind host must be a loopback IP address. Configure the service URL with `LIGHTING_SERVICE_URL`; configure the adapter bind with `LIGHTING_MCP_HTTP_HOST` and `LIGHTING_MCP_HTTP_PORT` (defaults `127.0.0.1` and `4318`). Startup logs report the local bind address. The main REST service remains on its own loopback endpoint at port `4317`, and its routes are not mounted on the MCP listener.

The remote catalogue contains exactly:

- `lantern_status`
- `lantern_search`
- `lantern_context`
- `lantern_why`

Each advertises `readOnlyHint: true`, `destructiveHint: false`, and `openWorldHint: false`. Calls to `lantern_remember`, `lantern_correct`, `lantern_foreman_queue`, `lantern_foreman_review`, and all other unlisted tool names fail at the MCP routing boundary. Argument validation and service-call behavior use the existing stdio implementation.

To make a temporary HTTPS development proof, first confirm a Secure MCP Tunnel is available in the actual account and product surface. If it is unavailable, an ephemeral free HTTPS tunnel such as Cloudflare Quick Tunnel can forward only the adapter:

```bash
cloudflared tunnel --no-autoupdate --url http://127.0.0.1:4318 --http-host-header 127.0.0.1:4318
```

Use the generated `https://…/mcp` URL only while that tunnel process is running. Do not forward port `4317`, open a firewall port, or treat the random HTTPS URL as durable configuration. Stop the tunnel after the proof. This is temporary evidence plumbing; durable production exposure needs a separately designed authenticated/private deployment.

For protocol verification, connect an MCP client or MCP Inspector to `http://127.0.0.1:4318/mcp`, initialize, list tools, call all four reads, and attempt one unavailable tool name and one invalid argument. Repeat against the temporary HTTPS URL while the tunnel runs. A raw curl request alone is not protocol proof.

For the shared-store check, search for the known marker through remote MCP, take the memory ID from the actual result, call `lantern_why` with that ID, then independently repeat search and provenance retrieval through `lighting mcp` or Pi's existing stdio connection. Match both ID and provenance. Restarting the adapters or Lantern service must not change the record; the durable Lantern service remains the only store.

## Exposed tools

The local stdio bridge exposes exactly eight bounded tools:

- `lantern_context`
- `lantern_remember`
- `lantern_search`
- `lantern_why`
- `lantern_correct`
- `lantern_status`
- `lantern_foreman_queue`
- `lantern_foreman_review`

Remote MCP intentionally exposes only the four read tools listed above.

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
