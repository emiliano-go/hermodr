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
