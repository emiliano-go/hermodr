use super::*;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn store(retention: Retention) -> MessageStore {
    // In-memory keeps tests independent and fast. The real schema is used,
    // so adding a column never breaks the tests.
    MessageStore::open(Path::new(":memory:"), retention).unwrap()
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
    let s = store(Retention { max_age_hours: None, max_messages_per_chat: Some(1) });
    for chat in ["a@s", "b@s"] {
        s.insert_message(&msg(chat, "old", 2, "x")).unwrap();
        s.insert_message(&msg(chat, "new", 1, "y")).unwrap();
    }
    // The first call is the hourly full pass; mark it done to test the scoped one.
    s.last_full_prune.store(unix_now(), std::sync::atomic::Ordering::Relaxed);
    assert_eq!(s.enforce_retention_for(&["a@s".to_string()]).unwrap(), 1);
    assert_eq!(s.messages_for("a@s", 10).unwrap().len(), 1);
    assert_eq!(s.messages_for("b@s", 10).unwrap().len(), 2);
    assert_eq!(s.enforce_retention().unwrap(), 1);
    assert_eq!(s.messages_for("b@s", 10).unwrap().len(), 1);
}

#[test]
fn search_stays_fast_at_fifty_thousand_messages() {
    let s = store(Retention::unlimited());
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
    let s = store(Retention::unlimited());
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
    let s = store(Retention::unlimited());
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
    let s = store(Retention::unlimited());
    assert_eq!(s.chat_privacy("a@s").unwrap(), (None, None));
    s.set_chat_privacy("a@s", Some(false), None).unwrap();
    assert_eq!(s.chat_privacy("a@s").unwrap(), (Some(false), None));
    s.set_chat_privacy("a@s", None, None).unwrap();
    assert_eq!(s.chat_privacy("a@s").unwrap(), (None, None));
}

#[test]
fn overlapping_batches_commit_when_the_last_drops() {
    let s = store(Retention::default());
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
    let s = store(Retention { max_age_hours: Some(24), max_messages_per_chat: Some(1) });
    for (chat, id, age) in [("a", "1", 1), ("a", "2", 2), ("a", "3", 48), ("b", "1", 1), ("b", "2", 48), ("c", "1", 1), ("c", "2", 3)] {
        s.insert_message(&msg(chat, id, age, "x")).unwrap();
    }
    let keep_all = ChatRetention { max_age_hours: Some(0), max_messages: Some(0), on_demand: true };
    s.set_chat_retention("a", &keep_all).unwrap();
    let two_hours = ChatRetention { max_age_hours: Some(2), max_messages: None, on_demand: true };
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
    let s = store(Retention::unlimited());
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
fn delete_message_clears_its_related_rows() {
    let s = store(Retention::unlimited());
    s.insert_message(&msg("a", "1", 0, "hi")).unwrap();
    s.set_forwarded("a", "1").unwrap();
    s.update_message_content("a", "1", "edited").unwrap();
    s.set_view_once("a", "1", true).unwrap();
    s.record_receipt("1", "them", "read", 10).unwrap();

    s.delete_message("a", "1").unwrap();

    let marks = s.marks("a").unwrap();
    assert!(marks.forwarded.is_empty());
    assert!(marks.edited.is_empty());
    assert!(marks.view_once.is_empty());
    assert!(s.receipts("1").unwrap().is_empty());
}

#[test]
fn edits_replace_text_and_mark_the_message() {
    let s = store(Retention::unlimited());
    s.insert_message(&msg("a", "1", 0, "old")).unwrap();
    assert!(s.update_message_content("a", "1", "new").unwrap());
    assert!(!s.update_message_content("a", "missing", "new").unwrap());
    assert_eq!(s.messages_for("a", 1).unwrap()[0].text, "new");
    assert_eq!(s.marks("a").unwrap().edited, vec!["1".to_string()]);
}

#[test]
fn pings_and_message_search() {
    let s = store(Retention::unlimited());
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
    let s = store(Retention::unlimited());
    s.insert_message(&msg("a@s", "1", 0, "hello")).unwrap();
    let got = s.messages_for("a@s", 10).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].text, "hello");
}

#[test]
fn insert_replaces_same_id() {
    let s = store(Retention::unlimited());
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
    let s = store(Retention::unlimited());
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
    let s = store(Retention::unlimited());
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
    assert_eq!(got.text, "");
}

#[test]
fn audio_duration_survives_a_replay_without_it() {
    let s = store(Retention::unlimited());
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
    let s = store(Retention {
        max_age_hours: Some(24),
        max_messages_per_chat: None,
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
    let s = store(Retention {
        max_age_hours: None,
        max_messages_per_chat: Some(3),
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
    let s = store(Retention {
        max_age_hours: None,
        max_messages_per_chat: Some(2),
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
    let s = store(Retention::unlimited());
    s.insert_message(&msg("old@s", "1", 10, "older")).unwrap();
    s.insert_message(&msg("new@s", "1", 1, "newer")).unwrap();
    let chats = s.chats().unwrap();
    assert_eq!(chats[0].chat, "new@s");
    assert_eq!(chats[0].last_text, "newer");
    assert_eq!(chats[1].chat, "old@s");
}

#[test]
fn names_resolve_in_reads() {
    let s = store(Retention::unlimited());
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
    let s = store(Retention::unlimited());
    s.set_name("a@s", "Alice").unwrap();
    s.set_name("a@s", "   ").unwrap();
    assert_eq!(s.name_for("a@s").unwrap().as_deref(), Some("Alice"));
}

#[test]
fn push_names_replace_a_saved_number() {
    let s = store(Retention::default());
    s.set_saved_name("1@lid", "59899022028").unwrap();
    s.set_name("1@lid", "Ana").unwrap();
    assert_eq!(s.name_for("1@lid").unwrap().as_deref(), Some("Ana"));
    s.set_saved_name("2@lid", "Bea").unwrap();
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
    let s = store(Retention {
        max_age_hours: Some(1),
        max_messages_per_chat: None,
    });
    s.insert_message(&msg("a@s", "older", 72, "hello")).unwrap();
    s.insert_message(&msg("a@s", "old", 48, "hi")).unwrap();
    s.set_name("a@s", "Alice").unwrap();
    s.enforce_retention().unwrap();
    assert_eq!(s.count().unwrap(), 1);
    assert_eq!(s.name_for("a@s").unwrap().as_deref(), Some("Alice"));
}

#[test]
fn quiet_chats_keep_their_newest_message() {
    let s = store(Retention { max_age_hours: Some(24), max_messages_per_chat: None });
    s.insert_message(&msg("quiet@s", "1", 100, "first")).unwrap();
    s.insert_message(&msg("quiet@s", "2", 50, "last word")).unwrap();
    s.insert_message(&msg("busy@s", "1", 50, "old")).unwrap();
    s.insert_message(&msg("busy@s", "2", 1, "new")).unwrap();
    s.enforce_retention().unwrap();
    let chats = s.chats().unwrap();
    assert_eq!(chats.len(), 2, "no chat vanishes from the list");
    assert_eq!(s.messages_for("quiet@s", 9).unwrap()[0].text, "last word");
    assert_eq!(s.messages_for("busy@s", 9).unwrap().len(), 1);
}

#[test]
fn unread_counts_only_incoming_unread() {
    let s = store(Retention::unlimited());
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
fn marking_read_is_idempotent() {
    let s = store(Retention::unlimited());
    s.insert_message(&msg("a@s", "1", 0, "hi")).unwrap();
    assert_eq!(s.mark_read("a@s").unwrap(), 1);
    // Nothing left to change the second time.
    assert_eq!(s.mark_read("a@s").unwrap(), 0);
}

#[test]
fn mark_read_until_only_marks_up_to_the_cutoff() {
    let s = store(Retention::unlimited());
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
fn media_and_reply_fields_round_trip() {
    let s = store(Retention::unlimited());
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

    let s = store(Retention::unlimited());
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
    let s = store(Retention::unlimited());
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
    let s = store(Retention::unlimited());
    s.insert_message(&msg("a@s", "1", 0, "hi")).unwrap();
    assert!(!s.set_delivery_state("a@s", "1", "read").unwrap());
}

#[test]
fn status_by_id_advances_without_the_chat() {
    // Server acks name the message id but only sometimes the chat, and the
    // named JID can differ in form from the stored one. The id alone must
    // still move a pending message to sent.
    let s = store(Retention::unlimited());
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
fn revoking_keeps_the_row_but_clears_content() {
    let s = store(Retention::unlimited());
    s.insert_message(&msg("a@s", "1", 0, "oops")).unwrap();
    assert!(s.revoke_message("a@s", "1").unwrap());

    let got = &s.messages_for("a@s", 1).unwrap()[0];
    assert!(got.local.revoked);
    assert_eq!(got.text, "");
    // Revoking twice changes nothing the second time.
    assert!(!s.revoke_message("a@s", "1").unwrap());
}

#[test]
fn unlimited_retention_keeps_everything() {
    let s = store(Retention::unlimited());
    for i in 0..50 {
        s.insert_message(&msg("a@s", &i.to_string(), i * 100, "x")).unwrap();
    }
    assert_eq!(s.enforce_retention().unwrap(), 0);
    assert_eq!(s.count().unwrap(), 50);
}
