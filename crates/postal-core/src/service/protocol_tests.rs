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
    }, received)
}

fn message_event(chat: &str, sender: &str, id: &str, message: wa::Message) -> Event {
    let info = MessageInfo { id: id.into(), source: MessageSource {
        chat: chat.parse().unwrap(), sender: sender.parse().unwrap(), is_group: chat.ends_with("@g.us"), ..Default::default()
    }, ..Default::default() };
    let message = InboundMessage::builder().message(Arc::new(message)).info(Arc::new(info)).build();
    Event::Messages(MessageBatch::builder().messages(vec![message].into()).origin(BatchOrigin::Live).build())
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
    for action in [
        A::Locked { threshold: None }, A::Unlocked, A::Announce, A::NotAnnounce,
        A::Ephemeral { expiration: 86400, trigger: None },
        A::MembershipApprovalMode { enabled: true }, A::MembershipApprovalMode { enabled: false },
        A::MemberAddMode { mode: "admin_add".into() },
        A::RevokeInvite, A::Delete { reason: None },
    ] {
        inbound.handle(&change(action)).await;
    }
    assert_eq!(inbound.store.count().await.unwrap(), 12);
    let web = |id: &str, kind: StubType, params: Vec<String>| wa::HistorySyncMsg {
        message: MessageField::some(wa::WebMessageInfo {
            key: MessageField::some(wa::MessageKey { remote_jid: Some("1@g.us".into()), id: Some(id.into()), participant: Some("100@lid".into()), ..Default::default() }),
            message_timestamp: Some(rows[0].header.timestamp as u64),
            message_stub_type: Some(kind), message_stub_parameters: params,
            ..Default::default()
        }), ..Default::default()
    };
    let history = wa::HistorySync {
        sync_type: wa::history_sync::HistorySyncType::RECENT,
        conversations: vec![wa::Conversation { id: "1@g.us".into(), messages: vec![
            web("history-add", StubType::GROUP_PARTICIPANT_ADD, vec!["200@lid".into()]),
            web("history-number", StubType::INDIVIDUAL_CHANGE_NUMBER, vec!["200@lid".into(), "400@s.whatsapp.net".into()]),
            web("history-failed", StubType::GROUP_CREATE_FAILED, vec![]),
        ], ..Default::default() }], ..Default::default()
    };
    let raw = history.encode_to_vec();
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(&raw).unwrap();
    let payload = LazyHistorySync::new(encoder.finish().unwrap().into(), raw.len(), history.sync_type as i32, None, None);
    inbound.on_history_sync(&payload).await;
    inbound.on_history_sync(&payload).await;
    assert_eq!(inbound.store.count().await.unwrap(), 13);
    let number = inbound.store.message("1@g.us", "history-number").await.unwrap();
    assert_eq!(number.header.sender, "100@lid");
    assert_eq!(number.system.params, ["200@lid", "400@s.whatsapp.net"]);
    assert!(inbound.store.message("1@g.us", "history-failed").await.is_err());
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
