import type { KVNamespace } from '@cloudflare/workers-types';

/**
 * Minimal KV-backed record store for the refresh-rotation wrapper.
 * Immediate writes (no staging/commit): adequate without the Durable Object
 * serialisation layer; the residual concurrent-rotation race is documented
 * in LK-CHAT-001 follow-ups (port of Switchboard SWB-007 remains optional).
 */
export class KVRecords {
  constructor(private readonly kv: KVNamespace) {}

  async get(name: string, options?: 'text' | 'json' | { type?: 'text' | 'json' }): Promise<unknown> {
    // A revoked grant must fence its access tokens even if a caller holds
    // a direct token key.
    if (name.startsWith('token:')) {
      const [, user, grant] = name.split(':');
      if ((await this.kv.get('grant:' + user + ':' + grant)) === null) return null;
    }
    const value = await this.kv.get(name);
    if (value === null) return null;
    const type = typeof options === 'string' ? options : options?.type;
    return type === 'json' ? JSON.parse(value) : value;
  }

  async put(name: string, value: string, options?: { expirationTtl?: number }): Promise<void> {
    await this.kv.put(name, value, options?.expirationTtl === undefined ? undefined : {
      expirationTtl: options.expirationTtl,
    });
  }

  async delete(name: string): Promise<void> {
    await this.kv.delete(name);
  }

  commit(): void {
    // Immediate KV writes need no commit step.
  }
}

/**
 * Grant-fenced KV view for the OAuth provider itself. The base provider
 * validates access tokens against their token record only; substituting
 * this view makes grant deletion (revocation) immediately fence every
 * outstanding access token of that grant — the same guarantee Switchboard
 * gets from its OAuthRecords layer. All writes pass straight through.
 */
export function fencedKV(kv: KVNamespace): KVNamespace {
  const get = async (name: string, options?: unknown): Promise<unknown> => {
    if (name.startsWith('token:')) {
      const [, user, grant] = name.split(':');
      if ((await kv.get('grant:' + user + ':' + grant)) === null) return null;
    }
    return kv.get(name, options as never);
  };
  return {
    get: get as KVNamespace['get'],
    put: ((name: string, value: string, options?: unknown) =>
      kv.put(name, value as never, options as never)) as KVNamespace['put'],
    delete: ((name: string) => kv.delete(name)) as KVNamespace['delete'],
    list: ((options?: unknown) => kv.list(options as never)) as KVNamespace['list'],
    getWithMetadata: ((name: string, options?: unknown) =>
      kv.getWithMetadata(name, options as never)) as KVNamespace['getWithMetadata'],
  };
}
