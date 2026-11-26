# LK-CHAT-001 Native ChatGPT OAuth — staging deployment

Target MCP URL: `https://<chatgpt-worker-host>/mcp/chatgpt`
(where `<chatgpt-worker-host>` is the deployed `lantern-keeper-chatgpt`
Worker hostname, e.g. `lantern-keeper-chatgpt.<account>.workers.dev`).

After the OAuth-enabled candidate is deployed, open ChatGPT Plugins,
select **+ → Add custom MCP server**, and enter these settings.

| Field | Value |
| --- | --- |
| Name | Lantern Keeper (private) |
| Server URL | The target MCP URL above |
| Authentication | OAuth |
| OAuth client | Dynamic client registration |
| Client ID and secret | Leave blank for dynamic registration |
| Scope | `lantern:read` if the form asks |

Select **Create as a plugin**, keep it personal, then install and connect it.
Verify that the OAuth approval page is on the exact Worker hostname above.
Enter the owner passphrase there and choose **Verify and approve**. Obtain the
passphrase through the owner's password manager.

The authorization server returns `iss` on successful redirects and advertises
RFC 9207 support. The expected ChatGPT callback is
`https://chatgpt.com/connector_platform_oauth_redirect`. Confirm the callback
on the connection's management page. If the UI shows a different callback, use
the exact HTTPS URL shown. The stable callback is shared by ChatGPT
connections. Only approve a flow you started from your own personal plugin.
The Worker uses an exact allowlist. See
[OpenAI plugin authentication](https://developers.openai.com/plugins/build/auth)
and [plugin connection](https://developers.openai.com/plugins/deploy/connect-chatgpt).

## Deploy

Use a **new** Worker (`lantern-keeper-chatgpt`). Do not attach this route to
the existing `lantern-keeper-mcp` Worker: bearer clients there are untouched
and rollback stays a single-worker operation.

1. Create the KV namespace and note its ID:
   `wrangler kv namespace create OAUTH_KV` (plus the `_preview_id` for local
   `wrangler dev` if needed).
2. Choose an unused account-local numeric namespace ID for the ratelimit
   binding (`CHATGPT_OAUTH_LIMIT`, 60 per 60 seconds per route).
3. Prepare a private `wrangler.jsonc` (or `--var` / `--secret` flags) with:
   - `CHATGPT_OAUTH_ORIGIN`: the new Worker HTTPS origin, no trailing slash.
   - `CHATGPT_OAUTH_SCOPE_JSON`: the single workshop scope object the
     backend requires, e.g. `{"project":"workshop"}`. The gateway injects
     exactly this normalized object into every scoped tool call; a client
     scope that normalizes differently is denied without backend contact.
   - `CHATGPT_OAUTH_REDIRECTS`: comma-separated exact approved callbacks.
   - `CHATGPT_OAUTH_OWNER_HASH`: Worker secret holding the lowercase SHA-256
     digest of a locally generated, uniformly random owner passphrase of at
     least 32 bytes, encoded before hashing. Keep the passphrase in the
     owner's password manager.
   - `OAUTH_KV`: the KV binding from step 1.
   - `LANTERN_MCP_UPSTREAM`: `https://lantern-keeper-mcp.<account>.workers.dev/mcp`.
   - `LANTERN_MCP_CREDENTIAL`: Worker secret holding one **read-only,
     single-scope** bearer token. Provision it as a dedicated entry in the
     existing `LANTERN_MCP_CREDENTIALS` set with `can_read: true`,
     `can_write: false`, and scopes limited to the single workshop scope
     object above. The gateway allowlist already blocks every write tool,
     but the credential must not be able to exceed read-only workshop
     access even if the proxy were bypassed. Never log it.
   - `CHATGPT_OAUTH_KV_PROPAGATION_NOTE`: revocation and grant deletion
     propagate at Workers KV speed; allow for edge propagation delay before
     treating a revocation as complete (staging uses disposable synthetic
     credentials, so this is not a production revocation path yet).
4. `wrangler deploy`. Record the version ID.
5. Qualification (staging): anonymous 401 with `resource_metadata` challenge;
   discovery documents; unapproved redirect refused; wrong resource/scope or
   plain PKCE refused; owner approval over the exact origin only; code replay
   refused AND revokes the grant (RFC 6749 §4.1); expired/tampered tokens 401;
   refresh rotates single-use with a two-minute exact-retry window;
   `tools/list` shows exactly `lantern_status`, `lantern_search`,
   `lantern_context`, `lantern_why`; write tools denied without backend
   contact; cross-scope calls denied (including same-letters-different-case
   and malformed scope objects); absent scopes injected with the exact
   configured object; misconfigured scope binding fails closed; a fresh
   ordinary ChatGPT conversation invokes `lantern_search` and the Worker
   emits the `lantern.oauth.tool` receipt, correlated by tool name and
   timestamp (±2 minutes — the gateway receipt ID is server-side only and
   the upstream result is passed through verbatim); no credentials in logs
   (run the local suite: `npm test` in `apps/lantern-chatgpt`).

The OAuth token grants read-only memory access in the configured scope only.
Access tokens expire after one hour; refresh grants after seven days. OAuth
grants cannot mint static bearer credentials. Existing bearer clients
continue to use the untouched `/mcp` Worker.

Keep request/invocation logging off to avoid retaining OAuth URLs. A live log
tail or Workers Logs configured for console events only can observe the
application receipts during a controlled acceptance run.

If staging fails qualification, use `wrangler rollback <prior-version-id>
--name lantern-keeper-chatgpt` (or delete the Worker). Verify the OAuth
route is gone and legacy bearer MCP is unaffected. Choose a new owner hash
before later re-enabling OAuth if all previous grants should be invalidated.
Removing an allowed callback also disables grants for that callback. Allow
for Workers KV propagation on edge revocation.

## Acceptance (ordinary ChatGPT, fresh conversation)

Explicitly select the personal plugin and ask it to call `lantern_status`
and `lantern_search` with a bounded fictional query. Ask it to return the
Lantern receipt IDs without quoting memory content. Match each tool name to
an independent `lantern.oauth.tool` Worker console event. Repeat in a fresh
Work conversation with a separate set of receipt IDs. Keep message content,
token material and OAuth URLs out of the evidence. A receipt on a read
confirms the Worker handled that call.

Local runtime coverage is in
`apps/lantern-chatgpt/tests/adversarial/lantern-chatgpt-oauth.test.ts`.
The test builds the bundled Worker and exercises the real MCP SDK over
Streamable HTTP against a stub backend. Native Chat and Work acceptance are
separate from this test.

## Rollback

`wrangler rollback <prior-version-id> --name lantern-keeper-chatgpt`, or
`wrangler delete --name lantern-keeper-chatgpt` if the route should not
exist at all. Verify: the `/mcp/chatgpt` URL goes away (404/DNS) and the
existing `lantern-keeper-mcp` bearer `/mcp` still serves authorised clients.
No D1 migration is involved; no state is shared between the workers.
