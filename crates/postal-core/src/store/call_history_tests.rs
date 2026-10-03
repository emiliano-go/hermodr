use super::*;

fn store() -> MessageStore {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    migrate(&store.conn.lock().unwrap()).unwrap();
    store
}

fn update(id: &str, revision: i64) -> CallHistoryUpdate {
    CallHistoryUpdate {
        call_id: id.into(), creator_jid: "111@s.whatsapp.net".into(), group_jid: None,
        peer_jids: vec!["333@s.whatsapp.net".into()], from_me: true, is_video: None,
        outcome_raw: None, call_type_raw: None, start_time_raw: None, duration_raw: None,
        mutation_at_ms: revision, from_full_sync: false,
    }
}

#[test]
fn call_capture_replay_is_idempotent_and_older_or_equal_metadata_cannot_replace_record() {
    let store = store();
    let mut first = update("one", 20);
    first.outcome_raw = Some(0); first.is_video = Some(true);
    first.start_time_raw = Some(123); first.duration_raw = Some(91);
    assert!(store.capture_call(&first).unwrap());
    assert!(!store.capture_call(&first).unwrap());
    let mut replay = update("one", 19);
    replay.from_full_sync = true; replay.outcome_raw = Some(4);
    assert!(!store.capture_call(&replay).unwrap());
    replay.mutation_at_ms = 20;
    assert!(!store.capture_call(&replay).unwrap());
    let rows = store.call_history(200).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].outcome, CallOutcome::Connected);
    assert_eq!(rows[0].is_video, Some(true));
    assert_eq!(rows[0].start_time_raw.as_deref(), Some("123"));
    assert_eq!(rows[0].duration_raw.as_deref(), Some("91"));
}

#[test]
fn newer_partial_record_keeps_optional_facts_and_replay_never_adds_a_second_id() {
    let store = store();
    let mut first = update("one", 10);
    first.is_video = Some(false); first.outcome_raw = Some(4); first.duration_raw = Some(0);
    store.capture_call(&first).unwrap();
    let mut newer = update("one", 11);
    newer.peer_jids.clear(); newer.from_full_sync = true;
    assert!(store.capture_call(&newer).unwrap());
    let row = store.call_history(200).unwrap().remove(0);
    assert_eq!(row.peer_jid.as_deref(), Some("333@s.whatsapp.net"));
    assert_eq!(row.is_video, Some(false));
    assert_eq!(row.outcome, CallOutcome::Missed);
    assert_eq!(row.duration_raw.as_deref(), Some("0"));
    assert!(row.from_full_sync);
}

#[test]
fn call_targets_use_canonical_unique_peers_groups_and_incoming_creator() {
    let store = store();
    store.set_lid_pn("444", "333").unwrap();
    let mut outgoing = update("out", 1);
    outgoing.peer_jids.push("444@lid".into());
    store.capture_call(&outgoing).unwrap();
    let mut group = update("group", 2);
    group.group_jid = Some("123@g.us".into());
    group.peer_jids.push("555@lid".into());
    store.capture_call(&group).unwrap();
    let mut incoming = update("in", 3);
    incoming.from_me = false; incoming.creator_jid = "444@lid".into();
    incoming.peer_jids.clear();
    store.capture_call(&incoming).unwrap();
    let rows = store.call_history(200).unwrap();
    assert_eq!(rows[0].chat.as_deref(), Some("333@s.whatsapp.net"));
    assert_eq!(rows[0].creator_jid, "333@s.whatsapp.net");
    assert_eq!(rows[1].chat.as_deref(), Some("123@g.us"));
    assert_eq!(rows[1].group_jid.as_deref(), Some("123@g.us"));
    assert_eq!(rows[2].chat.as_deref(), Some("333@s.whatsapp.net"));
}

#[test]
fn call_read_projection_uses_aliases_learned_after_capture_without_rewriting_raw_clocks() {
    let store = store();
    let mut record = update("late-alias", 1);
    record.creator_jid = "444@lid".into(); record.from_me = false; record.peer_jids.clear();
    record.start_time_raw = Some(17); record.duration_raw = Some(91);
    store.capture_call(&record).unwrap();
    assert_eq!(store.call_history(1).unwrap()[0].chat.as_deref(), Some("444@lid"));
    store.set_lid_pn("444", "333").unwrap();
    let row = store.call_history(1).unwrap().remove(0);
    assert_eq!(row.creator_jid, "333@s.whatsapp.net");
    assert_eq!(row.peer_jid.as_deref(), Some("333@s.whatsapp.net"));
    assert_eq!(row.chat.as_deref(), Some("333@s.whatsapp.net"));
    assert_eq!(row.start_time_raw.as_deref(), Some("17"));
    assert_eq!(row.duration_raw.as_deref(), Some("91"));
}

#[test]
fn ambiguous_missing_and_invalid_group_targets_do_not_guess_a_chat() {
    let store = store();
    let mut missing = update("missing", 1); missing.peer_jids.clear();
    let mut many = update("many", 2); many.peer_jids.push("555@lid".into());
    let mut invalid_group = update("bad-group", 3); invalid_group.group_jid = Some("not-a-group".into());
    for record in [&missing, &many, &invalid_group] { store.capture_call(record).unwrap(); }
    for row in store.call_history(200).unwrap() { assert!(row.chat.is_none()); }
    let mut ambiguous_update = update("missing", 4);
    ambiguous_update.peer_jids.push("555@lid".into());
    store.capture_call(&ambiguous_update).unwrap();
    assert!(store.call_history(200).unwrap()[0].chat.is_none());
}

#[test]
fn all_call_outcomes_preserve_raw_values_and_missing_or_future_enums_stay_unknown() {
    let store = store();
    let expected = [CallOutcome::Connected, CallOutcome::Rejected, CallOutcome::Cancelled,
        CallOutcome::AcceptedElsewhere, CallOutcome::Missed, CallOutcome::Invalid,
        CallOutcome::Unavailable, CallOutcome::Upcoming, CallOutcome::Failed,
        CallOutcome::Abandoned, CallOutcome::Ongoing];
    for (raw, outcome) in expected.into_iter().enumerate() {
        let mut record = update(&raw.to_string(), raw as i64);
        record.outcome_raw = Some(raw as i32); record.call_type_raw = Some(99);
        store.capture_call(&record).unwrap();
        let row = store.call_history(1).unwrap().remove(0);
        assert_eq!(row.outcome, outcome); assert_eq!(row.outcome_raw, Some(raw as i32));
        assert_eq!(row.call_type_raw, Some(99));
    }
    for (id, raw, revision) in [("missing", None, 20), ("future", Some(99), 21)] {
        let mut record = update(id, revision); record.outcome_raw = raw;
        store.capture_call(&record).unwrap();
        let row = store.call_history(1).unwrap().remove(0);
        assert_eq!(row.outcome, CallOutcome::Unknown); assert_eq!(row.outcome_raw, raw);
    }
}

#[test]
fn raw_clock_integers_are_exact_and_never_inferred_from_mutation_time() {
    let store = store();
    let mut record = update("raw", 1000);
    record.start_time_raw = Some(i64::MAX); record.duration_raw = Some(i64::MIN);
    store.capture_call(&record).unwrap();
    store.capture_call(&update("absent", 1001)).unwrap();
    let rows = store.call_history(200).unwrap();
    assert!(rows[0].start_time_raw.is_none()); assert!(rows[0].duration_raw.is_none());
    assert_eq!(rows[1].start_time_raw.as_deref(), Some(i64::MAX.to_string().as_str()));
    assert_eq!(rows[1].duration_raw.as_deref(), Some(i64::MIN.to_string().as_str()));
    let json = serde_json::to_value(&rows[1]).unwrap();
    assert!(json["start_time_raw"].is_string());
    assert!(json.get("started_at").is_none());
}

#[test]
fn conflicting_incoming_identity_or_direction_cannot_overwrite_existing_call() {
    let store = store();
    let mut first = update("one", 1); first.from_me = false;
    store.capture_call(&first).unwrap();
    let mut different = first.clone(); different.creator_jid = "999@lid".into(); different.mutation_at_ms = 2;
    assert!(store.capture_call(&different).is_err());
    different.creator_jid = first.creator_jid.clone(); different.from_me = true;
    assert!(store.capture_call(&different).is_err());
    assert_eq!(store.call_history(200).unwrap()[0].mutation_at_ms, 1);
}

#[test]
fn malformed_capture_bounds_fail_without_writing_and_unknown_video_stays_nullable() {
    let store = store();
    for id in ["", "   ", "bad\nid"] {
        let record = update(id, 1); assert!(store.capture_call(&record).is_err());
    }
    let mut record = update(&"a".repeat(257), 1); assert!(store.capture_call(&record).is_err());
    record = update("bad", -1); assert!(store.capture_call(&record).is_err());
    record = update("bad", 1); record.creator_jid = "123@g.us".into(); assert!(store.capture_call(&record).is_err());
    record = update("bad", 1); record.peer_jids = vec!["333@lid".into(); MAX_CALL_PARTICIPANTS + 1];
    assert!(store.capture_call(&record).is_err()); assert!(store.call_history(200).unwrap().is_empty());
    store.capture_call(&update("valid", 1)).unwrap();
    assert!(store.call_history(0).unwrap()[0].is_video.is_none());
}

#[test]
fn call_reads_are_bounded_without_evicting_stored_records() {
    let store = store();
    store.conn.lock().unwrap().execute_batch(&format!(
        "WITH RECURSIVE entries(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM entries WHERE n<{})
         INSERT INTO call_history(call_id,creator_jid,from_me,mutation_at_ms,from_full_sync)
         SELECT CAST(n AS TEXT),'111@s.whatsapp.net',1,n,0 FROM entries;", MAX_CALL_HISTORY_PAGE + 1
    )).unwrap();
    assert_eq!(store.call_history(u32::MAX).unwrap().len(), MAX_CALL_HISTORY_PAGE as usize);
    assert_eq!(store.call_history(0).unwrap().len(), 1);
    assert_eq!(store.conn.lock().unwrap().query_row("SELECT COUNT(*) FROM call_history", [], |row| row.get::<_, u32>(0)).unwrap(), MAX_CALL_HISTORY_PAGE + 1);
}

#[test]
fn captured_calls_survive_store_restart_without_sdk_or_extra_database() {
    let path = std::env::temp_dir().join(format!("postal-call-capture-{}-{}.db", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    {
        let store = MessageStore::open(&path).unwrap(); migrate(&store.conn.lock().unwrap()).unwrap();
        store.capture_call(&update("persistent", 1)).unwrap();
    }
    let store = MessageStore::open(&path).unwrap(); migrate(&store.conn.lock().unwrap()).unwrap();
    assert_eq!(store.call_history(200).unwrap()[0].call_id, "persistent");
    drop(store);
    for suffix in ["", "-wal", "-shm"] { let _ = std::fs::remove_file(format!("{}{suffix}", path.display())); }
}
