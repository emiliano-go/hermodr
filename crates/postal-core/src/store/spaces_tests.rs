use super::*;

fn store() -> MessageStore {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    migrate(&store.conn.lock().unwrap()).unwrap();
    store
}

fn create(store: &MessageStore, id: &str, parent: Option<&str>) {
    store.spaces_action(SpaceAction::Create { id: id.into(), parent_id: parent.map(str::to_owned),
        name: id.into(), icon: None, color: None }, 100).unwrap();
}

fn add(store: &MessageStore, space: &str, id: &str, target: SpaceTarget) {
    store.spaces_action(SpaceAction::AddItem { id: id.into(), space_id: space.into(), target }, 100).unwrap();
}

fn message(chat: &str, id: &str, text: &str, timestamp: i64) -> StoredMessage {
    StoredMessage { header: MessageHeader { chat: chat.into(), id: id.into(), sender: "99@s.whatsapp.net".into(), timestamp,
        ..Default::default() }, text: text.into(), ..Default::default() }
}

fn resolve(store: &MessageStore, id: &str) -> SpaceResolution {
    store.resolve_spaces(&SpaceSelection::Space { space_id: id.into() }, 100, &[]).unwrap()
}

fn group(jid: &str, parent: Option<&str>, community: bool, announcements: bool) -> CachedSpaceGroup {
    CachedSpaceGroup { jid: jid.into(), subject: Some(jid.into()), parent: parent.map(str::to_owned), community, announcements }
}

#[test]
fn hierarchy_actions_preserve_child_items_reject_cycles_and_require_complete_orders() {
    let store = store();
    for id in ["a", "b", "c"] { create(&store, id, None); }
    create(&store, "d", Some("a")); create(&store, "e", Some("a")); create(&store, "f", Some("d"));
    add(&store, "a", "own", SpaceTarget::Chat { jid: "1@s.whatsapp.net".into() });
    add(&store, "d", "child", SpaceTarget::Chat { jid: "2@s.whatsapp.net".into() });
    let before = store.spaces_snapshot().unwrap();
    for action in [SpaceAction::Reparent { id: "a".into(), parent_id: Some("f".into()) },
        SpaceAction::Rename { id: "b".into(), name: "   ".into() },
        SpaceAction::Reorder { parent_id: None, ids: vec!["b".into()] }] {
        assert!(store.spaces_action(action, 100).is_err());
        assert_eq!(store.spaces_snapshot().unwrap(), before);
    }
    store.spaces_action(SpaceAction::Rename { id: "b".into(), name: " New name ".into() }, 100).unwrap();
    store.spaces_action(SpaceAction::Reorder { parent_id: None, ids: vec!["c".into(), "b".into(), "a".into()] }, 100).unwrap();
    store.spaces_action(SpaceAction::Delete { id: "a".into() }, 100).unwrap();
    let after = store.spaces_snapshot().unwrap();
    assert_eq!(after.spaces.iter().filter(|space| space.parent_id.is_none()).map(|space| space.id.as_str()).collect::<Vec<_>>(), ["c", "b", "d", "e"]);
    assert_eq!(after.spaces.iter().find(|space| space.id == "b").unwrap().name, "New name");
    assert_eq!(after.spaces.iter().find(|space| space.id == "f").unwrap().parent_id.as_deref(), Some("d"));
    assert_eq!(after.items.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(), ["child"]);
    store.spaces_action(SpaceAction::Reparent { id: "e".into(), parent_id: Some("b".into()) }, 100).unwrap();
    assert_eq!(store.spaces_snapshot().unwrap().spaces.iter().find(|space| space.id == "e").unwrap().parent_id.as_deref(), Some("b"));
}

#[test]
fn membership_aliases_and_ordering_are_local_unique_per_space_and_many_to_many() {
    let store = store();
    store.set_lid_pn("77", "1").unwrap();
    for chat in ["1@s.whatsapp.net", "2@s.whatsapp.net"] { store.insert_message(&message(chat, "m", "kept", 10)).unwrap(); }
    create(&store, "a", None); create(&store, "b", None);
    add(&store, "a", "one", SpaceTarget::Chat { jid: "77@lid".into() });
    add(&store, "a", "two", SpaceTarget::Chat { jid: "2@s.whatsapp.net".into() });
    add(&store, "b", "other", SpaceTarget::Chat { jid: "1@s.whatsapp.net".into() });
    let before = store.spaces_snapshot().unwrap();
    for action in [SpaceAction::AddItem { id: "duplicate".into(), space_id: "a".into(), target: SpaceTarget::Chat { jid: "1@s.whatsapp.net".into() } },
        SpaceAction::AddItem { id: "one".into(), space_id: "b".into(), target: SpaceTarget::Chat { jid: "2@s.whatsapp.net".into() } },
        SpaceAction::ReorderItems { space_id: "a".into(), ids: vec!["one".into(), "one".into()] }] {
        assert!(store.spaces_action(action, 100).is_err()); assert_eq!(store.spaces_snapshot().unwrap(), before);
    }
    store.spaces_action(SpaceAction::ReorderItems { space_id: "a".into(), ids: vec!["two".into(), "one".into()] }, 100).unwrap();
    assert_eq!(resolve(&store, "a").chats, ["2@s.whatsapp.net", "1@s.whatsapp.net"]);
    store.spaces_action(SpaceAction::RemoveItem { id: "one".into() }, 100).unwrap();
    assert_eq!(resolve(&store, "b").chats, ["1@s.whatsapp.net"]);
    assert_eq!(store.message("1@s.whatsapp.net", "m").unwrap().text, "kept");
    assert!(!store.message("1@s.whatsapp.net", "m").unwrap().local.deleted);
}

#[test]
fn saved_search_has_no_message_limit_and_excludes_every_private_source() {
    let store = store(); create(&store, "search", None);
    for index in 1..=600 { store.insert_message(&message(&format!("{index}@s.whatsapp.net"), "same-id", "needle_100%", index)).unwrap(); }
    for index in 0..9 {
        let mut row = message(&format!("{}@s.whatsapp.net", 1000 + index), "private", "needle_100%", 1000);
        match index {
            0 => row.local.deleted = true, 1 => row.local.revoked = true, 2 => row.spoiler = true,
            3 => row.media.once_kind = Some("image".into()), 4 => row.media.kind = Some("view_once".into()),
            5 => row.media.kind = Some("unknown".into()), 6 => row.system.kind = Some("UNAVAILABLE_MESSAGE".into()),
            7 => row.system.kind = Some("GROUP_CREATE".into()), _ => {},
        }
        store.insert_message(&row).unwrap();
        if index == 8 { store.conn.lock().unwrap().execute("INSERT INTO hidden_chats(jid) VALUES (?1)", [&row.header.chat]).unwrap(); }
    }
    add(&store, "search", "query", SpaceTarget::SavedSearch { query: "_100%".into(), chat: None });
    assert_eq!(resolve(&store, "search").chats.len(), 600);
    add(&store, "search", "literal", SpaceTarget::SavedSearch { query: "%' OR 1=1 --".into(), chat: None });
    assert!(resolve(&store, "search").items.iter().find(|item| item.item_id == "literal").unwrap().chats.is_empty());
    add(&store, "search", "scoped", SpaceTarget::SavedSearch { query: "needle".into(), chat: Some("2@s.whatsapp.net".into()) });
    assert_eq!(resolve(&store, "search").items.iter().find(|item| item.item_id == "scoped").unwrap().chats, ["2@s.whatsapp.net"]);
    let mut preview = message("9000@s.whatsapp.net", "preview", "No URL in this text", 9000);
    preview.link.url = Some("https://preview-only.synthetic.test".into());
    store.insert_message(&preview).unwrap();
    preview.header.chat = "9001@s.whatsapp.net".into(); preview.local.revoked = true;
    store.insert_message(&preview).unwrap();
    add(&store, "search", "url-only", SpaceTarget::SavedSearch { query: "preview-only.synthetic.test".into(), chat: None });
    assert_eq!(resolve(&store, "search").items.iter().find(|item| item.item_id == "url-only").unwrap().chats, ["9000@s.whatsapp.net"]);
    store.update_message_content("9000@s.whatsapp.net", "preview", "Edited without a link").unwrap();
    assert!(resolve(&store, "search").items.iter().find(|item| item.item_id == "url-only").unwrap().chats.is_empty());
    store.set_label("work", Some("Work team"), None, Some(false), 1).unwrap();
    for (chat, id) in [("2@s.whatsapp.net", "same-id"), ("1001@s.whatsapp.net", "private")] {
        store.set_message_label("work", chat, id, true, 1).unwrap();
    }
    add(&store, "search", "scoped-label", SpaceTarget::SavedSearch { query: "nEEdLE LABEL:\"Work team\"".into(), chat: Some("2@s.whatsapp.net".into()) });
    add(&store, "search", "quoted-label", SpaceTarget::SavedSearch { query: "label:'work TEAM' _100%".into(), chat: Some("2@s.whatsapp.net".into()) });
    add(&store, "search", "missing-label", SpaceTarget::SavedSearch { query: "label:Missing needle".into(), chat: Some("2@s.whatsapp.net".into()) });
    add(&store, "search", "private-label", SpaceTarget::SavedSearch { query: "label:\"Work team\" needle".into(), chat: Some("1001@s.whatsapp.net".into()) });
    store.insert_message(&message("9002@s.whatsapp.net", "literal-label", "literal label:Work", 9002)).unwrap();
    add(&store, "search", "global-label", SpaceTarget::SavedSearch { query: "label:Work".into(), chat: None });
    let resolution = resolve(&store, "search");
    for id in ["scoped-label", "quoted-label"] {
        assert_eq!(resolution.items.iter().find(|item| item.item_id == id).unwrap().chats, ["2@s.whatsapp.net"]);
    }
    assert!(resolution.items.iter().find(|item| item.item_id == "missing-label").unwrap().unavailable.is_some());
    assert!(resolution.items.iter().find(|item| item.item_id == "private-label").unwrap().chats.is_empty());
    assert_eq!(resolution.items.iter().find(|item| item.item_id == "global-label").unwrap().chats, ["9002@s.whatsapp.net"]);
}

#[test]
fn label_resolution_unions_chat_and_public_message_associations_and_keeps_tombstones() {
    let store = store(); create(&store, "labels", None);
    store.set_label("a", Some("Alpha"), None, Some(false), 1).unwrap();
    for chat in ["1@s.whatsapp.net", "2@s.whatsapp.net", "3@s.whatsapp.net"] { store.insert_message(&message(chat, "m", "label source", 10)).unwrap(); }
    store.set_chat_label("a", "1@s.whatsapp.net", true, 1).unwrap();
    store.set_message_label("a", "2@s.whatsapp.net", "m", true, 1).unwrap();
    store.set_message_label("a", "3@s.whatsapp.net", "m", true, 1).unwrap();
    store.conn.lock().unwrap().execute("UPDATE messages SET spoiler=1 WHERE chat='3@s.whatsapp.net'", []).unwrap();
    add(&store, "labels", "label", SpaceTarget::Label { label_id: "a".into() });
    assert_eq!(resolve(&store, "labels").chats.iter().map(String::as_str).collect::<HashSet<_>>(),
        HashSet::from(["1@s.whatsapp.net", "2@s.whatsapp.net"]));
    store.set_label("a", None, None, Some(true), 20).unwrap();
    let resolution = resolve(&store, "labels");
    assert!(resolution.chats.is_empty()); assert!(resolution.items[0].unavailable.is_some());
    assert_eq!(store.spaces_snapshot().unwrap().items.len(), 1);
}

#[test]
fn saved_messages_use_full_message_keys_and_disappear_after_privacy_changes() {
    let store = store(); create(&store, "saved", None);
    for chat in ["1@s.whatsapp.net", "2@s.whatsapp.net"] { store.insert_message(&message(chat, "same", "saved", 1)).unwrap(); }
    for (id, chat) in [("one", "1@s.whatsapp.net"), ("two", "2@s.whatsapp.net")] {
        add(&store, "saved", id, SpaceTarget::SavedMessage { chat: chat.into(), message_id: "same".into() });
    }
    assert_eq!(resolve(&store, "saved").chats.len(), 2);
    store.conn.lock().unwrap().execute("UPDATE messages SET revoked=1 WHERE chat='2@s.whatsapp.net'", []).unwrap();
    let resolution = resolve(&store, "saved");
    assert_eq!(resolution.chats, ["1@s.whatsapp.net"]); assert!(resolution.items[1].unavailable.is_some());
    store.conn.lock().unwrap().execute("INSERT INTO view_once(chat,id,opened) VALUES ('1@s.whatsapp.net','same',0)", []).unwrap();
    assert!(resolve(&store, "saved").chats.is_empty());
    let mut notice = message("100@g.us", "notice", "Public group notice", 10);
    notice.system.kind = Some("GROUP_CREATE".into()); store.insert_message(&notice).unwrap();
    add(&store, "saved", "notice", SpaceTarget::SavedMessage { chat: "100@g.us".into(), message_id: "notice".into() });
    assert_eq!(resolve(&store, "saved").chats, ["100@g.us"]);
    let mut unavailable = message("101@g.us", "missing", "Unavailable", 10);
    unavailable.system.kind = Some("UNAVAILABLE_MESSAGE".into()); store.insert_message(&unavailable).unwrap();
    add(&store, "saved", "unavailable", SpaceTarget::SavedMessage { chat: "101@g.us".into(), message_id: "missing".into() });
    let mut control = message("102@g.us", "control", "", 10);
    control.local.deleted = true; store.insert_message(&control).unwrap();
    add(&store, "saved", "control", SpaceTarget::SavedMessage { chat: "102@g.us".into(), message_id: "control".into() });
    let resolution = resolve(&store, "saved");
    assert_eq!(resolution.chats, ["100@g.us"]);
    assert!(resolution.items.iter().filter(|item| item.item_id != "notice").all(|item| item.unavailable.is_some()));
    assert_eq!(store.spaces_snapshot().unwrap().items.len(), 5);
}

#[test]
fn all_direct_reference_kinds_resolve_existing_local_entities_without_changing_them() {
    let store = store(); create(&store, "refs", None);
    store.set_saved_name("1@s.whatsapp.net", "Known contact").unwrap();
    store.set_saved_name("2@lid", "Known LID contact").unwrap();
    store.insert_message(&message("3@newsletter", "m", "channel content", 10)).unwrap();
    store.cache_space_groups(&[group("4@g.us", None, false, false)]).unwrap();
    for (id, target) in [("chat", SpaceTarget::Chat { jid: "1@s.whatsapp.net".into() }),
        ("contact", SpaceTarget::Contact { jid: "2@lid".into() }),
        ("favorite", SpaceTarget::FavoriteContact { jid: "1@s.whatsapp.net".into() }),
        ("channel", SpaceTarget::Channel { jid: "3@newsletter".into() }),
        ("group", SpaceTarget::Group { jid: "4@g.us".into() }),
        ("missing", SpaceTarget::Chat { jid: "999@s.whatsapp.net".into() })] { add(&store, "refs", id, target); }
    let resolution = resolve(&store, "refs");
    assert_eq!(resolution.chats, ["1@s.whatsapp.net", "2@lid", "3@newsletter", "4@g.us"]);
    assert!(resolution.items.last().unwrap().unavailable.is_some());
    assert_eq!(store.name_for("1@s.whatsapp.net").unwrap().as_deref(), Some("Known contact"));
    assert_eq!(store.message("3@newsletter", "m").unwrap().text, "channel content");
    for target in [SpaceTarget::Channel { jid: "1@g.us".into() }, SpaceTarget::Contact { jid: "1@g.us".into() },
        SpaceTarget::Chat { jid: "1:2@s.whatsapp.net".into() }, SpaceTarget::Chat { jid: "status@broadcast".into() }] {
        assert!(store.spaces_action(SpaceAction::AddItem { id: "bad".into(), space_id: "refs".into(), target }, 100).is_err());
    }
}

#[test]
fn community_cache_and_nested_projection_deduplicate_and_unsorted_uses_dynamic_membership() {
    let store = store(); create(&store, "parent", None); create(&store, "child", Some("parent")); create(&store, "search", None);
    for chat in ["1@s.whatsapp.net", "2@s.whatsapp.net", "3@s.whatsapp.net", "101@g.us", "102@g.us"] {
        store.insert_message(&message(chat, "m", if chat == "2@s.whatsapp.net" { "needle" } else { "plain" }, 10)).unwrap();
    }
    add(&store, "parent", "community", SpaceTarget::Community { jid: "100@g.us".into() });
    assert!(resolve(&store, "parent").items[0].unavailable.is_some());
    let mut groups = vec![group("100@g.us", None, true, false), group("101@g.us", Some("100@g.us"), false, true),
        group("102@g.us", Some("100@g.us"), false, false)];
    groups[0].subject = Some("Community\nAnnouncements\r\nMembers".into());
    store.cache_space_groups(&groups).unwrap();
    store.conn.lock().unwrap().execute("INSERT INTO hidden_chats(jid) VALUES ('102@g.us')", []).unwrap();
    add(&store, "child", "same-group", SpaceTarget::Group { jid: "101@g.us".into() });
    add(&store, "child", "contact", SpaceTarget::Contact { jid: "1@s.whatsapp.net".into() });
    add(&store, "search", "dynamic", SpaceTarget::SavedSearch { query: "needle".into(), chat: None });
    assert_eq!(resolve(&store, "parent").chats, ["101@g.us", "1@s.whatsapp.net"]);
    assert_eq!(store.resolve_spaces(&SpaceSelection::Unsorted, 100, &[]).unwrap().chats, ["3@s.whatsapp.net"]);
    assert_eq!(store.resolve_spaces(&SpaceSelection::All, 100, &[]).unwrap().chats.len(), 4);
    let mut invalid = groups.clone(); invalid[1].parent = Some("101@g.us".into());
    assert!(store.cache_space_groups(&invalid).is_err()); assert_eq!(store.cached_space_groups().unwrap(), groups);
    for subject in ["invalid\0subject".into(), "x".repeat(4097)] {
        let mut invalid = groups.clone(); invalid[0].subject = Some(subject);
        assert!(store.cache_space_groups(&invalid).is_err()); assert_eq!(store.cached_space_groups().unwrap(), groups);
    }
    store.cache_space_groups(&[]).unwrap();
    assert!(resolve(&store, "parent").items[0].unavailable.is_some());
}

#[test]
fn inbox_recipes_match_all_selected_flags_default_to_any_category_and_respect_aliases_and_time() {
    let store = store(); create(&store, "inbox", None);
    store.set_label("a", Some("A"), None, Some(false), 1).unwrap();
    for number in 1..=3 {
        let chat = format!("{number}@s.whatsapp.net");
        let mut row = message(&chat, "m", "plain", 10); row.local.read = number != 1; row.local.mentioned = number == 1;
        store.insert_message(&row).unwrap();
    }
    store.set_chat_label("a", "1@s.whatsapp.net", true, 1).unwrap();
    store.set_chat_label("a", "2@s.whatsapp.net", true, 1).unwrap();
    store.set_muted_until("1@s.whatsapp.net", 101).unwrap(); store.set_archived("1@s.whatsapp.net", true).unwrap();
    add(&store, "inbox", "default", SpaceTarget::InboxView { filters: SpaceInboxFilters::default() });
    let filters = SpaceInboxFilters { unread: true, mentions: true, labelled: true, muted: true, archived: true, label: "a".into(), query: "work".into() };
    add(&store, "inbox", "and", SpaceTarget::InboxView { filters });
    let aliases = vec![("1@s.whatsapp.net".into(), "Work".into())];
    let resolution = store.resolve_spaces(&SpaceSelection::Space { space_id: "inbox".into() }, 100, &aliases).unwrap();
    assert_eq!(resolution.items[0].chats.iter().map(String::as_str).collect::<HashSet<_>>(),
        HashSet::from(["1@s.whatsapp.net", "2@s.whatsapp.net"]));
    assert_eq!(resolution.items[1].chats, ["1@s.whatsapp.net"]);
    assert!(store.resolve_spaces(&SpaceSelection::Space { space_id: "inbox".into() }, 101, &aliases).unwrap().items[1].chats.is_empty());
    store.set_muted_until("1@s.whatsapp.net", -1).unwrap();
    assert_eq!(store.resolve_spaces(&SpaceSelection::Space { space_id: "inbox".into() }, 200, &aliases).unwrap().items[1].chats, ["1@s.whatsapp.net"]);
    add(&store, "inbox", "mentions", SpaceTarget::InboxView { filters: SpaceInboxFilters { mentions: true, ..Default::default() } });
    let selection = SpaceSelection::Space { space_id: "inbox".into() };
    let plain = store.resolve_spaces(&selection, 200, &aliases).unwrap();
    assert_eq!(plain.items.iter().find(|item| item.item_id == "mentions").unwrap().chats, ["1@s.whatsapp.net"]);
    store.set_lid_pn("77", "3").unwrap();
    let counts = HashMap::from([("77@lid".into(), 1), ("2@s.whatsapp.net".into(), 0)]);
    let with_keywords = store.resolve_spaces_with_keywords(&selection, 200, &aliases, &counts).unwrap();
    assert_eq!(with_keywords.items.iter().find(|item| item.item_id == "mentions").unwrap().chats.iter().map(String::as_str).collect::<HashSet<_>>(),
        HashSet::from(["1@s.whatsapp.net", "3@s.whatsapp.net"]));
}

#[test]
fn metadata_import_is_additive_validated_atomic_and_contains_no_account_data() {
    let source = store(); create(&source, "new", None);
    source.insert_message(&message("1@s.whatsapp.net", "m", "NeverCopyMessageText", 1)).unwrap();
    let mut cached = group("1@g.us", None, false, false); cached.subject = Some("NeverCopyCacheSubject".into());
    source.cache_space_groups(&[cached]).unwrap();
    add(&source, "new", "new-item", SpaceTarget::SavedMessage { chat: "1@s.whatsapp.net".into(), message_id: "m".into() });
    let archive = source.export_spaces().unwrap();
    let json = serde_json::to_string(&archive).unwrap();
    assert!(!json.contains("NeverCopy")); assert!(!json.contains("cached_group_catalog"));
    let target = store(); create(&target, "existing", None);
    let before = target.spaces_snapshot().unwrap();
    for invalid in [SpaceArchive { version: 2, ..archive.clone() }, {
        let mut invalid = archive.clone(); invalid.snapshot.spaces[0].parent_id = Some("existing".into()); invalid
    }, { let mut invalid = archive.clone(); invalid.snapshot.items[0].order = 1; invalid }] {
        assert!(target.import_spaces(invalid).is_err()); assert_eq!(target.spaces_snapshot().unwrap(), before);
    }
    target.conn.lock().unwrap().execute_batch("CREATE TEMP TRIGGER reject_space_item BEFORE INSERT ON local_space_items BEGIN SELECT RAISE(ABORT,'synthetic import failure'); END;").unwrap();
    assert!(target.import_spaces(archive.clone()).is_err()); assert_eq!(target.spaces_snapshot().unwrap(), before);
    target.conn.lock().unwrap().execute_batch("DROP TRIGGER reject_space_item").unwrap();
    let merged = target.import_spaces(archive.clone()).unwrap();
    assert_eq!(merged.spaces.iter().map(|space| (space.id.as_str(), space.order)).collect::<Vec<_>>(), [("existing", 0), ("new", 1)]);
    assert!(target.import_spaces(archive).is_err()); assert_eq!(target.spaces_snapshot().unwrap(), merged);
    assert!(target.cached_space_groups().unwrap().is_empty());
    assert!(target.message("1@s.whatsapp.net", "m").is_err());
}

#[test]
fn metadata_and_group_cache_survive_restart_and_backup_without_cross_account_changes() {
    let root = std::env::temp_dir().join(format!("postal-spaces-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&root).unwrap();
    let media = root.join("media"); std::fs::create_dir(&media).unwrap();
    let path = root.join("messages.db");
    let source = MessageStore::open(&path).unwrap(); migrate(&source.conn.lock().unwrap()).unwrap();
    create(&source, "root", None); create(&source, "later", None);
    create(&source, "child", Some("root")); create(&source, "sibling", Some("root")); create(&source, "nested", Some("child"));
    add(&source, "child", "ref", SpaceTarget::SavedSearch { query: "needle".into(), chat: None });
    source.insert_message(&message("1@s.whatsapp.net", "saved", "Conversation belongs in the full backup", 10)).unwrap();
    add(&source, "child", "saved", SpaceTarget::SavedMessage { chat: "1@s.whatsapp.net".into(), message_id: "saved".into() });
    source.spaces_action(SpaceAction::Reorder { parent_id: None, ids: vec!["later".into(), "root".into()] }, 100).unwrap();
    source.spaces_action(SpaceAction::Reorder { parent_id: Some("root".into()), ids: vec!["sibling".into(), "child".into()] }, 100).unwrap();
    source.spaces_action(SpaceAction::ReorderItems { space_id: "child".into(), ids: vec!["saved".into(), "ref".into()] }, 100).unwrap();
    let mut groups = vec![group("100@g.us", None, true, false), group("101@g.us", Some("100@g.us"), false, true)];
    groups[0].subject = Some("Community\nAnnouncements".into());
    source.cache_space_groups(&groups).unwrap();
    source.conn.lock().unwrap().execute_batch("CREATE TABLE synthetic_protocol_credentials(secret TEXT);
        INSERT INTO synthetic_protocol_credentials VALUES ('SPACE_PROTOCOL_SECRET_NEVER_EXPORT');").unwrap();
    std::fs::write(root.join("session.db"), b"SPACE_PROTOCOL_SECRET_NEVER_EXPORT").unwrap();
    let snapshot = source.spaces_snapshot().unwrap(); drop(source);
    let reopened = MessageStore::open(&path).unwrap();
    assert_eq!(reopened.spaces_snapshot().unwrap(), snapshot);
    assert_eq!(reopened.cached_space_groups().unwrap(), groups);
    let isolated = store(); assert!(isolated.spaces_snapshot().unwrap().spaces.is_empty());
    let backup = root.join("backup"); reopened.export_backup(&backup, &media, &[]).unwrap(); drop(reopened);
    assert!(!backup.join("session.db").exists());
    let backup_bytes = std::fs::read(backup.join("messages.db")).unwrap();
    assert!(!backup_bytes.windows(b"SPACE_PROTOCOL_SECRET_NEVER_EXPORT".len()).any(|bytes| bytes == b"SPACE_PROTOCOL_SECRET_NEVER_EXPORT"));
    let restored_path = root.join("restored"); let restored_media = root.join("restored-media");
    super::super::archive::restore_backup(&backup, &restored_path, &restored_media).unwrap();
    let restored = MessageStore::open(&restored_path.join("messages.db")).unwrap();
    assert_eq!(restored.spaces_snapshot().unwrap(), snapshot);
    assert_eq!(restored.cached_space_groups().unwrap(), groups);
    assert_eq!(restored.message("1@s.whatsapp.net", "saved").unwrap().text, "Conversation belongs in the full backup");
    assert!(!restored_path.join("session.db").exists());
    assert_eq!(restored.conn.lock().unwrap().query_row("SELECT COUNT(*) FROM sqlite_master WHERE name='synthetic_protocol_credentials'", [],
        |row| row.get::<_, u32>(0)).unwrap(), 0);
    drop(restored); drop(isolated); std::fs::remove_dir_all(root).unwrap();
}
