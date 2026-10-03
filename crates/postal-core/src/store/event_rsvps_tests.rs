use super::*;

const CHAT: &str = "100@g.us";
const ID: &str = "event";
const WHO: &str = "1@s.whatsapp.net";

fn open() -> MessageStore {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    migrate(&store.conn.lock().unwrap()).unwrap(); seed(&store); store
}

fn seed(store: &MessageStore) {
    store.insert_message(&StoredMessage { header: MessageHeader { chat: CHAT.into(), id: ID.into(), sender: "9@s.whatsapp.net".into(),
        timestamp: 1, ..Default::default() }, text: "Event".into(), media: Media { kind: Some("event".into()), ..Default::default() },
        ..Default::default() }).unwrap();
    store.save_event(CHAT, ID, "9@s.whatsapp.net", &NewEvent { name: "Event".into(), extra_guests_allowed: Some(true),
        is_scheduled_call: Some(true), has_reminder: Some(true), reminder_offset_sec: Some(300), invitation_id: Some("actual-invitation".into()),
        ..Default::default() }, Some(&[7; 32])).unwrap();
}

fn update(response: &str, time: Option<i64>, source: &str, guests: Option<i32>) -> EventRsvpUpdate {
    EventRsvpUpdate { response: response.into(), timestamp_ms: time, source_id: source.into(), extra_guest_count: guests }
}

fn current(store: &MessageStore) -> Option<RowState> {
    read_state(&store.conn.lock().unwrap(), CHAT, ID, WHO).unwrap()
}

#[test]
fn dated_unknown_tombstone_blocks_older_equal_lexical_and_undated_replays() {
    let store = open();
    store.set_event_response(CHAT, ID, WHO, "going").unwrap();
    assert!(store.apply_event_rsvp(CHAT, ID, WHO, &update("maybe", Some(3000), "z-first", None)).unwrap());
    for change in [update("going", Some(2000), "new-id", None), update("not_going", Some(3000), "a-lexically-first", None),
        update("going", None, "undated", None), update("maybe", Some(3000), "z-first", None)] {
        assert!(!store.apply_event_rsvp(CHAT, ID, WHO, &change).unwrap());
    }
    assert_eq!(current(&store).unwrap().response, "maybe");
    assert!(store.apply_event_rsvp(CHAT, ID, WHO, &update("future-enum", Some(4000), "unknown", Some(2))).unwrap());
    assert!(store.marks(CHAT).unwrap().events[0].responses.is_empty());
    assert!(!store.apply_event_rsvp(CHAT, ID, WHO, &update("going", Some(3500), "older", None)).unwrap());
    store.set_event_response(CHAT, ID, WHO, "going").unwrap();
    assert_eq!(current(&store).unwrap().response, "");
    assert_eq!(current(&store).unwrap().watermark_ms, Some(4000));
}

#[test]
fn late_ack_cas_and_private_bound_preserve_newer_authenticated_state_without_fake_wire_time() {
    let store = open(); let empty = store.event_rsvp_context(CHAT, ID, WHO).unwrap().unwrap();
    store.apply_event_rsvp(CHAT, ID, WHO, &update("maybe", Some(1500), "incoming", None)).unwrap();
    assert!(!store.commit_event_rsvp_ack_with_bound(CHAT, ID, WHO, &empty.prior, "going", None, "late", Some(1000)).unwrap());
    let capture = store.event_rsvp_context(CHAT, ID, WHO).unwrap().unwrap();
    assert!(store.commit_event_rsvp_ack_with_bound(CHAT, ID, WHO, &capture.prior, "not_going", None, "own", Some(2000)).unwrap());
    let own = current(&store).unwrap();
    assert_eq!(own.timestamp_ms, None); assert_eq!(own.watermark_ms, Some(1500)); assert_eq!(own.ack_lower_bound_ms, Some(2000));
    assert!(!store.apply_event_rsvp(CHAT, ID, WHO, &update("going", Some(1800), "certainly-older", None)).unwrap());
    assert!(store.apply_event_rsvp(CHAT, ID, WHO, &update("not_going", Some(2000), "own", None)).unwrap());
    assert_eq!(current(&store).unwrap().timestamp_ms, Some(2000));
    assert!(!store.apply_event_rsvp(CHAT, ID, WHO, &update("going", Some(2000), "other-same-ms", None)).unwrap());
    assert!(!store.commit_event_rsvp_ack(CHAT, ID, WHO, &capture.prior, "going", None, "stale-again").unwrap());
}

#[test]
fn alias_reconciliation_canonicalizes_chat_and_responder_before_collision_loss_and_tolerates_old_schema() {
    let old = Connection::open_in_memory().unwrap();
    old.execute_batch("CREATE TABLE event_responses(chat TEXT,event TEXT,responder TEXT,response TEXT)").unwrap();
    reconcile_event_responders(&old).unwrap();
    let store = open();
    {
        let conn = store.conn.lock().unwrap();
        conn.execute_batch("INSERT INTO lid_pn(lid,pn) VALUES('77','7'),('88','8');
            INSERT INTO event_responses(chat,event,responder,response,timestamp_ms,watermark_ms,source_id,revision)
            VALUES('77@lid','same','88@lid','going',1000,1000,'z-old',1),
            ('7@s.whatsapp.net','same','8@s.whatsapp.net','maybe',2000,2000,'a-new',2);").unwrap();
        reconcile_event_responders(&conn).unwrap();
        let winner = read_state(&conn, "7@s.whatsapp.net", "same", "8@s.whatsapp.net").unwrap().unwrap();
        assert_eq!(winner.response, "maybe");
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM event_responses WHERE event='same'", [], |row| row.get::<_, u32>(0)).unwrap(), 1);
    }
    store.set_lid_pn("11", "1").unwrap();
    store.apply_event_rsvp(CHAT, ID, "11@lid", &update("going", Some(3000), "first", Some(1))).unwrap();
    assert!(!store.apply_event_rsvp(CHAT, ID, WHO, &update("maybe", Some(3000), "second", None)).unwrap());
    assert_eq!(store.marks(CHAT).unwrap().events[0].responses[0].responder, WHO);
}

#[test]
fn guests_are_preserved_for_authenticated_history_but_outgoing_permissions_are_current() {
    let store = open();
    store.save_event(CHAT, ID, "9@s.whatsapp.net", &NewEvent { name: "Event".into(), extra_guests_allowed: Some(false),
        ..Default::default() }, None).unwrap();
    assert!(store.apply_event_rsvp(CHAT, ID, WHO, &update("going", Some(1000), "past-permission", Some(3))).unwrap());
    let context = store.event_rsvp_context(CHAT, ID, WHO).unwrap().unwrap(); assert!(!context.extra_guests_allowed);
    assert!(!store.commit_event_rsvp_ack(CHAT, ID, WHO, &context.prior, "going", Some(1), "not-allowed").unwrap());
    assert!(store.apply_event_rsvp(CHAT, ID, WHO, &update("going", Some(2000), "later", Some(4))).unwrap());
    let marks = store.marks(CHAT).unwrap(); let response = &marks.events[0].responses[0];
    assert_eq!(response.extra_guest_count, Some(4)); assert_eq!(response.timestamp_ms, Some(2000));
    assert!(store.apply_event_rsvp(CHAT, ID, WHO, &update("going", Some(3000), "negative", Some(-1))).is_err());
    assert_eq!(current(&store).unwrap().source_id.as_deref(), Some("later"));
    store.save_event(CHAT, ID, "9@s.whatsapp.net", &NewEvent { name: "Event".into(), extra_guests_allowed: Some(true),
        ..Default::default() }, None).unwrap();
    let context = store.event_rsvp_context(CHAT, ID, WHO).unwrap().unwrap();
    assert!(store.commit_event_rsvp_ack(CHAT, ID, WHO, &context.prior, "maybe", Some(2), "maybe-guests").unwrap());
    let own = current(&store).unwrap(); assert_eq!(own.response, "maybe"); assert_eq!(own.extra_guest_count, Some(2));
    assert_eq!(own.timestamp_ms, None);
}

#[test]
fn canceled_and_invitation_contexts_remain_readable_with_outgoing_disabled_and_historical_answers_loaded() {
    let store = open();
    for (canceled, invitation) in [(true, false), (false, true)] {
        store.conn.lock().unwrap().execute("UPDATE events SET canceled=?1,invitation=?2 WHERE chat=?3 AND id=?4",
            params![canceled, invitation, CHAT, ID]).unwrap();
        let context = store.event_rsvp_context(CHAT, ID, WHO).unwrap().unwrap();
        assert_eq!((context.canceled, context.invitation), (canceled, invitation));
        assert!(!store.commit_event_rsvp_ack(CHAT, ID, WHO, &context.prior, "going", None, "own").unwrap());
        assert!(!store.marks(CHAT).unwrap().events[0].can_respond);
        assert!(store.apply_event_rsvp(CHAT, ID, WHO, &update("going", Some(if canceled { 1000 } else { 2000 }),
            if canceled { "historical" } else { "invite-history" }, None)).unwrap());
    }
    store.conn.lock().unwrap().execute("UPDATE events SET canceled=0,invitation=0,invitation_id=NULL", []).unwrap();
    assert!(store.marks(CHAT).unwrap().events[0].can_respond);
}

#[test]
fn private_parent_variants_never_admit_responses_contexts_or_own_ack() {
    let store = open();
    for sql in ["UPDATE messages SET deleted=1", "UPDATE messages SET revoked=1", "UPDATE messages SET spoiler=1",
        "UPDATE messages SET system_kind='UNAVAILABLE_MESSAGE'", "UPDATE messages SET media_once_kind='event'",
        "INSERT INTO view_once(chat,id) VALUES('100@g.us','event')", "INSERT INTO hidden_chats(jid) VALUES('100@g.us')"] {
        store.conn.lock().unwrap().execute_batch(sql).unwrap();
        assert!(!store.apply_event_rsvp(CHAT, ID, WHO, &update("going", Some(1000), "private", None)).unwrap());
        assert!(store.event_rsvp_context(CHAT, ID, WHO).unwrap().is_none());
        assert!(!store.commit_event_rsvp_ack(CHAT, ID, WHO, &None, "going", None, "private-ack").unwrap());
        assert!(!store.marks(CHAT).unwrap().events[0].can_respond);
        store.conn.lock().unwrap().execute_batch("UPDATE messages SET deleted=0,revoked=0,spoiler=0,system_kind=NULL,media_once_kind=NULL;
            DELETE FROM view_once; DELETE FROM hidden_chats;").unwrap();
    }
    store.conn.lock().unwrap().execute("UPDATE events SET secret=NULL", []).unwrap();
    assert!(!store.marks(CHAT).unwrap().events[0].can_respond);
    assert!(store.event_rsvp_context(CHAT, ID, WHO).unwrap().is_none());
}

#[test]
fn metadata_survives_sparse_encrypted_replacement_and_specific_pin_expiry_is_known_positive_only() {
    let store = open();
    let revision = super::super::secret_edits::EditRevision { timestamp_ms: 2000, message_id: "encrypted-edit".into() };
    assert!(store.replace_event_content(CHAT, ID, &NewEvent { name: "Edited".into(), ..Default::default() }, &revision).unwrap());
    let context = store.event_rsvp_context(CHAT, ID, WHO).unwrap().unwrap();
    assert_eq!(context.event.has_reminder, Some(true)); assert_eq!(context.event.reminder_offset_sec, Some(300));
    assert_eq!(context.invitation_id.as_deref(), Some("actual-invitation")); assert_eq!(context.event.is_scheduled_call, Some(true));
    store.apply_message_pin_update(CHAT, &super::super::history_pins::MessagePinUpdate {
        target: ID.into(), remote: None, pinned: true, timestamp: 10, expires_at: None }, false).unwrap();
    assert!(store.marks(CHAT).unwrap().events[0].pinned);
    store.apply_message_pin_update(CHAT, &super::super::history_pins::MessagePinUpdate {
        target: ID.into(), remote: None, pinned: true, timestamp: 20, expires_at: Some(1) }, false).unwrap();
    assert!(!store.marks(CHAT).unwrap().events[0].pinned);
}

#[test]
fn clearing_parent_removes_response_state_and_rejects_pre_clear_dated_ack() {
    let store = open(); store.apply_event_rsvp(CHAT, ID, WHO, &update("going", Some(1000), "first", None)).unwrap();
    let prior = store.event_rsvp_context(CHAT, ID, WHO).unwrap().unwrap().prior;
    store.clear_chat(CHAT).unwrap();
    assert!(current(&store).is_none()); assert!(store.marks(CHAT).unwrap().events.is_empty());
    assert!(!store.commit_event_rsvp_ack(CHAT, ID, WHO, &prior, "maybe", None, "late").unwrap());
    seed(&store);
    assert!(!store.commit_event_rsvp_ack(CHAT, ID, WHO, &prior, "maybe", None, "late-restored").unwrap());
}

#[test]
fn failed_response_write_rolls_back_clock_and_existing_winner() {
    let store = open(); store.apply_event_rsvp(CHAT, ID, WHO, &update("going", Some(1000), "original", None)).unwrap();
    let before = current(&store).unwrap();
    let clock = store.conn.lock().unwrap().query_row("SELECT value FROM event_rsvp_clock WHERE id=1", [], |row| row.get::<_, i64>(0)).unwrap();
    store.conn.lock().unwrap().execute_batch("CREATE TEMP TRIGGER fail_rsvp BEFORE UPDATE ON event_responses
        BEGIN SELECT RAISE(ABORT,'synthetic RSVP failure'); END;").unwrap();
    assert!(store.apply_event_rsvp(CHAT, ID, WHO, &update("maybe", Some(2000), "new", Some(2))).is_err());
    assert_eq!(current(&store).unwrap(), before);
    assert_eq!(store.conn.lock().unwrap().query_row("SELECT value FROM event_rsvp_clock WHERE id=1", [], |row| row.get::<_, i64>(0)).unwrap(), clock);
}

#[test]
fn late_secret_fill_preserves_edited_metadata_revisions_and_guards_creator_privacy_and_invitation() {
    let store = open();
    let revision = super::super::secret_edits::EditRevision { timestamp_ms: 2000, message_id: "latest-edit".into() };
    store.replace_event_content(CHAT, ID, &NewEvent { name: "Edited canceled event".into(), canceled: true,
        extra_guests_allowed: Some(false), ..Default::default() }, &revision).unwrap();
    store.conn.lock().unwrap().execute("UPDATE events SET secret=NULL", []).unwrap();
    let before = serde_json::to_value(store.marks(CHAT).unwrap()).unwrap();
    let message = store.message(CHAT, ID).unwrap();
    let count = store.conn.lock().unwrap().query_row("SELECT COUNT(*) FROM messages", [], |row| row.get::<_, u32>(0)).unwrap();
    let creators = vec!["9@s.whatsapp.net".into()];
    assert!(!store.fill_event_secret(CHAT, ID, &["other@s.whatsapp.net".into()], &[8; 32]).unwrap());
    assert!(store.fill_event_secret(CHAT, ID, &creators, &[8; 32]).unwrap());
    assert_eq!(serde_json::to_value(store.marks(CHAT).unwrap()).unwrap(), before);
    assert_eq!(store.message(CHAT, ID).unwrap().text, message.text);
    assert_eq!(store.conn.lock().unwrap().query_row("SELECT COUNT(*) FROM messages", [], |row| row.get::<_, u32>(0)).unwrap(), count);
    let context = store.event_rsvp_context(CHAT, ID, WHO).unwrap().unwrap();
    assert!(context.canceled); assert_eq!(context.secret.secret, vec![8; 32]);
    assert_eq!(context.event.extra_guests_allowed, Some(false)); assert_eq!(context.event.has_reminder, Some(true));
    assert!(!store.fill_event_secret(CHAT, ID, &creators, &[9; 32]).unwrap());
    assert_eq!(store.conn.lock().unwrap().query_row("SELECT message_id FROM secret_edit_revisions WHERE chat=?1 AND id=?2",
        params![CHAT, ID], |row| row.get::<_, String>(0)).unwrap(), "latest-edit");
    assert!(store.fill_event_secret(CHAT, ID, &creators, &[1; 31]).is_err());
    for sql in ["UPDATE messages SET deleted=1", "UPDATE messages SET revoked=1", "UPDATE messages SET spoiler=1",
        "UPDATE events SET invitation=1"] {
        store.conn.lock().unwrap().execute_batch("UPDATE events SET secret=X'01',invitation=0;
            UPDATE messages SET deleted=0,revoked=0,spoiler=0;").unwrap();
        store.conn.lock().unwrap().execute_batch(sql).unwrap();
        assert!(!store.fill_event_secret(CHAT, ID, &creators, &[9; 32]).unwrap());
    }
    store.conn.lock().unwrap().execute_batch("UPDATE events SET invitation=0; UPDATE messages SET deleted=0,revoked=0,spoiler=0;").unwrap();
    assert!(store.fill_event_secret(CHAT, ID, &creators, &[9; 32]).unwrap());
    assert_eq!(store.message(CHAT, ID).unwrap().text, "Edited canceled event");
}

#[test]
fn response_order_metadata_and_clock_survive_restart_and_full_backup() {
    let root = std::env::temp_dir().join(format!("postal-event-rsvps-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&root).unwrap(); let media = root.join("media"); std::fs::create_dir(&media).unwrap();
    let path = root.join("messages.db"); let store = MessageStore::open(&path).unwrap(); migrate(&store.conn.lock().unwrap()).unwrap(); seed(&store);
    store.apply_event_rsvp(CHAT, ID, WHO, &update("future", Some(9000), "barrier", Some(2))).unwrap();
    let state = current(&store).unwrap(); drop(store);
    let reopened = MessageStore::open(&path).unwrap(); assert_eq!(current(&reopened).unwrap(), state);
    assert!(!reopened.apply_event_rsvp(CHAT, ID, WHO, &update("going", Some(8000), "old", None)).unwrap());
    let backup = root.join("backup"); reopened.export_backup(&backup, &media, &[]).unwrap(); drop(reopened);
    let account = root.join("restored"); let restored_media = root.join("restored-media");
    super::super::archive::restore_backup(&backup, &account, &restored_media).unwrap();
    let restored = MessageStore::open(&account.join("messages.db")).unwrap(); assert_eq!(current(&restored).unwrap(), state);
    assert_eq!(restored.event_rsvp_context(CHAT, ID, WHO).unwrap().unwrap().event.has_reminder, Some(true));
    assert!(restored.apply_event_rsvp(CHAT, ID, WHO, &update("maybe", Some(10000), "new", None)).unwrap());
    assert!(current(&restored).unwrap().revision > state.revision);
    drop(restored); std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn rsvp_validation_preserves_typed_reason_and_existing_state() {
    let store = open();
    let error = store.fill_event_secret(CHAT, ID, &[WHO.into()], &[7; 31]).unwrap_err();
    let reference = error.downcast_ref::<MessageRef>().unwrap();
    assert_eq!(reference.code, "error.event_secret_length");
    assert_eq!(serde_json::to_value(reference).unwrap()["params"]["expected_bytes"], 32);
    let error = store.apply_event_rsvp(CHAT, ID, WHO, &update("going", Some(-1), "source", None)).unwrap_err();
    assert_eq!(error.downcast_ref::<MessageRef>().unwrap().code, "error.event_timestamp_guests");
    assert!(current(&store).is_none());
    assert_eq!(token_id("").unwrap_err().downcast_ref::<MessageRef>().unwrap().code, "error.event_source_id");
}
