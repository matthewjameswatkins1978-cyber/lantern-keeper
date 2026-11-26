import type { ExecutionContext } from '@cloudflare/workers-types';
import {
  TOKEN_PATH,
  isReadTool,
  routeLanternChatGptOAuth,
  type LanternChatGptOAuthEnv,
  type LanternPrincipal,
} from './lantern-chatgpt-oauth.js';
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

async function proxyUpstream(env: Env, body: string): Promise<Response> {
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
  // Never forward the client's OAuth token upstream or log either token.
  const upstreamResponse = await fetch(url.toString(), { method: 'POST', headers, body });
  const text = await upstreamResponse.text();
  return new Response(text, {
    status: upstreamResponse.status,
    headers: { 'content-type': 'application/json', 'cache-control': 'no-store' },
  });
}

async function serveChatGpt(request: Request, env: Env, _auth: LanternPrincipal): Promise<Response> {
  const body = await request.text().catch(() => '');
  // Client request bodies are tiny tool calls; refuse anything larger
  // before parsing or proxying.
  if (body.length > 1_048_576) {
    return new Response(null, { status: 413 });
  }
  let message: { id?: unknown; method?: string; params?: { name?: string; arguments?: Record<string, unknown> } };
  try {
    message = JSON.parse(body);
  } catch {
    return rpcError(null, -32700, 'Parse error');
  }
  if (typeof message !== 'object' || message === null || Array.isArray(message)) {
    return rpcError(null, -32600, 'Invalid Request');
  }
  // Notifications (including notifications/initialized) get 202, no body.
  if (typeof message.method === 'string' && (message.id === undefined || message.id === null)) {
    return new Response(null, { status: 202 });
  }
  const { id, method, params } = message;
  if (typeof method !== 'string' || !READ_METHODS.has(method)) {
    return rpcError(id, -32601, 'method not found');
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
    // Memory-scope confinement: the caller's scope argument must equal the
    // configured workshop scope. Anything else is a cross-project attempt.
    const requestedScope = params?.arguments?.['scope'];
    const allowedScope = env.CHATGPT_OAUTH_SCOPE;
    if (typeof requestedScope === 'string' && requestedScope !== allowedScope) {
      console.info(JSON.stringify({
        event: 'lantern.oauth.tool', tool: name, outcome: 'denied_scope_mismatch',
        receiptId: crypto.randomUUID(),
      }));
      return rpcError(id, -32603, 'scope not authorized');
    }
  }
  if (method === 'tools/list') {
    const proxied = await proxyUpstream(env, body);
    if (proxied.status !== 200) return proxied;
    let payload: { result?: { tools?: Array<{ name?: string }> } };
    try {
      payload = JSON.parse(await proxied.text());
    } catch {
      return rpcError(id, -32603, 'upstream error');
    }
    if (Array.isArray(payload?.result?.tools)) {
      payload.result.tools = payload.result.tools.filter((tool) => isReadTool(tool?.name ?? ''));
    }
    const receiptId = crypto.randomUUID();
    console.info(JSON.stringify({ event: 'lantern.oauth.tool', tool: 'tools/list', outcome: 'success', receiptId }));
    return Response.json({ jsonrpc: '2.0', id, result: payload.result });
  }
  const tool = method === 'tools/call' ? (params?.name ?? 'unknown') : method;
  const receiptId = crypto.randomUUID();
  const proxied = await proxyUpstream(env, body);
  console.info(JSON.stringify({ event: 'lantern.oauth.tool', tool, outcome: String(proxied.status), receiptId }));
  if (proxied.status !== 200) {
    const text = await proxied.text().catch(() => '');
    return new Response(text, { status: proxied.status, headers: { 'content-type': 'application/json' } });
  }
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
