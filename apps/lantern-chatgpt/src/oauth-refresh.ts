import { sha256Hex } from './hash.js';
import type { LanternChatGptOAuthEnv } from './lantern-chatgpt-oauth.js';
import type { KVRecords } from './oauth-records.js';

// Exact duplicate refresh deliveries within two minutes receive the same
// successor (lost-response retry). Any other reuse of a rotated-out refresh
// token is rejected: the grant may already be in an attacker's hands.
const replaySeconds = 120;
const clock = () => Math.floor(Date.now() / 1000);
const encoder = new TextEncoder();
const decoder = new TextDecoder();
const tokenFormat = /^lantern-chat:[a-zA-Z0-9_-]{1,128}:[a-zA-Z0-9_-]{1,128}$/;

type TokenResponse = {
  access_token: string;
  refresh_token: string;
  expires_in: number;
};
type Grant = { refreshTokenId?: string; previousRefreshTokenId?: string; expiresAt?: number };
type RetryRecord = { iv: string; body: string; headers: [string, string][]; expiresAt: number };

async function encryptionKey(refreshToken: string) {
  const bytes = await crypto.subtle.digest('SHA-256',
    encoder.encode('lantern refresh retry v1\0' + refreshToken));
  return crypto.subtle.importKey('raw', bytes, 'AES-GCM', false, ['encrypt', 'decrypt']);
}

function rejected() {
  console.warn(JSON.stringify({ event: 'lantern.oauth.token_error', status: 400,
    code: 'invalid_grant', category: 'refresh-token-grant', reason: 'retry_window_or_request_mismatch' }));
  return Response.json({ error: 'invalid_grant' }, { status: 400, headers: { 'Cache-Control': 'no-store' } });
}

async function readForm(request: Request) {
  const reader = request.clone().body?.getReader();
  if (!reader) return '';
  const chunks = new Uint8Array(16_384);
  let length = 0;
  try {
    for (;;) {
      const entry = await reader.read();
      if (entry.done) break;
      if (length + entry.value.byteLength > chunks.length) return null;
      chunks.set(entry.value, length);
      length += entry.value.byteLength;
    }
    return decoder.decode(chunks.subarray(0, length));
  } finally {
    // A cloned request has two consumers. Waiting on cancellation could wait
    // for the original body, which is deliberately untouched here.
    void reader.cancel().catch(() => {});
  }
}

export async function rotateLanternOAuthToken(
  request: Request,
  env: LanternChatGptOAuthEnv,
  records: KVRecords,
  issue: () => Promise<Response>,
): Promise<Response> {
  if (request.method !== 'POST' ||
      !request.headers.get('Content-Type')?.toLowerCase().startsWith('application/x-www-form-urlencoded')) {
    return issue();
  }
  const body = await readForm(request);
  if (body === null) return new Response(null, { status: 413 });
  const params = new URLSearchParams(body);
  if (new Set(params.keys()).size !== params.size) {
    return Response.json({ error: 'invalid_request' }, { status: 400, headers: { 'Cache-Control': 'no-store' } });
  }
  const incoming = params.get('refresh_token') ?? '';
  if (params.get('grant_type') !== 'refresh_token' || !tokenFormat.test(incoming)) return issue();

  const [user, grantId] = incoming.split(':');
  const grantKey = 'grant:' + user + ':' + grantId;
  // Normalize the order of form parameters without changing their values.
  params.sort();
  const digest = await sha256Hex(JSON.stringify([
    params.toString(), request.url, request.headers.get('Content-Type'),
    request.headers.get('Authorization'), request.headers.get('Origin'),
    env.CHATGPT_OAUTH_ORIGIN, env.CHATGPT_OAUTH_SCOPE, env.CHATGPT_OAUTH_REDIRECTS, env.CHATGPT_OAUTH_OWNER_HASH,
  ]));
  const retryKey = 'retry:' + digest;
  const retry = await records.get(retryKey, 'json') as RetryRecord | null;
  const grant = await records.get(grantKey, 'json') as Grant | null;
  if (retry && retry.expiresAt > clock() && grant && (grant.expiresAt ?? 0) > clock()) {
    const plaintext = await crypto.subtle.decrypt({
      name: 'AES-GCM', iv: Buffer.from(retry.iv, 'base64url'), additionalData: encoder.encode(digest),
    }, await encryptionKey(incoming), Buffer.from(retry.body, 'base64url'));
    const response = JSON.parse(decoder.decode(plaintext)) as TokenResponse;
    const accessKey = 'token:' + user + ':' + grantId + ':' + await sha256Hex(response.access_token);
    const access = await records.get(accessKey, 'json') as { expiresAt: number } | null;
    if (grant.refreshTokenId === await sha256Hex(response.refresh_token) && access && access.expiresAt > clock()) {
      response.expires_in = access.expiresAt - clock();
      return Response.json(response, { headers: retry.headers });
    }
    return rejected();
  }
  if (grant?.previousRefreshTokenId === await sha256Hex(incoming)) return rejected();

  const response = await issue();
  if (response.status !== 200) return response;
  const payload = await response.clone().text();
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const sealed = await crypto.subtle.encrypt({
    name: 'AES-GCM', iv, additionalData: encoder.encode(digest),
  }, await encryptionKey(incoming), encoder.encode(payload));
  await records.put(retryKey, JSON.stringify({
    iv: Buffer.from(iv).toString('base64url'), body: Buffer.from(sealed).toString('base64url'),
    headers: [...response.headers], expiresAt: clock() + replaySeconds,
  } satisfies RetryRecord), { expirationTtl: replaySeconds });
  return response;
}
