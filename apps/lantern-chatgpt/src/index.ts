import type { ExecutionContext } from '@cloudflare/workers-types';
import {
  TOKEN_PATH,
  isReadTool,
  routeLanternChatGptOAuth,
  type LanternChatGptOAuthEnv,
  type LanternPrincipal,
} from './lantern-chatgpt-oauth.js';
import { configuredScope, parseScopeObject, scopeEquals, type ScopeObject } from './scope.js';
import { KVRecords, fencedKV } from './oauth-records.js';
import { rotateLanternOAuthToken } from './oauth-refresh.js';

export interface Env extends LanternChatGptOAuthEnv {
  // Base URL of the existing bearer-authenticated Lantern MCP endpoint,
  // e.g. https://lantern-keeper-mcp.<account>.workers.dev/mcp
  LANTERN_MCP_UPSTREAM?: string;
  // Server-side bearer credential for the upstream (a read-scoped entry
  // from the existing LANTERN_MCP_CREDENTIALS set). Never logged, never
  // forwarded to or from the OAuth client.
  LANTERN_MCP_CREDENTIAL?: string;
}

const READ_METHODS = new Set(['initialize', 'ping', 'tools/list', 'tools/call']);

function rpcError(id: unknown, code: number, message: string) {
  return Response.json({ jsonrpc: '2.0', id: id ?? null, error: { code, message } });
}

// MCP protocol headers forwarded both ways (allowlist). Everything else is
// dropped: the backend must never see client auth, and the client must never
// see backend internals beyond the MCP stream.
const FORWARD_REQUEST_HEADERS = ['mcp-protocol-version', 'mcp-session-id'];
const FORWARD_RESPONSE_HEADERS = ['mcp-session-id'];

async function proxyUpstream(env: Env, body: string, incoming?: Headers): Promise<Response> {
  const upstream = env.LANTERN_MCP_UPSTREAM;
  const credential = env.LANTERN_MCP_CREDENTIAL;
  if (!upstream || !credential) return new Response(null, { status: 503 });
  let url: URL;
  try {
    url = new URL(upstream);
  } catch {
    return new Response(null, { status: 503 });
  }
  // Production upstream is always HTTPS. Plain HTTP is accepted only for
  // loopback hosts so tests can run a local stub backend; anything else
  // fails closed. There is an adversarial test pinning this boundary.
  const loopback = url.hostname === '127.0.0.1' || url.hostname === '::1' || url.hostname.toLowerCase() === 'localhost';
  if (url.protocol !== 'https:' && !(url.protocol === 'http:' && loopback)) {
    return new Response(null, { status: 503 });
  }
  const headers = new Headers();
  headers.set('content-type', 'application/json');
  headers.set('accept', 'application/json, text/event-stream');
  headers.set('authorization', `Bearer ${credential}`);
  if (incoming) {
    for (const name of FORWARD_REQUEST_HEADERS) {
      const value = incoming.get(name);
      if (value !== null) headers.set(name, value);
    }
  }
  // Never forward the client's OAuth token upstream or log either token.
  const upstreamResponse = await fetch(url.toString(), { method: 'POST', headers, body });
  // Preserve the real upstream content type: this gateway constrains its own
  // filtering to JSON, but must not mislabel an SSE (or other) stream as
  // JSON. The body streams through untouched.
  const passthrough = new Headers();
  passthrough.set('content-type', upstreamResponse.headers.get('content-type') ?? 'application/json');
  passthrough.set('cache-control', 'no-store');
  for (const name of FORWARD_RESPONSE_HEADERS) {
    const value = upstreamResponse.headers.get(name);
    if (value !== null) passthrough.set(name, value);
  }
  return new Response(upstreamResponse.body, {
    status: upstreamResponse.status,
    headers: passthrough,
  });
}

async function serveChatGpt(request: Request, env: Env, _auth: LanternPrincipal): Promise<Response> {
  // Streamable HTTP here is POST-only: there is no SSE listener, so GET is
  // 405 (consistent with the upstream), not a body parse error.
  if (request.method !== 'POST') {
    return new Response(null, { status: 405, headers: { allow: 'POST' } });
  }
  const body = await request.text().catch(() => '');
  // Client request bodies are tiny tool calls; refuse anything larger
  // before parsing or proxying.
  if (body.length > 1_048_576) {
    return new Response(null, { status: 413 });
  }
  let message: { id?: unknown; method?: string; params?: { protocolVersion?: unknown; name?: string; arguments?: Record<string, unknown> } };
  try {
    message = JSON.parse(body);
  } catch {
    return rpcError(null, -32700, 'Parse error');
  }
  if (typeof message !== 'object' || message === null || Array.isArray(message)) {
    return rpcError(null, -32600, 'Invalid Request');
  }
  // Notifications have no id member at all. An explicit null id is a
  // request that still expects a response.
  if (typeof message.method === 'string' && message.id === undefined) {
    return new Response(null, { status: 202 });
  }
  const { id, method } = message;
  let params = message.params;
  if (typeof method !== 'string' || !READ_METHODS.has(method)) {
    return rpcError(id, -32601, 'method not found');
  }
  if (method === 'initialize') {
    const upstream = await proxyUpstream(env, body, request.headers);
    if (!upstream.ok || !upstream.headers.get('content-type')?.includes('json')) return upstream;
    const response = await upstream.clone().json().catch(() => null) as {
      result?: { protocolVersion?: string }
    } | null;
    if (typeof response?.result?.protocolVersion !== 'string') return upstream;
    const supported = ['2025-11-25', '2025-06-18', '2025-03-26', '2024-11-05', '2024-10-07'];
    const requested = params?.protocolVersion;
    response.result.protocolVersion = typeof requested === 'string' && supported.includes(requested)
      ? requested
      : supported[0];
    await upstream.body?.cancel();
    return Response.json(response, { status: upstream.status, headers: upstream.headers });
  }
  if (method === 'tools/call') {
    const name = params?.name ?? '';
    if (!isReadTool(name)) {
      // Unknown or write tool: denied at the ChatGPT boundary, never proxied.
      console.info(JSON.stringify({
        event: 'lantern.oauth.tool', tool: name, outcome: 'denied_tool_not_allowed',
        receiptId: crypto.randomUUID(),
      }));
      return rpcError(id, -32601, 'method not found');
    }
    // Memory-scope binding: the backend requires scope as a normalized
    // object, never a string. An absent scope is injected with the exact
    // configured object; a supplied scope must normalize to exactly the
    // configured object, else the call is a cross-project attempt denied
    // here without backend contact. Malformed scopes are rejected with the
    // same -32602 code the backend uses for Invalid. lantern_status takes
    // no scope and passes through untouched.
    if (name !== 'lantern_status') {
      const configured = configuredScope(env.CHATGPT_OAUTH_SCOPE_JSON);
      if (configured === null) {
        // Misconfigured scope binding: fail closed, never proxy unscoped.
        return new Response(null, { status: 503 });
      }
      const supplied = params?.arguments?.['scope'];
      let bound: ScopeObject;
      if (supplied === undefined) {
        bound = configured;
      } else {
        let normalized: ScopeObject;
        try {
          normalized = parseScopeObject(supplied);
        } catch {
          console.info(JSON.stringify({
            event: 'lantern.oauth.tool', tool: name, outcome: 'denied_scope_malformed',
            receiptId: crypto.randomUUID(),
          }));
          return rpcError(id, -32602, 'invalid: scope must be a nonempty object of string values');
        }
        if (!scopeEquals(normalized, configured)) {
          console.info(JSON.stringify({
            event: 'lantern.oauth.tool', tool: name, outcome: 'denied_scope_mismatch',
            receiptId: crypto.randomUUID(),
          }));
          return rpcError(id, -32603, 'scope not authorized');
        }
        bound = normalized;
      }
      params = { ...params, arguments: { ...params?.arguments, scope: bound } };
    }
  }
  if (method === 'tools/list') {
    const proxied = await proxyUpstream(env, body, request.headers);
    const upstreamContentType = proxied.headers.get('content-type') ?? '';
    if (!upstreamContentType.includes('json')) {
      // Filtering requires JSON. A non-JSON upstream (e.g. an SSE stream)
      // passes through verbatim rather than being mislabeled or dropped.
      return proxied;
    }
    const text = await proxied.text().catch(() => '');
    let payload: { result?: { tools?: Array<{ name?: string; annotations?: Record<string, unknown> }> } };
    try {
      payload = JSON.parse(text);
    } catch {
      // Preserve the upstream's own error envelope instead of replacing it.
      return new Response(text, { status: proxied.status, headers: proxied.headers });
    }
    if (Array.isArray(payload?.result?.tools)) {
      payload.result.tools = payload.result.tools
        .filter((tool) => isReadTool(tool?.name ?? ''))
        .map((tool) => ({ ...tool, annotations: { ...tool.annotations, readOnlyHint: true } }));
    }
    const receiptId = crypto.randomUUID();
    console.info(JSON.stringify({ event: 'lantern.oauth.tool', tool: 'tools/list', outcome: 'success', receiptId }));
    // Rebuilt with the filtered catalogue; memory payloads stay uncacheable.
    return new Response(JSON.stringify({ jsonrpc: '2.0', id, result: payload.result }), {
      headers: {
        'content-type': 'application/json',
        'cache-control': 'no-store',
        ...(proxied.headers.get('mcp-session-id')
          ? { 'mcp-session-id': proxied.headers.get('mcp-session-id') as string }
          : {}),
      },
    });
  }
  const tool = method === 'tools/call' ? (params?.name ?? 'unknown') : method;
  const receiptId = crypto.randomUUID();
  // Proxy the rewritten message so the bound scope (not the client's
  // original arguments) is what the backend authorizes.
  const proxied = await proxyUpstream(env, JSON.stringify({ ...message, params }), request.headers);
  console.info(JSON.stringify({ event: 'lantern.oauth.tool', tool, outcome: String(proxied.status), receiptId }));
  // Non-200 upstream responses (including typed JSON-RPC errors) pass
  // through verbatim; this gateway never rewrites the error envelope.
  return proxied;
}

export default {
  async fetch(request: Request, env: Env, ctx?: ExecutionContext): Promise<Response> {
    // The provider sees grant-fenced KV: deleting a grant revokes its
    // outstanding access tokens immediately (not just at expiry).
    const fenced = env.OAUTH_KV ? { ...env, OAUTH_KV: fencedKV(env.OAUTH_KV) } : env;
    // Refresh rotation is serialized through single-use refresh tokens with
    // a two-minute exact-retry window (theft protection). This wraps only
    // the token endpoint; every other OAuth path goes straight through.
    if (new URL(request.url).pathname === TOKEN_PATH) {
      const kv = fenced.OAUTH_KV;
      if (kv) {
        const records = new KVRecords(kv);
        return rotateLanternOAuthToken(request, fenced, records, async () =>
          (await routeLanternChatGptOAuth(request, fenced, ctx, (req, auth) =>
            serveChatGpt(req, env, auth))) ?? new Response(null, { status: 404 }));
      }
    }
    const oauth = await routeLanternChatGptOAuth(request, fenced, ctx, (req, auth) =>
      serveChatGpt(req, env, auth));
    if (oauth) return oauth;
    return new Response(null, { status: 404 });
  },
};
