use super::*;

fn store() -> MessageStore {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    migrate(&store.conn.lock().unwrap()).unwrap();
    store
}

fn event(id: &str, kind: GroupAuditKind, timestamp: Option<i64>) -> GroupAuditRecord {
    GroupAuditRecord { chat: "1@g.us".into(), source_id: Some(id.into()), kind,
        actor: None, target: None, old_value: None, new_value: None, old_source: None,
        timestamp, observed_at: 1000, source: GroupAuditSource::Notification, message_id: None }
}

#[test]
fn audit_keeps_unknown_fields_null_and_replays_idempotently_without_creating_chats() {
    let store = store();
    let entry = event("unknown", GroupAuditKind::Join, None);
    assert_eq!(store.record_group_audit(&[entry.clone()]).unwrap(), 1);
    assert_eq!(store.record_group_audit(&[entry]).unwrap(), 0);
    let page = store.group_audit_page(None, &GroupAuditFilter::default()).unwrap();
    let entry = &page.entries[0];
    assert_eq!((&entry.actor, &entry.target, &entry.old_value, &entry.new_value, &entry.old_source, entry.timestamp),
        (&None, &None, &None, &None, &None, None));
    assert_eq!(entry.observed_at, 1000);
    assert!(!entry.jump_available && !page.has_more && page.next_cursor.is_none());
    assert!(store.chats().unwrap().is_empty());
    migrate(&store.conn.lock().unwrap()).unwrap();
    assert_eq!(store.group_audit_page(None, &GroupAuditFilter::default()).unwrap().entries.len(), 1);
}

#[test]
fn audit_retains_out_of_order_facts_without_attaching_later_cached_roles_as_old_values() {
    let store = store();
    let mut later = event("later", GroupAuditKind::Demote, Some(300));
    later.target = Some("123@lid".into());
    later.new_value = Some("member".into());
    store.record_group_audit(&[later]).unwrap();
    let mut earlier = event("earlier", GroupAuditKind::Promote, Some(200));
    earlier.target = Some("123@lid".into());
    earlier.old_value = Some("member".into());
    earlier.old_source = Some(GroupAuditOldSource::Cached);
    earlier.new_value = Some("admin".into());
    store.record_group_audit(&[earlier.clone()]).unwrap();
    let entries = store.group_audit_page(None, &GroupAuditFilter::default()).unwrap().entries;
    assert_eq!(entries.iter().map(|e| e.timestamp).collect::<Vec<_>>(), [Some(300), Some(200)]);
    assert_eq!((entries[1].old_value.clone(), entries[1].old_source), (None, None));
    earlier.source_id = Some("explicit-old".into());
    earlier.old_source = Some(GroupAuditOldSource::Protocol);
    store.record_group_audit(&[earlier]).unwrap();
    let entries = store.group_audit_page(None, &GroupAuditFilter::default()).unwrap().entries;
    assert_eq!(entries[1].old_value.as_deref(), Some("member"));
    assert_eq!(entries[1].old_source, Some(GroupAuditOldSource::Protocol));
}

#[test]
fn audit_filters_and_keyset_paging_handle_tied_and_unknown_timestamps() {
    let store = store();
    let mut entries = Vec::new();
    for (id, timestamp, kind, chat) in [("a", Some(100), GroupAuditKind::Join, "1@g.us"),
        ("b", Some(100), GroupAuditKind::Promote, "1@g.us"), ("c", Some(200), GroupAuditKind::Promote, "2@g.us"),
        ("d", None, GroupAuditKind::Leave, "1@g.us")] {
        let mut entry = event(id, kind, timestamp);
        entry.chat = chat.into();
        entry.actor = Some("9@lid".into());
        entry.target = Some("1@s.whatsapp.net".into());
        entries.push(entry);
    }
    store.record_group_audit(&entries).unwrap();
    let first = store.group_audit_page(None, &GroupAuditFilter { limit: Some(2), ..Default::default() }).unwrap();
    assert!(first.has_more);
    assert_eq!(first.entries.iter().map(|e| e.kind).collect::<Vec<_>>(), [GroupAuditKind::Leave, GroupAuditKind::Promote]);
    let second = store.group_audit_page(None, &GroupAuditFilter { before: first.next_cursor, limit: Some(2), ..Default::default() }).unwrap();
    assert!(!second.has_more);
    assert_eq!(second.entries.iter().map(|e| e.kind).collect::<Vec<_>>(), [GroupAuditKind::Promote, GroupAuditKind::Join]);
    let filter = GroupAuditFilter { kind: Some(GroupAuditKind::Promote), actor: Some("9@lid".into()),
        target: Some("1@s.whatsapp.net".into()), since: Some(100), until: Some(100), ..Default::default() };
    assert_eq!(store.group_audit_page(Some("1@g.us"), &filter).unwrap().entries.len(), 1);
    assert!(store.group_audit_page(Some("wrong@g.us"), &filter).unwrap().entries.is_empty());
    assert!(store.group_audit_page(None, &GroupAuditFilter { since: Some(2), until: Some(1), ..Default::default() }).is_err());
    assert!(store.group_audit_page(Some("1@s.whatsapp.net"), &filter).is_err());
}

#[test]
fn audit_message_jumps_follow_local_rows_and_private_history_stays_hidden() {
    let store = store();
    let mut record = event("delete", GroupAuditKind::MessageDelete, Some(100));
    record.message_id = Some("m".into());
    record.new_value = Some("revoked".into());
    store.record_group_audit(&[record.clone()]).unwrap();
    assert!(!store.group_audit_page(None, &GroupAuditFilter::default()).unwrap().entries[0].jump_available);
    let mut message = StoredMessage::default();
    message.header.chat = "1@g.us".into();
    message.header.id = "m".into();
    message.text = "private body must never be copied into audit".into();
    message.local.revoked = true;
    store.insert_message(&message).unwrap();
    assert!(store.group_audit_page(None, &GroupAuditFilter::default()).unwrap().entries[0].jump_available);
    let conn = store.conn.lock().unwrap();
    let values: String = conn.query_row("SELECT COALESCE(old_value, '') || COALESCE(new_value, '') FROM group_audit", [], |r| r.get(0)).unwrap();
    assert!(!values.contains("private body"));
    drop(conn);
    store.update_message_spoiler("1@g.us", "m", "", true).unwrap();
    assert!(store.group_audit_page(None, &GroupAuditFilter::default()).unwrap().entries.is_empty());
    record.source_id = Some("another".into());
    assert_eq!(store.record_group_audit(&[record]).unwrap(), 0);
    let mut marker = event("view-once", GroupAuditKind::MessagePin, Some(100));
    marker.message_id = Some("only-marker".into());
    store.set_view_once("1@g.us", "only-marker", false).unwrap();
    assert_eq!(store.record_group_audit(&[marker]).unwrap(), 0);
    store.record_group_audit(&[event("metadata", GroupAuditKind::Subject, Some(100))]).unwrap();
    store.delete_chat("1@g.us").unwrap();
    assert!(store.group_audit_page(None, &GroupAuditFilter::default()).unwrap().entries.is_empty());
}

#[test]
fn audit_canonicalizes_actor_target_and_deduplicates_alias_replays() {
    let store = store();
    let mut lid = event("same", GroupAuditKind::Promote, Some(100));
    lid.actor = Some("123@lid".into());
    lid.target = Some("123@lid".into());
    let mut pn = lid.clone();
    pn.target = Some("5989@s.whatsapp.net".into());
    store.record_group_audit(&[lid.clone(), pn]).unwrap();
    store.set_lid_pn("123", "5989").unwrap();
    merge(&store.conn.lock().unwrap(), "123@lid", "5989@s.whatsapp.net").unwrap();
    let entries = store.group_audit_page(None, &GroupAuditFilter { target: Some("123@lid".into()), ..Default::default() }).unwrap().entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].actor.as_deref(), Some("5989@s.whatsapp.net"));
    assert_eq!(entries[0].target.as_deref(), Some("5989@s.whatsapp.net"));
    assert_eq!(store.record_group_audit(&[lid]).unwrap(), 0);
    assert_eq!(store.group_audit_page(None, &GroupAuditFilter { actor: Some("123@lid".into()), ..Default::default() }).unwrap().entries.len(), 1);
    assert_eq!(store.group_audit_page(None, &GroupAuditFilter { member: Some("123@lid".into()), ..Default::default() }).unwrap().entries.len(), 1);
    let independent = MessageStore::open(Path::new(":memory:")).unwrap();
    migrate(&independent.conn.lock().unwrap()).unwrap();
    assert!(independent.group_audit_page(None, &GroupAuditFilter::default()).unwrap().entries.is_empty());
}

#[test]
fn audit_survives_reopen_and_unversioned_adoption() {
    let path = std::env::temp_dir().join(format!("postal-audit-{}-{}.db", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    {
        let store = MessageStore::open(&path).unwrap();
        migrate(&store.conn.lock().unwrap()).unwrap();
        store.record_group_audit(&[event("persisted", GroupAuditKind::Join, Some(100))]).unwrap();
        let conn = store.conn.lock().unwrap();
        conn.pragma_update(None, "user_version", 0).unwrap();
        schema::migrate(&conn).unwrap();
    }
    {
        let store = MessageStore::open(&path).unwrap();
        assert_eq!(store.group_audit_page(None, &GroupAuditFilter::default()).unwrap().entries.len(), 1);
    }
    for suffix in ["", "-wal", "-shm"] { let _ = std::fs::remove_file(format!("{}{suffix}", path.display())); }
}

#[test]
fn explicit_local_chat_clear_discards_only_that_groups_audit() {
    let store = store();
    let first = event("first", GroupAuditKind::Join, Some(100));
    let mut second = event("second", GroupAuditKind::Join, Some(100));
    second.chat = "2@g.us".into();
    store.record_group_audit(&[first, second]).unwrap();
    store.clear_chat("1@g.us").unwrap();
    assert!(store.group_audit_page(Some("1@g.us"), &GroupAuditFilter::default()).unwrap().entries.is_empty());
    assert_eq!(store.group_audit_page(Some("2@g.us"), &GroupAuditFilter::default()).unwrap().entries.len(), 1);
}

#[test]
fn one_source_can_carry_same_kind_facts_for_distinct_referenced_messages() {
    let store = store();
    let mut first = event("combined", GroupAuditKind::MessagePin, Some(100));
    first.message_id = Some("first".into());
    let mut second = first.clone();
    second.message_id = Some("second".into());
    assert_eq!(store.record_group_audit(&[first.clone(), second.clone()]).unwrap(), 2);
    assert_eq!(store.record_group_audit(&[first, second]).unwrap(), 0);
    assert_eq!(store.group_audit_page(None, &GroupAuditFilter::default()).unwrap().entries.len(), 2);
}
