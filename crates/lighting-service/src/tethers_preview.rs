//! Tethers 0.1 preview protocol DTOs and pure request builder.
//!
//! Models the frozen Tethers 0.1 JSON contract.  No process spawning,
//! storage access, or async code.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Status enum ───────────────────────────────────────────────────

/// Tethers 0.1 evaluation status.
///
/// The engine returns exactly one of these three status values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TethersStatus {
    Matched,
    NotMatched,
    Error,
}

// ── Request DTOs ──────────────────────────────────────────────────

/// Top-level Tethers 0.1 engine request.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct TethersRequest {
    pub protocol_version: String,
    pub language_version: String,
    pub evaluation_id: String,
    pub tether: TetherInfo,
    pub event: EventInfo,
    pub facts: Value,
    pub capabilities: Vec<CapabilitySchema>,
}

/// The Tether definition inside a request.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TetherInfo {
    pub id: String,
    pub version: String,
    pub source: String,
}

/// The event envelope.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct EventInfo {
    pub id: String,
    pub name: String,
    pub data: Value,
}

/// A Capability schema supplied by the host.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CapabilitySchema {
    pub name: String,
    pub version: String,
    pub inputs: Value,
    pub effects: Vec<String>,
    pub reversibility: String,
}

// ── Response DTOs ─────────────────────────────────────────────────

/// Top-level Tethers 0.1 engine response.
///
/// Supports all three statuses:
/// - `matched` — identifiers, plan, trail
/// - `not_matched` — identifiers, no plan, trail
/// - `error` — minimal envelope (protocol_version + status + error only)
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct TethersResponse {
    pub protocol_version: String,

    #[serde(default)]
    pub evaluation_id: Option<String>,
    #[serde(default)]
    pub event_id: Option<String>,
    #[serde(default)]
    pub tether_id: Option<String>,
    #[serde(default)]
    pub tether_version: Option<String>,

    pub status: TethersStatus,

    #[serde(default)]
    pub plan: Option<Plan>,

    #[serde(default)]
    pub trail: Option<Vec<TrailEntry>>,

    #[serde(default)]
    pub error: Option<TethersError>,
}

impl Serialize for TethersResponse {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;

        // Determine how many keys we need.
        let mut len = 2; // protocol_version + status are always present
        let has_correlation = self.evaluation_id.is_some();
        if has_correlation {
            len += 4; // evaluation_id, event_id, tether_id, tether_version
        }
        // plan key is always present when we have correlation (matched, not_matched, correlated error)
        // or when status is matched/not_matched with plan
        let emit_plan = has_correlation || self.plan.is_some();
        if emit_plan {
            len += 1;
        }
        if self.trail.is_some() {
            len += 1;
        }
        if self.error.is_some() {
            len += 1;
        }

        let mut map = serializer.serialize_map(Some(len))?;
        map.serialize_entry("protocol_version", &self.protocol_version)?;

        if let Some(ref v) = self.evaluation_id {
            map.serialize_entry("evaluation_id", v)?;
        }
        if let Some(ref v) = self.event_id {
            map.serialize_entry("event_id", v)?;
        }
        if let Some(ref v) = self.tether_id {
            map.serialize_entry("tether_id", v)?;
        }
        if let Some(ref v) = self.tether_version {
            map.serialize_entry("tether_version", v)?;
        }

        map.serialize_entry("status", &self.status)?;

        if emit_plan {
            // Explicit null for not_matched and correlated error; object for matched
            map.serialize_entry("plan", &self.plan)?;
        }
        if let Some(ref v) = self.trail {
            map.serialize_entry("trail", v)?;
        }
        if let Some(ref v) = self.error {
            map.serialize_entry("error", v)?;
        }

        map.end()
    }
}

/// A proposed Action Plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct Plan {
    pub id: String,
    pub required_effects: Vec<String>,
    pub actions: Vec<PlannedAction>,
}

/// One ordered Action within a Plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct PlannedAction {
    pub action_id: String,
    pub idempotency_key: String,
    pub capability: String,
    pub capability_version: String,
    pub arguments: Value,
    pub effects: Vec<String>,
}

/// One entry in the evaluation Trail.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrailEntry {
    pub sequence: u64,
    pub phase: String,
    pub kind: String,
    pub outcome: String,
    pub message: String,
}

/// Error payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TethersError {
    pub code: String,
    pub message: String,
}

// ── Preview input ─────────────────────────────────────────────────

/// Explicit input for building a Tethers preview request.
#[derive(Debug, Clone)]
pub struct PreviewInput {
    pub evaluation_id: String,
    pub event_id: String,
    pub project_id: String,
    pub task: String,
    pub changed_files: u64,
}

// ── Capability constants ──────────────────────────────────────────

const CAPABILITY_NAME: &str = "lantern.task.record";
const CAPABILITY_VERSION: &str = "1.0.0";
const CAPABILITY_REVERSIBILITY: &str = "compensatable";
const EFFECT_LANTERN_WRITE: &str = "lantern.write";

/// Canonical Tethers 0.1 source for the preview Tether.
///
/// Uses the exact indentation convention from the frozen protocol fixtures:
/// 4-space indentation for anchor body, condition body, and action body.
const TETHER_SOURCE: &str = r#"tether "Preview project result"

anchor
    lantern.project_result_preview_requested

when
    project.type is "software"
    and task.changed_files greater_than 0

do
    lantern.task.record
        project: anchor.project
        task: anchor.task
"#;

// ── Builder ───────────────────────────────────────────────────────

/// Build an infallible Tethers 0.1 preview request from the given input.
///
/// The returned request is ready for JSON serialisation and submission
/// to the Tethers engine.
pub fn build_preview_request(input: &PreviewInput) -> TethersRequest {
    let event_data = serde_json::json!({
        "project": input.project_id,
        "task": input.task,
    });

    let facts = serde_json::json!({
        "project.type": "software",
        "task.changed_files": input.changed_files,
    });

    let capability_inputs = serde_json::json!({
        "project": "string",
        "task": "string",
    });

    TethersRequest {
        protocol_version: "0.1".into(),
        language_version: "0.1".into(),
        evaluation_id: input.evaluation_id.clone(),
        tether: TetherInfo {
            id: "preview-project-result".into(),
            version: "0.1".into(),
            source: TETHER_SOURCE.into(),
        },
        event: EventInfo {
            id: input.event_id.clone(),
            name: "lantern.project_result_preview_requested".into(),
            data: event_data,
        },
        facts,
        capabilities: vec![CapabilitySchema {
            name: CAPABILITY_NAME.into(),
            version: CAPABILITY_VERSION.into(),
            inputs: capability_inputs,
            effects: vec![EFFECT_LANTERN_WRITE.into()],
            reversibility: CAPABILITY_REVERSIBILITY.into(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helpers ────────────────────────────────────────────────

    fn sample_input() -> PreviewInput {
        PreviewInput {
            evaluation_id: "eval-test-001".into(),
            event_id: "evt-test-001".into(),
            project_id: "lantern-keeper".into(),
            task: "LK-39".into(),
            changed_files: 3,
        }
    }

    fn matched_response_json() -> serde_json::Value {
        serde_json::json!({
            "protocol_version": "0.1",
            "evaluation_id": "eval_demo_001",
            "event_id": "evt_demo_001",
            "tether_id": "record-completed-task",
            "tether_version": "demo-v1",
            "status": "matched",
            "plan": {
                "id": "eval_demo_001/plan",
                "required_effects": ["lantern.write"],
                "actions": [
                    {
                        "action_id": "action_1",
                        "idempotency_key": "eval_demo_001/action_1",
                        "capability": "lantern.task.record",
                        "capability_version": "1.0.0",
                        "arguments": {"project": "lantern-keeper", "task": "LK-39"},
                        "effects": ["lantern.write"]
                    }
                ]
            },
            "trail": [
                {
                    "sequence": 1,
                    "phase": "reception",
                    "kind": "event_received",
                    "outcome": "accepted",
                    "message": "Received coding.task_completed"
                }
            ]
        })
    }

    // ── Builder tests ──────────────────────────────────────────

    #[test]
    fn builder_produces_expected_json_structure() {
        let input = sample_input();
        let req = build_preview_request(&input);
        let json = serde_json::to_value(&req).expect("serialize");

        // Top-level keys
        assert_eq!(json["protocol_version"], "0.1");
        assert_eq!(json["language_version"], "0.1");
        assert_eq!(json["evaluation_id"], "eval-test-001");

        // Tether
        assert_eq!(json["tether"]["id"], "preview-project-result");
        assert_eq!(json["tether"]["version"], "0.1");
        assert!(
            json["tether"]["source"]
                .as_str()
                .expect("source string")
                .contains("lantern.project_result_preview_requested")
        );

        // Event
        assert_eq!(json["event"]["id"], "evt-test-001");
        assert_eq!(
            json["event"]["name"],
            "lantern.project_result_preview_requested"
        );
        assert_eq!(json["event"]["data"]["project"], "lantern-keeper");
        assert_eq!(json["event"]["data"]["task"], "LK-39");

        // Facts
        assert_eq!(json["facts"]["project.type"], "software");
        assert_eq!(json["facts"]["task.changed_files"], 3);

        // Capabilities
        let caps = json["capabilities"].as_array().expect("capabilities array");
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0]["name"], "lantern.task.record");
        assert_eq!(caps[0]["version"], "1.0.0");
        assert_eq!(caps[0]["inputs"]["project"], "string");
        assert_eq!(caps[0]["inputs"]["task"], "string");
        assert_eq!(caps[0]["effects"][0], "lantern.write");
        assert_eq!(caps[0]["reversibility"], "compensatable");
    }

    #[test]
    fn correlation_ids_come_from_caller() {
        let input = PreviewInput {
            evaluation_id: "caller-eval-42".into(),
            event_id: "caller-evt-99".into(),
            project_id: "p".into(),
            task: "t".into(),
            changed_files: 1,
        };
        let req = build_preview_request(&input);

        assert_eq!(req.evaluation_id, "caller-eval-42");
        assert_eq!(req.event.id, "caller-evt-99");
    }

    #[test]
    fn event_data_facts_map_correctly() {
        let input = PreviewInput {
            evaluation_id: "e1".into(),
            event_id: "ev1".into(),
            project_id: "my-project".into(),
            task: "TASK-001".into(),
            changed_files: 7,
        };
        let req = build_preview_request(&input);

        let event_data = &req.event.data;
        assert_eq!(event_data["project"], "my-project");
        assert_eq!(event_data["task"], "TASK-001");

        let facts = &req.facts;
        assert_eq!(facts["project.type"], "software");
        assert_eq!(facts["task.changed_files"], 7);
    }

    #[test]
    fn embedded_tether_indentation_is_canonical() {
        let input = sample_input();
        let req = build_preview_request(&input);

        let source = &req.tether.source;

        // Anchor line with 4-space indent
        assert!(
            source.contains("\n    lantern.project_result_preview_requested\n"),
            "anchor body should have 4-space indent, got: {:?}",
            source
        );

        // Condition lines with 4-space indent
        assert!(
            source.contains("\n    project.type is \"software\"\n"),
            "condition should have 4-space indent"
        );
        assert!(
            source.contains("\n    and task.changed_files greater_than 0\n"),
            "second condition should have 4-space indent with 'and'"
        );

        // Action name with 4-space indent
        assert!(
            source.contains("\n    lantern.task.record\n"),
            "action name should have 4-space indent"
        );

        // Action arguments with 8-space indent
        assert!(
            source.contains("\n        project: anchor.project\n"),
            "action argument should have 8-space indent"
        );
        assert!(
            source.contains("\n        task: anchor.task\n"),
            "second action argument should have 8-space indent"
        );

        // Verify exact structure line by line
        let lines: Vec<&str> = source.lines().collect();
        assert_eq!(lines[0], "tether \"Preview project result\"");
        assert_eq!(lines[1], "");
        assert_eq!(lines[2], "anchor");
        assert_eq!(lines[3], "    lantern.project_result_preview_requested");
        assert_eq!(lines[4], "");
        assert_eq!(lines[5], "when");
        assert_eq!(lines[6], "    project.type is \"software\"");
        assert_eq!(lines[7], "    and task.changed_files greater_than 0");
        assert_eq!(lines[8], "");
        assert_eq!(lines[9], "do");
        assert_eq!(lines[10], "    lantern.task.record");
        assert_eq!(lines[11], "        project: anchor.project");
        assert_eq!(lines[12], "        task: anchor.task");
        assert_eq!(lines.len(), 13);
    }

    #[test]
    fn capability_inputs_and_effects_are_exact() {
        let req = build_preview_request(&sample_input());
        let cap = &req.capabilities[0];

        assert_eq!(cap.name, "lantern.task.record");
        assert_eq!(cap.version, "1.0.0");
        assert_eq!(cap.effects, vec!["lantern.write"]);
        assert_eq!(cap.reversibility, "compensatable");

        let inputs = &cap.inputs;
        assert_eq!(inputs["project"], "string");
        assert_eq!(inputs["task"], "string");
        assert_eq!(inputs.as_object().expect("inputs object").len(), 2);
    }

    // ── Response deserialisation tests ─────────────────────────

    #[test]
    fn deserialize_matched_response() {
        let json = matched_response_json();
        let resp: TethersResponse = serde_json::from_value(json).expect("deserialize matched");

        assert_eq!(resp.protocol_version, "0.1");
        assert_eq!(resp.status, TethersStatus::Matched);
        assert_eq!(resp.evaluation_id.as_deref(), Some("eval_demo_001"));
        assert_eq!(resp.event_id.as_deref(), Some("evt_demo_001"));

        let plan = resp.plan.expect("plan present");
        assert_eq!(plan.id, "eval_demo_001/plan");
        assert_eq!(plan.required_effects, vec!["lantern.write"]);
        assert_eq!(plan.actions.len(), 1);

        let action = &plan.actions[0];
        assert_eq!(action.action_id, "action_1");
        assert_eq!(action.idempotency_key, "eval_demo_001/action_1");
        assert_eq!(action.capability, "lantern.task.record");
        assert_eq!(action.capability_version, "1.0.0");
        assert_eq!(action.arguments["project"], "lantern-keeper");
        assert_eq!(action.arguments["task"], "LK-39");
        assert_eq!(action.effects, vec!["lantern.write"]);

        let trail = resp.trail.expect("trail present");
        assert_eq!(trail.len(), 1);
        assert_eq!(trail[0].sequence, 1);
        assert_eq!(trail[0].phase, "reception");
    }

    #[test]
    fn deserialize_not_matched_response() {
        let json = serde_json::json!({
            "protocol_version": "0.1",
            "evaluation_id": "eval-nomatch-1",
            "event_id": "evt-nomatch-1",
            "tether_id": "preview-project-result",
            "tether_version": "0.1",
            "status": "not_matched",
            "plan": null,
            "trail": [
                {
                    "sequence": 1,
                    "phase": "reception",
                    "kind": "event_received",
                    "outcome": "accepted",
                    "message": "Received lantern.project_result_preview_requested"
                },
                {
                    "sequence": 2,
                    "phase": "evaluation",
                    "kind": "condition_checked",
                    "outcome": "not_matched",
                    "message": "project.type is \"software\""
                }
            ]
        });

        let resp: TethersResponse = serde_json::from_value(json).expect("deserialize not_matched");

        assert_eq!(resp.status, TethersStatus::NotMatched);
        assert_eq!(resp.evaluation_id.as_deref(), Some("eval-nomatch-1"));
        assert!(resp.plan.is_none());
        assert!(resp.error.is_none());

        let trail = resp.trail.expect("trail present");
        assert_eq!(trail.len(), 2);
        assert_eq!(trail[1].outcome, "not_matched");
    }

    #[test]
    fn deserialize_minimal_error_response_no_correlation_fields() {
        let json = serde_json::json!({
            "protocol_version": "0.1",
            "status": "error",
            "error": {
                "code": "parse_error",
                "message": "unexpected token at line 3"
            }
        });

        let resp: TethersResponse = serde_json::from_value(json).expect("deserialize error");

        assert_eq!(resp.protocol_version, "0.1");
        assert_eq!(resp.status, TethersStatus::Error);
        assert!(resp.evaluation_id.is_none());
        assert!(resp.event_id.is_none());
        assert!(resp.tether_id.is_none());
        assert!(resp.tether_version.is_none());
        assert!(resp.plan.is_none());
        assert!(resp.trail.is_none());

        let err = resp.error.expect("error present");
        assert_eq!(err.code, "parse_error");
        assert!(!err.message.is_empty());
    }

    #[test]
    fn deserialize_correlated_error_response() {
        // Represents a SPEC §11.2 correlated error (e.g. missing_fact or
        // action-planning-failed).  Uses the exact shape: all correlation
        // identifiers present, plan: null, error code/message, accumulated
        // Trail entries with condition_failed kind.
        let json = serde_json::json!({
            "protocol_version": "0.1",
            "evaluation_id": "eval-corr-err-1",
            "event_id": "evt-corr-err-1",
            "tether_id": "preview-project-result",
            "tether_version": "0.1",
            "status": "error",
            "plan": null,
            "error": {
                "code": "missing_fact",
                "message": "Fact 'project.type' not found in the supplied Facts"
            },
            "trail": [
                {
                    "sequence": 1,
                    "phase": "reception",
                    "kind": "event_received",
                    "outcome": "accepted",
                    "message": "Received lantern.project_result_preview_requested"
                },
                {
                    "sequence": 2,
                    "phase": "evaluation",
                    "kind": "anchor_checked",
                    "outcome": "matched",
                    "message": "Anchor lantern.project_result_preview_requested matched"
                },
                {
                    "sequence": 3,
                    "phase": "evaluation",
                    "kind": "condition_failed",
                    "outcome": "error",
                    "message": "Fact 'project.type' not found in the supplied Facts"
                }
            ]
        });

        let resp: TethersResponse =
            serde_json::from_value(json).expect("deserialize correlated error");

        // Protocol version always present
        assert_eq!(resp.protocol_version, "0.1");

        // Status is typed Error
        assert_eq!(resp.status, TethersStatus::Error);

        // Every correlation identifier preserved
        assert_eq!(resp.evaluation_id.as_deref(), Some("eval-corr-err-1"));
        assert_eq!(resp.event_id.as_deref(), Some("evt-corr-err-1"));
        assert_eq!(resp.tether_id.as_deref(), Some("preview-project-result"));
        assert_eq!(resp.tether_version.as_deref(), Some("0.1"));

        // Plan is null (None after deserialisation)
        assert!(resp.plan.is_none());

        // Error code and message preserved
        let err = resp.error.expect("error present");
        assert_eq!(err.code, "missing_fact");
        assert!(!err.message.is_empty());

        // Trail present with correct sequence and order
        let trail = resp.trail.expect("trail present");
        assert_eq!(trail.len(), 3);
        assert_eq!(trail[0].sequence, 1);
        assert_eq!(trail[0].kind, "event_received");
        assert_eq!(trail[1].sequence, 2);
        assert_eq!(trail[1].kind, "anchor_checked");
        assert_eq!(trail[2].sequence, 3);
        assert_eq!(trail[2].kind, "condition_failed");
        assert_eq!(trail[2].outcome, "error");
    }

    #[test]
    fn action_and_trail_array_order_preserved() {
        let json = serde_json::json!({
            "protocol_version": "0.1",
            "evaluation_id": "eval-order-1",
            "event_id": "evt-order-1",
            "tether_id": "t1",
            "tether_version": "v1",
            "status": "matched",
            "plan": {
                "id": "eval-order-1/plan",
                "required_effects": ["lantern.write", "network.access"],
                "actions": [
                    {
                        "action_id": "action_1",
                        "idempotency_key": "eval-order-1/action_1",
                        "capability": "first.cap",
                        "capability_version": "1.0.0",
                        "arguments": {},
                        "effects": ["lantern.write"]
                    },
                    {
                        "action_id": "action_2",
                        "idempotency_key": "eval-order-1/action_2",
                        "capability": "second.cap",
                        "capability_version": "2.0.0",
                        "arguments": {},
                        "effects": ["network.access"]
                    },
                    {
                        "action_id": "action_3",
                        "idempotency_key": "eval-order-1/action_3",
                        "capability": "third.cap",
                        "capability_version": "3.0.0",
                        "arguments": {},
                        "effects": ["filesystem.read"]
                    }
                ]
            },
            "trail": [
                {
                    "sequence": 1,
                    "phase": "reception",
                    "kind": "event_received",
                    "outcome": "accepted",
                    "message": "First"
                },
                {
                    "sequence": 2,
                    "phase": "evaluation",
                    "kind": "anchor_checked",
                    "outcome": "matched",
                    "message": "Second"
                },
                {
                    "sequence": 3,
                    "phase": "evaluation",
                    "kind": "action_planned",
                    "outcome": "accepted",
                    "message": "Third"
                }
            ]
        });

        let resp: TethersResponse = serde_json::from_value(json).expect("deserialize order test");

        assert_eq!(resp.status, TethersStatus::Matched);

        let plan = resp.plan.expect("plan");
        assert_eq!(plan.required_effects.len(), 2);
        assert_eq!(plan.required_effects[0], "lantern.write");
        assert_eq!(plan.required_effects[1], "network.access");

        assert_eq!(plan.actions.len(), 3);
        assert_eq!(plan.actions[0].capability, "first.cap");
        assert_eq!(plan.actions[1].capability, "second.cap");
        assert_eq!(plan.actions[2].capability, "third.cap");

        let trail = resp.trail.expect("trail");
        assert_eq!(trail.len(), 3);
        assert_eq!(trail[0].message, "First");
        assert_eq!(trail[1].message, "Second");
        assert_eq!(trail[2].message, "Third");
    }

    // ── Serialization round-trip tests ─────────────────────────

    #[test]
    fn serialize_matched_round_trip() {
        let json = matched_response_json();
        let resp: TethersResponse = serde_json::from_value(json.clone()).expect("deserialize");
        let re_serialized = serde_json::to_value(&resp).expect("serialize");

        // Re-serialized value must equal the original JSON value
        assert_eq!(re_serialized, json);
    }

    #[test]
    fn serialize_not_matched_preserves_plan_null() {
        let json = serde_json::json!({
            "protocol_version": "0.1",
            "evaluation_id": "eval-nomatch-1",
            "event_id": "evt-nomatch-1",
            "tether_id": "preview-project-result",
            "tether_version": "0.1",
            "status": "not_matched",
            "plan": null,
            "trail": [
                {
                    "sequence": 1,
                    "phase": "reception",
                    "kind": "event_received",
                    "outcome": "accepted",
                    "message": "Received lantern.project_result_preview_requested"
                },
                {
                    "sequence": 2,
                    "phase": "evaluation",
                    "kind": "condition_checked",
                    "outcome": "not_matched",
                    "message": "project.type is \"software\""
                }
            ]
        });

        let resp: TethersResponse = serde_json::from_value(json.clone()).expect("deserialize");
        let re_serialized = serde_json::to_value(&resp).expect("serialize");

        // plan key must exist and be JSON null
        let plan_val = re_serialized
            .get("plan")
            .expect("plan key must be present in not_matched response");
        assert!(plan_val.is_null(), "plan must be JSON null");

        // Full round-trip equality
        assert_eq!(re_serialized, json);
    }

    #[test]
    fn serialize_minimal_error_remains_minimal() {
        let json = serde_json::json!({
            "protocol_version": "0.1",
            "status": "error",
            "error": {
                "code": "parse_error",
                "message": "unexpected token at line 3"
            }
        });

        let resp: TethersResponse = serde_json::from_value(json.clone()).expect("deserialize");
        let re_serialized = serde_json::to_value(&resp).expect("serialize");

        // Full round-trip equality (no extra keys)
        assert_eq!(re_serialized, json);

        // Explicitly assert absent keys
        assert!(re_serialized.get("evaluation_id").is_none());
        assert!(re_serialized.get("event_id").is_none());
        assert!(re_serialized.get("tether_id").is_none());
        assert!(re_serialized.get("tether_version").is_none());
        assert!(re_serialized.get("plan").is_none());
        assert!(re_serialized.get("trail").is_none());
    }

    #[test]
    fn serialize_correlated_error_round_trip() {
        let json = serde_json::json!({
            "protocol_version": "0.1",
            "evaluation_id": "eval-corr-err-1",
            "event_id": "evt-corr-err-1",
            "tether_id": "preview-project-result",
            "tether_version": "0.1",
            "status": "error",
            "plan": null,
            "error": {
                "code": "missing_fact",
                "message": "Fact 'project.type' not found in the supplied Facts"
            },
            "trail": [
                {
                    "sequence": 1,
                    "phase": "reception",
                    "kind": "event_received",
                    "outcome": "accepted",
                    "message": "Received lantern.project_result_preview_requested"
                },
                {
                    "sequence": 2,
                    "phase": "evaluation",
                    "kind": "anchor_checked",
                    "outcome": "matched",
                    "message": "Anchor lantern.project_result_preview_requested matched"
                },
                {
                    "sequence": 3,
                    "phase": "evaluation",
                    "kind": "condition_failed",
                    "outcome": "error",
                    "message": "Fact 'project.type' not found in the supplied Facts"
                }
            ]
        });

        let resp: TethersResponse = serde_json::from_value(json.clone()).expect("deserialize");
        let re_serialized = serde_json::to_value(&resp).expect("serialize");

        // plan key must exist and be JSON null
        let plan_val = re_serialized
            .get("plan")
            .expect("plan key must be present in correlated error response");
        assert!(
            plan_val.is_null(),
            "plan must be JSON null in correlated error"
        );

        // Full round-trip equality
        assert_eq!(re_serialized, json);
    }
}
