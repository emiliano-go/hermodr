use super::*;

#[test]
fn writable_target_blocks_only_non_status_broadcast_lists() {
    for chat in ["12345@broadcast", "999@broadcast"] {
        assert_eq!(
            writable_target(chat).unwrap_err().to_string(),
            "Sending to broadcast lists is not supported."
        );
    }
    for chat in [
        "15550000001@s.whatsapp.net",
        "777@lid",
        "123@g.us",
        "status@broadcast",
    ] {
        assert_eq!(writable_target(chat).unwrap().to_string(), chat);
    }
    assert!(writable_target("bad").is_err());
}

#[tokio::test]
async fn admitted_broadcast_batch_keeps_list_title_and_coalesces_committed_recipient_notice() {
    use whatsapp_rust::wacore::types::{
        events::{BatchOrigin, InboundMessage, MessageBatch},
        message::{MessageInfo, MessageSource},
    };
    let (handler, mut notices) = super::super::protocol_tests::inbound().await;
    let chat = "12345@broadcast";
    let sender = "777@lid";
    let pn = "15550000001@s.whatsapp.net";
    handler.store.set_name(chat, "Family list").await.unwrap();
    handler
        .store
        .set_saved_name(pn, "Saved sender")
        .await
        .unwrap();
    let messages = [
        ("first", "2026-10-01T00:00:00Z", pn),
        ("latest", "2026-10-01T00:01:00Z", "888@lid"),
    ]
    .into_iter()
    .map(|(id, time, recipient)| {
        let info = MessageInfo {
            id: id.into(),
            timestamp: time.parse().unwrap(),
            push_name: "Push sender".into(),
            source: MessageSource {
                chat: chat.parse().unwrap(),
                sender: sender.parse().unwrap(),
                sender_alt: Some(pn.parse().unwrap()),
                is_group: false,
                ..Default::default()
            },
            bcl_participants: vec![recipient.parse().unwrap()],
            ..Default::default()
        };
        InboundMessage::builder()
            .message(Arc::new(wa::Message {
                conversation: Some("Broadcast".into()),
                ..Default::default()
            }))
            .info(Arc::new(info))
            .build()
    })
    .collect::<Vec<_>>();
    handler
        .handle(&Event::Messages(
            MessageBatch::builder()
                .messages(messages.into())
                .origin(BatchOrigin::Live)
                .build(),
        ))
        .await;
    assert_eq!(
        handler.store.name_for(chat).await.unwrap().as_deref(),
        Some("Family list")
    );
    assert_eq!(
        handler.store.name_for(sender).await.unwrap().as_deref(),
        Some("Saved sender")
    );
    let mut metadata_notices = 0;
    while let Ok(notice) = notices.try_recv() {
        if let ServiceEvent::ChatStateChanged { chat: changed } = notice {
            assert_eq!(changed, chat);
            assert_eq!(
                handler
                    .store
                    .broadcast_list(chat)
                    .await
                    .unwrap()
                    .unwrap()
                    .recipients,
                vec!["888@lid"]
            );
            metadata_notices += 1;
        }
    }
    assert_eq!(metadata_notices, 1);
}
