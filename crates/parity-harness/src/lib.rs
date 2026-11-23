use chrono::{DateTime, TimeZone, Utc};
use lighting_core::epistemic::{
    Belief, Claim, NewBelief, NewClaim, PredicateStatus, Scope, Stance, TrustClass, reconcile_claim,
};

fn fixed_time() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
}

fn fixture_claim() -> Claim {
    let now = fixed_time();
    Claim::new(NewClaim {
        episode_id: None,
        source_id: None,
        evidence_span: None,
        subject_key: "matthew".to_owned(),
        predicate_key: Some("preferred_editor".to_owned()),
        predicate_candidate: None,
        predicate_status: PredicateStatus::Resolved,
        value: "Helix".to_owned(),
        scope: Scope::new(),
        polarity: true,
        originator_actor_id: "lucy".to_owned(),
        speaker_actor_id: "lucy".to_owned(),
        transmitter_actor_id: None,
        holder_actor_id: Some("lucy".to_owned()),
        stance: Stance::Endorsing,
        framing_path: Vec::new(),
        confidence: 1.0,
        known_at: now,
        valid_from: None,
        valid_to: None,
        extractor: "test".to_owned(),
        extractor_version: "1".to_owned(),
        created_at: now,
    })
    .expect("fixture claim is valid")
}

fn fixture_belief() -> Belief {
    let now = fixed_time();
    Belief::new(NewBelief {
        holder_key: "matthew".to_owned(),
        subject_key: "matthew".to_owned(),
        predicate_key: "preferred_editor".to_owned(),
        current_value: "Zed".to_owned(),
        scope: Scope::new(),
        confidence: 0.9,
        trust_class: TrustClass::Direct,
        known_from: now,
        valid_from: None,
        created_at: now,
    })
    .expect("fixture belief is valid")
}

pub fn run_fixture_json() -> String {
    let claim = fixture_claim();
    let belief = fixture_belief();
    let decision = reconcile_claim(&claim, Some(&belief), true);
    serde_json::to_string(&decision).expect("decision serializes")
}

static OUT_BUF: std::sync::Mutex<[u8; 4096]> = std::sync::Mutex::new([0; 4096]);

#[unsafe(no_mangle)]
pub extern "C" fn parity_buf() -> *mut u8 {
    OUT_BUF.lock().expect("buf").as_mut_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn parity_run() -> usize {
    let rendered = run_fixture_json();
    let bytes = rendered.as_bytes();
    assert!(bytes.len() <= 4096);
    unsafe {
        let dst = OUT_BUF.lock().expect("buf").as_mut_ptr();
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
    }
    bytes.len()
}

#[cfg(test)]
mod tests {
    use super::run_fixture_json;

    #[test]
    fn native_fixture_is_contradict() {
        let got = run_fixture_json();
        assert_eq!(
            got,
            r#"{"action":"contradict","reason":"different value lacks direct holder authority"}"#
        );
    }
}
