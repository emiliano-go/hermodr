use super::*;
use super::protocol_tests::{history_chunk, inbound, message_event};
use buffa::Message as _;
use std::io::Write;

fn requested_chunk(chat: Option<&str>, rows: &[(&str, u64)], session: &str) -> whatsapp_rust::wacore::types::events::LazyHistorySync {
    let history = wa::HistorySync {
        sync_type: wa::history_sync::HistorySyncType::ON_DEMAND,
        conversations: chat.into_iter().map(|chat| wa::Conversation {
            id: chat.into(), messages: rows.iter().map(|(id, timestamp)| wa::HistorySyncMsg {
                message: MessageField::some(wa::WebMessageInfo {
                    key: MessageField::some(wa::MessageKey { remote_jid: Some(chat.into()), id: Some((*id).into()), from_me: Some(false), ..Default::default() }),
                    message: MessageField::some(wa::Message { conversation: Some("synthetic requested history".into()), ..Default::default() }),
                    message_timestamp: Some(*timestamp), ..Default::default()
                }), ..Default::default()
            }).collect(), ..Default::default()
        }).collect(), ..Default::default()
    };
    let raw = history.encode_to_vec();
    let mut compressed = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    compressed.write_all(&raw).unwrap();
    whatsapp_rust::wacore::types::events::LazyHistorySync::new(compressed.finish().unwrap().into(), raw.len(), 6, None, None)
        .with_peer_data_request_session_id(Some(session.into()))
}

fn completion_count(events: &mut broadcast::Receiver<ServiceEvent>, chat: &str) -> usize {
    let mut count = 0;
    while let Ok(event) = events.try_recv() {
        if matches!(event, ServiceEvent::HistoryLoaded { chats } if chats.iter().any(|loaded| loaded == chat)) { count += 1; }
    }
    count
}

async fn seed(handler: &Inbound, chat: &str, id: &str, timestamp: i64) {
    handler.store.insert_message(&StoredMessage { header: MessageHeader {
        chat: chat.into(), id: id.into(), sender: "100@s.whatsapp.net".into(), timestamp, ..Default::default()
    }, text: "synthetic stored history".into(), ..Default::default() }).await.unwrap();
}

fn live(chat: &str, id: &str) -> Event {
    let Event::Messages(batch) = message_event(chat, "100@s.whatsapp.net", id, wa::Message {
        conversation: Some("synthetic live message".into()), ..Default::default()
    }) else { unreachable!() };
    let message = &batch.messages[0];
    let mut info = (*message.info).clone();
    info.timestamp = std::time::SystemTime::now().into();
    Event::Messages(whatsapp_rust::wacore::types::events::MessageBatch::builder()
        .messages(vec![whatsapp_rust::wacore::types::events::InboundMessage::builder()
            .message(message.message.clone()).info(Arc::new(info)).build()].into())
        .origin(whatsapp_rust::wacore::types::events::BatchOrigin::Live).build())
}

fn limits() -> DiskRetention {
    DiskRetention { max_age_hours: crate::store::RetentionLimit::Limited(24),
        max_messages_per_chat: crate::store::RetentionLimit::Limited(1) }
}

#[tokio::test]
async fn requested_history_floor_survives_live_writes_for_inherited_and_chat_limits() {
    for per_chat in [false, true] {
        let (mut handler, _) = inbound().await;
        let chat = "101@g.us";
        handler.disk_retention = Arc::new(DiskRetentionManager::new(if per_chat { DiskRetention::unlimited() } else { limits() }));
        if per_chat {
            handler.store.set_chat_retention(chat, &crate::store::ChatRetention {
                max_age_hours: crate::store::RetentionLimit::Limited(24), max_messages: crate::store::RetentionLimit::Limited(1),
                ..Default::default()
            }).await.unwrap();
        }
        handler.older_waits.lock().unwrap().remember(std::time::Instant::now(), "explicit", chat);
        handler.handle(&Event::HistorySync(Box::new(history_chunk(chat, "requested", Some("explicit"))))).await;
        for id in ["live-1", "live-2"] { handler.handle(&live(chat, id)).await; }
        assert!(handler.store.message(chat, "requested").await.is_ok());
        assert_eq!(handler.store.messages_for(chat, 20).await.unwrap().len(), 3);
    }
}

#[tokio::test]
async fn automatic_pairing_unknown_and_wrong_chat_history_receive_no_floor() {
    for (session, remembered_chat) in [(None, None), (Some("automatic"), None),
        (Some("explicit"), Some("other@g.us"))] {
        let (mut handler, _) = inbound().await;
        let chat = "101@g.us";
        handler.disk_retention = Arc::new(DiskRetentionManager::new(limits()));
        if let Some(remembered) = remembered_chat {
            handler.older_waits.lock().unwrap().remember(std::time::Instant::now(), "explicit", remembered);
        }
        handler.handle(&Event::HistorySync(Box::new(history_chunk(chat, "ordinary", session)))).await;
        handler.handle(&live(chat, "live")).await;
        assert!(handler.store.message(chat, "ordinary").await.is_err());
        assert!(handler.store.message(chat, "live").await.is_ok());
    }
}

#[tokio::test]
async fn early_actual_session_promotes_floor_before_temporary_prune_protection_ends() {
    let (mut handler, _) = inbound().await;
    let chat = "101@g.us";
    handler.disk_retention = Arc::new(DiskRetentionManager::new(limits()));
    let (request, generation) = handler.older_waits.lock().unwrap().start(chat).unwrap();
    handler.handle(&Event::HistorySync(Box::new(history_chunk(chat, "automatic", Some("different-session"))))).await;
    handler.handle(&Event::HistorySync(Box::new(history_chunk(chat, "early", Some("actual-session"))))).await;
    handler.handle(&Event::HistorySync(Box::new(history_chunk("202@g.us", "unrequested", None)))).await;
    handler.handle(&live(chat, "live-before-ack")).await;
    assert!(handler.store.message(chat, "early").await.is_ok());
    assert!(handler.store.message("202@g.us", "unrequested").await.is_err());
    let waits = handler.older_waits.clone();
    let events = handler.events.clone();
    handler.store.run(move |store| {
        let mut waits = waits.lock().unwrap();
        assert!(waits.acknowledge_for_store(store, request, generation, "actual-session", &events)?);
        assert!(waits.protected_chats(store)?.is_empty());
        Ok(())
    }).await.unwrap();
    handler.handle(&live(chat, "live-after-ack")).await;
    assert!(handler.store.message(chat, "early").await.is_ok());
}

#[tokio::test]
async fn an_unrelated_early_session_does_not_become_an_explicit_history_floor() {
    let (mut handler, _) = inbound().await;
    let chat = "101@g.us";
    handler.disk_retention = Arc::new(DiskRetentionManager::new(limits()));
    let (request, generation) = handler.older_waits.lock().unwrap().start(chat).unwrap();
    handler.handle(&Event::HistorySync(Box::new(history_chunk(chat, "automatic", Some("different-session"))))).await;
    let waits = handler.older_waits.clone();
    let events = handler.events.clone();
    handler.store.run(move |store| {
        assert!(waits.lock().unwrap().acknowledge_for_store(store, request, generation, "actual-session", &events)?);
        Ok(())
    }).await.unwrap();
    handler.handle(&live(chat, "live")).await;
    assert!(handler.store.message(chat, "automatic").await.is_err());
    handler.handle(&Event::HistorySync(Box::new(history_chunk(chat, "requested", Some("actual-session"))))).await;
    handler.handle(&live(chat, "live-after-answer")).await;
    assert!(handler.store.message(chat, "requested").await.is_ok());
}

#[test]
fn cancelled_alias_request_cannot_restore_floor_from_a_late_send_ack() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    let mut waits = OlderWaits::default();
    let (events, _) = broadcast::channel(4);
    let (request, generation) = waits.start("9@lid").unwrap();
    let (other, other_generation) = waits.start("2@s.whatsapp.net").unwrap();
    waits.remember(std::time::Instant::now(), "already-acked", "9@lid");
    store.set_lid_pn("9", "1").unwrap();
    waits.cancel_for_store(&store, Some("1@s.whatsapp.net")).unwrap();
    assert_eq!(waits.resolve("already-acked"), None);
    assert!(!waits.acknowledge_for_store(&store, request, generation, "late-ack", &events).unwrap());
    assert_eq!(waits.protected_chats(&store).unwrap(), vec!["2@s.whatsapp.net"]);
    assert!(waits.acknowledge_for_store(&store, other, other_generation, "other-ack", &events).unwrap());
    assert_eq!(waits.resolve("other-ack").as_deref(), Some("2@s.whatsapp.net"));
    let (fresh, fresh_generation) = waits.start("1@s.whatsapp.net").unwrap();
    waits.cancel_for_store(&store, None).unwrap();
    assert!(!waits.acknowledge_for_store(&store, fresh, fresh_generation, "after-clear-all", &events).unwrap());
    assert!(waits.protected_chats(&store).unwrap().is_empty());
}

#[tokio::test]
async fn same_session_later_history_chunk_lowers_floor_after_first_completion_and_early_ack() {
    let chat = "101@g.us";
    for early in [false, true] {
        let (mut handler, mut received) = inbound().await;
        handler.disk_retention = Arc::new(DiskRetentionManager::new(limits()));
        let intent = if early { Some(handler.older_waits.lock().unwrap().start(chat).unwrap()) } else {
            handler.older_waits.lock().unwrap().remember(std::time::Instant::now(), "session", chat);
            None
        };
        handler.handle(&Event::HistorySync(Box::new(requested_chunk(Some(chat), &[("first", 100)], "session")))).await;
        assert_eq!(completion_count(&mut received, chat), 1);
        if let Some((request, generation)) = intent {
            let (waits, events) = (handler.older_waits.clone(), handler.events.clone());
            handler.store.run(move |store| {
                assert!(waits.lock().unwrap().acknowledge_for_store(store, request, generation, "session", &events)?);
                Ok(())
            }).await.unwrap();
            assert_eq!(completion_count(&mut received, chat), 0, "ACK repeated an existing completion");
        }
        handler.handle(&live(chat, "live-first")).await;
        seed(&handler, chat, "outside-request", 25).await;
        handler.handle(&Event::HistorySync(Box::new(requested_chunk(Some(chat), &[("first", 1), ("later", 50)], "session")))).await;
        handler.handle(&live(chat, "live-later")).await;
        assert!(handler.store.message(chat, "first").await.is_ok());
        assert!(handler.store.message(chat, "later").await.is_ok(), "a later requested chunk was pruned");
        assert!(handler.store.message(chat, "outside-request").await.is_err(), "duplicate payload lowered the floor using a forged timestamp");
        assert_eq!(handler.older_waits.lock().unwrap().resolve("session"), None);
    }
}

#[tokio::test]
async fn early_empty_and_duplicate_history_answers_complete_once_after_ack() {
    let chat = "101@g.us";
    for duplicate in [false, true] {
        let (mut handler, mut received) = inbound().await;
        handler.disk_retention = Arc::new(DiskRetentionManager::new(limits()));
        seed(&handler, chat, "outside-request", 50).await;
        if duplicate { seed(&handler, chat, "duplicate", 100).await; }
        let (request, generation) = handler.older_waits.lock().unwrap().start(chat).unwrap();
        let chunk = if duplicate { requested_chunk(Some(chat), &[("duplicate", 1)], "session") }
            else { requested_chunk(None, &[], "session") };
        handler.handle(&Event::HistorySync(Box::new(chunk))).await;
        assert_eq!(completion_count(&mut received, chat), 0);
        let (waits, events) = (handler.older_waits.clone(), handler.events.clone());
        handler.store.run(move |store| {
            let mut waits = waits.lock().unwrap();
            assert!(waits.acknowledge_for_store(store, request, generation, "session", &events)?);
            assert!(!waits.acknowledge_for_store(store, request, generation, "session", &events)?);
            assert_eq!(waits.resolve("session"), None);
            Ok(())
        }).await.unwrap();
        assert_eq!(completion_count(&mut received, chat), 1, "early answer did not complete exactly once after ACK");
        handler.handle(&live(chat, "live")).await;
        assert!(handler.store.message(chat, "outside-request").await.is_err());
        if duplicate { assert!(handler.store.message(chat, "duplicate").await.is_ok()); }
        assert_eq!(completion_count(&mut received, chat), 0);
    }
}
