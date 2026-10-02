use super::*;
use whatsapp_rust::wacore::types::events::{DecryptedPayload, EventHandler};

async fn handle_verified(handler: &Inbound, event: &Event) {
    if let Event::Messages(batch) = event {
        for message in batch.messages.iter() {
            let raw = DecryptedPayload::builder()
                .info(message.info.clone())
                .enc_index(0)
                .enc_type("msg")
                .payload(message.message.as_ref().encode_to_vec().into())
                .build();
            handler
                .secret_edits
                .handler()
                .handle_event(Arc::new(Event::DecryptedPayload(raw)));
        }
    }
    handler.handle(event).await;
}

const CHAT: &str = "1@g.us";
const CREATOR: &str = "15550000001@s.whatsapp.net";
const VOTER: &str = "15550000002@s.whatsapp.net";
const AUTHORED: i64 = 1_700_000_000_000;

fn creation(secret: Option<&[u8]>, name: &str, canceled: bool) -> wa::Message {
    wa::Message {
        event_message: MessageField::some(wa::message::EventMessage {
            name: Some(name.into()),
            start_time: Some(100),
            end_time: Some(200),
            extra_guests_allowed: Some(true),
            is_canceled: Some(canceled),
            ..Default::default()
        }),
        message_context_info: secret
            .map(|secret| wa::MessageContextInfo {
                message_secret: Some(secret.to_vec()),
                ..Default::default()
            })
            .into(),
        ..Default::default()
    }
}

fn rsvp(
    voter: &str,
    key_chat: &str,
    creator_hint: &str,
    state: wa::message::event_response_message::EventResponseType,
    timestamp: i64,
) -> wa::Message {
    let answer = wa::message::EventResponseMessage {
        response: Some(state),
        timestamp_ms: Some(timestamp),
        extra_guest_count: Some(1),
    };
    let (payload, iv) = whatsapp_rust::wacore::event::encrypt_event_response_with_secret(
        &answer, &[7; 32], "event", CREATOR, voter,
    )
    .unwrap();
    wa::Message {
        enc_event_response_message: MessageField::some(wa::message::EncEventResponseMessage {
            event_creation_message_key: MessageField::some(wa::MessageKey {
                id: Some("event".into()),
                remote_jid: Some(key_chat.into()),
                participant: Some(creator_hint.into()),
                from_me: Some(false),
                ..Default::default()
            }),
            enc_payload: Some(payload),
            enc_iv: Some(iv.to_vec()),
        }),
        ..Default::default()
    }
}

fn wire(sender: &str, id: &str, message: wa::Message, from_me: bool) -> Event {
    let info = MessageInfo {
        id: id.into(),
        source: MessageSource {
            chat: CHAT.parse().unwrap(),
            sender: sender.parse().unwrap(),
            is_group: true,
            is_from_me: from_me,
            ..Default::default()
        },
        ..Default::default()
    };
    let message = InboundMessage::builder()
        .message(Arc::new(message))
        .info(Arc::new(info))
        .build();
    Event::Messages(
        MessageBatch::builder()
            .messages(vec![message].into())
            .origin(BatchOrigin::Live)
            .build(),
    )
}

fn history_entry(sender: &str, id: &str, message: wa::Message) -> wa::HistorySyncMsg {
    wa::HistorySyncMsg {
        message: MessageField::some(wa::WebMessageInfo {
            key: MessageField::some(wa::MessageKey {
                id: Some(id.into()),
                remote_jid: Some(CHAT.into()),
                participant: Some(sender.into()),
                from_me: Some(false),
                ..Default::default()
            }),
            participant: Some(sender.into()),
            message_timestamp: Some(100),
            message: MessageField::some(message),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn history_event(entries: Vec<wa::HistorySyncMsg>) -> Event {
    let history = wa::HistorySync {
        sync_type: wa::history_sync::HistorySyncType::RECENT,
        conversations: vec![wa::Conversation {
            id: CHAT.into(),
            messages: entries,
            ..Default::default()
        }],
        ..Default::default()
    };
    let raw = history.encode_to_vec();
    let mut compressed = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    compressed.write_all(&raw).unwrap();
    Event::HistorySync(Box::new(LazyHistorySync::new(
        compressed.finish().unwrap().into(),
        raw.len(),
        history.sync_type as i32,
        Some(0),
        Some(100),
    )))
}

async fn receipts_for(handler: &Inbound, ids: &[&str]) -> usize {
    let mut count = 0;
    for id in ids {
        count += handler.store.receipts(id).await.unwrap().len();
    }
    count
}

#[tokio::test]
async fn live_rsvp_before_creation_flushes_after_admission_without_phantom_rows_or_receipts() {
    use wa::message::event_response_message::EventResponseType;
    let (handler, mut notices) = inbound().await;
    handler
        .handle(&wire(
            VOTER,
            "before",
            rsvp(VOTER, CHAT, CREATOR, EventResponseType::GOING, AUTHORED),
            false,
        ))
        .await;
    assert_eq!(handler.store.count().await.unwrap(), 0);
    assert_eq!(receipts_for(&handler, &["before", "event"]).await, 0);
    assert_eq!(
        handler
            .store
            .pending_event_rsvps(CHAT, None)
            .await
            .unwrap()
            .len(),
        1
    );
    while let Ok(notice) = notices.try_recv() {
        assert!(!matches!(notice, ServiceEvent::Marks { .. }));
    }
    handle_verified(
        &handler,
        &message_event(
            CHAT,
            CREATOR,
            "event",
            creation(Some(&[7; 32]), "Dinner", false),
        ),
    )
    .await;
    assert_eq!(handler.store.count().await.unwrap(), 1);
    assert_eq!(receipts_for(&handler, &["before", "event"]).await, 0);
    assert!(handler
        .store
        .pending_event_rsvps(CHAT, None)
        .await
        .unwrap()
        .is_empty());
    let event = handler.store.marks(CHAT).await.unwrap().events.remove(0);
    assert_eq!(event.responses.len(), 1);
    assert_eq!(event.responses[0].responder, VOTER);
    assert_eq!(event.responses[0].response, "going");
    assert_eq!(event.responses[0].extra_guest_count, Some(1));
    assert_eq!(event.responses[0].timestamp_ms, Some(AUTHORED));
    let mut marks = 0;
    while let Ok(notice) = notices.try_recv() {
        if matches!(notice, ServiceEvent::Marks { .. }) {
            marks += 1;
        }
    }
    assert_eq!(marks, 1);
}

#[tokio::test]
async fn history_late_key_fills_existing_public_row_and_flushes_without_rewriting_event() {
    use wa::message::event_response_message::EventResponseType;
    let (handler, _) = inbound().await;
    handle_verified(
        &handler,
        &message_event(
            CHAT,
            CREATOR,
            "event",
            creation(None, "Original dinner", false),
        ),
    )
    .await;
    handler
        .handle(&wire(
            VOTER,
            "waiting-key",
            rsvp(VOTER, CHAT, CREATOR, EventResponseType::MAYBE, AUTHORED),
            false,
        ))
        .await;
    assert_eq!(
        handler
            .store
            .pending_event_rsvps(CHAT, None)
            .await
            .unwrap()
            .len(),
        1
    );
    handler
        .handle(&history_event(vec![history_entry(
            CREATOR,
            "event",
            creation(Some(&[7; 32]), "Replay must not rename", false),
        )]))
        .await;
    let event = handler.store.marks(CHAT).await.unwrap().events.remove(0);
    assert_eq!(event.name, "Original dinner");
    assert!(event.can_respond);
    assert_eq!(event.responses.len(), 1);
    assert_eq!(event.responses[0].response, "maybe");
    assert_eq!(handler.store.count().await.unwrap(), 1);
    assert_eq!(receipts_for(&handler, &["waiting-key", "event"]).await, 0);
    assert!(handler
        .store
        .pending_event_rsvps(CHAT, None)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn history_rsvp_before_canceled_creation_preserves_attendees_without_creating_control_rows() {
    use wa::message::event_response_message::EventResponseType;
    let (handler, _) = inbound().await;
    handler
        .handle(&history_event(vec![
            history_entry(
                VOTER,
                "historical-response",
                rsvp(VOTER, CHAT, CREATOR, EventResponseType::GOING, AUTHORED),
            ),
            history_entry(
                CREATOR,
                "event",
                creation(Some(&[7; 32]), "Canceled dinner", true),
            ),
        ]))
        .await;
    let event = handler.store.marks(CHAT).await.unwrap().events.remove(0);
    assert!(event.canceled && !event.can_respond);
    assert_eq!(event.responses.len(), 1);
    assert_eq!(event.responses[0].response, "going");
    assert_eq!(event.responses[0].timestamp_ms, Some(AUTHORED));
    assert_eq!(handler.store.count().await.unwrap(), 1);
    assert_eq!(
        receipts_for(&handler, &["historical-response", "event"]).await,
        0
    );
}

#[tokio::test]
async fn authenticated_payload_cannot_cross_group_key_or_creator_hint() {
    use wa::message::event_response_message::EventResponseType;
    let (handler, _) = inbound().await;
    handle_verified(
        &handler,
        &message_event(
            CHAT,
            CREATOR,
            "event",
            creation(Some(&[7; 32]), "Dinner", false),
        ),
    )
    .await;
    for (id, key_chat, hint) in [
        ("wrong-chat", "2@g.us", CREATOR),
        ("wrong-creator", CHAT, "15550000009@s.whatsapp.net"),
    ] {
        handler
            .handle(&wire(
                VOTER,
                id,
                rsvp(VOTER, key_chat, hint, EventResponseType::GOING, AUTHORED),
                false,
            ))
            .await;
    }
    assert!(handler.store.marks(CHAT).await.unwrap().events[0]
        .responses
        .is_empty());
    assert_eq!(handler.store.count().await.unwrap(), 1);
    handler
        .handle(&wire(
            VOTER,
            "valid",
            rsvp(VOTER, CHAT, CREATOR, EventResponseType::GOING, AUTHORED + 1),
            false,
        ))
        .await;
    assert_eq!(
        handler.store.marks(CHAT).await.unwrap().events[0]
            .responses
            .len(),
        1
    );
}

#[tokio::test]
async fn raw_alternate_cannot_authenticate_as_self_without_approved_mapping() {
    use wa::message::event_response_message::EventResponseType;
    let (handler, _) = inbound().await;
    handle_verified(
        &handler,
        &message_event(
            CHAT,
            CREATOR,
            "event",
            creation(Some(&[7; 32]), "Dinner", false),
        ),
    )
    .await;
    let message = rsvp(CREATOR, CHAT, CREATOR, EventResponseType::GOING, AUTHORED);
    let changed = super::super::event_rsvps::capture_event_response(
        &handler.store,
        None,
        CHAT,
        "spoof",
        &Jid::lid("222"),
        Some(&CREATOR.parse().unwrap()),
        false,
        &[CREATOR.into()],
        message.enc_event_response_message.as_option().unwrap(),
    )
    .await
    .unwrap();
    assert!(!changed);
    assert!(handler.store.lid_pn("222").await.unwrap().is_none());
    assert!(handler.store.marks(CHAT).await.unwrap().events[0]
        .responses
        .is_empty());
    assert_eq!(handler.store.count().await.unwrap(), 1);
    assert_eq!(receipts_for(&handler, &["spoof", "event"]).await, 0);
}

#[tokio::test]
async fn authored_replay_and_self_namespace_change_keep_latest_single_own_response() {
    use wa::message::event_response_message::EventResponseType;
    let (handler, _) = inbound().await;
    handler
        .store
        .set_lid_pn("111", "15550000001")
        .await
        .unwrap();
    handle_verified(
        &handler,
        &message_event(
            CHAT,
            CREATOR,
            "event",
            creation(Some(&[7; 32]), "Dinner", false),
        ),
    )
    .await;
    handler
        .handle(&wire(
            CREATOR,
            "new-own",
            rsvp(
                CREATOR,
                CHAT,
                CREATOR,
                EventResponseType::GOING,
                AUTHORED + 2,
            ),
            true,
        ))
        .await;
    handler
        .handle(&wire(
            "111@lid",
            "old-own",
            rsvp(
                CREATOR,
                CHAT,
                CREATOR,
                EventResponseType::NOT_GOING,
                AUTHORED + 1,
            ),
            true,
        ))
        .await;
    let event = handler.store.marks(CHAT).await.unwrap().events.remove(0);
    assert_eq!(event.responses.len(), 1);
    assert_eq!(event.responses[0].responder, "@me");
    assert_eq!(event.responses[0].response, "going");
    assert_eq!(event.responses[0].timestamp_ms, Some(AUTHORED + 2));
    assert!(handler
        .store
        .pending_event_rsvps(CHAT, None)
        .await
        .unwrap()
        .is_empty());
}
