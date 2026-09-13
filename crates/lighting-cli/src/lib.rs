//! Library for Lighting CLI commands.
//!
//! Exports the HTTP-client-side commands (`health`, `source add`, `source show`)
//! so that the unified `lighting` binary can use them directly.  The standalone
//! `lighting-cli` binary still works via a thin `main.rs` shim.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, bail};
use chrono::Utc;
use clap::Subcommand;
use serde::{Deserialize, Serialize};

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
        CliCommand::Remember {
            content,
            kind,
            project_id,
            confidence,
            importance,
            derived_from,
            supersedes,
            contradicts,
            supports,
            agent,
            json,
        } => cmd_remember(
            &client,
            RememberInput {
                content: &content,
                kind: &kind,
                project_id: project_id.as_deref(),
                confidence,
                importance,
                derived_from,
                supersedes,
                contradicts,
                supports,
                agent: &agent,
                json,
            },
        ),
        CliCommand::Recall {
            project_id,
            phrase,
            include_inactive,
            json,
        } => cmd_recall(
            &client,
            project_id.as_deref(),
            phrase.as_deref(),
            include_inactive,
            json,
        ),
        CliCommand::Context {
            project_id,
            query,
            json,
        } => cmd_context(&client, project_id.as_deref(), query.as_deref(), json),
        CliCommand::ContextPack {
            query,
            actor,
            item_budget,
            json,
        } => cmd_context_pack(&client, &query, actor.as_deref(), item_budget, json),
        CliCommand::MemorySupersede { memory_id, json } => {
            cmd_memory_supersede(&client, &memory_id, json)
        }
        CliCommand::LedgerIngest { path, json } => cmd_ledger_ingest(&client, &path, json),
        CliCommand::BasicMemoryImport {
            path,
            dry_run,
            json,
        } => cmd_basic_memory_import(&client, &path, dry_run, json),
        CliCommand::BasicMemoryAccounting { path, output, json } => {
            cmd_basic_memory_accounting(&path, &output, json)
        }
        CliCommand::Predicate { command } => match command {
            PredicateCommand::List { json } => cmd_predicate_list(&client, json),
            PredicateCommand::Get { key, json } => cmd_predicate_get(&client, &key, json),
            PredicateCommand::Aliases { key, json } => cmd_predicate_aliases(&client, &key, json),
            PredicateCommand::Unmapped { json } => cmd_predicate_unmapped(&client, json),
        },
        CliCommand::Belief { command } => match command {
            BeliefCommand::Get { belief_id, json } => cmd_belief_get(&client, &belief_id, json),
            BeliefCommand::Search {
                query,
                include_stale,
                json,
            } => cmd_belief_search(&client, &query, include_stale, json),
            BeliefCommand::History { belief_id, json } => {
                cmd_belief_history(&client, &belief_id, json)
            }
            BeliefCommand::Explain { belief_id, json } => {
                cmd_belief_explain(&client, &belief_id, json)
            }
            BeliefCommand::Stale { json } => cmd_belief_stale(&client, json),
        },
        CliCommand::CorrectionRecord {
            target_belief_id,
            correction_text,
            replacement_value,
            json,
        } => cmd_correction_record(
            &client,
            &target_belief_id,
            &correction_text,
            &replacement_value,
            json,
        ),
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
        CliCommand::ProjectShow { project_id, json } => {
            cmd_project_show(&client, &project_id, json)
        }
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
    /// Record a derived, provenance-bearing Memory.
    Remember {
        /// Memory content supplied by the agent.
        content: String,
        /// One of fact, decision, preference, instruction, lesson, gotcha, open_loop, workflow, summary, entity.
        #[arg(long)]
        kind: String,
        #[arg(long)]
        project_id: Option<String>,
        #[arg(long, default_value_t = 0.8)]
        confidence: f32,
        #[arg(long, default_value_t = 0.5)]
        importance: f32,
        #[arg(long)]
        derived_from: Vec<String>,
        #[arg(long)]
        supersedes: Vec<String>,
        #[arg(long)]
        contradicts: Vec<String>,
        #[arg(long)]
        supports: Vec<String>,
        #[arg(long, default_value = "lucy")]
        agent: String,
        #[arg(long)]
        json: bool,
    },
    /// Recall active Memory records using lexical/project filters.
    Recall {
        #[arg(long)]
        project_id: Option<String>,
        #[arg(long)]
        phrase: Option<String>,
        #[arg(long)]
        include_inactive: bool,
        #[arg(long)]
        json: bool,
    },
    /// Build a compact working context from active Memory records.
    Context {
        #[arg(long)]
        project_id: Option<String>,
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Compile a typed deterministic Context Pack from epistemic memory.
    ContextPack {
        query: String,
        #[arg(long)]
        actor: Option<String>,
        #[arg(long, default_value_t = 10)]
        item_budget: usize,
        #[arg(long)]
        json: bool,
    },
    /// Mark a Memory as superseded while retaining its history.
    MemorySupersede {
        memory_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Ingest one event or an {"events": [...]} / [...] JSON export into the source ledger.
    LedgerIngest {
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Import a validated Basic Memory snapshot as Source evidence and ledger metadata.
    BasicMemoryImport {
        /// Snapshot directory or one notes-*.ndjson shard.
        path: PathBuf,
        /// Validate and report the snapshot without contacting Lighting.
        #[arg(long)]
        dry_run: bool,
        /// Output JSON only.
        #[arg(long)]
        json: bool,
    },
    /// Produce deterministic accounting for every observation and relation in a Basic Memory snapshot.
    BasicMemoryAccounting {
        /// Validated snapshot directory.
        path: PathBuf,
        /// JSON report destination.
        #[arg(long)]
        output: PathBuf,
        /// Also print the report as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Inspect the canonical predicate registry and unmapped claim queue.
    Predicate {
        #[command(subcommand)]
        command: PredicateCommand,
    },
    /// Inspect reconciled Belief projections and their evidence.
    Belief {
        #[command(subcommand)]
        command: BeliefCommand,
    },
    /// Record a direct Matthew correction against one active Belief.
    CorrectionRecord {
        target_belief_id: String,
        correction_text: String,
        replacement_value: String,
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
    /// Inspect a Project and its linked Episodes.
    ProjectShow {
        /// Project ID (UUID).
        project_id: String,
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

#[derive(Debug, Subcommand, Clone)]
pub enum PredicateCommand {
    /// List registered predicates.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show one registered predicate by canonical key or alias.
    Get {
        key: String,
        #[arg(long)]
        json: bool,
    },
    /// Show aliases for one registered predicate.
    Aliases {
        key: String,
        #[arg(long)]
        json: bool,
    },
    /// List claims that have no resolved canonical predicate.
    Unmapped {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand, Clone)]
pub enum BeliefCommand {
    /// Get one belief projection.
    Get {
        belief_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Search belief projections by deterministic lexical matching.
    Search {
        query: String,
        #[arg(long)]
        include_stale: bool,
        #[arg(long)]
        json: bool,
    },
    /// Show immutable revisions for one belief.
    History {
        belief_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Explain one belief from stored revisions and Claims.
    Explain {
        belief_id: String,
        #[arg(long)]
        json: bool,
    },
    /// List stale belief projections.
    Stale {
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

    fn get_query(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<reqwest::blocking::Response, CliError> {
        let url = format!("{}{path}", self.base_url);
        self.client
            .get(&url)
            .query(query)
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

#[derive(Debug, Deserialize)]
struct ProjectShowEpisodeEntry {
    episode_id: String,
    link_kind: String,
    source_id: String,
    start_byte: usize,
    end_byte: usize,
    source_title: String,
    source_kind: String,
    #[serde(default)]
    latest_source_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectShowResponse {
    project_id: String,
    name: String,
    status: String,
    created_at: String,
    episodes: Vec<ProjectShowEpisodeEntry>,
}

// ---------------------------------------------------------------------------
// Command implementations
// ---------------------------------------------------------------------------

fn cmd_predicate_list(client: &HttpClient, json: bool) -> anyhow::Result<()> {
    let response = client
        .get("/api/v1/predicates")
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;
    let body = HttpClient::handle_response(response)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&body)?);
    } else if let Some(predicates) = body["predicates"].as_array() {
        if predicates.is_empty() {
            println!("(no registered predicates)");
        } else {
            for predicate in predicates {
                println!(
                    "{} [{}]",
                    predicate["key"].as_str().unwrap_or("<invalid>"),
                    predicate["status"].as_str().unwrap_or("unknown")
                );
            }
        }
    }
    Ok(())
}

fn cmd_predicate_get(client: &HttpClient, key: &str, json: bool) -> anyhow::Result<()> {
    let response = client
        .get(&format!("/api/v1/predicates/{}", urlencoding::encode(key)))
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;
    let body = HttpClient::handle_response(response)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&body)?);
    } else {
        let predicate = &body["predicate"];
        println!(
            "Key         : {}",
            predicate["key"].as_str().unwrap_or("<invalid>")
        );
        println!(
            "Status      : {}",
            predicate["status"].as_str().unwrap_or("unknown")
        );
        println!(
            "Value type  : {}",
            predicate["value_type"].as_str().unwrap_or("unknown")
        );
        println!("Aliases     : {}", predicate["aliases"]);
        println!(
            "Description : {}",
            predicate["description"].as_str().unwrap_or("")
        );
    }
    Ok(())
}

fn cmd_predicate_aliases(client: &HttpClient, key: &str, json: bool) -> anyhow::Result<()> {
    let response = client
        .get(&format!("/api/v1/predicates/{}", urlencoding::encode(key)))
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;
    let body = HttpClient::handle_response(response)?;
    let aliases = body["predicate"]["aliases"].clone();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({"aliases": aliases}))?
        );
    } else {
        println!("{}", aliases);
    }
    Ok(())
}

fn cmd_predicate_unmapped(client: &HttpClient, json: bool) -> anyhow::Result<()> {
    let response = client
        .get("/api/v1/claims?unmapped=true")
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;
    let body = HttpClient::handle_response(response)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&body)?);
    } else if let Some(claims) = body["claims"].as_array() {
        println!("Unmapped claims: {}", claims.len());
        for claim in claims {
            println!(
                "- {}: {}",
                claim["predicate_candidate"].as_str().unwrap_or("<none>"),
                claim["value"].as_str().unwrap_or("<invalid>")
            );
        }
    }
    Ok(())
}

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

struct RememberInput<'a> {
    content: &'a str,
    kind: &'a str,
    project_id: Option<&'a str>,
    confidence: f32,
    importance: f32,
    derived_from: Vec<String>,
    supersedes: Vec<String>,
    contradicts: Vec<String>,
    supports: Vec<String>,
    agent: &'a str,
    json: bool,
}

#[derive(Debug, Deserialize)]
struct BasicMemorySnapshotRecord {
    source_project: Option<String>,
    source_project_id: Option<String>,
    source_path: String,
    title: Option<String>,
    permalink: Option<String>,
    external_id: Option<String>,
    entity_id: Option<i64>,
    note_type: Option<String>,
    content_type: Option<String>,
    updated_at: Option<String>,
    raw_markdown: String,
}

#[derive(Debug, Serialize)]
struct BasicMemoryImportSummary {
    source_records: usize,
    sources_stored: usize,
    sources_duplicate: usize,
    episodes_submitted: usize,
    claim_candidates_submitted: usize,
    memory_items_submitted: usize,
    relations_submitted: usize,
    ledger_events_stored: usize,
    ledger_events_duplicate: usize,
    observation_events_stored: usize,
    observation_events_duplicate: usize,
    relation_events_stored: usize,
    relation_events_duplicate: usize,
}

const BASIC_MEMORY_ACCOUNTING_VERSION: &str = "basic-memory-accounting-v1";

#[derive(Debug, Deserialize)]
struct BasicMemorySnapshotManifest {
    captured_at: Option<String>,
    note_count: Option<usize>,
    observation_count: Option<usize>,
    relation_count: Option<usize>,
    export_sha256: Option<String>,
}

#[derive(Debug, Serialize)]
struct BasicMemoryAccountingItem {
    source_id: String,
    source_path: String,
    line_number: usize,
    item_type: String,
    raw_value: String,
    outcome: String,
    reason: String,
    importer_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
}

#[derive(Debug, Serialize)]
struct BasicMemoryAccountingReport {
    schema: &'static str,
    importer_version: &'static str,
    snapshot_captured_at: Option<String>,
    snapshot_export_sha256: Option<String>,
    source_records: usize,
    expected_observations: Option<usize>,
    accounted_observations: usize,
    expected_relations: Option<usize>,
    accounted_relations: usize,
    unexplained_observations: usize,
    unexplained_relations: usize,
    observation_outcomes: BTreeMap<String, usize>,
    relation_outcomes: BTreeMap<String, usize>,
    items: Vec<BasicMemoryAccountingItem>,
}

fn load_basic_memory_snapshot(path: &Path) -> anyhow::Result<Vec<BasicMemorySnapshotRecord>> {
    let files = if path.is_file() {
        vec![path.to_path_buf()]
    } else if path.is_dir() {
        let manifest_path = path.join("manifest.json");
        let manifest_text = fs::read_to_string(&manifest_path).with_context(|| {
            format!(
                "failed to read snapshot manifest {}",
                manifest_path.display()
            )
        })?;
        let manifest: serde_json::Value =
            serde_json::from_str(&manifest_text).with_context(|| {
                format!(
                    "snapshot manifest is not valid JSON: {}",
                    manifest_path.display()
                )
            })?;
        let expected_count = manifest
            .get("note_count")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| anyhow::anyhow!("snapshot manifest has no numeric note_count"))?;
        let mut files: Vec<PathBuf> = fs::read_dir(path)
            .with_context(|| format!("failed to list snapshot directory {}", path.display()))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<_, _>>()
            .with_context(|| format!("failed to inspect snapshot directory {}", path.display()))?;
        files.retain(|candidate| {
            candidate
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("notes-") && name.ends_with(".ndjson"))
        });
        files.sort();
        if files.is_empty() {
            bail!("snapshot directory contains no notes-*.ndjson shards");
        }
        let records = read_basic_memory_shards(&files)?;
        if records.len() as u64 != expected_count {
            bail!(
                "snapshot manifest expects {expected_count} notes but shards contain {}",
                records.len()
            );
        }
        return Ok(records);
    } else {
        bail!("snapshot path does not exist: {}", path.display());
    };

    read_basic_memory_shards(&files)
}

fn read_basic_memory_shards(files: &[PathBuf]) -> anyhow::Result<Vec<BasicMemorySnapshotRecord>> {
    let mut records = Vec::new();
    let mut paths = std::collections::BTreeSet::new();
    for file in files {
        let text = fs::read_to_string(file)
            .with_context(|| format!("failed to read snapshot shard {}", file.display()))?;
        for (line_number, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let record: BasicMemorySnapshotRecord =
                serde_json::from_str(line).with_context(|| {
                    format!(
                        "snapshot shard {} line {} is not a valid Basic Memory record",
                        file.display(),
                        line_number + 1
                    )
                })?;
            if record.source_path.trim().is_empty() {
                bail!(
                    "snapshot record in {} has a blank source_path",
                    file.display()
                );
            }
            if record.raw_markdown.is_empty() {
                bail!(
                    "snapshot record {} has an empty raw_markdown body",
                    record.source_path
                );
            }
            if !paths.insert(record.source_path.clone()) {
                bail!(
                    "snapshot contains duplicate source_path {}",
                    record.source_path
                );
            }
            records.push(record);
        }
    }
    records.sort_by(|left, right| left.source_path.cmp(&right.source_path));
    Ok(records)
}

fn load_basic_memory_snapshot_manifest(path: &Path) -> anyhow::Result<BasicMemorySnapshotManifest> {
    if !path.is_dir() {
        bail!(
            "accounting requires a snapshot directory: {}",
            path.display()
        );
    }
    let manifest_path = path.join("manifest.json");
    let manifest_text = fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "failed to read snapshot manifest {}",
            manifest_path.display()
        )
    })?;
    serde_json::from_str(&manifest_text).with_context(|| {
        format!(
            "snapshot manifest is not valid JSON: {}",
            manifest_path.display()
        )
    })
}

fn basic_memory_body_lines(markdown: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut in_frontmatter = false;
    let mut frontmatter_seen = false;
    markdown.lines().enumerate().filter(move |(_, line)| {
        if line.trim() == "---" {
            if !frontmatter_seen {
                frontmatter_seen = true;
                in_frontmatter = true;
            } else {
                in_frontmatter = false;
            }
            return false;
        }
        frontmatter_seen && !in_frontmatter
    })
}

fn basic_memory_source_id(record: &BasicMemorySnapshotRecord) -> String {
    record
        .external_id
        .clone()
        .unwrap_or_else(|| format!("basic-memory:source:{}", record.source_path))
}

fn classify_accounted_observation(category: &str) -> (&'static str, String) {
    let category = category.trim().to_ascii_lowercase();
    if is_truth_bearing_basic_memory_category(&category) {
        return (
            "claim",
            "truth-bearing category retained as a Claim candidate; predicate resolution remains separate".to_owned(),
        );
    }
    if category == "history" || category == "historical" {
        return (
            "historical_memory",
            "historical category retained as dated source-backed memory".to_owned(),
        );
    }
    if matches!(
        category.as_str(),
        "anecdote"
            | "concept"
            | "creative_seed"
            | "humour"
            | "idea"
            | "impression"
            | "insight"
            | "lesson"
            | "memory"
            | "negative_constraint"
            | "open_loop"
            | "pattern"
            | "pattern_candidate"
            | "principle"
            | "quote"
            | "reference"
            | "rejected_path"
            | "strength_observation"
            | "tension"
    ) {
        return (
            "soft_memory",
            "recognized soft-memory category retained without promoting it to canonical truth"
                .to_owned(),
        );
    }
    (
        "source_only_intentional",
        format!(
            "category `{category}` has no dedicated canonical import mapping; exact text remains in the Source and ledger"
        ),
    )
}

fn first_basic_memory_target(line: &str) -> Option<(String, bool)> {
    let start = line.find("[[")?;
    let rest = &line[start + 2..];
    let end = rest.find("]]")?;
    let target = rest[..end].trim();
    if target.is_empty() {
        return None;
    }
    let multiple = rest[end + 2..].contains("[[");
    Some((target.to_owned(), multiple))
}

fn basic_memory_target_key(target: &str) -> String {
    target
        .split_once('|')
        .map(|(path, _)| path)
        .unwrap_or(target)
        .trim()
        .trim_end_matches(".md")
        .replace('\\', "/")
        .to_ascii_lowercase()
}

fn basic_memory_known_target_keys(
    records: &[BasicMemorySnapshotRecord],
) -> std::collections::BTreeSet<String> {
    records
        .iter()
        .flat_map(|record| {
            [
                Some(record.source_path.as_str()),
                record.title.as_deref(),
                record.permalink.as_deref(),
            ]
            .into_iter()
            .flatten()
            .map(basic_memory_target_key)
        })
        .collect()
}

fn build_basic_memory_accounting(
    records: &[BasicMemorySnapshotRecord],
    manifest: &BasicMemorySnapshotManifest,
) -> BasicMemoryAccountingReport {
    let known_targets = basic_memory_known_target_keys(records);
    let mut items = Vec::new();
    let mut observation_outcomes = BTreeMap::new();
    let mut relation_outcomes = BTreeMap::new();
    let mut accounted_observations = 0;
    let mut accounted_relations = 0;

    for record in records {
        let source_id = basic_memory_source_id(record);
        let metadata_source = record
            .source_path
            .to_ascii_lowercase()
            .ends_with("index.md")
            || record
                .note_type
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case("index"));
        let historical_source = record
            .source_path
            .to_ascii_lowercase()
            .starts_with("archive/")
            || record
                .source_path
                .to_ascii_lowercase()
                .starts_with("history/");

        for (line_index, line) in basic_memory_body_lines(&record.raw_markdown) {
            let raw_value = line.trim();
            if raw_value.starts_with("- [[") {
                if first_basic_memory_target(raw_value).is_some() {
                    let reason =
                        "index/navigation link retained as source metadata and relation evidence";
                    *observation_outcomes
                        .entry("metadata".to_owned())
                        .or_insert(0) += 1;
                    accounted_observations += 1;
                    items.push(BasicMemoryAccountingItem {
                        source_id: source_id.clone(),
                        source_path: record.source_path.clone(),
                        line_number: line_index + 1,
                        item_type: "observation".to_owned(),
                        raw_value: raw_value.to_owned(),
                        outcome: "metadata".to_owned(),
                        reason: reason.to_owned(),
                        importer_version: BASIC_MEMORY_ACCOUNTING_VERSION.to_owned(),
                        category: None,
                        target: first_basic_memory_target(raw_value).map(|(target, _)| target),
                    });
                }
            } else if let Some(value) = raw_value.strip_prefix("- [")
                && let Some((category, content)) = value.split_once("] ")
                && !category.trim().is_empty()
                && !content.trim().is_empty()
            {
                let (outcome, reason) = classify_accounted_observation(category);
                *observation_outcomes.entry(outcome.to_owned()).or_insert(0) += 1;
                accounted_observations += 1;
                items.push(BasicMemoryAccountingItem {
                    source_id: source_id.clone(),
                    source_path: record.source_path.clone(),
                    line_number: line_index + 1,
                    item_type: "observation".to_owned(),
                    raw_value: raw_value.to_owned(),
                    outcome: outcome.to_owned(),
                    reason,
                    importer_version: BASIC_MEMORY_ACCOUNTING_VERSION.to_owned(),
                    category: Some(category.trim().to_owned()),
                    target: None,
                });
            }

            if raw_value.starts_with("- ")
                && let Some((target, multiple)) = first_basic_memory_target(raw_value)
            {
                let target_key = basic_memory_target_key(&target);
                let (outcome, reason) = if multiple {
                    (
                            "unsupported_with_reason",
                            "one legacy bullet contains multiple wiki targets; raw line is preserved without guessing a single edge".to_owned(),
                        )
                } else if !known_targets.contains(&target_key) {
                    (
                        "unresolved_target",
                        "wiki target does not match any imported source path, title or permalink"
                            .to_owned(),
                    )
                } else if metadata_source {
                    (
                            "metadata_only",
                            "index/navigation relation is retained as metadata rather than treated as semantic project evidence".to_owned(),
                        )
                } else if historical_source {
                    (
                        "historical_relation",
                        "relation is retained with historical source context".to_owned(),
                    )
                } else {
                    (
                        "resolved_relation",
                        "wiki target resolves to an imported source path, title or permalink"
                            .to_owned(),
                    )
                };
                *relation_outcomes.entry(outcome.to_owned()).or_insert(0) += 1;
                accounted_relations += 1;
                items.push(BasicMemoryAccountingItem {
                    source_id: source_id.clone(),
                    source_path: record.source_path.clone(),
                    line_number: line_index + 1,
                    item_type: "relation".to_owned(),
                    raw_value: raw_value.to_owned(),
                    outcome: outcome.to_owned(),
                    reason,
                    importer_version: BASIC_MEMORY_ACCOUNTING_VERSION.to_owned(),
                    category: raw_value
                        .strip_prefix("- ")
                        .and_then(|value| value.split_once("[["))
                        .map(|(predicate, _)| predicate.trim().to_owned())
                        .filter(|value| !value.is_empty()),
                    target: Some(target),
                });
            }
        }
    }

    let unexplained_observations = manifest
        .observation_count
        .map(|expected| expected.saturating_sub(accounted_observations))
        .unwrap_or(0);
    let unexplained_relations = manifest
        .relation_count
        .map(|expected| expected.saturating_sub(accounted_relations))
        .unwrap_or(0);

    BasicMemoryAccountingReport {
        schema: "lantern.basic-memory-accounting/1",
        importer_version: BASIC_MEMORY_ACCOUNTING_VERSION,
        snapshot_captured_at: manifest.captured_at.clone(),
        snapshot_export_sha256: manifest.export_sha256.clone(),
        source_records: records.len(),
        expected_observations: manifest.observation_count,
        accounted_observations,
        expected_relations: manifest.relation_count,
        accounted_relations,
        unexplained_observations,
        unexplained_relations,
        observation_outcomes,
        relation_outcomes,
        items,
    }
}

fn cmd_basic_memory_accounting(path: &Path, output: &Path, json: bool) -> anyhow::Result<()> {
    let manifest = load_basic_memory_snapshot_manifest(path)?;
    let records = load_basic_memory_snapshot(path)?;
    if manifest
        .note_count
        .is_some_and(|expected| expected != records.len())
    {
        bail!(
            "snapshot manifest note_count does not match records: expected {:?}, actual {}",
            manifest.note_count,
            records.len()
        );
    }
    let report = build_basic_memory_accounting(&records, &manifest);
    if report.unexplained_observations != 0 || report.unexplained_relations != 0 {
        bail!(
            "snapshot accounting is incomplete: unexplained observations={}, relations={}",
            report.unexplained_observations,
            report.unexplained_relations
        );
    }
    let encoded = serde_json::to_vec_pretty(&report)?;
    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create accounting report directory {}",
                parent.display()
            )
        })?;
    }
    fs::write(output, encoded)
        .with_context(|| format!("failed to write accounting report {}", output.display()))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "Basic Memory accounting: {} notes, {}/{} observations, {}/{} relations, unexplained=0",
            report.source_records,
            report.accounted_observations,
            report
                .expected_observations
                .unwrap_or(report.accounted_observations),
            report.accounted_relations,
            report
                .expected_relations
                .unwrap_or(report.accounted_relations),
        );
        println!("Report: {}", output.display());
    }
    Ok(())
}

fn cmd_basic_memory_import(
    client: &HttpClient,
    path: &Path,
    dry_run: bool,
    json: bool,
) -> anyhow::Result<()> {
    let records = load_basic_memory_snapshot(path)?;
    if dry_run {
        let result = serde_json::json!({
            "dry_run": true,
            "source_records": records.len(),
            "source_paths": records.iter().map(|record| record.source_path.as_str()).collect::<Vec<_>>()
        });
        if json {
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else {
            println!(
                "Basic Memory snapshot is valid: {} source records",
                records.len()
            );
        }
        return Ok(());
    }

    let mut summary = BasicMemoryImportSummary {
        source_records: records.len(),
        sources_stored: 0,
        sources_duplicate: 0,
        episodes_submitted: 0,
        claim_candidates_submitted: 0,
        memory_items_submitted: 0,
        relations_submitted: 0,
        ledger_events_stored: 0,
        ledger_events_duplicate: 0,
        observation_events_stored: 0,
        observation_events_duplicate: 0,
        relation_events_stored: 0,
        relation_events_duplicate: 0,
    };

    for record in records {
        let source_response = client
            .post_json(
                "/api/v1/sources",
                &serde_json::json!({
                    "title": format!("basic-memory/{}", record.source_path),
                    "kind": "markdown",
                    "content": record.raw_markdown,
                }),
            )
            .map_err(|error| {
                anyhow::Error::msg(error).context("is Lighting running? Try: lighting serve")
            })?;
        let source_body = HttpClient::handle_response(source_response)?;
        let source_id = source_body
            .get("source_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("source import response has no source_id"))?;
        match source_body
            .get("outcome")
            .and_then(serde_json::Value::as_str)
        {
            Some("duplicate") => summary.sources_duplicate += 1,
            Some("stored") => summary.sources_stored += 1,
            other => bail!("source import response has unexpected outcome {other:?}"),
        }

        let episode_body = client
            .post_json(
                "/api/v1/episodes",
                &serde_json::json!({
                    "title": format!("Basic Memory whole note: {}", record.source_path),
                    "source_id": source_id,
                    "start_byte": 0,
                    "end_byte": record.raw_markdown.len(),
                }),
            )
            .map_err(|error| {
                anyhow::Error::msg(error).context("failed to create imported Episode")
            })?;
        let episode_body = HttpClient::handle_response(episode_body)?;
        let episode_id = episode_body
            .get("episode_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("episode import response has no episode_id"))?;
        summary.episodes_submitted += 1;

        let upstream_key = record
            .external_id
            .clone()
            .unwrap_or_else(|| record.source_path.clone());
        let mut metadata = BTreeMap::new();
        metadata.insert("source_path".to_owned(), record.source_path.clone());
        metadata.insert("source_id".to_owned(), source_id.to_owned());
        if let Some(value) = &record.title {
            metadata.insert("source_title".to_owned(), value.clone());
        }
        if let Some(value) = &record.permalink {
            metadata.insert("permalink".to_owned(), value.clone());
        }
        if let Some(value) = &record.note_type {
            metadata.insert("note_type".to_owned(), value.clone());
        }
        if let Some(value) = &record.content_type {
            metadata.insert("content_type".to_owned(), value.clone());
        }
        if let Some(value) = &record.entity_id {
            metadata.insert("entity_id".to_owned(), value.to_string());
        }
        if let Some(value) = &record.source_project {
            metadata.insert("source_project".to_owned(), value.clone());
        }
        if let Some(value) = &record.source_project_id {
            metadata.insert("source_project_id".to_owned(), value.clone());
        }
        let note_event = post_basic_memory_ledger_event(
            client,
            format!("basic-memory:{upstream_key}"),
            record.external_id.as_deref(),
            format!("Imported Basic Memory note {}", record.source_path),
            record.updated_at.as_deref(),
            metadata.clone(),
        )?;
        if note_event {
            summary.ledger_events_duplicate += 1;
        } else {
            summary.ledger_events_stored += 1;
        }

        for (index, (category, content)) in extract_basic_memory_observations(&record.raw_markdown)
            .into_iter()
            .enumerate()
        {
            let mut observation_metadata = metadata.clone();
            observation_metadata.insert("record_kind".to_owned(), "observation".to_owned());
            observation_metadata.insert("observation_index".to_owned(), index.to_string());
            observation_metadata.insert("category".to_owned(), category.clone());
            let duplicate = post_basic_memory_ledger_event(
                client,
                format!("basic-memory:{upstream_key}:observation:{index}"),
                record.external_id.as_deref(),
                format!(
                    "Imported Basic Memory observation [{category}] from {}: {content}",
                    record.source_path
                ),
                record.updated_at.as_deref(),
                observation_metadata,
            )?;
            if duplicate {
                summary.observation_events_duplicate += 1;
            } else {
                summary.observation_events_stored += 1;
            }

            if is_truth_bearing_basic_memory_category(&category) {
                post_basic_memory_claim(
                    client, &record, source_id, episode_id, &category, &content,
                )?;
                summary.claim_candidates_submitted += 1;
            } else {
                post_basic_memory_item(client, source_id, episode_id, &category, &content)?;
                summary.memory_items_submitted += 1;
            }
        }

        for (index, (predicate, target)) in extract_basic_memory_relations(&record.raw_markdown)
            .into_iter()
            .enumerate()
        {
            let mut relation_metadata = metadata.clone();
            relation_metadata.insert("record_kind".to_owned(), "relation".to_owned());
            relation_metadata.insert("relation_index".to_owned(), index.to_string());
            relation_metadata.insert("predicate".to_owned(), predicate.clone());
            relation_metadata.insert("target".to_owned(), target.clone());
            let duplicate = post_basic_memory_ledger_event(
                client,
                format!("basic-memory:{upstream_key}:relation:{index}"),
                record.external_id.as_deref(),
                format!(
                    "Imported Basic Memory relation {predicate} -> [[{target}]] from {}",
                    record.source_path
                ),
                record.updated_at.as_deref(),
                relation_metadata,
            )?;
            if duplicate {
                summary.relation_events_duplicate += 1;
            } else {
                summary.relation_events_stored += 1;
            }
            post_basic_memory_relation(client, episode_id, &predicate, &target)?;
            summary.relations_submitted += 1;
        }
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        println!(
            "Basic Memory import: {} notes, {} sources stored, {} sources already present, {} whole-note Episodes submitted, {} Claim candidates submitted, {} soft Memory Items submitted, {} relations submitted, {} note events stored, {} note events already present, {} observations stored, {} observations already present, {} relations stored, {} relations already present",
            summary.source_records,
            summary.sources_stored,
            summary.sources_duplicate,
            summary.episodes_submitted,
            summary.claim_candidates_submitted,
            summary.memory_items_submitted,
            summary.relations_submitted,
            summary.ledger_events_stored,
            summary.ledger_events_duplicate,
            summary.observation_events_stored,
            summary.observation_events_duplicate,
            summary.relation_events_stored,
            summary.relation_events_duplicate
        );
    }
    Ok(())
}

fn is_truth_bearing_basic_memory_category(category: &str) -> bool {
    matches!(
        category,
        "decision" | "rule" | "current" | "preference" | "constraint" | "problem" | "opportunity"
    )
}

fn post_basic_memory_claim(
    client: &HttpClient,
    record: &BasicMemorySnapshotRecord,
    source_id: &str,
    episode_id: &str,
    category: &str,
    content: &str,
) -> anyhow::Result<()> {
    let response = client
        .post_json(
            "/api/v1/claims",
            &serde_json::json!({
                "source_id": source_id,
                "episode_id": episode_id,
                "subject_key": format!("legacy:basic-memory:{}", record.source_path),
                "value": content,
                "predicate_candidate": category,
                "predicate_status": "candidate",
                "originator_actor_id": "basic-memory",
                "speaker_actor_id": "basic-memory",
                "transmitter_actor_id": "basic-memory",
                "holder_actor_id": "legacy:shared",
                "stance": "unobserved",
                "confidence": 0.35,
                "extractor": "basic-memory-import",
                "extractor_version": "1",
            }),
        )
        .map_err(|error| {
            anyhow::Error::msg(error).context("failed to import Basic Memory Claim candidate")
        })?;
    HttpClient::handle_response(response)?;
    Ok(())
}

fn post_basic_memory_item(
    client: &HttpClient,
    source_id: &str,
    episode_id: &str,
    category: &str,
    content: &str,
) -> anyhow::Result<()> {
    let kind = match category {
        "idea" | "concept" => "idea",
        "history" => "anecdote",
        "quote" => "quote",
        "lesson" => "lesson",
        _ => "other",
    };
    let response = client
        .post_json(
            "/api/v1/memory-items",
            &serde_json::json!({
                "source_id": source_id,
                "episode_id": episode_id,
                "kind": kind,
                "content": content,
                "originator_actor_id": "unknown",
                "transmitter_actor_id": "basic-memory",
                "holder_actor_id": "legacy:shared",
                "salience": 0.4,
            }),
        )
        .map_err(|error| {
            anyhow::Error::msg(error).context("failed to import Basic Memory soft Memory Item")
        })?;
    HttpClient::handle_response(response)?;
    Ok(())
}

fn post_basic_memory_relation(
    client: &HttpClient,
    episode_id: &str,
    relation_type: &str,
    target: &str,
) -> anyhow::Result<()> {
    let response = client
        .post_json(
            "/api/v1/relations",
            &serde_json::json!({
                "in_id": episode_id,
                "out_id": format!("basic-memory:{target}"),
                "relation_type": relation_type,
                "origin": "basic-memory-cloud",
                "confidence": 1.0,
                "resolved": false,
            }),
        )
        .map_err(|error| {
            anyhow::Error::msg(error).context("failed to import Basic Memory relation")
        })?;
    HttpClient::handle_response(response)?;
    Ok(())
}

fn post_basic_memory_ledger_event(
    client: &HttpClient,
    event_id: String,
    external_id: Option<&str>,
    content: String,
    observed_at: Option<&str>,
    metadata: BTreeMap<String, String>,
) -> anyhow::Result<bool> {
    let raw_payload = serde_json::to_string(&metadata)?;
    let response = client
        .post_json(
            "/api/v1/ledger/events",
            &serde_json::json!({
                "event_id": event_id,
                "source": "basic-memory-cloud",
                "external_id": external_id,
                "session_id": null,
                "conversation_id": null,
                "turn_id": null,
                "actor": "basic-memory-import",
                "role": "tool",
                "content": content,
                "observed_at": observed_at,
                "received_at": Utc::now().to_rfc3339(),
                "reply_to": null,
                "project_hint": "Lantern",
                "idempotency_key": event_id,
                "raw_payload": raw_payload,
                "metadata": metadata,
            }),
        )
        .map_err(|error| {
            anyhow::Error::msg(error).context("failed to record migration ledger event")
        })?;
    let body = HttpClient::handle_response(response)?;
    body.get("duplicate")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| anyhow::anyhow!("ledger import response has no duplicate field"))
}

fn extract_basic_memory_observations(markdown: &str) -> Vec<(String, String)> {
    markdown
        .lines()
        .filter_map(|line| {
            let value = line.trim().strip_prefix("- [")?;
            let (category, content) = value.split_once("] ")?;
            let category = category.trim();
            let content = content.trim();
            if category.is_empty() || content.is_empty() {
                return None;
            }
            Some((category.to_owned(), content.to_owned()))
        })
        .collect()
}

fn extract_basic_memory_relations(markdown: &str) -> Vec<(String, String)> {
    markdown
        .lines()
        .filter_map(|line| {
            let value = line.trim().strip_prefix("- ")?;
            let (predicate, target) = value.split_once(" [[")?;
            let target = target.strip_suffix("]]")?.trim();
            let predicate = predicate.trim();
            if predicate.is_empty() || target.is_empty() {
                return None;
            }
            Some((predicate.to_owned(), target.to_owned()))
        })
        .collect()
}

fn cmd_ledger_ingest(client: &HttpClient, path: &Path, json: bool) -> anyhow::Result<()> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read ledger event file {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("ledger event file is not valid JSON: {}", path.display()))?;
    let events = match value {
        serde_json::Value::Array(events) => events,
        serde_json::Value::Object(mut object) => object
            .remove("events")
            .and_then(|events| events.as_array().cloned())
            .ok_or_else(|| anyhow::anyhow!("ledger JSON object must contain an events array"))?,
        _ => bail!("ledger JSON must be an event array or an object containing events"),
    };
    let mut results = Vec::new();
    for event in events {
        let response = client
            .post_json("/api/v1/ledger/events", &event)
            .map_err(|e| {
                anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve")
            })?;
        results.push(HttpClient::handle_response(response)?);
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        let duplicates = results
            .iter()
            .filter(|result| result["duplicate"].as_bool().unwrap_or(false))
            .count();
        println!(
            "Ledger events accepted: {} (duplicates: {duplicates})",
            results.len()
        );
    }
    Ok(())
}

fn cmd_remember(client: &HttpClient, input: RememberInput<'_>) -> anyhow::Result<()> {
    if input.content.trim().is_empty() {
        bail!("memory content must not be blank");
    }
    let mut request = serde_json::json!({
        "content": input.content,
        "kind": input.kind,
        "confidence": input.confidence,
        "importance": input.importance,
        "derived_from": input.derived_from,
        "supersedes": input.supersedes,
        "contradicts": input.contradicts,
        "supports": input.supports,
        "agent": input.agent,
    });
    if let Some(project_id) = input.project_id {
        request["project_id"] = serde_json::json!(project_id);
    }

    let response = client
        .post_json("/api/v1/memories", &request)
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;
    let body = HttpClient::handle_response(response)?;
    if input.json {
        println!("{}", serde_json::to_string_pretty(&body).unwrap());
    } else {
        let memory = body
            .get("memory")
            .ok_or_else(|| anyhow::anyhow!("unexpected API response: {body}"))?;
        println!("Memory ID : {}", memory["id"]);
        println!("Kind      : {}", memory["kind"]);
        println!("Status    : {}", memory["status"]);
        println!("Content   : {}", memory["content"]);
        println!("Agent     : {}", memory["agent"]);
        println!("Confidence: {}", memory["confidence"]);
        println!("Recorded  : {}", memory["recorded_at"]);
    }
    Ok(())
}

fn cmd_recall(
    client: &HttpClient,
    project_id: Option<&str>,
    phrase: Option<&str>,
    include_inactive: bool,
    json: bool,
) -> anyhow::Result<()> {
    let body = serde_json::json!({
        "project_id": project_id,
        "phrase": phrase,
        "include_inactive": include_inactive,
    });
    let response = client
        .post_json("/api/v1/memories/recall", &body)
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;
    let body = HttpClient::handle_response(response)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&body).unwrap());
        return Ok(());
    }
    if body["abstained"].as_bool().unwrap_or(false) {
        println!("I do not have reliable memory of this.");
        if let Some(reason) = body["reason"].as_str() {
            println!("{reason}");
        }
        return Ok(());
    }
    for memory in body["memories"].as_array().into_iter().flatten() {
        println!(
            "- [{} | {} | confidence {}] {}",
            memory["kind"], memory["status"], memory["confidence"], memory["content"]
        );
    }
    Ok(())
}

fn cmd_context(
    client: &HttpClient,
    project_id: Option<&str>,
    query: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    let body = serde_json::json!({"project_id": project_id, "query": query});
    let response = client
        .post_json("/api/v1/memories/context", &body)
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;
    let body = HttpClient::handle_response(response)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&body).unwrap());
    } else if let Some(context) = body["context"].as_str() {
        print!("{context}");
    }
    Ok(())
}

fn cmd_context_pack(
    client: &HttpClient,
    query: &str,
    actor: Option<&str>,
    item_budget: usize,
    json: bool,
) -> anyhow::Result<()> {
    if query.trim().is_empty() {
        bail!("context query must not be blank");
    }
    let response = client
        .post_json(
            "/api/v1/epistemic/context",
            &serde_json::json!({
                "query": query,
                "actor": actor,
                "item_budget": item_budget,
            }),
        )
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;
    let body = HttpClient::handle_response(response)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&body)?);
    } else if let Some(context) = body["context"]["generated_context"].as_str() {
        print!("{context}");
    }
    Ok(())
}

fn cmd_belief_get(client: &HttpClient, belief_id: &str, json: bool) -> anyhow::Result<()> {
    let response = client
        .get(&format!("/api/v1/beliefs/{belief_id}"))
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;
    let body = HttpClient::handle_response(response)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&body)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&body["belief"])?);
    }
    Ok(())
}

fn cmd_belief_search(
    client: &HttpClient,
    query: &str,
    include_stale: bool,
    json: bool,
) -> anyhow::Result<()> {
    let response = client
        .get_query(
            "/api/v1/beliefs/search",
            &[
                ("query", query),
                (
                    "include_stale",
                    if include_stale { "true" } else { "false" },
                ),
            ],
        )
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;
    let body = HttpClient::handle_response(response)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&body)?);
    } else {
        for belief in body["beliefs"].as_array().into_iter().flatten() {
            println!(
                "- [{}] {}.{} = {}{}",
                belief["holder_key"],
                belief["subject_key"],
                belief["predicate_key"],
                belief["current_value"],
                if belief["stale"].as_bool().unwrap_or(false) {
                    " [STALE]"
                } else {
                    ""
                }
            );
        }
    }
    Ok(())
}

fn cmd_belief_history(client: &HttpClient, belief_id: &str, json: bool) -> anyhow::Result<()> {
    let response = client
        .get(&format!("/api/v1/beliefs/{belief_id}/history"))
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;
    let body = HttpClient::handle_response(response)?;
    let value = if json { &body } else { &body["revisions"] };
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn cmd_belief_explain(client: &HttpClient, belief_id: &str, json: bool) -> anyhow::Result<()> {
    let response = client
        .get(&format!("/api/v1/beliefs/{belief_id}/explain"))
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;
    let body = HttpClient::handle_response(response)?;
    println!("{}", serde_json::to_string_pretty(&body)?);
    if !json {
        println!("Belief explanation returned with stored Claims and revisions.");
    }
    Ok(())
}

fn cmd_belief_stale(client: &HttpClient, json: bool) -> anyhow::Result<()> {
    let response = client
        .get("/api/v1/beliefs/stale")
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;
    let body = HttpClient::handle_response(response)?;
    println!("{}", serde_json::to_string_pretty(&body)?);
    if !json {
        println!("Stale projections are excluded from current Context Packs.");
    }
    Ok(())
}

fn cmd_correction_record(
    client: &HttpClient,
    target_belief_id: &str,
    correction_text: &str,
    replacement_value: &str,
    json: bool,
) -> anyhow::Result<()> {
    if correction_text.trim().is_empty() || replacement_value.trim().is_empty() {
        bail!("correction text and replacement value must not be blank");
    }
    let response = client
        .post_json(
            "/api/v1/corrections",
            &serde_json::json!({
                "target_belief_id": target_belief_id,
                "correction_text": correction_text,
                "replacement_value": replacement_value,
            }),
        )
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;
    let body = HttpClient::handle_response(response)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&body)?);
    } else {
        println!("Correction recorded for belief {target_belief_id}.");
        println!(
            "{}",
            body["correction"]["reconciliation"]["decision"]["action"]
        );
    }
    Ok(())
}

fn cmd_memory_supersede(client: &HttpClient, memory_id: &str, json: bool) -> anyhow::Result<()> {
    let response = client
        .post_json(
            &format!("/api/v1/memories/{memory_id}/supersede"),
            &serde_json::json!({}),
        )
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;
    let body = HttpClient::handle_response(response)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&body).unwrap());
    } else {
        println!("Memory superseded: {memory_id}");
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

fn cmd_project_show(client: &HttpClient, project_id: &str, json: bool) -> anyhow::Result<()> {
    if uuid::Uuid::parse_str(project_id).is_err() {
        bail!("invalid Project ID: {project_id} — must be a valid UUID");
    }

    let response = client
        .get(&format!("/api/v1/projects/{project_id}"))
        .map_err(|e| anyhow::Error::msg(e).context("is Lighting running? Try: lighting serve"))?;

    let body = HttpClient::handle_response(response)?;
    let show: ProjectShowResponse = serde_json::from_value(body)
        .map_err(|e| anyhow::Error::msg(format!("unexpected API response: {e}")))?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "project_id": show.project_id,
                "name": show.name,
                "status": show.status,
                "created_at": show.created_at,
                "episodes": show.episodes.iter().map(|e| {
                    let mut obj = serde_json::json!({
                        "episode_id": e.episode_id,
                        "link_kind": e.link_kind,
                        "source_id": e.source_id,
                        "start_byte": e.start_byte,
                        "end_byte": e.end_byte,
                        "source_title": e.source_title,
                        "source_kind": e.source_kind,
                    });
                    if let Some(ref lsid) = e.latest_source_id {
                        obj["latest_source_id"] = serde_json::json!(lsid);
                    }
                    obj
                }).collect::<Vec<_>>(),
            }))
            .unwrap()
        );
    } else {
        println!("Project ID : {}", show.project_id);
        println!("Name       : {}", show.name);
        println!("Status     : {}", show.status);
        println!();

        if show.episodes.is_empty() {
            println!("Episodes   : (none)");
        } else {
            for (i, ep) in show.episodes.iter().enumerate() {
                println!(
                    "{}. [{}] {} ({} bytes {}..{})",
                    i + 1,
                    ep.episode_id,
                    ep.source_title,
                    ep.source_kind,
                    ep.start_byte,
                    ep.end_byte,
                );
                println!("   link  : {}", ep.link_kind);
                println!("   source: {}", ep.source_id);
                if let Some(ref lsid) = ep.latest_source_id {
                    println!("   latest: {lsid}");
                }
            }
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
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must not be blank")
        );
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

    #[test]
    fn basic_memory_extractors_preserve_unknown_categories_and_relation_targets() {
        let markdown = "# Note\n\n- [decision] Keep the source intact\n- [unusual-label] Preserve this too\n- related_to [[Lantern Keeper]]\n- governed_by [[A title with spaces]]\n";

        assert_eq!(
            extract_basic_memory_observations(markdown),
            vec![
                ("decision".to_owned(), "Keep the source intact".to_owned()),
                ("unusual-label".to_owned(), "Preserve this too".to_owned())
            ]
        );
        assert_eq!(
            extract_basic_memory_relations(markdown),
            vec![
                ("related_to".to_owned(), "Lantern Keeper".to_owned()),
                ("governed_by".to_owned(), "A title with spaces".to_owned())
            ]
        );
    }

    #[test]
    fn basic_memory_accounting_covers_tagged_and_index_items() {
        let records = vec![
            BasicMemorySnapshotRecord {
                source_project: None,
                source_project_id: None,
                source_path: "index.md".to_owned(),
                title: Some("Index".to_owned()),
                permalink: Some("index".to_owned()),
                external_id: Some("index-source".to_owned()),
                entity_id: None,
                note_type: Some("index".to_owned()),
                content_type: Some("text/markdown".to_owned()),
                updated_at: None,
                raw_markdown: "---\ntitle: Index\n---\n\n# Index\n\n- [[Known]]\n".to_owned(),
            },
            BasicMemorySnapshotRecord {
                source_project: None,
                source_project_id: None,
                source_path: "Known.md".to_owned(),
                title: Some("Known".to_owned()),
                permalink: Some("known".to_owned()),
                external_id: Some("known-source".to_owned()),
                entity_id: None,
                note_type: Some("note".to_owned()),
                content_type: Some("text/markdown".to_owned()),
                updated_at: None,
                raw_markdown:
                    "---\ntitle: Known\n---\n\n- [decision] Keep it\n- related_to [[Index]]\n"
                        .to_owned(),
            },
        ];
        let manifest = BasicMemorySnapshotManifest {
            captured_at: Some("2026-09-12".to_owned()),
            note_count: Some(2),
            observation_count: Some(2),
            relation_count: Some(2),
            export_sha256: None,
        };

        let report = build_basic_memory_accounting(&records, &manifest);

        assert_eq!(report.accounted_observations, 2);
        assert_eq!(report.accounted_relations, 2);
        assert_eq!(report.unexplained_observations, 0);
        assert_eq!(report.unexplained_relations, 0);
        assert_eq!(report.observation_outcomes["metadata"], 1);
        assert_eq!(report.observation_outcomes["claim"], 1);
        assert_eq!(report.relation_outcomes["metadata_only"], 1);
        assert_eq!(report.relation_outcomes["resolved_relation"], 1);
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

    // ── Project show tests ─────────────────────────────────────────────────

    #[test]
    fn project_show_command_is_parsed() {
        let cmd = CliCommand::ProjectShow {
            project_id: "550e8400-e29b-41d4-a716-446655440000".into(),
            json: false,
        };
        match cmd {
            CliCommand::ProjectShow { project_id, json } => {
                assert_eq!(project_id, "550e8400-e29b-41d4-a716-446655440000");
                assert!(!json);
            }
            _ => panic!("expected ProjectShow variant"),
        }
    }

    #[test]
    fn project_show_invalid_uuid_rejected_before_request() {
        let client = HttpClient::new("http://127.0.0.1:1");
        let result = cmd_project_show(&client, "not-a-uuid", false);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("invalid Project ID"));
        assert!(msg.contains("must be a valid UUID"));
    }
}
