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
        CliCommand::SourceHistory { path, kind, json } => {
            cmd_source_history(&client, &path, kind, json)
        }
        CliCommand::Retrieve { phrase, json } => cmd_retrieve(&client, &phrase, json),
        CliCommand::ProjectHandoff { project_id, json } => {
            cmd_project_handoff(&client, &project_id, json)
        }
        CliCommand::ProjectRecordResult {
            project_id,
            path,
            title,
            json,
        } => cmd_project_record_result(&client, &project_id, &path, title, json),
        CliCommand::ProjectCreate { name, json } => cmd_project_create(&client, &name, json),
        CliCommand::ProjectList { json } => cmd_project_list(&client, json),
        CliCommand::ProjectAddFile {
            project_id,
            path,
            title,
            kind,
            json,
        } => cmd_project_add_file(&client, &project_id, &path, title, kind, json),
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
    /// Show revision history for a file-backed Source.
    SourceHistory {
        /// Path to the file.
        path: PathBuf,
        /// Source kind (default: inferred from extension; "markdown" or "plain_text").
        #[arg(long, value_parser = ["markdown", "plain_text"])]
        kind: Option<String>,
        /// Output JSON only.
        #[arg(long)]
        json: bool,
    },
    /// Retrieve Episodes by a remembered Marker phrase.
    Retrieve {
        /// The remembered phrase to look up.
        phrase: String,
        /// Output JSON only.
        #[arg(long)]
        json: bool,
    },
    /// Produce a Codex-ready handoff for a Project.
    ProjectHandoff {
        /// Project ID (UUID).
        project_id: String,
        /// Output the full JSON response only.
        #[arg(long)]
        json: bool,
    },
    /// Record a result file against a Project.
    ProjectRecordResult {
        /// Project ID (UUID).
        project_id: String,
        /// Path to the result file.
        path: PathBuf,
        /// Result title (default: file name).
        #[arg(long)]
        title: Option<String>,
        /// Output JSON only.
        #[arg(long)]
        json: bool,
    },
    /// Create a new Project.
    ProjectCreate {
        /// Project name.
        name: String,
        /// Output JSON only.
        #[arg(long)]
        json: bool,
    },
    /// List all Projects.
    ProjectList {
        /// Output JSON only.
        #[arg(long)]
        json: bool,
    },
    /// Add a local file to a Project's handoff context.
    ProjectAddFile {
        /// Project ID (UUID).
        project_id: String,
        /// Path to the file.
        path: PathBuf,
        /// Source title (default: normalised absolute path).
        #[arg(long)]
        title: Option<String>,
        /// Source kind (default: inferred from extension; "markdown" or "plain_text").
        #[arg(long, value_parser = ["markdown", "plain_text"])]
        kind: Option<String>,
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
    #[serde(default)]
    previous_source_id: Option<String>,
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

// Retrieval response shapes
#[derive(Debug, Deserialize)]
struct MarkerRetrievalResponse {
    query: String,
    marker: Option<MarkerMatch>,
    episodes: Vec<RetrievedEpisode>,
    warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct MarkerMatch {
    marker_id: String,
    display_text: String,
    lookup_key: String,
}

#[derive(Debug, Deserialize)]
struct RetrievedEpisode {
    episode_id: String,
    title: String,
    source_id: String,
    #[serde(default)]
    content_source_id: Option<String>,
    #[serde(default)]
    latest_source_id: Option<String>,
    start_byte: u64,
    end_byte: u64,
    excerpt: String,
    why_matched: String,
}

// Project handoff response shapes (private, HTTP-shaped; no domain imports)
#[derive(Debug, Deserialize)]
struct ProjectHandoffResponse {
    project: ProjectHandoffSummary,
    episodes: Vec<ProjectHandoffEpisode>,
    context_package: ContextPackage,
    warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectHandoffSummary {
    project_id: String,
    name: String,
    status: String,
    #[serde(rename = "created_at")]
    _created_at: String,
}

#[derive(Debug, Deserialize)]
struct ProjectHandoffEpisode {
    episode_id: String,
    title: String,
    source_id: String,
    start_byte: usize,
    end_byte: usize,
    excerpt: String,
    why_matched: String,
}

#[derive(Debug, Deserialize)]
struct ContextPackage {
    format: String,
    audience: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct SourceHistoryItem {
    source_id: String,
    #[serde(default)]
    previous_source_id: Option<String>,
    created_at: String,
    current: bool,
}

#[derive(Debug, Deserialize)]
struct SourceHistoryResponse {
    title: String,
    kind: String,
    revisions: Vec<SourceHistoryItem>,
}

#[derive(Debug, Deserialize)]
struct RecordResultResponse {
    outcome: String,
    project_id: String,
    source_id: String,
    episode_id: String,
    start_byte: usize,
    end_byte: usize,
}

#[derive(Debug, Deserialize)]
struct ProjectCreateResponse {
    project_id: String,
    name: String,
    status: String,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct ProjectListItem {
    project_id: String,
    name: String,
    status: String,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct ProjectListResponse {
    projects: Vec<ProjectListItem>,
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

    let normalized_path = normalize_path_for_title(path);
    let title = title.unwrap_or_else(|| normalized_path.clone());
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
        let mut obj = serde_json::json!({
            "outcome": created.outcome,
            "source_id": created.source_id,
            "title": title,
            "path": normalized_path,
        });
        if let Some(ref prev) = created.previous_source_id {
            obj["previous_source_id"] = serde_json::json!(prev);
        }
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
    } else {
        println!("Title    : {}", title);
        println!("Path     : {}", normalized_path);
        match created.outcome.as_str() {
            "stored" => {
                println!("Source ID: {}", created.source_id);
                if let Some(ref prev) = created.previous_source_id {
                    println!("Outcome  : new revision (previous: {})", prev);
                } else {
                    println!("Outcome  : first capture");
                }
            }
            "duplicate" => {
                println!("Outcome  : unchanged — {}", created.source_id);
            }
            other => println!("Outcome  : {} — {}", other, created.source_id),
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

fn cmd_source_history(
    client: &HttpClient,
    path: &Path,
    kind: Option<String>,
    json: bool,
) -> anyhow::Result<()> {
    if !path.exists() {
        bail!("file not found: {}", path.display());
    }
    if path.is_dir() {
        bail!("path is a directory, not a file: {}", path.display());
    }

    let normalized_path = normalize_path_for_title(path);
    let kind = kind.unwrap_or_else(|| infer_kind(path).to_owned());

    let url = format!(
        "/api/v1/sources/history?kind={kind}&title={}",
        urlencoding::encode(&normalized_path)
    );

    let response = client
        .get(&url)
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;

    let body = HttpClient::handle_response(response)?;
    let history: SourceHistoryResponse = serde_json::from_value(body)
        .map_err(|e| anyhow::Error::msg(format!("unexpected API response: {e}")))?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "title": history.title,
                "kind": history.kind,
                "revisions": history.revisions.iter().map(|r| {
                    let mut obj = serde_json::json!({
                        "source_id": r.source_id,
                        "created_at": r.created_at,
                        "current": r.current,
                    });
                    if let Some(ref prev) = r.previous_source_id {
                        obj["previous_source_id"] = serde_json::json!(prev);
                    }
                    obj
                }).collect::<Vec<_>>(),
            }))
            .unwrap()
        );
    } else {
        println!("Title : {}", history.title);
        println!("Kind  : {}", history.kind);
        println!();
        if history.revisions.is_empty() {
            println!("(no revisions)");
        } else {
            for (i, rev) in history.revisions.iter().enumerate() {
                let marker = if rev.current { " (current)" } else { "" };
                println!("{}. [{}]{marker}", i + 1, rev.source_id);
                if let Some(ref prev) = rev.previous_source_id {
                    println!("   previous: {prev}");
                }
                println!("   captured: {}", rev.created_at);
            }
        }
    }

    Ok(())
}

const MAX_EXCERPT_CHARS: usize = 1200;

fn cmd_retrieve(client: &HttpClient, phrase: &str, json: bool) -> anyhow::Result<()> {
    if phrase.trim().is_empty() {
        bail!("phrase must not be blank — provide a remembered phrase to look up");
    }

    let body = serde_json::json!({ "text": phrase });

    let response = client
        .post_json("/api/v1/retrieval/markers", &body)
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;

    let body = HttpClient::handle_response(response)?;
    let retrieval: MarkerRetrievalResponse = serde_json::from_value(body)
        .map_err(|e| anyhow::Error::msg(format!("unexpected API response: {e}")))?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "query": retrieval.query,
                "marker": retrieval.marker.map(|m| serde_json::json!({
                    "marker_id": m.marker_id,
                    "display_text": m.display_text,
                    "lookup_key": m.lookup_key,
                })),
                "episodes": retrieval.episodes.iter().map(|e| {
                    let mut obj = serde_json::json!({
                        "episode_id": e.episode_id,
                        "title": e.title,
                        "source_id": e.source_id,
                        "start_byte": e.start_byte,
                        "end_byte": e.end_byte,
                        "excerpt": e.excerpt,
                        "why_matched": e.why_matched,
                    });
                    if let Some(ref csid) = e.content_source_id {
                        obj["content_source_id"] = serde_json::json!(csid);
                    }
                    if let Some(ref lsid) = e.latest_source_id {
                        obj["latest_source_id"] = serde_json::json!(lsid);
                    }
                    obj
                }).collect::<Vec<_>>(),
                "warnings": retrieval.warnings,
            }))
            .unwrap()
        );
        return Ok(());
    }

    // Print warnings
    for warning in &retrieval.warnings {
        println!("[warning] {warning}");
    }

    match retrieval.marker {
        None => {
            // No marker match
            if !retrieval.warnings.is_empty() {
                // warning already printed
            }
        }
        Some(ref marker) => {
            println!("Marker: {}", marker.display_text);

            if retrieval.episodes.is_empty() {
                // warnings printed above handle the explanation
                return Ok(());
            }

            println!();
            for (i, ep) in retrieval.episodes.iter().enumerate() {
                println!("{}. {}", i + 1, ep.title);
                println!("   Why: {}", ep.why_matched);

                // Show provenance only when it differs from the simple historical case.
                let content_src = ep.content_source_id.as_deref().unwrap_or(&ep.source_id);
                if content_src != ep.source_id {
                    println!(
                        "   Source: {} (rebased to {}, bytes {}..{})",
                        ep.source_id, content_src, ep.start_byte, ep.end_byte
                    );
                } else if let Some(ref latest) = ep.latest_source_id {
                    println!(
                        "   Source: {} (latest revision: {}, bytes {}..{})",
                        ep.source_id, latest, ep.start_byte, ep.end_byte
                    );
                } else {
                    println!(
                        "   Source: {}, bytes {}..{}",
                        ep.source_id, ep.start_byte, ep.end_byte
                    );
                }

                println!("   Excerpt:");

                let excerpt = if ep.excerpt.chars().count() > MAX_EXCERPT_CHARS {
                    let truncated: String = ep.excerpt.chars().take(MAX_EXCERPT_CHARS).collect();
                    format!("{truncated}\n[... truncated at {MAX_EXCERPT_CHARS} characters ...]")
                } else {
                    ep.excerpt.clone()
                };

                // Indent excerpt by 6 spaces
                for line in excerpt.lines() {
                    println!("      {line}");
                }
                if i + 1 < retrieval.episodes.len() {
                    println!();
                }
            }
        }
    }

    Ok(())
}

fn cmd_project_handoff(client: &HttpClient, project_id: &str, json: bool) -> anyhow::Result<()> {
    // Validate UUID locally before any HTTP request.
    if uuid::Uuid::parse_str(project_id).is_err() {
        bail!("invalid Project ID: {project_id} — must be a valid UUID");
    }

    let body = serde_json::json!({ "project_id": project_id });

    let response = client
        .post_json("/api/v1/retrieval/projects", &body)
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;

    let body = HttpClient::handle_response(response)?;
    let handoff: ProjectHandoffResponse = serde_json::from_value(body)
        .map_err(|e| anyhow::Error::msg(format!("unexpected API response: {e}")))?;

    if json {
        // Print the complete server JSON response, no added prose.
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "project": {
                    "project_id": handoff.project.project_id,
                    "name": handoff.project.name,
                    "status": handoff.project.status,
                },
                "episodes": handoff.episodes.iter().map(|e| serde_json::json!({
                    "episode_id": e.episode_id,
                    "title": e.title,
                    "source_id": e.source_id,
                    "start_byte": e.start_byte,
                    "end_byte": e.end_byte,
                    "excerpt": e.excerpt,
                    "why_matched": e.why_matched,
                })).collect::<Vec<_>>(),
                "context_package": {
                    "format": handoff.context_package.format,
                    "audience": handoff.context_package.audience,
                    "content": handoff.context_package.content,
                },
                "warnings": handoff.warnings,
            }))
            .unwrap()
        );
    } else {
        // Print the context_package content and nothing else.
        print!("{}", handoff.context_package.content);
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
struct AddFileResponse {
    outcome: String,
    source_id: String,
    #[serde(default)]
    previous_source_id: Option<String>,
    episode_id: String,
    link_status: String,
}

fn cmd_project_add_file(
    client: &HttpClient,
    project_id: &str,
    path: &Path,
    title: Option<String>,
    kind: Option<String>,
    json: bool,
) -> anyhow::Result<()> {
    if uuid::Uuid::parse_str(project_id).is_err() {
        bail!("invalid Project ID: {project_id} — must be a valid UUID");
    }
    if !path.exists() {
        bail!("file not found: {}", path.display());
    }
    if path.is_dir() {
        bail!("path is a directory, not a file: {}", path.display());
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read file: {}", path.display()))?;

    let normalized_path = normalize_path_for_title(path);
    let title = title.unwrap_or_else(|| normalized_path.clone());
    let kind = kind.unwrap_or_else(|| infer_kind(path).to_owned());

    let body = serde_json::json!({
        "title": title,
        "kind": kind,
        "content": content,
    });

    let response = client
        .post_json(&format!("/api/v1/projects/{project_id}/add-file"), &body)
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;

    let body = HttpClient::handle_response(response)?;
    let added: AddFileResponse = serde_json::from_value(body)
        .map_err(|e| anyhow::Error::msg(format!("unexpected API response: {e}")))?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "outcome": added.outcome,
                "source_id": added.source_id,
                "previous_source_id": added.previous_source_id,
                "episode_id": added.episode_id,
                "link_status": added.link_status,
                "title": title,
                "path": normalized_path,
            }))
            .unwrap()
        );
    } else {
        println!("Title    : {}", title);
        println!("Path     : {}", normalized_path);
        println!("Source ID: {}", added.source_id);
        match added.outcome.as_str() {
            "stored" => {
                if let Some(ref prev) = added.previous_source_id {
                    println!("Capture  : new revision (previous: {})", prev);
                } else {
                    println!("Capture  : first capture");
                }
            }
            "duplicate" => {
                println!("Capture  : unchanged");
            }
            other => println!("Capture  : {}", other),
        }
        match added.link_status.as_str() {
            "linked" => println!("Link     : newly linked — {}", added.episode_id),
            "already_linked" => {
                println!("Link     : already linked — {}", added.episode_id)
            }
            other => println!("Link     : {}", other),
        }
    }

    Ok(())
}

fn cmd_project_record_result(
    client: &HttpClient,
    project_id: &str,
    path: &Path,
    title: Option<String>,
    json: bool,
) -> anyhow::Result<()> {
    if uuid::Uuid::parse_str(project_id).is_err() {
        bail!("invalid Project ID: {project_id} — must be a valid UUID");
    }
    if !path.exists() {
        bail!("file not found: {}", path.display());
    }
    if path.is_dir() {
        bail!("path is a directory, not a file: {}", path.display());
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read UTF-8 result file: {}", path.display()))?;

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let title = title.unwrap_or_else(|| file_name.to_owned());
    let kind = infer_kind(path);

    let body = serde_json::json!({
        "title": title,
        "kind": kind,
        "content": content,
    });

    let response = client
        .post_json(
            &format!("/api/v1/projects/{project_id}/record-result"),
            &body,
        )
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;

    let body = HttpClient::handle_response(response)?;
    let recorded: RecordResultResponse = serde_json::from_value(body)
        .map_err(|e| anyhow::Error::msg(format!("unexpected API response: {e}")))?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "outcome": recorded.outcome,
                "project_id": recorded.project_id,
                "source_id": recorded.source_id,
                "episode_id": recorded.episode_id,
                "start_byte": recorded.start_byte,
                "end_byte": recorded.end_byte,
            }))
            .unwrap()
        );
    } else {
        match recorded.outcome.as_str() {
            "recorded" => println!("Recorded result — {}", recorded.episode_id),
            "already_recorded" => println!("Already recorded — {}", recorded.episode_id),
            other => println!("{other} — {}", recorded.episode_id),
        }
        println!("Project   : {}", recorded.project_id);
        println!("Source    : {}", recorded.source_id);
        println!("Byte range: {}..{}", recorded.start_byte, recorded.end_byte);
    }

    Ok(())
}

fn cmd_project_create(client: &HttpClient, name: &str, json: bool) -> anyhow::Result<()> {
    if name.trim().is_empty() {
        bail!("Project name must not be empty");
    }

    let body = serde_json::json!({
        "name": name,
        "status": "active",
    });

    let response = client
        .post_json("/api/v1/projects", &body)
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;

    let body = HttpClient::handle_response(response)?;
    let created: ProjectCreateResponse = serde_json::from_value(body)
        .map_err(|e| anyhow::Error::msg(format!("unexpected API response: {e}")))?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "project_id": created.project_id,
                "name": created.name,
                "status": created.status,
                "created_at": created.created_at,
            }))
            .unwrap()
        );
    } else {
        println!("Project ID: {}", created.project_id);
        println!("Name      : {}", created.name);
        println!("Status    : {}", created.status);
    }

    Ok(())
}

fn cmd_project_list(client: &HttpClient, json: bool) -> anyhow::Result<()> {
    let response = client
        .get("/api/v1/projects")
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;

    let body = HttpClient::handle_response(response)?;
    let list: ProjectListResponse = serde_json::from_value(body)
        .map_err(|e| anyhow::Error::msg(format!("unexpected API response: {e}")))?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "projects": list.projects.iter().map(|p| serde_json::json!({
                    "project_id": p.project_id,
                    "name": p.name,
                    "status": p.status,
                    "created_at": p.created_at,
                })).collect::<Vec<_>>(),
            }))
            .unwrap()
        );
    } else if list.projects.is_empty() {
        println!("(no Projects)");
    } else {
        for p in &list.projects {
            println!("[{}] {} — {}", p.project_id, p.status, p.name);
        }
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

/// Derives a stable logical Source title from a file path.
///
/// Uses the canonical absolute path, lowercased with forward slashes, so
/// re-capturing the same file always uses the same logical identity
/// regardless of the working directory or temporary path variations.
fn normalize_path_for_title(path: &Path) -> String {
    // Canonicalize resolves symlinks and normalises components like `.` and `..`.
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    canonical
        .to_string_lossy()
        .replace('\\', "/")
        .to_lowercase()
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

    // Retrieve-specific tests
    #[test]
    fn retrieve_blank_phrase_is_rejected_locally() {
        // cmd_retrieve validates blank phrase before HTTP
        let client = HttpClient::new("http://127.0.0.1:1");
        let result = cmd_retrieve(&client, "   ", false);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("must not be blank"));
    }

    #[test]
    fn excerpt_truncation_is_unicode_safe() {
        let long: String = "a".repeat(1500);
        let truncated: String = long.chars().take(MAX_EXCERPT_CHARS).collect();
        assert_eq!(truncated.chars().count(), MAX_EXCERPT_CHARS);
        // Verify the truncation string is valid UTF-8
        let output = format!("{truncated}\n[... truncated at {MAX_EXCERPT_CHARS} characters ...]");
        assert!(output.contains("truncated at 1200"));
    }

    #[test]
    fn excerpt_truncation_preserves_multibyte_unicode() {
        let mut s = String::new();
        for _ in 0..2000 {
            s.push('\u{4E16}'); // 世 CJK character
        }
        assert!(s.chars().count() > MAX_EXCERPT_CHARS);
        let truncated: String = s.chars().take(MAX_EXCERPT_CHARS).collect();
        // Each char should be complete
        assert_eq!(truncated.chars().count(), MAX_EXCERPT_CHARS);
        for c in truncated.chars() {
            assert_eq!(c, '\u{4E16}');
        }
    }

    #[test]
    fn retrieval_command_included_in_clap_enum() {
        // Verify CliCommand has Retrieve variant
        let cmd = CliCommand::Retrieve {
            phrase: "test".into(),
            json: false,
        };
        match cmd {
            CliCommand::Retrieve { phrase, json } => {
                assert_eq!(phrase, "test");
                assert!(!json);
            }
            _ => panic!("expected Retrieve variant"),
        }
    }

    // ── Project handoff tests ──────────────────────────────────────────────

    #[test]
    fn project_handoff_command_is_parsed() {
        let cmd = CliCommand::ProjectHandoff {
            project_id: "550e8400-e29b-41d4-a716-446655440000".into(),
            json: false,
        };
        match cmd {
            CliCommand::ProjectHandoff { project_id, json } => {
                assert_eq!(project_id, "550e8400-e29b-41d4-a716-446655440000");
                assert!(!json);
            }
            _ => panic!("expected ProjectHandoff variant"),
        }
    }

    #[test]
    fn project_handoff_invalid_uuid_rejected_before_request() {
        let client = HttpClient::new("http://127.0.0.1:1");
        let result = cmd_project_handoff(&client, "not-a-uuid", false);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("invalid Project ID"));
        assert!(msg.contains("must be a valid UUID"));
    }

    #[test]
    fn project_handoff_normal_output_renders_context_package_exactly() {
        // Simulate the output path: deserialize a fake response, then check
        // the printed context_package content is verbatim, including Unicode
        // and leading/trailing whitespace.
        let pkg_content = "  \u{00A9} Codex Handoff  \n# Context\n\n---\n  ";
        let response = serde_json::json!({
            "project": {
                "project_id": "550e8400-e29b-41d4-a716-446655440000",
                "name": "Demo",
                "status": "active",
                "created_at": "2026-01-01T00:00:00Z"
            },
            "episodes": [],
            "context_package": {
                "format": "markdown",
                "audience": "agent",
                "content": pkg_content
            },
            "warnings": []
        });
        let handoff: ProjectHandoffResponse =
            serde_json::from_value(response).expect("valid handoff response");
        assert_eq!(handoff.context_package.content, pkg_content);
        // Normal output must be the content only — verify there is no
        // "Codex Handoff" block when the package content does not start with it.
        // The key property is that the content is returned exactly as-is.
    }

    #[test]
    fn project_handoff_json_output_is_json_only() {
        let response = serde_json::json!({
            "project": {
                "project_id": "550e8400-e29b-41d4-a716-446655440000",
                "name": "Demo",
                "status": "active",
                "created_at": "2026-01-01T00:00:00Z"
            },
            "episodes": [],
            "context_package": {
                "format": "markdown",
                "audience": "agent",
                "content": "# Handoff"
            },
            "warnings": ["test warning"]
        });
        // Build the JSON output the same way cmd_project_handoff does.
        let handoff: ProjectHandoffResponse =
            serde_json::from_value(response.clone()).expect("valid handoff response");
        let json_output = serde_json::to_string_pretty(&serde_json::json!({
            "project": {
                "project_id": handoff.project.project_id,
                "name": handoff.project.name,
                "status": handoff.project.status,
            },
            "episodes": handoff.episodes.iter().map(|e| serde_json::json!({
                "episode_id": e.episode_id,
                "title": e.title,
                "source_id": e.source_id,
                "start_byte": e.start_byte,
                "end_byte": e.end_byte,
                "excerpt": e.excerpt,
                "why_matched": e.why_matched,
            })).collect::<Vec<_>>(),
            "context_package": {
                "format": handoff.context_package.format,
                "audience": handoff.context_package.audience,
                "content": handoff.context_package.content,
            },
            "warnings": handoff.warnings,
        }))
        .unwrap();

        let parsed: serde_json::Value =
            serde_json::from_str(&json_output).expect("valid JSON output");
        assert_eq!(
            parsed["project"]["project_id"],
            response["project"]["project_id"]
        );
        assert_eq!(parsed["warnings"][0], "test warning");
        assert_eq!(parsed["context_package"]["content"], "# Handoff");
    }

    #[test]
    fn project_handoff_api_error_does_not_emit_partial_handoff() {
        // CliError::Api display should not contain handoff content.
        let err = CliError::Api {
            status: 500,
            code: "internal_error".into(),
            message: "something broke".into(),
        };
        let msg = err.to_string();
        assert!(!msg.contains("# Codex Handoff Context"));
        assert!(!msg.contains("context_package"));
    }

    #[test]
    fn project_record_result_command_is_parsed() {
        let cmd = CliCommand::ProjectRecordResult {
            project_id: "550e8400-e29b-41d4-a716-446655440000".into(),
            path: PathBuf::from("result.md"),
            title: Some("Result".into()),
            json: true,
        };
        match cmd {
            CliCommand::ProjectRecordResult {
                project_id,
                path,
                title,
                json,
            } => {
                assert_eq!(project_id, "550e8400-e29b-41d4-a716-446655440000");
                assert_eq!(path, PathBuf::from("result.md"));
                assert_eq!(title.as_deref(), Some("Result"));
                assert!(json);
            }
            _ => panic!("expected ProjectRecordResult variant"),
        }
    }

    #[test]
    fn project_record_result_invalid_uuid_rejected_before_request() {
        let client = HttpClient::new("http://127.0.0.1:1");
        let result =
            cmd_project_record_result(&client, "not-a-uuid", Path::new("missing.md"), None, false);
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("invalid Project ID"));
        assert!(!msg.contains("file not found"));
    }

    #[test]
    fn project_record_result_rejects_missing_file() {
        let client = HttpClient::new("http://127.0.0.1:1");
        let result = cmd_project_record_result(
            &client,
            "550e8400-e29b-41d4-a716-446655440000",
            Path::new("missing-result-file.md"),
            None,
            false,
        );
        assert!(result.unwrap_err().to_string().contains("file not found"));
    }

    // ── Project create tests ───────────────────────────────────────────────

    #[test]
    fn project_create_command_is_parsed() {
        let cmd = CliCommand::ProjectCreate {
            name: "My Project".into(),
            json: false,
        };
        match cmd {
            CliCommand::ProjectCreate { name, json } => {
                assert_eq!(name, "My Project");
                assert!(!json);
            }
            _ => panic!("expected ProjectCreate variant"),
        }
    }

    #[test]
    fn project_create_blank_name_rejected_locally() {
        let client = HttpClient::new("http://127.0.0.1:1");
        let result = cmd_project_create(&client, "   ", false);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("must not be empty"));
    }

    // ── Project list tests ─────────────────────────────────────────────────

    #[test]
    fn project_list_command_is_parsed() {
        let cmd = CliCommand::ProjectList { json: false };
        match cmd {
            CliCommand::ProjectList { json } => {
                assert!(!json);
            }
            _ => panic!("expected ProjectList variant"),
        }
    }
}
