use super::*;

fn store() -> MessageStore {
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    super::super::sticker_sync::migrate(&store.conn.lock().unwrap()).unwrap();
    store
}

#[test]
fn favorite_clock_survives_sparse_metadata_and_rejects_old_changes() {
    let store = store();
    assert!(store.observe_sticker_favorite("hash", true, 2000).unwrap());
    assert!(!store.observe_sticker_favorite("hash", false, 1999).unwrap());
    assert!(store.observe_sticker_favorite("hash", false, 2000).unwrap());
    assert!(!store.observe_sticker_favorite("hash", true, 2000).unwrap());
    store
        .upsert_sticker(&Sticker {
            filehash: "hash".into(),
            favorite: true,
            updated_at: 9999,
            ..Default::default()
        })
        .unwrap();
    assert!(!store.sticker("hash").unwrap().unwrap().favorite);
    assert!(store.observe_sticker_favorite("hash", true, 2001).unwrap());
    assert!(store.sticker("hash").unwrap().unwrap().favorite);
}

#[test]
fn recent_removal_respects_threshold_and_retains_tombstone() {
    let store = store();
    store.observe_sticker_recent("hash", 2000).unwrap();
    assert!(!store
        .remove_sticker_recent("hash", 3000, Some(1000))
        .unwrap());
    assert_eq!(store.sticker_recent_sent_ms("hash").unwrap(), Some(2000));
    assert!(store.observe_sticker_recent("hash", 2500).unwrap());
    assert!(!store.remove_sticker_recent("hash", 2999, None).unwrap());
    assert!(store
        .remove_sticker_recent("hash", 4000, Some(2500))
        .unwrap());
    assert!(!store.observe_sticker_recent("hash", 2500).unwrap());
    store
        .upsert_sticker(&Sticker {
            filehash: "hash".into(),
            recent_at: Some(2),
            updated_at: 9999,
            ..Default::default()
        })
        .unwrap();
    assert!(store.sticker("hash").unwrap().unwrap().recent_at.is_none());
    assert!(store.observe_sticker_recent("hash", 3000).unwrap());
    assert_eq!(store.sticker_recent_sent_ms("hash").unwrap(), Some(3000));
}

#[test]
fn unconditional_removal_blocks_old_history_but_not_new_sends() {
    let store = store();
    store.observe_sticker_recent("hash", 2000).unwrap();
    assert!(store.remove_sticker_recent("hash", 3000, None).unwrap());
    assert!(!store.observe_sticker_recent("hash", 2500).unwrap());
    assert!(store.observe_sticker_recent("hash", 4000).unwrap());
    assert!(!store.remove_sticker_recent("hash", 3999, None).unwrap());
    assert!(store.sticker("hash").unwrap().unwrap().recent_at.is_some());
}

#[test]
fn unknown_recent_removal_and_independent_clocks_do_not_delete_rows() {
    let store = store();
    assert!(!store.remove_sticker_recent("hash", 3000, None).unwrap());
    assert!(store.sticker("hash").unwrap().is_some());
    assert!(!store.observe_sticker_recent("hash", 2999).unwrap());
    store.observe_sticker_favorite("hash", true, 9000).unwrap();
    assert!(store.observe_sticker_recent("hash", 4000).unwrap());
    assert!(store
        .remove_sticker_recent("hash", 5000, Some(4000))
        .unwrap());
    assert!(store.sticker("hash").unwrap().unwrap().favorite);
    assert!(store.observe_sticker_favorite("hash", false, 9001).unwrap());
    assert!(store.sticker("hash").unwrap().is_some());
}

#[test]
fn sparse_upsert_and_sync_keep_local_file_and_download_reference() {
    let store = store();
    store
        .upsert_sticker(&Sticker {
            filehash: "hash".into(),
            path: Some("C:/synthetic/sticker.webp".into()),
            animated: true,
            lottie: true,
            emojis: vec!["😀".into()],
            updated_at: 1,
            ..Default::default()
        })
        .unwrap();
    store
        .set_sticker_locator("hash", b"valid-reference")
        .unwrap();
    store.observe_sticker_favorite("hash", false, 2000).unwrap();
    store.remove_sticker_recent("hash", 3000, None).unwrap();
    store
        .upsert_sticker(&Sticker {
            filehash: "hash".into(),
            updated_at: 4,
            ..Default::default()
        })
        .unwrap();
    let sticker = store.sticker("hash").unwrap().unwrap();
    assert_eq!(sticker.path.as_deref(), Some("C:/synthetic/sticker.webp"));
    assert!(sticker.animated && sticker.lottie);
    assert_eq!(sticker.emojis, vec!["😀"]);
    assert_eq!(
        store.sticker_locator("hash").unwrap(),
        Some(b"valid-reference".to_vec())
    );
}

#[test]
fn changed_counts_ignore_timestamp_only_refresh_and_track_locator_changes() {
    let store = store();
    let mut pack = StickerPack {
        pack_id: "pack".into(),
        name: Some("Name".into()),
        updated_at: 1,
        ..Default::default()
    };
    assert!(store.upsert_sticker_pack_changed(&pack).unwrap());
    pack.updated_at = 2;
    assert!(!store.upsert_sticker_pack_changed(&pack).unwrap());
    let mut sticker = Sticker {
        filehash: "hash".into(),
        pack_id: Some("pack".into()),
        updated_at: 1,
        ..Default::default()
    };
    assert!(store.upsert_sticker_changed(&sticker).unwrap());
    sticker.updated_at = 2;
    assert!(!store.upsert_sticker_changed(&sticker).unwrap());
    assert!(store.set_sticker_locator_changed("hash", b"first").unwrap());
    assert!(!store.set_sticker_locator_changed("hash", b"first").unwrap());
    assert!(store
        .set_sticker_locator_changed("hash", b"second")
        .unwrap());
}

#[test]
fn clock_migration_is_idempotent_and_preserves_existing_flags() {
    let store = store();
    store.observe_sticker_favorite("hash", true, 2001).unwrap();
    store.observe_sticker_recent("hash", 3001).unwrap();
    super::super::sticker_sync::migrate(&store.conn.lock().unwrap()).unwrap();
    assert_eq!(store.sticker_recent_sent_ms("hash").unwrap(), Some(3001));
    assert!(!store.observe_sticker_favorite("hash", false, 2000).unwrap());
    assert!(store.sticker("hash").unwrap().unwrap().favorite);
}
