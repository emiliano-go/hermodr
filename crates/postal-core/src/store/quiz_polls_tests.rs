use super::*;

const CHAT: &str = "15550000001@s.whatsapp.net";
fn store() -> MessageStore {
    MessageStore::open(Path::new(":memory:")).unwrap()
}
fn parent(store: &MessageStore, id: &str, private: bool) {
    let mut row = StoredMessage::default();
    row.header.chat = CHAT.into();
    row.header.id = id.into();
    row.header.sender = "15550000002@s.whatsapp.net".into();
    row.header.timestamp = 100;
    row.media.kind = Some("poll".into());
    row.text = "Question".into();
    row.spoiler = private;
    store.insert_message(&row).unwrap();
}
fn cipher(id: &str, voter: &str, time: Option<i64>) -> QuizCipher {
    QuizCipher {
        update_id: id.into(),
        voter: voter.into(),
        alt: None,
        from_me: false,
        source_time: time,
        payload: vec![1; 20],
        iv: vec![2; 12],
    }
}
fn definition(store: &MessageStore, id: &str, secret: Option<&[u8]>) -> bool {
    store
        .save_quiz_definition(
            CHAT,
            id,
            "15550000002@s.whatsapp.net",
            "Question",
            &["A".into(), "B".into()],
            Some(&[7; 32]),
            true,
            secret,
        )
        .unwrap()
}

#[test]
fn definitions_require_public_parent_and_replay_changes_only_missing_secret() {
    let store = store();
    assert!(!definition(&store, "missing", Some(&[9; 32])));
    parent(&store, "private", true);
    assert!(!definition(&store, "private", Some(&[9; 32])));
    parent(&store, "public", false);
    assert!(definition(&store, "public", None));
    assert!(!definition(&store, "public", None));
    assert!(definition(&store, "public", Some(&[9; 32])));
    assert!(!definition(&store, "public", Some(&[9; 32])));
    assert_eq!(
        store
            .quiz_definition(CHAT, "public")
            .unwrap()
            .unwrap()
            .secret,
        Some(vec![9; 32])
    );
    store.clear_chat(CHAT).unwrap();
    assert!(store.quiz_definition(CHAT, "public").unwrap().is_none());
}

#[test]
fn ciphers_latest_timestamp_wins_across_aliases_and_sparse_old_writes() {
    let store = store();
    parent(&store, "quiz", false);
    definition(&store, "quiz", Some(&[9; 32]));
    let mut first = cipher("first", "15550000002@s.whatsapp.net", Some(1000));
    first.alt = Some("777@lid".into());
    assert!(store.capture_quiz_cipher(CHAT, "quiz", first).unwrap());
    let mut newer = cipher("newer", "777@lid", Some(2000));
    newer.alt = Some("15550000002@s.whatsapp.net".into());
    assert!(store
        .capture_quiz_cipher(CHAT, "quiz", newer.clone())
        .unwrap());
    assert!(!store.capture_quiz_cipher(CHAT, "quiz", newer).unwrap());
    assert!(!store
        .capture_quiz_cipher(CHAT, "quiz", cipher("old", "777@lid", Some(1500)))
        .unwrap());
    assert!(!store
        .capture_quiz_cipher(CHAT, "quiz", cipher("unknown", "777@lid", None))
        .unwrap());
    let rows = store.quiz_ciphers(CHAT, "quiz").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].update_id, "newer");
    let mut invalid = cipher("invalid", "777@lid", Some(3000));
    invalid.payload.clear();
    assert!(store.capture_quiz_cipher(CHAT, "quiz", invalid).unwrap());
    assert!(store.quiz_ciphers(CHAT, "quiz").unwrap()[0]
        .payload
        .is_empty());
}

#[test]
fn private_and_cleared_targets_reject_ciphers_and_orphans_are_bounded() {
    let store = store();
    parent(&store, "private", true);
    assert!(!store
        .capture_quiz_cipher(
            CHAT,
            "private",
            cipher("v", "15550000002@s.whatsapp.net", Some(2000))
        )
        .unwrap());
    for index in 0..140 {
        store
            .capture_quiz_cipher(
                CHAT,
                &format!("pending-{index}"),
                cipher(
                    &format!("u-{index:03}"),
                    "15550000002@s.whatsapp.net",
                    Some(2000),
                ),
            )
            .unwrap();
    }
    let count: i64 = store
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT count(*) FROM quiz_vote_ciphers", [], |r| r.get(0))
        .unwrap();
    assert!(count <= 128);
    store.clear_chat(CHAT).unwrap();
    assert!(!store
        .capture_quiz_cipher(
            CHAT,
            "missing",
            cipher("late", "15550000002@s.whatsapp.net", Some(3000))
        )
        .unwrap());
}

#[test]
fn private_source_is_excluded_on_read_and_cannot_be_replayed() {
    let store = store();
    parent(&store, "quiz", false);
    definition(&store, "quiz", Some(&[9; 32]));
    let vote = cipher("source-update", "15550000002@s.whatsapp.net", Some(2000));
    assert!(store
        .capture_quiz_cipher(CHAT, "quiz", vote.clone())
        .unwrap());
    assert_eq!(store.quiz_ciphers(CHAT, "quiz").unwrap().len(), 1);
    let mut source = StoredMessage::default();
    source.header.chat = CHAT.into();
    source.header.id = vote.update_id.clone();
    source.header.sender = vote.voter.clone();
    source.header.timestamp = 2;
    source.text = "synthetic source".into();
    source.spoiler = true;
    store.insert_message(&source).unwrap();
    let snapshot = store.quiz_cipher_snapshot(CHAT, "quiz").unwrap();
    assert_eq!(snapshot.records.len(), 1);
    assert!(snapshot.records[0].payload.is_empty());
    assert!(snapshot.records[0].iv.is_empty());
    assert!(snapshot.suppressed);
    assert!(!store.capture_quiz_cipher(CHAT, "quiz", vote).unwrap());
    assert!(store
        .capture_quiz_cipher(
            CHAT,
            "quiz",
            cipher(
                "absent-control-source",
                "15550000003@s.whatsapp.net",
                Some(3000)
            )
        )
        .unwrap());
    assert_eq!(store.quiz_ciphers(CHAT, "quiz").unwrap().len(), 2);
}

fn unavailable_source(store: &MessageStore, id: &str) {
    store
        .insert_unavailable(&MessageHeader {
            chat: CHAT.into(),
            id: id.into(),
            sender: "15550000002@s.whatsapp.net".into(),
            timestamp: 2,
            from_me: false,
        })
        .unwrap()
        .unwrap();
}

#[test]
fn recovered_unavailable_vote_survives_automatic_retirement() {
    let store = store();
    parent(&store, "quiz", false);
    definition(&store, "quiz", Some(&[9; 32]));
    let vote = cipher("recover", "15550000002@s.whatsapp.net", Some(2000));
    assert!(store
        .capture_quiz_cipher(CHAT, "quiz", vote.clone())
        .unwrap());
    unavailable_source(&store, "recover");
    assert!(store
        .capture_quiz_cipher(CHAT, "quiz", vote.clone())
        .unwrap());
    assert!(!store
        .capture_quiz_cipher(CHAT, "quiz", vote.clone())
        .unwrap());
    store.retire_quiz_source(CHAT, "quiz", "recover").unwrap().unwrap();
    let snapshot = store.quiz_cipher_snapshot(CHAT, "quiz").unwrap();
    assert!(!snapshot.suppressed);
    assert_eq!(snapshot.records[0].payload, vote.payload);
}

#[test]
fn user_deleted_unavailable_source_never_gets_retirement_grant() {
    let store = store();
    parent(&store, "quiz", false);
    definition(&store, "quiz", Some(&[9; 32]));
    unavailable_source(&store, "deleted");
    store
        .conn
        .lock()
        .unwrap()
        .execute(
            "UPDATE messages SET deleted=1 WHERE chat=?1 AND id='deleted'",
            [CHAT],
        )
        .unwrap();
    assert!(!store
        .capture_quiz_cipher(
            CHAT,
            "quiz",
            cipher("deleted", "15550000002@s.whatsapp.net", Some(2000))
        )
        .unwrap());
    assert!(store.quiz_ciphers(CHAT, "quiz").unwrap().is_empty());
    let grants: i64 = store
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT count(*) FROM quiz_source_retirements", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(grants, 0);
}

#[test]
fn explicit_source_revoke_removes_retirement_grant_and_blocks_replay() {
    let store = store();
    parent(&store, "quiz", false);
    definition(&store, "quiz", Some(&[9; 32]));
    unavailable_source(&store, "revoked");
    let vote = cipher("revoked", "15550000002@s.whatsapp.net", Some(2000));
    assert!(store
        .capture_quiz_cipher(CHAT, "quiz", vote.clone())
        .unwrap());
    store.retire_unavailable(CHAT, "revoked").unwrap().unwrap();
    revoke_source(&store.conn.lock().unwrap(), CHAT, "revoked").unwrap();
    assert!(!store.capture_quiz_cipher(CHAT, "quiz", vote).unwrap());
    let snapshot = store.quiz_cipher_snapshot(CHAT, "quiz").unwrap();
    assert!(snapshot.suppressed);
    assert!(snapshot.records[0].payload.is_empty());
    assert!(snapshot.records[0].iv.is_empty());
}
