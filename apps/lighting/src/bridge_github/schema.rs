use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub const INTENT_SCHEMA_V1: &str = "lantern.intent.v1";
pub const RECEIPT_SCHEMA_V1: &str = "lantern.receipt.v1";
pub const MAX_INTENT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntentAction {
    #[serde(rename = "memory.create")]
    MemoryCreate,
    #[serde(rename = "memory.archive")]
    MemoryArchive,
    #[serde(rename = "memory.reinforce")]
    MemoryReinforce,
    #[serde(rename = "mirror.refresh")]
    MirrorRefresh,
}

impl IntentAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MemoryCreate => "memory.create",
            Self::MemoryArchive => "memory.archive",
            Self::MemoryReinforce => "memory.reinforce",
            Self::MirrorRefresh => "mirror.refresh",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestedBy {
    pub actor: String,
    pub transport: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentTarget {
    pub record_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentObserved {
    #[serde(default)]
    pub snapshot_generated_at: Option<String>,
    #[serde(default)]
    pub record_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub schema: String,
    pub intent_id: String,
    pub action: IntentAction,
    pub requested_by: RequestedBy,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub target: Option<IntentTarget>,
    #[serde(default)]
    pub observed: Option<IntentObserved>,
    #[serde(default)]
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReceiptStatus {
    Applied,
    Denied,
    RequiresApproval,
    Conflict,
    Expired,
    Failed,
    Indeterminate,
    Suspect,
}

impl ReceiptStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Applied => "APPLIED",
            Self::Denied => "DENIED",
            Self::RequiresApproval => "REQUIRES_APPROVAL",
            Self::Conflict => "CONFLICT",
            Self::Expired => "EXPIRED",
            Self::Failed => "FAILED",
            Self::Indeterminate => "INDETERMINATE",
            Self::Suspect => "SUSPECT",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MirrorRefreshStatus {
    NotRequested,
    Requested,
    Completed,
    Failed,
    Skipped,
}

impl MirrorRefreshStatus {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NotRequested => "NOT_REQUESTED",
            Self::Requested => "REQUESTED",
            Self::Completed => "COMPLETED",
            Self::Failed => "FAILED",
            Self::Skipped => "SKIPPED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptAuthority {
    pub decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptLantern {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub intent_id: String,
    pub intent_commit: String,
    pub status: ReceiptStatus,
    pub authority: ReceiptAuthority,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lantern: Option<ReceiptLantern>,
    pub processed_at: DateTime<Utc>,
    pub bridge_version: String,
    pub mirror_refresh: MirrorRefreshStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SchemaError {
    #[error("intent payload exceeds size limit of {0} bytes")]
    OversizedPayload(usize),
    #[error("failed to deserialize intent JSON: {0}")]
    JsonParse(String),
    #[error("unsupported schema version: expected '{0}', got '{1}'")]
    InvalidSchema(String, String),
    #[error(
        "invalid intent_id: '{0}' (must be 8-64 alphanumeric, hyphen, or underscore characters)"
    )]
    InvalidIntentId(String),
    #[error("actor or transport cannot be empty")]
    InvalidRequestedBy,
    #[error("action {0} requires target.record_id")]
    MissingTarget(String),
    #[error("target.record_id '{0}' is not a valid UUID")]
    InvalidRecordId(String),
    #[error("action {0} requires observed.record_sha256")]
    MissingObservedHash(String),
    #[error("observed.record_sha256 '{0}' is not a valid 64-char hex SHA-256")]
    InvalidObservedHash(String),
    #[error("action {0} payload is invalid: {1}")]
    InvalidPayload(String, String),
    #[error("intent contains forbidden executable pattern or arbitrary system path")]
    ForbiddenPattern,
}

pub fn validate_intent_id(id: &str) -> bool {
    let trimmed = id.trim();
    if trimmed.len() < 8 || trimmed.len() > 64 {
        return false;
    }
    trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn contains_forbidden_execution_pattern(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let forbidden_substrings = [
        "powershell",
        "cmd.exe",
        "/bin/sh",
        "/bin/bash",
        "python.eval",
        "sql.execute",
        "c:\\",
        "d:\\",
        "/etc/passwd",
        "/root",
        "rm -rf",
    ];
    for pattern in &forbidden_substrings {
        if lower.contains(pattern) {
            return true;
        }
    }
    // Check path traversal attempts
    if value.contains("../") || value.contains("..\\") {
        return true;
    }
    false
}

fn check_value_for_forbidden_patterns(val: &serde_json::Value) -> bool {
    match val {
        serde_json::Value::String(s) => contains_forbidden_execution_pattern(s),
        serde_json::Value::Array(arr) => arr.iter().any(check_value_for_forbidden_patterns),
        serde_json::Value::Object(map) => map.iter().any(|(k, v)| {
            contains_forbidden_execution_pattern(k) || check_value_for_forbidden_patterns(v)
        }),
        _ => false,
    }
}

pub fn parse_and_validate_intent(raw: &str) -> Result<Intent, SchemaError> {
    if raw.len() > MAX_INTENT_BYTES {
        return Err(SchemaError::OversizedPayload(raw.len()));
    }

    let intent: Intent =
        serde_json::from_str(raw).map_err(|e| SchemaError::JsonParse(e.to_string()))?;

    if intent.schema != INTENT_SCHEMA_V1 {
        return Err(SchemaError::InvalidSchema(
            INTENT_SCHEMA_V1.to_string(),
            intent.schema,
        ));
    }

    if !validate_intent_id(&intent.intent_id) {
        return Err(SchemaError::InvalidIntentId(intent.intent_id));
    }

    if intent.requested_by.actor.trim().is_empty()
        || intent.requested_by.transport.trim().is_empty()
    {
        return Err(SchemaError::InvalidRequestedBy);
    }

    if check_value_for_forbidden_patterns(&intent.payload) {
        return Err(SchemaError::ForbiddenPattern);
    }

    match intent.action {
        IntentAction::MemoryCreate => {
            let content = intent.payload.get("content").and_then(|v| v.as_str());
            if content.map(str::trim).unwrap_or("").is_empty() {
                return Err(SchemaError::InvalidPayload(
                    intent.action.as_str().to_string(),
                    "payload.content must be a non-empty string".to_string(),
                ));
            }
            if let Some(kind) = intent.payload.get("kind").and_then(|v| v.as_str())
                && kind.trim().is_empty()
            {
                return Err(SchemaError::InvalidPayload(
                    intent.action.as_str().to_string(),
                    "payload.kind must not be blank".to_string(),
                ));
            }
            if let Some(project_id) = intent.payload.get("project_id").and_then(|v| v.as_str())
                && Uuid::parse_str(project_id).is_err()
            {
                return Err(SchemaError::InvalidPayload(
                    intent.action.as_str().to_string(),
                    format!("payload.project_id '{project_id}' is not a valid UUID"),
                ));
            }
        }
        IntentAction::MemoryArchive => {
            let target = intent
                .target
                .as_ref()
                .ok_or_else(|| SchemaError::MissingTarget(intent.action.as_str().to_string()))?;
            if Uuid::parse_str(&target.record_id).is_err() {
                return Err(SchemaError::InvalidRecordId(target.record_id.clone()));
            }
            let observed = intent.observed.as_ref().ok_or_else(|| {
                SchemaError::MissingObservedHash(intent.action.as_str().to_string())
            })?;
            let hash = observed.record_sha256.as_deref().ok_or_else(|| {
                SchemaError::MissingObservedHash(intent.action.as_str().to_string())
            })?;
            let hash_clean = hash.strip_prefix("sha256:").unwrap_or(hash);
            if hash_clean.len() != 64 || !hash_clean.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(SchemaError::InvalidObservedHash(hash.to_string()));
            }
        }
        IntentAction::MemoryReinforce => {
            let target = intent
                .target
                .as_ref()
                .ok_or_else(|| SchemaError::MissingTarget(intent.action.as_str().to_string()))?;
            if Uuid::parse_str(&target.record_id).is_err() {
                return Err(SchemaError::InvalidRecordId(target.record_id.clone()));
            }
        }
        IntentAction::MirrorRefresh => {
            // Mirror refresh requires no target or observed preconditions
        }
    }

    Ok(intent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_memory_create_intent() {
        let json = r#"{
            "schema": "lantern.intent.v1",
            "intent_id": "01K999ABCDEF123456",
            "action": "memory.create",
            "requested_by": {
                "actor": "chatgpt-lucy",
                "transport": "github"
            },
            "created_at": "2026-10-02T05:00:00Z",
            "payload": {
                "content": "Rust Edition 2024 is pinned across the workspace",
                "kind": "fact"
            }
        }"#;

        let intent = parse_and_validate_intent(json).expect("valid intent should parse");
        assert_eq!(intent.intent_id, "01K999ABCDEF123456");
        assert_eq!(intent.action, IntentAction::MemoryCreate);
        assert_eq!(intent.requested_by.actor, "chatgpt-lucy");
    }

    #[test]
    fn valid_memory_archive_intent() {
        let json = r#"{
            "schema": "lantern.intent.v1",
            "intent_id": "01K999ARCHIVE000001",
            "action": "memory.archive",
            "requested_by": {
                "actor": "pi",
                "transport": "github"
            },
            "created_at": "2026-10-02T05:00:00Z",
            "target": {
                "record_id": "ed7e2de6-2d4d-464a-903d-f41df1b990f3"
            },
            "observed": {
                "record_sha256": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            }
        }"#;

        let intent = parse_and_validate_intent(json).expect("valid archive should parse");
        assert_eq!(intent.action, IntentAction::MemoryArchive);
        assert_eq!(
            intent.target.as_ref().unwrap().record_id,
            "ed7e2de6-2d4d-464a-903d-f41df1b990f3"
        );
    }

    #[test]
    fn rejects_invalid_schema_version() {
        let json = r#"{
            "schema": "lantern.intent.v2",
            "intent_id": "01K999ABCDEF123456",
            "action": "memory.create",
            "requested_by": {"actor": "lucy", "transport": "github"},
            "created_at": "2026-10-02T05:00:00Z",
            "payload": {"content": "text"}
        }"#;
        let err = parse_and_validate_intent(json).unwrap_err();
        assert!(matches!(err, SchemaError::InvalidSchema(_, _)));
    }

    #[test]
    fn rejects_unknown_action() {
        let json = r#"{
            "schema": "lantern.intent.v1",
            "intent_id": "01K999ABCDEF123456",
            "action": "shell.run",
            "requested_by": {"actor": "lucy", "transport": "github"},
            "created_at": "2026-10-02T05:00:00Z",
            "payload": {"command": "whoami"}
        }"#;
        let err = parse_and_validate_intent(json).unwrap_err();
        assert!(matches!(err, SchemaError::JsonParse(_)));
    }

    #[test]
    fn rejects_unknown_consequential_field() {
        let json = r#"{
            "schema": "lantern.intent.v1",
            "intent_id": "01K999ABCDEF123456",
            "action": "memory.create",
            "requested_by": {"actor": "lucy", "transport": "github"},
            "created_at": "2026-10-02T05:00:00Z",
            "unexpected_root_field": true,
            "payload": {"content": "text"}
        }"#;
        let err = parse_and_validate_intent(json).unwrap_err();
        assert!(matches!(err, SchemaError::JsonParse(_)));
    }

    #[test]
    fn rejects_malformed_id() {
        let json = r#"{
            "schema": "lantern.intent.v1",
            "intent_id": "short",
            "action": "memory.create",
            "requested_by": {"actor": "lucy", "transport": "github"},
            "created_at": "2026-10-02T05:00:00Z",
            "payload": {"content": "text"}
        }"#;
        let err = parse_and_validate_intent(json).unwrap_err();
        assert!(matches!(err, SchemaError::InvalidIntentId(_)));
    }

    #[test]
    fn rejects_oversized_payload() {
        let big_str = "x".repeat(MAX_INTENT_BYTES + 10);
        let err = parse_and_validate_intent(&big_str).unwrap_err();
        assert!(matches!(err, SchemaError::OversizedPayload(_)));
    }

    #[test]
    fn rejects_arbitrary_filesystem_path_attempt() {
        let json = r#"{
            "schema": "lantern.intent.v1",
            "intent_id": "01K999ABCDEF123456",
            "action": "memory.create",
            "requested_by": {"actor": "lucy", "transport": "github"},
            "created_at": "2026-10-02T05:00:00Z",
            "payload": {"content": "test", "path": "C:\\Windows\\System32"}
        }"#;
        let err = parse_and_validate_intent(json).unwrap_err();
        assert_eq!(err, SchemaError::ForbiddenPattern);
    }

    #[test]
    fn rejects_executable_style_payload() {
        let json = r#"{
            "schema": "lantern.intent.v1",
            "intent_id": "01K999ABCDEF123456",
            "action": "memory.create",
            "requested_by": {"actor": "lucy", "transport": "github"},
            "created_at": "2026-10-02T05:00:00Z",
            "payload": {"content": "test", "exec": "powershell -Command whoami"}
        }"#;
        let err = parse_and_validate_intent(json).unwrap_err();
        assert_eq!(err, SchemaError::ForbiddenPattern);
    }

    #[test]
    fn valid_receipt_round_trip() {
        let receipt = Receipt {
            schema: RECEIPT_SCHEMA_V1.to_string(),
            intent_id: "01K999ABCDEF123456".to_string(),
            intent_commit: "abcdef1234567890abcdef1234567890abcdef12".to_string(),
            status: ReceiptStatus::Applied,
            authority: ReceiptAuthority {
                decision: "ALLOW".to_string(),
                reason: None,
            },
            lantern: Some(ReceiptLantern {
                record_id: Some("ed7e2de6-2d4d-464a-903d-f41df1b990f3".to_string()),
                result_sha256: Some("sha256:abcd".to_string()),
            }),
            processed_at: Utc::now(),
            bridge_version: "0.1.0".to_string(),
            mirror_refresh: MirrorRefreshStatus::Completed,
            error: None,
        };

        let serialized = serde_json::to_string_pretty(&receipt).expect("serialize");
        let deserialized: Receipt = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(receipt.intent_id, deserialized.intent_id);
        assert_eq!(receipt.status, deserialized.status);
    }
}
