use lighting_core::LedgerEvent;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct LedgerEventResponse {
    pub event: LedgerEvent,
    pub duplicate: bool,
}
