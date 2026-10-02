use super::*;
use whatsapp_rust::wacore::stanza::groups::GroupParticipantInfo;

fn update(action: Action) -> GroupUpdate {
    GroupUpdate::builder().group_jid("1@g.us".parse().unwrap()).notification_id("notice".into())
        .timestamp((std::time::UNIX_EPOCH + Duration::from_secs(200)).into())
        .is_lid_addressing_mode(true).action(Box::new(action)).build()
}

fn participant(jid: &str) -> GroupParticipantInfo {
    GroupParticipantInfo { jid: jid.parse().unwrap(), phone_number: None, display_name: None,
        r#type: None, lid: None, username: None, join_time: None, group_history_sent_state: None }
}

#[test]
fn group_metadata_audit_distinguishes_cached_old_values_and_missing_actors() {
    let subject_update = update(Action::Subject { subject: "new".into(), subject_owner: None,
        subject_owner_pn: None, subject_owner_username: None, subject_time: None });
    let uncached = group_records(&subject_update, None);
    assert_eq!((uncached[0].old_value.clone(), uncached[0].old_source, uncached[0].actor.clone()), (None, None, None));
    assert_eq!(uncached[0].new_value.as_deref(), Some("new"));
    assert_eq!(uncached[0].timestamp, Some(200));
    assert_eq!(uncached[0].message_id.as_deref(), Some("group-notice-0"));
    let cache = GroupInfo { subject: Some("previous local observation".into()), ..Default::default() };
    let cached = group_records(&subject_update, Some(&cache));
    assert_eq!(cached[0].old_value.as_deref(), Some("previous local observation"));
    assert_eq!(cached[0].old_source, Some(OldSource::Cached));
    let deletion = update(Action::Description { id: "revision".into(), description: None });
    assert_eq!(group_records(&deletion, None)[0].new_value, None);
    let invite = update(Action::Invite { code: "NEVER-STORE-INVITE-CREDENTIAL".into() });
    let entry = &group_records(&invite, None)[0];
    assert_eq!(entry.kind, Kind::InviteChange);
    assert!(entry.new_value.is_none() && !entry.source_id.as_deref().unwrap().contains("CREDENTIAL"));
}

#[test]
fn group_roster_audit_has_separate_targets_roles_and_nullable_join_facts() {
    let mut first = participant("123@lid");
    first.phone_number = Some("5989@s.whatsapp.net".parse().unwrap());
    first.join_time = Some(150);
    let addition = update(Action::Add { participants: vec![first.clone(), participant("456@lid")], reason: None });
    let records = group_records(&addition, None);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].target.as_deref(), Some("5989@s.whatsapp.net"));
    assert_eq!(records[1].target.as_deref(), Some("456@lid"));
    assert!(records.iter().all(|r| r.kind == Kind::Join && r.actor.is_none() && r.old_value.is_none() && r.timestamp == Some(200)));
    let cache = GroupInfo { participants: vec![Participant { jid: "123@lid".into(), name: "Member".into(),
        admin: false, owner: false, number: None, username: None, label: None }], ..Default::default() };
    let promoted = group_records(&update(Action::Promote { participants: vec![first] }), Some(&cache));
    assert_eq!(promoted[0].new_value.as_deref(), Some("admin"));
    assert_eq!((promoted[0].old_value.as_deref(), promoted[0].old_source), (Some("member"), Some(OldSource::Cached)));
    let removal = GroupUpdate::builder().group_jid("1@g.us".parse().unwrap()).participant("123@lid".parse().unwrap())
        .timestamp((std::time::UNIX_EPOCH + Duration::from_secs(200)).into()).is_lid_addressing_mode(true)
        .action(Box::new(Action::Remove { participants: vec![participant("123@lid")], reason: None })).build();
    assert_eq!(group_records(&removal, None)[0].kind, Kind::Leave);
}

fn control(kind: wa::message::protocol_message::Type, remote: &str) -> wa::Message {
    wa::Message { protocol_message: MessageField::some(wa::message::ProtocolMessage {
        r#type: Some(kind), key: MessageField::some(wa::MessageKey { id: Some("target".into()),
            remote_jid: Some(remote.into()), participant: Some("8@lid".into()), ..Default::default() }),
        ..Default::default()
    }), ..Default::default() }
}

#[test]
fn message_audit_records_revoke_and_edits_without_copying_body_or_cross_chat_targets() {
    let entry = base("1@g.us".into(), Some("action".into()), Some(200), Source::Message);
    let mut original = StoredMessage::default();
    original.header.chat = "1@g.us".into();
    original.header.id = "target".into();
    original.header.sender = "8@lid".into();
    original.text = "deleted content must stay out of audit".into();
    let revoke = control(wa::message::protocol_message::Type::REVOKE, "1@g.us");
    let records = message_records(entry.clone(), &revoke, Some(&original));
    assert_eq!(records[0].kind, Kind::MessageDelete);
    assert_eq!(records[0].target.as_deref(), Some("8@lid"));
    assert_eq!(records[0].old_value.as_deref(), Some("not_revoked"));
    assert_eq!(records[0].new_value.as_deref(), Some("revoked"));
    assert!(!format!("{:?}", records).contains("deleted content"));
    assert!(message_records(entry.clone(), &control(wa::message::protocol_message::Type::REVOKE, "2@g.us"), None).is_empty());
    assert_eq!(audit_message_target(&revoke, "1@g.us").as_deref(), Some("target"));
    assert!(audit_message_target(&revoke, "2@g.us").is_none());
    let mut live_location = original.clone();
    live_location.media.kind = Some("live_location".into());
    assert!(message_records(entry.clone(), &revoke, Some(&live_location)).is_empty());
    let mut edit = control(wa::message::protocol_message::Type::MESSAGE_EDIT, "1@g.us");
    edit.protocol_message.as_option_mut().unwrap().edited_message = MessageField::some(wa::Message { conversation: Some("replacement body".into()), ..Default::default() });
    let edited = message_records(entry.clone(), &edit, Some(&original));
    assert_eq!(edited[0].kind, Kind::MessageEdit);
    assert!(edited[0].old_value.is_none() && edited[0].new_value.is_none());
    for field in 0..4 {
        let mut private_row = original.clone();
        match field { 0 => private_row.spoiler = true, 1 => private_row.media.kind = Some("view_once".into()),
            2 => private_row.media.once_kind = Some("image".into()), _ => private_row.system.kind = Some("UNAVAILABLE_MESSAGE".into()) }
        assert!(message_records(entry.clone(), &revoke, Some(&private_row)).is_empty());
    }
}

#[test]
fn message_pin_audit_uses_seconds_and_never_guesses_prior_pin_state() {
    let entry = base("1@g.us".into(), Some("action".into()), Some(200), Source::Message);
    let pin = |kind| wa::Message { pin_in_chat_message: MessageField::some(wa::message::PinInChatMessage {
        r#type: Some(kind), key: MessageField::some(wa::MessageKey { id: Some("target".into()), ..Default::default() }),
        sender_timestamp_ms: Some(250_000), ..Default::default()
    }), ..Default::default() };
    let pinned = message_records(entry.clone(), &pin(wa::message::pin_in_chat_message::Type::PIN_FOR_ALL), None);
    assert_eq!((pinned[0].kind, pinned[0].timestamp), (Kind::MessagePin, Some(250)));
    assert!(pinned[0].old_value.is_none() && pinned[0].old_source.is_none());
    let unpinned = message_records(entry, &pin(wa::message::pin_in_chat_message::Type::UNPIN_FOR_ALL), None);
    assert_eq!(unpinned[0].kind, Kind::MessageUnpin);
    assert_eq!(unpinned[0].new_value.as_deref(), Some("false"));
}

#[test]
fn picture_and_historical_notices_keep_missing_actor_timestamp_and_values_unknown() {
    let picture = PictureUpdate::builder().jid("1@g.us".parse().unwrap())
        .timestamp((std::time::UNIX_EPOCH + Duration::from_secs(200)).into()).removed(true).build();
    let records = picture_records(&picture);
    assert!(records[0].actor.is_none() && records[0].old_value.is_none());
    assert!(records[0].timestamp.is_none());
    assert_eq!(records[0].new_value.as_deref(), Some("removed"));
    let row = system_row("1@g.us", "history".into(), 0, "GROUP_PARTICIPANT_ADD".into(), vec!["not-an-address".into()]);
    let record = &notice_records(&row, Source::History)[0];
    assert!(record.actor.is_none() && record.target.is_none() && record.timestamp.is_none());
    assert_eq!(record.source, Source::History);
    let mode = system_row("1@g.us", "mode".into(), 200, "GROUP_MEMBER_LINK_MODE".into(), vec!["admin_link".into()]);
    assert_eq!(notice_records(&mode, Source::Notification)[0].new_value.as_deref(), Some("admin_link"));
    assert_eq!(notice_records(&mode, Source::Notification)[0].source, Source::Notification);
    let owner = system_row("1@g.us", "owner".into(), 200, "COMMUNITY_OWNER_CHANGED".into(), vec!["123@lid".into(), "456@lid".into()]);
    let comparison = &notice_records(&owner, Source::Notification)[0];
    assert_eq!(comparison.kind, Kind::OwnerChange);
    assert!(comparison.actor.is_none() && comparison.timestamp.is_none());
    assert_eq!(comparison.old_source, Some(OldSource::Cached));
    assert_eq!(comparison.source, Source::Local);
    assert_eq!(comparison.target.as_deref(), Some("456@lid"));
}

#[tokio::test]
async fn typed_live_and_historical_notice_replay_deduplicate_without_guessing_missing_pin_time() {
    let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
    let addition = update(Action::Add { participants: vec![participant("123@lid")], reason: None });
    assert!(audit_group_update(&store, &addition, None).await.unwrap());
    assert!(!audit_group_update(&store, &addition, None).await.unwrap());
    let row = system_row("1@g.us", "group-notice-0".into(), 200, "GROUP_PARTICIPANT_ADD".into(), vec!["123@lid".into()]);
    assert!(!audit_group_notice(&store, &row, Source::History).await.unwrap());
    let history = wa::WebMessageInfo { key: MessageField::some(wa::MessageKey { id: Some("missing-time-pin".into()),
        from_me: Some(true), ..Default::default() }), pin_in_chat: MessageField::some(wa::PinInChat {
            r#type: Some(wa::pin_in_chat::Type::PIN_FOR_ALL), key: MessageField::some(wa::MessageKey { id: Some("target".into()), ..Default::default() }),
            ..Default::default()
        }), ..Default::default() };
    assert!(audit_group_history_message(&store, "1@g.us", &history, None).await.unwrap());
    let page = store.run(|store| store.group_audit_page(None, &GroupAuditFilter::default())).await.unwrap();
    let pin = page.entries.iter().find(|e| e.kind == Kind::MessagePin).unwrap();
    assert!(pin.timestamp.is_none() && pin.actor.is_none());
    assert!(pin.observed_at < 100_000_000_000);
}

#[tokio::test]
async fn local_sent_observations_have_no_invented_server_time_or_message_body() {
    let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
    let actor: Jid = "9@lid".parse().unwrap();
    let mut local = local_group_record("1@g.us", Kind::Subject, Some(&actor));
    local.new_value = Some("new".into());
    assert!(audit_group_local(&store, local.clone()).await.unwrap());
    assert!(audit_group_local(&store, local.clone()).await.unwrap());
    local.kind = Kind::MessageEdit;
    local.source_id = Some("actual-ack-id".into());
    local.old_value = Some("old private body".into());
    local.new_value = Some("new private body".into());
    assert!(audit_group_local(&store, local.clone()).await.unwrap());
    assert!(!audit_group_local(&store, local).await.unwrap());
    let page = store.run(|store| store.group_audit_page(None, &GroupAuditFilter::default())).await.unwrap();
    assert_eq!(page.entries.len(), 3);
    assert!(page.entries.iter().all(|entry| entry.timestamp.is_none() && entry.source == Source::Local));
    let edit = page.entries.iter().find(|entry| entry.kind == Kind::MessageEdit).unwrap();
    assert!(edit.old_value.is_none() && edit.new_value.is_none());
    let mut provided = local_group_record("1@g.us", Kind::Create, Some(&actor));
    provided.timestamp = Some(150);
    provided.target = Some("1@g.us".into());
    assert!(audit_group_local(&store, provided).await.unwrap());
    let page = store.run(|store| store.group_audit_page(None, &GroupAuditFilter { kind: Some(Kind::Create), ..Default::default() })).await.unwrap();
    assert_eq!(page.entries[0].timestamp, Some(150));
    assert!(page.entries[0].target.is_none());
}

#[tokio::test]
async fn history_pin_metadata_does_not_use_another_protocol_targets_cached_row() {
    let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
    let mut previous = StoredMessage::default();
    previous.header.chat = "1@g.us".into();
    previous.header.id = "protocol-target".into();
    previous.header.sender = "8@lid".into();
    previous.spoiler = true;
    let pin = wa::PinInChat { r#type: Some(wa::pin_in_chat::Type::PIN_FOR_ALL),
        key: MessageField::some(wa::MessageKey { id: Some("pin-target".into()), ..Default::default() }), ..Default::default() };
    let web = wa::WebMessageInfo { key: MessageField::some(wa::MessageKey { id: Some("combined".into()), ..Default::default() }),
        participant: Some("9@lid".into()), message: MessageField::some(wa::Message { protocol_message: MessageField::some(wa::message::ProtocolMessage {
            r#type: Some(wa::message::protocol_message::Type::REVOKE), key: MessageField::some(wa::MessageKey { id: Some("protocol-target".into()), ..Default::default() }),
            ..Default::default() }), ..Default::default() }), pin_in_chat: MessageField::some(pin), ..Default::default() };
    assert!(audit_group_history_message(&store, "1@g.us", &web, Some(&previous)).await.unwrap());
    let page = store.run(|store| store.group_audit_page(None, &GroupAuditFilter::default())).await.unwrap();
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].kind, Kind::MessagePin);
    assert_eq!(page.entries[0].message_id.as_deref(), Some("pin-target"));
    assert!(page.entries[0].target.is_none() && page.entries[0].actor.is_none());
}

#[tokio::test]
async fn history_pin_metadata_preserves_millisecond_identity_and_unknown_state_times() {
    let store = StoreWorker::open(Path::new(":memory:")).await.unwrap();
    let mut web = wa::WebMessageInfo { key: MessageField::some(wa::MessageKey { id: Some("original".into()), ..Default::default() }),
        participant: Some("original-author@lid".into()), message_timestamp: Some(100),
        pin_in_chat: MessageField::some(wa::PinInChat {
            r#type: Some(wa::pin_in_chat::Type::PIN_FOR_ALL), key: MessageField::some(wa::MessageKey { id: Some("target".into()), ..Default::default() }),
            server_timestamp_ms: Some(250_001), sender_timestamp_ms: Some(249_999), ..Default::default()
        }), ..Default::default() };
    assert!(audit_group_history_message(&store, "1@g.us", &web, None).await.unwrap());
    assert!(!audit_group_history_message(&store, "1@g.us", &web, None).await.unwrap());
    web.pin_in_chat.as_option_mut().unwrap().server_timestamp_ms = Some(250_002);
    assert!(audit_group_history_message(&store, "1@g.us", &web, None).await.unwrap());
    web.pin_in_chat.as_option_mut().unwrap().server_timestamp_ms = None;
    web.pin_in_chat.as_option_mut().unwrap().sender_timestamp_ms = None;
    assert!(audit_group_history_message(&store, "1@g.us", &web, None).await.unwrap());
    assert!(!audit_group_history_message(&store, "1@g.us", &web, None).await.unwrap());
    let page = store.run(|store| store.group_audit_page(None, &GroupAuditFilter::default())).await.unwrap();
    assert_eq!(page.entries.len(), 3);
    assert_eq!(page.entries.iter().filter(|row| row.timestamp == Some(250)).count(), 2);
    assert_eq!(page.entries.iter().filter(|row| row.timestamp.is_none()).count(), 1);
    assert!(page.entries.iter().all(|row| row.actor.is_none() && row.old_value.is_none()));
}
