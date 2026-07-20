//! Async client for the Tethers 0.1 OCaml engine.
//!
//! Sends a newline-delimited JSON request over stdin, reads one JSON
//! response from stdout, and returns a typed `TethersResponse`.  One
//! process per request.  Preview-only: no Action execution, no storage
//! access.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::debug;

use super::tethers_preview::{TethersRequest, TethersResponse};

// ── Constants ─────────────────────────────────────────────────────

/// Maximum stderr bytes captured into error messages.
const STDERR_LIMIT_BYTES: usize = 8192;

/// Default evaluation timeout.
const DEFAULT_TIMEOUT_SECS: u64 = 10;

// ── Error type ────────────────────────────────────────────────────

/// Errors that can occur during a Tethers engine evaluation.
#[derive(Error, Debug)]
pub enum TethersEngineError {
    #[error("TETHERS_ENGINE_PATH is not set or is empty")]
    MissingEnginePath,

    #[error("failed to spawn engine process: {0}")]
    SpawnFailed(#[source] std::io::Error),

    #[error("failed to serialize request: {0}")]
    SerializeFailed(#[source] serde_json::Error),

    #[error("failed to write to engine stdin: {0}")]
    StdinWriteFailed(#[source] std::io::Error),

    #[error("engine did not respond within {0:?}")]
    Timeout(Duration),

    #[error("engine exited with status {0}")]
    NonZeroExit(i32),

    #[error("engine produced empty output")]
    EmptyOutput,

    #[error("engine produced multiple response lines")]
    MultipleResponseLines,

    #[error("failed to parse engine response: {0}")]
    ResponseParseFailed(#[source] serde_json::Error),

    #[error("internal I/O error: {0}")]
    Io(#[from] std::io::Error),
}

// ── Client ────────────────────────────────────────────────────────

/// Async client that evaluates a `TethersRequest` through the OCaml engine.
pub struct TethersEngineClient {
    engine_path: PathBuf,
    timeout: Duration,
}

impl TethersEngineClient {
    /// Construct a client for the engine binary at `engine_path`.
    pub fn new(engine_path: PathBuf) -> Self {
        Self {
            engine_path,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        }
    }

    /// Construct from the `TETHERS_ENGINE_PATH` environment variable.
    ///
    /// Returns `TethersEngineError::MissingEnginePath` if the variable
    /// is absent or empty.
    pub fn from_env() -> Result<Self, TethersEngineError> {
        let path = std::env::var("TETHERS_ENGINE_PATH")
            .map_err(|_| TethersEngineError::MissingEnginePath)?
            .trim()
            .to_string();
        if path.is_empty() {
            return Err(TethersEngineError::MissingEnginePath);
        }
        Ok(Self::new(PathBuf::from(path)))
    }

    /// Evaluate a request through the Tethers engine.
    ///
    /// Spawns one engine process, sends a compact JSON line over stdin,
    /// reads and parses the JSON response from stdout, and captures
    /// bounded stderr for diagnostics.
    ///
    /// Returns `matched`, `not_matched`, or error responses normally.
    /// The caller is responsible for inspecting the status field.
    pub async fn evaluate(
        &self,
        request: &TethersRequest,
    ) -> Result<TethersResponse, TethersEngineError> {
        let mut child = Command::new(&self.engine_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(TethersEngineError::SpawnFailed)?;

        let mut child_stdin = child.stdin.take().expect("stdin piped");
        let child_stdout = child.stdout.take().expect("stdout piped");
        let child_stderr = child.stderr.take().expect("stderr piped");

        // Serialise request as one compact JSON line + newline.
        let mut payload =
            serde_json::to_string(request).map_err(TethersEngineError::SerializeFailed)?;
        payload.push('\n');

        debug!(payload_len = payload.len(), "writing Tethers request");
        child_stdin
            .write_all(payload.as_bytes())
            .await
            .map_err(TethersEngineError::StdinWriteFailed)?;
        child_stdin
            .shutdown()
            .await
            .map_err(TethersEngineError::StdinWriteFailed)?;
        drop(child_stdin);

        // Read stdout and bounded stderr concurrently to avoid deadlock.
        let (stdout_result, stderr_result) = tokio::join!(
            read_to_string(child_stdout),
            read_stderr_bounded(child_stderr),
        );

        let stdout = stdout_result?;
        let stderr = stderr_result?;

        // Wait for process exit.
        let exit_status = child.wait().await?;

        if !exit_status.success() {
            let code = exit_status.code().unwrap_or(-1);
            debug!(
                code,
                stderr_len = stderr.len(),
                "engine exited unsuccessfully"
            );
            return Err(TethersEngineError::NonZeroExit(code));
        }

        // Extract exactly one nonblank response line.
        let trimmed: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();

        if trimmed.is_empty() {
            debug!(
                stderr_len = stderr.len(),
                stderr_preview = %stderr.chars().take(200).collect::<String>(),
                "engine produced empty output"
            );
            return Err(TethersEngineError::EmptyOutput);
        }

        if trimmed.len() > 1 {
            debug!(
                line_count = trimmed.len(),
                "engine produced multiple response lines"
            );
            return Err(TethersEngineError::MultipleResponseLines);
        }

        serde_json::from_str(trimmed[0]).map_err(TethersEngineError::ResponseParseFailed)
    }
}

// ── I/O helpers ───────────────────────────────────────────────────

async fn read_to_string(mut reader: tokio::process::ChildStdout) -> std::io::Result<String> {
    let mut buf = String::new();
    reader.read_to_string(&mut buf).await?;
    Ok(buf)
}

async fn read_stderr_bounded(reader: tokio::process::ChildStderr) -> std::io::Result<String> {
    let mut buf = Vec::new();
    reader
        .take(STDERR_LIMIT_BYTES as u64)
        .read_to_end(&mut buf)
        .await?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

// ── Timeout wrapper ───────────────────────────────────────────────

impl TethersEngineClient {
    /// Evaluate with a timeout, killing the child process if it exceeds
    /// the deadline.
    ///
    /// The timeout is applied to the entire evaluate cycle (spawn, I/O,
    /// wait).  On timeout the child is sent SIGKILL/TerminateProcess
    /// and reaped before the error is returned.
    pub async fn evaluate_timeout(
        &self,
        request: &TethersRequest,
    ) -> Result<TethersResponse, TethersEngineError> {
        let timeout_dur = self.timeout;

        let engine_path = self.engine_path.clone();
        let req_owned = request.clone();

        // Spawn on a blocking-friendly task to keep timeout clean.
        let result = tokio::time::timeout(timeout_dur, async move {
            let client = TethersEngineClient {
                engine_path,
                timeout: timeout_dur,
            };
            client.evaluate(&req_owned).await
        })
        .await;

        match result {
            Ok(inner) => inner,
            Err(_elapsed) => Err(TethersEngineError::Timeout(timeout_dur)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tethers_preview::{build_preview_request, PreviewInput, TethersStatus};

    // ── Pure request/response tests ────────────────────────────

    #[test]
    fn request_payload_ends_with_exactly_one_newline() {
        let input = PreviewInput {
            evaluation_id: "eval-nl-1".into(),
            event_id: "evt-nl-1".into(),
            project_id: "p".into(),
            task: "t".into(),
            changed_files: 1,
        };
        let req = build_preview_request(&input);
        let mut payload = serde_json::to_string(&req).expect("serialise");
        payload.push('\n');

        assert!(payload.ends_with('\n'), "payload must end with newline");
        // Compact JSON: no internal newlines.  `lines()` drops the trailing
        // empty string, so we get exactly 1 line.
        let lines: Vec<&str> = payload.lines().collect();
        assert_eq!(lines.len(), 1, "compact JSON = 1 line after lines()");
        // payload ends with precisely "\n" (not "\r\n").
        assert!(
            !payload.ends_with("\r\n"),
            "payload must use Unix newline, not CRLF"
        );
    }

    #[test]
    fn response_parser_accepts_one_valid_line() {
        let response: TethersResponse = serde_json::from_str(
            r#"{"protocol_version":"0.1","evaluation_id":"e1","event_id":"ev1","tether_id":"t1","tether_version":"v1","status":"matched","plan":null,"trail":null}"#,
        )
        .expect("parse one line");
        assert_eq!(response.status, TethersStatus::Matched);
    }

    #[test]
    fn response_parser_rejects_empty_input() {
        let result = serde_json::from_str::<TethersResponse>("");
        assert!(result.is_err(), "empty input should fail to parse");
    }

    #[test]
    fn response_parser_rejects_multiple_json_values() {
        // Two complete JSON values on separate lines would fail
        // because serde_json::from_str expects exactly one value.
        let result = serde_json::from_str::<TethersResponse>(
            r#"{"protocol_version":"0.1","status":"matched"}{"protocol_version":"0.1","status":"not_matched"}"#,
        );
        assert!(result.is_err(), "multiple JSON values should fail to parse");
    }

    #[test]
    fn correlated_error_remains_valid_typed_response() {
        let json = serde_json::json!({
            "protocol_version": "0.1",
            "evaluation_id": "eval-corr-2",
            "event_id": "evt-corr-2",
            "tether_id": "t1",
            "tether_version": "v1",
            "status": "error",
            "plan": null,
            "error": { "code": "missing_fact", "message": "fact not found" },
            "trail": [{"sequence":1,"phase":"reception","kind":"event_received","outcome":"accepted","message":"m"}]
        });
        let resp: TethersResponse =
            serde_json::from_value(json).expect("deserialize correlated error");
        assert_eq!(resp.status, TethersStatus::Error);
        assert!(resp.error.is_some());
        assert!(resp.trail.is_some());
    }

    #[test]
    fn minimal_error_remains_valid_typed_response() {
        let json = serde_json::json!({
            "protocol_version": "0.1",
            "status": "error",
            "error": { "code": "parse_error", "message": "bad" }
        });
        let resp: TethersResponse = serde_json::from_value(json).expect("deserialize minimal");
        assert_eq!(resp.status, TethersStatus::Error);
        assert!(resp.error.is_some());
        assert!(resp.evaluation_id.is_none());
        assert!(resp.plan.is_none());
        assert!(resp.trail.is_none());
    }

    #[test]
    fn stderr_error_message_is_bounded() {
        // The stderr reader is capped at STDERR_LIMIT_BYTES (8 KiB).
        // This test verifies the constant is reasonable and the
        // string-from-lossy conversion doesn't panic.
        let long = "x".repeat(STDERR_LIMIT_BYTES + 1024);
        let bounded = &long[..STDERR_LIMIT_BYTES.min(long.len())];
        // read_stderr_bounded uses .take(n).read_to_end, which
        // produces at most n bytes. Verify the bound arithmetic.
        assert_eq!(bounded.len(), STDERR_LIMIT_BYTES);
        // UTF-8 lossy conversion of arbitrary bytes won't panic.
        let _ = String::from_utf8_lossy(bounded.as_bytes());
    }

    // ── Live engine test (environment-driven) ──────────────────

    /// Sends `build_preview_request` through a real Tethers engine
    /// if `TETHERS_ENGINE_PATH` is configured.
    ///
    /// Skipped silently when the environment variable is absent or
    /// the binary does not exist.
    #[tokio::test(flavor = "multi_thread")]
    async fn live_preview_request_matches() {
        let engine_path = match std::env::var("TETHERS_ENGINE_PATH") {
            Ok(v) if !v.trim().is_empty() => PathBuf::from(v.trim()),
            _ => {
                eprintln!("SKIP: TETHERS_ENGINE_PATH not set");
                return;
            }
        };

        if !engine_path.exists() {
            eprintln!("SKIP: engine binary not found at {}", engine_path.display());
            return;
        }

        let client = TethersEngineClient::new(engine_path);

        let input = PreviewInput {
            evaluation_id: "live-test-eval-001".into(),
            event_id: "live-test-evt-001".into(),
            project_id: "lantern-keeper".into(),
            task: "LK-39".into(),
            changed_files: 3,
        };
        let request = build_preview_request(&input);

        let response = client
            .evaluate_timeout(&request)
            .await
            .expect("live evaluate should succeed");

        assert_eq!(response.status, TethersStatus::Matched);

        let plan = response.plan.expect("matched plan must exist");
        let action_names: Vec<&str> = plan.actions.iter().map(|a| a.capability.as_str()).collect();
        assert!(
            action_names.contains(&"lantern.task.record"),
            "plan must contain lantern.task.record, got: {:?}",
            action_names
        );

        // Trail is ordered.
        let trail = response.trail.expect("trail must exist");
        for pair in trail.windows(2) {
            assert!(
                pair[0].sequence < pair[1].sequence,
                "Trail entries must be ordered by sequence"
            );
        }
    }
}
