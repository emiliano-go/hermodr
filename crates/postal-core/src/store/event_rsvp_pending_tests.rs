use super::*;

const CHAT: &str = "1@g.us";
fn store() -> MessageStore {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    migrate(&store.conn.lock().unwrap()).unwrap();
    store
}
fn record(source: &str) -> PendingEventRsvp {
    PendingEventRsvp {
        event_id: "event".into(),
        source_id: source.into(),
        responder: "15550000002@s.whatsapp.net".into(),
        responder_alt: None,
        from_me: false,
        key_chat: Some(CHAT.into()),
        creator_hint: Some("15550000001@s.whatsapp.net".into()),
        key_from_me: Some(false),
        payload: vec![7; 32],
        iv: vec![8; 12],
        received_at: whatsapp_rust::wacore::time::now_millis(),
    }
}
fn parent(store: &MessageStore) {
    let mut row = StoredMessage::default();
    row.header.chat = CHAT.into();
    row.header.id = "event".into();
    row.header.timestamp = 1;
    row.header.sender = "15550000001@s.whatsapp.net".into();
    row.media.kind = Some("event".into());
    row.text = "Dinner".into();
    store.insert_message(&row).unwrap();
}

#[test]
fn pending_cipher_deduplicates_without_creating_message_or_receipt_rows() {
    let store = store();
    let pending = record("source");
    assert!(store.queue_event_rsvp(CHAT, &pending).unwrap());
    assert!(!store.queue_event_rsvp(CHAT, &pending).unwrap());
    assert_eq!(store.pending_event_rsvps(CHAT, None).unwrap().len(), 1);
    assert_eq!(store.count().unwrap(), 0);
    assert_eq!(
        store
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT count(*) FROM receipts", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    store
        .remove_pending_event_rsvp(CHAT, "event", "source")
        .unwrap();
    assert!(store.pending_event_rsvps(CHAT, None).unwrap().is_empty());
}

#[test]
fn opaque_future_parent_remains_pending_even_when_chat_has_history_floor() {
    let store = store();
    store
        .conn
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO chat_history_floor(jid,timestamp) VALUES(?1,100)",
            [CHAT],
        )
        .unwrap();
    assert!(store.queue_event_rsvp(CHAT, &record("future")).unwrap());
    assert_eq!(store.pending_event_rsvps(CHAT, None).unwrap().len(), 1);
}

#[test]
fn known_private_target_or_source_rejects_and_purges_ciphertext() {
    for target in [true, false] {
        for privacy in [
            "deleted=1",
            "revoked=1",
            "spoiler=1",
            "media_once_kind='event'",
            "media_kind='view_once'",
            "system_kind='OTHER_CONTROL'",
        ] {
            let store = store();
            parent(&store);
            let pending = record("source");
            assert!(store.queue_event_rsvp(CHAT, &pending).unwrap());
            let id = if target { "event" } else { "source" };
            if !target {
                let mut row = StoredMessage::default();
                row.header.chat = CHAT.into();
                row.header.id = id.into();
                row.header.timestamp = 2;
                row.text = "Control".into();
                store.insert_message(&row).unwrap();
            }
            store
                .conn
                .lock()
                .unwrap()
                .execute(
                    &format!("UPDATE messages SET {privacy} WHERE chat=?1 AND id=?2"),
                    params![CHAT, id],
                )
                .unwrap();
            assert!(store.pending_event_rsvps(CHAT, None).unwrap().is_empty());
            assert!(!store.queue_event_rsvp(CHAT, &pending).unwrap());
        }
    }
}

#[test]
fn clear_hidden_and_view_once_guards_allow_new_public_parent_after_revive() {
    let store = store();
    parent(&store);
    store.clear_chat(CHAT).unwrap();
    assert!(!store.queue_event_rsvp(CHAT, &record("cleared")).unwrap());
    parent(&store);
    assert!(store.queue_event_rsvp(CHAT, &record("new-public")).unwrap());
    store.delete_chat(CHAT).unwrap();
    assert!(!store.queue_event_rsvp(CHAT, &record("hidden")).unwrap());
    parent(&store);
    store
        .conn
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO view_once(chat,id,opened) VALUES(?1,'event',0)",
            [CHAT],
        )
        .unwrap();
    assert!(!store.queue_event_rsvp(CHAT, &record("once")).unwrap());
    assert!(store.pending_event_rsvps(CHAT, None).unwrap().is_empty());
}

#[test]
fn queue_is_globally_bounded_and_rejects_definitely_invalid_cipher() {
    let store = store();
    for index in 0..520 {
        assert!(store
            .queue_event_rsvp(CHAT, &record(&format!("source-{index}")))
            .unwrap());
    }
    assert_eq!(store.pending_event_rsvps(CHAT, None).unwrap().len(), 512);
    assert!(store
        .pending_event_rsvps(CHAT, None)
        .unwrap()
        .iter()
        .all(|record| record.source_id != "source-0"));
    let mut invalid = record("bad");
    invalid.payload = vec![0; 4097];
    assert!(!store.queue_event_rsvp(CHAT, &invalid).unwrap());
    invalid = record("bad");
    invalid.iv.clear();
    assert!(!store.queue_event_rsvp(CHAT, &invalid).unwrap());
}

#[test]
fn pending_cipher_survives_multi_day_restart_without_invented_expiry() {
    let path = std::env::temp_dir().join(format!(
        "postal-rsvp-pending-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    {
        let store = MessageStore::open(&path).unwrap();
        migrate(&store.conn.lock().unwrap()).unwrap();
        let mut pending = record("durable");
        pending.received_at -= 3 * 86_400_000;
        assert!(store.queue_event_rsvp(CHAT, &pending).unwrap());
    }
    let store = MessageStore::open(&path).unwrap();
    let loaded = store.pending_event_rsvps(CHAT, None).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].source_id, "durable");
    assert_eq!(loaded[0].payload, vec![7; 32]);
    drop(store);
    std::fs::remove_file(path).unwrap();
}
