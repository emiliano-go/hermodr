use super::*;
use buffa::Message as _;
use std::io::Write;
use whatsapp_rust::wacore::types::events::LazyHistorySync;

fn inbound() -> (Inbound, broadcast::Receiver<ServiceEvent>) {
    let (events, received) = broadcast::channel(32);
    (Inbound {
        store: Arc::new(MessageStore::open(Path::new(":memory:")).unwrap()),
        disk_retention: Arc::new(DiskRetentionManager::new(DiskRetention::unlimited())),
        events, connected: Arc::default(), client_for_events: Arc::default(), media_dir: None,
        group_cache: Arc::default(), groups_cache: Arc::default(), older_waits: Arc::default(),
        downloads: Arc::new(tokio::sync::Semaphore::new(1)), sync_progress: Arc::default(),
        auto_download_default: false, keep_archived: Arc::default(), keep_view_once: Arc::default(),
    }, received)
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
    let (inbound, mut received) = inbound();
    let lid = "123@lid";
    let pn = "5989@s.whatsapp.net";
    inbound.on_history_sync(&history_chunk(lid, "first", None)).await;
    inbound.on_history_sync(&history_chunk(pn, "second", None)).await;
    assert_eq!(inbound.store.chats().unwrap().len(), 2);
    inbound.store.set_lid_pn("123", "5989").unwrap();
    inbound.on_history_sync(&history_chunk(lid, "third", None)).await;
    inbound.on_history_sync(&history_chunk(lid, "first", None)).await;
    assert_eq!(inbound.store.chats().unwrap().len(), 1);
    assert!(inbound.store.messages_for(lid, 10).unwrap().iter().all(|row| row.header.chat == pn));
    let rows = inbound.store.messages_for(pn, 10).unwrap();
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
    assert_eq!(inbound.store.count().unwrap(), 3);
}
