use super::*;

#[test]
fn spoiler_edit_updates_flags_and_text_together() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    let mut message = StoredMessage::default();
    message.header.chat = "spoiler@g.us".into();
    message.header.id = "edit".into();
    message.text = "plain".into();
    store.insert_message(&message).unwrap();
    store.update_message_spoiler(&message.header.chat, "edit", "hidden", true).unwrap();
    let updated = store.message(&message.header.chat, "edit").unwrap();
    assert!(updated.spoiler);
    assert_eq!(updated.text, "hidden");
    store.update_message_content(&message.header.chat, "edit", "still hidden").unwrap();
    assert!(store.message(&message.header.chat, "edit").unwrap().spoiler);
}

#[test]
fn optional_rows_do_not_hide_database_failures() {
    let reads: &[(&str, fn(&MessageStore) -> Result<()>)] = &[
        ("names", |s| s.name_for("absent").map(|_| ())),
        ("names", |s| s.name_is_saved("absent").map(|_| ())),
        ("lid_pn", |s| s.lid_pn("absent").map(|_| ())),
        ("chat_settings", |s| s.chat_auto_download("absent").map(|_| ())),
        ("chat_privacy", |s| s.chat_privacy("absent").map(|_| ())),
        ("chat_retention", |s| s.chat_retention("absent").map(|_| ())),
        ("messages", |s| s.oldest_message("absent").map(|_| ())),
        ("messages", |s| s.chat_of_message("absent").map(|_| ())),
        ("messages", |s| s.set_delivery_state("chat", "absent", "read").map(|_| ())),
        ("messages", |s| s.media_ref_for("chat", "absent").map(|_| ())),
        ("messages", |s| s.quote_media_path("chat", "absent").map(|_| ())),
        ("messages", |s| s.view_once_copy("chat", "absent").map(|_| ())),
        ("messages", |s| s.quote_source_for("absent").map(|_| ())),
        ("view_once", |s| s.is_view_once("chat", "absent").map(|_| ())),
        ("polls", |s| s.poll_secret("chat", "absent").map(|_| ())),
        ("events", |s| s.event_secret("chat", "absent").map(|_| ())),
        ("message_pins", |s| s.marks("absent").map(|_| ())),
    ];
    for (table, read) in reads {
        let s = store(DiskRetention::unlimited());
        assert!(read(&s).is_ok(), "{table}: absent row is valid");
        s.conn.lock().unwrap().execute_batch(&format!("DROP TABLE {table}")).unwrap();
        assert!(read(&s).is_err(), "{table}: database failure must propagate");
    }
}

#[test]
fn address_reconciliation_uses_indexed_quote_lookup() {
    let store = store(DiskRetention::unlimited());
    let conn = store.conn.lock().unwrap();
    let mut query = conn.prepare("EXPLAIN QUERY PLAN UPDATE messages SET reply_to_chat = ?1
        WHERE reply_to_chat = ?2").unwrap();
    let plan = query.query_map(["1@s.whatsapp.net", "1@lid"], |row| row.get::<_, String>(3))
        .unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
    assert!(plan.iter().any(|step| step.contains("idx_messages_reply_chat")), "{plan:?}");
}

#[test]
fn retention_handles_a_large_backlog_across_many_chats() {
    let store = store(DiskRetention {
        max_age_hours: RetentionLimit::Limited(24), max_messages_per_chat: RetentionLimit::Limited(19_999_999),
    });
    {
        let _batch = store.batch();
        let conn = store.conn.lock().unwrap();
        let mut insert = conn.prepare("INSERT INTO messages (chat, id, sender, timestamp, from_me, text)
            VALUES (?1, ?2, 'them', ?3, 0, 'old message')").unwrap();
        for chat in 0..1000 {
            let chat = format!("{chat}@s.whatsapp.net");
            for id in 0..150 {
                insert.execute(params![chat, id.to_string(), id]).unwrap();
            }
        }
    }
    let started = std::time::Instant::now();
    let removed = store.enforce_retention_for(&["0@s.whatsapp.net".into()]).unwrap();
    eprintln!("retention: {removed} rows in {:?}", started.elapsed());
    assert_eq!(removed, 150_000);
    assert_eq!(store.chats().unwrap().len(), 1000);
}

#[test]
fn new_mappings_merge_immediately_and_survive_reopen() {
    let path = std::env::temp_dir().join(format!(
        "postal-schema-reopen-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
    ));
    {
        let store = MessageStore::open(&path).unwrap();
        store.insert_message(&msg("123@lid", "kept", 0, "hello")).unwrap();
        store.set_lid_pn("123", "5989").unwrap();
        assert_eq!(store.messages_for("123@lid", 10).unwrap()[0].header.chat, "5989@s.whatsapp.net");
        assert_eq!(store.messages_for("5989@s.whatsapp.net", 10).unwrap().len(), 1);
    }
    {
        let store = MessageStore::open(&path).unwrap();
        let conn = store.conn.lock().unwrap();
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(version, schema::MIGRATIONS.len() as i64);
        drop(conn);
        let messages = store.messages_for("5989@s.whatsapp.net", 10).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "hello");
        assert_eq!(store.messages_for("123@lid", 10).unwrap()[0].header.chat, "5989@s.whatsapp.net");
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn late_mapping_merges_local_state_and_rolls_back_on_failure() {
    let s = store(DiskRetention::unlimited());
    let lid = "123@lid";
    let pn = "5989@s.whatsapp.net";
    let mut rich = msg(lid, "same", 0, "caption");
    rich.local.read = true;
    rich.local.status = Some("read".into());
    rich.media.path = Some("synthetic.jpg".into());
    s.insert_message(&rich).unwrap();
    s.insert_message(&msg(pn, "same", 0, "caption")).unwrap();
    s.set_starred(lid, "same", true).unwrap();
    s.set_pinned(lid, true).unwrap();
    s.conn.lock().unwrap().execute_batch(
        "CREATE TRIGGER fail_merge BEFORE UPDATE OF chat ON messages BEGIN SELECT RAISE(ABORT, 'synthetic merge failure'); END;"
    ).unwrap();
    assert!(s.set_lid_pn("123", "5989").is_err());
    assert!(s.lid_pn("123").unwrap().is_none());
    assert_eq!(s.chats().unwrap().len(), 2);
    s.conn.lock().unwrap().execute_batch("DROP TRIGGER fail_merge").unwrap();
    let batch = s.batch();
    s.set_lid_pn("123", "5989").unwrap();
    drop(batch);
    assert_eq!(s.chats().unwrap().len(), 1);
    let merged = s.message(pn, "same").unwrap();
    assert!(merged.local.read);
    assert_eq!(merged.local.status.as_deref(), Some("read"));
    assert_eq!(merged.media.path.as_deref(), Some("synthetic.jpg"));
    assert_eq!(s.marks(pn).unwrap().starred, ["same"]);
    assert_eq!(s.pinned_chats().unwrap(), [pn]);
    s.set_lid_pn("123", "5989").unwrap();
    assert_eq!(s.count().unwrap(), 1);
    s.insert_message(&msg(lid, "queued-send", 0, "sent after mapping")).unwrap();
    s.set_media_path(lid, "queued-send", "new.jpg").unwrap();
    s.set_reaction(lid, "queued-send", "peer@s", "x").unwrap();
    s.set_archived(lid, true).unwrap();
    assert_eq!(s.chats().unwrap().len(), 1);
    assert_eq!(s.message(lid, "queued-send").unwrap().header.chat, pn);
    assert_eq!(s.message(pn, "queued-send").unwrap().media.path.as_deref(), Some("new.jpg"));
    assert_eq!(s.marks(pn).unwrap().reactions.len(), 1);
    assert!(s.is_archived(pn).unwrap());
    let conn = s.conn.lock().unwrap();
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM messages WHERE chat = ?1", [lid], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

struct RetainedStore {
    repository: MessageStore,
    pruning: DiskRetentionManager,
}

impl std::ops::Deref for RetainedStore {
    type Target = MessageStore;
    fn deref(&self) -> &MessageStore { &self.repository }
}

impl RetainedStore {
    fn enforce_retention(&self) -> Result<usize> { self.pruning.enforce(self) }
    fn enforce_retention_for(&self, chats: &[String]) -> Result<usize> { self.pruning.enforce_for(self, chats) }
}

fn store(retention: DiskRetention) -> RetainedStore {
    RetainedStore {
        repository: MessageStore::open(Path::new(":memory:")).unwrap(),
        pruning: DiskRetentionManager::new(retention),
    }
}

fn msg(chat: &str, id: &str, age_hours: i64, text: &str) -> StoredMessage {
    StoredMessage {
        header: MessageHeader {
            chat: chat.into(),
            id: id.into(),
            sender: "them".into(),
            timestamp: now() - age_hours * 3600,
            from_me: false,
        },
        text: text.into(),
        ..Default::default()
    }
}

#[test]
fn scoped_retention_only_touches_the_given_chats() {
    let s = store(DiskRetention { max_age_hours: RetentionLimit::Unlimited, max_messages_per_chat: RetentionLimit::Limited(1) });
    s.enforce_retention_for(&[]).unwrap();
    for chat in ["a@s", "b@s"] {
        s.insert_message(&msg(chat, "old", 2, "x")).unwrap();
        s.insert_message(&msg(chat, "new", 1, "y")).unwrap();
    }
    assert_eq!(s.enforce_retention_for(&["a@s".to_string()]).unwrap(), 1);
    assert_eq!(s.messages_for("a@s", 10).unwrap().len(), 1);
    assert_eq!(s.messages_for("b@s", 10).unwrap().len(), 2);
    assert_eq!(s.enforce_retention().unwrap(), 1);
    assert_eq!(s.messages_for("b@s", 10).unwrap().len(), 1);
}

#[test]
fn repository_and_disk_policy_are_independent() {
    let repository = MessageStore::open(Path::new(":memory:")).unwrap();
    let policy = DiskRetentionManager::new(DiskRetention::unlimited());
    repository.insert_message(&msg("quiet@s", "old", 100, "kept until explicit pruning")).unwrap();
    assert_eq!(policy.enforce_for(&repository, &[]).unwrap(), 0);
    policy.set_policy(DiskRetention { max_age_hours: RetentionLimit::Limited(24), max_messages_per_chat: RetentionLimit::Unlimited });
    assert_eq!(repository.count().unwrap(), 1);
    assert_eq!(policy.enforce_for(&repository, &[]).unwrap(), 1);
    assert_eq!(repository.chats().unwrap().len(), 1);
}

#[test]
fn no_policy_prunes_nothing_and_keeps_everything() {
    let s = store(DiskRetention::unlimited());
    s.insert_message(&msg("a@s", "old", 24 * 30, "x")).unwrap();
    s.insert_message(&msg("a@s", "new", 1, "y")).unwrap();
    // Even a month-old message stays: with nothing to enforce, the prune is a
    // no-op rather than a scan, so it never holds the store lock per message.
    assert_eq!(s.enforce_retention().unwrap(), 0);
    assert_eq!(s.messages_for("a@s", 10).unwrap().len(), 2);
}

#[test]
fn search_stays_fast_at_fifty_thousand_messages() {
    let s = store(DiskRetention::unlimited());
    {
        let _commit = s.batch();
        for i in 0..50_000 {
            s.insert_message(&msg("big@s", &i.to_string(), 0, &format!("message number {i}"))).unwrap();
        }
    }
    let started = std::time::Instant::now();
    let found = s.search_messages("big@s", "number 49999", 50).unwrap();
    assert_eq!(found.len(), 1);
    assert!(started.elapsed() < std::time::Duration::from_secs(1), "took {:?}", started.elapsed());
}

#[test]
fn clearing_history_keeps_names_and_settings() {
    let s = store(DiskRetention::unlimited());
    s.insert_message(&msg("a@s", "1", 0, "hi")).unwrap();
    s.set_name("a@s", "Ann").unwrap();
    s.set_chat_auto_download("a@s", false).unwrap();
    assert_eq!(s.clear_history().unwrap(), 1);
    assert_eq!(s.count().unwrap(), 0);
    assert_eq!(s.name_for("a@s").unwrap().as_deref(), Some("Ann"));
    assert_eq!(s.chat_auto_download("a@s").unwrap(), Some(false));
}

#[test]
fn clear_chat_keeps_an_empty_row_and_delete_hides_it() {
    let s = store(DiskRetention::unlimited());
    s.insert_message(&msg("a@s", "1", 0, "hi")).unwrap();
    s.insert_message(&msg("b@s", "1", 0, "yo")).unwrap();
    s.set_name("a@s", "Ann").unwrap();
    s.set_name("b@s", "Bob").unwrap();

    assert_eq!(s.clear_chat("a@s").unwrap(), 1);
    assert!(s.messages_for("a@s", 10).unwrap().is_empty());
    let chats = s.chats().unwrap();
    // Cleared chat stays as an empty row; the other chat is untouched.
    let kept = chats.iter().find(|c| c.chat == "a@s").expect("cleared chat stays");
    assert_eq!(kept.message_count, 0);
    assert_eq!(kept.display_name.as_deref(), Some("Ann"));
    assert_eq!(s.messages_for("b@s", 10).unwrap().len(), 1);

    // A new message retires the kept-empty row back into a normal chat.
    s.insert_message(&msg("a@s", "2", 0, "back")).unwrap();
    let kept = s.chats().unwrap().into_iter().find(|c| c.chat == "a@s").unwrap();
    assert_eq!(kept.message_count, 1);

    assert_eq!(s.delete_chat("b@s").unwrap(), 1);
    assert!(s.chats().unwrap().iter().all(|c| c.chat != "b@s"));
    // A new message brings a deleted chat back.
    s.insert_message(&msg("b@s", "2", 0, "again")).unwrap();
    assert!(s.chats().unwrap().iter().any(|c| c.chat == "b@s"));
}

#[test]
fn chat_privacy_overrides_round_trip() {
    let s = store(DiskRetention::unlimited());
    assert_eq!(s.chat_privacy("a@s").unwrap(), (None, None));
    s.set_chat_privacy("a@s", Some(false), None).unwrap();
    assert_eq!(s.chat_privacy("a@s").unwrap(), (Some(false), None));
    s.set_chat_privacy("a@s", None, None).unwrap();
    assert_eq!(s.chat_privacy("a@s").unwrap(), (None, None));
}

#[test]
fn overlapping_batches_commit_when_the_last_drops() {
    let s = store(DiskRetention::default());
    let autocommit = |s: &MessageStore| s.conn.lock().unwrap().is_autocommit();
    let first = s.batch();
    let second = s.batch();
    s.insert_message(&msg("a", "1", 0, "x")).unwrap();
    drop(first);
    assert!(!autocommit(&s), "still inside the second batch");
    drop(second);
    assert!(autocommit(&s), "committed");
    assert_eq!(s.messages_for("a", 10).unwrap().len(), 1);
}

#[test]
fn chat_retention_overrides_the_global_policy() {
    let s = store(DiskRetention { max_age_hours: RetentionLimit::Limited(24), max_messages_per_chat: RetentionLimit::Limited(1) });
    for (chat, id, age) in [("a", "1", 1), ("a", "2", 2), ("a", "3", 48), ("b", "1", 1), ("b", "2", 48), ("c", "1", 1), ("c", "2", 3)] {
        s.insert_message(&msg(chat, id, age, "x")).unwrap();
    }
    let keep_all = ChatRetention { max_age_hours: RetentionLimit::Unlimited, max_messages: RetentionLimit::Unlimited, on_demand: true };
    s.set_chat_retention("a", &keep_all).unwrap();
    let two_hours = ChatRetention { max_age_hours: RetentionLimit::Limited(2), max_messages: RetentionLimit::Inherit, on_demand: true };
    s.set_chat_retention("c", &two_hours).unwrap();
    s.enforce_retention().unwrap();
    assert_eq!(s.messages_for("a", 10).unwrap().len(), 3, "unlimited override");
    assert_eq!(s.messages_for("b", 10).unwrap().len(), 1, "global policy");
    // "c" keeps its own 2 h window but still falls back to the global cap.
    assert_eq!(s.messages_for("c", 10).unwrap().len(), 1);
    assert_eq!(s.chat_retention("a").unwrap(), keep_all);
    s.set_chat_retention("a", &ChatRetention::default()).unwrap();
    assert_eq!(s.chat_retention("a").unwrap(), ChatRetention::default());
}

#[test]
fn receipts_only_move_forward() {
    let s = store(DiskRetention::unlimited());
    s.record_receipt("m", "a", "delivered", 10).unwrap();
    s.record_receipt("m", "a", "read", 20).unwrap();
    s.record_receipt("m", "a", "delivered", 30).unwrap();
    s.record_receipt("m", "b", "played", 40).unwrap();
    let got = s.receipts("m").unwrap();
    let a = got.iter().find(|r| r.recipient == "a").unwrap();
    assert_eq!((a.delivered_at, a.read_at, a.played_at), (Some(10), Some(20), None));
    let b = got.iter().find(|r| r.recipient == "b").unwrap();
    assert_eq!((b.delivered_at, b.read_at, b.played_at), (Some(40), Some(40), Some(40)));
}

#[test]
fn soft_delete_keeps_the_row_and_its_marks() {
    let s = store(DiskRetention::unlimited());
    s.insert_message(&msg("a", "1", 0, "hi")).unwrap();
    s.set_forwarded("a", "1").unwrap();
    s.set_view_once("a", "1", true).unwrap();

    s.set_message_deleted("a", "1", true).unwrap();

    // The row and its marks are all still here; only the flag changed.
    let kept = s.message("a", "1").unwrap();
    assert!(kept.local.deleted);
    assert_eq!(kept.text, "hi");
    let marks = s.marks("a").unwrap();
    assert_eq!(marks.forwarded, vec!["1".to_string()]);
    assert_eq!(marks.view_once.len(), 1);
    // The chat preview skips deleted rows.
    let summary = s.chats().unwrap().into_iter().find(|c| c.chat == "a").unwrap();
    assert_ne!(summary.last_text, "hi");

    // Clearing the flag restores it; nothing was ever removed.
    s.set_message_deleted("a", "1", false).unwrap();
    assert!(!s.message("a", "1").unwrap().local.deleted);
}

#[test]
fn edits_replace_text_and_mark_the_message() {
    let s = store(DiskRetention::unlimited());
    s.insert_message(&msg("a", "1", 0, "old")).unwrap();
    assert!(s.update_message_content("a", "1", "new").unwrap());
    assert!(!s.update_message_content("a", "missing", "new").unwrap());
    assert_eq!(s.messages_for("a", 1).unwrap()[0].text, "new");
    assert_eq!(s.marks("a").unwrap().edited, vec!["1".to_string()]);
}

#[test]
fn pings_and_message_search() {
    let s = store(DiskRetention::unlimited());
    let mut ping = msg("g", "1", 1, "hey @123 look");
    ping.local.mentioned = true;
    s.insert_message(&ping).unwrap();
    s.insert_message(&msg("g", "2", 0, "100% done_ok")).unwrap();
    let mut elsewhere = msg("h", "3", 0, "@123");
    elsewhere.local.mentioned = true;
    s.insert_message(&elsewhere).unwrap();
    assert_eq!(s.pings(Some("g"), 10).unwrap().len(), 1);
    assert_eq!(s.pings(None, 10).unwrap().len(), 2);
    assert_eq!(s.search_messages("g", "LOOK", 10).unwrap()[0].header.id, "1");
    assert_eq!(s.search_messages("g", "0% d", 10).unwrap()[0].header.id, "2");
    assert!(s.search_messages("g", "_", 10).unwrap().iter().all(|m| m.text.contains('_')));
}

#[test]
fn stores_and_reads_messages() {
    let s = store(DiskRetention::unlimited());
    s.insert_message(&msg("a@s", "1", 0, "hello")).unwrap();
    let got = s.messages_for("a@s", 10).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].text, "hello");
}

#[test]
fn insert_replaces_same_id() {
    let s = store(DiskRetention::unlimited());
    s.insert_message(&msg("a@s", "1", 0, "first")).unwrap();
    s.insert_message(&msg("a@s", "1", 0, "edited")).unwrap();
    let got = s.messages_for("a@s", 10).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].text, "edited");
}

#[test]
fn burst_of_inbound_messages_lands_once_with_one_chat_summary() {
    // Regression coverage: a burst of inbound messages must be writable in
    // one batch and readable with one chat-list query and one conversation
    // query, instead of a refresh per message.
    let s = store(DiskRetention::unlimited());
    let started = std::time::Instant::now();
    {
        let _commit = s.batch();
        for i in 0..500 {
            s.insert_message(&msg("burst@s", &i.to_string(), 0, &format!("message {i}"))).unwrap();
        }
    }
    assert_eq!(s.chats().unwrap().len(), 1);
    let summary = &s.chats().unwrap()[0];
    assert_eq!(summary.message_count, 500);
    assert_eq!(summary.unread_count, 500);
    assert_eq!(s.messages_for("burst@s", 500).unwrap().len(), 500);
    eprintln!("burst 500: batch insert + chats + messages in {:?}", started.elapsed());
}

#[test]
fn replayed_messages_never_regress_local_state() {
    let s = store(DiskRetention::unlimited());
    let mut sent = msg("a@s", "1", 0, "hi");
    sent.header.from_me = true;
    sent.local.status =Some("pending".into());
    s.insert_message(&sent).unwrap();
    assert!(s.set_delivery_state("a@s", "1", "delivered").unwrap());
    s.set_media_path("a@s", "1", "/tmp/1.jpg").unwrap();
    sent.local.status =None;
    s.insert_message(&sent).unwrap();
    let got = s.message("a@s", "1").unwrap();
    assert_eq!(got.local.status.as_deref(), Some("delivered"));
    assert_eq!(got.media.path.as_deref(), Some("/tmp/1.jpg"));

    s.insert_message(&msg("a@s", "2", 0, "original")).unwrap();
    s.mark_read("a@s").unwrap();
    assert!(s.update_message_content("a@s", "2", "fixed").unwrap());
    s.insert_message(&msg("a@s", "2", 0, "original")).unwrap();
    let got = s.message("a@s", "2").unwrap();
    assert!(got.local.read);
    assert_eq!(got.text, "fixed");

    s.insert_message(&msg("a@s", "3", 0, "oops")).unwrap();
    assert!(s.revoke_message("a@s", "3").unwrap());
    s.insert_message(&msg("a@s", "3", 0, "oops")).unwrap();
    let got = s.message("a@s", "3").unwrap();
    assert!(got.local.revoked);
    // The revoke keeps the local copy, and a replay may not clear the flag.
    assert_eq!(got.text, "oops");
}

#[test]
fn audio_duration_survives_a_replay_without_it() {
    let s = store(DiskRetention::unlimited());
    let mut note = msg("a@s", "1", 0, "[audio]");
    note.media = Media { kind: Some("audio".into()), duration: Some(7), ..Default::default() };
    s.insert_message(&note).unwrap();
    assert_eq!(s.message("a@s", "1").unwrap().media.duration, Some(7));
    // A replay carrying no metadata must not erase the kept length.
    note.media.duration = None;
    s.insert_message(&note).unwrap();
    assert_eq!(s.message("a@s", "1").unwrap().media.duration, Some(7));
}

#[test]
fn drops_messages_older_than_the_window() {
    let s = store(DiskRetention {
        max_age_hours: RetentionLimit::Limited(24),
        max_messages_per_chat: RetentionLimit::Unlimited,
    });
    s.insert_message(&msg("a@s", "old", 48, "ancient")).unwrap();
    s.insert_message(&msg("a@s", "new", 1, "recent")).unwrap();
    assert_eq!(s.enforce_retention().unwrap(), 1);
    let got = s.messages_for("a@s", 10).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].text, "recent");
}

#[test]
fn caps_messages_per_chat() {
    let s = store(DiskRetention {
        max_age_hours: RetentionLimit::Unlimited,
        max_messages_per_chat: RetentionLimit::Limited(3),
    });
    for i in 0..10 {
        s.insert_message(&msg("a@s", &i.to_string(), i, &format!("m{i}")))
            .unwrap();
    }
    s.enforce_retention().unwrap();
    assert_eq!(s.count().unwrap(), 3);
    // The newest survive.
    let got = s.messages_for("a@s", 10).unwrap();
    assert_eq!(got[0].text, "m0");
}

#[test]
fn cap_applies_per_chat() {
    let s = store(DiskRetention {
        max_age_hours: RetentionLimit::Unlimited,
        max_messages_per_chat: RetentionLimit::Limited(2),
    });
    for i in 0..5 {
        s.insert_message(&msg("a@s", &i.to_string(), i, "x")).unwrap();
        s.insert_message(&msg("b@s", &i.to_string(), i, "y")).unwrap();
    }
    s.enforce_retention().unwrap();
    assert_eq!(s.messages_for("a@s", 99).unwrap().len(), 2);
    assert_eq!(s.messages_for("b@s", 99).unwrap().len(), 2);
}

#[test]
fn summaries_are_newest_first() {
    let s = store(DiskRetention::unlimited());
    s.insert_message(&msg("old@s", "1", 10, "older")).unwrap();
    s.insert_message(&msg("new@s", "1", 1, "newer")).unwrap();
    let chats = s.chats().unwrap();
    assert_eq!(chats[0].chat, "new@s");
    assert_eq!(chats[0].last_text, "newer");
    assert_eq!(chats[1].chat, "old@s");
}

#[test]
fn names_resolve_in_reads() {
    let s = store(DiskRetention::unlimited());
    s.insert_message(&msg("group@g.us", "1", 0, "hi")).unwrap();
    s.set_name("group@g.us", "Team Chat").unwrap();
    s.set_name("them", "Alice").unwrap();

    let got = s.messages_for("group@g.us", 10).unwrap();
    assert_eq!(got[0].sender_name.as_deref(), Some("Alice"));

    let chats = s.chats().unwrap();
    assert_eq!(chats[0].display_name.as_deref(), Some("Team Chat"));
}

#[test]
fn empty_name_does_not_erase_a_known_name() {
    let s = store(DiskRetention::unlimited());
    s.set_name("a@s", "Alice").unwrap();
    s.set_name("a@s", "   ").unwrap();
    assert_eq!(s.name_for("a@s").unwrap().as_deref(), Some("Alice"));
}

#[test]
fn saved_names_keep_numeric_labels_until_contact_removed() {
    let s = store(DiskRetention::default());
    s.set_contact_state("1@lid", Some("59899022028"), true, 10).unwrap();
    s.set_push_name("1@lid", "Ana").unwrap();
    assert_eq!(s.name_for("1@lid").unwrap().as_deref(), Some("59899022028"));
    assert_eq!(s.contact_identity("1@lid").unwrap().push_name.as_deref(), Some("Ana"));
    s.set_contact_state("1@lid", None, false, 20).unwrap();
    assert_eq!(s.name_for("1@lid").unwrap().as_deref(), Some("Ana"));
    assert!(!s.name_is_saved("1@lid").unwrap());
    s.set_contact_state("2@lid", Some("Bea"), true, 10).unwrap();
    s.set_name("2@lid", "Other").unwrap();
    assert_eq!(s.name_for("2@lid").unwrap().as_deref(), Some("Bea"));
    // A masked group label never replaces a push name, and a push name replaces it.
    s.set_name("3@lid", "Cata").unwrap();
    s.set_name("3@lid", "+598∙∙∙∙∙27").unwrap();
    assert_eq!(s.name_for("3@lid").unwrap().as_deref(), Some("Cata"));
    s.set_name("4@lid", "+598∙∙∙∙∙41").unwrap();
    s.set_name("4@lid", "Dani").unwrap();
    assert_eq!(s.name_for("4@lid").unwrap().as_deref(), Some("Dani"));
    assert!(is_placeholder_name("+598∙∙∙∙∙27") && is_placeholder_name("59899") && !is_placeholder_name("Ana"));
}

#[test]
fn names_survive_message_pruning() {
    // A name is learned from a message but must outlive it, otherwise the
    // chat list falls back to a raw number once history ages out.
    let s = store(DiskRetention {
        max_age_hours: RetentionLimit::Limited(1),
        max_messages_per_chat: RetentionLimit::Unlimited,
    });
    s.insert_message(&msg("a@s", "older", 72, "hello")).unwrap();
    s.insert_message(&msg("a@s", "old", 48, "hi")).unwrap();
    s.set_name("a@s", "Alice").unwrap();
    s.enforce_retention().unwrap();
    assert_eq!(s.count().unwrap(), 0);
    assert_eq!(s.name_for("a@s").unwrap().as_deref(), Some("Alice"));
}

#[test]
fn quiet_chats_keep_metadata_without_expired_message_content() {
    let s = store(DiskRetention { max_age_hours: RetentionLimit::Limited(24), max_messages_per_chat: RetentionLimit::Unlimited });
    s.insert_message(&msg("quiet@s", "1", 100, "first")).unwrap();
    s.insert_message(&msg("quiet@s", "2", 50, "last word")).unwrap();
    s.insert_message(&msg("busy@s", "1", 50, "old")).unwrap();
    s.insert_message(&msg("busy@s", "2", 1, "new")).unwrap();
    s.set_saved_name("quiet@s", "Quiet contact").unwrap();
    s.set_pinned("quiet@s", true).unwrap();
    let last = s.messages_for("quiet@s", 1).unwrap()[0].header.timestamp;
    s.enforce_retention().unwrap();
    let chats = s.chats().unwrap();
    assert_eq!(chats.len(), 2, "no chat vanishes from the list");
    assert!(s.messages_for("quiet@s", 9).unwrap().is_empty());
    let quiet = chats.iter().find(|chat| chat.chat == "quiet@s").unwrap();
    assert_eq!(quiet.last_message_at, last);
    assert_eq!(quiet.display_name.as_deref(), Some("Quiet contact"));
    assert!(quiet.pinned);
    assert_eq!(quiet.message_count, 0);
    assert!(quiet.last_text.is_empty());
    assert!(s.search_messages("quiet@s", "last word", 10).unwrap().is_empty());
    assert_eq!(s.messages_for("busy@s", 9).unwrap().len(), 1);
}

#[test]
fn unpinned_pin_tombstones_do_not_freeze_the_chat_order() {
    let s = MessageStore::open(Path::new(":memory:")).unwrap();
    s.insert_message(&msg("old@s", "1", 100, "old")).unwrap();
    s.insert_message(&msg("new@s", "1", 1, "new")).unwrap();
    // The account's pin state keeps a row for every synced chat, pinned or not,
    // with a millisecond timestamp. An unpinned tombstone must not outrank a
    // newer message: doing so pins the whole list to the old account order.
    s.replace_pin_state(&[pins::PinState {
        chat: "old@s".into(),
        pinned: false,
        timestamp: 1_790_000_000_000,
        sequence: 5,
    }])
    .unwrap();
    let order: Vec<_> = s.chats().unwrap().iter().map(|chat| chat.chat.clone()).collect();
    assert_eq!(order, ["new@s", "old@s"]);
}

#[test]
fn unread_counts_only_incoming_unread() {
    let s = store(DiskRetention::unlimited());
    let mut incoming = msg("a@s", "1", 0, "hi");
    incoming.local.read = false;
    s.insert_message(&incoming).unwrap();

    let mut outgoing = msg("a@s", "2", 0, "hello");
    outgoing.header.from_me = true;
    outgoing.local.read = true;
    s.insert_message(&outgoing).unwrap();

    let chats = s.chats().unwrap();
    assert_eq!(chats[0].unread_count, 1);

    assert_eq!(s.mark_read("a@s").unwrap(), 1);
    assert_eq!(s.chats().unwrap()[0].unread_count, 0);
}

#[test]
fn explicit_zero_count_keeps_no_messages_without_becoming_unlimited_or_inherited() {
    let s = store(DiskRetention::unlimited());
    for chat in ["empty", "unlimited", "inherited"] {
        s.insert_message(&msg(chat, "one", 1, "kept by default")).unwrap();
    }
    s.set_chat_retention("empty", &ChatRetention {
        max_age_hours: RetentionLimit::Inherit,
        max_messages: RetentionLimit::Limited(0),
        on_demand: true,
    }).unwrap();
    assert_eq!(s.enforce_retention().unwrap(), 1);
    assert!(s.messages_for("empty", 10).unwrap().is_empty());
    assert_eq!(s.messages_for("unlimited", 10).unwrap().len(), 1);
    assert_eq!(s.messages_for("inherited", 10).unwrap().len(), 1);
    assert_eq!(s.chats().unwrap().len(), 3);
}

#[test]
fn per_chat_age_removes_last_message_and_its_poll_state() {
    let s = store(DiskRetention::unlimited());
    s.insert_message(&msg("quiet@s", "poll", 48, "Expired question")).unwrap();
    s.save_poll("quiet@s", "poll", "them", "Expired question", &["one".into()], false, Some(&[1; 32])).unwrap();
    s.set_poll_vote("quiet@s", "poll", "them", &["one".into()]).unwrap();
    s.set_reaction("quiet@s", "poll", "them", "yes").unwrap();
    s.set_starred("quiet@s", "poll", true).unwrap();
    s.set_message_pin("quiet@s", Some("poll")).unwrap();
    s.set_chat_retention("quiet@s", &ChatRetention {
        max_age_hours: RetentionLimit::Limited(1), max_messages: RetentionLimit::Inherit, on_demand: true,
    }).unwrap();
    assert_eq!(s.enforce_retention().unwrap(), 1);
    let marks = s.marks("quiet@s").unwrap();
    assert!(marks.polls.is_empty());
    assert!(marks.reactions.is_empty());
    assert!(marks.starred.is_empty());
    assert!(marks.pinned.is_none());
    assert!(s.poll_secret("quiet@s", "poll").unwrap().is_none());
    assert_eq!(s.chats().unwrap()[0].message_count, 0);
}

#[test]
fn marking_read_is_idempotent() {
    let s = store(DiskRetention::unlimited());
    s.insert_message(&msg("a@s", "1", 0, "hi")).unwrap();
    assert_eq!(s.mark_read("a@s").unwrap(), 1);
    // Nothing left to change the second time.
    assert_eq!(s.mark_read("a@s").unwrap(), 0);
}

#[test]
fn mark_read_until_only_marks_up_to_the_cutoff() {
    let s = store(DiskRetention::unlimited());
    s.insert_message(&msg("a@s", "1", 3, "one")).unwrap();
    s.insert_message(&msg("a@s", "2", 2, "two")).unwrap();
    s.insert_message(&msg("a@s", "3", 1, "three")).unwrap();

    let up_to_two: Vec<String> =
        s.unread_until("a@s", "2").unwrap().into_iter().map(|(id, _)| id).collect();
    assert_eq!(up_to_two, vec!["1".to_string(), "2".to_string()]);
    assert_eq!(s.mark_read_until("a@s", "2").unwrap(), 2);
    // The newest message stays unread until its own cutoff.
    assert_eq!(s.unread_ids("a@s").unwrap().len(), 1);
    assert_eq!(s.mark_read_until("a@s", "3").unwrap(), 1);
    assert!(s.unread_ids("a@s").unwrap().is_empty());
}

#[test]
fn mark_read_through_marks_only_messages_at_or_before_the_time() {
    let s = store(DiskRetention::unlimited());
    let t = now();
    let mut old = msg("a@s", "1", 0, "old");
    old.header.timestamp = t - 100;
    let mut recent = msg("a@s", "2", 0, "recent");
    recent.header.timestamp = t;
    s.insert_message(&old).unwrap();
    s.insert_message(&recent).unwrap();

    assert_eq!(s.mark_read_through("a@s", t - 50).unwrap(), 1);
    assert_eq!(s.unread_ids("a@s").unwrap().len(), 1);
    assert_eq!(s.mark_read_through("a@s", t).unwrap(), 1);
    assert!(s.unread_ids("a@s").unwrap().is_empty());
}

#[test]
fn media_and_reply_fields_round_trip() {
    let s = store(DiskRetention::unlimited());
    let mut m = msg("a@s", "1", 0, "look");
    m.media.kind = Some("image".into());
    m.media.path = Some("/tmp/pic.jpg".into());
    m.quote.id = Some("0".into());
    m.quote.text = Some("earlier".into());
    s.insert_message(&m).unwrap();

    let got = &s.messages_for("a@s", 1).unwrap()[0];
    assert_eq!(got.media.kind.as_deref(), Some("image"));
    assert_eq!(got.quote.text.as_deref(), Some("earlier"));
}

#[test]
fn relocate_media_moves_only_referenced_files() {
    let root = std::env::temp_dir().join(format!("postal-relocate-{}", std::process::id()));
    let (shared, to) = (root.join("shared"), root.join("app"));
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::write(shared.join("1.jpg"), b"ours").unwrap();
    std::fs::write(shared.join("other.jpg"), b"not ours").unwrap();

    let s = store(DiskRetention::unlimited());
    let mut m = msg("a@s", "1", 0, "");
    m.media.path = Some(shared.join("1.jpg").to_string_lossy().into());
    m.media.thumb = Some(shared.join("1.jpg").to_string_lossy().into());
    s.insert_message(&m).unwrap();

    assert_eq!(s.relocate_media(&[&shared], &to).unwrap(), 2);
    let got = s.message("a@s", "1").unwrap();
    assert_eq!(got.media.path.as_deref(), Some(&*to.join("1.jpg").to_string_lossy()));
    assert_eq!(got.media.thumb, got.media.path);
    assert_eq!(std::fs::read(to.join("1.jpg")).unwrap(), b"ours");
    assert!(shared.join("other.jpg").exists());
    // Idempotent: a second run finds nothing to do.
    assert_eq!(s.relocate_media(&[&shared], &to).unwrap(), 0);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn status_advances_but_never_regresses() {
    let s = store(DiskRetention::unlimited());
    let mut m = msg("a@s", "1", 0, "hi");
    m.header.from_me = true;
    m.local.status = Some("pending".into());
    s.insert_message(&m).unwrap();

    assert!(s.set_delivery_state("a@s", "1", "sent").unwrap());
    assert!(s.set_delivery_state("a@s", "1", "delivered").unwrap());
    assert!(s.set_delivery_state("a@s", "1", "read").unwrap());
    // A late duplicate must not undo the read state.
    assert!(!s.set_delivery_state("a@s", "1", "delivered").unwrap());
    assert_eq!(s.message("a@s", "1").unwrap().local.status.as_deref(), Some("read"));
}

#[test]
fn status_ignores_incoming_messages() {
    let s = store(DiskRetention::unlimited());
    s.insert_message(&msg("a@s", "1", 0, "hi")).unwrap();
    assert!(!s.set_delivery_state("a@s", "1", "read").unwrap());
}

#[test]
fn status_by_id_advances_without_the_chat() {
    // Server acks name the message id but only sometimes the chat, and the
    // named JID can differ in form from the stored one. The id alone must
    // still move a pending message to sent.
    let s = store(DiskRetention::unlimited());
    let mut m = msg("a@s.whatsapp.net", "1", 0, "hi");
    m.header.from_me = true;
    m.local.status = Some("pending".into());
    s.insert_message(&m).unwrap();

    // Wrong chat: the addressed update misses.
    assert!(!s.set_delivery_state("b@s.whatsapp.net", "1", "sent").unwrap());
    // Id-only update still advances it.
    let updated = s.set_delivery_state_by_id("1", "sent").unwrap();
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].local.status.as_deref(), Some("sent"));
    assert_eq!(
        s.message("a@s.whatsapp.net", "1").unwrap().local.status.as_deref(),
        Some("sent")
    );
    // Forward-only still holds through the id path.
    assert!(s.set_delivery_state_by_id("1", "delivered").unwrap().len() == 1);
    assert!(s.set_delivery_state_by_id("1", "sent").unwrap().is_empty());
}

#[test]
fn revoking_keeps_the_row_and_its_content() {
    let s = store(DiskRetention::unlimited());
    s.insert_message(&msg("a@s", "1", 0, "oops")).unwrap();
    assert!(s.revoke_message("a@s", "1").unwrap());

    let got = &s.messages_for("a@s", 1).unwrap()[0];
    assert!(got.local.revoked);
    assert_eq!(got.text, "oops");
    // Revoking twice changes nothing the second time.
    assert!(!s.revoke_message("a@s", "1").unwrap());
}

#[test]
fn system_rows_round_trip_and_stay_out_of_the_preview() {
    let s = store(DiskRetention::unlimited());
    s.insert_message(&msg("a@s", "1", 1, "hello")).unwrap();
    let mut notice = msg("a@s", "2", 0, "");
    notice.system = SystemNotice { kind: Some("E2E_IDENTITY_CHANGED".into()), params: vec!["a@s".into()] };
    notice.local.read = true;
    s.insert_message(&notice).unwrap();
    let got = s.message("a@s", "2").unwrap();
    assert_eq!(got.system, notice.system);
    let chat = &s.chats().unwrap()[0];
    assert_eq!(chat.last_text, "hello");
    assert_eq!(chat.unread_count, 1);
}

#[test]
fn chat_unread_marks_survive_reopen_without_changing_message_read_state() {
    let root = std::env::temp_dir().join(format!("postal-unread-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("synthetic.db");
    {
        let s = MessageStore::open(&path).unwrap();
        let mut message = msg("200@s.whatsapp.net", "read", 1, "already read");
        message.local.read = true;
        s.insert_message(&message).unwrap();
        s.set_marked_unread("200@s.whatsapp.net", true).unwrap();
    }
    {
        let s = MessageStore::open(&path).unwrap();
        let chat = s.chats().unwrap().remove(0);
        assert!(chat.marked_unread);
        assert_eq!(chat.unread_count, 0);
        assert!(s.message("200@s.whatsapp.net", "read").unwrap().local.read);
        s.set_marked_unread("200@s.whatsapp.net", false).unwrap();
        assert!(!s.chats().unwrap()[0].marked_unread);
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn missed_calls_reach_the_preview_and_notice_only_chats_stay_listed() {
    let s = store(DiskRetention::unlimited());
    s.insert_message(&msg("a@s", "1", 10, "hello")).unwrap();
    let mut call = msg("a@s", "2", 1, "");
    call.system = SystemNotice { kind: Some("CALL_MISSED_VOICE".into()), params: vec![] };
    s.insert_message(&call).unwrap();
    let mut created = msg("g@g.us", "3", 5, "");
    created.system = SystemNotice { kind: Some("GROUP_CREATE".into()), params: vec![] };
    s.insert_message(&created).unwrap();
    let chats = s.chats().unwrap();
    let a = chats.iter().find(|c| c.chat == "a@s").unwrap();
    assert_eq!(a.last_media_kind.as_deref(), Some("missed_call"));
    assert!(chats.iter().any(|c| c.chat == "g@g.us"));
    let mut stranger = msg("b@s", "4", 1, "");
    stranger.system = SystemNotice { kind: Some("E2E_IDENTITY_CHANGED".into()), params: vec![] };
    s.insert_message(&stranger).unwrap();
    assert!(!s.chats().unwrap().iter().any(|c| c.chat == "b@s"));
    let at = call.header.timestamp;
    assert!(s.has_system_near("a@s", "CALL_MISSED_VOICE", &[], at + 3, true).unwrap());
    assert!(!s.has_system_near("a@s", "CALL_MISSED_VOICE", &[], at + 30, true).unwrap());
    assert!(!s.has_system_near("a@s", "GROUP_CREATE", &[], at, true).unwrap());
    assert!(!s.has_system_near("a@s", "CALL_MISSED_VOICE", &["other@s".into()], at, true).unwrap());
}

#[test]
fn unlimited_retention_keeps_everything() {
    let s = store(DiskRetention::unlimited());
    for i in 0..50 {
        s.insert_message(&msg("a@s", &i.to_string(), i * 100, "x")).unwrap();
    }
    assert_eq!(s.enforce_retention().unwrap(), 0);
    assert_eq!(s.count().unwrap(), 50);
}

#[test]
fn merging_chats_folds_history_state_and_keeps_the_chat_visible() {
    let s = store(DiskRetention::unlimited());
    s.insert_message(&msg("123@lid", "a", 2, "hi")).unwrap();
    s.insert_message(&msg("123@lid", "b", 1, "there")).unwrap();
    s.insert_message(&msg("5989@s.whatsapp.net", "c", 3, "old")).unwrap();
    s.set_archived("123@lid", true).unwrap();
    s.set_pinned("123@lid", true).unwrap();
    s.set_name("123@lid", "Ma cherie").unwrap();

    s.set_lid_pn("123", "5989").unwrap();

    assert_eq!(s.messages_for("123@lid", 10).unwrap()[0].header.chat, "5989@s.whatsapp.net");
    assert_eq!(s.messages_for("5989@s.whatsapp.net", 10).unwrap().len(), 3);
    assert!(s.is_archived("5989@s.whatsapp.net").unwrap());
    assert!(s.pinned_chats().unwrap().iter().any(|j| j == "5989@s.whatsapp.net"));
    assert_eq!(s.name_for("5989@s.whatsapp.net").unwrap().as_deref(), Some("Ma cherie"));
    // The merged chat still shows: folding must not hide it.
    assert!(s.chats().unwrap().iter().any(|c| c.chat == "5989@s.whatsapp.net"));
}

#[test]
fn pending_view_once_lists_only_unopened_incoming_stubs() {
    let s = store(DiskRetention::unlimited());
    for (id, age_hours) in [("1", 1), ("2", 3)] {
        let mut stub = msg("a@s", id, age_hours, "photo");
        stub.media.kind = Some("view_once".into());
        s.insert_message(&stub).unwrap();
        s.set_view_once("a@s", id, false).unwrap();
    }
    // Only the one inside the window counts as demand.
    let window = std::time::Duration::from_secs(2 * 3600);
    assert_eq!(s.pending_view_once(window).unwrap(), vec![("a@s".to_string(), "1".to_string())]);

    // Opening it on this device takes it off the list.
    s.open_view_once("a@s", "1").unwrap();
    assert!(s.pending_view_once(window).unwrap().is_empty());

    // Our own one-time sends are not demand either.
    let mut own = msg("a@s", "3", 0, "sent");
    own.header.from_me = true;
    s.insert_message(&own).unwrap();
    s.set_view_once("a@s", "3", true).unwrap();
    assert!(s.pending_view_once(window).unwrap().is_empty());

    // A kept copy is not demand: keeping it drops the mark.
    let mut kept = msg("a@s", "4", 0, "photo");
    kept.media.kind = Some("view_once".into());
    s.insert_message(&kept).unwrap();
    s.set_view_once("a@s", "4", false).unwrap();
    s.set_once_kind("a@s", "4", "image").unwrap();
    s.set_media_path("a@s", "4", "/tmp/kept.jpg").unwrap();
    s.keep_view_once("a@s", "4").unwrap();
    assert!(s.pending_view_once(window).unwrap().is_empty());
}

#[test]
fn live_location_updates_and_ends_survive_a_reload() {
    let s = store(DiskRetention::unlimited());
    let mut message = msg("a", "1", 0, "");
    message.media.kind = Some("live_location".into());
    message.live_location = Some(LiveLocation {
        lat: 1.0,
        lng: 2.0,
        sequence: Some(1),
        started_at: 10,
        updated_at: 10,
        ..Default::default()
    });
    s.insert_message(&message).unwrap();

    let moved = LiveLocation {
        lat: 3.0,
        lng: 4.0,
        accuracy: Some(20),
        sequence: Some(2),
        started_at: 10,
        updated_at: 20,
        ..Default::default()
    };
    assert!(s.update_live_location("a", "1", &moved, Some("data:image/jpeg;base64,eA==")).unwrap());
    let stored = s.message("a", "1").unwrap();
    let live = stored.live_location.clone().unwrap();
    assert_eq!((live.lat, live.lng, live.sequence), (3.0, 4.0, Some(2)));
    assert_eq!(stored.media.thumb.as_deref(), Some("data:image/jpeg;base64,eA=="));

    assert!(s.end_live_location("a", "1").unwrap());
    assert!(s.message("a", "1").unwrap().live_location.unwrap().ended);
    // Already ended: nothing left to change.
    assert!(!s.end_live_location("a", "1").unwrap());
}

#[test]
fn muting_at_all_hides_only_at_all_mentions() {
    let s = store(DiskRetention::unlimited());
    let mut all_only = msg("g@g.us", "1", 0, "@all hello");
    all_only.local.mentioned = true;
    all_only.local.mentioned_all_only = true;
    s.insert_message(&all_only).unwrap();
    let mut direct = msg("g@g.us", "2", 0, "@me hello");
    direct.local.mentioned = true;
    direct.local.mentioned_all_only = false;
    s.insert_message(&direct).unwrap();

    // Unmuted: both count.
    assert_eq!(s.chats().unwrap()[0].mention_count, 2);
    assert_eq!(s.pings(Some("g@g.us"), 10).unwrap().len(), 2);
    assert_eq!(s.unread_mentions("g@g.us").unwrap().len(), 2);
    assert!(!s.chat_mute_at_all("g@g.us").unwrap());

    s.set_chat_mute_at_all("g@g.us", true).unwrap();
    assert!(s.chat_mute_at_all("g@g.us").unwrap());
    // Muted: only the direct mention counts.
    assert_eq!(s.chats().unwrap()[0].mention_count, 1);
    assert!(s.chats().unwrap()[0].mute_at_all);
    assert_eq!(s.pings(Some("g@g.us"), 10).unwrap().len(), 1);
    assert_eq!(s.pings(Some("g@g.us"), 10).unwrap()[0].header.id, "2");
    assert_eq!(s.unread_mentions("g@g.us").unwrap(), vec!["2".to_string()]);

    s.set_chat_mute_at_all("g@g.us", false).unwrap();
    assert_eq!(s.chats().unwrap()[0].mention_count, 2);
}

#[test]
fn reopen_heals_version_stamped_databases_missing_new_columns() {
    use rusqlite::Connection;
    let root = std::env::temp_dir().join(format!("postal-heal-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("heal.db");
    {
        // A database migrated to the old tip, then version-bumped without
        // ever adding the new columns: every new query fails at prepare
        // time with "no such column".
        let conn = Connection::open(&path).unwrap();
        super::schema::migrate_to(&conn, super::schema::MIGRATIONS.len() - 2).unwrap();
        // `mute_at_all` never existed at the old tip (v17 adds it); drop the
        // messages column to complete the version-stamped-but-missing state.
        conn.execute_batch("ALTER TABLE messages DROP COLUMN mentioned_all_only;").unwrap();
        conn.pragma_update(None, "user_version", super::schema::MIGRATIONS.len() as i64).unwrap();
        let missing: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('messages') WHERE name = 'mentioned_all_only')",
            [], |r| r.get(0)).unwrap();
        assert!(!missing, "fixture must lack the new column");
    }
    {
        // Opening heals the columns, so the chat list, pings and the mute
        // toggle work on the previously broken file.
        let s = MessageStore::open(&path).unwrap();
        let mut all_only = msg("g@g.us", "1", 0, "@all hello");
        all_only.local.mentioned = true;
        all_only.local.mentioned_all_only = true;
        s.insert_message(&all_only).unwrap();
        assert_eq!(s.chats().unwrap()[0].mention_count, 1);
        s.set_chat_mute_at_all("g@g.us", true).unwrap();
        assert!(s.chat_mute_at_all("g@g.us").unwrap());
        assert_eq!(s.chats().unwrap()[0].mention_count, 0);
        assert!(s.unread_mentions("g@g.us").unwrap().is_empty());
    }
    std::fs::remove_dir_all(root).unwrap();
}
