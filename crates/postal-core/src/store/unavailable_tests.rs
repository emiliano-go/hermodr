use super::*;

fn header(id: &str) -> MessageHeader {
    MessageHeader { chat: "100@s.whatsapp.net".into(), id: id.into(), sender: "100@s.whatsapp.net".into(),
        timestamp: 100, from_me: false }
}

#[test]
fn unavailable_replay_never_replaces_recovered_deleted_revoked_or_view_once_rows() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    let row = store.insert_unavailable(&header("recover")).unwrap().unwrap();
    assert!(row.is_unavailable());
    assert!(row.text.is_empty() && row.system.params.is_empty());
    assert!(store.insert_unavailable(&header("recover")).unwrap().is_none());
    let recovered = StoredMessage { header: header("recover"), text: "Real content".into(), history_shareable: true, ..Default::default() };
    store.insert_message(&recovered).unwrap();
    assert!(!store.message(&recovered.header.chat, "recover").unwrap().is_unavailable());
    assert!(store.insert_unavailable(&header("recover")).unwrap().is_none());
    assert_eq!(store.message(&recovered.header.chat, "recover").unwrap().text, "Real content");
    for id in ["deleted", "revoked", "once"] {
        let row = StoredMessage { header: header(id), text: "Kept content".into(), ..Default::default() };
        store.insert_message(&row).unwrap();
        if id == "deleted" { store.set_message_deleted(&row.header.chat, id, true).unwrap(); }
        if id == "revoked" { store.revoke_message(&row.header.chat, id).unwrap(); }
        if id == "once" { store.set_view_once(&row.header.chat, id, true).unwrap(); }
        let before = serde_json::to_value(store.message(&row.header.chat, id).unwrap()).unwrap();
        assert!(store.insert_unavailable(&header(id)).unwrap().is_none());
        assert_eq!(serde_json::to_value(store.message(&row.header.chat, id).unwrap()).unwrap(), before);
    }
    store.set_view_once(&header("bare-once").chat, "bare-once", false).unwrap();
    assert!(store.insert_unavailable(&header("bare-once")).unwrap().is_none());
}

#[test]
fn unavailable_survives_restart_and_history_heals_only_the_marker_preserving_local_state() {
    let path = std::env::temp_dir().join(format!("postal-unavailable-{}-{}.sqlite", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let mut key = header("history"); key.from_me = true;
    {
        let store = MessageStore::open(&path).unwrap();
        store.insert_unavailable(&key).unwrap().unwrap();
        store.set_delivery_state(&key.chat, &key.id, "delivered").unwrap();
    }
    let store = MessageStore::open(&path).unwrap();
    let before = store.message(&key.chat, &key.id).unwrap();
    assert!(before.is_unavailable() && before.local.read);
    let history = StoredMessage { header: key.clone(), text: "Recovered from history".into(),
        local: LocalState { read: true, status: Some("sent".into()), ..Default::default() },
        history_shareable: false, spoiler: true, ..Default::default() };
    let row = store.insert_history_row(&history).unwrap().unwrap();
    assert!(!row.is_unavailable() && row.local.read == before.local.read && row.spoiler && !row.history_shareable);
    assert_eq!(row.local.status.as_deref(), Some("delivered"));
    assert_eq!(row.local.sort_order, before.local.sort_order);
    assert_eq!(row.text, "Recovered from history");
    assert!(store.insert_history_row(&StoredMessage { text: "Stale replay".into(), ..history }).unwrap().is_none());
    assert_eq!(store.message(&key.chat, &key.id).unwrap().text, "Recovered from history");
    drop(store);
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn unavailable_rows_never_count_or_send_receipts_until_decoded_content_arrives() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    let key = header("unknown");
    store.insert_unavailable(&key).unwrap().unwrap();
    assert_eq!(store.chats().unwrap()[0].unread_count, 0);
    assert_eq!(store.chats().unwrap()[0].last_text, "Message unavailable");
    assert!(store.unread_ids(&key.chat).unwrap().is_empty());
    assert!(store.unread_until(&key.chat, &key.id).unwrap().is_empty());
    assert_eq!(store.mark_read(&key.chat).unwrap(), 0);
    assert_eq!(store.mark_read_until(&key.chat, &key.id).unwrap(), 0);
    assert!(store.unavailable_unread(&key.chat, None).unwrap());
    let decoded = StoredMessage { header: key.clone(), text: "Available now".into(), history_shareable: true, ..Default::default() };
    let (_, fresh) = store.insert_incoming_row(&decoded).unwrap();
    assert!(!fresh);
    assert_eq!(store.chats().unwrap()[0].unread_count, 1);
    assert_eq!(store.chats().unwrap()[0].last_text, "Available now");
    assert_eq!(store.unread_ids(&key.chat).unwrap(), vec![(key.id.clone(), key.sender.clone())]);
    assert!(!store.unavailable_unread(&key.chat, None).unwrap());
    assert_eq!(store.mark_read(&key.chat).unwrap(), 1);
}

#[test]
fn unavailable_respects_chat_tombstones_aliases_and_actual_read_state_from_phone() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    let key = header("aliased");
    store.set_lid_pn("200", "100").unwrap();
    let mut alias = key.clone(); alias.chat = "200@lid".into();
    let row = store.insert_unavailable(&alias).unwrap().unwrap();
    assert_eq!(row.header.chat, key.chat);
    assert!(store.insert_unavailable(&key).unwrap().is_none());
    store.mark_read_through(&key.chat, 100).unwrap();
    let decoded = StoredMessage { header: key.clone(), text: "Seen on phone".into(), ..Default::default() };
    store.insert_message(&decoded).unwrap();
    assert!(store.message(&key.chat, &key.id).unwrap().local.read);
    store.clear_chat(&key.chat).unwrap();
    assert!(store.insert_unavailable(&header("late-after-clear")).unwrap().is_none());
    store.delete_chat(&key.chat).unwrap();
    assert!(store.insert_unavailable(&header("late-after-delete")).unwrap().is_none());
}

#[test]
fn unavailable_alias_collisions_keep_decoded_content_and_monotonic_local_state_in_both_directions() {
    for marker_on_lid in [true, false] {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        let mut pn = header("collision"); pn.from_me = true;
        let mut lid = pn.clone(); lid.chat = "200@lid".into();
        let (marker, decoded) = if marker_on_lid { (&lid, &pn) } else { (&pn, &lid) };
        store.insert_unavailable(marker).unwrap().unwrap();
        store.mark_read_through(&marker.chat, 100).unwrap();
        store.set_delivery_state(&marker.chat, &marker.id, "delivered").unwrap();
        store.insert_message(&StoredMessage { header: decoded.clone(), text: "Keep decoded content".into(),
            local: LocalState { status: Some("sent".into()), ..Default::default() },
            spoiler: true, history_shareable: false, ..Default::default() }).unwrap();
        store.set_lid_pn("200", "100").unwrap();
        let row = store.message(&pn.chat, &pn.id).unwrap();
        assert_eq!(row.text, "Keep decoded content");
        assert!(!row.is_unavailable() && row.local.read && row.spoiler && !row.history_shareable);
        assert_eq!(row.local.status.as_deref(), Some("delivered"));
        assert!(store.insert_unavailable(&lid).unwrap().is_none());
    }
}

#[test]
fn unavailable_promotes_to_typed_view_once_atomically_and_late_failure_cannot_downgrade() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    let key = header("once-promotion");
    store.insert_unavailable(&key).unwrap().unwrap();
    let once = StoredMessage { header: key.clone(), media: Media { kind: Some("view_once".into()), ..Default::default() }, ..Default::default() };
    let row = store.insert_view_once_stub(&once).unwrap().unwrap();
    assert!(!row.is_unavailable());
    assert_eq!(row.media.kind.as_deref(), Some("view_once"));
    assert_eq!(store.conn.lock().unwrap().query_row("SELECT opened FROM view_once WHERE chat = ?1 AND id = ?2",
        params![key.chat, key.id], |row| row.get::<_, bool>(0)).unwrap(), false);
    assert!(store.insert_unavailable(&key).unwrap().is_none());
    assert!(store.insert_view_once_stub(&once).unwrap().is_none());
}

#[test]
fn unavailable_hidden_tombstones_survive_age_and_caps_without_visible_epoch_bubbles() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    let key = header("retired");
    store.insert_unavailable(&key).unwrap().unwrap();
    store.retire_unavailable(&key.chat, &key.id).unwrap().unwrap();
    store.revoke_message(&key.chat, "never-seen").unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    for n in 0..3 {
        let mut row_header = header(&format!("decoded-{n}")); row_header.timestamp = now + n;
        store.insert_message(&StoredMessage { header: row_header, text: "Decoded".into(), ..Default::default() }).unwrap();
    }
    let policy = DiskRetention { max_age_hours: RetentionLimit::Limited(1), max_messages_per_chat: RetentionLimit::Limited(1) };
    DiskRetentionManager::new(policy).enforce(&store).unwrap();
    assert!(store.insert_unavailable(&key).unwrap().is_none());
    assert!(store.insert_unavailable(&header("never-seen")).unwrap().is_none());
    assert!(store.message(&key.chat, "retired").is_err());
    assert!(store.message(&key.chat, "never-seen").is_err());
    let page = store.message_page(&key.chat, 100, None, MessagePageDirection::Before).unwrap();
    assert_eq!(page.messages.len(), 1);
    assert_eq!(page.messages[0].header.id, "decoded-2");
    assert_eq!(store.chats().unwrap()[0].message_count, 1);
    assert_eq!(store.oldest_message(&key.chat).unwrap().unwrap().0, "decoded-2");
}
