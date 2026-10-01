use super::*;
use buffa::MessageField;
use buffa::Message as _;
use std::io::Write;
use whatsapp_rust::wacore::types::message::MessageSource;

fn info(id: &str) -> MessageInfo {
    MessageInfo { id: id.into(), timestamp: "1970-01-01T00:01:40Z".parse().unwrap(),
        source: MessageSource { chat: "100@s.whatsapp.net".parse().unwrap(),
            sender: "100@s.whatsapp.net".parse().unwrap(), ..Default::default() }, ..Default::default() }
}

#[tokio::test]
async fn unavailable_live_recovery_uses_existing_events_and_authoritative_badges() {
    let (handler, mut received) = super::super::protocol_tests::inbound().await;
    let metadata = info("live");
    handler.store_unavailable(&metadata).await;
    let hint = serde_json::to_value(received.try_recv().unwrap()).unwrap();
    assert_eq!(hint["kind"], "messageHint");
    assert_eq!(hint["fresh"], false);
    assert_eq!(handler.store.chats().await.unwrap()[0].unread_count, 0);
    handler.store_unavailable(&metadata).await;
    assert!(received.try_recv().is_err());
    handler.handle(&super::super::protocol_tests::message_event("100@s.whatsapp.net", "100@s.whatsapp.net", "live",
        wa::Message::text("Recovered content"))).await;
    let row = handler.store.message("100@s.whatsapp.net", "live").await.unwrap();
    assert!(!row.is_unavailable());
    assert_eq!(row.text, "Recovered content");
    assert_eq!(handler.store.chats().await.unwrap()[0].unread_count, 1);
    assert_eq!(handler.store.unread_ids("100@s.whatsapp.net").await.unwrap().len(), 1);
    handler.store_unavailable(&metadata).await;
    assert_eq!(handler.store.message("100@s.whatsapp.net", "live").await.unwrap().text, "Recovered content");
}

#[tokio::test]
async fn unavailable_history_recovery_heals_existing_id_without_rewriting_real_history() {
    let (handler, _) = super::super::protocol_tests::inbound().await;
    handler.store_unavailable(&info("history")).await;
    handler.handle(&Event::HistorySync(Box::new(super::super::protocol_tests::history_chunk("100@s.whatsapp.net", "history", None)))).await;
    let row = handler.store.message("100@s.whatsapp.net", "history").await.unwrap();
    assert_eq!(row.text, "synthetic history");
    assert!(!row.is_unavailable() && !row.local.read);
    assert_eq!(handler.store.chats().await.unwrap()[0].unread_count, 1);
    handler.store.update_message_content("100@s.whatsapp.net", "history", "Edited locally").await.unwrap();
    handler.handle(&Event::HistorySync(Box::new(super::super::protocol_tests::history_chunk("100@s.whatsapp.net", "history", None)))).await;
    assert_eq!(handler.store.message("100@s.whatsapp.net", "history").await.unwrap().text, "Edited locally");
}

#[tokio::test]
async fn unavailable_controls_retire_with_content_hint_and_unknown_revoke_cannot_resurrect() {
    let (handler, mut received) = super::super::protocol_tests::inbound().await;
    handler.store_unavailable(&info("control")).await;
    received.try_recv().unwrap();
    let revoke = wa::Message { protocol_message: MessageField::some(wa::message::ProtocolMessage {
        r#type: Some(wa::message::protocol_message::Type::REVOKE),
        key: MessageField::some(wa::MessageKey { id: Some("unknown-target".into()), ..Default::default() }),
        ..Default::default()
    }), ..Default::default() };
    handler.handle(&super::super::protocol_tests::message_event("100@s.whatsapp.net", "100@s.whatsapp.net", "control", revoke)).await;
    let hint = serde_json::to_value(received.try_recv().unwrap()).unwrap();
    assert_eq!(hint["kind"], "messageHint");
    assert_eq!(hint["fresh"], false);
    assert!(handler.store.message("100@s.whatsapp.net", "control").await.is_err());
    assert!(handler.store.message("100@s.whatsapp.net", "unknown-target").await.is_err());
    for id in ["control", "unknown-target"] { handler.store_unavailable(&info(id)).await; }
    assert!(handler.store.messages_for("100@s.whatsapp.net", 100).await.unwrap().is_empty());
    handler.handle(&super::super::protocol_tests::message_event("100@s.whatsapp.net", "100@s.whatsapp.net", "unknown-target",
        wa::Message::text("Must remain revoked"))).await;
    assert!(handler.store.message("100@s.whatsapp.net", "unknown-target").await.is_err());
}

#[tokio::test]
async fn unavailable_history_revoke_retires_both_control_and_failed_target() {
    let (handler, _) = super::super::protocol_tests::inbound().await;
    for id in ["history-control", "failed-target"] { handler.store_unavailable(&info(id)).await; }
    let revoke = wa::Message { protocol_message: MessageField::some(wa::message::ProtocolMessage {
        r#type: Some(wa::message::protocol_message::Type::REVOKE),
        key: MessageField::some(wa::MessageKey { id: Some("failed-target".into()), ..Default::default() }),
        ..Default::default()
    }), ..Default::default() };
    let history = wa::HistorySync {
        sync_type: wa::history_sync::HistorySyncType::RECENT,
        conversations: vec![wa::Conversation { id: "100@s.whatsapp.net".into(), messages: vec![wa::HistorySyncMsg {
            message: MessageField::some(wa::WebMessageInfo {
                key: MessageField::some(wa::MessageKey { remote_jid: Some("100@s.whatsapp.net".into()),
                    id: Some("history-control".into()), from_me: Some(false), ..Default::default() }),
                message: MessageField::some(revoke), message_timestamp: Some(100), ..Default::default()
            }), ..Default::default()
        }], ..Default::default() }], ..Default::default()
    };
    let raw = history.encode_to_vec();
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(&raw).unwrap();
    let sync = whatsapp_rust::wacore::types::events::LazyHistorySync::new(encoder.finish().unwrap().into(), raw.len(),
        wa::history_sync::HistorySyncType::RECENT as i32, Some(0), Some(100));
    handler.handle(&Event::HistorySync(Box::new(sync))).await;
    assert!(handler.store.messages_for("100@s.whatsapp.net", 100).await.unwrap().is_empty());
    for id in ["history-control", "failed-target"] { handler.store_unavailable(&info(id)).await; }
    assert!(handler.store.messages_for("100@s.whatsapp.net", 100).await.unwrap().is_empty());
}

#[tokio::test]
async fn unavailable_remote_whole_chat_read_survives_healing_without_local_receipts() {
    let (handler, _) = super::super::protocol_tests::inbound().await;
    handler.store_unavailable(&info("remote-read")).await;
    assert_eq!(handler.store.mark_read("100@s.whatsapp.net").await.unwrap(), 0);
    let update = whatsapp_rust::wacore::types::events::MarkChatAsReadUpdate::builder()
        .jid("100@s.whatsapp.net".parse().unwrap())
        .action(Box::new(wa::sync_action_value::MarkChatAsReadAction { read: Some(true), ..Default::default() }))
        .timestamp("1970-01-01T00:01:40Z".parse().unwrap()).from_full_sync(false).build();
    handler.handle(&Event::MarkChatAsReadUpdate(update.into())).await;
    assert!(handler.store.message("100@s.whatsapp.net", "remote-read").await.unwrap().local.read);
    assert!(handler.store.unread_ids("100@s.whatsapp.net").await.unwrap().is_empty());
    handler.handle(&super::super::protocol_tests::message_event("100@s.whatsapp.net", "100@s.whatsapp.net", "remote-read",
        wa::Message::text("Read on the phone"))).await;
    let row = handler.store.message("100@s.whatsapp.net", "remote-read").await.unwrap();
    assert!(!row.is_unavailable() && row.local.read);
    assert_eq!(handler.store.chats().await.unwrap()[0].unread_count, 0);
}
