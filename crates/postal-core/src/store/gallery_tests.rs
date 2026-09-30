use super::*;

fn insert(store: &MessageStore, chat: &str, id: &str, timestamp: i64, kind: Option<&str>, sent: bool) {
    store.insert_message(&StoredMessage { header: MessageHeader { chat: chat.into(), id: id.into(), sender: "peer@s".into(), timestamp, from_me: sent },
        media: Media { kind: kind.map(str::to_owned), ..Default::default() }, text: "https://synthetic.test/page".into(), ..Default::default() }).unwrap();
}

#[test]
fn gallery_filters_compose_and_queries_use_ordered_indexes() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    for (n, kind) in ["image", "video", "audio", "document", "sticker", "gif"].into_iter().enumerate() {
        insert(&store, "a@s", kind, 100 + n as i64, Some(kind), n % 2 == 0);
        insert(&store, "b@g.us", kind, 100 + n as i64, Some(kind), n % 2 == 0);
    }
    insert(&store, "a@s", "text-link", 200, None, false);
    insert(&store, "a@s", "view-once", 200, Some("view_once"), false);
    let all = store.gallery_page(&GalleryFilter::default(), None, 60).unwrap();
    assert_eq!(all.items.len(), 12);
    for kind in [GalleryKind::Image, GalleryKind::Video, GalleryKind::Audio, GalleryKind::Document, GalleryKind::Sticker, GalleryKind::Gif] {
        for chat in [None, Some("a@s".into())] {
            let filter = GalleryFilter { chat, kind: Some(kind), ..Default::default() };
            let page = store.gallery_page(&filter, None, 60).unwrap();
            assert_eq!(page.items.len(), if filter.chat.is_some() { 1 } else { 2 });
            assert!(page.items.iter().all(|item| item.message.media.kind.as_deref() == Some(kind.as_str())));
        }
    }
    let composed = GalleryFilter { chat: Some("a@s".into()), kind: Some(GalleryKind::Audio), from_me: Some(true), since: Some(102), until: Some(103) };
    assert_eq!(store.gallery_page(&composed, None, 60).unwrap().items[0].message.header.id, "audio");
    assert!(store.gallery_page(&GalleryFilter { from_me: Some(false), ..composed.clone() }, None, 60).unwrap().items.is_empty());
    assert!(store.gallery_page(&GalleryFilter { since: Some(103), until: Some(103), ..Default::default() }, None, 60).is_err());
    let linked = GalleryFilter { kind: Some(GalleryKind::Link), ..Default::default() };
    assert_eq!(store.gallery_page(&linked, None, 60).unwrap().items.len(), 13);
    for filter in [GalleryFilter::default(), composed, linked.clone(), GalleryFilter { chat: Some("a@s".into()), ..linked }] {
        let (sql, values) = query(&filter, None, 60).unwrap();
        let conn = store.conn.lock().unwrap();
        let plans = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap()
            .query_map(params_from_iter(values), |row| row.get::<_, String>(3)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
        assert!(plans.iter().any(|plan| plan.contains("idx_gallery_")), "{plans:?}");
        assert!(!plans.iter().any(|plan| plan.contains("TEMP B-TREE")), "{plans:?}");
    }
}

#[test]
fn keyset_paging_is_bounded_stable_across_equal_keys_and_deleted_anchors() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    for n in 0..120 {
        for chat in ["a@s", "b@g.us"] { insert(&store, chat, &n.to_string(), 100, Some("image"), false); }
    }
    store.conn.lock().unwrap().execute_batch("UPDATE messages SET sort_order = 1").unwrap();
    let filter = GalleryFilter::default();
    let first = store.gallery_page(&filter, None, u32::MAX).unwrap();
    assert_eq!(first.items.len(), MAX_GALLERY_PAGE as usize);
    let anchor = first.next_cursor.as_ref().unwrap();
    store.conn.lock().unwrap().execute("DELETE FROM messages WHERE chat = ?1 AND id = ?2", params![anchor.chat, anchor.id]).unwrap();
    let mut seen = first.items.iter().map(|item| (item.message.header.chat.clone(), item.message.header.id.clone())).collect::<std::collections::HashSet<_>>();
    let mut cursor = first.next_cursor;
    while let Some(next) = cursor {
        let page = store.gallery_page(&filter, Some(&next), 37).unwrap();
        assert!(page.items.len() <= 37);
        for item in page.items { assert!(seen.insert((item.message.header.chat, item.message.header.id))); }
        cursor = page.next_cursor;
    }
    assert_eq!(seen.len(), 240);
    assert_eq!(store.gallery_page(&filter, None, 0).unwrap().items.len(), 1);
    let message = StoredMessage { header: MessageHeader { chat: "a@s".into(), id: "spoiler".into(), timestamp: 200, ..Default::default() },
        spoiler: true, media: Media { kind: Some("image".into()), thumb: Some("data:image/jpeg;base64,A".into()), ..Default::default() }, ..Default::default() };
    store.insert_message(&message).unwrap();
    assert!(store.gallery_page(&filter, None, 1).unwrap().items[0].message.spoiler);
}

#[test]
fn versioned_url_migration_backfills_batches_and_rolls_back_on_failure() {
    let conn = Connection::open_in_memory().unwrap();
    super::super::schema::migrate_to(&conn, 15).unwrap();
    for n in 0..520 {
        conn.execute("INSERT INTO messages (chat,id,sender,timestamp,from_me,text) VALUES ('a@s',?1,'peer@s',100,0,?2)",
            params![n.to_string(), format!("https://synthetic.test/{n}")]).unwrap();
    }
    // Abort the backfill midway; the migration must roll back the added column
    // and leave the version at 15.
    conn.execute_batch("CREATE TRIGGER fail_link_backfill BEFORE UPDATE ON messages BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END;").unwrap();
    assert!(super::super::schema::migrate_to(&conn, 16).is_err());
    assert_eq!(conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).unwrap(), 15);
    assert!(conn.prepare("SELECT link_urls FROM messages").is_err());
    conn.execute_batch("DROP TRIGGER fail_link_backfill").unwrap();
    super::super::schema::migrate_to(&conn, 16).unwrap();
    let count = conn.query_row("SELECT COUNT(*) FROM messages WHERE link_urls != '[]' AND media_path IS NULL", [], |row| row.get::<_, u32>(0)).unwrap();
    assert_eq!(count, 520);
    super::super::schema::migrate_to(&conn, 16).unwrap();
    assert_eq!(conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).unwrap(), 16);
}

#[test]
fn video_filter_includes_round_video_with_ordered_type_indexes() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    for chat in ["a@s", "b@g.us"] {
        insert(&store, chat, "standard", 100, Some("video"), true);
        insert(&store, chat, "round", 101, Some("round_video"), true);
        insert(&store, chat, "image", 102, Some("image"), true);
    }
    assert_eq!(store.gallery_page(&GalleryFilter::default(), None, 10).unwrap().items.len(), 6);
    for chat in [None, Some("a@s".into())] {
        let filter = GalleryFilter { chat, kind: Some(GalleryKind::Video), from_me: Some(true), since: Some(100), until: Some(102) };
        let page = store.gallery_page(&filter, None, 10).unwrap();
        assert_eq!(page.items.len(), if filter.chat.is_some() { 2 } else { 4 });
        assert_eq!(page.items[0].message.media.kind.as_deref(), Some("round_video"));
        let (sql, values) = query(&filter, None, 10).unwrap();
        let conn = store.conn.lock().unwrap();
        let plans = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap()
            .query_map(params_from_iter(values), |row| row.get::<_, String>(3)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
        assert!(plans.iter().any(|plan| plan.contains("idx_gallery_")), "{plans:?}");
        assert!(!plans.iter().any(|plan| plan.contains("TEMP B-TREE")), "{plans:?}");
    }
}

#[test]
fn alias_duplicate_merge_reindexes_effective_text_and_canonical_gallery_scope() {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    for (chat, text) in [("111@lid", "https://alias.synthetic.test"), ("222@s.whatsapp.net", "https://phone.synthetic.test")] {
        let message = StoredMessage { header: MessageHeader { chat: chat.into(), id: "same".into(), timestamp: 100, ..Default::default() },
            text: text.into(), ..Default::default() };
        store.insert_message(&message).unwrap();
    }
    store.set_lid_pn("111", "222").unwrap();
    let filter = GalleryFilter { chat: Some("111@lid".into()), kind: Some(GalleryKind::Link), ..Default::default() };
    let page = store.gallery_page(&filter, None, 10).unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].message.header.chat, "222@s.whatsapp.net");
    assert_eq!(page.items[0].urls, ["https://alias.synthetic.test"]);
}
