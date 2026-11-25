//! Pure protocol/auth logic for the hosted Lantern MCP worker (LK-CUTOVER-001 C2).
//!
//! No I/O, no worker types: everything here is unit-testable natively and
//! compiled into the wasm worker as the single dispatch authority. The
//! worker executes statements; this crate decides *what* may run.
//!
//! Trust rules:
//! * authentication is a SHA-256 credential match over a configured JSON
//!   file; missing/malformed configuration fails closed (deny);
//! * the storage namespace is the normalised scope alone — the principal
//!   never enters the partition key, so two principals granted the same
//!   scope share one namespace (shared workshop visibility);
//! * principal enters only receipt attribution and idempotency identity;
//! * read/write policy comes from the credential's permission list.

use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use lighting_store_d1::BatchRefusal;

/// Server-side MCP protocol version the worker speaks.
pub const PROTOCOL_VERSION: &str = "2026-07-28";
pub const SERVER_NAME: &str = "lantern-keeper-mcp";
pub const SERVER_VERSION: &str = "0.1.0";

/// Normalised scope type shared with the native engine.
pub type Scope = std::collections::BTreeMap<String, String>;

// ---------------------------------------------------------------------------
// Credentials
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CredentialsFile {
    pub credentials: Vec<CredentialEntry>,
}

#[derive(Debug, Deserialize)]
pub struct CredentialEntry {
    /// Lower- or upper-case hex SHA-256 of the bearer token. The raw token
    /// is never stored, echoed, or logged.
    pub token_sha256: String,
    pub principal: String,
    /// Scope objects (e.g. `{"project":"smoke"}`), normalised at load.
    pub scopes: Vec<Scope>,
    pub permissions: Vec<String>,
}

/// An authenticated caller. Scope authorisation uses `scopes` alone;
/// `principal` is attribution only.
#[derive(Debug, Clone)]
pub struct Principal {
    pub principal: String,
    pub scopes: Vec<Scope>,
    pub can_read: bool,
    pub can_write: bool,
}

/// Parse the credential secret. Strict: an entry with an unknown permission
/// or blank identity invalidates the whole configuration (fail closed), so
/// a typo cannot silently widen or narrow access.
pub fn load_credentials(json_text: &str) -> Result<Vec<CredentialEntry>, CoreError> {
    let file: CredentialsFile = serde_json::from_str(json_text)
        .map_err(|error| CoreError::Unconfigured(format!("credentials JSON: {error}")))?;
    if file.credentials.is_empty() {
        return Err(CoreError::Unconfigured("no credentials configured".into()));
    }
    for entry in &file.credentials {
        if entry.token_sha256.len() != 64
            || !entry.token_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(CoreError::Unconfigured(
                "token_sha256 must be 64 hex chars".into(),
            ));
        }
        if entry.principal.trim().is_empty() || entry.scopes.is_empty() {
            return Err(CoreError::Unconfigured(
                "credential needs principal and at least one scope".into(),
            ));
        }
        for permission in &entry.permissions {
            if permission != "read" && permission != "write" {
                return Err(CoreError::Unconfigured(format!(
                    "unknown permission '{permission}'"
                )));
            }
        }
    }
    Ok(file.credentials)
}

/// Authenticate a presented bearer token against loaded credentials.
/// Constant-flow-ish comparison: compare digests of equal length only.
pub fn authenticate(entries: &[CredentialEntry], presented: &str) -> Result<Principal, CoreError> {
    if presented.is_empty() {
        return Err(CoreError::Auth);
    }
    let digest = hex_digest(presented.as_bytes());
    let mut matched: Option<&CredentialEntry> = None;
    for entry in entries {
        let stored = entry.token_sha256.to_ascii_lowercase();
        // Compare always, ignore result until the end: avoids early exit.
        let equal = stored.len() == digest.len()
            && stored
                .as_bytes()
                .iter()
                .zip(digest.as_bytes())
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0;
        if equal {
            matched = Some(entry);
        }
    }
    let entry = matched.ok_or(CoreError::Auth)?;
    Ok(Principal {
        principal: entry.principal.clone(),
        scopes: entry
            .scopes
            .iter()
            .map(|scope| lighting_core::normalize_scope(scope).unwrap_or_else(|_| scope.clone()))
            .collect(),
        can_read: entry.permissions.iter().any(|p| p == "read"),
        can_write: entry.permissions.iter().any(|p| p == "write"),
    })
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Status,
    Search,
    Context,
    Why,
    Correct,
    Remember,
    GetSource,
    ListNotes,
}

impl Tool {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "lantern_status" => Some(Self::Status),
            "lantern_search" => Some(Self::Search),
            "lantern_context" => Some(Self::Context),
            "lantern_why" => Some(Self::Why),
            "lantern_correct" => Some(Self::Correct),
            "lantern_remember" => Some(Self::Remember),
            "lantern_get_source" => Some(Self::GetSource),
            "lantern_list_notes" => Some(Self::ListNotes),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Status => "lantern_status",
            Self::Search => "lantern_search",
            Self::Context => "lantern_context",
            Self::Why => "lantern_why",
            Self::Correct => "lantern_correct",
            Self::Remember => "lantern_remember",
            Self::GetSource => "lantern_get_source",
            Self::ListNotes => "lantern_list_notes",
        }
    }

    /// Writes are keyed mutations (correction, remember); reads persist nothing.
    pub fn is_write(self) -> bool {
        matches!(self, Self::Correct | Self::Remember)
    }
}

/// tools/list catalogue. Every belief-bearing read returns `belief_id` and
/// `current_revision` so a follow-up correction has its target and token.
pub fn tool_catalog() -> Value {
    let scope_schema = json!({
        "type": "object",
        "description": "Exact normalised scope; must be listed in your credential.",
        "additionalProperties": { "type": "string" }
    });
    json!({
        "tools": [
            {
                "name": "lantern_status",
                "description": "Server and schema status. Read-only, zero writes.",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "lantern_search",
                "description": "Scoped bounded search over beliefs. Read-only.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "scope": scope_schema,
                        "query": { "type": "string" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 128 }
                    },
                    "required": ["scope", "query"]
                }
            },
            {
                "name": "lantern_context",
                "description": "Bounded scoped context pack. Read-only, zero writes.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "scope": scope_schema,
                        "query": { "type": "string" },
                        "item_budget": { "type": "integer", "minimum": 1, "maximum": 32 },
                        "token_budget": { "type": "integer", "minimum": 128, "maximum": 8000 }
                    },
                    "required": ["scope", "query"]
                }
            },
            {
                "name": "lantern_why",
                "description": "Belief detail with lineage and current_revision for the next correction.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "scope": scope_schema,
                        "belief_id": { "type": "string" }
                    },
                    "required": ["scope", "belief_id"]
                }
            },
            {
                "name": "lantern_correct",
                "description": "Keyed correction with expected_revision (CAS) and idempotency_key. Write.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "scope": scope_schema,
                        "belief_id": { "type": "string" },
                        "expected_revision": { "type": "string" },
                        "new_value": { "type": "string" },
                        "idempotency_key": { "type": "string" }
                    },
                    "required": ["scope", "belief_id", "expected_revision", "new_value", "idempotency_key"]
                }
            },
            {
                "name": "lantern_remember",
                "description": "Save an ordinary note (byte-exact) with idempotency_key. A note is NEVER a verified claim. Write.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "scope": scope_schema,
                        "content": { "type": "string" },
                        "idempotency_key": { "type": "string" }
                    },
                    "required": ["scope", "content", "idempotency_key"]
                }
            },
            {
                "name": "lantern_get_source",
                "description": "Exact source bytes by id, scope-confined. Read-only.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "scope": scope_schema,
                        "source_id": { "type": "string" }
                    },
                    "required": ["scope", "source_id"]
                }
            },
            {
                "name": "lantern_list_notes",
                "description": "Bounded scoped note list, newest first. Read-only, zero writes.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "scope": scope_schema,
                        "limit": { "type": "integer", "minimum": 1, "maximum": 128 }
                    },
                    "required": ["scope"]
                }
            }
        ]
    })
}

// ---------------------------------------------------------------------------
// Authorisation
// ---------------------------------------------------------------------------

/// Authorise a tool call: permission bit first, then exact normalised scope
/// membership. A scope the credential does not list fails closed (403).
pub fn authorize(
    principal: &Principal,
    tool: Tool,
    scope: Option<&Scope>,
) -> Result<(), CoreError> {
    if let Some(scope) = scope {
        let normalised = lighting_core::normalize_scope(scope)
            .map_err(|error| CoreError::Invalid(format!("scope: {error}")))?;
        if !principal
            .scopes
            .iter()
            .any(|granted| granted == &normalised)
        {
            return Err(CoreError::Forbidden(
                "scope not granted to this credential".into(),
            ));
        }
    }
    if tool.is_write() && !principal.can_write {
        return Err(CoreError::Forbidden(
            "credential lacks write permission".into(),
        ));
    }
    if !tool.is_write() && !principal.can_read {
        return Err(CoreError::Forbidden(
            "credential lacks read permission".into(),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// JSON-RPC envelope
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum ClientMsg {
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
    },
}

pub fn parse_client_msg(body: &str) -> Result<ClientMsg, CoreError> {
    let value: Value = serde_json::from_str(body).map_err(|_| CoreError::Rpc {
        code: -32700,
        message: "parse error",
    })?;
    let method = value
        .get("method")
        .and_then(Value::as_str)
        .ok_or(CoreError::Rpc {
            code: -32600,
            message: "invalid request",
        })?
        .to_owned();
    let params = value.get("params").cloned().unwrap_or(json!({}));
    match value.get("id") {
        None | Some(Value::Null) => Ok(ClientMsg::Notification { method }),
        Some(id) => Ok(ClientMsg::Request {
            id: id.clone(),
            method,
            params,
        }),
    }
}

pub fn rpc_result(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

pub fn rpc_error(id: &Value, code: i32, message: &str, typed: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message, "data": { "typed": typed } }
    })
}

// ---------------------------------------------------------------------------
// Tool arguments
// ---------------------------------------------------------------------------

fn require_scope(params: &Value) -> Result<Scope, CoreError> {
    let raw = params
        .get("scope")
        .ok_or_else(|| CoreError::Invalid("scope is required".into()))?;
    let object = raw
        .as_object()
        .ok_or_else(|| CoreError::Invalid("scope must be an object".into()))?;
    if object.is_empty() {
        return Err(CoreError::Invalid("scope must not be empty".into()));
    }
    let mut scope = Scope::new();
    for (key, value) in object {
        let value = value
            .as_str()
            .ok_or_else(|| CoreError::Invalid("scope values must be strings".into()))?;
        scope.insert(key.clone(), value.to_owned());
    }
    lighting_core::normalize_scope(&scope)
        .map_err(|error| CoreError::Invalid(format!("scope: {error}")))
}

fn require_text(params: &Value, field: &str) -> Result<String, CoreError> {
    let value = params
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CoreError::Invalid(format!("{field} must be a non-empty string")))?;
    Ok(value.to_owned())
}

#[derive(Debug)]
pub struct SearchArgs {
    pub scope: Scope,
    pub query: String,
    pub limit: usize,
}

pub fn parse_search(params: &Value) -> Result<SearchArgs, CoreError> {
    let query = require_text(params, "query")?;
    if query.len() > 512 {
        return Err(CoreError::Invalid("query capped at 512 chars".into()));
    }
    let limit = params
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(64)
        .clamp(1, 128);
    Ok(SearchArgs {
        scope: require_scope(params)?,
        query,
        limit,
    })
}

#[derive(Debug)]
pub struct ContextArgs {
    pub scope: Scope,
    pub query: String,
    pub item_budget: usize,
    pub token_budget: usize,
}

pub fn parse_context(params: &Value) -> Result<ContextArgs, CoreError> {
    let query = require_text(params, "query")?;
    if query.len() > 512 {
        return Err(CoreError::Invalid("query capped at 512 chars".into()));
    }
    Ok(ContextArgs {
        scope: require_scope(params)?,
        query,
        item_budget: params
            .get("item_budget")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .unwrap_or(16)
            .clamp(1, 32),
        token_budget: params
            .get("token_budget")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .unwrap_or(2_000)
            .clamp(128, 8_000),
    })
}

#[derive(Debug)]
pub struct WhyArgs {
    pub scope: Scope,
    pub belief_id: String,
}

pub fn parse_why(params: &Value) -> Result<WhyArgs, CoreError> {
    Ok(WhyArgs {
        scope: require_scope(params)?,
        belief_id: require_text(params, "belief_id")?,
    })
}

#[derive(Debug)]
pub struct CorrectArgs {
    pub scope: Scope,
    pub belief_id: String,
    pub expected_revision: String,
    pub new_value: String,
    pub idempotency_key: String,
}

pub fn parse_correct(params: &Value) -> Result<CorrectArgs, CoreError> {
    let key = require_text(params, "idempotency_key")?;
    if key.len() > 128 || key.contains('|') {
        return Err(CoreError::Invalid(
            "idempotency_key max 128 chars, no '|'".into(),
        ));
    }
    Ok(CorrectArgs {
        scope: require_scope(params)?,
        belief_id: require_text(params, "belief_id")?,
        expected_revision: require_text(params, "expected_revision")?,
        new_value: require_text(params, "new_value")?,
        idempotency_key: key,
    })
}

#[derive(Debug)]
pub struct RememberArgs {
    pub scope: Scope,
    pub content: String,
    pub idempotency_key: String,
}

pub fn parse_remember(params: &Value) -> Result<RememberArgs, CoreError> {
    let key = require_text(params, "idempotency_key")?;
    if key.len() > 128 || key.contains('|') {
        return Err(CoreError::Invalid(
            "idempotency_key max 128 chars, no '|'".into(),
        ));
    }
    let content = require_text(params, "content")?;
    if content.trim().is_empty() {
        return Err(CoreError::Invalid("content must not be blank".into()));
    }
    if content.len() > 1_048_576 {
        return Err(CoreError::Invalid("content max 1 MiB".into()));
    }
    Ok(RememberArgs {
        scope: require_scope(params)?,
        content,
        idempotency_key: key,
    })
}

#[derive(Debug)]
pub struct GetSourceArgs {
    pub scope: Scope,
    pub source_id: String,
}

#[derive(Debug)]
pub struct ListNotesArgs {
    pub scope: Scope,
    pub limit: usize,
}

pub fn parse_list_notes(params: &Value) -> Result<ListNotesArgs, CoreError> {
    let limit = match params.get("limit") {
        None | Some(Value::Null) => 32,
        Some(Value::Number(n)) => n.as_u64().unwrap_or(32) as usize,
        Some(_) => return Err(CoreError::Invalid("limit must be a number".into())),
    };
    if limit == 0 || limit > 128 {
        return Err(CoreError::Invalid("limit must be 1..=128".into()));
    }
    Ok(ListNotesArgs {
        scope: require_scope(params)?,
        limit,
    })
}

pub fn parse_get_source(params: &Value) -> Result<GetSourceArgs, CoreError> {
    Ok(GetSourceArgs {
        scope: require_scope(params)?,
        source_id: require_text(params, "source_id")?,
    })
}

// ---------------------------------------------------------------------------
// Errors and HTTP mapping
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// Authentication failed or credentials not configured: fail closed.
    Auth,
    Unconfigured(String),
    Forbidden(String),
    NotFound,
    Invalid(String),
    RevisionConflict,
    IdempotencyConflict,
    Storage(String),
    Rpc {
        code: i32,
        message: &'static str,
    },
}

impl CoreError {
    /// Stable machine tag surfaced in error payloads.
    pub fn typed(&self) -> &'static str {
        match self {
            Self::Auth | Self::Unconfigured(_) => "unauthenticated",
            Self::Forbidden(_) => "forbidden",
            Self::NotFound => "not_found",
            Self::Invalid(_) => "invalid",
            Self::RevisionConflict => "revision_conflict",
            Self::IdempotencyConflict => "idempotency_conflict",
            Self::Storage(_) => "storage",
            Self::Rpc { .. } => "protocol",
        }
    }

    pub fn http_status(&self) -> u16 {
        match self {
            Self::Auth | Self::Unconfigured(_) => 401,
            Self::Forbidden(_) => 403,
            Self::NotFound => 404,
            Self::Invalid(_) => 400,
            Self::RevisionConflict | Self::IdempotencyConflict => 409,
            Self::Storage(_) => 500,
            Self::Rpc { code, .. } => match code {
                -32700 | -32600 => 400,
                -32601 => 404,
                _ => 500,
            },
        }
    }

    pub fn rpc_code(&self) -> i32 {
        match self {
            Self::RevisionConflict => -32009,
            Self::IdempotencyConflict => -32010,
            Self::Forbidden(_) => -32002,
            Self::Invalid(_) => -32602,
            Self::Rpc { code, .. } => *code,
            _ => -32000,
        }
    }
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auth | Self::Unconfigured(_) => write!(f, "authentication failed"),
            Self::Forbidden(reason) => write!(f, "forbidden: {reason}"),
            Self::NotFound => write!(f, "not found"),
            Self::Invalid(reason) => write!(f, "invalid: {reason}"),
            Self::RevisionConflict => write!(f, "expected_revision conflict"),
            Self::IdempotencyConflict => write!(f, "idempotency key conflict"),
            Self::Storage(reason) => write!(f, "storage error: {reason}"),
            Self::Rpc { message, .. } => write!(f, "{message}"),
        }
    }
}

/// Map a builder refusal onto the typed error contract.
pub fn from_batch_refusal(refusal: BatchRefusal) -> CoreError {
    CoreError::Invalid(refusal.to_string())
}

/// Classify a D1 batch failure. The CAS trigger aborts with
/// `revision_conflict`; UNIQUE guards name their table. Anything else is a
/// storage fault and never a success.
pub fn classify_d1_error(message: &str) -> CoreError {
    let lower = message.to_ascii_lowercase();
    if lower.contains("revision_conflict") {
        return CoreError::RevisionConflict;
    }
    if lower.contains("belief_cas_expected") {
        return CoreError::RevisionConflict;
    }
    if lower.contains("unique constraint failed") && lower.contains("belief_revision") {
        return CoreError::RevisionConflict;
    }
    if lower.contains("unique constraint failed")
        && (lower.contains("correction_idempotency") || lower.contains("idempotency_binding"))
    {
        return CoreError::IdempotencyConflict;
    }
    if lower.contains("unique constraint failed")
        && (lower.contains("remember_idempotency") || lower.contains("remember_binding"))
    {
        return CoreError::IdempotencyConflict;
    }
    if lower.contains("unique constraint failed") && lower.contains("mutation_receipt") {
        return CoreError::IdempotencyConflict;
    }
    CoreError::Storage("statement failed".into())
}

// ---------------------------------------------------------------------------
// Hashing, timestamps, deterministic ids
// ---------------------------------------------------------------------------

pub fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn sha256_hex(text: &str) -> String {
    hex_digest(text.as_bytes())
}

/// Canonical hosted timestamp from epoch milliseconds (RFC3339 UTC,
/// nanosecond precision, trailing Z — exactly 30 chars).
pub fn canonical_from_millis(epoch_ms: i64) -> String {
    let days = epoch_ms.div_euclid(86_400_000);
    let millis = epoch_ms.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    let hours = millis / 3_600_000;
    let minutes = (millis % 3_600_000) / 60_000;
    let seconds = (millis % 60_000) / 1_000;
    let frac = millis % 1_000;
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}.{frac:03}000000Z")
}

/// Howard Hinnant's civil_from_days (proleptic Gregorian).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

/// New belief token strictly after `expected` (canonical string order is
/// chronological). Guarantees the no-op-advance refusal never fires from a
/// same-millisecond second write.
pub fn advance_token(expected: &str, epoch_ms: i64) -> String {
    let mut ms = epoch_ms;
    loop {
        let candidate = canonical_from_millis(ms);
        if candidate.as_str() > expected {
            return candidate;
        }
        ms += 1;
    }
}

/// Payload hash covers the envelope version and every CAS-relevant field.
pub fn payload_hash(
    envelope_version: &str,
    composite_key: &str,
    belief_id: &str,
    expected_revision: &str,
    new_value: &str,
) -> String {
    sha256_hex(&format!(
        "{envelope_version}|{composite_key}|{belief_id}|{expected_revision}|{new_value}"
    ))
}

/// Deterministic receipt/revision ids: same accepted write -> same id, so a
/// replay collides on the primary key instead of minting a duplicate.
pub fn deterministic_id(prefix: &str, composite_key: &str, payload_hash: &str) -> String {
    sha256_hex(&format!("{prefix}|{composite_key}|{payload_hash}"))
        .chars()
        .take(32)
        .collect()
}

/// Merge a new value into an existing belief payload JSON. Objects keep
/// their fields; `current_value` and `value` are set. Non-object payloads
/// are replaced by a minimal envelope (documented behaviour).
pub fn merge_belief_payload(existing: &str, new_value: &str) -> String {
    let mut value: Value = serde_json::from_str(existing).unwrap_or(json!({}));
    match value.as_object_mut() {
        Some(object) => {
            object.insert("current_value".into(), json!(new_value));
            object.insert("value".into(), json!(new_value));
        }
        None => {
            value = json!({ "current_value": new_value, "value": new_value });
        }
    }
    value.to_string()
}

/// Escape SQLite LIKE wildcards in caller text so user input matches
/// literally; the worker wraps the result in `%...%`.
pub fn escape_like(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Simple token-overlap relevance score for the hosted context tool. This is
/// NOT native compile_select parity; it is a bounded, honest subset.
pub fn context_score(payload: &str, query: &str) -> u32 {
    let haystack = payload.to_ascii_lowercase();
    query
        .to_ascii_lowercase()
        .split_whitespace()
        .map(|token| {
            if token.len() >= 2 && haystack.contains(token) {
                1
            } else {
                0
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(token: &str, principal: &str, perms: &[&str], scope: &[(&str, &str)]) -> Value {
        let map: serde_json::Map<String, Value> = scope
            .iter()
            .map(|(key, value)| (key.to_string(), json!(value)))
            .collect();
        json!({
            "token_sha256": sha256_hex(token),
            "principal": principal,
            "scopes": [map],
            "permissions": perms
        })
    }

    fn two_principals() -> Vec<CredentialEntry> {
        let wrapped = json!({
            "credentials": [
                entry("token-pi", "pi", &["read", "write"], &[("project", "smoke")]),
                entry("token-codex", "codex", &["read"], &[("project", "smoke")])
            ]
        })
        .to_string();
        load_credentials(&wrapped).expect("credentials")
    }

    #[test]
    fn authenticate_matches_and_fails_closed() {
        let entries = two_principals();
        let pi = authenticate(&entries, "token-pi").expect("pi authenticates");
        assert_eq!(pi.principal, "pi");
        assert!(pi.can_write && pi.can_read);
        assert!(authenticate(&entries, "token-guess").is_err());
        assert!(authenticate(&entries, "").is_err());
    }

    #[test]
    fn shared_scope_visibility_and_denied_scope() {
        let entries = two_principals();
        let pi = authenticate(&entries, "token-pi").expect("pi");
        let codex = authenticate(&entries, "token-codex").expect("codex");
        let shared: Scope = [("project".to_owned(), "smoke".to_owned())]
            .into_iter()
            .collect();
        let denied: Scope = [("project".to_owned(), "other".to_owned())]
            .into_iter()
            .collect();

        // Both principals authorised for the same scope: one namespace.
        authorize(&pi, Tool::Search, Some(&shared)).expect("pi reads shared");
        authorize(&codex, Tool::Search, Some(&shared)).expect("codex reads shared");
        authorize(&pi, Tool::Correct, Some(&shared)).expect("pi writes shared");

        // Denials fail closed.
        assert!(matches!(
            authorize(&pi, Tool::Search, Some(&denied)),
            Err(CoreError::Forbidden(_))
        ));
        // Codex lacks write permission even on its granted scope.
        assert!(matches!(
            authorize(&codex, Tool::Correct, Some(&shared)),
            Err(CoreError::Forbidden(_))
        ));
        // Write tool without scope is still permission-gated.
        assert!(matches!(
            authorize(&codex, Tool::Correct, None),
            Err(CoreError::Forbidden(_))
        ));
    }

    #[test]
    fn malformed_credentials_fail_closed() {
        assert!(load_credentials("not json").is_err());
        assert!(load_credentials(r#"{"credentials":[]}"#).is_err());
        assert!(load_credentials(
            r#"{"credentials":[{"token_sha256":"zz","principal":"x","scopes":[{"a":"b"}],"permissions":["read"]}]}"#
        )
        .is_err());
        assert!(load_credentials(
            r#"{"credentials":[{"token_sha256":"0000000000000000000000000000000000000000000000000000000000000000","principal":"x","scopes":[{"a":"b"}],"permissions":["admin"]}]}"#
        )
        .is_err());
    }

    #[test]
    fn rpc_parse_request_and_notification() {
        let request =
            parse_client_msg(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#)
                .expect("request");
        match request {
            ClientMsg::Request { id, method, .. } => {
                assert_eq!(id, json!(1));
                assert_eq!(method, "initialize");
            }
            other => panic!("expected request, got {other:?}"),
        }
        let notification =
            parse_client_msg(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .expect("notification");
        assert!(matches!(notification, ClientMsg::Notification { .. }));
        assert!(matches!(
            parse_client_msg("{broken"),
            Err(CoreError::Rpc { code: -32700, .. })
        ));
    }

    #[test]
    fn remember_rejects_blank_oversize_and_piped_key() {
        let scope = json!({"project": "smoke"});
        let bad_blank = json!({"scope": scope, "content": "  ", "idempotency_key": "k"});
        assert!(parse_remember(&bad_blank).is_err());
        let bad_big =
            json!({"scope": scope, "content": "x".repeat(2_000_000), "idempotency_key": "k"});
        assert!(parse_remember(&bad_big).is_err());
        let bad_pipe = json!({"scope": scope, "content": "ok", "idempotency_key": "a|b"});
        assert!(parse_remember(&bad_pipe).is_err());
        let good = json!({"scope": scope, "content": "hi", "idempotency_key": "k"});
        let args = parse_remember(&good).expect("valid remember");
        assert_eq!(args.content, "hi");
    }

    #[test]
    fn get_source_and_list_notes_parse_and_bound() {
        let scope = json!({"project": "smoke"});
        let get = json!({"scope": scope, "source_id": "abc"});
        assert_eq!(parse_get_source(&get).expect("valid").source_id, "abc");
        let list = json!({"scope": scope});
        assert_eq!(parse_list_notes(&list).expect("valid").limit, 32);
        let zero = json!({"scope": scope, "limit": 0});
        assert!(parse_list_notes(&zero).is_err());
        let huge = json!({"scope": scope, "limit": 999});
        assert!(parse_list_notes(&huge).is_err());
    }

    #[test]
    fn catalog_lists_eight_tools() {
        let catalog = tool_catalog();
        let tools = catalog["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 8);
        for tool in tools {
            let name = tool["name"].as_str().expect("name");
            assert!(Tool::from_name(name).is_some(), "unknown tool {name}");
            assert!(tool.get("inputSchema").is_some(), "{name} needs schema");
        }
    }

    #[test]
    fn http_statuses_follow_contract() {
        assert_eq!(CoreError::Auth.http_status(), 401);
        assert_eq!(CoreError::Forbidden("x".into()).http_status(), 403);
        assert_eq!(CoreError::NotFound.http_status(), 404);
        assert_eq!(CoreError::Invalid("x".into()).http_status(), 400);
        assert_eq!(CoreError::RevisionConflict.http_status(), 409);
        assert_eq!(CoreError::IdempotencyConflict.http_status(), 409);
        assert_eq!(CoreError::Storage("x".into()).http_status(), 500);
    }

    #[test]
    fn d1_error_classification() {
        assert_eq!(
            classify_d1_error("SqliteError: RAISE(ABORT, revision_conflict)"),
            CoreError::RevisionConflict
        );
        assert_eq!(
            classify_d1_error("UNIQUE constraint failed: belief_revision.belief_id"),
            CoreError::RevisionConflict
        );
        assert_eq!(
            classify_d1_error("UNIQUE constraint failed: correction_idempotency.key"),
            CoreError::IdempotencyConflict
        );
        assert!(matches!(
            classify_d1_error("disk I/O error"),
            CoreError::Storage(_)
        ));
    }

    #[test]
    fn canonical_timestamps_are_30_chars_and_ordered() {
        let epoch = canonical_from_millis(0);
        assert_eq!(epoch, "1970-01-01T00:00:00.000000000Z");
        assert_eq!(epoch.len(), 30);
        assert!(lighting_store_d1::is_canonical_timestamp(&epoch));
        let later = canonical_from_millis(1_767_225_600_000); // 2026-01-01T00:00:00Z
        assert_eq!(later, "2026-01-01T00:00:00.000000000Z");
        assert!(lighting_store_d1::is_canonical_timestamp(&later));
        assert!(later > epoch);
    }

    #[test]
    fn advance_token_is_strictly_greater_even_same_millisecond() {
        let t0 = canonical_from_millis(1_767_225_600_000);
        let next = advance_token(&t0, 1_767_225_600_000);
        assert!(next > t0, "same-ms advance must bump: {next} vs {t0}");
        assert!(lighting_store_d1::is_canonical_timestamp(&next));
        let far = advance_token(&t0, 1_767_325_600_000);
        assert!(far > t0);
    }

    #[test]
    fn payload_hash_and_ids_are_deterministic_and_bounded() {
        let composite = lighting_store_d1::idempotency_composite_key("pi", "sc", "k1");
        let hash = payload_hash(
            "lantern-keeper-write-envelope-v1",
            &composite,
            "b1",
            "r1",
            "v1",
        );
        assert_eq!(
            hash,
            payload_hash(
                "lantern-keeper-write-envelope-v1",
                &composite,
                "b1",
                "r1",
                "v1"
            )
        );
        assert_ne!(
            hash,
            payload_hash(
                "lantern-keeper-write-envelope-v1",
                &composite,
                "b1",
                "r1",
                "v2"
            )
        );
        let receipt = deterministic_id("receipt", &composite, &hash);
        assert_eq!(receipt.len(), 32);
        assert_eq!(receipt, deterministic_id("receipt", &composite, &hash));
        assert_ne!(receipt, deterministic_id("revision", &composite, &hash));
    }

    #[test]
    fn merge_payload_sets_current_value_and_preserves_fields() {
        let merged = merge_belief_payload(r#"{"subject_key":"s","current_value":"old"}"#, "new");
        let value: Value = serde_json::from_str(&merged).expect("json");
        assert_eq!(value["current_value"], "new");
        assert_eq!(value["value"], "new");
        assert_eq!(value["subject_key"], "s");
        let replaced = merge_belief_payload("not json", "new");
        assert!(replaced.contains("new"));
    }

    #[test]
    fn context_score_ranks_matching_payload_higher() {
        assert!(
            context_score("the lantern workshop notes", "lantern workshop")
                > context_score("unrelated", "lantern workshop")
        );
        assert_eq!(context_score("anything", "x"), 0);
    }

    #[test]
    fn escape_like_neutralises_wildcards() {
        assert_eq!(escape_like("50%_done\\x"), "50\\%\\_done\\\\x");
        assert_eq!(escape_like("plain"), "plain");
    }

    #[test]
    fn correct_args_validation() {
        let params = json!({
            "scope": {"project": "smoke"},
            "belief_id": "b1",
            "expected_revision": "2026-01-01T00:00:00.000000000Z",
            "new_value": "v",
            "idempotency_key": "k1"
        });
        let args = parse_correct(&params).expect("parses");
        assert_eq!(args.belief_id, "b1");

        let mut bad = params.clone();
        bad["idempotency_key"] = json!("a|b");
        assert!(matches!(parse_correct(&bad), Err(CoreError::Invalid(_))));

        let mut missing = params.clone();
        missing.as_object_mut().unwrap().remove("expected_revision");
        assert!(matches!(
            parse_correct(&missing),
            Err(CoreError::Invalid(_))
        ));
    }
}
