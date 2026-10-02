use super::*;

const CHAT: &str = "15550000001@s.whatsapp.net";
const SOURCE: &str = "vote-source";

fn store(retired: bool) -> MessageStore {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    super::super::quiz_polls::migrate_source_retirements(&store.conn.lock().unwrap()).unwrap();
    store.set_lid_pn("777", "15550000001").unwrap();
    let parent = StoredMessage {
        header: MessageHeader {
            chat: CHAT.into(),
            id: "quiz".into(),
            sender: CHAT.into(),
            timestamp: 100,
            ..Default::default()
        },
        media: Media {
            kind: Some("poll".into()),
            ..Default::default()
        },
        text: "Question".into(),
        ..Default::default()
    };
    store.insert_message(&parent).unwrap();
    store
        .save_poll(
            CHAT,
            "quiz",
            CHAT,
            "Question",
            &["A".into(), "B".into()],
            false,
            Some(&[7; 32]),
        )
        .unwrap();
    let header = MessageHeader {
        chat: CHAT.into(),
        id: SOURCE.into(),
        sender: CHAT.into(),
        timestamp: 101,
        ..Default::default()
    };
    store.insert_unavailable(&header).unwrap().unwrap();
    {
        let conn = store.conn.lock().unwrap();
        conn.execute("INSERT INTO quiz_polls(chat,id,original_name,original_options,correct_hash,answer_valid)
            VALUES(?1,'quiz','Question','[\"A\",\"B\"]',NULL,0)", [CHAT]).unwrap();
        conn.execute("INSERT INTO quiz_vote_ciphers(chat,poll,update_id,voter,alt,from_me,source_time,enc_payload,enc_iv,captured_at)
            VALUES(?1,'quiz',?2,?1,NULL,0,3000,X'0102',X'0304',?3)",params![CHAT,SOURCE,whatsapp_rust::wacore::time::now_millis()]).unwrap();
        conn.execute(
            "INSERT INTO quiz_source_retirements(chat,poll,update_id) VALUES(?1,'quiz',?2)",
            params![CHAT, SOURCE],
        )
        .unwrap();
    }
    if retired {
        store.retire_unavailable(CHAT, SOURCE).unwrap().unwrap();
    }
    store
}

fn state(store: &MessageStore) -> (bool, bool, i64, Vec<u8>, Vec<u8>, Option<i64>) {
    let conn = store.conn.lock().unwrap();
    let (deleted, revoked) = conn
        .query_row(
            "SELECT deleted,revoked FROM messages WHERE chat=?1 AND id=?2",
            params![CHAT, SOURCE],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let origins = conn
        .query_row(
            "SELECT COUNT(*) FROM quiz_source_retirements WHERE chat=?1 AND update_id=?2",
            params![CHAT, SOURCE],
            |row| row.get(0),
        )
        .unwrap();
    let (payload,iv,time)=conn.query_row("SELECT enc_payload,enc_iv,source_time FROM quiz_vote_ciphers WHERE chat=?1 AND poll='quiz' AND update_id=?2",
        params![CHAT,SOURCE],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
    (deleted, revoked, origins, payload, iv, time)
}

#[test]
fn deleting_retired_source_revokes_authority_even_when_already_deleted() {
    let store = store(true);
    assert!(state(&store).0);
    store.set_message_deleted("777@lid", SOURCE, true).unwrap();
    assert_eq!(
        state(&store),
        (true, false, 0, Vec::new(), Vec::new(), Some(3000))
    );
    store.set_message_deleted(CHAT, SOURCE, false).unwrap();
    assert_eq!(
        state(&store),
        (false, false, 0, Vec::new(), Vec::new(), Some(3000))
    );
    let cipher = super::super::quiz_polls::QuizCipher {
        update_id: SOURCE.into(),
        voter: CHAT.into(),
        alt: None,
        from_me: false,
        source_time: Some(3000),
        payload: vec![1, 2],
        iv: vec![3, 4],
    };
    assert!(!store.capture_quiz_cipher(CHAT, "quiz", cipher).unwrap());
    assert_eq!(state(&store).3, Vec::<u8>::new());
}

#[test]
fn revoking_retired_source_and_repeated_revoke_always_remove_authority() {
    let store = store(true);
    assert!(store.revoke_message("777@lid", SOURCE).unwrap());
    assert_eq!(
        state(&store),
        (true, true, 0, Vec::new(), Vec::new(), Some(3000))
    );
    {
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO quiz_source_retirements(chat,poll,update_id) VALUES(?1,'quiz',?2)",
            params![CHAT, SOURCE],
        )
        .unwrap();
        conn.execute("UPDATE quiz_vote_ciphers SET enc_payload=X'01',enc_iv=X'02' WHERE chat=?1 AND update_id=?2",params![CHAT,SOURCE]).unwrap();
    }
    assert!(!store.revoke_message(CHAT, SOURCE).unwrap());
    assert_eq!(
        state(&store),
        (true, true, 0, Vec::new(), Vec::new(), Some(3000))
    );
}

#[test]
fn source_revocation_failure_rolls_back_message_flags_and_provenance() {
    for revoke in [false, true] {
        let store = store(false);
        store.conn.lock().unwrap().execute_batch("CREATE TEMP TRIGGER fail_source_redaction BEFORE UPDATE OF enc_payload,enc_iv ON quiz_vote_ciphers
            BEGIN SELECT RAISE(ABORT,'synthetic redaction failure'); END;").unwrap();
        let result = if revoke {
            store.revoke_message(CHAT, SOURCE).map(|_| ())
        } else {
            store.set_message_deleted(CHAT, SOURCE, true)
        };
        assert!(result.is_err());
        assert_eq!(
            state(&store),
            (false, false, 1, vec![1, 2], vec![3, 4], Some(3000))
        );
    }
}

#[test]
fn revoking_missing_source_clears_stale_authority_and_keeps_revoke_tombstone() {
    let store = store(true);
    store
        .conn
        .lock()
        .unwrap()
        .execute(
            "DELETE FROM messages WHERE chat=?1 AND id=?2",
            params![CHAT, SOURCE],
        )
        .unwrap();
    assert!(store.revoke_message(CHAT, SOURCE).unwrap());
    assert_eq!(
        state(&store),
        (true, true, 0, Vec::new(), Vec::new(), Some(3000))
    );
}
