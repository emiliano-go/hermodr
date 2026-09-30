use super::*;

fn row(chat: &str, id: &str, timestamp: i64) -> StoredMessage {
    StoredMessage { header: MessageHeader { chat: chat.into(), id: id.into(), sender: "them".into(), timestamp,
        ..Default::default() }, text: "synthetic history".into(), ..Default::default() }
}

fn floor(store: &MessageStore, chat: &str) -> Option<i64> {
    let conn = store.conn.lock().unwrap();
    let chat = names::canonical_chat(&conn, chat).unwrap();
    conn.query_row("SELECT timestamp FROM chat_history_floor WHERE jid=?1", [chat.as_ref()], |row| row.get(0)).optional().unwrap()
}

#[test]
fn history_floor_keeps_boundary_and_newer_rows_without_disabling_other_caps() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    let retention = DiskRetentionManager::new(DiskRetention {
        max_age_hours: RetentionLimit::Unlimited, max_messages_per_chat: RetentionLimit::Limited(2),
    });
    for chat in ["requested", "ordinary"] {
        for timestamp in 99..=102 { store.insert_message(&row(chat, &timestamp.to_string(), timestamp)).unwrap(); }
    }
    assert!(store.remember_history_floor("ordinary", 0).is_err());
    assert!(store.remember_history_floor("ordinary", -1).is_err());
    store.remember_history_floor("requested", 100).unwrap();
    store.remember_history_floor("requested", 101).unwrap();
    assert_eq!(floor(&store, "requested"), Some(100));
    assert_eq!(retention.enforce_for(&store, &["requested".into()]).unwrap(), 3);
    assert!(store.message("requested", "100").is_ok());
    assert!(store.message("requested", "99").is_err());
    assert_eq!(store.messages_for("requested", 20).unwrap().len(), 3);
    assert_eq!(store.messages_for("ordinary", 20).unwrap().len(), 2);
    store.insert_message(&row("requested", "live", 103)).unwrap();
    assert_eq!(retention.enforce_for(&store, &["requested".into()]).unwrap(), 0);
    assert_eq!(store.messages_for("requested", 20).unwrap().len(), 4);
}

#[test]
fn history_floor_resets_on_clear_delete_and_retention_changes() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    for chat in ["cleared", "deleted", "changed", "other"] {
        store.insert_message(&row(chat, "history", 100)).unwrap();
        store.remember_history_floor(chat, 100).unwrap();
    }
    store.clear_chat("cleared").unwrap();
    store.delete_chat("deleted").unwrap();
    store.set_chat_retention("changed", &ChatRetention {
        max_messages: RetentionLimit::Limited(1), ..Default::default()
    }).unwrap();
    for chat in ["cleared", "deleted", "changed"] { assert_eq!(floor(&store, chat), None); }
    assert_eq!(floor(&store, "other"), Some(100));
    for timestamp in 101..=103 { store.insert_message(&row("changed", &timestamp.to_string(), timestamp)).unwrap(); }
    DiskRetentionManager::new(DiskRetention::unlimited()).enforce(&store).unwrap();
    assert_eq!(store.messages_for("changed", 20).unwrap().len(), 1);
    store.clear_history().unwrap();
    assert_eq!(floor(&store, "other"), None);
}

#[test]
fn history_floor_full_prune_respects_only_temporary_requested_chat_protection() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    let retention = DiskRetentionManager::new(DiskRetention {
        max_age_hours: RetentionLimit::Limited(24), max_messages_per_chat: RetentionLimit::Limited(0),
    });
    for chat in ["requested", "ordinary"] { store.insert_message(&row(chat, "history", 100)).unwrap(); }
    assert_eq!(retention.enforce_protected(&store, &["requested".into()]).unwrap(), 1);
    assert!(store.message("requested", "history").is_ok());
    assert!(store.message("ordinary", "history").is_err());
    assert_eq!(floor(&store, "requested"), None);
    assert_eq!(retention.enforce(&store).unwrap(), 1);
    assert!(store.message("requested", "history").is_err());
}

#[test]
fn history_floor_survives_reopen_migration_replay_and_alias_collision() {
    let path = std::env::temp_dir().join(format!("postal-history-floor-{}-{}.db", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    {
        let store = MessageStore::open(&path).unwrap();
        store.remember_history_floor("9@lid", 100).unwrap();
        store.remember_history_floor("1@s.whatsapp.net", 200).unwrap();
        store.set_lid_pn("9", "1").unwrap();
        assert_eq!(floor(&store, "1@s.whatsapp.net"), Some(100));
        assert_eq!(floor(&store, "9@lid"), Some(100));
        retention::migrate_history_floor(&store.conn.lock().unwrap()).unwrap();
    }
    let store = MessageStore::open(&path).unwrap();
    assert_eq!(floor(&store, "1@s.whatsapp.net"), Some(100));
    store.clear_chat("9@lid").unwrap();
    assert_eq!(floor(&store, "1@s.whatsapp.net"), None);
    drop(store);
    for suffix in ["", "-wal", "-shm", "-journal"] { let _ = std::fs::remove_file(format!("{}{suffix}", path.display())); }
}
