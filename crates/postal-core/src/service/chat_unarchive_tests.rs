use super::super::protocol_tests::{history_chunk, inbound, message_event};
use super::super::*;
use whatsapp_rust::wacore::types::events::{BatchOrigin, InboundMessage, MessageBatch};

fn arrival(chat: &str, id: &str, origin: BatchOrigin, offline: bool, from_me: bool) -> Event {
    let Event::Messages(batch) = message_event(
        chat,
        "100@s.whatsapp.net",
        id,
        wa::Message {
            conversation: Some("synthetic incoming message".into()),
            ..Default::default()
        },
    ) else {
        unreachable!()
    };
    let original = &batch.messages[0];
    let mut info = (*original.info).clone();
    info.is_offline = offline;
    info.source.is_from_me = from_me;
    let message = InboundMessage::builder()
        .message(original.message.clone())
        .info(Arc::new(info))
        .build();
    Event::Messages(
        MessageBatch::builder()
            .messages(vec![message].into())
            .origin(origin)
            .build(),
    )
}

#[tokio::test]
async fn only_new_live_incoming_unarchives_with_nullable_override_and_global_default() {
    let chat = "101@g.us";
    for keep_archived in [false, true] {
        for override_value in [None, Some(false), Some(true)] {
            let (handler, mut events) = inbound().await;
            handler.keep_archived.store(keep_archived, Ordering::SeqCst);
            handler
                .store
                .set_chat_unarchive(chat, override_value)
                .await
                .unwrap();
            handler.store.set_archived(chat, true).await.unwrap();
            handler
                .handle(&Event::HistorySync(Box::new(history_chunk(
                    chat, "history", None,
                ))))
                .await;
            assert!(handler.store.message(chat, "history").await.is_ok());
            assert!(
                handler.store.is_archived(chat).await.unwrap(),
                "history unarchived"
            );
            for event in [
                arrival(chat, "offline", BatchOrigin::Live, true, false),
                arrival(chat, "drain", BatchOrigin::OfflineDrain, false, false),
                arrival(chat, "outbound", BatchOrigin::Live, false, true),
            ] {
                handler.handle(&event).await;
                assert!(
                    handler.store.is_archived(chat).await.unwrap(),
                    "non-live incoming unarchived"
                );
            }
            handler
                .store
                .insert_message(&StoredMessage {
                    header: MessageHeader {
                        chat: chat.into(),
                        id: "duplicate".into(),
                        sender: "100@s.whatsapp.net".into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .await
                .unwrap();
            handler
                .handle(&arrival(chat, "duplicate", BatchOrigin::Live, false, false))
                .await;
            assert!(
                handler.store.is_archived(chat).await.unwrap(),
                "repeated live id unarchived"
            );
            while let Ok(event) = events.try_recv() {
                assert!(!matches!(event, ServiceEvent::ChatStateChanged { .. }));
            }
            let expected = override_value.unwrap_or(!keep_archived);
            handler
                .handle(&arrival(chat, "fresh", BatchOrigin::Live, false, false))
                .await;
            assert_eq!(
                handler.store.is_archived(chat).await.unwrap(),
                !expected,
                "keep_archived={keep_archived}, override={override_value:?}"
            );
            let mut state_changes = 0;
            while let Ok(event) = events.try_recv() {
                if matches!(event, ServiceEvent::ChatStateChanged { .. }) {
                    state_changes += 1;
                }
            }
            assert_eq!(state_changes, expected as usize);
        }
    }
}
