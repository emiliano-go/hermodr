use super::*;

pub(super) fn insert_event_notice(conn: &Connection, header: MessageHeader, target: &str, title: &str, canceled: bool) -> Result<()> {
    anyhow::ensure!(!header.id.trim().is_empty() && !target.trim().is_empty() && header.id != target, "event notice has invalid message identity");
    let collision: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM messages WHERE chat = ?1 AND id = ?2
        AND (COALESCE(system_kind, '') NOT IN ('UNAVAILABLE_MESSAGE', 'EVENT_UPDATED', 'EVENT_CANCELED')
        OR (system_kind IN ('EVENT_UPDATED', 'EVENT_CANCELED') AND COALESCE(system_params, '[]') != ?3)))",
        params![header.chat, header.id, serde_json::to_string(&[target])?], |row| row.get(0))?;
    anyhow::ensure!(!collision, "event notice would replace an ordinary message");
    MessageStore::insert_row(conn, &StoredMessage {
        header,
        text: title.into(),
        local: LocalState { read: true, ..Default::default() },
        system: SystemNotice { kind: Some(if canceled { "EVENT_CANCELED" } else { "EVENT_UPDATED" }.into()), params: vec![target.into()] },
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> MessageStore {
        let store = MessageStore::open(std::path::Path::new(":memory:")).unwrap();
        store.insert_message(&StoredMessage {
            header: MessageHeader { chat: "100@g.us".into(), id: "event".into(), sender: "200@lid".into(), timestamp: 1, from_me: false },
            text: "Original".into(), media: Media { kind: Some("event".into()), ..Default::default() }, ..Default::default()
        }).unwrap();
        store.save_event("100@g.us", "event", "200@lid", &NewEvent { name: "Original".into(), ..Default::default() }, Some(b"synthetic-key")).unwrap();
        store
    }

    fn apply(store: &MessageStore, id: &str, timestamp_ms: i64, canceled: bool) -> Result<bool> {
        store.apply_secret_edit("100@g.us", "event", &["200@lid".into()], &["200@lid".into()],
            &SecretEdit::Event(EventEdit { name: Some("Updated @ home".into()), canceled: Some(canceled), ..Default::default() }),
            &EditRevision { timestamp_ms, message_id: id.into() })
    }

    #[test]
    fn accepted_event_edits_and_cancellations_add_read_notices_once() {
        let store = setup();
        assert!(apply(&store, "edit", 2000, false).unwrap());
        assert!(!apply(&store, "edit", 2000, false).unwrap());
        let updated = store.message("100@g.us", "edit").unwrap();
        assert_eq!(updated.system.kind.as_deref(), Some("EVENT_UPDATED"));
        assert_eq!(updated.system.params, vec!["event"]);
        assert_eq!(updated.text, "Updated @ home");
        assert!(updated.local.read);
        assert_eq!(updated.header.sender, "200@lid");
        let history = StoredMessage { header: updated.header.clone(), system: updated.system.clone(), ..Default::default() };
        assert!(store.insert_history_row(&history).unwrap().is_none());
        assert_eq!(store.message("100@g.us", "edit").unwrap().text, "Updated @ home");
        assert!(store.message("100@g.us", "edit").unwrap().local.read);
        assert!(apply(&store, "cancel", 3000, true).unwrap());
        assert_eq!(store.message("100@g.us", "cancel").unwrap().system.kind.as_deref(), Some("EVENT_CANCELED"));
        assert_eq!(store.count().unwrap(), 3);
        assert!(!apply(&store, "edit", 5000, false).unwrap());
        assert_eq!(store.event_secret("100@g.us", "event").unwrap().unwrap().secret, b"synthetic-key");
        assert!(!store.apply_secret_edit("100@g.us", "event", &["300@lid".into()], &["200@lid".into()],
            &SecretEdit::Event(EventEdit { name: Some("Rejected".into()), ..Default::default() }),
            &EditRevision { timestamp_ms: 4000, message_id: "rejected".into() }).unwrap());
        assert_eq!(store.count().unwrap(), 3);
        assert!(apply(&store, "event", 4000, false).is_err());
        assert_eq!(store.message("100@g.us", "event").unwrap().media.kind.as_deref(), Some("event"));
        store.insert_message(&StoredMessage { header: MessageHeader { chat: "100@g.us".into(), id: "occupied".into(), ..Default::default() }, text: "Ordinary".into(), ..Default::default() }).unwrap();
        assert!(apply(&store, "occupied", 6000, false).is_err());
        assert_eq!(store.message("100@g.us", "occupied").unwrap().text, "Ordinary");
    }

    #[test]
    fn event_notice_failure_rolls_back_content_and_revision_for_retry() {
        let store = setup();
        store.conn.lock().unwrap().execute_batch("CREATE TEMP TRIGGER fail_event_notice BEFORE INSERT ON messages
            WHEN NEW.system_kind IN ('EVENT_UPDATED', 'EVENT_CANCELED') BEGIN SELECT RAISE(FAIL, 'synthetic notice write failure'); END;").unwrap();
        assert!(apply(&store, "edit", 2000, false).is_err());
        assert_eq!(store.message("100@g.us", "event").unwrap().text, "Original");
        assert_eq!(store.count().unwrap(), 1);
        store.conn.lock().unwrap().execute_batch("DROP TRIGGER fail_event_notice").unwrap();
        assert!(apply(&store, "edit", 2000, false).unwrap());
        assert_eq!(store.count().unwrap(), 2);
    }

    #[test]
    fn outbound_event_notice_uses_sent_identity_and_deduplicates_its_echo() {
        let store = setup();
        store.conn.lock().unwrap().execute("UPDATE messages SET from_me = 1 WHERE id = 'event'", []).unwrap();
        let event = NewEvent { name: "Local change".into(), canceled: true, ..Default::default() };
        let revision = EditRevision { timestamp_ms: 2000, message_id: "sent-stanza".into() };
        assert!(store.replace_event_content("100@g.us", "event", &event, &revision).unwrap());
        let notice = store.message("100@g.us", "sent-stanza").unwrap();
        assert!(notice.header.from_me && notice.local.read);
        assert_eq!(notice.system.kind.as_deref(), Some("EVENT_CANCELED"));
        assert_eq!(notice.text, "Local change");
        assert!(!apply(&store, "sent-stanza", 3000, true).unwrap());
        assert_eq!(store.count().unwrap(), 2);
        assert_eq!(store.message("100@g.us", "event").unwrap().text, "Local change");
        let repeated = EditRevision { timestamp_ms: 1500, message_id: "sent-stanza".into() };
        assert!(store.replace_event_content("100@g.us", "event", &event, &repeated).unwrap());
        assert!(!apply(&store, "stale-edit", 1800, false).unwrap());
        assert!(apply(&store, "newer-edit", 4000, false).unwrap());
        assert!(!store.replace_event_content("100@g.us", "event", &event, &revision).unwrap());
        assert_eq!(store.message("100@g.us", "event").unwrap().text, "Updated @ home");
    }

    #[test]
    fn outbound_notice_failure_preserves_original_and_hidden_titles_never_reach_notices() {
        let store = setup();
        store.conn.lock().unwrap().execute_batch("CREATE TEMP TRIGGER fail_outbound_notice BEFORE INSERT ON messages
            WHEN NEW.system_kind = 'EVENT_CANCELED' BEGIN SELECT RAISE(FAIL, 'synthetic outbound notice failure'); END;").unwrap();
        let event = NewEvent { name: "Private title".into(), canceled: true, ..Default::default() };
        let revision = EditRevision { timestamp_ms: 2000, message_id: "sent-stanza".into() };
        assert!(store.replace_event_content("100@g.us", "event", &event, &revision).is_err());
        assert_eq!(store.message("100@g.us", "event").unwrap().text, "Original");
        assert_eq!(store.count().unwrap(), 1);
        store.conn.lock().unwrap().execute_batch("DROP TRIGGER fail_outbound_notice; UPDATE messages SET spoiler = 1 WHERE id = 'event';").unwrap();
        assert!(store.replace_event_content("100@g.us", "event", &event, &revision).unwrap());
        assert!(store.message("100@g.us", "sent-stanza").unwrap().text.is_empty());
        assert!(apply(&store, "remote-edit", 3000, false).unwrap());
        assert!(store.message("100@g.us", "remote-edit").unwrap().text.is_empty());
    }

    #[test]
    fn authored_event_revision_accepts_newer_phone_edits_across_delayed_ack() {
        for phone_before_ack in [false, true] {
            let store = setup();
            let authored = EditRevision { timestamp_ms: 2000, message_id: "sent-stanza".into() };
            let local = NewEvent { name: "Local change".into(), ..Default::default() };
            if phone_before_ack {
                assert!(apply(&store, "phone-edit", 3000, false).unwrap());
                assert!(!store.replace_event_content("100@g.us", "event", &local, &authored).unwrap());
            } else {
                assert!(store.replace_event_content("100@g.us", "event", &local, &authored).unwrap());
                assert!(!apply(&store, "same-second", 2000, false).unwrap());
                assert!(apply(&store, "phone-edit", 3000, false).unwrap());
            }
            assert_eq!(store.message("100@g.us", "event").unwrap().text, "Updated @ home");
            assert_eq!(store.message("100@g.us", "phone-edit").unwrap().system.kind.as_deref(), Some("EVENT_UPDATED"));
        }
    }
}
