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
use tokio::io::AsyncWriteExt;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::task::JoinError;
use tracing::{debug, warn};

use super::tethers_preview::{TethersRequest, TethersResponse};

// ── Constants ─────────────────────────────────────────────────────

/// Maximum stderr bytes captured into error messages.
const STDERR_LIMIT_BYTES: usize = 8192;

/// Maximum stdout bytes accepted from one engine response.
const STDOUT_LIMIT_BYTES: usize = 1024 * 1024;

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

    #[error("engine exited with status {code}")]
    NonZeroExit {
        code: i32,
        stderr: String,
        stderr_truncated: bool,
    },

    #[error("engine stdout exceeded {0} bytes")]
    StdoutTooLarge(usize),

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
///
/// In production this spawns the external engine binary.  Under `#[cfg(test)]`
/// a client can also be constructed with a pre-canned response via
/// `TethersEngineClient::fixed(…)` — the only injection seam exposed for
/// handler-level tests.
#[derive(Clone)]
pub struct TethersEngineClient {
    engine_path: PathBuf,
    engine_args: Vec<String>,
    timeout: Duration,
    /// Only populated via `#[cfg(test)] TethersEngineClient::fixed(…)`.
    #[cfg(test)]
    fixed_response: Option<TethersResponse>,
    /// Only populated via `#[cfg(test)] TethersEngineClient::fixed_error(…)`.
    #[cfg(test)]
    fixed_evaluate_error: Option<String>,
}

impl TethersEngineClient {
    /// Construct a client for the engine binary at `engine_path`.
    pub fn new(engine_path: PathBuf) -> Self {
        Self {
            engine_path,
            engine_args: Vec::new(),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            #[cfg(test)]
            fixed_response: None,
            #[cfg(test)]
            fixed_evaluate_error: None,
        }
    }

    /// Construct a client that always returns `response` without spawning
    /// a process.  Only available in tests — this is the seam that lets
    /// handler-level tests provide deterministic Tethers responses.
    #[cfg(test)]
    pub(crate) fn fixed(response: TethersResponse) -> Self {
        Self {
            engine_path: PathBuf::new(),
            engine_args: Vec::new(),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            fixed_response: Some(response),
            fixed_evaluate_error: None,
        }
    }

    /// Construct a client whose `evaluate()` always returns the given error.
    /// Only available in tests.
    #[cfg(test)]
    pub(crate) fn fixed_error(err: TethersEngineError) -> Self {
        Self {
            engine_path: PathBuf::new(),
            engine_args: Vec::new(),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            fixed_response: None,
            fixed_evaluate_error: Some(err.to_string()),
        }
    }

    #[cfg(test)]
    fn new_with_args(engine_path: PathBuf, engine_args: Vec<String>, timeout: Duration) -> Self {
        Self {
            engine_path,
            engine_args,
            timeout,
            fixed_response: None,
            fixed_evaluate_error: None,
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
        #[cfg(test)]
        if let Some(ref message) = self.fixed_evaluate_error {
            return Err(TethersEngineError::Io(std::io::Error::other(
                message.clone(),
            )));
        }
        #[cfg(test)]
        if let Some(ref response) = self.fixed_response {
            return Ok(response.clone());
        }
        let _ = request;
        self.evaluate_with_timeout(request, self.timeout).await
    }

    async fn evaluate_with_timeout(
        &self,
        request: &TethersRequest,
        timeout_dur: Duration,
    ) -> Result<TethersResponse, TethersEngineError> {
        // Serialise before spawning so a request error cannot leak a child.
        let mut payload =
            serde_json::to_string(request).map_err(TethersEngineError::SerializeFailed)?;
        payload.push('\n');

        let mut command = Command::new(&self.engine_path);
        command
            .args(&self.engine_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Backup only: Tokio 1.53 documents that strict cleanup should still
        // use child.kill().await or child.wait().await where possible.
        command.kill_on_drop(true);

        let mut child = command.spawn().map_err(TethersEngineError::SpawnFailed)?;

        let mut child_stdin = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("engine stdin was not piped"))?;
        let child_stdout = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("engine stdout was not piped"))?;
        let child_stderr = child
            .stderr
            .take()
            .ok_or_else(|| std::io::Error::other("engine stderr was not piped"))?;

        let stdout_task = tokio::spawn(read_bounded(child_stdout, STDOUT_LIMIT_BYTES, "stdout"));
        let stderr_task = tokio::spawn(read_bounded(child_stderr, STDERR_LIMIT_BYTES, "stderr"));

        let operation = async {
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

            child.wait().await.map_err(TethersEngineError::Io)
        };

        let exit_status = match tokio::time::timeout(timeout_dur, operation).await {
            Ok(result) => result?,
            Err(_elapsed) => {
                // child.kill().await in Tokio 1.53 sends the kill request and
                // then waits, giving this client a definite reap point.
                if let Err(error) = child.kill().await {
                    warn!(%error, "failed to kill timed-out Tethers engine");
                }

                let _ = await_reader(stdout_task).await;
                let _ = await_reader(stderr_task).await;
                return Err(TethersEngineError::Timeout(timeout_dur));
            }
        };

        let stdout = await_reader(stdout_task).await?;
        let stderr = await_reader(stderr_task).await?;

        if !exit_status.success() {
            let code = exit_status.code().unwrap_or(-1);
            debug!(
                code,
                stderr_len = stderr.text.len(),
                stderr_truncated = stderr.truncated,
                "engine exited unsuccessfully"
            );
            return Err(TethersEngineError::NonZeroExit {
                code,
                stderr: stderr.text,
                stderr_truncated: stderr.truncated,
            });
        }

        if stdout.truncated {
            return Err(TethersEngineError::StdoutTooLarge(STDOUT_LIMIT_BYTES));
        }

        // Extract exactly one nonblank response line.
        let trimmed: Vec<&str> = stdout
            .text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect();

        if trimmed.is_empty() {
            debug!(
                stderr_len = stderr.text.len(),
                stderr_preview = %stderr.text.chars().take(200).collect::<String>(),
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

#[derive(Debug)]
struct CapturedOutput {
    text: String,
    truncated: bool,
}

async fn read_bounded<R: AsyncRead + Unpin>(
    mut reader: R,
    limit: usize,
    stream_name: &'static str,
) -> std::io::Result<CapturedOutput> {
    let mut retained = Vec::new();
    let mut scratch = [0_u8; 8192];
    let mut truncated = false;

    loop {
        let read = reader.read(&mut scratch).await?;
        if read == 0 {
            break;
        }

        let remaining = limit.saturating_sub(retained.len());
        if remaining > 0 {
            let keep = remaining.min(read);
            retained.extend_from_slice(&scratch[..keep]);
        }
        if read > remaining {
            truncated = true;
        }
    }

    debug!(
        stream = stream_name,
        retained_len = retained.len(),
        truncated,
        "drained engine stream"
    );

    Ok(CapturedOutput {
        text: String::from_utf8_lossy(&retained).into_owned(),
        truncated,
    })
}

async fn await_reader(
    task: tokio::task::JoinHandle<std::io::Result<CapturedOutput>>,
) -> Result<CapturedOutput, TethersEngineError> {
    task.await
        .map_err(join_error_to_io)?
        .map_err(TethersEngineError::Io)
}

fn join_error_to_io(error: JoinError) -> TethersEngineError {
    TethersEngineError::Io(std::io::Error::other(error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tethers_preview::{build_preview_request, PreviewInput, TethersStatus};
    use std::fs;
    use std::process::Command as StdCommand;
    use std::time::{SystemTime, UNIX_EPOCH};

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
    fn stdout_and_stderr_limits_are_named_and_ordered() {
        let stderr_limit = STDERR_LIMIT_BYTES;
        let stdout_limit = STDOUT_LIMIT_BYTES;

        assert_eq!(stderr_limit, 8192);
        assert!(
            stdout_limit > stderr_limit,
            "stdout accepts full protocol responses; stderr only keeps diagnostics"
        );
    }

    // ── Controlled child-process tests ─────────────────────────

    fn sample_request() -> TethersRequest {
        build_preview_request(&PreviewInput {
            evaluation_id: "eval-child-001".into(),
            event_id: "evt-child-001".into(),
            project_id: "lantern-keeper".into(),
            task: "LK-39".into(),
            changed_files: 3,
        })
    }

    fn powershell_client(script: String, timeout: Duration) -> TethersEngineClient {
        TethersEngineClient::new_with_args(
            PathBuf::from("powershell.exe"),
            vec![
                "-NoProfile".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-Command".into(),
                script,
            ],
            timeout,
        )
    }

    fn minimal_error_json() -> &'static str {
        r#"{"protocol_version":"0.1","status":"error","error":{"code":"parse_error","message":"bad"}}"#
    }

    fn ps_single_quoted(value: &str) -> String {
        format!("'{}'", value.replace('\'', "''"))
    }

    fn unique_temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("lighting-{name}-{nanos}.txt"))
    }

    async fn read_pid_file(path: &PathBuf) -> u32 {
        for _ in 0..20 {
            if let Ok(text) = fs::read_to_string(path) {
                if let Ok(pid) = text.trim().parse::<u32>() {
                    return pid;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("timed-out child did not write its pid file");
    }

    fn process_is_running(pid: u32) -> bool {
        let script =
            format!("if (Get-Process -Id {pid} -ErrorAction SilentlyContinue) {{ exit 0 }} else {{ exit 1 }}");
        StdCommand::new("powershell.exe")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &script,
            ])
            .status()
            .expect("powershell should run")
            .success()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn timeout_kills_and_reaps_child_process() {
        let pid_file = unique_temp_path("tethers-timeout-pid");
        let script = format!(
            "Set-Content -LiteralPath {} -Value $PID; Start-Sleep -Seconds 60",
            ps_single_quoted(&pid_file.to_string_lossy())
        );
        let client = powershell_client(script, Duration::from_secs(2));

        let result = client.evaluate(&sample_request()).await;

        assert!(
            matches!(result, Err(TethersEngineError::Timeout(_))),
            "expected timeout, got: {result:?}"
        );

        let pid = read_pid_file(&pid_file).await;
        let _ = fs::remove_file(&pid_file);

        assert!(
            !process_is_running(pid),
            "timed-out child process {pid} should have been killed and reaped"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn large_stderr_is_drained_without_unbounded_diagnostics() {
        let stderr_bytes = STDERR_LIMIT_BYTES + 32_768;
        let script = format!(
            "[Console]::Error.Write(('x' * {stderr_bytes})); [Console]::Out.WriteLine({}); exit 37",
            ps_single_quoted(minimal_error_json())
        );
        let client = powershell_client(script, Duration::from_secs(5));

        let result = client.evaluate(&sample_request()).await;

        match result {
            Err(TethersEngineError::NonZeroExit {
                code,
                stderr,
                stderr_truncated,
            }) => {
                assert_eq!(code, 37);
                assert!(stderr_truncated, "stderr truncation should be identifiable");
                assert_eq!(stderr.len(), STDERR_LIMIT_BYTES);
            }
            other => panic!("expected nonzero exit with bounded stderr, got: {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn oversized_stdout_is_drained_and_rejected() {
        let stdout_bytes = STDOUT_LIMIT_BYTES + 1024;
        let script = format!("[Console]::Out.Write(('x' * {stdout_bytes})); exit 0");
        let client = powershell_client(script, Duration::from_secs(5));

        let result = client.evaluate(&sample_request()).await;

        assert!(
            matches!(
                result,
                Err(TethersEngineError::StdoutTooLarge(STDOUT_LIMIT_BYTES))
            ),
            "expected stdout limit error, got: {result:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn empty_output_is_distinguishable() {
        let client = powershell_client("exit 0".into(), Duration::from_secs(5));

        let result = client.evaluate(&sample_request()).await;

        assert!(
            matches!(result, Err(TethersEngineError::EmptyOutput)),
            "expected empty output, got: {result:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn invalid_json_is_distinguishable() {
        let script = "[Console]::Out.WriteLine('not-json'); exit 0".to_owned();
        let client = powershell_client(script, Duration::from_secs(5));

        let result = client.evaluate(&sample_request()).await;

        assert!(
            matches!(result, Err(TethersEngineError::ResponseParseFailed(_))),
            "expected invalid JSON error, got: {result:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn multiple_response_lines_are_distinguishable() {
        let script = format!(
            "[Console]::Out.WriteLine({0}); [Console]::Out.WriteLine({0}); exit 0",
            ps_single_quoted(minimal_error_json())
        );
        let client = powershell_client(script, Duration::from_secs(5));

        let result = client.evaluate(&sample_request()).await;

        assert!(
            matches!(result, Err(TethersEngineError::MultipleResponseLines)),
            "expected multiple response lines, got: {result:?}"
        );
    }

    // ── Live engine test (environment-driven) ──────────────────

    /// Sends `build_preview_request` through a real Tethers engine
    /// using the engine at `TETHERS_ENGINE_PATH`.
    ///
    /// Ignored by default because it requires the external OCaml engine to
    /// have been built deliberately.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires TETHERS_ENGINE_PATH pointing at a built Tethers engine"]
    async fn live_preview_request_matches() {
        let client =
            TethersEngineClient::from_env().expect("TETHERS_ENGINE_PATH must be set for live test");
        assert!(
            client.engine_path.exists(),
            "engine binary must exist at {}",
            client.engine_path.display()
        );

        let input = PreviewInput {
            evaluation_id: "live-test-eval-001".into(),
            event_id: "live-test-evt-001".into(),
            project_id: "lantern-keeper".into(),
            task: "LK-39".into(),
            changed_files: 3,
        };
        let request = build_preview_request(&input);

        let response = client
            .evaluate(&request)
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
