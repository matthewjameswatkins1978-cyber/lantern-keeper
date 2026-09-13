use std::{
    collections::BTreeSet,
    io::{self, BufRead, Write},
};

use serde_json::{Value, json};

const PROTOCOL_VERSION: &str = "2025-06-18";

pub fn run(service_url: &str) -> anyhow::Result<()> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request = match serde_json::from_str::<RpcRequest>(&line) {
            Ok(request) => request,
            Err(error) => {
                write_response(
                    &mut stdout,
                    rpc_error(Value::Null, -32700, format!("invalid JSON: {error}")),
                )?;
                continue;
            }
        };
        if request.method.starts_with("notifications/") {
            continue;
        }
        let response = handle_request(&client, service_url, &request);
        write_response(&mut stdout, response)?;
    }

    Ok(())
}

#[derive(Debug, serde::Deserialize)]
struct RpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

fn handle_request(
    client: &reqwest::blocking::Client,
    service_url: &str,
    request: &RpcRequest,
) -> Value {
    match request.method.as_str() {
        "initialize" => rpc_result(
            request.id.clone(),
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "lantern-keeper", "version": env!("CARGO_PKG_VERSION")}
            }),
        ),
        "ping" => rpc_result(request.id.clone(), json!({})),
        "tools/list" => rpc_result(request.id.clone(), json!({"tools": tool_definitions()})),
        "tools/call" => match call_tool(client, service_url, &request.params) {
            Ok(body) => rpc_result(request.id.clone(), tool_success(body)),
            Err(error) => rpc_result(request.id.clone(), tool_failure(error)),
        },
        _ => rpc_error(
            request.id.clone().unwrap_or(Value::Null),
            -32601,
            format!("method not found: {}", request.method),
        ),
    }
}

fn call_tool(
    client: &reqwest::blocking::Client,
    service_url: &str,
    params: &Value,
) -> Result<Value, String> {
    let params = params
        .as_object()
        .ok_or_else(|| "tools/call params must be an object".to_owned())?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "tools/call requires a tool name".to_owned())?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let arguments = arguments
        .as_object()
        .ok_or_else(|| "tool arguments must be an object".to_owned())?;

    match name {
        "lantern_context" => {
            reject_unknown(
                arguments,
                &[
                    "query",
                    "actor",
                    "project_hints",
                    "scope",
                    "intent",
                    "item_budget",
                    "token_budget",
                ],
            )?;
            let query = required_string(arguments, "query")?;
            let body = json!({
                "query": query,
                "actor": arguments.get("actor"),
                "project_hints": arguments
                    .get("project_hints")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
                "scope": arguments.get("scope").cloned().unwrap_or_else(|| json!({})),
                "intent": arguments.get("intent"),
                "item_budget": arguments.get("item_budget").cloned().unwrap_or_else(|| json!(10)),
                "token_budget": arguments
                    .get("token_budget")
                    .cloned()
                    .unwrap_or_else(|| json!(2048)),
            });
            post_json(client, service_url, "/api/v1/epistemic/context", body)
        }
        "lantern_remember" => {
            reject_unknown(
                arguments,
                &[
                    "kind",
                    "content",
                    "subject_key",
                    "predicate_key",
                    "scope",
                    "source_id",
                    "episode_id",
                    "originator_actor_id",
                    "transmitter_actor_id",
                    "holder_actor_id",
                    "salience",
                ],
            )?;
            let kind = required_string(arguments, "kind")?;
            let content = required_string(arguments, "content")?;
            if kind == "claim" {
                let predicate_key = required_string(arguments, "predicate_key")?;
                let subject_key = arguments
                    .get("subject_key")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or("matthew");
                let holder = arguments
                    .get("holder_actor_id")
                    .cloned()
                    .unwrap_or_else(|| json!("matthew"));
                let originator = arguments
                    .get("originator_actor_id")
                    .cloned()
                    .unwrap_or_else(|| holder.clone());
                let claim = json!({
                    "subject_key": subject_key,
                    "value": content,
                    "source_id": arguments.get("source_id").cloned().unwrap_or(Value::Null),
                    "episode_id": arguments.get("episode_id").cloned().unwrap_or(Value::Null),
                    "evidence_span": Value::Null,
                    "predicate_key": predicate_key,
                    "predicate_candidate": Value::Null,
                    "predicate_status": Value::Null,
                    "scope": arguments.get("scope").cloned().unwrap_or_else(|| json!({})),
                    "polarity": true,
                    "originator_actor_id": originator.clone(),
                    "speaker_actor_id": originator,
                    "transmitter_actor_id": arguments.get("transmitter_actor_id").cloned().unwrap_or(Value::Null),
                    "holder_actor_id": holder,
                    "stance": "endorsing",
                    "framing_path": [],
                    "confidence": arguments.get("salience").and_then(Value::as_f64).unwrap_or(0.8),
                    "known_at": Value::Null,
                    "valid_from": Value::Null,
                    "valid_to": Value::Null,
                    "extractor": "lucy_mcp",
                    "extractor_version": "1"
                });
                let captured = post_json(client, service_url, "/api/v1/claims", claim)?;
                let claim_id = captured
                    .get("claim")
                    .and_then(|claim| claim.get("id"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| "claim capture returned no claim ID".to_owned())?;
                let reconciliation = post_json(
                    client,
                    service_url,
                    &format!("/api/v1/claims/{claim_id}/reconcile"),
                    json!({"independent_evidence": false}),
                )?;
                Ok(json!({
                    "claim": captured.get("claim").cloned().unwrap_or(Value::Null),
                    "reconciliation": reconciliation
                }))
            } else {
                post_json(
                    client,
                    service_url,
                    "/api/v1/memory-items",
                    Value::Object(arguments.clone()),
                )
            }
        }
        "lantern_search" => {
            reject_unknown(arguments, &["phrase", "include_archived", "limit"])?;
            post_json(
                client,
                service_url,
                "/api/v1/memory-items/search",
                Value::Object(arguments.clone()),
            )
        }
        "lantern_why" => {
            reject_unknown(arguments, &["belief_id", "memory_id", "query"])?;
            match parse_why_target(arguments)? {
                WhyTarget::Belief(belief_id) => {
                    reject_path_segment(&belief_id)?;
                    get_json(
                        client,
                        service_url,
                        &format!("/api/v1/beliefs/{belief_id}/explain"),
                    )
                }
                WhyTarget::Memory(memory_id) => {
                    reject_path_segment(&memory_id)?;
                    let body = post_json(
                        client,
                        service_url,
                        "/api/v1/memory-items/search",
                        json!({"include_archived": true, "limit": 0}),
                    )?;
                    let memory_item = body
                        .get("memory_items")
                        .and_then(Value::as_array)
                        .and_then(|items| {
                            items.iter().find(|item| {
                                item.get("id").and_then(Value::as_str) == Some(memory_id.as_str())
                            })
                        })
                        .cloned()
                        .ok_or_else(|| format!("memory item not found: {memory_id}"))?;
                    Ok(json!({
                        "memory_item": memory_item,
                        "provenance": {
                            "source_id": memory_item.get("source_id").cloned().unwrap_or(Value::Null),
                            "episode_id": memory_item.get("episode_id").cloned().unwrap_or(Value::Null),
                            "originator_actor_id": memory_item.get("originator_actor_id").cloned().unwrap_or(Value::Null),
                            "transmitter_actor_id": memory_item.get("transmitter_actor_id").cloned().unwrap_or(Value::Null),
                            "holder_actor_id": memory_item.get("holder_actor_id").cloned().unwrap_or(Value::Null),
                        }
                    }))
                }
                WhyTarget::Query(query) => {
                    let matches = get_json_query(
                        client,
                        service_url,
                        "/api/v1/beliefs/search",
                        &[("query", query.as_str()), ("include_stale", "false")],
                    )?;
                    let beliefs = matches
                        .get("beliefs")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    if beliefs.len() != 1 {
                        return Ok(json!({
                            "ambiguous": beliefs.len() > 1,
                            "query": query,
                            "beliefs": beliefs,
                        }));
                    }
                    let belief_id =
                        beliefs[0]
                            .get("id")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                "belief search returned a record without an ID".to_owned()
                            })?;
                    reject_path_segment(belief_id)?;
                    get_json(
                        client,
                        service_url,
                        &format!("/api/v1/beliefs/{belief_id}/explain"),
                    )
                }
            }
        }
        "lantern_correct" => {
            reject_unknown(
                arguments,
                &[
                    "target_belief_id",
                    "target_query",
                    "correction_text",
                    "replacement_value",
                    "context_pack_id",
                    "scope",
                ],
            )?;
            let has_belief_id = arguments
                .get("target_belief_id")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty());
            let has_target_query = arguments
                .get("target_query")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty());
            if has_belief_id == has_target_query {
                return Err(
                    "lantern_correct requires exactly one nonblank target: target_belief_id or target_query"
                        .to_owned(),
                );
            }
            required_string(arguments, "correction_text")?;
            required_string(arguments, "replacement_value")?;
            post_json(
                client,
                service_url,
                "/api/v1/corrections",
                Value::Object(arguments.clone()),
            )
        }
        "lantern_status" => {
            reject_unknown(arguments, &[])?;
            let ready = get_json(client, service_url, "/health/ready")?;
            let version = get_json(client, service_url, "/api/v1/version")?;
            Ok(json!({"ready": ready, "version": version}))
        }
        "lantern_foreman_queue" => {
            reject_unknown(arguments, &["limit"])?;
            let limit = arguments.get("limit").and_then(Value::as_u64).unwrap_or(10);
            get_json(
                client,
                service_url,
                &format!("/api/v1/foreman/queue?limit={limit}"),
            )
        }
        "lantern_foreman_review" => {
            reject_unknown(arguments, &["proposal_id", "decision", "payload"])?;
            let proposal_id = required_string(arguments, "proposal_id")?;
            reject_path_segment(&proposal_id)?;
            required_string(arguments, "decision")?;
            post_json(
                client,
                service_url,
                &format!("/api/v1/foreman/{proposal_id}/review"),
                Value::Object(arguments.clone()),
            )
        }
        _ => Err(format!("unknown tool: {name}")),
    }
}

fn required_string(
    arguments: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<String, String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("tool argument '{key}' must be a nonblank string"))
}

enum WhyTarget {
    Belief(String),
    Memory(String),
    Query(String),
}

fn parse_why_target(arguments: &serde_json::Map<String, Value>) -> Result<WhyTarget, String> {
    let mut targets = ["belief_id", "memory_id", "query"]
        .into_iter()
        .filter_map(|key| {
            arguments
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| (key, value.to_owned()))
        });
    let Some((key, value)) = targets.next() else {
        return Err(
            "lantern_why requires exactly one nonblank target: belief_id, memory_id, or query"
                .to_owned(),
        );
    };
    if targets.next().is_some() {
        return Err(
            "lantern_why requires exactly one nonblank target: belief_id, memory_id, or query"
                .to_owned(),
        );
    }
    Ok(match key {
        "belief_id" => WhyTarget::Belief(value),
        "memory_id" => WhyTarget::Memory(value),
        "query" => WhyTarget::Query(value),
        _ => unreachable!("target keys are fixed above"),
    })
}

fn reject_unknown(
    arguments: &serde_json::Map<String, Value>,
    allowed: &[&str],
) -> Result<(), String> {
    let allowed = allowed.iter().copied().collect::<BTreeSet<_>>();
    if let Some(key) = arguments.keys().find(|key| !allowed.contains(key.as_str())) {
        return Err(format!("unknown tool argument: {key}"));
    }
    Ok(())
}

fn reject_path_segment(value: &str) -> Result<(), String> {
    if value.contains('/') || value.contains('\\') || value.contains('?') || value.contains('#') {
        Err("path identifier contains unsupported characters".to_owned())
    } else {
        Ok(())
    }
}

fn get_json(
    client: &reqwest::blocking::Client,
    service_url: &str,
    path: &str,
) -> Result<Value, String> {
    let response = client
        .get(format!("{}{}", service_url.trim_end_matches('/'), path))
        .send()
        .map_err(|error| format!("request failed: {error}"))?;
    decode_response(response)
}

fn get_json_query(
    client: &reqwest::blocking::Client,
    service_url: &str,
    path: &str,
    query: &[(&str, &str)],
) -> Result<Value, String> {
    let response = client
        .get(format!("{}{}", service_url.trim_end_matches('/'), path))
        .query(query)
        .send()
        .map_err(|error| format!("request failed: {error}"))?;
    decode_response(response)
}

fn post_json(
    client: &reqwest::blocking::Client,
    service_url: &str,
    path: &str,
    body: Value,
) -> Result<Value, String> {
    let response = client
        .post(format!("{}{}", service_url.trim_end_matches('/'), path))
        .json(&body)
        .send()
        .map_err(|error| format!("request failed: {error}"))?;
    decode_response(response)
}

fn decode_response(response: reqwest::blocking::Response) -> Result<Value, String> {
    let status = response.status();
    let text = response
        .text()
        .map_err(|error| format!("response read failed: {error}"))?;
    let body = serde_json::from_str(&text).unwrap_or_else(|_| json!({"message": text}));
    if status.is_success() {
        Ok(body)
    } else {
        Err(body.to_string())
    }
}

fn tool_success(body: Value) -> Value {
    json!({
        "content": [{"type": "text", "text": body.to_string()}],
        "structuredContent": body,
        "isError": false
    })
}

fn tool_failure(error: String) -> Value {
    json!({
        "content": [{"type": "text", "text": error}],
        "isError": true
    })
}

fn rpc_result(id: Option<Value>, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id.unwrap_or(Value::Null), "result": result})
}

fn rpc_error(id: Value, code: i32, message: String) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn write_response(stdout: &mut impl Write, response: Value) -> io::Result<()> {
    serde_json::to_writer(&mut *stdout, &response)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

fn tool_definitions() -> Vec<Value> {
    vec![
        tool(
            "lantern_context",
            "Compile bounded current and historical context",
            json!({
                    "type": "object", "properties": {
                    "query": {"type": "string"}, "actor": {"type": "string"},
                    "project_hints": {"type": "array", "items": {"type": "string"}},
                    "scope": {"type": "object", "additionalProperties": {"type": "string"}},
                    "intent": {"type": "string"}, "item_budget": {"type": "integer", "minimum": 1, "maximum": 100},
                    "token_budget": {"type": "integer", "minimum": 128, "maximum": 16000}
                }, "required": ["query"], "additionalProperties": false
            }),
        ),
        tool(
            "lantern_remember",
            "Capture a soft memory without raw database access",
            json!({
                "type": "object", "properties": {
                    "kind": {"type": "string"}, "content": {"type": "string"},
                    "subject_key": {"type": ["string", "null"]},
                    "predicate_key": {"type": ["string", "null"]},
                    "scope": {"type": "object", "additionalProperties": {"type": "string"}},
                    "source_id": {"type": ["string", "null"]}, "episode_id": {"type": ["string", "null"]},
                    "originator_actor_id": {"type": ["string", "null"]}, "transmitter_actor_id": {"type": ["string", "null"]},
                    "holder_actor_id": {"type": ["string", "null"]}, "salience": {"type": "number", "minimum": 0, "maximum": 1}
                }, "required": ["kind", "content"], "additionalProperties": false
            }),
        ),
        tool(
            "lantern_search",
            "Search soft memories",
            json!({
                "type": "object", "properties": {
                    "phrase": {"type": ["string", "null"]}, "include_archived": {"type": "boolean"}, "limit": {"type": "integer", "minimum": 1, "maximum": 100}
                }, "additionalProperties": false
            }),
        ),
        tool(
            "lantern_why",
            "Explain a belief or memory from recorded provenance; natural queries must resolve uniquely",
            json!({
                "type": "object", "properties": {
                    "belief_id": {"type": "string"},
                    "memory_id": {"type": "string"},
                    "query": {"type": "string"}
                },
                "oneOf": [
                    {"required": ["belief_id"]},
                    {"required": ["memory_id"]},
                    {"required": ["query"]}
                ],
                "additionalProperties": false
            }),
        ),
        tool(
            "lantern_correct",
            "Record a Matthew correction against one belief",
            json!({
                "type": "object", "properties": {
                    "target_belief_id": {"type": "string"}, "correction_text": {"type": "string"},
                    "target_query": {"type": "string"},
                    "replacement_value": {"type": "string"}, "context_pack_id": {"type": ["string", "null"]},
                    "scope": {"type": "object", "additionalProperties": {"type": "string"}}
                }, "required": ["correction_text", "replacement_value"],
                "oneOf": [
                    {"required": ["target_belief_id"]},
                    {"required": ["target_query"]}
                ], "additionalProperties": false
            }),
        ),
        tool(
            "lantern_status",
            "Read local Lantern service status",
            json!({"type": "object", "additionalProperties": false}),
        ),
        tool(
            "lantern_foreman_queue",
            "Read a bounded queue of Lucy-owned proposals",
            json!({
                "type": "object", "properties": {"limit": {"type": "integer", "minimum": 1, "maximum": 50}}, "additionalProperties": false
            }),
        ),
        tool(
            "lantern_foreman_review",
            "Review one Lucy-owned proposal",
            json!({
                "type": "object", "properties": {
                    "proposal_id": {"type": "string"}, "decision": {"type": "string", "enum": ["accept", "reject", "modify", "defer"]}, "payload": {"type": ["string", "null"]}
                }, "required": ["proposal_id", "decision"], "additionalProperties": false
            }),
        ),
    ]
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({"name": name, "description": description, "inputSchema": input_schema})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_catalog_is_bounded_and_tool_neutral() {
        let definitions = tool_definitions();
        assert_eq!(definitions.len(), 8);
        assert!(definitions.iter().all(|tool| {
            tool.get("inputSchema")
                .and_then(|schema| schema.get("additionalProperties"))
                .is_some()
        }));
    }

    #[test]
    fn unknown_arguments_are_rejected() {
        let arguments = serde_json::from_value(json!({"query": "x", "surprise": true})).unwrap();
        let error = reject_unknown(&arguments, &["query"]).expect_err("unknown field must fail");
        assert!(error.contains("surprise"));
    }

    #[test]
    fn why_requires_one_target() {
        let arguments = serde_json::from_value(json!({"query": "editor"})).unwrap();
        assert!(matches!(
            parse_why_target(&arguments),
            Ok(WhyTarget::Query(query)) if query == "editor"
        ));

        let arguments =
            serde_json::from_value(json!({"belief_id": "b", "query": "editor"})).unwrap();
        assert!(parse_why_target(&arguments).is_err());
    }
}
