use lighting_core::BridgeIntentRecord;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct ClaimBridgeIntentRequest {
    pub intent_id: String,
    pub intent_commit: String,
    pub canonical_digest: String,
    pub action: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ClaimBridgeIntentResponse {
    Claimed {
        record: BridgeIntentRecord,
    },
    Existing {
        record: BridgeIntentRecord,
    },
    Conflict {
        existing_digest: String,
        incoming_digest: String,
    },
}

#[derive(Debug, Deserialize)]
pub struct CompleteBridgeIntentRequest {
    pub intent_id: String,
    pub status: String,
    #[serde(default)]
    pub lantern_record_id: Option<String>,
    #[serde(default)]
    pub result_digest: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BridgeIntentResponse {
    pub intent: BridgeIntentRecord,
}

#[derive(Debug, Serialize)]
pub struct ListBridgeIntentsResponse {
    pub intents: Vec<BridgeIntentRecord>,
}
