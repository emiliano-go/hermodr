use super::*;
use buffa::Message as _;
use std::io::Write;
use whatsapp_rust::wacore::types::events::LazyHistorySync;
use whatsapp_rust::wacore::types::{events::{InboundMessage, MessageBatch, BatchOrigin, Receipt}, message::{MessageInfo, MessageSource}};

async fn inbound() -> (Inbound, broadcast::Receiver<ServiceEvent>) {
    let (events, received) = broadcast::channel(32);
    (Inbound {
        store: StoreWorker::open(Path::new(":memory:")).await.unwrap(),
        disk_retention: Arc::new(DiskRetentionManager::new(DiskRetention::unlimited())),
        events, connected: Arc::default(), client_for_events: Arc::default(), media_dir: None,
        group_cache: Arc::default(), groups_cache: Arc::default(), older_waits: Arc::default(),
        downloads: Arc::new(tokio::sync::Semaphore::new(1)), sync_progress: Arc::default(),
        auto_download_default: false, keep_archived: Arc::default(), keep_view_once: Arc::default(),
        one_time_only: false, tally: Arc::default(), secret_edits: Default::default(),
    }, received)
}

fn message_event(chat: &str, sender: &str, id: &str, message: wa::Message) -> Event {
    let info = MessageInfo { id: id.into(), source: MessageSource {
        chat: chat.parse().unwrap(), sender: sender.parse().unwrap(), is_group: chat.ends_with("@g.us"), ..Default::default()
    }, ..Default::default() };
    let message = InboundMessage::builder().message(Arc::new(message)).info(Arc::new(info)).build();
    Event::Messages(MessageBatch::builder().messages(vec![message].into()).origin(BatchOrigin::Live).build())
}

fn view_once_image() -> wa::Message {
    wa::Message {
        view_once_message_v2: MessageField::some(wa::message::FutureProofMessage {
            message: MessageField::some(wa::Message {
                image_message: MessageField::some(wa::message::ImageMessage::default()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[tokio::test]
async fn a_one_time_companion_stores_only_view_once_messages() {
    let (mut inbound, _) = inbound().await;
    inbound.one_time_only = true;
    inbound.handle(&message_event("1@g.us", "100@s.whatsapp.net", "plain", wa::Message {
        conversation: Some("the main link's to store".into()), ..Default::default()
    })).await;
    inbound.handle(&message_event("1@g.us", "100@s.whatsapp.net", "picture", wa::Message {
        image_message: MessageField::some(wa::message::ImageMessage::default()), ..Default::default()
    })).await;
    assert_eq!(inbound.store.count().await.unwrap(), 0, "ordinary media is the main link's");
    inbound.handle(&message_event("1@g.us", "100@s.whatsapp.net", "once", view_once_image())).await;
    let stored = inbound.store.message("1@g.us", "once").await.unwrap();
    assert_eq!(stored.media.kind.as_deref(), Some("view_once"));
    inbound.handle(&Event::HistorySync(Box::new(history_chunk("1@g.us", "old", None)))).await;
    assert_eq!(inbound.store.count().await.unwrap(), 1, "history stays out of a companion");
}

#[tokio::test]
async fn retention_keeps_unowned_and_inflight_files_in_shared_media_directory() {
    let (mut inbound, _) = inbound().await;
    let root = std::env::temp_dir().join(format!("postal-retention-files-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&root).unwrap();
    for name in ["other-account.jpg", "active-download.part", "notes.txt", "favorite.webp"] {
        std::fs::write(root.join(name), b"preserve").unwrap();
    }
    inbound.media_dir = Some(root.clone());
    inbound.disk_retention = Arc::new(DiskRetentionManager::new(DiskRetention {
        max_age_hours: crate::store::RetentionLimit::Unlimited,
        max_messages_per_chat: crate::store::RetentionLimit::Limited(0),
    }));
    let path = root.join("favorite.webp").to_string_lossy().into_owned();
    inbound.store.run(move |store| store.upsert_sticker(&crate::store::Sticker {
        filehash: "synthetic-favorite".into(), path: Some(path), favorite: true, ..Default::default()
    })).await.unwrap();
    inbound.handle(&message_event("200@s.whatsapp.net", "100@s.whatsapp.net", "expired", wa::Message {
        conversation: Some("synthetic message".into()), ..Default::default()
    })).await;
    assert_eq!(inbound.store.count().await.unwrap(), 0);
    for name in ["other-account.jpg", "active-download.part", "notes.txt", "favorite.webp"] {
        assert_eq!(std::fs::read(root.join(name)).unwrap(), b"preserve", "retention removed {name}");
    }
    drop(inbound);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn decrypted_community_and_plaintext_reactions_share_parent_and_removal_semantics() {
    use whatsapp_rust::wacore::reaction::{encrypt_reaction_with_secret, decrypt_reaction_with_secret};
    let (inbound, _) = inbound().await;
    let author = "100@s.whatsapp.net";
    let reactor = "300@lid";
    for chat in ["1@g.us", "200@s.whatsapp.net"] {
        inbound.store.insert_message(&StoredMessage { header: MessageHeader {
            chat: chat.into(), id: "parent".into(), sender: author.into(), ..Default::default()
        }, text: "parent stays".into(), ..Default::default() }).await.unwrap();
        for emoji in ["x", ""] {
            let key = wa::MessageKey { remote_jid: Some(chat.into()), id: Some("parent".into()), participant: Some(author.into()), ..Default::default() };
            let message = if chat.ends_with("@g.us") {
                let secret = [7; 32];
                let (payload, iv) = encrypt_reaction_with_secret(emoji, 100, &secret, "parent", author, reactor).unwrap();
                assert!(decrypt_reaction_with_secret(&payload, &iv, &[8; 32], "parent", author, reactor).is_err());
                let mut reaction = decrypt_reaction_with_secret(&payload, &iv, &secret, "parent", author, reactor).unwrap();
                reaction.key = MessageField::some(key);
                wa::Message { reaction_message: MessageField::some(reaction), ..Default::default() }
            } else {
                whatsapp_rust::wacore::proto_helpers::build_reaction_message(key, emoji, 100)
            };
            inbound.handle(&message_event(chat, reactor, "reaction", message)).await;
            let reactions = inbound.store.marks(chat).await.unwrap().reactions;
            if emoji.is_empty() { assert!(reactions.is_empty()); }
            else {
                assert_eq!(reactions.len(), 1);
                assert_eq!(reactions[0].target, "parent");
                assert_eq!(reactions[0].sender, reactor);
                assert_eq!(reactions[0].emoji, emoji);
            }
        }
    }
    assert_eq!(inbound.store.count().await.unwrap(), 2);
}

#[tokio::test]
async fn a_live_message_arrives_with_its_row() {
    let (inbound, mut received) = inbound().await;
    inbound.handle(&message_event("1@g.us", "100@s.whatsapp.net", "live", wa::Message {
        conversation: Some("hello".into()), ..Default::default()
    })).await;
    let arrived = std::iter::from_fn(|| received.try_recv().ok())
        .find_map(|event| match event {
            ServiceEvent::Message { message } => Some(message),
            _ => None,
        })
        .expect("a live arrival goes out with its row");
    assert_eq!(arrived.header.id, "live");
    assert!(arrived.local.sort_order > 0);
    assert_eq!(arrived.text, "hello");
}

#[tokio::test]
async fn live_replay_payload_preserves_stored_spoiler_edit_and_local_state() {
    let (inbound, mut received) = inbound().await;
    let chat = "1@g.us";
    let stored = StoredMessage {
        header: MessageHeader { chat: chat.into(), id: "repeat".into(), sender: "100@s.whatsapp.net".into(), ..Default::default() },
        text: "original".into(),
        local: LocalState { read: true, status: Some("read".into()), ..Default::default() },
        ..Default::default()
    };
    inbound.store.insert_message(&stored).await.unwrap();
    inbound.store.update_message_spoiler(chat, "repeat", "hidden edit", true).await.unwrap();
    inbound.handle(&message_event(chat, "100@s.whatsapp.net", "repeat", wa::Message::text("original"))).await;
    let arrived = std::iter::from_fn(|| received.try_recv().ok()).find_map(|event| match event {
        ServiceEvent::Message { message } => Some(message), _ => None,
    }).expect("live replay carries effective row");
    let effective = inbound.store.message(chat, "repeat").await.unwrap();
    assert_eq!(serde_json::to_value(&arrived).unwrap(), serde_json::to_value(&effective).unwrap());
    assert!(arrived.spoiler && arrived.local.read && arrived.local.sort_order > 0);
    assert_eq!(arrived.text, "hidden edit");
    assert_eq!(arrived.local.status.as_deref(), Some("read"));
}

#[tokio::test]
async fn a_delivery_receipt_arrives_as_a_status_hint() {
    let (inbound, mut received) = inbound().await;
    let chat = "1@g.us";
    inbound.store.insert_message(&StoredMessage {
        header: MessageHeader { chat: chat.into(), id: "sent".into(), sender: "100@s.whatsapp.net".into(), from_me: true, ..Default::default() },
        local: LocalState { status: Some("pending".into()), ..Default::default() }, ..Default::default()
    }).await.unwrap();
    let receipt = Receipt::builder().source(MessageSource {
        chat: chat.parse().unwrap(), sender: "200:2@s.whatsapp.net".parse().unwrap(), is_group: true, ..Default::default()
    }).message_ids(vec!["sent".into()]).timestamp("2026-09-27T00:00:00Z".parse().unwrap()).r#type(ReceiptType::Read).offline(false).build();
    inbound.handle(&Event::Receipt(receipt)).await;
    let hint = (0..4).find_map(|_| received.try_recv().ok().filter(|event| matches!(event, ServiceEvent::MessageHint { .. })))
        .expect("a receipt goes out as a status hint");
    match hint {
        ServiceEvent::MessageHint { change, status, id, .. } => {
            assert_eq!(change, HintChange::Status);
            assert_eq!(id, "sent");
            assert_eq!(status.as_deref(), Some("read"));
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn receipt_events_advance_delivery_without_regression() {
    let (inbound, _) = inbound().await;
    let chat = "1@g.us";
    inbound.store.insert_message(&StoredMessage {
        header: MessageHeader { chat: chat.into(), id: "sent".into(), sender: "100@s.whatsapp.net".into(), from_me: true, ..Default::default() },
        local: LocalState { status: Some("pending".into()), ..Default::default() }, ..Default::default()
    }).await.unwrap();
    for kind in [ReceiptType::Read, ReceiptType::Delivered] {
        let receipt = Receipt::builder().source(MessageSource {
            chat: chat.parse().unwrap(), sender: "200:2@s.whatsapp.net".parse().unwrap(), is_group: true, ..Default::default()
        }).message_ids(vec!["sent".into()]).timestamp("2026-09-27T00:00:00Z".parse().unwrap()).r#type(kind).offline(false).build();
        inbound.handle(&Event::Receipt(receipt)).await;
    }
    assert_eq!(inbound.store.message(chat, "sent").await.unwrap().local.status.as_deref(), Some("read"));
    let receipts = inbound.store.receipts("sent").await.unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].recipient, "200@s.whatsapp.net");
}

fn history_chunk(chat: &str, id: &str, session: Option<&str>) -> LazyHistorySync {
    let kind = if session.is_some() { wa::history_sync::HistorySyncType::ON_DEMAND }
        else { wa::history_sync::HistorySyncType::RECENT };
    let history = wa::HistorySync {
        sync_type: kind,
        conversations: vec![wa::Conversation { id: chat.into(), messages: vec![wa::HistorySyncMsg {
            message: MessageField::some(wa::WebMessageInfo {
                key: MessageField::some(wa::MessageKey { remote_jid: Some(chat.into()), id: Some(id.into()), from_me: Some(false), ..Default::default() }),
                message: MessageField::some(wa::Message { conversation: Some("synthetic history".into()), ..Default::default() }),
                message_timestamp: Some(100), ..Default::default()
            }), ..Default::default()
        }], ..Default::default() }], ..Default::default()
    };
    let raw = history.encode_to_vec();
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(&raw).unwrap();
    LazyHistorySync::new(encoder.finish().unwrap().into(), raw.len(), kind as i32, Some(0), Some(100))
        .with_peer_data_request_session_id(session.map(str::to_string))
}

#[tokio::test]
async fn group_notices_preserve_distinct_changes_actors_and_history_replay() {
    use whatsapp_rust::wacore::{stanza::groups::{GroupNotificationAction as A, GroupParticipantInfo}, types::events::GroupUpdate};
    use wa::web_message_info::StubType;
    let (inbound, _) = inbound().await;
    let participant = |jid: &str| GroupParticipantInfo {
        jid: jid.parse().unwrap(), phone_number: None, display_name: None, r#type: None,
        lid: None, username: None, join_time: None, group_history_sent_state: None,
    };
    let change = |action: A| Event::GroupUpdate(GroupUpdate::builder()
        .group_jid("1@g.us".parse().unwrap()).participant("100@lid".parse().unwrap())
        .timestamp("2026-09-27T00:00:00Z".parse().unwrap()).is_lid_addressing_mode(false)
        .action(Box::new(action)).build());
    for target in ["200@lid", "300@lid", "200@lid"] {
        inbound.handle(&change(A::Add { participants: vec![participant(target)], reason: None })).await;
    }
    let rows = inbound.store.messages_for("1@g.us", 100).await.unwrap();
    assert_eq!(rows.len(), 2, "different participants survive; replay stays idempotent");
    assert!(rows.iter().all(|row| row.header.sender == "100@lid" && row.local.read));
    for (id, remove) in [("leave-a", true), ("rejoin-a", false), ("rejoin-a", false)] {
        let action = if remove { A::Remove { participants: vec![participant("200@lid")], reason: None } }
            else { A::Add { participants: vec![participant("200@lid")], reason: None } };
        let Event::GroupUpdate(mut update) = change(action) else { unreachable!() };
        update.notification_id = Some(id.into());
        inbound.handle(&Event::GroupUpdate(update)).await;
    }
    assert_eq!(inbound.store.count().await.unwrap(), 4, "leave/rejoin survives; repeated notification ID does not");
    for action in [
        A::Locked { threshold: None }, A::Unlocked, A::Announce, A::NotAnnounce,
        A::Ephemeral { expiration: 86400, trigger: None },
        A::MembershipApprovalMode { enabled: true }, A::MembershipApprovalMode { enabled: false },
        A::MemberAddMode { mode: "admin_add".into() },
        A::RevokeInvite, A::Delete { reason: None },
    ] {
        inbound.handle(&change(action)).await;
    }
    assert_eq!(inbound.store.count().await.unwrap(), 14);
    let web = |id: &str, kind: StubType, params: Vec<String>| wa::HistorySyncMsg {
        message: MessageField::some(wa::WebMessageInfo {
            key: MessageField::some(wa::MessageKey { remote_jid: Some("1@g.us".into()), id: Some(id.into()), participant: Some("100@lid".into()), ..Default::default() }),
            message_timestamp: Some(rows[0].header.timestamp as u64),
            message_stub_type: Some(kind), message_stub_parameters: params,
            message: MessageField::some(wa::Message::text("stub body must not become a normal message")),
            ..Default::default()
        }), ..Default::default()
    };
    let history = wa::HistorySync {
        sync_type: wa::history_sync::HistorySyncType::RECENT,
        conversations: vec![wa::Conversation { id: "1@g.us".into(), messages: vec![
            web("history-add", StubType::GROUP_PARTICIPANT_ADD, vec!["200@lid".into()]),
            web("history-number", StubType::INDIVIDUAL_CHANGE_NUMBER, vec!["200@lid".into(), "400@s.whatsapp.net".into()]),
            web("history-created", StubType::GROUP_CREATE, vec!["Synthetic group".into()]),
            web("history-deactivated", StubType::GROUP_DEACTIVATED, vec![]),
            web("history-failed", StubType::GROUP_CREATE_FAILED, vec![]),
            web("history-generic", StubType::GENERIC_NOTIFICATION, vec![]),
            web("history-payment", StubType::PAYMENT_CIPHERTEXT, vec![]),
            web("history-business", StubType::BIZ_INTRO_TOP, vec![]),
        ], ..Default::default() }], ..Default::default()
    };
    let raw = history.encode_to_vec();
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(&raw).unwrap();
    let payload = LazyHistorySync::new(encoder.finish().unwrap().into(), raw.len(), history.sync_type as i32, None, None);
    inbound.on_history_sync(&payload).await;
    inbound.on_history_sync(&payload).await;
    assert_eq!(inbound.store.count().await.unwrap(), 17);
    let number = inbound.store.message("1@g.us", "history-number").await.unwrap();
    assert_eq!(number.header.sender, "100@lid");
    assert_eq!(number.system.params, ["200@lid", "400@s.whatsapp.net"]);
    assert!(inbound.store.message("1@g.us", "history-failed").await.is_err());
}

#[tokio::test]
async fn private_disappearing_settings_become_read_notices_and_malformed_changes_stay_empty() {
    let (inbound, _) = inbound().await;
    let setting = |expiration| wa::Message { protocol_message: MessageField::some(wa::message::ProtocolMessage {
        r#type: Some(wa::message::protocol_message::Type::EPHEMERAL_SETTING), ephemeral_expiration: expiration,
        ..Default::default()
    }), ..Default::default() };
    inbound.handle(&message_event("200@s.whatsapp.net", "200@s.whatsapp.net", "timer", setting(Some(86400)))).await;
    inbound.handle(&message_event("200@s.whatsapp.net", "200@s.whatsapp.net", "bad", setting(None))).await;
    let row = inbound.store.message("200@s.whatsapp.net", "timer").await.unwrap();
    assert_eq!(row.system.kind.as_deref(), Some("CHANGE_EPHEMERAL_SETTING"));
    assert_eq!(row.system.params, ["86400"]);
    assert_eq!(row.header.sender, "200@s.whatsapp.net");
    assert!(row.local.read);
    assert_eq!(inbound.store.count().await.unwrap(), 1);
}

#[tokio::test]
async fn raw_group_mode_notices_preserve_values_actors_and_replay_identity() {
    use whatsapp_rust::wacore_binary::{builder::NodeBuilder, OwnedNodeRef};
    use whatsapp_rust::wacore::stanza::groups::GroupNotificationAction as Action;
    let (inbound, _) = inbound().await;
    let node = NodeBuilder::new("notification").attr("type", "w:gp2").attr("from", "1@g.us")
        .attr("id", "modes").attr("t", "200").attr("participant", "100@lid").attr("participant_pn", "200@s.whatsapp.net")
        .children(vec![NodeBuilder::new("subject").attr("subject", "Topic").build(),
            NodeBuilder::new("member_link_mode").string_content("admin_link").build(),
            NodeBuilder::new("member_share_group_history_mode").string_content("all_member_share").build(),
            NodeBuilder::new("group_history").build()]).build();
    let encoded = whatsapp_rust::wacore_binary::marshal::marshal(&node).unwrap();
    let event = Event::Notification(Arc::new(OwnedNodeRef::new(encoded[1..].to_vec()).unwrap()));
    inbound.handle(&event).await;
    inbound.handle(&event).await;
    let rows = inbound.store.messages_for("1@g.us", 20).await.unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|message| message.header.sender == "100@lid" && message.local.read));
    for (index, kind, value) in [(1, "GROUP_MEMBER_LINK_MODE", "admin_link"),
        (2, "GROUP_MEMBER_SHARE_GROUP_HISTORY_MODE", "all_member_share"), (3, "GROUP_CHANGE_RECENT_HISTORY_SHARING", "on")] {
        let row = inbound.store.message("1@g.us", &format!("group-mode-modes-{index}")).await.unwrap();
        assert_eq!(row.system.kind.as_deref(), Some(kind));
        assert_eq!(row.system.params, [value]);
    }
    assert_eq!(resolve_chat(None, &inbound.store, &"100@lid".parse().unwrap()).await, "200@s.whatsapp.net");
    let update = whatsapp_rust::wacore::types::events::GroupUpdate::builder()
        .group_jid("1@g.us".parse().unwrap()).notification_id("modes".into())
        .timestamp((std::time::UNIX_EPOCH + Duration::from_secs(200)).into()).is_lid_addressing_mode(true)
        .action(Box::new(Action::Subject { subject: "Topic".into(), subject_owner: None, subject_owner_pn: None,
            subject_owner_username: None, subject_time: None })).build();
    inbound.handle(&Event::GroupUpdate(update)).await;
    assert_eq!(inbound.store.count().await.unwrap(), 4, "raw mode IDs cannot collide with typed notice IDs");
    let history = wa::HistorySync {
        sync_type: wa::history_sync::HistorySyncType::RECENT,
        conversations: vec![wa::Conversation { id: "1@g.us".into(), messages: vec![wa::HistorySyncMsg {
            message: MessageField::some(wa::WebMessageInfo {
                key: MessageField::some(wa::MessageKey { remote_jid: Some("1@g.us".into()), id: Some("history-mode".into()), ..Default::default() }),
                message_timestamp: Some(200), message_stub_type: Some(wa::web_message_info::StubType::GROUP_MEMBER_LINK_MODE),
                message_stub_parameters: vec!["admin_link".into()], ..Default::default()
            }), ..Default::default()
        }], ..Default::default() }], ..Default::default()
    };
    let raw = history.encode_to_vec();
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(&raw).unwrap();
    inbound.on_history_sync(&LazyHistorySync::new(encoder.finish().unwrap().into(), raw.len(), history.sync_type as i32, None, None)).await;
    assert_eq!(inbound.store.count().await.unwrap(), 4, "history replay does not duplicate live mode notices");
}

#[tokio::test]
async fn raw_group_mode_notices_ignore_unrelated_nodes_and_keep_missing_values_unknown() {
    use whatsapp_rust::wacore_binary::builder::NodeBuilder;
    let (inbound, _) = inbound().await;
    let node = |kind: &str, from: &str, id: &str, children| NodeBuilder::new("notification")
        .attr("type", kind).attr("from", from).attr("id", id).attr("t", "200").children(children).build();
    for unsupported in [
        node("mex", "1@g.us", "wrong-type", vec![NodeBuilder::new("member_link_mode").build()]),
        node("w:gp2", "200@s.whatsapp.net", "private", vec![NodeBuilder::new("member_link_mode").build()]),
        node("w:gp2", "1@g.us", "typed", vec![NodeBuilder::new("member_add_mode").string_content("admin_add").build()]),
    ] { inbound.on_group_mode_notice(&unsupported.as_node_ref()).await; }
    assert_eq!(inbound.store.count().await.unwrap(), 0);
    let incomplete = node("w:gp2", "1@g.us", "incomplete", vec![NodeBuilder::new("member_link_mode").build(), NodeBuilder::new("no_group_history").build()]);
    inbound.on_group_mode_notice(&incomplete.as_node_ref()).await;
    let row = inbound.store.message("1@g.us", "group-mode-incomplete-0").await.unwrap();
    assert!(row.system.params.is_empty());
    assert_eq!(inbound.store.message("1@g.us", "group-mode-incomplete-1").await.unwrap().system.params, ["off"]);
}

#[tokio::test]
async fn community_owner_departures_require_confirmed_new_identity_and_replay_once() {
    use whatsapp_rust::wacore::{stanza::groups::{GroupNotificationAction as A, GroupParticipantInfo}, types::wire_enums::GroupParticipantType};
    let (inbound, _) = inbound().await;
    let previous = GroupInfo { community: true, participants: vec![Participant {
        jid: "100@s.whatsapp.net".into(), name: "Alice".into(), admin: true, owner: true,
        number: None, username: None, label: None,
    }], ..Default::default() };
    let member = |jid: &str, role| GroupParticipantInfo {
        jid: jid.parse().unwrap(), phone_number: None, display_name: None, r#type: role,
        lid: None, username: None, join_time: None, group_history_sent_state: None,
    };
    assert_eq!(super::notices::departed_owner(&A::Remove { participants: vec![member("100:2@s.whatsapp.net", None)], reason: None }, Some(&previous)).as_deref(), Some("100@s.whatsapp.net"));
    assert!(super::notices::departed_owner(&A::Remove { participants: vec![member("300@lid", None)], reason: None }, Some(&previous)).is_none());
    assert_eq!(super::notices::departed_owner(&A::Remove { participants: vec![member("100@s.whatsapp.net", Some(GroupParticipantType::SuperAdmin))], reason: None }, None).as_deref(), Some("100@s.whatsapp.net"));
    inbound.store.set_lid_pn("123", "100").await.unwrap();
    let same: Jid = "123@lid".parse().unwrap();
    let next: Jid = "200@lid".parse().unwrap();
    for (community, owner) in [(false, Some(&next)), (true, None), (true, Some(&same))] {
        inbound.record_owner_change("1@g.us", "owner-test".into(), 100, "100@s.whatsapp.net", community, owner).await;
    }
    assert_eq!(inbound.store.count().await.unwrap(), 0);
    for _ in 0..2 {
        inbound.record_owner_change("1@g.us", "owner-test".into(), 100, "100@s.whatsapp.net", true, Some(&next)).await;
    }
    let rows = inbound.store.messages_for("1@g.us", 10).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].system.kind.as_deref(), Some("COMMUNITY_OWNER_CHANGED"));
    assert_eq!(rows[0].system.params, ["100@s.whatsapp.net", "200@lid"]);
    assert!(rows[0].local.read);
}

#[tokio::test]
async fn community_links_preserve_target_and_actor_and_unlinks_are_distinct() {
    use whatsapp_rust::wacore::{stanza::groups::GroupNotificationAction as A, types::events::GroupUpdate};
    let (inbound, _) = inbound().await;
    let raw = |tag| NodeBuilder::new(tag).children([NodeBuilder::new("group").attr("jid", "2@g.us").build()]).build();
    for action in [A::Link { link_type: "sub_group".into(), raw: raw("link") },
        A::Unlink { unlink_type: "sub_group".into(), unlink_reason: None, raw: raw("unlink") }] {
        inbound.handle(&Event::GroupUpdate(GroupUpdate::builder()
            .group_jid("1@g.us".parse().unwrap()).participant("100@lid".parse().unwrap())
            .timestamp("2026-09-27T00:00:00Z".parse().unwrap()).is_lid_addressing_mode(false)
            .action(Box::new(action)).build())).await;
    }
    let rows = inbound.store.messages_for("1@g.us", 10).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.system.params == ["2@g.us"] && row.header.sender == "100@lid"));
    assert_ne!(rows[0].system.kind, rows[1].system.kind);
}

#[tokio::test]
async fn group_picture_and_subject_changes_keep_the_provided_actor() {
    use whatsapp_rust::wacore::{stanza::groups::GroupNotificationAction as A, types::events::{GroupUpdate, PictureUpdate}};
    let (inbound, mut received) = inbound().await;
    for id in ["picture-one", "picture-two", "picture-two"] {
        let mut picture = PictureUpdate::builder().jid("1@g.us".parse().unwrap())
            .timestamp("2026-09-27T00:00:00Z".parse().unwrap()).removed(false).build();
        picture.author = Some("100@lid".parse().unwrap());
        picture.picture_id = Some(id.into());
        inbound.handle(&Event::PictureUpdate(picture)).await;
    }
    let update = GroupUpdate::builder().group_jid("1@g.us".parse().unwrap())
        .timestamp("2026-09-27T00:00:00Z".parse().unwrap()).is_lid_addressing_mode(false)
        .action(Box::new(A::Subject { subject: "New title".into(), subject_owner: Some("100@lid".parse().unwrap()),
            subject_owner_pn: Some("200@s.whatsapp.net".parse().unwrap()), subject_owner_username: None, subject_time: None })).build();
    inbound.handle(&Event::GroupUpdate(update)).await;
    let rows = inbound.store.messages_for("1@g.us", 10).await.unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row.header.sender == "100@lid"));
    assert_eq!(inbound.store.name_for("1@g.us").await.unwrap().as_deref(), Some("New title"));
    assert_eq!(resolve_chat(None, &inbound.store, &"100@lid".parse().unwrap()).await, "200@s.whatsapp.net");
    assert!(std::iter::from_fn(|| received.try_recv().ok()).any(|event| matches!(event, ServiceEvent::AvatarChanged { jid } if jid == "1@g.us")));
}

#[tokio::test]
async fn history_chunks_replay_under_one_chat_after_late_mapping() {
    let (inbound, mut received) = inbound().await;
    let lid = "123@lid";
    let pn = "5989@s.whatsapp.net";
    inbound.on_history_sync(&history_chunk(lid, "first", None)).await;
    inbound.on_history_sync(&history_chunk(pn, "second", None)).await;
    assert_eq!(inbound.store.chats().await.unwrap().len(), 2);
    inbound.store.set_lid_pn("123", "5989").await.unwrap();
    inbound.on_history_sync(&history_chunk(lid, "third", None)).await;
    inbound.on_history_sync(&history_chunk(lid, "first", None)).await;
    assert_eq!(inbound.store.chats().await.unwrap().len(), 1);
    assert!(inbound.store.messages_for(lid, 10).await.unwrap().iter().all(|row| row.header.chat == pn));
    let rows = inbound.store.messages_for(pn, 10).await.unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row.local.read && row.text == "synthetic history"));
    assert_eq!(resolve_chat(None, &inbound.store, &lid.parse().unwrap()).await, pn);
    while received.try_recv().is_ok() {}
    inbound.older_waits.lock().unwrap().remember(std::time::Instant::now(), "request-1", pn);
    inbound.on_history_sync(&history_chunk(lid, "first", Some("request-1"))).await;
    assert!(matches!(received.try_recv().unwrap(), ServiceEvent::HistoryLoaded { chats } if chats == [pn]));
    assert!(received.try_recv().is_err());
    let corrupt = LazyHistorySync::new(vec![0, 1, 2].into(), 3, 3, None, None);
    inbound.on_history_sync(&corrupt).await;
    assert_eq!(inbound.store.count().await.unwrap(), 3);
}
