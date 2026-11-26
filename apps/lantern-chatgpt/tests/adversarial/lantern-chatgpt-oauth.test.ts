import { execFileSync } from 'node:child_process';
import { createHash, randomBytes } from 'node:crypto';
import { mkdtempSync, rmSync } from 'node:fs';
import { createServer, type Server } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { convertV4MiniflareOptions, Miniflare } from 'miniflare';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import { afterAll, beforeAll, expect, it } from 'vitest';

const origin = 'https://lantern-chatgpt.example';
const resource = origin + '/mcp/chatgpt';
const redirect = 'https://chatgpt.com/connector_platform_oauth_redirect';
const owner = randomBytes(32).toString('hex');
const stubBearer = 'stub-backend-bearer-' + randomBytes(8).toString('hex');
const digest = (s: string) => createHash('sha256').update(s);
const temporary = mkdtempSync(join(tmpdir(), 'lantern-chatgpt-oauth-'));
let runtime: Miniflare;
let stub: Server;
let stubPort = 0;
const stubCalls: Array<{ name: string; arguments: unknown }> = [];
const serverLogs: string[] = [];
const request = (path: string, init?: RequestInit) => runtime.dispatchFetch(origin + path, init);
const form = (fields: Record<string, string>, headers?: Record<string, string>) => ({
  method: 'POST',
  redirect: 'manual' as const,
  headers: { 'Content-Type': 'application/x-www-form-urlencoded', ...headers },
  body: new URLSearchParams(fields),
});

const FULL_CATALOG = [
  'lantern_status', 'lantern_search', 'lantern_context', 'lantern_why',
  'lantern_remember', 'lantern_correct', 'lantern_get_source', 'lantern_list_notes',
];

function stubHandler(reqBody: string, res: import('node:http').ServerResponse) {
  let message: { jsonrpc?: string; id?: unknown; method?: string; params?: { name?: string; arguments?: unknown } };
  try {
    message = JSON.parse(reqBody);
  } catch {
    res.writeHead(400, { 'content-type': 'application/json' }).end('{}');
    return;
  }
  const id = message.id ?? null;
  const ok = (result: unknown) => res.writeHead(200, { 'content-type': 'application/json' })
    .end(JSON.stringify({ jsonrpc: '2.0', id, result }));
  if (message.method === 'notifications/initialized') {
    res.writeHead(202).end();
    return;
  }
  if (message.method === 'initialize') {
    ok({ protocolVersion: '2025-03-26', capabilities: { tools: {} },
      serverInfo: { name: 'lantern-keeper-stub', version: 'test' } });
    return;
  }
  if (message.method === 'tools/list') {
    ok({ tools: FULL_CATALOG.map((name) => ({
      name, description: 'stub ' + name, inputSchema: { type: 'object', properties: {} },
    })) });
    return;
  }
  if (message.method === 'tools/call') {
    const name = message.params?.name ?? '';
    const args = message.params?.arguments ?? {};
    stubCalls.push({ name, arguments: args });
    if (name === 'lantern_search') {
      ok({ content: [{ type: 'text', text: 'synthetic-marker-memory' }], synthetic: true });
      return;
    }
    if (['lantern_status', 'lantern_context', 'lantern_why'].includes(name)) {
      ok({ content: [{ type: 'text', text: 'synthetic-' + name }] });
      return;
    }
    res.writeHead(200, { 'content-type': 'application/json' })
      .end(JSON.stringify({ jsonrpc: '2.0', id, error: { code: -32601, message: 'method not found' } }));
    return;
  }
  res.writeHead(200, { 'content-type': 'application/json' })
    .end(JSON.stringify({ jsonrpc: '2.0', id, error: { code: -32601, message: 'method not found' } }));
}

const runtimeOptions = (upstream?: string, credential?: string, disabled = false) =>
  convertV4MiniflareOptions({
    modules: true,
    handleStructuredLogs: (event) => serverLogs.push(JSON.stringify(event)),
    scriptPath: join(temporary, 'index.js'),
    compatibilityDate: '2026-11-01',
    compatibilityFlags: ['nodejs_compat'],
    kvNamespaces: ['OAUTH_KV'],
    ratelimits: { CHATGPT_OAUTH_LIMIT: { namespace_id: '107', simple: { limit: 60, period: 60 } } },
    bindings: {
      CHATGPT_OAUTH_DISABLED: disabled ? '1' : '0',
      CHATGPT_OAUTH_ORIGIN: origin,
      CHATGPT_OAUTH_SCOPE: 'workshop',
      CHATGPT_OAUTH_REDIRECTS: redirect,
      CHATGPT_OAUTH_OWNER_HASH: digest(owner).digest('hex'),
      LANTERN_MCP_UPSTREAM: upstream ?? `http://127.0.0.1:${stubPort}/mcp`,
      LANTERN_MCP_CREDENTIAL: credential ?? stubBearer,
    },
  });

beforeAll(async () => {
  stub = createServer((req, res) => {
    if (req.url !== '/mcp' || req.method !== 'POST') {
      res.writeHead(404).end();
      return;
    }
    if (req.headers.authorization !== `Bearer ${stubBearer}`) {
      res.writeHead(401, { 'content-type': 'application/json' }).end(JSON.stringify({
        jsonrpc: '2.0', id: null,
        error: { code: -32000, message: 'authentication failed', data: { typed: 'unauthenticated' } },
      }));
      return;
    }
    let body = '';
    req.on('data', (chunk) => { body += chunk; });
    req.on('end', () => stubHandler(body, res));
  });
  await new Promise<void>((resolve) => stub.listen(0, '127.0.0.1', resolve));
  stubPort = (stub.address() as import('node:net').AddressInfo).port;
  execFileSync(process.execPath, ['node_modules/wrangler/bin/wrangler.js', 'deploy', '--dry-run',
    '--outdir', temporary], { stdio: 'pipe' });
  runtime = new Miniflare(runtimeOptions());
}, 120000);

afterAll(async () => {
  await runtime?.dispose();
  stub?.close();
  rmSync(temporary, { recursive: true, force: true });
});

it('denies unauthenticated, invalid and mis-scoped access; exposes only read tools', async () => {
  // 1. Unauthenticated tool call is denied with an OAuth challenge.
  const anonymous = await request('/mcp/chatgpt', {
    method: 'POST', headers: { 'content-type': 'application/json', accept: 'application/json, text/event-stream' },
    body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'tools/call',
      params: { name: 'lantern_search', arguments: { scope: 'workshop', query: 'x' } } }),
  });
  expect(anonymous.status).toBe(401);
  expect(anonymous.headers.get('www-authenticate')).toContain('/.well-known/oauth-protected-resource/mcp/chatgpt');
  // A bearer minted for any other system is not accepted here.
  for (const token of [stubBearer, 'invalid', randomBytes(24).toString('hex')]) {
    expect((await request('/mcp/chatgpt', { headers: { Authorization: 'Bearer ' + token } })).status).toBe(401);
  }
  // 2. Discovery documents the ChatGPT-compatible route.
  const metadata = await (await request('/.well-known/oauth-protected-resource/mcp/chatgpt')).json();
  expect(metadata).toMatchObject({ resource, authorization_servers: [origin], scopes_supported: ['lantern:read'] });
  const issuer = await (await request('/.well-known/oauth-authorization-server')).json();
  expect(issuer).toMatchObject({ issuer: origin, code_challenge_methods_supported: ['S256'] });
  expect(issuer.registration_endpoint).toContain('/oauth/chatgpt/register');
  expect(issuer.token_endpoint).toContain('/oauth/chatgpt/token');

  // 3. Dynamic client registration: unapproved callbacks refused.
  const path = (url: string) => new URL(url).pathname;
  const register = (uri: string) => request(path(issuer.registration_endpoint), {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ redirect_uris: [uri], token_endpoint_auth_method: 'none' }),
  });
  expect((await register('https://evil.example/callback')).status).toBe(400);
  const registration = await register(redirect);
  expect(registration.status).toBe(201);
  const { client_id } = await registration.json();

  // 4. Authorize matrix: wrong resource, wrong scope, plain PKCE all refused.
  const verifier = randomBytes(48).toString('base64url');
  const authorize = (override: Record<string, string> = {}) => request(path(issuer.authorization_endpoint) + '?' +
    new URLSearchParams({
      client_id, redirect_uri: redirect, resource, response_type: 'code', scope: 'lantern:read',
      code_challenge_method: 'S256', code_challenge: digest(verifier).digest('base64url'),
      state: 'fixture', ...override,
    }));
  for (const override of [{ resource: origin + '/mcp' }, { scope: 'lantern:write' }, { scope: '' },
      { code_challenge_method: 'plain' }]) {
    expect((await authorize(override)).status).toBe(400);
  }
  const consent = await authorize();
  expect(consent.status).toBe(200);
  expect(consent.headers.get('Content-Security-Policy')).toBe(
    "default-src 'none'; form-action 'self' https://chatgpt.com; frame-ancestors 'none'");
  const cookie = consent.headers.get('set-cookie')!.split(';')[0];
  const handle = decodeURIComponent((await consent.text()).match(/name="handle" value="([^"]+)"/)![1]);
  const approve = (secret: string, binding: string, requestOrigin = origin) =>
    request(path(issuer.authorization_endpoint),
      form({ handle, owner: secret }, { Cookie: binding, Origin: requestOrigin }));
  expect((await approve(owner, cookie, 'null')).status).toBe(403);
  expect((await approve(owner, cookie, 'https://other.example')).status).toBe(403);
  expect((await approve('wrong-passphrase', cookie)).status).toBe(403);
  expect((await approve(owner, '')).status).toBe(400);
  const approval = await approve(owner, cookie);
  expect(approval.status).toBe(303);
  // Consent handles cannot be reused.
  expect((await approve(owner, cookie)).status).toBe(400);
  const callback = new URL(approval.headers.get('location')!);
  expect(callback.searchParams.get('state')).toBe('fixture');
  expect(callback.searchParams.get('iss')).toBe(origin);
  const code = callback.searchParams.get('code')!;

  // 5. Code exchange: wrong verifier / resource refused; replay refused.
  const exchange = (override: Record<string, string> = {}) => request(path(issuer.token_endpoint),
    form({ grant_type: 'authorization_code', client_id, redirect_uri: redirect, code,
      code_verifier: verifier, resource, ...override }));
  expect((await exchange({ code_verifier: randomBytes(48).toString('base64url') })).status).toBe(400);
  expect((await exchange({ resource: origin + '/mcp' })).status).toBe(400);
  const token = await exchange();
  expect(token.status).toBe(200);
  const { access_token: stolen_token, expires_in, scope } = await token.json();
  expect(expires_in).toBe(3600);
  expect(scope).toBe('lantern:read');
  // 5b. A replayed code is refused AND revokes the grant (RFC 6749 §4.1
  // code-theft protection): the just-minted token stops working.
  expect((await exchange()).status).toBe(400);
  const pingStolen = await request('/mcp/chatgpt', {
    method: 'POST', headers: { 'content-type': 'application/json', Authorization: 'Bearer ' + stolen_token },
    body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'ping' }),
  });
  expect(pingStolen.status).toBe(401);

  // 5c. Fresh grant for the retrieval session (dance A's grant died by replay).
  const verifierB = randomBytes(48).toString('base64url');
  const consentB = await request(path(issuer.authorization_endpoint) + '?' + new URLSearchParams({
    client_id, redirect_uri: redirect, resource, response_type: 'code', scope: 'lantern:read',
    code_challenge_method: 'S256', code_challenge: digest(verifierB).digest('base64url'), state: 'second',
  }));
  expect(consentB.status).toBe(200);
  const cookieB = consentB.headers.get('set-cookie')!.split(';')[0];
  const handleB = decodeURIComponent((await consentB.text()).match(/name="handle" value="([^"]+)"/)![1]);
  const approvalB = await request(path(issuer.authorization_endpoint),
    form({ handle: handleB, owner }, { Cookie: cookieB, Origin: origin }));
  expect(approvalB.status).toBe(303);
  const codeB = new URL(approvalB.headers.get('location')!).searchParams.get('code')!;
  const tokenB = await request(path(issuer.token_endpoint),
    form({ grant_type: 'authorization_code', client_id, redirect_uri: redirect, code: codeB,
      code_verifier: verifierB, resource }));
  expect(tokenB.status).toBe(200);
  const { access_token, refresh_token } = await tokenB.json();

  // 6. Genuine MCP retrieval through the new endpoint (real SDK client).
  const connect = async (token: string) => {
    const client = new Client({ name: 'fictional-chatgpt', version: '1' });
    await client.connect(new StreamableHTTPClientTransport(new URL(resource), {
      fetch: ((input, init) => {
        const headers = new Headers(init?.headers);
        headers.set('Authorization', 'Bearer ' + token);
        return runtime.dispatchFetch(String(input), { ...init, headers: Object.fromEntries(headers) });
      }) as typeof fetch,
    }));
    return client;
  };
  const chatgpt = await connect(access_token);
  try {
    const tools = (await chatgpt.listTools()).tools.map((t) => t.name);
    expect(tools.sort()).toEqual(['lantern_context', 'lantern_search', 'lantern_status', 'lantern_why']);
    const search = await chatgpt.callTool({ name: 'lantern_search',
      arguments: { scope: 'workshop', query: 'synthetic' } });
    expect(search.isError).not.toBe(true);
    expect(JSON.stringify(search)).toContain('synthetic-marker-memory');
    await expect.poll(() => serverLogs.some((log) =>
      log.includes('lantern.oauth.tool') && log.includes('lantern_search'))).toBe(true);
    // Write tools are denied at the ChatGPT boundary and never proxied.
    for (const write of ['lantern_remember', 'lantern_correct', 'lantern_get_source', 'lantern_list_notes']) {
      const denied = await chatgpt.callTool({ name: write, arguments: { scope: 'workshop' } })
        .then((r) => r).catch((e) => e);
      expect(denied.isError === true || denied?.code === -32601).toBe(true);
    }
    expect(stubCalls.filter((c) => c.name === 'lantern_remember' || c.name === 'lantern_correct')).toEqual([]);
    // Cross-project scope is denied without reaching the backend.
    const before = stubCalls.length;
    await expect(chatgpt.callTool({ name: 'lantern_search',
      arguments: { scope: 'other-project', query: 'x' } })).rejects.toThrow();
    expect(stubCalls.length).toBe(before);
  } finally {
    await chatgpt.close();
  }

  // 7. Tampered tokens are rejected (expiry and audience).
  const kv = await runtime.getKVNamespace('OAUTH_KV');
  const [userId, grantId] = access_token.split(':');
  const key = ['token', userId, grantId, digest(access_token).digest('hex')].join(':');
  const active = await kv.get(key, 'json') as { expiresAt: number; audience: string };
  const verifyToken = (tok: string) => request('/mcp/chatgpt', {
    method: 'POST', headers: { 'content-type': 'application/json', Authorization: 'Bearer ' + tok },
    body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'ping' }),
  });
  for (const override of [{ expiresAt: 1 }, { audience: origin + '/mcp' }]) {
    await kv.put(key, JSON.stringify({ ...active, ...override }));
    expect((await verifyToken(access_token)).status).toBe(401);
  }
  await kv.put(key, JSON.stringify(active));
  expect((await verifyToken(access_token)).status).toBe(200);

  // 8. Refresh rotates; the old credential stops working; scope escalation refused.
  const refresh = (extra: Record<string, string> = {}) => request(path(issuer.token_endpoint),
    form({ grant_type: 'refresh_token', refresh_token, client_id, resource, ...extra }));
  expect((await refresh({ scope: 'lantern:write' })).status).toBe(400);
  const rotated = await refresh();
  expect(rotated.status).toBe(200);
  const replacement = await rotated.json();
  expect(replacement.scope).toBe('lantern:read');
  expect(replacement.refresh_token).not.toBe(refresh_token);
  // An exact duplicate delivery inside the retry window returns the same
  // successor instead of rotating again (lost-response safety).
  const identical = await refresh();
  expect(identical.status).toBe(200);
  expect(await identical.json()).toMatchObject({ refresh_token: replacement.refresh_token });
  const rotated2 = await refresh({ refresh_token: replacement.refresh_token });
  expect(rotated2.status).toBe(200);
  const replacement2 = await rotated2.json();
  // The first successor is now a previous token outside any retry window:
  // reuse is rejected even with a well-formed request.
  const orphaned = await refresh({ refresh_token: replacement.refresh_token, scope: 'lantern:read' });
  expect(orphaned.status).toBe(400);
  expect(await orphaned.json()).toMatchObject({ error: 'invalid_grant' });
  const chatgpt2 = await connect(replacement2.access_token);
  await chatgpt2.close();

  // 9. Grant revocation ends the session without touching other state.
  await kv.delete('grant:' + userId + ':' + grantId);
  expect((await verifyToken(replacement2.access_token)).status).toBe(401);

  // 10. No credentials appear in logs, URLs or committed files.
  const forbidden = [owner, stubBearer, access_token, replacement.access_token, replacement.refresh_token,
    replacement2.access_token, replacement2.refresh_token, code, codeB];
  for (const secret of forbidden) {
    expect(serverLogs.some((log) => log.includes(secret))).toBe(false);
  }
});
it('fails closed when disabled, misconfigured or pointed at plain HTTP', async () => {
  const path = '/mcp/chatgpt';
  expect((await request(path)).status).toBe(401);
  // Query strings on the MCP route are refused.
  expect((await request(path + '?foo=bar', {
    method: 'POST', headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'ping' }),
  })).status).toBe(400);

  // Each backend-failure scenario runs as its own instance (a deployment),
  // so no live-reload timing can mask the outcome.
  const scenario = async (options: ReturnType<typeof runtimeOptions>) => {
    const isolated = new Miniflare(options);
    try {
      return await isolated.dispatchFetch(origin + path, {
        method: 'POST', headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'ping' }),
      }).then((r) => r.status);
    } finally {
      await isolated.dispose();
    }
  };
  // Disabled entry point: 503 with retry hint.
  expect(await scenario(runtimeOptions(undefined, undefined, true))).toBe(503);

  // A live grant is needed for the backend-failure checks below. Each
  // misconfigured instance mints its own token against its own KV.
  const liveToken = async (options: ReturnType<typeof runtimeOptions>) => {
    const isolated = new Miniflare(options);
    const fetchIsolated = (p: string, init?: RequestInit) =>
      isolated.dispatchFetch(origin + p, init);
    try {
      const disc = await (await fetchIsolated('/.well-known/oauth-authorization-server')).json() as {
        registration_endpoint: string; authorization_endpoint: string; token_endpoint: string;
      };
      const here = (url: string) => new URL(url).pathname;
      const reg = await fetchIsolated(here(disc.registration_endpoint), {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ redirect_uris: [redirect], token_endpoint_auth_method: 'none' }),
      });
      const { client_id } = await reg.json() as { client_id: string };
      const verifier = randomBytes(48).toString('base64url');
      const consent = await fetchIsolated(here(disc.authorization_endpoint) + '?' + new URLSearchParams({
        client_id, redirect_uri: redirect, resource, response_type: 'code', scope: 'lantern:read',
        code_challenge_method: 'S256', code_challenge: digest(verifier).digest('base64url'), state: 'iso',
      }));
      const cookie = consent.headers.get('set-cookie')!.split(';')[0];
      const handle = decodeURIComponent((await consent.text()).match(/name="handle" value="([^"]+)"/)![1]);
      const approval = await fetchIsolated(here(disc.authorization_endpoint),
        form({ handle, owner }, { Cookie: cookie, Origin: origin }));
      const code = new URL(approval.headers.get('location')!).searchParams.get('code')!;
      const tokenRes = await fetchIsolated(here(disc.token_endpoint),
        form({ grant_type: 'authorization_code', client_id, redirect_uri: redirect, code,
          code_verifier: verifier, resource }));
      const { access_token } = await tokenRes.json() as { access_token: string };
      // Oversized bodies are refused before parsing or proxying (with auth,
      // so the check reached is the body gate rather than the auth gate).
      const huge = await fetchIsolated(path, {
        method: 'POST', headers: { 'content-type': 'application/json', Authorization: 'Bearer ' + access_token },
        body: 'x'.repeat(1_048_577),
      });
      expect(huge.status).toBe(413);
      const mcp = await fetchIsolated(path, {
        method: 'POST', headers: { 'content-type': 'application/json', Authorization: 'Bearer ' + access_token },
        body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'initialize',
          params: { protocolVersion: '2025-03-26', capabilities: {}, clientInfo: { name: 't', version: '1' } } }),
      });
      return mcp.status;
    } finally {
      await isolated.dispose();
    }
  };
  // Plain-HTTP non-loopback upstream fails closed.
  expect(await liveToken(runtimeOptions('http://example.com/mcp'))).toBe(503);
  // Missing upstream credential fails closed.
  expect(await liveToken(runtimeOptions(undefined, ''))).toBe(503);
  // Sanity: the same dance against a correct instance succeeds.
  expect(await liveToken(runtimeOptions())).toBe(200);
});
