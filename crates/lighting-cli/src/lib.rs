//! Library for Lighting CLI commands.
//!
//! Exports the HTTP-client-side commands (`health`, `source add`, `source show`)
//! so that the unified `lighting` binary can use them directly.  The standalone
//! `lighting-cli` binary still works via a thin `main.rs` shim.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context};
use clap::Subcommand;
use serde::Deserialize;

/// The Lighting service port the CLI connects to by default.
///
/// Must match the actual service default in `apps/lighting/src/main.rs`.
pub const DEFAULT_SERVICE_PORT: u16 = 4317;

const HTTP_TIMEOUT_SECS: u64 = 10;

// ---------------------------------------------------------------------------
// Public command dispatch
// ---------------------------------------------------------------------------

/// Runs the CLI subcommands that talk to the Lighting HTTP API.
///
/// Called by the unified `lighting` binary (and by the standalone
/// `lighting-cli` binary).
pub fn run_cli_command(service_url: &str, command: CliCommand) -> anyhow::Result<()> {
    let client = HttpClient::new(service_url);

    match command {
        CliCommand::Health { json } => cmd_health(&client, json),
        CliCommand::SourceAdd {
            path,
            title,
            kind,
            json,
        } => cmd_source_add(&client, &path, title, kind, json),
        CliCommand::SourceShow { source_id, json } => cmd_source_show(&client, &source_id, json),
    }
}

/// CLI commands that talk to the Lighting HTTP API.
///
/// Embedded inside the top-level `lighting` binary's command enum.
#[derive(Debug, Subcommand, Clone)]
pub enum CliCommand {
    /// Check Lighting service readiness.
    Health {
        /// Output JSON only.
        #[arg(long)]
        json: bool,
    },
    /// Add a Source from a local file.
    SourceAdd {
        /// Path to the file.
        path: PathBuf,
        /// Source title (default: file name).
        #[arg(long)]
        title: Option<String>,
        /// Source kind (default: inferred from extension; "markdown" or "plain_text").
        #[arg(long, value_parser = ["markdown", "plain_text"])]
        kind: Option<String>,
        /// Output JSON only.
        #[arg(long)]
        json: bool,
    },
    /// Show a Source by ID.
    SourceShow {
        /// Source ID (UUID).
        source_id: String,
        /// Output JSON only.
        #[arg(long)]
        json: bool,
    },
}

// ---------------------------------------------------------------------------
// HTTP client
// ---------------------------------------------------------------------------

struct HttpClient {
    client: reqwest::blocking::Client,
    base_url: String,
}

impl HttpClient {
    fn new(base_url: &str) -> Self {
        Self {
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
                .build()
                .expect("http client should build"),
            base_url: base_url.trim_end_matches('/').to_owned(),
        }
    }

    fn get(&self, path: &str) -> Result<reqwest::blocking::Response, CliError> {
        let url = format!("{}{path}", self.base_url);
        self.client
            .get(&url)
            .send()
            .map_err(|e| CliError::Connection {
                url: url.clone(),
                source: e,
            })
    }

    fn post_json(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<reqwest::blocking::Response, CliError> {
        let url = format!("{}{path}", self.base_url);
        self.client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .map_err(|e| CliError::Connection {
                url: url.clone(),
                source: e,
            })
    }

    fn handle_response(
        response: reqwest::blocking::Response,
    ) -> Result<serde_json::Value, CliError> {
        let status = response.status();
        let body: serde_json::Value = response
            .json()
            .map_err(|e| CliError::Decode(e.to_string()))?;

        if status.is_success() {
            return Ok(body);
        }

        let api_err = ApiError::from_value(&body);
        Err(CliError::Api {
            status: status.as_u16(),
            code: api_err.code,
            message: api_err.message,
        })
    }
}

// ---------------------------------------------------------------------------
// CLI error types
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("Lighting is unavailable at {url}: {source}")]
    Connection {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("Lighting returned {status} ({code}): {message}")]
    Api {
        status: u16,
        code: String,
        message: String,
    },
    #[error("failed to decode API response: {0}")]
    Decode(String),
}

// ---------------------------------------------------------------------------
// API response shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    code: String,
    message: String,
}

struct ApiError {
    code: String,
    message: String,
}

impl ApiError {
    fn from_value(value: &serde_json::Value) -> Self {
        match serde_json::from_value::<ApiErrorBody>(value.clone()) {
            Ok(body) => Self {
                code: body.code,
                message: body.message,
            },
            Err(_) => Self {
                code: "unknown".to_owned(),
                message: format!("{value}"),
            },
        }
    }
}

#[derive(Debug, Deserialize)]
struct CreateSourceResponse {
    outcome: String,
    source_id: String,
}

#[derive(Debug, Deserialize)]
struct SourceResponse {
    source_id: String,
    title: String,
    kind: String,
    content: String,
    fingerprint: String,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct ReadinessResponse {
    ready: bool,
    reason: String,
}

// ---------------------------------------------------------------------------
// Command implementations
// ---------------------------------------------------------------------------

fn cmd_health(client: &HttpClient, json: bool) -> anyhow::Result<()> {
    let response = client
        .get("/health/ready")
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;

    let body = HttpClient::handle_response(response)?;
    let readiness: ReadinessResponse = serde_json::from_value(body)
        .map_err(|e| anyhow::Error::msg(format!("unexpected health response: {e}")))?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "ready": readiness.ready,
                "reason": readiness.reason,
            }))
            .unwrap()
        );
    } else if readiness.ready {
        println!("Lighting is ready — {}", readiness.reason);
    } else {
        println!("Lighting is NOT ready — {}", readiness.reason);
    }

    if !readiness.ready {
        std::process::exit(1);
    }

    Ok(())
}

fn cmd_source_add(
    client: &HttpClient,
    path: &Path,
    title: Option<String>,
    kind: Option<String>,
    json: bool,
) -> anyhow::Result<()> {
    if !path.exists() {
        bail!("file not found: {}", path.display());
    }
    if path.is_dir() {
        bail!("path is a directory, not a file: {}", path.display());
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read file: {}", path.display()))?;

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let title = title.unwrap_or_else(|| file_name.to_owned());
    let kind = kind.unwrap_or_else(|| infer_kind(path).to_owned());

    let body = serde_json::json!({
        "title": title,
        "kind": kind,
        "content": content,
    });

    let response = client
        .post_json("/api/v1/sources", &body)
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;

    let body = HttpClient::handle_response(response)?;
    let created: CreateSourceResponse = serde_json::from_value(body)
        .map_err(|e| anyhow::Error::msg(format!("unexpected API response: {e}")))?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "outcome": created.outcome,
                "source_id": created.source_id,
            }))
            .unwrap()
        );
    } else {
        match created.outcome.as_str() {
            "stored" => println!("Stored — {}", created.source_id),
            "duplicate" => println!("Already exists — {}", created.source_id),
            other => println!("{} — {}", other, created.source_id),
        }
    }

    Ok(())
}

fn cmd_source_show(client: &HttpClient, source_id: &str, json: bool) -> anyhow::Result<()> {
    if uuid::Uuid::parse_str(source_id).is_err() {
        bail!("invalid Source ID: {source_id} — must be a valid UUID");
    }

    let response = client
        .get(&format!("/api/v1/sources/{source_id}"))
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;

    let body = HttpClient::handle_response(response)?;
    let source: SourceResponse = serde_json::from_value(body)
        .map_err(|e| anyhow::Error::msg(format!("unexpected API response: {e}")))?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "source_id": source.source_id,
                "title": source.title,
                "kind": source.kind,
                "content": source.content,
                "fingerprint": source.fingerprint,
                "created_at": source.created_at,
            }))
            .unwrap()
        );
    } else {
        println!("Source ID : {}", source.source_id);
        println!("Title     : {}", source.title);
        println!("Kind      : {}", source.kind);
        println!("Fingerprint: {}", source.fingerprint);
        println!("Created   : {}", source.created_at);
        println!();
        println!("--- Content ---");
        print!("{}", source.content);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Builds the default service URL using the documented Lighting port.
pub fn default_service_url() -> String {
    format!("http://127.0.0.1:{DEFAULT_SERVICE_PORT}")
}

pub fn validate_service_url(url: &str) -> Result<String, String> {
    let url = url.trim().to_owned();
    let host = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .and_then(|rest| rest.split(':').next())
        .unwrap_or(&url);

    if host != "127.0.0.1" && host != "localhost" {
        return Err(format!(
            "service URL host must be 127.0.0.1 or localhost, got: {host}"
        ));
    }
    Ok(url)
}

fn infer_kind(path: &Path) -> &str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("md") | Some("markdown") => "markdown",
        _ => "plain_text",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_kind_markdown_from_md() {
        assert_eq!(infer_kind(Path::new("readme.md")), "markdown");
    }

    #[test]
    fn infer_kind_markdown_from_markdown() {
        assert_eq!(infer_kind(Path::new("notes.markdown")), "markdown");
    }

    #[test]
    fn infer_kind_plain_text_from_txt() {
        assert_eq!(infer_kind(Path::new("notes.txt")), "plain_text");
    }

    #[test]
    fn infer_kind_plain_text_no_extension() {
        assert_eq!(infer_kind(Path::new("README")), "plain_text");
    }

    #[test]
    fn title_defaults_to_file_name() {
        let path = Path::new("my-notes.md");
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap();
        assert_eq!(file_name, "my-notes.md");
    }

    #[test]
    fn validate_service_url_accepts_localhost() {
        assert!(validate_service_url("http://localhost:9417").is_ok());
        assert!(validate_service_url("http://127.0.0.1:4317").is_ok());
    }

    #[test]
    fn validate_service_url_rejects_remote() {
        assert!(validate_service_url("http://192.168.1.1:9417").is_err());
        assert!(validate_service_url("http://example.com").is_err());
    }

    #[test]
    fn validate_service_url_rejects_empty() {
        assert!(validate_service_url("http://0.0.0.0:9417").is_err());
    }

    #[test]
    fn invalid_uuid_is_caught_before_request() {
        assert!(uuid::Uuid::parse_str("not-a-uuid").is_err());
        assert!(uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000000").is_ok());
    }

    #[test]
    fn json_output_does_not_contain_prose() {
        let output = serde_json::to_string_pretty(&serde_json::json!({
            "outcome": "stored",
            "source_id": "abc-123",
        }))
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["outcome"], "stored");
        assert_eq!(parsed["source_id"], "abc-123");
    }

    #[test]
    fn api_error_parse_preserves_code_and_message() {
        let body = serde_json::json!({
            "code": "invalid_source",
            "message": "Source title must not be empty"
        });
        let err = ApiError::from_value(&body);
        assert_eq!(err.code, "invalid_source");
        assert_eq!(err.message, "Source title must not be empty");
    }

    #[test]
    fn error_output_does_not_leak_source_secret() {
        let secret = "xyzzy-secret-cli-marker";
        let body = serde_json::json!({
            "code": "invalid_source",
            "message": "Source title must not be empty"
        });
        let err = ApiError::from_value(&body);
        assert!(!err.message.contains(secret));
        assert!(!err.code.contains(secret));
    }

    #[test]
    fn default_service_url_uses_documented_port() {
        let url = default_service_url();
        assert_eq!(url, "http://127.0.0.1:4317");
    }
}
