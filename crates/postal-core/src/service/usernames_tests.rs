use super::*;

#[test]
fn lookup_outcomes_preserve_not_found_and_required_key_without_an_address() {
    assert_eq!(
        serde_json::to_value(map_lookup(UsernameLookup::NotFound).unwrap()).unwrap(),
        serde_json::json!({ "kind": "notFound" })
    );
    let required = map_lookup(UsernameLookup::KeyRequired {
        username: Some("server.handle".into()),
    })
    .unwrap();
    assert_eq!(
        serde_json::to_value(required).unwrap(),
        serde_json::json!({ "kind": "keyRequired", "username": "server.handle" })
    );
    assert_eq!(
        serde_json::to_value(map_lookup(UsernameLookup::KeyRequired { username: None }).unwrap())
            .unwrap(),
        serde_json::json!({ "kind": "keyRequired", "username": null })
    );
}

#[test]
fn found_identity_uses_real_bare_jid_and_source_username_only() {
    let result = found(&"777:2@lid".parse().unwrap(), Some("Server.Handle")).unwrap();
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        serde_json::json!({ "kind": "found", "jid": "777@lid", "username": "Server.Handle" })
    );
    let result = found(&Jid::pn("15550000001"), None).unwrap();
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        serde_json::json!({ "kind": "found", "jid": "15550000001@s.whatsapp.net", "username": null })
    );
}

#[test]
fn found_lookup_rejects_non_individual_and_empty_addresses() {
    for address in [
        "123@g.us",
        "status@broadcast",
        "123@broadcast",
        "123@newsletter",
    ] {
        assert!(found(&address.parse().unwrap(), Some("real.handle")).is_err());
    }
    assert!(found(&Jid::lid(""), Some("real.handle")).is_err());
    assert!(found(&Jid::pn(""), Some("real.handle")).is_err());
}

#[test]
fn found_lookup_opens_existing_canonical_chat_and_preserves_saved_name() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    store.set_lid_pn("777", "15550000001").unwrap();
    store
        .set_saved_name("15550000001@s.whatsapp.net", "Saved name")
        .unwrap();
    let mapped = found(&Jid::lid("777"), Some("Server.Handle")).unwrap();
    let (result, changed) = persist_lookup(&store, mapped).unwrap();
    assert!(changed);
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        serde_json::json!({ "kind": "found", "jid": "15550000001@s.whatsapp.net", "username": "Server.Handle" })
    );
    assert_eq!(
        store
            .name_for("15550000001@s.whatsapp.net")
            .unwrap()
            .as_deref(),
        Some("Saved name")
    );
    assert!(store.name_is_saved("15550000001@s.whatsapp.net").unwrap());
    let identity = store.contact_identity("777@lid").unwrap();
    assert_eq!(identity.legacy_name.as_deref(), Some("Saved name"));
    assert_eq!(identity.username.as_deref(), Some("Server.Handle"));
}

#[test]
fn unresolved_outcomes_do_not_create_identity_records() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    let (result, changed) = persist_lookup(
        &store,
        map_lookup(UsernameLookup::KeyRequired {
            username: Some("server.handle".into()),
        })
        .unwrap(),
    )
    .unwrap();
    assert!(!changed);
    assert!(matches!(result, UsernameLookupResult::KeyRequired { .. }));
    assert!(store.search_usernames("server", 50).unwrap().is_empty());
}

fn offer() -> IncomingCall {
    let mut call = IncomingCall::new_for_test(
        "777:2@lid".parse().unwrap(),
        "offer-stanza".into(),
        "2026-10-02T00:00:00Z".parse().unwrap(),
        CallAction::Offer {
            call_id: "call".into(),
            call_creator: Jid::lid("999"),
            caller_pn: Some(Jid::pn("15550000002")),
            caller_country_code: None,
            device_class: None,
            joinable: false,
            is_video: false,
            audio: Vec::new(),
            group_jid: None,
        },
    );
    call.caller_username = Some("Source.Handle".into());
    call
}

#[tokio::test]
async fn call_username_uses_outer_sender_without_importing_raw_creator_or_phone_pair() {
    let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
    store
        .set_saved_name("777@lid", "Saved caller")
        .await
        .unwrap();
    let call = offer();
    assert!(record_incoming_call(&store, None, &call).await.unwrap());
    assert!(!record_incoming_call(&store, None, &call).await.unwrap());
    assert!(store.lid_pn("777").await.unwrap().is_none());
    assert!(store.lid_pn("999").await.unwrap().is_none());
    assert_eq!(
        store.name_for("777@lid").await.unwrap().as_deref(),
        Some("Saved caller")
    );
    let identity = store
        .run(|store| store.contact_identity("777@lid"))
        .await
        .unwrap();
    assert_eq!(identity.username.as_deref(), Some("Source.Handle"));
    assert!(identity.number.is_none());
    assert!(store
        .run(|store| store.contact_identity("999@lid"))
        .await
        .unwrap()
        .username
        .is_none());
}

#[tokio::test]
async fn call_identity_ignores_non_offer_actions_and_rejects_non_individual_sender() {
    let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
    let mut call = offer();
    call.from = "123@g.us".parse().unwrap();
    assert!(record_incoming_call(&store, None, &call).await.is_err());
    call.from = Jid::lid("777");
    call.action = CallAction::OfferNotice {
        call_id: "call".into(),
        call_creator: Jid::lid("999"),
        is_video: false,
        is_group: false,
    };
    assert!(!record_incoming_call(&store, None, &call).await.unwrap());
    assert!(store
        .search_usernames("Source", 50)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn incoming_call_dispatch_updates_username_once_without_importing_raw_phone_or_creator() {
    let (handler, mut notices) = super::super::protocol_tests::inbound().await;
    handler
        .store
        .set_saved_name("777@lid", "Saved caller")
        .await
        .unwrap();
    handler
        .handle(&Event::IncomingCall(Box::new(offer())))
        .await;
    assert!(matches!(
        notices.try_recv(),
        Ok(ServiceEvent::NamesUpdated { count: 1 })
    ));
    let identity = handler
        .store
        .run(|store| store.contact_identity("777@lid"))
        .await
        .unwrap();
    assert_eq!(identity.username.as_deref(), Some("Source.Handle"));
    assert!(identity.number.is_none());
    assert_eq!(
        handler.store.name_for("777@lid").await.unwrap().as_deref(),
        Some("Saved caller")
    );
    assert!(handler.store.lid_pn("777").await.unwrap().is_none());
    assert!(handler.store.lid_pn("999").await.unwrap().is_none());
    assert!(handler
        .store
        .run(|store| store.contact_identity("999@lid"))
        .await
        .unwrap()
        .username
        .is_none());
    assert_eq!(handler.store.count().await.unwrap(), 0);
    handler
        .handle(&Event::IncomingCall(Box::new(offer())))
        .await;
    assert!(matches!(
        notices.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
}
