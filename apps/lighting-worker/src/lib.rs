//! Lantern Keeper hosted MCP worker (LK-CUTOVER-001 C2).
//!
//! One deployable Worker exposing `/mcp` (Streamable HTTP, protocol
//! `2026-07-28`) backed by the `lantern-keeper` D1 database. There is no
//! legacy native write path here: every mutation is the keyed correction
//! batch from `lighting-store-d1`, executed as one D1 batch, guarded by the
//! CAS trigger + UNIQUE constraints. Reads are SELECTs only.
//!
//! Trust boundaries:
//! * bearer credentials come from the `LANTERN_MCP_CREDENTIALS` secret
//!   (SHA-256 match; missing/malformed secret fails closed);
//! * scope authorisation is exact normalised membership against the
//!   credential's granted scopes — shared workshop scope, denied fails 403;
//! * principal appears only in receipt attribution and idempotency identity,
//!   never in the storage namespace;
//! * tokens and memory content are never logged.

use lantern_mcp_core::{
    ClientMsg, CoreError, Principal, Tool, advance_token, authenticate,
    canonical_from_millis, classify_d1_error, context_score, deterministic_id, escape_like,
    from_batch_refusal, load_credentials, merge_belief_payload, parse_client_msg, parse_context,
    parse_correct, parse_get_source, parse_list_notes, parse_remember, parse_search,
    parse_why, payload_hash, remember_payload_hash, rpc_error, rpc_result, tool_catalog,
    sha256_hex, PROTOCOL_VERSION, SERVER_NAME, SERVER_VERSION,
};
use lighting_store_d1 as store;
use serde_json::{Value, json};
use worker::*;

/// Read-only bootstrap fact for `lantern_status`.
const SCHEMA_VERSION_SQL: &str = "SELECT value FROM schema_meta WHERE key = 'schema_version'";

const CREDENTIALS_SECRET: &str = "LANTERN_MCP_CREDENTIALS";
const PROJECT_CREDENTIALS_SECRET: &str = "LANTERN_MCP_PROJECT_CREDENTIALS";
const D1_BINDING: &str = "LANTERN_DB";

fn branding_icons() -> Value {
    json!([{ "src": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAYAAABzenr0AAAFUUlEQVRYw7WXS2xUVRjHf+fcO3du592Z6UhtS2mBUoJWRBBLTEyDj4UoaKJB3UBi3MgCY+Jjo2XpwrAxLkx8bHSJ1eAWTXgHFjyEAAFaUkAcOp2ZTqfzuvceF3cYKLYzN4JfcieZ+517vv/3/x7nfAJg+eAAQBvwKvAusB6IAoKHIwrIAyeBb4FfgNKVC5cQdeNxYBTYCYS87OY4at47KYVXtLPA93V70zpg1v/s8uKxoyDol6ztj7CqOwjAxetFTl2doVhxkK1RhOq2AD7SgW3ADq/GuxJ+PtjWy8hQnJCpuy6VLX4/M83esWvcyFS8gBB1m0e0eDKxF1jthfagX/Lpm/1s2dDB9UyFX4+lOXW1QCJisHEgSiLi4/D5LDVbeQmHH4jqwFNeAuc4irX9EUaG4lz9u8TH313i7EQBgP0nwnyxc4CRoTg/90c4dD6L5oEGYL0EYl5WAqzqDhIydQ7+meXsRAEpBVIKzk4UOHguS8jUG3nhUWI6LWKvVD3rFZiGBCBXtLBVPS6A7UBu1gLcNY4CodyNRXMihN4KYrhNIxzQsR1FwK+hlCJoanQl/A2abcd9p5S75o6uMGcxW7abI1g+OKAWUijlGv/87RU80RfGUYpQm040oJOfsyiWrQZ1Cgiad3WzJQspBKfHC+z56TKFkr0oE00ZEAIeaTfo7vAzW7LJFGrMzFkI4dJ7L/JCyWroEmEfoTaNW7kKokUM9GbG80WLsaNphvrC3Jyu8OW+CWbL9qJJo4CQqfHha8voW9LG2NE0+WIN2aQiWjAgOHBmmtc3pXh8WZjP3lqO7ahmn6BJQSpmcHaiwIEz0y0ZkK1CkJmpsu9IGkcp4mEfmhQIAfK+RwjXeDzsw1GKfUfSZGaqraqAllUAkM5XQcHp8QKjP16mZi/Mgk8TjL6zgif7w6RzVS9bewPQkzTx+yTjt0pMpEvc2zo06fYBVxTjt0o8sypKd4fpCYBsuUAIepImCJicKqPUXdp1CSsfDdJmSKRwS3dyqgwCliZNZCv+vQDw6YLupEnNUg0AACNDcVIxPy+uSzC8OtZIzsmpMjVL0Z008ekPCEApCJoanXGDYsXmr+kKChgejPHJG328tC7B0yuj7N7ay6quII6CW9MVihWbzrhR744PAgBFe8hHMmKQLdS4na+yJGaw65WldCVM3t+ylMeWhVjRGWD3tl7agzq381WyhRrJiEF7yIdCPQAABamYQSSgk85XyRcthgdjrFkawnYUpk+iSYHtKDatjjG8OkauaJHOV4kEdFJR40EZgK6EiWlIbmQqlKs2nQk/uiYa+jti6JLeVBvlqs2NTAXTkHQlzRb+twAguZvNk7fLWI7iRqaCtUAfqNmKm9MVLEcxebvcqJ5WWd5Ur+uS7g4Ty1ZcS5dwFBw+l+XYhdy8DicEHL+Q4/C5LI6Ca+kSlq3o6TDR9eYQFm1ESoHpk3S2+7GV4tk17fSm2gDIFmtUag5GffNKzSFbrLH9uU4AejpMHKXobPdj+iTFcvPjWLHArUjTBJvXxulNmUgBWzemGps4Ss1LLkOXvLyho9F43FuUojdlsnltnN9OTP1rjrjjpwRyC3kfDejseL6LZMRACjHPAykEmpz/3Nv13MNKkIwY7Hi+i2hAX6wacjruuPTCvW+FgJk5i6/3TzLQHQBFy2y+X0T959L1ucZFZQE5KZYPDmwHvgHC92tbnf1eZZEregF4TwfGgE0sMJp5vNv/F1HAD8CYFk8mLOA4EADWAMb/ZbUus7iMjwIFLTuVIZ5MlIA/gIu4g0ocd3R62OP5IWAP8BVQuHLhEv8AIuT/hsL9HTAAAAAASUVORK5CYII=", "mimeType": "image/png", "sizes": ["32x32"] }])
}

#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    serve(req, env).await
}

async fn serve(mut req: Request, env: Env) -> Result<Response> {
    let path = req.url()?.path().to_owned();
    if path != "/mcp" {
        return json_status(&json_error_null(-32601, "method not found", "protocol"), 404);
    }
    match req.method() {
        Method::Post => {}
        // Streamable HTTP: no SSE listener on GET; POST is the only input.
        _ => return Ok(Response::empty()?.with_status(405)),
    }

    let principal = match authenticate_request(&req, &env).await {
        Ok(principal) => principal,
        Err(error) => {
            return json_status(
                &json_error_null(error.rpc_code(), &error.to_string(), error.typed()),
                error.http_status(),
            );
        }
    };

    let body = req.text().await.unwrap_or_default();
    let message = match parse_client_msg(&body) {
        Ok(message) => message,
        Err(error) => {
            return json_status(
                &json_error_null(error.rpc_code(), &error.to_string(), error.typed()),
                error.http_status(),
            );
        }
    };

    let (id, method, params) = match message {
        // Notifications (including notifications/initialized) get 202, no body.
        ClientMsg::Notification { .. } => return Ok(Response::empty()?.with_status(202)),
        ClientMsg::Request { id, method, params } => (id, method, params),
    };

    match method.as_str() {
        "initialize" => json_ok(&rpc_result(
            &id,
            json!({
                "protocolVersion": match params.get("protocolVersion").and_then(Value::as_str) {
                    Some(version @ ("2025-11-25" | "2025-06-18" | "2025-03-26" | "2024-11-05" | "2024-10-07")) => version,
                    _ => PROTOCOL_VERSION,
                },
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION, "icons": branding_icons() }
            }),
        )),
        "ping" => json_ok(&rpc_result(&id, json!({}))),
        "tools/list" => {
            let mut catalog = tool_catalog();
            if let Some(tools) = catalog["tools"].as_array_mut() {
                let icons = branding_icons();
                for tool in tools {
                    tool["icons"] = icons.clone();
                }
            }
            json_ok(&rpc_result(&id, catalog))
        }
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            match call_tool(&env, &principal, &name, &arguments).await {
                Ok(payload) => json_ok(&rpc_result(&id, tool_result(payload.to_string(), false))),
                Err(error) => {
                    // Documented contract (smoke): typed refusals carry their
                    // HTTP status in the JSON-RPC error body.
                    json_status(
                        &rpc_error(&id, error.rpc_code(), &error.to_string(), error.typed()),
                        error.http_status(),
                    )
                }
            }
        }
        _ => json_status(&rpc_error(&id, -32601, "method not found", "protocol"), 404),
    }
}

// ---------------------------------------------------------------------------
// Authentication
// ---------------------------------------------------------------------------

async fn authenticate_request(
    req: &Request,
    env: &Env,
) -> std::result::Result<Principal, CoreError> {
    let header = req
        .headers()
        .get("authorization")
        .ok()
        .flatten()
        .unwrap_or_default();
    let token = header
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or(CoreError::Auth)?;
    // Secret is a StringBinding: Display yields the value synchronously.
    let secret_text = env
        .secret(CREDENTIALS_SECRET)
        .map_err(|_| CoreError::Auth)?
        .to_string();
    let entries = load_credentials(&secret_text).map_err(|_| CoreError::Auth)?;
    if let Ok(principal) = authenticate(&entries, token) {
        return Ok(principal);
    }
    let project_secret = env
        .secret(PROJECT_CREDENTIALS_SECRET)
        .map_err(|_| CoreError::Auth)?
        .to_string();
    let project_entries = load_credentials(&project_secret).map_err(|_| CoreError::Auth)?;
    authenticate(&project_entries, token)
}

// ---------------------------------------------------------------------------
// Tool dispatch
// ---------------------------------------------------------------------------

async fn call_tool(
    env: &Env,
    principal: &Principal,
    name: &str,
    arguments: &Value,
) -> std::result::Result<Value, CoreError> {
    let tool =
        Tool::from_name(name).ok_or_else(|| CoreError::Invalid(format!("unknown tool: {name}")))?;

    match tool {
        Tool::Status => {
            // Scope-free, but still requires the read bit.
            lantern_mcp_core::authorize(principal, tool, None)?;
            let d1 = env
                .d1(D1_BINDING)
                .map_err(|_| CoreError::Storage("D1 binding missing".into()))?;
            let row = d1
                .prepare(SCHEMA_VERSION_SQL)
                .first::<Value>(None)
                .await
                .map_err(classify_any)?;
            Ok(json!({
                "server": SERVER_NAME,
                "version": SERVER_VERSION,
                "protocol": PROTOCOL_VERSION,
                "schema_version": row.as_ref().and_then(|v| v.get("value")).cloned().unwrap_or(Value::Null),
                "writer": "lighting-worker (D1 batch)",
                "legacy_native_write_path": false
            }))
        }
        Tool::Search => {
            let args = parse_search(arguments)?;
            lantern_mcp_core::authorize(principal, tool, Some(&args.scope))?;
            let scope_hash = lighting_core::scope_hash(&args.scope);
            let pattern = format!("%{}%", escape_like(&args.query));
            let statement = store::search_beliefs(&scope_hash, &[pattern], args.limit);
            let rows = d1_rows(env, &statement).await?;
            let results: Vec<Value> = rows
                .iter()
                .map(|row| {
                    json!({
                        "belief_id": row.get("id"),
                        "current_revision": row.get("updated_at"),
                        "payload": row.get("payload")
                    })
                })
                .collect();
            Ok(json!({ "results": results, "count": results.len(), "scope_hash": scope_hash }))
        }
        Tool::Context => {
            let args = parse_context(arguments)?;
            lantern_mcp_core::authorize(principal, tool, Some(&args.scope))?;
            let scope_hash = lighting_core::scope_hash(&args.scope);
            let patterns: Vec<String> = args
                .query
                .split_whitespace()
                .filter(|token| token.len() <= 64)
                .take(8)
                .map(|token| format!("%{}%", escape_like(token)))
                .collect();
            let statement = store::search_beliefs(&scope_hash, &patterns, 128);
            let rows = d1_rows(env, &statement).await?;
            let mut scored: Vec<(u32, Value)> = rows
                .iter()
                .map(|row| {
                    let payload = row
                        .get("payload")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    (context_score(payload, &args.query), row.clone())
                })
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0));
            let mut items = Vec::new();
            let mut estimated_tokens = 0usize;
            let mut truncated = false;
            for (score, row) in scored {
                let payload_len = row
                    .get("payload")
                    .and_then(Value::as_str)
                    .map(str::len)
                    .unwrap_or(0);
                let item_tokens = payload_len / 4 + 24;
                if items.len() >= args.item_budget
                    || estimated_tokens + item_tokens > args.token_budget
                {
                    truncated = true;
                    break;
                }
                estimated_tokens += item_tokens;
                items.push(json!({
                    "belief_id": row.get("id"),
                    "current_revision": row.get("updated_at"),
                    "payload": row.get("payload"),
                    "score": score
                }));
            }
            Ok(json!({
                "items": items,
                "count": items.len(),
                "estimated_tokens": estimated_tokens,
                "truncated": truncated,
                "ranking": "bounded token-overlap subset (not native compile_select parity)",
                "scope_hash": scope_hash
            }))
        }
        Tool::Why => {
            let args = parse_why(arguments)?;
            lantern_mcp_core::authorize(principal, tool, Some(&args.scope))?;
            let scope_hash = lighting_core::scope_hash(&args.scope);
            let rows = d1_rows(env, &store::belief_detail(&args.belief_id, &scope_hash)).await?;
            let belief = rows.first().cloned().ok_or(CoreError::NotFound)?;
            let revisions = d1_rows(env, &store::belief_revisions(&args.belief_id, 64)).await?;
            let current = belief.get("updated_at").cloned().unwrap_or(Value::Null);
            Ok(json!({
                "belief": {
                    "belief_id": belief.get("id"),
                    "scope": belief.get("scope"),
                    "subject_key": belief.get("subject_key"),
                    "predicate_key": belief.get("predicate_key"),
                    "current_value": belief.get("current_value"),
                    "state": belief.get("state"),
                    "stale": belief.get("stale"),
                    "current_revision": current,
                    "payload": belief.get("payload")
                },
                "lineage": revisions,
                "next_correction_target": {
                    "belief_id": belief.get("id"),
                    "expected_revision": current
                }
            }))
        }
        Tool::Correct => {
            let args = parse_correct(arguments)?;
            lantern_mcp_core::authorize(principal, tool, Some(&args.scope))?;
            correct(env, principal, args).await
        }

        Tool::Remember => {
            let args = parse_remember(arguments)?;
            lantern_mcp_core::authorize(principal, tool, Some(&args.scope))?;
            remember(env, principal, args).await
        }
        Tool::GetSource => {
            let args = parse_get_source(arguments)?;
            lantern_mcp_core::authorize(principal, tool, Some(&args.scope))?;
            let scope_hash = lighting_core::scope_hash(&args.scope);
            let rows =
                d1_rows(env, &store::source_bytes(&args.source_id, &scope_hash)).await?;
            let row = rows.first().cloned().ok_or(CoreError::NotFound)?;
            Ok(json!({
                "source": {
                    "source_id": row.get("id"),
                    "kind": row.get("kind"),
                    "fingerprint": row.get("fingerprint"),
                    "created_at": row.get("created_at"),
                    "content": row.get("content")
                },
                "scope_hash": scope_hash
            }))
        }
        Tool::ListNotes => {
            let args = parse_list_notes(arguments)?;
            lantern_mcp_core::authorize(principal, tool, Some(&args.scope))?;
            let scope_hash = lighting_core::scope_hash(&args.scope);
            // Optional scoped content filter: same bounds, same ordering as
            // the plain list, so notes older than the newest page stay
            // reachable without widening any read.
            let statement = match &args.query {
                Some(query) => {
                    let pattern = format!("%{}%", escape_like(query));
                    store::search_memory_notes(&scope_hash, &[pattern], args.limit)
                }
                None => store::memory_note_list(&scope_hash, args.limit),
            };
            let rows = d1_rows(env, &statement).await?;
            let notes: Vec<Value> = rows
                .iter()
                .map(|row| {
                    json!({
                        "note_id": row.get("id"),
                        "content": row.get("content"),
                        "created_at": row.get("created_at"),
                        "source_id": row.get("source_id")
                    })
                })
                .collect();
            Ok(json!({
                "notes": notes,
                "count": notes.len(),
                "query": args.query,
                "scope_hash": scope_hash
            }))
        }
    }
}

/* Q17 hosted capture: ordinary remember. Source row is content-addressed
 * (first 32 hex of the content hash), so identical bytes resolve to one row
 * across scopes; visibility stays scope-confined through the memory_item
 * join in source_bytes. The memory row and receipt are per-write identity
 * (composite key), so replays return the original receipt instead of
 * duplicating. No claim, episode or belief row is written here by
 * construction -- a note can never silently become a verified claim. */
async fn remember(
    env: &Env,
    principal: &Principal,
    args: lantern_mcp_core::RememberArgs,
) -> std::result::Result<Value, CoreError> {
    let d1 = env
        .d1(D1_BINDING)
        .map_err(|_| CoreError::Storage("D1 binding missing".into()))?;
    let scope_hash = lighting_core::scope_hash(&args.scope);
    let composite =
        store::idempotency_composite_key(&principal.principal, &scope_hash, &args.idempotency_key);
    let envelope = store::WRITE_ENVELOPE_VERSION;
    let content_hash = sha256_hex(&args.content);
    let hash = remember_payload_hash(envelope, &composite, &content_hash);
    let source_id: String = content_hash.chars().take(32).collect();

    // 1. Replay check: exact key + same payload -> original receipt back.
    if let Some(row) = first_row(&d1, &store::lookup_remember(&composite)).await? {
        return replay_response(row, &hash);
    }

    // 2. One atomic batch: source (idempotent), note, receipt.
    let now_ms = Date::now().as_millis() as i64;
    let accepted_at = canonical_from_millis(now_ms);
    let memory_id = deterministic_id("memory", &composite, &hash);
    let receipt_id = deterministic_id("receipt", &composite, &hash);
    let result_json = json!({
        "receipt_id": receipt_id,
        "source_id": source_id,
        "memory_id": memory_id,
        "envelope_version": envelope,
        "accepted_at": accepted_at,
        "scope_hash": scope_hash,
        "replayed": false
    })
    .to_string();

    let remember = store::RememberInput {
        source_id: source_id.clone(),
        memory_id,
        principal: principal.principal.clone(),
        scope_hash: scope_hash.clone(),
        actor: principal.principal.clone(),
        content: args.content.clone(),
        fingerprint: content_hash.clone(),
        payload_hash: hash.clone(),
        receipt_id,
        accepted_at,
        result_json: result_json.clone(),
        idempotency_key: args.idempotency_key.clone(),
    };
    let statements = store::build_remember_batch(&remember).map_err(from_batch_refusal)?;

    let mut prepared = Vec::with_capacity(statements.len());
    for statement in &statements {
        prepared.push(bind_statement(&d1, statement)?);
    }
    match d1.batch(prepared).await {
        // The receipt already carries scope_hash (built above), so fresh and
        // replayed responses have identical shape apart from `replayed`.
        Ok(_) => serde_json::from_str(&result_json)
            .map_err(|_| CoreError::Storage("receipt serialization".into())),
        Err(error) => {
            let classified = classify_any(error);
            if classified == CoreError::IdempotencyConflict {
                // Lost a race on the same key: return the winner's receipt.
                if let Some(row) = first_row(&d1, &store::lookup_remember(&composite)).await? {
                    return replay_response(row, &hash);
                }
            }
            Err(classified)
        }
    }
}


async fn correct(
    env: &Env,
    principal: &Principal,
    args: lantern_mcp_core::CorrectArgs,
) -> std::result::Result<Value, CoreError> {
    let d1 = env
        .d1(D1_BINDING)
        .map_err(|_| CoreError::Storage("D1 binding missing".into()))?;
    let scope_hash = lighting_core::scope_hash(&args.scope);
    let composite =
        store::idempotency_composite_key(&principal.principal, &scope_hash, &args.idempotency_key);
    let envelope = store::WRITE_ENVELOPE_VERSION;
    let hash = payload_hash(
        envelope,
        &composite,
        &args.belief_id,
        &args.expected_revision,
        &args.new_value,
    );

    // 1. Replay check: exact key + same payload -> original receipt back.
    if let Some(row) = first_row(&d1, &store::lookup_idempotency(&composite)).await? {
        return replay_response(row, &hash);
    }

    // 2. Point read (scope-confined): target must exist in this scope.
    let belief = first_row(&d1, &store::belief_detail(&args.belief_id, &scope_hash))
        .await?
        .ok_or(CoreError::NotFound)?;
    let current_revision = belief
        .get("updated_at")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    // Fast typed 409 here; the DB trigger remains the authority for races
    // between this read and the batch.
    if current_revision != args.expected_revision {
        return Err(CoreError::RevisionConflict);
    }

    // 3. One atomic batch: guard insert, projection advance, idempotency
    //    receipt, mutation receipt — all or nothing via D1 batch().
    let now_ms = Date::now().as_millis() as i64;
    let accepted_at = canonical_from_millis(now_ms);
    let new_token = advance_token(&args.expected_revision, now_ms);
    let merged_payload = merge_belief_payload(
        belief.get("payload").and_then(Value::as_str).unwrap_or("{}"),
        &args.new_value,
    );
    let receipt_id = deterministic_id("receipt", &composite, &hash);
    let revision_id = deterministic_id("revision", &composite, &hash);
    let result_json = json!({
        "receipt_id": receipt_id,
        "belief_id": args.belief_id,
        "current_revision": new_token,
        "value": args.new_value,
        "envelope_version": envelope,
        "accepted_at": accepted_at,
        "scope_hash": scope_hash,
        "replayed": false
    })
    .to_string();

    let correction = store::KeyedCorrection {
        revision_id,
        target_belief_id: args.belief_id.clone(),
        expected_revision: args.expected_revision.clone(),
        new_revision: new_token.clone(),
        new_value: args.new_value.clone(),
        revision_payload: json!({ "actor": principal.principal, "accepted_at": accepted_at })
            .to_string(),
        belief_payload: merged_payload,
        principal: principal.principal.clone(),
        scope_hash: scope_hash.clone(),
        idempotency_key: args.idempotency_key.clone(),
        payload_hash: hash.clone(),
        receipt_id,
        accepted_at,
        result_json: result_json.clone(),
    };
    let statements = store::build_correction_batch(&correction).map_err(from_batch_refusal)?;

    let mut prepared = Vec::with_capacity(statements.len());
    for statement in &statements {
        prepared.push(bind_statement(&d1, statement)?);
    }
    match d1.batch(prepared).await {
        // The receipt already carries scope_hash (built above), so fresh and
        // replayed responses have identical shape apart from `replayed`.
        Ok(_) => serde_json::from_str(&result_json)
            .map_err(|_| CoreError::Storage("receipt serialization".into())),
        Err(error) => {
            let classified = classify_any(error);
            if classified == CoreError::IdempotencyConflict {
                // Lost a race on the same key: return the winner's receipt.
                if let Some(row) = first_row(&d1, &store::lookup_idempotency(&composite)).await? {
                    return replay_response(row, &hash);
                }
            }
            Err(classified)
        }
    }
}

fn replay_response(row: Value, hash: &str) -> std::result::Result<Value, CoreError> {
    let stored_hash = row
        .get("payload_hash")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if stored_hash != hash {
        return Err(CoreError::IdempotencyConflict);
    }
    let mut stored: Value = row
        .get("result_json")
        .and_then(Value::as_str)
        .and_then(|text| serde_json::from_str(text).ok())
        .unwrap_or(json!({}));
    if let Some(object) = stored.as_object_mut() {
        object.insert("replayed".into(), json!(true));
    }
    Ok(stored)
}

// ---------------------------------------------------------------------------
// D1 helpers
// ---------------------------------------------------------------------------

fn classify_any(error: worker::Error) -> CoreError {
    let combined = format!("{error} {error:?}");
    classify_d1_error(&combined)
}

fn bind_statement(
    d1: &D1Database,
    statement: &store::SqlStatement,
) -> std::result::Result<D1PreparedStatement, CoreError> {
    let prepared = d1.prepare(&statement.sql);
    let mut args: Vec<_> = Vec::with_capacity(statement.params.len());
    for param in &statement.params {
        let value = match param {
            store::SqlValue::Text(text) => Value::String(text.clone()),
            store::SqlValue::Integer(int) => json!(int),
            store::SqlValue::Real(real) => json!(real),
            store::SqlValue::Null => Value::Null,
        };
        args.push(worker::d1::serde_wasm_bindgen::to_value(&value).map_err(|_| {
            CoreError::Storage("parameter conversion failed".into())
        })?);
    }
    prepared
        .bind(&args)
        .map_err(|_| CoreError::Storage("parameter binding failed".into()))
}

async fn d1_rows(
    env: &Env,
    statement: &store::SqlStatement,
) -> std::result::Result<Vec<Value>, CoreError> {
    let d1 = env
        .d1(D1_BINDING)
        .map_err(|_| CoreError::Storage("D1 binding missing".into()))?;
    let prepared = bind_statement(&d1, statement)?;
    let result = prepared.all().await.map_err(classify_any)?;
    result
        .results::<Value>()
        .map_err(|_| CoreError::Storage("row decoding failed".into()))
}

async fn first_row(
    d1: &D1Database,
    statement: &store::SqlStatement,
) -> std::result::Result<Option<Value>, CoreError> {
    let prepared = bind_statement(d1, statement)?;
    prepared
        .first::<Value>(None)
        .await
        .map_err(classify_any)
}

// ---------------------------------------------------------------------------
// Response helpers
// ---------------------------------------------------------------------------

fn tool_result(text: String, is_error: bool) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error
    })
}

fn json_ok(value: &Value) -> Result<Response> {
    Ok(Response::from_json(value)?)
}

fn json_status(value: &Value, status: u16) -> Result<Response> {
    let response = Response::from_json(value)?;
    Ok(if status == 200 {
        response
    } else {
        response.with_status(status)
    })
}

fn json_error_null(code: i32, message: &str, typed: &str) -> Value {
    rpc_error(&Value::Null, code, message, typed)
}
