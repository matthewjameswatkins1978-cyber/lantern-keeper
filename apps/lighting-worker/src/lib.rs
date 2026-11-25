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
    parse_correct, parse_search, parse_why, payload_hash, rpc_error, rpc_result, tool_catalog,
    PROTOCOL_VERSION, SERVER_NAME, SERVER_VERSION,
};
use lighting_store_d1 as store;
use serde_json::{Value, json};
use worker::*;

/// Read-only bootstrap fact for `lantern_status`.
const SCHEMA_VERSION_SQL: &str = "SELECT value FROM schema_meta WHERE key = 'schema_version'";

const CREDENTIALS_SECRET: &str = "LANTERN_MCP_CREDENTIALS";
const D1_BINDING: &str = "LANTERN_DB";

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
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION }
            }),
        )),
        "ping" => json_ok(&rpc_result(&id, json!({}))),
        "tools/list" => json_ok(&rpc_result(&id, tool_catalog())),
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
    authenticate(&entries, token)
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
        "replayed": false
    })
    .to_string();

    let correction = store::KeyedCorrection {
        revision_id,
        target_belief_id: args.belief_id.clone(),
        expected_revision: args.expected_revision.clone(),
        new_revision: new_token.clone(),
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
        Ok(_) => {
            let mut payload: Value = serde_json::from_str(&result_json)
                .map_err(|_| CoreError::Storage("receipt serialization".into()))?;
            if let Some(object) = payload.as_object_mut() {
                object.insert("scope_hash".into(), json!(scope_hash));
            }
            Ok(payload)
        }
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
