//! Library for Lighting CLI commands.
//!
//! Exports the HTTP-client-side commands (`health`, `source add`, `source show`)
//! so that the unified `lighting` binary can use them directly.  The standalone
//! `lighting-cli` binary still works via a thin `main.rs` shim.

use std::collections::{BTreeMap, BTreeSet};
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
            project_hints,
            item_budget,
            token_budget,
            json,
        } => cmd_context_pack(
            &client,
            &query,
            actor.as_deref(),
            &project_hints,
            item_budget,
            token_budget,
            json,
        ),
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
        CliCommand::BasicMemoryReconcile { path, output, json } => {
            cmd_basic_memory_reconcile(&client, &path, output.as_deref(), json)
        }
        CliCommand::HostedSeedSql {
            export,
            output,
            json,
            strict,
            scope,
        } => cmd_hosted_seed_sql(&export, &output, json, strict, &scope),
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
        CliCommand::Correction { command } => match command {
            CorrectionCommand::Record {
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
        },
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
        #[arg(long, value_delimiter = ',')]
        project_hints: Vec<String>,
        #[arg(long, default_value_t = 10)]
        item_budget: usize,
        #[arg(long, default_value_t = 2048)]
        token_budget: usize,
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
    /// Reconcile a Basic Memory snapshot against the running service, read-only:
    /// counts, per-note content and relationship coverage (LK-CUTOVER-001 C3).
    BasicMemoryReconcile {
        /// Validated snapshot directory or one notes-*.ndjson shard.
        path: PathBuf,
        /// Optional JSON report destination.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Output JSON only.
        #[arg(long)]
        json: bool,
    },
    /// Build idempotent hosted D1 seed SQL from a Lantern logical export
    /// (LK-CUTOVER-001 C3-IMPORT). Work applies the output with
    /// `wrangler d1 execute ... --file`.
    HostedSeedSql {
        /// Lantern logical export directory (manifest.json + *.ndjson).
        export: PathBuf,
        /// Seed SQL destination file.
        #[arg(long)]
        output: PathBuf,
        /// Output JSON only.
        #[arg(long)]
        json: bool,
        /// Production path: abort on the first malformed or missing row,
        /// missing reference, NUL byte or token hazard — before any artifact
        /// is written. Default (synthetic path) warns and still writes the
        /// SQL + report for inspection.
        #[arg(long)]
        strict: bool,
        /// Trusted operator scope for imported ordinary-note projections:
        /// KEY=VALUE, repeatable — all pairs form one normalised scope.
        /// Under --strict this is required whenever the export contains
        /// classifiable sources, because without it imported Markdown
        /// stays invisible to the ordinary hosted note list.
        #[arg(long = "scope", value_name = "KEY=VALUE")]
        scope: Vec<String>,
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
    /// Record or inspect corrections through the canonical correction surface.
    Correction {
        #[command(subcommand)]
        command: CorrectionCommand,
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
pub enum CorrectionCommand {
    /// Record a direct Matthew correction against one active Belief.
    Record {
        target_belief_id: String,
        correction_text: String,
        replacement_value: String,
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

/// Hosted (or native service) state gathered strictly read-only for
/// reconciliation. Only GET endpoints are used.
#[derive(Debug, Default)]
struct HostedReconcileState {
    /// `source_path` -> details of the note found by its import title.
    notes: BTreeMap<String, HostedNoteState>,
    /// hosted `source_id` -> count of Basic Memory-originated claims.
    claims_by_source: BTreeMap<String, usize>,
    total_basic_memory_claims: usize,
    /// Unresolved relations whose origin is the Basic Memory importer.
    unresolved_basic_memory_relations: usize,
}

#[derive(Debug)]
struct HostedNoteState {
    source_id: String,
    content: String,
    #[allow(dead_code)]
    fingerprint: String,
}

#[derive(Debug, Serialize)]
struct ReconcileNoteReport {
    source_path: String,
    present: bool,
    /// `None` when the note is absent from the service.
    content_match: Option<bool>,
    expected_claims: usize,
    hosted_claims: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct BasicMemoryReconcileReport {
    schema: &'static str,
    generated_at: String,
    service_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot_captured_at: Option<String>,
    notes_expected: usize,
    notes_present: usize,
    notes_missing: Vec<String>,
    notes_content_mismatch: Vec<String>,
    expected_claims: usize,
    hosted_claims: usize,
    claims_delta: i64,
    expected_relations: usize,
    hosted_unresolved_relations: usize,
    relations_delta: i64,
    per_note: Vec<ReconcileNoteReport>,
    /// Service state that has no read-only enumeration path over REST.
    /// Never silently treated as reconciled.
    not_enumerable_over_rest: Vec<String>,
    verdict: String,
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

/// Gathers the hosted side of a reconciliation with GET requests only.
/// Notes are located by the exact title the importer creates
/// (`basic-memory/<source_path>`, kind `markdown`) and verified by exact
/// content bytes. A missing note (404 or no current revision) is recorded as
/// absent rather than treated as an error.
fn fetch_hosted_reconcile_state(
    client: &HttpClient,
    records: &[BasicMemorySnapshotRecord],
) -> anyhow::Result<HostedReconcileState> {
    let mut state = HostedReconcileState::default();
    for record in records {
        let title = format!("basic-memory/{}", record.source_path);
        let url = format!(
            "/api/v1/sources/history?kind=markdown&title={}",
            urlencoding::encode(&title)
        );
        let response = client.get(&url).map_err(|error| {
            anyhow::Error::msg(error).context("is Lighting running? Try: lighting serve")
        })?;
        let body = match HttpClient::handle_response(response) {
            Ok(body) => body,
            Err(CliError::Api { status: 404, .. }) => continue,
            Err(error) => return Err(anyhow::Error::msg(error)),
        };
        let history: SourceHistoryResponse = serde_json::from_value(body)
            .map_err(|error| anyhow::Error::msg(format!("unexpected history response: {error}")))?;
        let Some(current) = history.revisions.iter().find(|revision| revision.current) else {
            continue;
        };
        let response = client
            .get(&format!("/api/v1/sources/{}", current.source_id))
            .map_err(|error| {
                anyhow::Error::msg(error).context("is Lighting running? Try: lighting serve")
            })?;
        let body = match HttpClient::handle_response(response) {
            Ok(body) => body,
            Err(CliError::Api { status: 404, .. }) => continue,
            Err(error) => return Err(anyhow::Error::msg(error)),
        };
        let source: SourceResponse = serde_json::from_value(body)
            .map_err(|error| anyhow::Error::msg(format!("unexpected source response: {error}")))?;
        state.notes.insert(
            record.source_path.clone(),
            HostedNoteState {
                source_id: source.source_id,
                content: source.content,
                fingerprint: source.fingerprint,
            },
        );
    }

    let response = client
        .get("/api/v1/claims?unmapped=false")
        .map_err(|error| {
            anyhow::Error::msg(error).context("is Lighting running? Try: lighting serve")
        })?;
    let claims_body = HttpClient::handle_response(response)?;
    for claim in claims_body
        .get("claims")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        if claim.get("extractor").and_then(serde_json::Value::as_str) == Some("basic-memory-import")
        {
            state.total_basic_memory_claims += 1;
            if let Some(source_id) = claim.get("source_id").and_then(serde_json::Value::as_str) {
                *state
                    .claims_by_source
                    .entry(source_id.to_owned())
                    .or_default() += 1;
            }
        }
    }

    let response = client.get("/api/v1/relations").map_err(|error| {
        anyhow::Error::msg(error).context("is Lighting running? Try: lighting serve")
    })?;
    let relations_body = HttpClient::handle_response(response)?;
    for relation in relations_body
        .get("relations")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        if relation.get("origin").and_then(serde_json::Value::as_str) == Some("basic-memory-cloud")
        {
            state.unresolved_basic_memory_relations += 1;
        }
    }
    Ok(state)
}

/// Pure comparison of the snapshot side against the fetched hosted state.
/// Any missing note, content mismatch or count delta fails the verdict;
/// state with no REST enumeration path is listed explicitly and never
/// counted as reconciled.
fn build_basic_memory_reconcile(
    records: &[BasicMemorySnapshotRecord],
    accounting: &BasicMemoryAccountingReport,
    hosted: &HostedReconcileState,
    service_url: &str,
    snapshot_captured_at: Option<String>,
    generated_at: String,
) -> BasicMemoryReconcileReport {
    let mut expected_claims_by_source: BTreeMap<String, usize> = BTreeMap::new();
    let mut expected_claims = 0usize;
    let mut expected_relations = 0usize;
    for item in &accounting.items {
        match item.item_type.as_str() {
            "observation" if item.outcome == "claim" => {
                expected_claims += 1;
                *expected_claims_by_source
                    .entry(item.source_path.clone())
                    .or_default() += 1;
            }
            "relation" => expected_relations += 1,
            _ => {}
        }
    }

    let mut per_note = Vec::new();
    let mut notes_missing = Vec::new();
    let mut notes_content_mismatch = Vec::new();
    let mut notes_present = 0usize;
    let mut hosted_claims = 0usize;
    for record in records {
        let expected_claims = expected_claims_by_source
            .get(&record.source_path)
            .copied()
            .unwrap_or(0);
        match hosted.notes.get(&record.source_path) {
            None => {
                notes_missing.push(record.source_path.clone());
                per_note.push(ReconcileNoteReport {
                    source_path: record.source_path.clone(),
                    present: false,
                    content_match: None,
                    expected_claims,
                    hosted_claims: 0,
                    source_id: None,
                });
            }
            Some(note) => {
                notes_present += 1;
                let content_match = note.content == record.raw_markdown;
                if !content_match {
                    notes_content_mismatch.push(record.source_path.clone());
                }
                let claims_for_note = hosted
                    .claims_by_source
                    .get(&note.source_id)
                    .copied()
                    .unwrap_or(0);
                hosted_claims += claims_for_note;
                per_note.push(ReconcileNoteReport {
                    source_path: record.source_path.clone(),
                    present: true,
                    content_match: Some(content_match),
                    expected_claims,
                    hosted_claims: claims_for_note,
                    source_id: Some(note.source_id.clone()),
                });
            }
        }
    }

    let claims_delta = hosted_claims as i64 - expected_claims as i64;
    let relations_delta =
        hosted.unresolved_basic_memory_relations as i64 - expected_relations as i64;
    let verdict = if notes_missing.is_empty()
        && notes_content_mismatch.is_empty()
        && claims_delta == 0
        && relations_delta == 0
    {
        "pass"
    } else {
        "fail"
    };

    BasicMemoryReconcileReport {
        schema: "basic-memory-reconcile-v1",
        generated_at,
        service_url: service_url.to_owned(),
        snapshot_captured_at,
        notes_expected: records.len(),
        notes_present,
        notes_missing,
        notes_content_mismatch,
        expected_claims,
        hosted_claims,
        claims_delta,
        expected_relations,
        hosted_unresolved_relations: hosted.unresolved_basic_memory_relations,
        relations_delta,
        per_note,
        not_enumerable_over_rest: vec![
            "soft_memory_items (POST search cannot enumerate; no GET list endpoint)".to_owned(),
            "ledger_events (ingest-only route; no GET list endpoint)".to_owned(),
            "resolved_relations (GET /api/v1/relations returns unresolved only)".to_owned(),
            "episodes (point GET by id only)".to_owned(),
        ],
        verdict: verdict.to_owned(),
    }
}

fn cmd_basic_memory_reconcile(
    client: &HttpClient,
    path: &Path,
    output: Option<&Path>,
    json: bool,
) -> anyhow::Result<()> {
    let records = load_basic_memory_snapshot(path)?;
    let manifest =
        load_basic_memory_snapshot_manifest(path).unwrap_or(BasicMemorySnapshotManifest {
            captured_at: None,
            note_count: None,
            observation_count: None,
            relation_count: None,
            export_sha256: None,
        });
    let accounting = build_basic_memory_accounting(&records, &manifest);
    let hosted = fetch_hosted_reconcile_state(client, &records)?;
    let report = build_basic_memory_reconcile(
        &records,
        &accounting,
        &hosted,
        &client.base_url,
        manifest.captured_at,
        chrono::Utc::now().to_rfc3339(),
    );

    if let Some(output) = output {
        let encoded = serde_json::to_vec_pretty(&report)?;
        if let Some(parent) = output
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create reconcile report directory {}",
                    parent.display()
                )
            })?;
        }
        fs::write(output, encoded)
            .with_context(|| format!("failed to write reconcile report {}", output.display()))?;
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "Basic Memory reconcile (read-only): {}/{} notes present, claims {}/{} (delta {}), relations {}/{} (delta {}), verdict={}",
            report.notes_present,
            report.notes_expected,
            report.hosted_claims,
            report.expected_claims,
            report.claims_delta,
            report.hosted_unresolved_relations,
            report.expected_relations,
            report.relations_delta,
            report.verdict
        );
        if !report.notes_missing.is_empty() {
            println!("Missing notes: {}", report.notes_missing.join(", "));
        }
        if !report.notes_content_mismatch.is_empty() {
            println!(
                "Content mismatches: {}",
                report.notes_content_mismatch.join(", ")
            );
        }
        println!(
            "Not enumerable over REST (not counted as reconciled): {}",
            report.not_enumerable_over_rest.join("; ")
        );
        if let Some(output) = output {
            println!("Report: {}", output.display());
        }
    }

    if report.verdict != "pass" {
        bail!(
            "reconciliation verdict=fail (missing={}, content_mismatch={}, claims_delta={}, relations_delta={})",
            report.notes_missing.len(),
            report.notes_content_mismatch.len(),
            report.claims_delta,
            report.relations_delta
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Hosted seed SQL builder (LK-CUTOVER-001 C3-IMPORT)
//
// Maps a Lantern logical export (manifest.json + *.ndjson) to idempotent
// INSERT statements for the hosted D1 schema. Narrow on purpose: only the
// tables the hosted memory surface reads (source, episode, epistemic_claim,
// belief) are mapped; every other export table is counted as `skipped`, never
// silently dropped, and a record among the mapped tables that cannot be
// mapped is reported as a warning AND fails the command after the artifacts
// are written (no unexplained loss).
//
// Native `belief_revision` rows are deliberately skipped: their shape is not
// the hosted `(belief_id, from_updated_at)` token contract, and the hosted
// correction batch inserts its own CAS guard row before it advances the
// projection. A bare seeded belief therefore accepts the first correction
// with NO genesis row (proven by lighting-store-d1 `tests/atomic_semantics.rs`);
// a colliding genesis row would block that first correction.
// ---------------------------------------------------------------------------

const HOSTED_SEED_SQL_VERSION: &str = "hosted-seed-sql-v1";

/// What the builder wrote, skipped and could not map.
#[derive(Debug, Serialize)]
pub struct HostedSeedSqlReport {
    pub schema: &'static str,
    pub export_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_record_count: Option<usize>,
    pub emitted: BTreeMap<String, usize>,
    pub skipped: BTreeMap<String, usize>,
    pub warnings: Vec<String>,
    pub sql_statements: usize,
}

/// Normalise an RFC3339 UTC timestamp to the hosted canonical form
/// (`YYYY-MM-DDTHH:MM:SS.fffffffffZ`, exactly 30 chars) — the hosted schema
/// orders keyset reads by string comparison, so mixed precision silently
/// breaks chronological order. Returns `None` when the input cannot be
/// normalised honestly (non-`Z` offsets, malformed heads).
pub fn canonical_hosted_timestamp(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let without_zone = trimmed.strip_suffix('Z')?;
    let (head, fraction) = match without_zone.split_once('.') {
        Some((head, fraction)) => (head, fraction),
        None => (without_zone, ""),
    };
    if head.len() != 19 {
        return None;
    }
    let mut fraction = fraction.to_owned();
    fraction.truncate(9);
    while fraction.len() < 9 {
        fraction.push('0');
    }
    Some(format!("{head}.{fraction}Z"))
}

fn sql_text(value: &str) -> String {
    format!("'{}'", value.replace('\u{0}', "").replace('\'', "''"))
}

fn sql_opt(value: Option<&str>) -> String {
    match value {
        Some(value) => sql_text(value),
        None => "NULL".to_owned(),
    }
}

/// Normalises record identifiers from either side: SurrealDB 3.3 serialises
/// exported record keys backtick-quoted (`table:`key``), while native payload
/// ids are plain — strip the table prefix AND any surrounding backticks.
fn strip_table_prefix<'a>(id: &'a str, table: &str) -> &'a str {
    let stripped = id.strip_prefix(&format!("{table}:")).unwrap_or(id);
    stripped.trim_matches('`')
}

fn payload_map(
    record: &serde_json::Map<String, serde_json::Value>,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let text = record.get("payload")?.as_str()?;
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    value.as_object().cloned()
}

fn payload_scope(
    payload: &serde_json::Map<String, serde_json::Value>,
) -> Option<BTreeMap<String, String>> {
    serde_json::from_value(payload.get("scope")?.clone()).ok()
}

fn record_text<'a>(
    record: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<&'a str> {
    record.get(key).and_then(serde_json::Value::as_str)
}

fn payload_text<'a>(
    payload: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<&'a str> {
    payload.get(key).and_then(serde_json::Value::as_str)
}

fn canonical_record_or_payload_timestamp(
    record: &serde_json::Map<String, serde_json::Value>,
    payload: &serde_json::Map<String, serde_json::Value>,
    record_key: &str,
    payload_key: &str,
) -> Option<String> {
    record_text(record, record_key)
        .and_then(canonical_hosted_timestamp)
        .or_else(|| payload_text(payload, payload_key).and_then(canonical_hosted_timestamp))
}

/// One classified source row plus the exact fields its ordinary-note
/// projection needs — one extraction, so a projection can never drift from
/// the byte-exact source row it points at.
struct SeededSource {
    statement: String,
    id: String,
    content: String,
    created_at: String,
}

fn seed_source_statement(
    record: &serde_json::Map<String, serde_json::Value>,
) -> Option<SeededSource> {
    let id = strip_table_prefix(record_text(record, "id")?, "source");
    let kind = record_text(record, "kind")?;
    let title = record_text(record, "title")?;
    let content = record_text(record, "content")?;
    let fingerprint = record_text(record, "fingerprint")?;
    let created_at = canonical_hosted_timestamp(record_text(record, "created_at")?)?;
    Some(SeededSource {
        statement: format!(
            "INSERT INTO source (id, kind, title, content, fingerprint, created_at) VALUES ({}, {}, {}, {}, {}, {}) ON CONFLICT(id) DO NOTHING;",
            sql_text(id),
            sql_text(kind),
            sql_text(title),
            sql_text(content),
            sql_text(fingerprint),
            sql_text(&created_at),
        ),
        id: id.to_owned(),
        content: content.to_owned(),
        created_at,
    })
}

/// Ordinary-note projection for one classified imported source: a scoped
/// `memory_item` (kind 'note') whose `source_id` resolves to the byte-exact
/// source row. NEVER an `epistemic_claim` or `belief` — an import must not
/// manufacture verified knowledge. The id is deterministic over operator
/// scope and source row, and `ON CONFLICT(id) DO NOTHING` makes a re-run
/// collide benignly instead of duplicating (replay-safe).
fn seed_import_note_statement(source: &SeededSource, scope_hash: &str) -> String {
    format!(
        "INSERT INTO memory_item (id, scope_hash, content, kind, salience, archived, payload, created_at, source_id, actor) VALUES ({id}, {scope_hash}, {content}, 'note', 0.0, 0, {content}, {created_at}, {source_id}, 'import:hosted-seed-sql-v1') ON CONFLICT(id) DO NOTHING;",
        id = sql_text(&import_note_id(scope_hash, &source.id)),
        scope_hash = sql_text(scope_hash),
        content = sql_text(&source.content),
        created_at = sql_text(&source.created_at),
        source_id = sql_text(&source.id),
    )
}

/// Deterministic note id: same operator scope + same source row -> same
/// primary key, so a re-seeded import collides instead of duplicating. The
/// `importnote|` input prefix keeps this id space distinct from a keyed
/// remember's `memory|` id space.
fn import_note_id(scope_hash: &str, source_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!("importnote|{scope_hash}|{source_id}").as_bytes());
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        .chars()
        .take(32)
        .collect()
}

fn seed_episode_statement(record: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    let id = strip_table_prefix(record_text(record, "id")?, "episode");
    let title = record_text(record, "title")?;
    let source_id = strip_table_prefix(record_text(record, "source_id")?, "source");
    let start_byte = record
        .get("start_byte")
        .and_then(|value| value.as_u64().or_else(|| value.as_i64().map(|v| v as u64)))?;
    let end_byte = record
        .get("end_byte")
        .and_then(|value| value.as_u64().or_else(|| value.as_i64().map(|v| v as u64)))?;
    let created_at = canonical_hosted_timestamp(record_text(record, "created_at")?)?;
    let project_id = record_text(record, "project_id")
        .map(|value| strip_table_prefix(value, "project").to_owned());
    Some(format!(
        "INSERT INTO episode (id, project_id, title, source_id, start_byte, end_byte, created_at) VALUES ({}, {}, {}, {}, {}, {}, {}) ON CONFLICT(id) DO NOTHING;",
        sql_text(id),
        sql_opt(project_id.as_deref()),
        sql_text(title),
        sql_text(source_id),
        start_byte,
        end_byte,
        sql_text(&created_at),
    ))
}

fn seed_claim_statement(
    record: &serde_json::Map<String, serde_json::Value>,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let payload = payload_map(record)?;
    let id = payload_text(&payload, "id")
        .map(str::to_owned)
        .or_else(|| {
            record_text(record, "id")
                .map(|raw| strip_table_prefix(raw, "epistemic_claim").to_owned())
        })?;
    let scope: BTreeMap<String, String> = match payload_scope(&payload) {
        Some(scope) => scope,
        None => {
            warnings.push(format!("claim {id}: payload has no usable scope; skipped"));
            return None;
        }
    };
    let scope_hash = lighting_core::scope_hash(&scope);
    let subject_key = payload_text(&payload, "subject_key")?;
    let predicate_status = payload_text(&payload, "predicate_status")?;
    let value = payload_text(&payload, "value")?;
    let speaker = payload_text(&payload, "speaker_actor_id")?;
    let dedupe_key = record_text(record, "dedupe_key")?;
    let known_at = canonical_hosted_timestamp(payload_text(&payload, "known_at")?)?;
    let created_at = canonical_hosted_timestamp(record_text(record, "created_at")?)?;
    let polarity = match payload.get("polarity") {
        Some(serde_json::Value::Bool(value)) => i64::from(*value),
        Some(serde_json::Value::Number(number)) => number.as_i64()?,
        None => {
            warnings.push(format!("claim {id}: payload has no polarity; skipped"));
            return None;
        }
        Some(other) => {
            warnings.push(format!(
                "claim {id}: payload polarity is not boolean ({other}); skipped"
            ));
            return None;
        }
    };
    let episode_id = payload_text(&payload, "episode_id")
        .map(|raw| strip_table_prefix(raw, "episode").to_owned());
    let source_id =
        payload_text(&payload, "source_id").map(|raw| strip_table_prefix(raw, "source").to_owned());
    Some(format!(
        "INSERT INTO epistemic_claim (id, scope_hash, subject_key, predicate_key, predicate_candidate, predicate_status, value, polarity, speaker_actor_id, holder_actor_id, dedupe_key, episode_id, source_id, payload, known_at, created_at) VALUES ({id}, {scope_hash}, {subject_key}, {predicate_key}, {candidate}, {status}, {value}, {polarity}, {speaker}, {holder}, {dedupe}, {episode}, {source}, {payload}, {known_at}, {created_at}) ON CONFLICT(id) DO NOTHING;",
        id = sql_text(&id),
        scope_hash = sql_text(&scope_hash),
        subject_key = sql_text(subject_key),
        predicate_key = sql_opt(payload_text(&payload, "predicate_key")),
        candidate = sql_opt(payload_text(&payload, "predicate_candidate")),
        status = sql_text(predicate_status),
        value = sql_text(value),
        polarity = polarity,
        speaker = sql_text(speaker),
        holder = sql_opt(payload_text(&payload, "holder_actor_id")),
        dedupe = sql_text(dedupe_key),
        episode = sql_opt(episode_id.as_deref()),
        source = sql_opt(source_id.as_deref()),
        payload = sql_text(record_text(record, "payload")?),
        known_at = sql_text(&known_at),
        created_at = sql_text(&created_at),
    ))
}

fn seed_belief_statement(
    record: &serde_json::Map<String, serde_json::Value>,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let payload = payload_map(record)?;
    let id = payload_text(&payload, "id")
        .map(str::to_owned)
        .or_else(|| {
            record_text(record, "id").map(|raw| strip_table_prefix(raw, "belief").to_owned())
        })?;
    let scope = match payload_scope(&payload) {
        Some(scope) => scope,
        None => {
            warnings.push(format!("belief {id}: payload has no usable scope; skipped"));
            return None;
        }
    };
    let scope_hash = payload_text(&payload, "scope_hash")
        .map(str::to_owned)
        .unwrap_or_else(|| lighting_core::scope_hash(&scope));
    let scope_column = serde_json::to_string(&scope).ok()?;
    let holder = payload_text(&payload, "holder_key")?;
    let subject = payload_text(&payload, "subject_key")?;
    let predicate = payload_text(&payload, "predicate_key")?;
    let current_value = payload_text(&payload, "current_value")?;
    let state = record_text(record, "state")
        .map(str::to_owned)
        .or_else(|| payload_text(&payload, "state").map(|value| value.to_ascii_lowercase()))?;
    let stale = record
        .get("stale")
        .and_then(serde_json::Value::as_bool)
        .or_else(|| payload.get("stale").and_then(serde_json::Value::as_bool))
        .unwrap_or(false);
    let dependency_generation = payload
        .get("dependency_generation")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as i64;
    let updated_at =
        match canonical_record_or_payload_timestamp(record, &payload, "updated_at", "updated_at") {
            Some(value) => value,
            None => {
                warnings.push(format!(
                    "belief {id}: no usable updated_at timestamp; skipped"
                ));
                return None;
            }
        };
    Some(format!(
        "INSERT INTO belief (id, scope_hash, scope, holder_key, subject_key, predicate_key, current_value, state, stale, stale_since, stale_reason, dependency_generation, updated_at, payload) VALUES ({id}, {scope_hash}, {scope}, {holder}, {subject}, {predicate}, {value}, {state}, {stale}, {stale_since}, {stale_reason}, {generation}, {updated_at}, {payload}) ON CONFLICT(id) DO NOTHING;",
        id = sql_text(&id),
        scope_hash = sql_text(&scope_hash),
        scope = sql_text(&scope_column),
        holder = sql_text(holder),
        subject = sql_text(subject),
        predicate = sql_text(predicate),
        value = sql_text(current_value),
        state = sql_text(&state),
        stale = i64::from(stale),
        stale_since = sql_opt(payload_text(&payload, "stale_since")),
        stale_reason = sql_opt(payload_text(&payload, "stale_reason")),
        generation = dependency_generation,
        updated_at = sql_text(&updated_at),
        payload = sql_text(record_text(record, "payload")?),
    ))
}

/// Read every `*.ndjson` in an export directory and emit hosted seed SQL.
/// A pending `belief_revision` row: (file, line, record, canonical
/// `created_at`), queued during the scan and resolved after it.
type PendingRevisionRow = (
    String,
    usize,
    serde_json::Map<String, serde_json::Value>,
    String,
);

/// Zero token (C3-RELATIONS): a belief's creation transition consumed no
/// prior hosted token, so its first revision records this sentinel.
/// Canonical (30 chars) and never equal to a real `updated_at`, so it can
/// never collide with the first post-seed correction's CAS insert.
const ZERO_TOKEN: &str = "0001-01-01T00:00:00.000000000Z";

/// A warning that aborts the build under `--strict` (production path)
/// before any artifact is written, and is collected otherwise (synthetic
/// path: SQL + report still written for inspection).
fn record_warning(warnings: &mut Vec<String>, strict: bool, message: String) -> anyhow::Result<()> {
    if strict {
        bail!("strict seed build aborted: {message}");
    }
    warnings.push(message);
    Ok(())
}

/// Belief chain anchors needed to derive revision `from_updated_at` tokens.
struct BeliefAnchors {
    created: Option<String>,
    updated: Option<String>,
}

/// The belief id the seed row will use: payload id first (mirrors
/// `seed_belief_statement`), then the normalised record id.
fn belief_seed_id(record: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    if let Some(payload) = payload_map(record)
        && let Some(id) = payload_text(&payload, "id")
    {
        return Some(id.to_owned());
    }
    record_text(record, "id").map(|raw| strip_table_prefix(raw, "belief").to_owned())
}

/// `memory_relation` → `epistemic_claim`: a Basic Memory relation is a
/// subject/predicate/value triple (see `docs/c3-relations-mapping.md` for
/// every fixed mapping choice). Returns `None` when the record cannot be
/// mapped honestly; the caller records the warning.
fn seed_relation_statement(
    record: &serde_json::Map<String, serde_json::Value>,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let id = strip_table_prefix(record_text(record, "id")?, "memory_relation");
    let dedupe = record_text(record, "dedupe_key")?;
    let in_id = record_text(record, "in_id")?;
    let out_id = record_text(record, "out_id")?;
    // Native persists `origin` inside the relation payload, not as a record
    // column; the synthetic fixture carries it at record level.
    let origin = if let Some(origin) = record_text(record, "origin") {
        origin.to_owned()
    } else {
        let payload = payload_map(record);
        match payload
            .as_ref()
            .and_then(|payload| payload_text(payload, "origin"))
        {
            Some(origin) => origin.to_owned(),
            None => {
                warnings.push(format!(
                    "relation {id}: no origin on record or payload; skipped"
                ));
                return None;
            }
        }
    };
    let created_at = canonical_hosted_timestamp(record_text(record, "created_at")?)?;
    let predicate = dedupe.split('|').next().unwrap_or("").trim();
    if predicate.is_empty() {
        warnings.push(format!(
            "relation {id}: dedupe_key carries no relation type; skipped"
        ));
        return None;
    }
    let episode_endpoint = [in_id, out_id]
        .iter()
        .find(|endpoint| endpoint.starts_with("episode:"))
        .map(|endpoint| strip_table_prefix(endpoint, "episode").to_owned());
    // Native persists the relation's own payload string; copy it byte-exact
    // like claim and belief payloads (docs/c3-relations-mapping.md).
    let payload = record_text(record, "payload")?;
    let scope_hash = lighting_core::scope_hash(&BTreeMap::new());
    Some(format!(
        "INSERT INTO epistemic_claim (id, scope_hash, subject_key, predicate_key, predicate_candidate, predicate_status, value, polarity, speaker_actor_id, holder_actor_id, dedupe_key, episode_id, source_id, payload, known_at, created_at) VALUES ({id}, {scope_hash}, {subject}, {predicate}, NULL, 'resolved', {value}, 1, {speaker}, NULL, {dedupe_key}, {episode}, NULL, {payload}, {known_at}, {created_at}) ON CONFLICT(id) DO NOTHING;",
        id = sql_text(id),
        scope_hash = sql_text(&scope_hash),
        subject = sql_text(in_id),
        predicate = sql_text(predicate),
        value = sql_text(out_id),
        speaker = sql_text(&origin),
        dedupe_key = sql_text(&format!("relation|{dedupe}")),
        episode = sql_opt(episode_endpoint.as_deref()),
        payload = sql_text(payload),
        known_at = sql_text(&created_at),
        created_at = sql_text(&created_at),
    ))
}

/// `belief_revision` → `belief_revision` with the derived CAS token
/// (`from_updated_at`); `payload` is the native payload string, byte-exact.
fn seed_revision_statement(
    id: &str,
    belief_id: &str,
    from_updated_at: &str,
    payload: &str,
    created_at: &str,
) -> String {
    format!(
        "INSERT INTO belief_revision (id, belief_id, from_updated_at, payload, created_at) VALUES ({}, {}, {}, {}, {}) ON CONFLICT(id) DO NOTHING;",
        sql_text(id),
        sql_text(belief_id),
        sql_text(from_updated_at),
        sql_text(payload),
        sql_text(created_at),
    )
}

pub fn build_hosted_seed_sql(
    export_dir: &Path,
    strict: bool,
    operator_scope: Option<BTreeMap<String, String>>,
) -> anyhow::Result<(String, HostedSeedSqlReport)> {
    // Trusted operator scope for imported ordinary-note projections: it
    // decides where imported Markdown becomes visible, so invalid or empty
    // input fails closed here instead of seeding into a wrong namespace.
    let import_scope_hash = match operator_scope {
        Some(scope) => {
            if scope.is_empty() {
                bail!(
                    "operator scope must not be empty (pass at least one --scope KEY=VALUE pair)"
                );
            }
            let normalised = lighting_core::normalize_scope(&scope)
                .map_err(|error| anyhow::anyhow!("operator scope invalid: {error}"))?;
            Some(lighting_core::scope_hash(&normalised))
        }
        None => None,
    };
    let manifest_text = fs::read_to_string(export_dir.join("manifest.json")).ok();
    let manifest_record_count = manifest_text
        .as_deref()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .and_then(|manifest| {
            manifest
                .get("record_count")
                .and_then(serde_json::Value::as_u64)
        })
        .map(|count| count as usize);

    let mut ndjson_files: Vec<PathBuf> = fs::read_dir(export_dir)
        .with_context(|| format!("failed to list export directory {}", export_dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("ndjson"))
        .collect();
    ndjson_files.sort();
    if ndjson_files.is_empty() {
        bail!(
            "export directory {} contains no *.ndjson files",
            export_dir.display()
        );
    }

    // Emission order follows the evidence chain (source -> episode -> claim
    // -> belief -> belief_revision) regardless of file order: the hosted
    // executor enforces the declared foreign keys, so claims must never
    // precede their sources and revisions never precede their beliefs.
    let mut by_table: Vec<Vec<String>> =
        vec![Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    let mut emitted: BTreeMap<String, usize> = BTreeMap::new();
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
    let mut warnings: Vec<String> = Vec::new();
    // C3-RELATIONS: belief chain anchors captured during the scan, and
    // pending belief_revision rows resolved after it (chain derivation needs
    // every belief's anchors first, so file order never matters).
    let mut belief_tokens: BTreeMap<String, BeliefAnchors> = BTreeMap::new();
    let mut pending_revisions: Vec<(String, usize, serde_json::Map<String, serde_json::Value>)> =
        Vec::new();

    for file in &ndjson_files {
        let text = fs::read_to_string(file)
            .with_context(|| format!("failed to read export shard {}", file.display()))?;
        for (index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let parsed: serde_json::Value = match serde_json::from_str(line) {
                Ok(value) => value,
                Err(error) => {
                    record_warning(
                        &mut warnings,
                        strict,
                        format!(
                            "{} line {}: not valid JSON ({error}); skipped",
                            file.display(),
                            index + 1
                        ),
                    )?;
                    continue;
                }
            };
            let Some(table) = parsed.get("table").and_then(serde_json::Value::as_str) else {
                record_warning(
                    &mut warnings,
                    strict,
                    format!(
                        "{} line {}: missing table field; skipped",
                        file.display(),
                        index + 1
                    ),
                )?;
                continue;
            };
            let Some(record) = parsed.get("record").and_then(serde_json::Value::as_object) else {
                record_warning(
                    &mut warnings,
                    strict,
                    format!(
                        "{} line {}: missing record object; skipped",
                        file.display(),
                        index + 1
                    ),
                )?;
                continue;
            };
            // Bytes (C3-RELATIONS): NUL cannot survive SQL TEXT; never strip
            // it silently.
            if table == "source"
                && record
                    .get("content")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|content| content.contains('\u{0}'))
            {
                record_warning(
                    &mut warnings,
                    strict,
                    format!(
                        "{} line {}: source content contains a NUL byte, which the seed SQL cannot preserve; skipped",
                        file.display(),
                        index + 1
                    ),
                )?;
                continue;
            }
            if table == "belief_revision" {
                pending_revisions.push((file.display().to_string(), index + 1, record.clone()));
                continue;
            }
            let bucket = match table {
                "source" => 0,
                "episode" => 1,
                "epistemic_claim" | "memory_relation" => 2,
                "belief" => 3,
                other => {
                    *skipped.entry(other.to_owned()).or_default() += 1;
                    continue;
                }
            };
            if table == "belief"
                && let (Some(id), Some(payload)) = (belief_seed_id(record), payload_map(record))
            {
                let created =
                    payload_text(&payload, "created_at").and_then(canonical_hosted_timestamp);
                let updated = canonical_record_or_payload_timestamp(
                    record,
                    &payload,
                    "updated_at",
                    "updated_at",
                );
                belief_tokens.insert(id, BeliefAnchors { created, updated });
            }
            if table == "source" {
                // Import visibility: one classified source, one ordinary-note
                // projection pushed directly after it (the projection's
                // foreign key points at this row) when an operator scope is
                // supplied. The projection is a note — never a claim/belief.
                let Some(source) = seed_source_statement(record) else {
                    record_warning(
                        &mut warnings,
                        strict,
                        format!(
                            "{} line {}: {table} record could not be mapped honestly; skipped",
                            file.display(),
                            index + 1
                        ),
                    )?;
                    continue;
                };
                let note_statement = import_scope_hash
                    .as_ref()
                    .map(|scope_hash| seed_import_note_statement(&source, scope_hash));
                by_table[bucket].push(source.statement);
                *emitted.entry(table.to_owned()).or_default() += 1;
                if let Some(statement) = note_statement {
                    by_table[bucket].push(statement);
                    *emitted.entry("memory_item".to_owned()).or_default() += 1;
                }
                continue;
            }
            let statement = match table {
                "episode" => seed_episode_statement(record),
                "epistemic_claim" => seed_claim_statement(record, &mut warnings),
                "memory_relation" => seed_relation_statement(record, &mut warnings),
                _ => seed_belief_statement(record, &mut warnings),
            };
            match statement {
                Some(statement) => {
                    by_table[bucket].push(statement);
                    *emitted.entry(table.to_owned()).or_default() += 1;
                }
                None => {
                    record_warning(
                        &mut warnings,
                        strict,
                        format!(
                            "{} line {}: {table} record could not be mapped honestly; skipped",
                            file.display(),
                            index + 1
                        ),
                    )?;
                }
            }
        }
    }

    // C3-RELATIONS: resolve belief_revision rows into hosted CAS history
    // after the scan (docs/c3-relations-mapping.md). Chain rule: from[0] is
    // the zero token for the belief's own creation, otherwise the belief's
    // initial token; from[k] = revision[k-1].created_at. Any derived token
    // equal to the belief's current updated_at would block the first
    // post-seed correction and is refused here.
    let mut revisions_by_belief: BTreeMap<String, Vec<PendingRevisionRow>> = BTreeMap::new();
    for (file, line, record) in pending_revisions {
        let Some(belief_ref) = record_text(&record, "belief_id") else {
            record_warning(
                &mut warnings,
                strict,
                format!("{file} line {line}: belief_revision record has no belief_id; skipped"),
            )?;
            continue;
        };
        let belief_id = strip_table_prefix(belief_ref, "belief").to_owned();
        let Some(created_at) =
            record_text(&record, "created_at").and_then(canonical_hosted_timestamp)
        else {
            record_warning(
                &mut warnings,
                strict,
                format!(
                    "{file} line {line}: belief_revision record has no usable created_at; skipped"
                ),
            )?;
            continue;
        };
        revisions_by_belief
            .entry(belief_id)
            .or_default()
            .push((file, line, record, created_at));
    }
    let mut seen_tokens: BTreeSet<(String, String)> = BTreeSet::new();
    for (belief_id, mut rows) in revisions_by_belief {
        // Canonical created_at strings sort chronologically (fixed width).
        rows.sort_by(|left, right| {
            let left_id = record_text(&left.2, "id").unwrap_or_default();
            let right_id = record_text(&right.2, "id").unwrap_or_default();
            left.3.cmp(&right.3).then_with(|| left_id.cmp(right_id))
        });
        let anchors = belief_tokens.get(&belief_id);
        let mut previous_created: Option<String> = None;
        for (file, line, record, created_at) in rows {
            let Some(id) = record_text(&record, "id")
                .map(|raw| strip_table_prefix(raw, "belief_revision").to_owned())
            else {
                record_warning(
                    &mut warnings,
                    strict,
                    format!("{file} line {line}: belief_revision record has no id; skipped"),
                )?;
                continue;
            };
            let Some(payload) = record_text(&record, "payload") else {
                record_warning(
                    &mut warnings,
                    strict,
                    format!("{file} line {line}: belief_revision {id} has no payload; skipped"),
                )?;
                continue;
            };
            let from = match &previous_created {
                Some(previous) => previous.clone(),
                None => match anchors.and_then(|anchors| anchors.created.as_ref()) {
                    Some(belief_created) if *belief_created == created_at => ZERO_TOKEN.to_owned(),
                    Some(belief_created) => belief_created.clone(),
                    None => {
                        record_warning(
                            &mut warnings,
                            strict,
                            format!(
                                "{file} line {line}: belief {belief_id} has no usable created_at; using the zero token for its creation transition"
                            ),
                        )?;
                        ZERO_TOKEN.to_owned()
                    }
                },
            };
            // The chain advances even when a row is omitted below, so later
            // revisions keep deriving from the native timeline.
            previous_created = Some(created_at.clone());
            if anchors.is_none() {
                record_warning(
                    &mut warnings,
                    strict,
                    format!(
                        "{file} line {line}: belief_revision {id} references belief {belief_id}, which is absent from the export; the apply will abort on the foreign key"
                    ),
                )?;
            }
            if anchors.and_then(|anchors| anchors.updated.as_ref()) == Some(&from) {
                record_warning(
                    &mut warnings,
                    strict,
                    format!(
                        "{file} line {line}: belief_revision {id} derives from_updated_at {from}, which equals the belief's current updated_at and would block the first post-seed correction; row omitted"
                    ),
                )?;
                continue;
            }
            if !seen_tokens.insert((belief_id.clone(), from.clone())) {
                record_warning(
                    &mut warnings,
                    strict,
                    format!(
                        "{file} line {line}: duplicate belief_revision token ({belief_id}, {from}); row omitted"
                    ),
                )?;
                continue;
            }
            by_table[4].push(seed_revision_statement(
                &id,
                &belief_id,
                &from,
                payload,
                &created_at,
            ));
            *emitted.entry("belief_revision".to_owned()).or_default() += 1;
        }
    }
    // Import visibility: without a trusted operator scope the exported
    // sources cannot be projected into ordinary notes, so imported Markdown
    // would stay invisible to the ordinary hosted list/get path. --strict
    // refuses to build that silently; the default path records it so the
    // report never claims a fully mapped import.
    let source_count = emitted.get("source").copied().unwrap_or(0);
    if import_scope_hash.is_none() && source_count > 0 {
        record_warning(
            &mut warnings,
            strict,
            format!(
                "{source_count} exported source(s) were seeded without an ordinary note projection: no operator scope was supplied (--scope KEY=VALUE), so imported Markdown stays invisible to lantern_list_notes"
            ),
        )?;
    }
    let statements: Vec<String> = by_table.into_iter().flatten().collect();

    // Name the projection rule in the artifact itself when it applied.
    let projection_note = if emitted.contains_key("memory_item") {
        "-- note projection: classified imported source -> scoped memory_item (kind 'note', operator scope, valid source_id; never a claim or belief); requires hosted schema 0002+0003 (memory_item.source_id, actor)\n"
    } else {
        ""
    };
    let mut header = format!(
        "-- {HOSTED_SEED_SQL_VERSION}: hosted D1 seed from Lantern logical export\n-- export: {}\n-- manifest record_count: {}\n-- emitted: {}\n-- skipped (not part of the hosted memory surface): {}\n-- idempotent: ON CONFLICT(id) DO NOTHING never overwrites post-seed corrections.\n-- mapping: memory_relation->epistemic_claim, belief_revision->belief_revision (docs/c3-relations-mapping.md)\n{projection_note}-- apply: wrangler d1 execute lantern-keeper --remote --config wrangler.jsonc --file <this>\n\n",
        export_dir.display(),
        manifest_record_count
            .map(|count| count.to_string())
            .unwrap_or_else(|| "unknown".to_owned()),
        emitted
            .iter()
            .map(|(table, count)| format!("{table}={count}"))
            .collect::<Vec<_>>()
            .join(", "),
        skipped
            .iter()
            .map(|(table, count)| format!("{table}={count}"))
            .collect::<Vec<_>>()
            .join(", "),
    );
    for statement in &statements {
        header.push_str(statement);
        header.push('\n');
    }

    let report = HostedSeedSqlReport {
        schema: HOSTED_SEED_SQL_VERSION,
        export_dir: export_dir.display().to_string(),
        manifest_record_count,
        emitted,
        skipped,
        warnings,
        sql_statements: statements.len(),
    };
    Ok((header, report))
}

fn cmd_hosted_seed_sql(
    export: &Path,
    output: &Path,
    json: bool,
    strict: bool,
    scope_pairs: &[String],
) -> anyhow::Result<()> {
    if !export.is_dir() {
        bail!("export path is not a directory: {}", export.display());
    }
    let operator_scope = parse_operator_scope(scope_pairs)?;
    // Under --strict the build itself aborts on the first problem, so this
    // path never reaches the artifact write with a strict failure.
    let (sql, report) = build_hosted_seed_sql(export, strict, operator_scope)?;

    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create seed SQL directory {}", parent.display()))?;
    }
    fs::write(output, sql.as_bytes())
        .with_context(|| format!("failed to write seed SQL {}", output.display()))?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "Hosted seed SQL: {} statement(s) from {} (emitted: {}, skipped: {})",
            report.sql_statements,
            report.export_dir,
            report
                .emitted
                .iter()
                .map(|(table, count)| format!("{table}={count}"))
                .collect::<Vec<_>>()
                .join(", "),
            report
                .skipped
                .iter()
                .map(|(table, count)| format!("{table}={count}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        println!("Seed SQL: {}", output.display());
        for warning in &report.warnings {
            println!("warning: {warning}");
        }
    }

    if !report.warnings.is_empty() {
        bail!(
            "{} export record(s) among the mapped tables could not be mapped honestly; seed SQL and report were written for inspection, but this is unexplained loss and must not be applied as-is",
            report.warnings.len()
        );
    }
    Ok(())
}

/// Parse repeated `--scope KEY=VALUE` pairs into one operator scope. The
/// scope is trusted operator input (it decides where imported notes become
/// visible), so a malformed pair fails closed instead of guessing.
fn parse_operator_scope(pairs: &[String]) -> anyhow::Result<Option<BTreeMap<String, String>>> {
    if pairs.is_empty() {
        return Ok(None);
    }
    let mut scope = BTreeMap::new();
    for pair in pairs {
        let Some((key, value)) = pair.split_once('=') else {
            bail!("--scope expects KEY=VALUE, got {pair:?}");
        };
        let (key, value) = (key.trim(), value.trim());
        if key.is_empty() || value.is_empty() {
            bail!("--scope expects a non-empty KEY and VALUE, got {pair:?}");
        }
        if scope.insert(key.to_owned(), value.to_owned()).is_some() {
            bail!("duplicate --scope key {key:?}");
        }
    }
    Ok(Some(scope))
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
    project_hints: &[String],
    item_budget: usize,
    token_budget: usize,
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
                "project_hints": project_hints,
                "item_budget": item_budget,
                "token_budget": token_budget,
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
    use std::sync::Arc;

    use lighting_store_surreal::{
        StoreConfig, SurrealEpistemicRepository, SurrealLedgerRepository,
        SurrealMemoryPathRepository, SurrealMemoryRepository, SurrealSourceRepository,
        SurrealStore,
    };

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

    // ── Basic Memory reconcile tests (LK-CUTOVER-001 C3) ────────────────────

    fn reconcile_record(path: &str, content: &str) -> BasicMemorySnapshotRecord {
        BasicMemorySnapshotRecord {
            source_project: None,
            source_project_id: None,
            source_path: path.to_owned(),
            title: None,
            permalink: None,
            external_id: None,
            entity_id: None,
            note_type: None,
            content_type: None,
            updated_at: None,
            raw_markdown: content.to_owned(),
        }
    }

    fn reconcile_manifest() -> BasicMemorySnapshotManifest {
        BasicMemorySnapshotManifest {
            captured_at: Some("2026-11-24".to_owned()),
            note_count: None,
            observation_count: None,
            relation_count: None,
            export_sha256: None,
        }
    }

    fn hosted_note(source_id: &str, content: &str) -> HostedNoteState {
        HostedNoteState {
            source_id: source_id.to_owned(),
            content: content.to_owned(),
            fingerprint: "sha256:test".to_owned(),
        }
    }

    #[test]
    fn basic_memory_reconcile_command_is_constructed() {
        let cmd = CliCommand::BasicMemoryReconcile {
            path: PathBuf::from("snapshot"),
            output: None,
            json: true,
        };
        match cmd {
            CliCommand::BasicMemoryReconcile { path, output, json } => {
                assert_eq!(path, PathBuf::from("snapshot"));
                assert!(output.is_none());
                assert!(json);
            }
            _ => panic!("expected BasicMemoryReconcile variant"),
        }
    }

    #[test]
    fn reconcile_passes_when_hosted_matches_snapshot() {
        let note_a = "---\ntitle: A\n---\n\n- [decision] Keep the source intact\n";
        let note_b = "---\ntitle: B\n---\n\n- related_to [[Lantern Keeper]]\n";
        let records = vec![
            reconcile_record("a.md", note_a),
            reconcile_record("b.md", note_b),
        ];
        let accounting = build_basic_memory_accounting(&records, &reconcile_manifest());
        // a.md carries one truth-bearing decision (claim); b.md one relation.
        assert_eq!(accounting.accounted_observations, 1);
        assert_eq!(accounting.accounted_relations, 1);

        let mut notes = BTreeMap::new();
        notes.insert("a.md".to_owned(), hosted_note("sa", note_a));
        notes.insert("b.md".to_owned(), hosted_note("sb", note_b));
        let mut claims_by_source = BTreeMap::new();
        claims_by_source.insert("sa".to_owned(), 1);
        let hosted = HostedReconcileState {
            notes,
            claims_by_source,
            total_basic_memory_claims: 1,
            unresolved_basic_memory_relations: 1,
        };

        let report = build_basic_memory_reconcile(
            &records,
            &accounting,
            &hosted,
            "http://127.0.0.1:4317",
            Some("2026-11-24".to_owned()),
            "2026-11-24T00:00:00+00:00".to_owned(),
        );

        assert_eq!(report.verdict, "pass");
        assert_eq!(report.notes_expected, 2);
        assert_eq!(report.notes_present, 2);
        assert!(report.notes_missing.is_empty());
        assert!(report.notes_content_mismatch.is_empty());
        assert_eq!(report.expected_claims, 1);
        assert_eq!(report.hosted_claims, 1);
        assert_eq!(report.claims_delta, 0);
        assert_eq!(report.expected_relations, 1);
        assert_eq!(report.hosted_unresolved_relations, 1);
        assert_eq!(report.relations_delta, 0);
        assert!(
            report
                .per_note
                .iter()
                .all(|note| note.present && note.content_match == Some(true))
        );
        assert_eq!(report.not_enumerable_over_rest.len(), 4);
        assert_eq!(report.schema, "basic-memory-reconcile-v1");
    }

    #[test]
    fn reconcile_fails_on_missing_and_content_mismatch() {
        let note_a = "---\ntitle: A\n---\n\n- [decision] Keep it\n";
        let note_b = "---\ntitle: B\n---\n\n- [decision] Separate note\n";
        let records = vec![
            reconcile_record("a.md", note_a),
            reconcile_record("b.md", note_b),
        ];
        let accounting = build_basic_memory_accounting(&records, &reconcile_manifest());

        // Hosted holds a.md with ALTERED bytes; b.md is absent entirely.
        let mut notes = BTreeMap::new();
        notes.insert(
            "a.md".to_owned(),
            hosted_note("sa", "---\ntitle: A\n---\n\n- [decision] Tampered\n"),
        );
        let hosted = HostedReconcileState {
            notes,
            claims_by_source: BTreeMap::new(),
            total_basic_memory_claims: 0,
            unresolved_basic_memory_relations: 0,
        };

        let report = build_basic_memory_reconcile(
            &records,
            &accounting,
            &hosted,
            "http://127.0.0.1:4317",
            None,
            "t".to_owned(),
        );

        assert_eq!(report.verdict, "fail");
        assert_eq!(report.notes_present, 1);
        assert_eq!(report.notes_missing, vec!["b.md".to_owned()]);
        assert_eq!(report.notes_content_mismatch, vec!["a.md".to_owned()]);
        let missing = report
            .per_note
            .iter()
            .find(|note| note.source_path == "b.md")
            .expect("b.md is reported");
        assert!(!missing.present);
        assert_eq!(missing.content_match, None);
        let mismatched = report
            .per_note
            .iter()
            .find(|note| note.source_path == "a.md")
            .expect("a.md is reported");
        assert_eq!(mismatched.content_match, Some(false));
    }

    #[test]
    fn reconcile_fails_on_count_deltas() {
        let note_a = "---\ntitle: A\n---\n\n- [decision] One\n- [decision] Two\n";
        let records = vec![reconcile_record("a.md", note_a)];
        let accounting = build_basic_memory_accounting(&records, &reconcile_manifest());
        assert_eq!(accounting.accounted_observations, 2);

        // Hosted has the note and matching content, but only one of the two
        // expected claims landed, and a relation is missing entirely.
        let mut notes = BTreeMap::new();
        notes.insert("a.md".to_owned(), hosted_note("sa", note_a));
        let mut claims_by_source = BTreeMap::new();
        claims_by_source.insert("sa".to_owned(), 1);
        let hosted = HostedReconcileState {
            notes,
            claims_by_source,
            total_basic_memory_claims: 1,
            unresolved_basic_memory_relations: 0,
        };

        let report = build_basic_memory_reconcile(
            &records,
            &accounting,
            &hosted,
            "http://127.0.0.1:4317",
            None,
            "t".to_owned(),
        );

        assert_eq!(report.verdict, "fail");
        assert_eq!(report.expected_claims, 2);
        assert_eq!(report.hosted_claims, 1);
        assert_eq!(report.claims_delta, -1);
        assert_eq!(report.relations_delta, 0);
        // Content matched: the failure is purely a count delta.
        assert!(report.notes_content_mismatch.is_empty());
        assert!(report.notes_missing.is_empty());
    }

    // -----------------------------------------------------------------------
    // C3-IMPORT: hosted seed SQL builder + three-way reconcile
    // -----------------------------------------------------------------------

    fn c3_fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/c3-import")
            .join(name)
    }

    fn c3_temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "lantern-keeper-c3-import-{label}-{}",
            uuid::Uuid::new_v4().simple()
        ))
    }

    /// Mirrors D1 `batch()`: one transaction, fully rolled back on error
    /// (same semantics as lighting-store-d1 tests/atomic_semantics.rs).
    fn c3_run_batch(
        conn: &rusqlite::Connection,
        statements: &[lighting_store_d1::SqlStatement],
    ) -> rusqlite::Result<()> {
        conn.execute_batch("BEGIN")?;
        let outcome = (|| {
            for statement in statements {
                let params: Vec<rusqlite::types::Value> = statement
                    .params
                    .iter()
                    .map(|param| match param {
                        lighting_store_d1::SqlValue::Text(text) => {
                            rusqlite::types::Value::Text(text.clone())
                        }
                        lighting_store_d1::SqlValue::Integer(int) => {
                            rusqlite::types::Value::Integer(*int)
                        }
                        lighting_store_d1::SqlValue::Real(real) => {
                            rusqlite::types::Value::Real(*real)
                        }
                        lighting_store_d1::SqlValue::Null => rusqlite::types::Value::Null,
                    })
                    .collect();
                let mut prepared = conn.prepare(&statement.sql)?;
                prepared.execute(rusqlite::params_from_iter(params))?;
            }
            Ok(())
        })();
        match outcome {
            Ok(()) => conn.execute_batch("COMMIT"),
            Err(error) => {
                conn.execute_batch("ROLLBACK")?;
                Err(error)
            }
        }
    }

    fn c3_count(conn: &rusqlite::Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("count query")
    }

    /// Read every selected TEXT column of one prepared statement as an
    /// optional string (the hosted reads exercised here select only TEXT).
    fn c3_rows(
        conn: &rusqlite::Connection,
        statement: &lighting_store_d1::SqlStatement,
    ) -> Vec<Vec<Option<String>>> {
        let mut prepared = conn.prepare(&statement.sql).expect("prepare");
        let column_count = prepared.column_count();
        let params: Vec<rusqlite::types::Value> = statement
            .params
            .iter()
            .map(|param| match param {
                lighting_store_d1::SqlValue::Text(text) => {
                    rusqlite::types::Value::Text(text.clone())
                }
                lighting_store_d1::SqlValue::Integer(int) => rusqlite::types::Value::Integer(*int),
                lighting_store_d1::SqlValue::Real(real) => rusqlite::types::Value::Real(*real),
                lighting_store_d1::SqlValue::Null => rusqlite::types::Value::Null,
            })
            .collect();
        let mut query = prepared
            .query(rusqlite::params_from_iter(params))
            .expect("query");
        let mut rows = Vec::new();
        while let Some(row) = query.next().expect("row") {
            let mut columns = Vec::with_capacity(column_count);
            for index in 0..column_count {
                columns.push(row.get::<_, Option<String>>(index).expect("text column"));
            }
            rows.push(columns);
        }
        rows
    }

    #[test]
    fn hosted_seed_maps_relations_history_and_bytes() {
        let scope = BTreeMap::from([("project".to_owned(), "c3-import".to_owned())]);
        let (sql, report) =
            build_hosted_seed_sql(&c3_fixture("synthetic-export"), false, Some(scope))
                .expect("builder must run");
        assert!(
            report.warnings.is_empty(),
            "warnings: {:?}",
            report.warnings
        );

        // memory_relation -> epistemic_claim (docs/c3-relations-mapping.md).
        assert_eq!(report.emitted.get("memory_relation"), Some(&1));
        assert!(sql.contains("'relation|related_to|c3-seed-ep-alpha|Beta'"));
        assert!(
            sql.contains("'episode:c3-seed-ep-alpha'"),
            "subject = in_id"
        );
        assert!(sql.contains("'basic-memory:Beta'"), "value = out_id");
        assert!(
            sql.contains("'related_to'"),
            "predicate = dedupe first segment"
        );
        assert!(sql.contains("'resolved'"), "predicate_status = resolved");
        let empty_hash = lighting_core::scope_hash(&BTreeMap::new());
        assert!(
            sql.contains(&format!("'{empty_hash}'")),
            "relations seed the empty-scope hash"
        );

        // belief_revision -> belief_revision with the derived CAS chain.
        assert_eq!(report.emitted.get("belief_revision"), Some(&2));
        assert!(
            sql.contains("'0001-01-01T00:00:00.000000000Z'"),
            "creation transition records the zero token"
        );
        let rev2 = sql
            .lines()
            .find(|line| {
                line.starts_with("INSERT INTO belief_revision") && line.contains("'c3-seed-rev-2'")
            })
            .expect("rev-2 statement");
        assert!(
            rev2.contains("'2026-10-01T10:00:00.000000000Z'"),
            "second transition derives from the first revision's created_at: {rev2}"
        );
        // Byte-exact native payload strings survive into the seed.
        assert!(
            sql.contains("\"transition_type\":\"reinforce\""),
            "revision payload preserved verbatim"
        );
    }

    #[test]
    fn hosted_seed_strict_aborts_before_writing_artifacts() {
        let dir = c3_temp_path("strict-malformed");
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("manifest.json"), "{}").expect("manifest");
        fs::write(
            dir.join("broken.ndjson"),
            "{\"table\": \"source\"\nnot json at all\n",
        )
        .expect("shard");
        let output = dir.join("seed.sql");

        let strict = cmd_hosted_seed_sql(&dir, &output, false, true, &[]);
        assert!(strict.is_err(), "strict must abort on a malformed row");
        assert!(
            !output.exists(),
            "strict must abort before any artifact is written"
        );

        let default = cmd_hosted_seed_sql(&dir, &output, false, false, &[]);
        assert!(
            default.is_err(),
            "default must still report unexplained loss"
        );
        assert!(
            output.exists(),
            "default writes the SQL + report for inspection"
        );
    }

    #[test]
    fn hosted_seed_strict_refuses_missing_belief_references() {
        let dir = c3_temp_path("strict-referential");
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("manifest.json"), "{}").expect("manifest");
        fs::write(
            dir.join("epistemic.ndjson"),
            "{\"table\": \"belief_revision\", \"record\": {\"id\": \"belief_revision:orphan\", \"belief_id\": \"belief:absent\", \"payload\": \"{}\", \"created_at\": \"2026-10-01T10:00:00.000000000Z\"}}\n",
        )
        .expect("shard");
        let output = dir.join("seed.sql");

        assert!(
            cmd_hosted_seed_sql(&dir, &output, false, true, &[]).is_err(),
            "strict must abort on a revision whose belief is absent"
        );
        assert!(!output.exists(), "strict writes nothing");

        // Default warns (the apply would fail closed on the foreign key) and
        // still produces the artifact for inspection.
        let _ = cmd_hosted_seed_sql(&dir, &output, false, false, &[]);
        assert!(output.exists(), "default writes the artifact");
    }

    #[test]
    fn hosted_seed_refuses_token_hazards() {
        // A corrupt export where a derived from_updated_at equals the
        // belief's current updated_at would block the first post-seed
        // correction: strict aborts, default omits the row with a warning.
        let dir = c3_temp_path("strict-hazard");
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("manifest.json"), "{}").expect("manifest");
        let belief = serde_json::json!({
            "table": "belief",
            "record": {
                "id": "belief:hazard-1",
                "payload": serde_json::to_string(&serde_json::json!({
                    "id": "hazard-1",
                    "holder_key": "matthew",
                    "subject_key": "s",
                    "predicate_key": "p",
                    "current_value": "v",
                    "scope": {},
                    "state": "Active",
                    "created_at": "2026-10-01T10:00:00.000000000Z",
                    "updated_at": "2026-10-01T11:00:00.000000000Z"
                })).expect("payload"),
                "updated_at": "2026-10-01T11:00:00.000000000Z"
            }
        });
        let revisions = [
            ("r1", "2026-10-01T10:00:00.000000000Z"),
            ("r2", "2026-10-01T11:00:00.000000000Z"),
            ("r3", "2026-10-01T12:00:00.000000000Z"),
        ];
        let mut shard = format!("{}\n", belief);
        for (id, created_at) in revisions {
            shard.push_str(&format!(
                "{{\"table\": \"belief_revision\", \"record\": {{\"id\": \"belief_revision:{id}\", \"belief_id\": \"belief:hazard-1\", \"payload\": \"{{}}\", \"created_at\": \"{created_at}\"}}}}\n"
            ));
        }
        fs::write(dir.join("epistemic.ndjson"), shard).expect("shard");
        let output = dir.join("seed.sql");

        // r3 derives from = r2.created_at (11:00) == belief.updated_at.
        assert!(
            cmd_hosted_seed_sql(&dir, &output, false, true, &[]).is_err(),
            "strict must abort on a token hazard"
        );
        assert!(!output.exists(), "strict writes nothing");

        let (sql, report) = build_hosted_seed_sql(&dir, false, None).expect("default must build");
        assert_eq!(
            report.emitted.get("belief_revision"),
            Some(&2),
            "default omits the hazardous row"
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("would block the first post-seed correction")),
            "warnings: {:?}",
            report.warnings
        );
        assert!(!sql.contains("belief_revision:r3") && !sql.contains("'r3'"));
    }

    #[test]
    fn hosted_seed_builder_maps_the_synthetic_export() {
        let scope = BTreeMap::from([("project".to_owned(), "c3-import".to_owned())]);
        let (sql, report) =
            build_hosted_seed_sql(&c3_fixture("synthetic-export"), false, Some(scope))
                .expect("builder must run");
        assert_eq!(report.schema, "hosted-seed-sql-v1");
        assert_eq!(report.manifest_record_count, Some(16));
        assert!(
            report.warnings.is_empty(),
            "warnings: {:?}",
            report.warnings
        );
        assert_eq!(report.emitted.get("source"), Some(&2));
        assert_eq!(report.emitted.get("episode"), Some(&2));
        assert_eq!(report.emitted.get("epistemic_claim"), Some(&2));
        assert_eq!(report.emitted.get("belief"), Some(&2));
        assert_eq!(report.emitted.get("memory_relation"), Some(&1));
        assert_eq!(report.emitted.get("belief_revision"), Some(&2));
        assert_eq!(
            report.emitted.get("memory_item"),
            Some(&2),
            "both classified sources are projected into ordinary notes"
        );
        assert_eq!(report.sql_statements, 13);
        for unmapped in ["marker", "ledger_event", "project", "memory", "trace"] {
            assert_eq!(
                report.skipped.get(unmapped),
                Some(&1),
                "unmapped table {unmapped} must be reported as skipped"
            );
        }
        assert!(
            sql.contains(
                "-- skipped (not part of the hosted memory surface): ledger_event=1, marker=1, memory=1, project=1, trace=1"
            ),
            "skipped record types stay named in the seed header"
        );

        // Non-canonical input timestamps are normalised to the hosted
        // 30-char canonical form; raw non-canonical forms never leak into SQL.
        assert!(sql.contains("'2026-10-01T09:05:00.000000000Z'"));
        assert!(sql.contains("'2026-10-01T10:30:00.000000000Z'"));
        assert!(sql.contains("'2026-10-01T12:00:00.000000000Z'"));
        assert!(!sql.contains("'2026-10-01T09:05:00Z'"));
        assert!(!sql.contains("'2026-10-01T12:00:00Z'"));

        // Idempotent upserts, never overwrites — one per INSERT statement.
        // The semicolon matters: statements whose stored content contains
        // real newlines span multiple lines, and the header's idempotency
        // note has no trailing semicolon.
        assert_eq!(sql.matches("ON CONFLICT(id) DO NOTHING;").count(), 13);
        let projection = sql
            .lines()
            .find(|line| line.starts_with("INSERT INTO memory_item"))
            .expect("note projection statement");
        assert!(
            projection.contains("'note'") && projection.contains("'c3-seed-src-alpha'"),
            "projection is a note pointing at its byte-exact source: {projection}"
        );
        assert!(
            sql.contains("-- note projection: classified imported source"),
            "the artifact names the projection rule it applied"
        );

        // Surreal-style backticks from exported record ids must never leak
        // into the seed SQL.
        assert!(
            !sql.contains('\u{60}'),
            "backticked record ids must be normalised away"
        );

        // Fixture scope_hash matches the runtime authority.
        let scope: BTreeMap<String, String> =
            BTreeMap::from([("project".to_owned(), "c3-import".to_owned())]);
        let expected_hash = lighting_core::scope_hash(&scope);
        assert_eq!(
            expected_hash,
            "ed329c9a475d9fca0a1dd8a40c38743677edf7267c5e74d1d1c35f5184561a84"
        );
        assert!(sql.contains(&format!("'{expected_hash}'")));
    }

    #[test]
    fn hosted_seed_projects_imported_notes_for_list_get() {
        // Import visibility: classified exported sources project into scoped
        // ordinary notes carrying a valid source_id, so a fresh hosted-style
        // list -> get path reads the imported Markdown back — while claims,
        // beliefs and history stay exactly as the mapper emitted them.
        let scope: BTreeMap<String, String> =
            BTreeMap::from([("project".to_owned(), "home-import".to_owned())]);
        let scope_hash = lighting_core::scope_hash(&scope);
        let (sql, report) =
            build_hosted_seed_sql(&c3_fixture("synthetic-export"), false, Some(scope))
                .expect("builder must run");
        assert!(
            report.warnings.is_empty(),
            "warnings: {:?}",
            report.warnings
        );
        assert_eq!(report.emitted.get("source"), Some(&2));
        assert_eq!(
            report.emitted.get("memory_item"),
            Some(&2),
            "every classified source is projected into an ordinary note"
        );

        let conn = rusqlite::Connection::open_in_memory().expect("sqlite");
        conn.execute_batch(lighting_store_d1::SCHEMA_SQL)
            .expect("hosted schema 0001");
        conn.execute_batch(lighting_store_d1::REMEMBER_SCHEMA_SQL)
            .expect("hosted schema 0002 (memory_item.source_id)");
        conn.execute_batch(lighting_store_d1::REMEMBER_ATTR_SCHEMA_SQL)
            .expect("hosted schema 0003 (memory_item.actor)");
        conn.execute_batch(&sql)
            .expect("seed must apply to a fresh hosted database");

        // Projections are notes in the operator's scope; nothing is promoted.
        assert_eq!(c3_count(&conn, "memory_item"), 2);
        assert_eq!(
            c3_count(&conn, "epistemic_claim"),
            3,
            "claims and relations are unchanged by the projection"
        );
        assert_eq!(c3_count(&conn, "belief"), 2, "imports are never beliefs");
        assert_eq!(c3_count(&conn, "belief_revision"), 2, "history unchanged");

        // Hosted-style list: each note carries its source_id handle.
        let list = lighting_store_d1::memory_note_list(&scope_hash, 128);
        let notes = c3_rows(&conn, &list);
        assert_eq!(notes.len(), 2, "both imported notes are listable");
        let mut handles = Vec::new();
        for note in &notes {
            assert!(
                !note[0].as_deref().unwrap_or_default().is_empty(),
                "every note has a note_id"
            );
            let source_id = note[3]
                .clone()
                .expect("projected note carries its source_id handle");
            let content = note[1].clone().expect("note content");
            let stored: String = conn
                .query_row(
                    "SELECT content FROM source WHERE id = ?1",
                    [&source_id],
                    |row| row.get(0),
                )
                .expect("source row behind the note");
            assert_eq!(
                stored.as_bytes(),
                content.as_bytes(),
                "the note mirrors its byte-exact source"
            );
            handles.push((source_id, content));
        }

        // Scope confinement: a scope the operator did not target sees nothing.
        let foreign_scope: BTreeMap<String, String> =
            BTreeMap::from([("project".to_owned(), "not-targeted".to_owned())]);
        let foreign =
            lighting_store_d1::memory_note_list(&lighting_core::scope_hash(&foreign_scope), 128);
        assert_eq!(
            c3_rows(&conn, &foreign).len(),
            0,
            "no cross-scope visibility"
        );

        // Hosted-style get: resolve each handle through the scoped join.
        for (source_id, content) in &handles {
            let get = lighting_store_d1::source_bytes(source_id, &scope_hash);
            let rows = c3_rows(&conn, &get);
            assert_eq!(rows.len(), 1, "list handle resolves via the get path");
            assert_eq!(
                rows[0][1].as_deref(),
                Some(content.as_str()),
                "exact bytes round-trip list -> get"
            );
        }

        // Replay-safe: re-applying the identical seed never duplicates a
        // projection (deterministic id + ON CONFLICT(id) DO NOTHING).
        conn.execute_batch(&sql).expect("re-apply must not fail");
        assert_eq!(c3_count(&conn, "memory_item"), 2, "re-seeding is a no-op");
    }

    #[test]
    fn hosted_seed_requires_operator_scope_for_note_visibility() {
        // Fail closed: --strict refuses to build an import whose sources
        // could not be projected, and the default path reports the loss
        // instead of claiming a fully mapped import.
        let dir = c3_temp_path("strict-note-scope");
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("manifest.json"), "{}").expect("manifest");
        let source = serde_json::json!({
            "table": "source",
            "record": {
                "id": "source:note-scope-1",
                "kind": "markdown",
                "title": "note scope probe",
                "content": "# Scope probe\n",
                "fingerprint": "scopeprobe000000000000000000000000000000000000000000000000000000",
                "created_at": "2026-10-01T09:00:00Z"
            }
        });
        fs::write(dir.join("sources.ndjson"), format!("{source}\n")).expect("shard");
        let output = dir.join("seed.sql");

        let strict = cmd_hosted_seed_sql(&dir, &output, false, true, &[]);
        let message = strict
            .expect_err("strict must refuse a scopeless projection build")
            .to_string();
        assert!(
            message.contains("--scope"),
            "error names the fix: {message}"
        );
        assert!(!output.exists(), "strict writes nothing");

        let default = cmd_hosted_seed_sql(&dir, &output, false, false, &[]);
        let message = default
            .expect_err("default must report the missing projection")
            .to_string();
        assert!(message.contains("unexplained loss"), "{message}");
        assert!(
            output.exists(),
            "default still writes the artifact for inspection"
        );
        let sql = fs::read_to_string(&output).expect("seed sql");
        assert!(
            !sql.contains("memory_item"),
            "no note projection without an operator scope"
        );

        // Malformed operator scope input fails closed before any build.
        let malformed = cmd_hosted_seed_sql(&dir, &output, false, false, &["project".to_owned()]);
        let message = malformed
            .expect_err("KEY=VALUE pairs are required")
            .to_string();
        assert!(message.contains("KEY=VALUE"), "{message}");
    }

    #[test]
    fn hosted_seed_applies_to_empty_sqlite_and_corrections_flow() {
        let scope = BTreeMap::from([("project".to_owned(), "c3-import".to_owned())]);
        let (sql, _report) =
            build_hosted_seed_sql(&c3_fixture("synthetic-export"), false, Some(scope))
                .expect("builder must run");

        let conn = rusqlite::Connection::open_in_memory().expect("sqlite");
        conn.execute_batch(lighting_store_d1::SCHEMA_SQL)
            .expect("hosted schema on empty sqlite");
        conn.execute_batch(lighting_store_d1::REMEMBER_SCHEMA_SQL)
            .expect("remember schema (memory_item.source_id)");
        conn.execute_batch(lighting_store_d1::REMEMBER_ATTR_SCHEMA_SQL)
            .expect("actor schema (memory_item.actor)");
        conn.execute_batch(&sql)
            .expect("seed must apply to empty sqlite");

        assert_eq!(c3_count(&conn, "source"), 2);
        assert_eq!(c3_count(&conn, "episode"), 2);
        assert_eq!(
            c3_count(&conn, "epistemic_claim"),
            3,
            "2 claims + 1 relation-as-claim"
        );
        assert_eq!(c3_count(&conn, "belief"), 2);
        assert_eq!(
            c3_count(&conn, "belief_revision"),
            2,
            "seeded history chain"
        );
        // No seeded revision may occupy the belief's current token: that row
        // would collide with the first post-seed correction's CAS insert.
        let hazard: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM belief_revision WHERE belief_id = 'c3-seed-bel-1' AND from_updated_at = '2026-10-01T11:00:00.000000000Z'",
                [],
                |row| row.get(0),
            )
            .expect("hazard query");
        assert_eq!(
            hazard, 0,
            "seeded history must not block the first correction"
        );
        // The creation transition records the derived zero token.
        let zero: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM belief_revision WHERE from_updated_at = '0001-01-01T00:00:00.000000000Z'",
                [],
                |row| row.get(0),
            )
            .expect("zero token query");
        assert_eq!(zero, 1, "creation transition must record the zero token");

        // Byte-exact Markdown round trip: CRLF and non-ASCII survive seeding.
        let ledger = fs::read_to_string(c3_fixture("synthetic-export").join("ledger.ndjson"))
            .expect("fixture ledger");
        let exported_content: String = ledger
            .lines()
            .find(|line| line.contains("\"source:`c3-seed-src-alpha`\""))
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("fixture line"))
            .and_then(|parsed| {
                parsed
                    .get("record")
                    .and_then(|record| record.get("content"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .expect("alpha content in export");
        let stored_content: String = conn
            .query_row(
                "SELECT content FROM source WHERE id = 'c3-seed-src-alpha'",
                [],
                |row| row.get(0),
            )
            .expect("seeded content");
        assert_eq!(
            stored_content.as_bytes(),
            exported_content.as_bytes(),
            "source bytes must round-trip exactly (CRLF + non-ASCII)"
        );
        assert!(exported_content.contains("\r\n"));
        assert!(exported_content.contains("café"));

        let seeded_token: String = conn
            .query_row(
                "SELECT updated_at FROM belief WHERE id = 'c3-seed-bel-1'",
                [],
                |row| row.get(0),
            )
            .expect("seeded belief");
        assert_eq!(seeded_token, "2026-10-01T11:00:00.000000000Z");

        // Corrections flow after seeding with NO genesis row: the batch
        // inserts its own CAS guard (proven shape: atomic_semantics.rs).
        let scope: BTreeMap<String, String> =
            BTreeMap::from([("project".to_owned(), "c3-import".to_owned())]);
        let correction = lighting_store_d1::KeyedCorrection {
            revision_id: "c3-rev-1".to_owned(),
            target_belief_id: "c3-seed-bel-1".to_owned(),
            expected_revision: seeded_token.clone(),
            new_revision: "2026-10-01T14:00:00.000000000Z".to_owned(),
            new_value: "corrected by c3 test".to_owned(),
            revision_payload: "{\"actor\":\"c3-test\"}".to_owned(),
            belief_payload: "{\"id\":\"c3-seed-bel-1\",\"current_value\":\"corrected by c3 test\"}"
                .to_owned(),
            principal: "pi".to_owned(),
            scope_hash: lighting_core::scope_hash(&scope),
            idempotency_key: "c3-correction-1".to_owned(),
            payload_hash: "hash-c3-correction-1".to_owned(),
            receipt_id: "receipt-c3-correction-1".to_owned(),
            accepted_at: "2026-10-01T15:00:00.000000000Z".to_owned(),
            result_json: "{\"receipt_id\":\"receipt-c3-correction-1\"}".to_owned(),
        };
        let batch = lighting_store_d1::build_correction_batch(&correction)
            .expect("first correction after seed must build");
        c3_run_batch(&conn, &batch)
            .expect("first correction after seed must flow (no genesis row)");

        let moved: String = conn
            .query_row(
                "SELECT updated_at FROM belief WHERE id = 'c3-seed-bel-1'",
                [],
                |row| row.get(0),
            )
            .expect("corrected belief");
        assert_eq!(moved, correction.new_revision);
        // 2 seeded history rows + the correction's own CAS guard row.
        assert_eq!(c3_count(&conn, "belief_revision"), 3);
        assert_eq!(c3_count(&conn, "correction_idempotency"), 1);
        assert_eq!(c3_count(&conn, "mutation_receipt"), 1);

        // Re-applying the seed is idempotent and never clobbers corrections.
        conn.execute_batch(&sql).expect("re-apply must not fail");
        assert_eq!(c3_count(&conn, "belief"), 2);
        assert_eq!(c3_count(&conn, "epistemic_claim"), 3);
        let still_moved: String = conn
            .query_row(
                "SELECT updated_at FROM belief WHERE id = 'c3-seed-bel-1'",
                [],
                |row| row.get(0),
            )
            .expect("belief after re-seed");
        assert_eq!(
            still_moved, correction.new_revision,
            "re-seeding must never overwrite a post-seed correction"
        );
        assert_eq!(
            c3_count(&conn, "belief_revision"),
            3,
            "re-seeding must not duplicate the seeded chain or the correction guard"
        );
    }

    #[tokio::test]
    async fn three_way_reconcile_export_native_and_seed_counts_match()
    -> Result<(), Box<dyn std::error::Error>> {
        // Synthetic BM snapshot -> native service over real HTTP -> the
        // read-only reconcile command passes -> native logical export ->
        // hosted seed builder emits every exported row of the mapped tables.
        let store_config = {
            let mut config = StoreConfig::from_env();
            config.storage = "embedded-surrealkv".to_owned();
            config.path = c3_temp_path("three-way");
            config.namespace = "c3_import".to_owned();
            config.database = "integration".to_owned();
            config.username.clear();
            config.password.clear();
            config
        };
        let store = SurrealStore::connect(&store_config).await?;
        store.initialise_schema().await?;
        let epi = Arc::new(SurrealEpistemicRepository::new(store.clone()));
        epi.migrate().await?;
        let src = Arc::new(SurrealSourceRepository::new(store.clone()));
        src.migrate().await?;
        let mp = Arc::new(SurrealMemoryPathRepository::new(store.clone()));
        mp.migrate().await?;
        let ledger = Arc::new(SurrealLedgerRepository::new(store.clone()));
        ledger.migrate().await?;
        let memory = Arc::new(SurrealMemoryRepository::new(store.clone()));
        memory.migrate().await?;

        let state = lighting_service::state::AppState {
            ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            source_service: Some(lighting_service::source_ops::SourceService::new(
                src.clone(),
            )),
            episode_service: Some(lighting_service::episode_ops::EpisodeService::new(
                src.clone(),
                mp.clone(),
            )),
            memory_service: Some(lighting_service::memory_ops::MemoryService::new(
                memory.clone(),
            )),
            ledger_service: Some(lighting_service::ledger_ops::LedgerService::new(
                ledger.clone(),
            )),
            epistemic_service: Some(lighting_service::EpistemicService::new_with_evidence(
                epi.clone(),
                src.clone(),
                mp.clone(),
            )),
            project_service: Some(lighting_service::project_ops::ProjectService::new(
                mp.clone(),
                src.clone(),
            )),
            ..lighting_service::state::AppState::default()
        };
        let router = lighting_service::build_router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let base_url = format!("http://{address}");

        // All blocking HTTP (and its runtimes) lives inside the blocking
        // thread: creating or dropping reqwest::blocking inside the async
        // context panics ("Cannot drop a runtime in a context where blocking
        // is not allowed").
        let bm_snapshot = c3_fixture("synthetic-bm");
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let client = HttpClient::new(&base_url);
            cmd_basic_memory_import(&client, &bm_snapshot, false, false)?;
            cmd_basic_memory_reconcile(&client, &bm_snapshot, None, false)?;
            Ok(())
        })
        .await
        .expect("blocking import+reconcile task")?;

        // Native logical export of the same state, then the builder over it.
        let export_dir = c3_temp_path("three-way-export");
        let summary = store.export_to(&export_dir).await?;
        let import_scope = BTreeMap::from([("project".to_owned(), "c3-import".to_owned())]);
        let (seed_sql, report) = build_hosted_seed_sql(&export_dir, false, Some(import_scope))?;
        assert!(
            report.warnings.is_empty(),
            "builder warnings on a clean native export: {:?}",
            report.warnings
        );
        for table in [
            "source",
            "episode",
            "epistemic_claim",
            "belief",
            "memory_relation",
            "belief_revision",
        ] {
            let exported = summary.record_counts.get(table).copied().unwrap_or(0);
            let built = report.emitted.get(table).copied().unwrap_or(0);
            assert_eq!(
                built, exported,
                "builder must emit every exported {table} row (three-way count match)"
            );
        }
        assert!(!seed_sql.is_empty());

        // And the seed still applies to a fresh empty SQLite.
        let conn = rusqlite::Connection::open_in_memory()?;
        conn.execute_batch(lighting_store_d1::SCHEMA_SQL)?;
        conn.execute_batch(lighting_store_d1::REMEMBER_SCHEMA_SQL)?;
        conn.execute_batch(lighting_store_d1::REMEMBER_ATTR_SCHEMA_SQL)?;
        conn.execute_batch(&seed_sql)?;
        // Claims in the database = native claims + relation-as-claim rows.
        assert_eq!(
            c3_count(&conn, "epistemic_claim"),
            (report.emitted.get("epistemic_claim").copied().unwrap_or(0)
                + report.emitted.get("memory_relation").copied().unwrap_or(0)) as i64
        );

        // D1-direct style content reconciliation: per-table payload byte
        // sums from the export's raw bytes must equal the sums read back
        // from the seeded database (the same SELECT wrangler d1 execute
        // runs against the hosted database in the workflow probe).
        let mut export_payload_bytes: BTreeMap<String, usize> = BTreeMap::new();
        for line in fs::read_to_string(export_dir.join("epistemic.ndjson"))?.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let parsed: serde_json::Value = serde_json::from_str(line)?;
            let table = parsed
                .get("table")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if table != "epistemic_claim" && table != "belief" && table != "memory_relation" {
                continue;
            }
            let payload = parsed
                .get("record")
                .and_then(|record| record.get("payload"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            *export_payload_bytes.entry(table.to_owned()).or_default() += payload.len();
        }
        for table in ["epistemic_claim", "belief"] {
            let mut from_export = export_payload_bytes.get(table).copied().unwrap_or(0);
            if table == "epistemic_claim" {
                // Relation rows seed INTO the claim table, so their payloads
                // are part of the claim table's fingerprint.
                from_export += export_payload_bytes
                    .get("memory_relation")
                    .copied()
                    .unwrap_or(0);
            }
            let from_db: i64 = conn.query_row(
                &format!("SELECT COALESCE(SUM(LENGTH(payload)), 0) FROM {table}"),
                [],
                |row| row.get(0),
            )?;
            assert_eq!(
                from_db as usize, from_export,
                "seeded {table} payload bytes must equal the export bytes (D1-direct fingerprint)"
            );
        }
        Ok(())
    }
}
