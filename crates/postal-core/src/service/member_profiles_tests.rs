use super::*;
use crate::store::member_profiles::{MemberMessageStats, MemberSignals};

#[test]
fn missing_and_failed_fields_do_not_claim_privacy() {
    let absent = observed_field::<String>(None, None);
    assert_eq!(absent.state, MemberFieldState::Unavailable);
    assert_eq!(
        MemberProfileField::<String>::failed("Network request failed.").state,
        MemberFieldState::Error
    );
    assert_eq!(
        MemberProfileField::<String>::restricted().state,
        MemberFieldState::Restricted
    );
    assert_eq!(device_count(&[]), None);
    assert_eq!(device_count(&[0, 1, 1, 2]), Some(3));
    for (code, state) in [
        (403, MemberFieldState::Restricted),
        (404, MemberFieldState::Unavailable),
        (429, MemberFieldState::Error),
    ] {
        let error: UsyncSubprotocolError =
            serde_json::from_value(serde_json::json!({"code":code})).unwrap();
        assert_eq!(
            observed_field(Some("discard".to_owned()), Some(&error)).state,
            state
        );
    }
}

#[test]
fn failed_refresh_keeps_stale_value_but_denial_clears_it() {
    let cached = MemberProfileField::available("previous".to_owned());
    let mut failed = MemberProfileField::failed("Network request failed.");
    retain_failed(&mut failed, &cached);
    assert_eq!(
        (failed.state, failed.value.as_deref(), failed.stale),
        (MemberFieldState::Error, Some("previous"), true)
    );
    let mut denied = MemberProfileField::restricted();
    retain_failed(&mut denied, &cached);
    assert_eq!(
        (denied.state, denied.value, denied.stale),
        (MemberFieldState::Restricted, None, false)
    );
    let mut absent = MemberProfileField::unavailable();
    retain_failed(&mut absent, &cached);
    assert_eq!(absent.value, None);
}

#[test]
fn cache_ttl_rejects_expired_and_future_observations() {
    assert!(fresh(100, 100));
    assert!(fresh(100, 129));
    assert!(!fresh(100, 130));
    assert!(!fresh(101, 100));
}

#[test]
fn moderation_requires_fresh_admin_and_eligible_target() {
    assert!(validate_moderation(true, true, false, false, false, "promote").is_ok());
    assert!(validate_moderation(true, true, false, false, true, "demote").is_ok());
    assert!(validate_moderation(true, true, false, false, false, "remove").is_ok());
    for (admin, present, own, owner, target_admin, action) in [
        (false, true, false, false, false, "remove"),
        (true, false, false, false, false, "remove"),
        (true, true, true, false, false, "remove"),
        (true, true, false, true, true, "remove"),
        (true, true, false, false, true, "promote"),
        (true, true, false, false, false, "demote"),
        (true, true, false, false, false, "invented"),
    ] {
        assert!(validate_moderation(admin, present, own, owner, target_admin, action).is_err());
    }
}

#[test]
fn profile_preserves_legacy_wire_fields_and_saved_provenance() {
    let local = MemberProfileLocal {
        jid: "77@lid".into(),
        addresses: vec!["77@lid".into()],
        identity: crate::store::contact_identity::ContactIdentity {
            saved_name: Some("Saved".into()),
            push_name: Some("Push".into()),
            username: Some("member".into()),
            number: Some("598999001".into()),
            ..Default::default()
        },
        pn_jid: None,
        lid_jid: Some("77@lid".into()),
        scope_chat: None,
        stats: MemberMessageStats::default(),
        note: MemberNote::default(),
        group: None,
        join: None,
        mutual_groups: Vec::new(),
        signals: MemberSignals::default(),
    };
    let profile = MemberProfile {
        legacy: legacy_profile(&local, None),
        local,
        live: None,
        live_cached: false,
        live_stale: false,
        moderation_admin_verified: false,
        moderation_verified_at: None,
        moderation_error: None,
        moderation_error_ref: None,
        moderation_diagnostic: None,
    };
    let value = serde_json::to_value(profile).unwrap();
    for key in ["jid", "name", "number", "username", "about", "business"] {
        assert!(value.get(key).is_some());
    }
    assert_eq!(value["name"], "Saved");
    assert_eq!(value["local"]["identity"]["push_name"], "Push");
    assert_eq!(value["moderation_admin_verified"], false);
    assert_eq!(value["moderation_verified_at"], serde_json::Value::Null);
}

#[test]
fn transient_member_failures_preserve_exact_cache_payload_shape() {
    let raw = serde_json::json!({
        "fetched_at": 1, "photo_id": null,
        "about": {"state":"error","value":"cached","error":"legacy opaque reason","stale":true},
        "username": {"state":"unavailable","value":null,"error":null,"stale":false},
        "photo": {"state":"restricted","value":null,"error":"legacy denial","stale":false},
        "business": {"state":"unavailable","value":null,"error":null,"stale":false},
        "business_name": {"state":"unavailable","value":null,"error":null,"stale":false},
        "device_count": {"state":"available","value":2,"error":null,"stale":false}
    });
    let snapshot: MemberProfileLive = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(serde_json::to_value(&snapshot).unwrap(), raw);
    let mut fresh = BTreeMap::new();
    fresh.insert("about".into(), MessageFailure {
        message: MessageRef::new("error.member_field_query"),
        diagnostic: Some("synthetic SDK detail".into()),
    });
    let view = MemberProfileLiveView::new(snapshot.clone(), fresh);
    let value = serde_json::to_value(view).unwrap();
    assert_eq!(value["field_failures"]["about"]["diagnostic"], "synthetic SDK detail");
    assert_eq!(value["field_failures"]["photo"]["code"], "error.member_field_restricted");
    assert_eq!(value["field_failures"]["photo"]["diagnostic"], "legacy denial");
    assert_eq!(value["about"], raw["about"]);
    assert_eq!(serde_json::to_value(snapshot).unwrap(), raw);
    assert!(value["field_failures"].get("device_count").is_none());
}

#[test]
fn observed_subprotocol_failure_keeps_sdk_detail_without_classifying_copy() {
    let error: UsyncSubprotocolError = serde_json::from_value(serde_json::json!({
        "code":429,"text":"synthetic localized SDK detail","backoff":30
    })).unwrap();
    let failure = observed_failure(Some(&error)).unwrap();
    assert_eq!(failure.message.code, "error.member_field_query");
    let detail: serde_json::Value = serde_json::from_str(failure.diagnostic.as_deref().unwrap()).unwrap();
    assert_eq!(detail["code"], 429);
    assert_eq!(detail["backoff"], 30);
    let missing: UsyncSubprotocolError = serde_json::from_value(serde_json::json!({"code":404})).unwrap();
    assert!(observed_failure(Some(&missing)).is_none());
}
