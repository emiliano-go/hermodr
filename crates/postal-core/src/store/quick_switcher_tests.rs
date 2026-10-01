use super::*;

fn memory() -> MessageStore { MessageStore::open(Path::new(":memory:")).unwrap() }

fn insert(store: &MessageStore, chat: &str, id: &str, timestamp: i64, text: &str) {
    store.insert_message(&StoredMessage {
        header: MessageHeader { chat: chat.into(), id: id.into(), sender: "12025550000@s.whatsapp.net".into(), timestamp, from_me: false },
        text: text.into(), ..Default::default()
    }).unwrap();
}

#[test]
fn catalog_is_complete_local_and_keeps_recent_chats_first() {
    let store = memory();
    for index in 0..80 { store.set_saved_name(&format!("1202555{index:04}@s.whatsapp.net"), &format!("Contact {index}")).unwrap(); }
    store.set_name("room@g.us", "Local group").unwrap();
    store.set_name("123@newsletter", "Local channel").unwrap();
    store.set_username("999@lid", "local_username").unwrap();
    insert(&store, "old@g.us", "old", 10, "old");
    insert(&store, "recent@g.us", "recent", 20, "recent");
    store.conn.lock().unwrap().execute("INSERT INTO pins(jid) VALUES ('old@g.us')", []).unwrap();
    let aliases = vec![("888@s.whatsapp.net".into(), "alias-only".into())];
    let groups = vec![("cached@g.us".into(), Some("Cached group".into()))];
    let rows = store.switcher_catalog(&aliases, &groups).unwrap();
    assert!(rows.len() > 80);
    assert_eq!(rows[0].jid, "recent@g.us");
    assert_eq!(rows[1].jid, "old@g.us");
    assert!(rows[0].has_messages);
    assert_eq!(rows.iter().find(|row| row.jid == "123@newsletter").unwrap().kind, "channel");
    assert_eq!(rows.iter().find(|row| row.jid == "cached@g.us").unwrap().name, "Cached group");
    assert_eq!(rows.iter().find(|row| row.jid == "999@lid").unwrap().name, "local_username");
    assert_eq!(rows.iter().find(|row| row.jid == "888@s.whatsapp.net").unwrap().aliases, ["alias-only"]);
}

#[test]
fn catalog_collapses_address_forms_and_keeps_local_name_and_alias_authority() {
    let store = memory();
    store.set_name("777@lid", "Learned LID").unwrap();
    store.set_lid_pn("777", "12025550123").unwrap();
    let aliases = vec![("777@lid".into(), "Zulu".into()), ("12025550123@s.whatsapp.net".into(), "Alpha".into()), ("777@lid".into(), "Alpha".into())];
    let rows = store.switcher_catalog(&aliases, &[]).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].jid, "12025550123@s.whatsapp.net");
    assert_eq!(rows[0].name, "Learned LID");
    assert!(!rows[0].saved);
    assert_eq!(rows[0].aliases, ["Alpha", "Zulu"]);
    store.set_contact_state("777@lid", Some("Saved"), true, 10).unwrap();
    assert_eq!(store.switcher_catalog(&aliases, &[]).unwrap()[0].name, "Saved");
    store.set_push_name("777@lid", "Current push").unwrap();
    store.set_contact_state("777@lid", None, false, 20).unwrap();
    let row = &store.switcher_catalog(&aliases, &[]).unwrap()[0];
    assert_eq!(row.name, "Current push");
    assert!(!row.saved);
}

#[test]
fn global_search_matches_literal_body_and_has_a_stable_cross_chat_order() {
    let store = memory();
    for (chat, id, text) in [("b@g.us", "same", "LITERAL 10%_\\"), ("a@g.us", "same", "literal 10%_\\"), ("a@g.us", "other", "literal 100xx")] {
        insert(&store, chat, id, 50, text);
    }
    store.conn.lock().unwrap().execute_batch("UPDATE messages SET sort_order = 1").unwrap();
    let rows = store.switcher_messages(" 10%_\\ ", 50).unwrap();
    assert_eq!(rows.iter().map(|row| row.header.chat.as_str()).collect::<Vec<_>>(), ["a@g.us", "b@g.us"]);
    assert_eq!(store.switcher_messages("LiTeRaL", 50).unwrap().len(), 3);
    assert!(store.switcher_messages("   ", 50).unwrap().is_empty());
    store.conn.lock().unwrap().execute("UPDATE messages SET link_urls = '[\"https://link-only.invalid\"]' WHERE id = 'other'", []).unwrap();
    assert!(store.switcher_messages("link-only", 50).unwrap().is_empty());
}

#[test]
fn global_search_excludes_private_hidden_and_unavailable_rows() {
    let store = memory();
    for id in ["visible", "deleted", "revoked", "spoiler", "once-kind", "once-table", "unavailable", "notice"] {
        insert(&store, "a@g.us", id, 10, "secret needle");
    }
    insert(&store, "hidden@g.us", "hidden-chat", 20, "secret needle");
    store.conn.lock().unwrap().execute_batch("UPDATE messages SET deleted = 1 WHERE id = 'deleted';
        UPDATE messages SET revoked = 1 WHERE id = 'revoked';
        UPDATE messages SET spoiler = 1 WHERE id = 'spoiler';
        UPDATE messages SET media_kind = 'view_once' WHERE id = 'once-kind';
        INSERT INTO view_once(chat, id, opened) VALUES ('a@g.us', 'once-table', 1);
        UPDATE messages SET system_kind = 'UNAVAILABLE_MESSAGE' WHERE id = 'unavailable';
        UPDATE messages SET system_kind = 'CALL_MISSED_VOICE' WHERE id = 'notice';
        INSERT INTO hidden_chats(jid) VALUES ('hidden@g.us');").unwrap();
    let rows = store.switcher_messages("needle", 50).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].header.id, "visible");
}

#[test]
fn global_search_clamps_limit_at_both_ends() {
    let store = memory();
    for index in 0..65 { insert(&store, "a@g.us", &index.to_string(), index + 1, "needle"); }
    let one = store.switcher_messages("needle", 0).unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].header.id, "64");
    assert_eq!(store.switcher_messages("needle", u32::MAX).unwrap().len(), 50);
}

#[tokio::test]
async fn worker_exposes_the_same_local_catalog_and_global_body_query() {
    let worker = StoreWorker::open(Path::new(":memory:")).await.unwrap();
    worker.run(|store| { insert(store, "room@g.us", "hit", 10, "worker body"); Ok(()) }).await.unwrap();
    assert_eq!(worker.switcher_catalog(Vec::new(), Vec::new()).await.unwrap()[0].jid, "room@g.us");
    assert_eq!(worker.switcher_messages("worker", 50).await.unwrap()[0].header.id, "hit");
}
