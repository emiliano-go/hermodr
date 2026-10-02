use super::*;

const CHAT: &str = "12345@broadcast";
const PN: &str = "15550000001@s.whatsapp.net";
const LID: &str = "777@lid";

fn store() -> MessageStore {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    migrate(&store.conn.lock().unwrap()).unwrap();
    store
}

fn source(store: &MessageStore, id: &str, timestamp: i64) {
    let mut row = StoredMessage::default();
    row.header.chat = CHAT.into();
    row.header.id = id.into();
    row.header.sender = PN.into();
    row.header.timestamp = timestamp;
    row.text = "Received broadcast".into();
    store.insert_message(&row).unwrap();
}

#[test]
fn recipients_are_normalized_without_replacing_jids_with_names() {
    let store = store();
    source(&store, "received", 100);
    store.set_lid_pn("777", "15550000001").unwrap();
    store.set_saved_name(PN, "Saved contact").unwrap();
    assert!(store
        .remember_broadcast_list(
            CHAT,
            "received",
            &["15550000001:2@s.whatsapp.net".into(), PN.into(), LID.into()]
        )
        .unwrap());
    let snapshot = store.broadcast_list(CHAT).unwrap().unwrap();
    assert_eq!(snapshot.chat, CHAT);
    assert_eq!(snapshot.source_timestamp, 100);
    assert_eq!(snapshot.recipients, vec![PN, LID]);
}

#[test]
fn only_newer_supplied_recipient_snapshot_replaces_cached_evidence() {
    let store = store();
    for (id, time) in [
        ("old", 100),
        ("latest", 200),
        ("tie", 200),
        ("missing", 300),
        ("undated", 0),
    ] {
        source(&store, id, time);
    }
    assert!(store
        .remember_broadcast_list(CHAT, "latest", &[LID.into()])
        .unwrap());
    assert!(!store
        .remember_broadcast_list(CHAT, "old", &[PN.into()])
        .unwrap());
    assert!(!store
        .remember_broadcast_list(CHAT, "tie", &[PN.into()])
        .unwrap());
    assert!(!store
        .remember_broadcast_list(CHAT, "latest", &[LID.into()])
        .unwrap());
    assert!(!store.remember_broadcast_list(CHAT, "missing", &[]).unwrap());
    assert!(!store
        .remember_broadcast_list(CHAT, "undated", &[PN.into()])
        .unwrap());
    assert_eq!(
        store.broadcast_list(CHAT).unwrap().unwrap().recipients,
        vec![LID]
    );
    source(&store, "new", 400);
    assert!(store
        .remember_broadcast_list(CHAT, "new", &[PN.into()])
        .unwrap());
    assert_eq!(
        store
            .broadcast_list(CHAT)
            .unwrap()
            .unwrap()
            .source_timestamp,
        400
    );
}

#[test]
fn malformed_status_and_unsupplied_recipients_never_create_metadata() {
    let store = store();
    source(&store, "received", 100);
    for chat in ["status@broadcast", "1@g.us", PN, "bad"] {
        assert!(!store
            .remember_broadcast_list(chat, "received", &[PN.into()])
            .unwrap());
        assert!(store.broadcast_list(chat).unwrap().is_none());
    }
    for recipients in [
        vec![],
        vec!["bad".into()],
        vec!["1@g.us".into()],
        vec![PN.into(), "status@broadcast".into()],
    ] {
        assert!(!store
            .remember_broadcast_list(CHAT, "received", &recipients)
            .unwrap());
    }
    assert!(!store
        .remember_broadcast_list(CHAT, "unadmitted", &[PN.into()])
        .unwrap());
    assert!(store.broadcast_list(CHAT).unwrap().is_none());
}

#[test]
fn private_source_is_rejected_and_cached_metadata_becomes_unavailable() {
    let store = store();
    for (index, privacy) in [
        "deleted=1",
        "revoked=1",
        "spoiler=1",
        "media_kind='view_once'",
        "media_once_kind='image'",
        "system_kind='UNAVAILABLE_MESSAGE'",
        "system_kind='OTHER_CONTROL'",
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("source-{index}");
        source(&store, &id, 100 + index as i64);
        assert!(store
            .remember_broadcast_list(CHAT, &id, &[PN.into()])
            .unwrap());
        store
            .conn
            .lock()
            .unwrap()
            .execute(
                &format!("UPDATE messages SET {privacy} WHERE chat=?1 AND id=?2"),
                params![CHAT, id],
            )
            .unwrap();
        assert!(store.broadcast_list(CHAT).unwrap().is_none());
        assert!(!store
            .remember_broadcast_list(CHAT, &id, &[LID.into()])
            .unwrap());
    }
}

#[test]
fn hidden_or_missing_source_cannot_expose_or_refresh_metadata() {
    let store = store();
    source(&store, "received", 100);
    source(&store, "new", 200);
    assert!(store
        .remember_broadcast_list(CHAT, "received", &[PN.into()])
        .unwrap());
    store
        .conn
        .lock()
        .unwrap()
        .execute("INSERT INTO hidden_chats(jid) VALUES(?1)", [CHAT])
        .unwrap();
    assert!(store.broadcast_list(CHAT).unwrap().is_none());
    assert!(!store
        .remember_broadcast_list(CHAT, "new", &[LID.into()])
        .unwrap());
    store
        .conn
        .lock()
        .unwrap()
        .execute("DELETE FROM hidden_chats WHERE jid=?1", [CHAT])
        .unwrap();
    store
        .conn
        .lock()
        .unwrap()
        .execute(
            "DELETE FROM messages WHERE chat=?1 AND id='received'",
            [CHAT],
        )
        .unwrap();
    assert!(store.broadcast_list(CHAT).unwrap().is_none());
}

fn cached_count(store: &MessageStore) -> i64 {
    store
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT count(*) FROM broadcast_lists", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn chat_clear_delete_and_global_clear_remove_recipient_evidence() {
    for action in 0..3 {
        let store = store();
        source(&store, "received", 100);
        assert!(store
            .remember_broadcast_list(CHAT, "received", &[PN.into()])
            .unwrap());
        match action {
            0 => {
                store.clear_chat(CHAT).unwrap();
            }
            1 => {
                store.delete_chat(CHAT).unwrap();
            }
            _ => {
                store.clear_history().unwrap();
            }
        }
        assert_eq!(cached_count(&store), 0);
        assert!(store.broadcast_list(CHAT).unwrap().is_none());
    }
}

#[test]
fn unlimited_retention_still_purges_private_hidden_and_missing_sources() {
    for state in [
        "deleted=1",
        "revoked=1",
        "spoiler=1",
        "media_kind='view_once'",
        "media_once_kind='image'",
        "system_kind='UNAVAILABLE_MESSAGE'",
        "system_kind='OTHER_CONTROL'",
        "hidden",
        "missing",
    ] {
        let store = store();
        source(&store, "received", 100);
        assert!(store
            .remember_broadcast_list(CHAT, "received", &[PN.into()])
            .unwrap());
        let conn = store.conn.lock().unwrap();
        match state {
            "hidden" => {
                conn.execute("INSERT INTO hidden_chats(jid) VALUES(?1)", [CHAT])
                    .unwrap();
            }
            "missing" => {
                conn.execute("DELETE FROM messages WHERE chat=?1", [CHAT])
                    .unwrap();
            }
            _ => {
                conn.execute(
                    &format!("UPDATE messages SET {state} WHERE chat=?1"),
                    [CHAT],
                )
                .unwrap();
            }
        }
        drop(conn);
        DiskRetentionManager::new(DiskRetention::unlimited())
            .enforce(&store)
            .unwrap();
        assert_eq!(cached_count(&store), 0);
    }
}

#[test]
fn retention_cap_removes_snapshot_when_its_source_is_pruned() {
    let store = store();
    source(&store, "received", 100);
    assert!(store
        .remember_broadcast_list(CHAT, "received", &[PN.into()])
        .unwrap());
    source(&store, "other", 200);
    let manager = DiskRetentionManager::new(DiskRetention {
        max_messages_per_chat: RetentionLimit::Limited(1),
        ..DiskRetention::unlimited()
    });
    assert_eq!(manager.enforce(&store).unwrap(), 1);
    assert_eq!(cached_count(&store), 0);
}
