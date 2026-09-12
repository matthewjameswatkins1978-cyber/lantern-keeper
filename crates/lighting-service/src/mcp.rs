//! Small HTTP JSON-RPC MCP capability surface.
//!
//! The domain API remains available for ordinary clients. This adapter keeps
//! the AI-facing contract semantic and prevents SurrealDB operations leaking
//! into agent prompts.

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    memory_dto::{MemoryContextRequest, MemorySearchQuery, RememberRequest},
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: Option<String>,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize)]
pub struct RpcResponse {
    pub jsonrpc: &'static str,
    pub id: Option<Value>,
    pub result: Option<Value>,
    pub error: Option<Value>,
}

pub async fn handle(
    State(state): State<AppState>,
    Json(request): Json<RpcRequest>,
) -> Json<RpcResponse> {
    let id = request.id.clone();
    let response = match request.method.as_str() {
        "initialize" => ok(
            id,
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "lantern-keeper", "version": env!("CARGO_PKG_VERSION")}
            }),
        ),
        "tools/list" => ok(id, json!({"tools": tools()})),
        "tools/call" => call_tool(&state, id, request.params).await,
        _ => err(id, -32601, "method not found"),
    };
    Json(response)
}

fn tools() -> Vec<Value> {
    vec![
        json!({"name":"lantern_search","description":"Search current or historical canonical memory with provenance.","inputSchema":{"type":"object","properties":{"q":{"type":"string"},"scope":{"type":"string"},"include_history":{"type":"boolean"},"limit":{"type":"integer"}},"required":["q"]}}),
        json!({"name":"lantern_context","description":"Build a bounded context package for a topic.","inputSchema":{"type":"object","properties":{"query":{"type":"string"},"scope":{"type":"string"},"include_history":{"type":"boolean"},"budget":{"type":"integer"}},"required":["query"]}}),
        json!({"name":"lantern_remember","description":"Submit durable information for deterministic reconciliation.","inputSchema":{"type":"object","properties":{"content":{"type":"string"},"scope":{"type":"string"},"kind":{"type":"string"},"identity_key":{"type":"string"},"confidence":{"type":"integer"},"importance":{"type":"integer"},"source_ids":{"type":"array","items":{"type":"string"}}},"required":["content"]}}),
        json!({"name":"lantern_relations","description":"List typed relations and unresolved imported targets.","inputSchema":{"type":"object","properties":{"memory_id":{"type":"string"}}}}),
        json!({"name":"lantern_audit","description":"Audit canonical memory for duplicate current identities and stale lineage.","inputSchema":{"type":"object"}}),
        json!({"name":"lantern_doctor","description":"Check canonical memory checksums and review state.","inputSchema":{"type":"object"}}),
        json!({"name":"lantern_forget","description":"Tombstone one canonical memory and record the destructive operation.","inputSchema":{"type":"object","properties":{"memory_id":{"type":"string"}},"required":["memory_id"]}}),
    ]
}

async fn call_tool(state: &AppState, id: Option<Value>, params: Value) -> RpcResponse {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let Some(service) = &state.memory_service else {
        return err(id, -32000, "memory service unavailable");
    };
    let result = match name {
        "lantern_search" => match serde_json::from_value::<MemorySearchQuery>(arguments) {
            Ok(query) => futures_search(service, query)
                .await
                .map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        },
        "lantern_context" => match serde_json::from_value::<MemoryContextRequest>(arguments) {
            Ok(query) => futures_context(service, query)
                .await
                .map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        },
        "lantern_remember" => match serde_json::from_value::<RememberRequest>(arguments) {
            Ok(request) => futures_remember(service, request)
                .await
                .map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        },
        "lantern_relations" => {
            let memory_id = arguments.get("memory_id").and_then(Value::as_str);
            service
                .relations(memory_id)
                .await
                .map(|value| json!(value))
                .map_err(|e| e.to_string())
        }
        "lantern_audit" => service.audit().await.map_err(|e| e.to_string()),
        "lantern_doctor" => service.doctor().await.map_err(|e| e.to_string()),
        "lantern_forget" => {
            let Some(memory_id) = arguments.get("memory_id").and_then(Value::as_str) else {
                return err(id, -32602, "memory_id is required");
            };
            match lighting_core::MemoryId::from_string(memory_id.to_owned()) {
                Ok(memory_id) => service
                    .forget(memory_id.as_str())
                    .await
                    .map(|value| json!(value))
                    .map_err(|e| e.to_string()),
                Err(e) => Err(e.to_string()),
            }
        }
        _ => return err(id, -32602, "unknown Lantern tool"),
    };
    match result {
        Ok(value) => ok(
            id,
            json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value}),
        ),
        Err(message) => err(id, -32001, &message),
    }
}

async fn futures_search(
    service: &crate::memory_ops::MemoryService,
    query: MemorySearchQuery,
) -> Result<Value, crate::memory_ops::MemoryOperationError> {
    Ok(json!(
        service
            .search(
                &query.q,
                query.scope.as_deref(),
                query.include_history.unwrap_or(false),
                query.limit.unwrap_or(20)
            )
            .await?
    ))
}
async fn futures_context(
    service: &crate::memory_ops::MemoryService,
    query: MemoryContextRequest,
) -> Result<Value, crate::memory_ops::MemoryOperationError> {
    Ok(json!(service.context(query).await?))
}
async fn futures_remember(
    service: &crate::memory_ops::MemoryService,
    request: RememberRequest,
) -> Result<Value, crate::memory_ops::MemoryOperationError> {
    Ok(json!(service.remember(request).await?))
}

fn ok(id: Option<Value>, result: Value) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    }
}
fn err(id: Option<Value>, code: i32, message: &str) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(json!({"code":code,"message":message})),
    }
}
