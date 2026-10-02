use super::*;

fn store() -> MessageStore {
    MessageStore::open(Path::new(":memory:")).unwrap()
}

#[test]
fn username_only_contact_is_searchable_without_a_name_or_phone_number() {
    let store = store();
    store.set_username("777@lid", "Only.Handle").unwrap();
    assert!(store.name_for("777@lid").unwrap().is_none());
    assert!(store.contact_identity("777@lid").unwrap().number.is_none());
    assert_eq!(
        store.search_usernames(" @only.hand ", 50).unwrap(),
        vec![("777@lid".into(), "Only.Handle".into())]
    );
    assert!(store.search_usernames("different", 50).unwrap().is_empty());
    assert!(store.search_usernames("@", 50).unwrap().is_empty());
    assert!(store.search_usernames("only", 0).unwrap().is_empty());
}

#[test]
fn username_search_deduplicates_known_aliases_without_overwriting_saved_name() {
    let store = store();
    store.set_lid_pn("777", "15550000001").unwrap();
    store
        .set_saved_name("15550000001@s.whatsapp.net", "Saved name")
        .unwrap();
    store.set_username("777@lid", "server.handle").unwrap();
    assert_eq!(
        store.search_usernames("server", 50).unwrap(),
        vec![("15550000001@s.whatsapp.net".into(), "server.handle".into())]
    );
    let identity = store.contact_identity("777@lid").unwrap();
    assert_eq!(identity.username.as_deref(), Some("server.handle"));
    assert_eq!(identity.legacy_name.as_deref(), Some("Saved name"));
    assert!(store.name_is_saved("15550000001@s.whatsapp.net").unwrap());
    assert_eq!(
        store
            .name_for("15550000001@s.whatsapp.net")
            .unwrap()
            .as_deref(),
        Some("Saved name")
    );
}

#[test]
fn username_substring_search_is_literal_and_excludes_non_individual_rows() {
    let store = store();
    store.set_username("777@lid", "percent%handle").unwrap();
    store.set_username("888@lid", "ordinary").unwrap();
    store.set_username("123@g.us", "percent%handle").unwrap();
    store
        .set_username("status@broadcast", "percent%handle")
        .unwrap();
    assert_eq!(
        store.search_usernames("%", 50).unwrap(),
        vec![("777@lid".into(), "percent%handle".into())]
    );
    assert_eq!(store.search_usernames("", 50).unwrap().len(), 0);
}

#[test]
fn username_search_respects_requested_result_budget() {
    let store = store();
    for lid in ["777", "888", "999"] {
        store
            .set_username(&format!("{lid}@lid"), "matching.handle")
            .unwrap();
    }
    assert_eq!(store.search_usernames("matching", 2).unwrap().len(), 2);
}
