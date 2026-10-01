use super::*;

fn store() -> MessageStore { MessageStore::open(Path::new(":memory:")).unwrap() }

#[test]
fn label_deltas_preserve_fields_reject_stale_updates_and_replay_idempotently() {
    let store = store();
    assert!(!store.set_label("opaque-id", None, None, None, 1).unwrap());
    store.set_label("opaque-id", Some("First"), Some(i32::MIN), Some(false), 10).unwrap();
    store.set_label("opaque-id", None, Some(i32::MAX), None, 20).unwrap();
    store.set_label("opaque-id", Some("Renamed"), None, None, 30).unwrap();
    assert!(!store.set_label("opaque-id", Some("Renamed"), None, None, 30).unwrap());
    assert!(!store.set_label("opaque-id", Some("Stale"), Some(0), Some(true), 29).unwrap());
    store.set_label("other", Some("Alpha"), Some(-1), Some(false), 10).unwrap();
    let view = store.labels_view().unwrap();
    assert!(!view.complete);
    assert_eq!(view.labels, vec![
        Label { id: "other".into(), name: "Alpha".into(), color: -1 },
        Label { id: "opaque-id".into(), name: "Renamed".into(), color: i32::MAX },
    ]);
}

#[test]
fn chat_and_message_associations_stay_distinct_and_replay_removals() {
    let store = store();
    store.set_label("a", Some("A"), None, Some(false), 1).unwrap();
    store.set_label("b", Some("B"), None, Some(false), 1).unwrap();
    store.set_chat_label("a", "1@g.us", true, 10).unwrap();
    store.set_message_label("a", "1@g.us", "same", true, 10).unwrap();
    store.set_message_label("a", "2@g.us", "same", true, 10).unwrap();
    store.set_message_label("b", "1@g.us", "same", true, 10).unwrap();
    assert!(!store.set_message_label("a", "1@g.us", "same", true, 10).unwrap());
    store.set_message_label("a", "1@g.us", "same", false, 20).unwrap();
    assert!(!store.set_message_label("a", "1@g.us", "same", true, 19).unwrap());
    let view = store.labels_view().unwrap();
    assert_eq!(view.chats, vec![ChatLabelAssociation { label_id: "a".into(), chat: "1@g.us".into() }]);
    assert_eq!(view.messages, vec![
        MessageLabelAssociation { label_id: "b".into(), chat: "1@g.us".into(), message_id: "same".into() },
        MessageLabelAssociation { label_id: "a".into(), chat: "2@g.us".into(), message_id: "same".into() },
    ]);
    store.set_chat_label("a", "1@g.us", false, 20).unwrap();
    assert!(!store.set_chat_label("a", "1@g.us", true, 19).unwrap());
    assert!(store.labels_view().unwrap().chats.is_empty());
    assert_eq!(store.labels_view().unwrap().messages.len(), 2);
}

#[test]
fn label_tombstones_block_stale_resurrection_and_clear_old_associations() {
    let store = store();
    store.set_chat_label("a", "1@g.us", true, 10).unwrap();
    store.set_message_label("a", "1@g.us", "m", true, 10).unwrap();
    store.set_label("a", Some("A"), Some(1), Some(false), 10).unwrap();
    store.set_label("a", None, None, Some(true), 20).unwrap();
    assert!(store.label_id_known("a").unwrap());
    assert!(!store.label_exists("a").unwrap());
    for timestamp in [10, 19] {
        assert!(!store.set_label("a", Some("Old"), Some(2), Some(false), timestamp).unwrap());
        assert!(!store.set_chat_label("a", "1@g.us", true, timestamp).unwrap());
        assert!(!store.set_message_label("a", "1@g.us", "m", true, timestamp).unwrap());
    }
    assert!(store.labels_view().unwrap().labels.is_empty());
    store.set_label("a", Some("New"), None, Some(false), 30).unwrap();
    let view = store.labels_view().unwrap();
    assert_eq!(view.labels[0].name, "New");
    assert!(view.chats.is_empty() && view.messages.is_empty());
    store.set_label("never-seen", None, None, Some(true), 40).unwrap();
    assert!(store.label_id_known("never-seen").unwrap());
    assert!(!store.set_label("never-seen", Some("Old"), None, Some(false), 30).unwrap());
}

#[test]
fn associations_can_arrive_before_catalog_without_losing_removal_tombstones() {
    let store = store();
    store.set_chat_label("late", "1@g.us", false, 20).unwrap();
    assert!(!store.set_chat_label("late", "1@g.us", true, 10).unwrap());
    store.set_message_label("late", "1@g.us", "kept", true, 10).unwrap();
    assert!(store.labels_view().unwrap().messages.is_empty());
    store.set_label("late", Some("Late"), None, Some(false), 1).unwrap();
    let view = store.labels_view().unwrap();
    assert!(view.chats.is_empty());
    assert_eq!(view.messages.len(), 1);
}

#[test]
fn accepted_chat_labels_discover_empty_chats_without_messages_or_unhiding() {
    let store = store();
    let chat = "1@g.us";
    store.set_label("a", Some("A"), None, Some(false), 1).unwrap();
    assert!(store.set_chat_label("a", chat, true, 10).unwrap());
    let chats = store.chats().unwrap();
    assert_eq!(chats.len(), 1);
    let empty = &chats[0];
    assert_eq!(empty.chat, chat);
    assert_eq!((empty.last_message_at, empty.message_count, empty.unread_count, empty.mention_count), (0, 0, 0, 0));
    assert!(empty.last_text.is_empty() && empty.last_sender.is_empty());
    assert!(!empty.last_from_me && !empty.marked_unread && !empty.pinned && !empty.archived);
    assert!(store.messages_for(chat, 10).unwrap().is_empty());
    store.delete_chat(chat).unwrap();
    assert!(!store.set_chat_label("a", chat, true, 9).unwrap());
    assert!(!store.set_chat_label("a", chat, true, 10).unwrap());
    assert_eq!(store.conn.lock().unwrap().query_row("SELECT COUNT(*) FROM chats", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    assert!(store.set_chat_label("a", chat, true, 11).unwrap());
    assert!(store.chats().unwrap().is_empty());
    assert_eq!(store.conn.lock().unwrap().query_row("SELECT COUNT(*) FROM hidden_chats WHERE jid = ?1", [chat], |r| r.get::<_, i64>(0)).unwrap(), 1);
    store.set_chat_label("a", "removed@g.us", false, 30).unwrap();
    assert!(!store.set_chat_label("a", "removed@g.us", true, 29).unwrap());
    assert!(!store.set_chat_label("a", "removed@g.us", false, 30).unwrap());
    store.set_label("deleted", None, None, Some(true), 20).unwrap();
    store.set_chat_label("deleted", "deleted@g.us", true, 19).unwrap();
    assert!(!store.set_chat_label("deleted", "deleted@g.us", true, 20).unwrap());
    store.set_message_label("a", "message-only@g.us", "m", true, 30).unwrap();
    let conn = store.conn.lock().unwrap();
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM chats", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM chat_state", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn empty_chat_discovery_rolls_back_association_when_metadata_write_fails() {
    let store = store();
    store.conn.lock().unwrap().execute_batch(
        "CREATE TRIGGER fail_label_discovery BEFORE INSERT ON chats BEGIN SELECT RAISE(ABORT, 'synthetic metadata failure'); END;"
    ).unwrap();
    assert!(store.set_chat_label("a", "1@g.us", true, 10).is_err());
    let conn = store.conn.lock().unwrap();
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM chat_labels", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM chats", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn pn_lid_merge_keeps_latest_removals_and_message_keys() {
    let store = store();
    let (lid, pn) = ("123@lid", "5989@s.whatsapp.net");
    store.set_label("a", Some("A"), None, Some(false), 1).unwrap();
    store.set_chat_label("a", pn, true, 10).unwrap();
    store.set_chat_label("a", lid, false, 20).unwrap();
    store.set_message_label("a", pn, "tie", true, 20).unwrap();
    store.set_message_label("a", lid, "tie", false, 20).unwrap();
    store.set_message_label("a", pn, "kept", true, 30).unwrap();
    store.set_message_label("a", lid, "kept", false, 20).unwrap();
    store.set_message_label("a", lid, "other", true, 30).unwrap();
    store.set_lid_pn("123", "5989").unwrap();
    assert!(!store.set_chat_label("a", lid, true, 10).unwrap());
    let view = store.labels_view().unwrap();
    assert!(view.chats.is_empty());
    assert_eq!(view.messages.iter().map(|a| (&*a.chat, &*a.message_id)).collect::<Vec<_>>(),
        vec![(pn, "kept"), (pn, "other")]);
    let conn = store.conn.lock().unwrap();
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM message_labels WHERE chat = ?1", [lid], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn labels_are_account_local_and_hidden_chats_stay_hidden() {
    let first = store();
    let second = store();
    first.set_label("a", Some("Account A"), None, Some(false), 1).unwrap();
    second.set_label("a", Some("Account B"), None, Some(false), 1).unwrap();
    first.set_chat_label("a", "1@g.us", true, 10).unwrap();
    first.set_message_label("a", "1@g.us", "m", true, 10).unwrap();
    first.delete_chat("1@g.us").unwrap();
    let view = first.labels_view().unwrap();
    assert!(view.chats.is_empty() && view.messages.is_empty());
    assert_eq!(view.labels[0].name, "Account A");
    assert_eq!(second.labels_view().unwrap().labels[0].name, "Account B");
    assert!(second.labels_view().unwrap().chats.is_empty());
    assert!(first.set_label("", Some("Bad"), None, None, 1).is_err());
    assert!(first.set_label("bad", Some("Bad"), None, None, -1).is_err());
    assert!(first.set_chat_label("", "1@g.us", true, 1).is_err());
    assert!(first.set_message_label("a", "1@g.us", "", true, 1).is_err());
}

#[test]
fn unversioned_schema_adoption_preserves_label_rows_and_tombstones() {
    let store = store();
    store.set_label("active", Some("Active"), Some(-1), Some(false), 10).unwrap();
    store.set_label("deleted", Some("Deleted"), Some(2), Some(true), 20).unwrap();
    store.set_chat_label("active", "1@g.us", true, 10).unwrap();
    store.set_message_label("active", "1@g.us", "kept", true, 10).unwrap();
    store.set_message_label("active", "1@g.us", "removed", false, 20).unwrap();
    let before = store.labels_view().unwrap();
    {
        let conn = store.conn.lock().unwrap();
        conn.pragma_update(None, "user_version", 0).unwrap();
        schema::migrate(&conn).unwrap();
        assert_eq!(conn.query_row("SELECT labeled, updated_at FROM message_labels WHERE message_id = 'removed'", [],
            |r| Ok((r.get::<_, bool>(0)?, r.get::<_, i64>(1)?))).unwrap(), (false, 20));
    }
    assert_eq!(store.labels_view().unwrap(), before);
    assert!(store.label_id_known("deleted").unwrap());
    assert!(!store.label_exists("deleted").unwrap());
    assert!(!store.set_message_label("active", "1@g.us", "removed", true, 19).unwrap());
}

#[test]
fn labelled_message_query_filters_before_limit_and_keeps_literal_text_and_urls() {
    let store = store();
    for label in ["a", "b", "deleted"] { store.set_label(label, Some(label), None, Some(false), 1).unwrap(); }
    let chat = "5989@s.whatsapp.net";
    let mut safe = StoredMessage::default();
    safe.header.chat = chat.into();
    safe.header.id = "safe".into();
    safe.header.timestamp = 1;
    safe.text = "Needle 100%_\\".into();
    safe.link.url = Some("https://needle.synthetic.test/literal".into());
    store.insert_message(&safe).unwrap();
    for label in ["a", "b"] { store.set_message_label(label, chat, "safe", true, 10).unwrap(); }
    for index in 0..501 {
        let mut newer = safe.clone();
        newer.header.id = format!("unlabeled-{index}");
        newer.header.timestamp = 100 + index;
        store.insert_message(&newer).unwrap();
    }
    for field in 0..13 {
        let mut excluded = safe.clone();
        excluded.header.id = format!("excluded-{field}");
        match field {
            0 => excluded.local.deleted = true,
            1 => excluded.local.revoked = true,
            2 => excluded.spoiler = true,
            3 => excluded.system.kind = Some("UNAVAILABLE_MESSAGE".into()),
            4 => excluded.system.kind = Some("GROUP_CHANGE".into()),
            5 => excluded.media.once_kind = Some("image".into()),
            6 => excluded.media.kind = Some("view_once".into()),
            7 => excluded.media.kind = Some("unknown".into()),
            9 => excluded.header.chat = "hidden@g.us".into(),
            _ => (),
        }
        store.insert_message(&excluded).unwrap();
        if field == 12 { store.set_chat_label("a", chat, true, 10).unwrap(); }
        else { store.set_message_label(if field == 10 { "deleted" } else { "a" }, &excluded.header.chat,
            &excluded.header.id, field != 11, 10).unwrap(); }
        if field == 8 { store.set_view_once(chat, &excluded.header.id, false).unwrap(); }
    }
    store.conn.lock().unwrap().execute("INSERT INTO hidden_chats VALUES ('hidden@g.us')", []).unwrap();
    store.set_label("deleted", None, None, Some(true), 20).unwrap();
    let ids = vec!["a".into(), "b".into(), "deleted".into()];
    let found = store.labelled_messages(&ids, None, "needle", 500).unwrap();
    assert_eq!(found.iter().map(|m| &*m.header.id).collect::<Vec<_>>(), vec!["safe"]);
    assert_eq!(store.labelled_messages(&ids, Some(chat), "100%_\\", 500).unwrap().len(), 1);
    assert_eq!(store.labelled_messages(&ids, None, "needle.synthetic.test", 500).unwrap().len(), 1);
    let mut other = safe.clone();
    other.header.chat = "2@g.us".into();
    store.insert_message(&other).unwrap();
    store.set_message_label("a", "2@g.us", "safe", true, 10).unwrap();
    assert_eq!(store.labelled_messages(&ids, Some(chat), "needle", 500).unwrap().len(), 1);
    assert_eq!(store.labelled_messages(&ids, None, "needle", 500).unwrap().len(), 2);
    store.set_lid_pn("123", "5989").unwrap();
    assert_eq!(store.labelled_messages(&ids, Some("123@lid"), "needle", 500).unwrap()[0].header.chat, chat);
    assert_eq!(store.labelled_messages(&ids, None, "", 0).unwrap().len(), 1);
    assert!(store.labelled_messages(&[], None, "", 500).is_err());
    assert!(store.labelled_messages(&["".into()], None, "", 500).is_err());
    assert!(store.labelled_messages(&vec!["a".into(); 51], None, "", 500).is_err());
    for index in 0..501 {
        store.set_message_label("a", chat, &format!("unlabeled-{index}"), true, 10).unwrap();
    }
    assert_eq!(store.labelled_messages(&ids, None, "", 1000).unwrap().len(), 500);
}
