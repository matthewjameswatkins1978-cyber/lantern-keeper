/**
 * Scope-object binding for the ChatGPT gateway (LK-CHAT-001 review delta).
 *
 * The real Lantern worker requires `scope` to be a nonempty object of string
 * values (`require_scope` in lantern-mcp-core), normalized with the same
 * rules as the native engine (`normalize_scope` in lighting-core). A string
 * scope is rejected by the backend as Invalid. This module ports those rules
 * so the gateway binds the exact normalized configured scope object to each
 * scoped tool call instead of comparing strings the backend would refuse.
 *
 * Normalization (mirrors the Rust implementation):
 * - keys: trim; runs of non-ASCII-alphanumeric become one `_`; strip
 *   leading/trailing `_`; lowercase. Blank after trim is an error.
 * - values: trim; lowercased for `os` (with macos/windows aliases),
 *   `context`, `environment`, `location_context`, `activity`,
 *   `time_context`; otherwise kept verbatim. Blank after trim is an error.
 * - duplicate keys after normalization are an error.
 */

export type ScopeObject = Record<string, string>;

const LOWERCASE_VALUE_KEYS = new Set([
  'os', 'context', 'environment', 'location_context', 'activity', 'time_context',
]);

function normaliseKey(raw: string): string | null {
  const value = raw.trim();
  if (value === '') return null;
  let normalized = '';
  let separator = false;
  for (const character of value) {
    if (/[A-Za-z0-9]/.test(character)) {
      normalized += character.toLowerCase();
      separator = false;
    } else if (!separator) {
      normalized += '_';
      separator = true;
    }
  }
  normalized = normalized.replace(/^_+|_+$/g, '');
  return normalized === '' ? null : normalized;
}

function normaliseValue(key: string, raw: string): string | null {
  const value = raw.trim();
  if (value === '') return null;
  if (key === 'os') {
    switch (value.toLowerCase()) {
      case 'mac': case 'macos': case 'osx': return 'macos';
      case 'win': case 'windows': case 'win32': return 'windows';
      case 'linux': return 'linux';
      default: return value.toLowerCase();
    }
  }
  if (LOWERCASE_VALUE_KEYS.has(key)) return value.toLowerCase();
  return value;
}

/** Parse an unknown value into a normalized scope object. Throws on any violation. */
export function parseScopeObject(raw: unknown): ScopeObject {
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) {
    throw new Error('scope must be an object');
  }
  const entries = Object.entries(raw as Record<string, unknown>);
  if (entries.length === 0) throw new Error('scope must not be empty');
  const scope: ScopeObject = {};
  for (const [key, value] of entries) {
    if (typeof value !== 'string') throw new Error('scope values must be strings');
    const normalizedKey = normaliseKey(key);
    if (normalizedKey === null) throw new Error('scope key is blank');
    const normalizedValue = normaliseValue(normalizedKey, value);
    if (normalizedValue === null) throw new Error('scope value is blank');
    if (normalizedKey in scope) throw new Error(`duplicate key after normalization: ${normalizedKey}`);
    scope[normalizedKey] = normalizedValue;
  }
  return scope;
}

/** Order-insensitive equality on normalized scope objects. */
export function scopeEquals(a: ScopeObject, b: ScopeObject): boolean {
  const keysA = Object.keys(a);
  const keysB = Object.keys(b);
  if (keysA.length !== keysB.length) return false;
  return keysA.every((key) => b[key] === a[key]);
}

/**
 * Parse and normalize the configured scope from the environment binding.
 * Returns null when the binding is missing or malformed (fail closed).
 */
export function configuredScope(json: string | undefined): ScopeObject | null {
  if (typeof json !== 'string' || json.trim() === '') return null;
  try {
    return parseScopeObject(JSON.parse(json));
  } catch {
    return null;
  }
}
