use std::{
    net::IpAddr,
    sync::{Arc, OnceLock},
};

use anyhow::{Context, bail};
use axum::Router;
use rmcp::{
    ErrorData as McpError, ServerHandler,
    model::{
        CallToolRequestMethod, CallToolRequestParams, CallToolResponse, CallToolResult,
        ContentBlock, Implementation, ListToolsResult, PaginatedRequestParams, ServerCapabilities,
        ServerConfig, Tool, ToolAnnotations,
    },
    service::RequestContext,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use tokio::net::TcpListener;
use tracing::info;

const REMOTE_READ_TOOLS: &[&str] = &[
    "lantern_status",
    "lantern_search",
    "lantern_context",
    "lantern_why",
];

#[derive(Clone)]
struct ReadOnlyLanternServer {
    service_url: Arc<str>,
    client: Arc<OnceLock<Result<reqwest::blocking::Client, String>>>,
}

impl ReadOnlyLanternServer {
    fn new(service_url: &str) -> Self {
        Self {
            service_url: Arc::from(service_url),
            client: Arc::default(),
        }
    }
}

impl ServerHandler for ReadOnlyLanternServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
            Implementation::new("lantern-keeper", env!("CARGO_PKG_VERSION")),
        )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(remote_read_tools()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        if !REMOTE_READ_TOOLS.contains(&request.name.as_ref()) {
            return Err(McpError::method_not_found::<CallToolRequestMethod>());
        }

        let server = self.clone();
        let name = request.name.to_string();
        let arguments = serde_json::Value::Object(request.arguments.unwrap_or_default());
        let result = tokio::task::spawn_blocking(move || {
            let client = server.client.get_or_init(|| {
                reqwest::blocking::Client::builder()
                    .timeout(std::time::Duration::from_secs(30))
                    .build()
                    .map_err(|error| error.to_string())
            });
            client.as_ref().map_err(Clone::clone).and_then(|client| {
                crate::mcp::call_tool(
                    client,
                    &server.service_url,
                    &serde_json::json!({ "name": name, "arguments": arguments }),
                )
            })
        })
        .await
        .map_err(|error| McpError::internal_error(error.to_string(), None))?;

        Ok(match result {
            Ok(value) => CallToolResult::structured(value).into(),
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error)]).into(),
        })
    }
}

fn remote_read_tools() -> Vec<Tool> {
    let definitions = crate::mcp::tool_definitions();
    REMOTE_READ_TOOLS
        .iter()
        .filter_map(|name| {
            let definition = definitions.iter().find(|definition| {
                definition.get("name").and_then(|value| value.as_str()) == Some(name)
            })?;
            let description = definition.get("description")?.as_str()?;
            let schema = definition.get("inputSchema")?.as_object()?.clone();
            Some(
                Tool::new((*name).to_owned(), description.to_owned(), Arc::new(schema))
                    .with_annotations(
                        ToolAnnotations::new()
                            .read_only(true)
                            .destructive(false)
                            .open_world(false),
                    ),
            )
        })
        .collect()
}

pub async fn run(service_url: &str, host: &str, port: u16) -> anyhow::Result<()> {
    let address = parse_loopback_address(host)?;

    let router = router(service_url);
    let listener = TcpListener::bind((address, port))
        .await
        .with_context(|| format!("failed to bind MCP HTTP on {address}:{port}"))?;
    let bound = listener.local_addr()?;
    info!(%bound, endpoint = "/mcp", "read-only Lantern MCP listening");
    axum::serve(listener, router)
        .await
        .context("read-only MCP HTTP server failed")
}

fn parse_loopback_address(host: &str) -> anyhow::Result<IpAddr> {
    let address: IpAddr = host
        .parse()
        .context("MCP HTTP host must be an IP address")?;
    if !address.is_loopback() {
        bail!("MCP HTTP binds to loopback only; received {address}");
    }
    Ok(address)
}

fn router(service_url: &str) -> Router {
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true);
    let server = ReadOnlyLanternServer::new(service_url);
    let service = StreamableHttpService::new(
        move || Ok(server.clone()),
        LocalSessionManager::default().into(),
        config,
    );
    Router::new().nest_service("/mcp", service)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_catalog_contains_only_the_four_read_tools_with_safety_hints() {
        let tools = remote_read_tools();
        let names = tools
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(names, REMOTE_READ_TOOLS);
        for tool in tools {
            let annotations = tool.annotations.expect("safety annotations are required");
            assert_eq!(annotations.read_only_hint, Some(true));
            assert_eq!(annotations.destructive_hint, Some(false));
            assert_eq!(annotations.open_world_hint, Some(false));
        }
    }

    #[test]
    fn remote_binding_rejects_non_loopback_addresses() {
        assert_eq!(
            parse_loopback_address("127.0.0.1").unwrap(),
            "127.0.0.1".parse::<IpAddr>().unwrap()
        );
        assert!(parse_loopback_address("0.0.0.0").is_err());
        assert!(parse_loopback_address("lantern.local").is_err());
    }

    #[tokio::test]
    async fn streamable_http_initializes_lists_only_reads_and_calls_lantern_service() {
        use axum::{
            body::to_bytes,
            http::Request,
            routing::{get, post},
        };
        use serde_json::{Value, json};
        use tower::ServiceExt;

        async fn ready() -> axum::Json<Value> {
            axum::Json(json!({"ready": true}))
        }
        async fn version() -> axum::Json<Value> {
            axum::Json(json!({"version": "test-service"}))
        }
        async fn search() -> axum::Json<Value> {
            axum::Json(json!({"memory_items": [{"id": "fixture-id", "content": "test marker"}]}))
        }
        async fn context() -> axum::Json<Value> {
            axum::Json(json!({"items": []}))
        }

        async fn rpc(router: &Router, body: Value) -> (axum::http::StatusCode, Value) {
            let response = router
                .clone()
                .oneshot(
                    Request::post("/mcp")
                        .header("host", "127.0.0.1:4318")
                        .header("content-type", "application/json")
                        .header("accept", "application/json, text/event-stream")
                        .body(axum::body::Body::from(body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
            (status, serde_json::from_slice(&body).unwrap())
        }

        let lantern = Router::new()
            .route("/health/ready", get(ready))
            .route("/api/v1/version", get(version))
            .route("/api/v1/memory-items/search", post(search))
            .route("/api/v1/epistemic/context", post(context));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let lantern_task =
            tokio::spawn(async move { axum::serve(listener, lantern).await.unwrap() });
        let remote = router(&format!("http://{address}"));

        let (status, initialized) = rpc(
            &remote,
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "2025-03-26", "capabilities": {},
                    "clientInfo": {"name": "lantern-test", "version": "1"}}
            }),
        )
        .await;
        assert!(status.is_success());
        assert_eq!(
            initialized["result"]["serverInfo"]["name"],
            "lantern-keeper"
        );

        let (_, listed) = rpc(
            &remote,
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
        )
        .await;
        let tools = listed["result"]["tools"].as_array().unwrap();
        let names = tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(names, REMOTE_READ_TOOLS);
        assert!(tools.iter().all(|tool| {
            tool["annotations"]["readOnlyHint"] == true
                && tool["annotations"]["destructiveHint"] == false
                && tool["annotations"]["openWorldHint"] == false
        }));

        let (_, status_call) = rpc(
            &remote,
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {"name": "lantern_status", "arguments": {}}}),
        )
        .await;
        assert_eq!(
            status_call["result"]["structuredContent"]["ready"]["ready"],
            true
        );

        let (_, search_call) = rpc(
            &remote,
            json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call",
                "params": {"name": "lantern_search", "arguments": {"phrase": "marker"}}}),
        )
        .await;
        assert_eq!(
            search_call["result"]["structuredContent"]["memory_items"][0]["id"],
            "fixture-id"
        );

        let (_, denied) = rpc(
            &remote,
            json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call",
                "params": {"name": "lantern_remember", "arguments": {}}}),
        )
        .await;
        assert_eq!(denied["error"]["code"], -32601);

        let (_, invalid_args) = rpc(
            &remote,
            json!({"jsonrpc": "2.0", "id": 6, "method": "tools/call",
                "params": {"name": "lantern_status", "arguments": {"unexpected": true}}}),
        )
        .await;
        assert_eq!(invalid_args["result"]["isError"], true);
        assert!(
            invalid_args["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("unknown tool argument")
        );

        lantern_task.abort();
    }
}
