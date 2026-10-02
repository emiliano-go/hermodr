use super::*;
use quiz_polls::QuizCipher;

#[test]
fn quiz_state_follows_aliases_and_retention_clear_boundaries() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    let mut parent = StoredMessage::default();
    parent.header.chat = "777@lid".into();
    parent.header.id = "quiz".into();
    parent.header.sender = "200@s.whatsapp.net".into();
    parent.header.timestamp = 100;
    parent.media.kind = Some("poll".into());
    parent.text = "Question".into();
    store.insert_message(&parent).unwrap();
    store
        .save_quiz_definition(
            "777@lid",
            "quiz",
            "200@s.whatsapp.net",
            "Question",
            &["A".into(), "B".into()],
            Some(&[7; 32]),
            true,
            Some(&[9; 32]),
        )
        .unwrap();
    let mut cipher = QuizCipher {
        update_id: "vote".into(),
        voter: "300@s.whatsapp.net".into(),
        alt: None,
        from_me: false,
        source_time: Some(100_000),
        payload: vec![1; 20],
        iv: vec![2; 12],
    };
    store
        .capture_quiz_cipher("777@lid", "quiz", cipher.clone())
        .unwrap();
    store.set_lid_pn("777", "100").unwrap();
    let chat = "100@s.whatsapp.net";
    assert!(store.quiz_definition(chat, "quiz").unwrap().is_some());
    assert_eq!(store.quiz_ciphers(chat, "quiz").unwrap().len(), 1);
    assert_eq!(
        store
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM quiz_polls WHERE chat='777@lid'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    parent.header.chat = chat.into();
    parent.header.id = "legacy".into();
    store.insert_message(&parent).unwrap();
    store
        .save_poll(
            chat,
            "legacy",
            "200@s.whatsapp.net",
            "Question",
            &["A".into(), "B".into()],
            false,
            Some(&[9; 32]),
        )
        .unwrap();
    cipher.update_id = "legacy-vote".into();
    store
        .capture_quiz_cipher(chat, "legacy", cipher.clone())
        .unwrap();
    assert!(store.quiz_definition(chat, "legacy").unwrap().is_none());
    DiskRetentionManager::new(DiskRetention {
        max_age_hours: RetentionLimit::Unlimited,
        max_messages_per_chat: RetentionLimit::Limited(0),
    })
    .enforce(&store)
    .unwrap();
    let counts = || {
        store.conn.lock().unwrap().query_row(
        "SELECT (SELECT count(*) FROM quiz_polls), (SELECT count(*) FROM quiz_vote_ciphers)", [],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))).unwrap()
    };
    assert_eq!(counts(), (0, 0));
    parent.header.id = "again".into();
    store.insert_message(&parent).unwrap();
    store
        .save_quiz_definition(
            chat,
            "again",
            "200@s.whatsapp.net",
            "Question",
            &["A".into(), "B".into()],
            Some(&[7; 32]),
            true,
            Some(&[9; 32]),
        )
        .unwrap();
    cipher.update_id = "again-vote".into();
    store.capture_quiz_cipher(chat, "again", cipher).unwrap();
    store.clear_history().unwrap();
    assert_eq!(counts(), (0, 0));
}

#[test]
fn quiz_private_source_pruning_keeps_redacted_latest_marker_and_rejects_replay() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    let chat = "100@s.whatsapp.net";
    let mut parent = StoredMessage::default();
    parent.header.chat = chat.into();
    parent.header.id = "quiz".into();
    parent.header.sender = "200@s.whatsapp.net".into();
    parent.header.timestamp = 200;
    parent.media.kind = Some("poll".into());
    parent.text = "Question".into();
    store.insert_message(&parent).unwrap();
    store
        .save_quiz_definition(
            chat,
            "quiz",
            "200@s.whatsapp.net",
            "Question",
            &["A".into(), "B".into()],
            Some(&[7; 32]),
            true,
            Some(&[9; 32]),
        )
        .unwrap();
    let vote = QuizCipher {
        update_id: "vote".into(),
        voter: "300@s.whatsapp.net".into(),
        alt: None,
        from_me: true,
        source_time: Some(100_000),
        payload: vec![1; 20],
        iv: vec![2; 12],
    };
    store
        .capture_quiz_cipher(chat, "quiz", vote.clone())
        .unwrap();
    let mut source = StoredMessage::default();
    source.header.chat = chat.into();
    source.header.id = "vote".into();
    source.header.timestamp = 100;
    source.text = "Synthetic private source".into();
    source.spoiler = true;
    store.insert_message(&source).unwrap();
    DiskRetentionManager::new(DiskRetention {
        max_age_hours: RetentionLimit::Unlimited,
        max_messages_per_chat: RetentionLimit::Limited(1),
    })
    .enforce(&store)
    .unwrap();
    assert!(store.message(chat, "vote").is_err());
    let markers = store.quiz_ciphers(chat, "quiz").unwrap();
    assert_eq!(markers.len(), 1);
    assert!(markers[0].payload.is_empty() && markers[0].iv.is_empty());
    assert!(!store.capture_quiz_cipher(chat, "quiz", vote).unwrap());
    assert!(store.quiz_ciphers(chat, "quiz").unwrap()[0]
        .payload
        .is_empty());
}
