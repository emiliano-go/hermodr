use super::*;

fn root() -> PathBuf {
    let path = std::env::temp_dir().join(format!("postal-backup-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    fs::create_dir(&path).unwrap();
    path
}

#[test]
fn backup_roundtrip_preserves_archive_but_excludes_session_credentials() {
    let root = root();
    let media = root.join("original-media");
    fs::create_dir(&media).unwrap();
    fs::write(media.join("photo.jpg"), b"synthetic attachment").unwrap();
    fs::write(root.join("session.db"), b"NEVER_EXPORT_LOGIN_SECRET").unwrap();
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    store.conn.lock().unwrap().execute_batch("CREATE TABLE session_credentials (secret TEXT);
        INSERT INTO session_credentials VALUES ('NEVER_EXPORT_LOGIN_SECRET')").unwrap();
    {
        let _batch = store.batch();
        for i in 0..1003 {
            store.insert_message(&StoredMessage { header: MessageHeader {
                chat: if i == 1002 { "private@s" } else { "test@s" }.into(), id: format!("m{i:04}"),
                sender: "sender@s".into(), timestamp: i, ..Default::default()
            }, text: format!("message {i}"), media: Media {
                path: match i { 0 | 1 => Some(media.join("photo.jpg").to_string_lossy().into()),
                    2 => Some(media.join("missing.jpg").to_string_lossy().into()), _ => None }, ..Default::default()
            }, ..Default::default() }).unwrap();
        }
    }
    store.set_saved_name("sender@s", "Synthetic sender").unwrap();
    store.set_starred("test@s", "m0000", true).unwrap();
    fs::write(media.join("library.webp"), b"synthetic sticker").unwrap();
    store.upsert_sticker_pack(&StickerPack { pack_id: "synthetic-pack".into(),
        tray_path: Some(media.join("photo.jpg").to_string_lossy().into_owned()), ..Default::default() }).unwrap();
    store.upsert_sticker(&Sticker { filehash: "synthetic-hash".into(), pack_id: Some("synthetic-pack".into()),
        path: Some(media.join("library.webp").to_string_lossy().into_owned()), favorite: true, ..Default::default() }).unwrap();
    store.save_poll("test@s", "m0001", "sender@s", "Question", &["Yes".into()], false, Some(&[7; 32])).unwrap();
    let backup = root.join("backup");
    let report = store.export_backup(&backup, &media, &[("sender@s".into(), "friend".into())]).unwrap();
    assert_eq!((report.messages, report.attachments, report.missing_attachments), (1003, 2, 1));
    assert!(!backup.join("session.db").exists());
    assert!(!fs::read(backup.join("messages.db")).unwrap().windows(25).any(|bytes| bytes == b"NEVER_EXPORT_LOGIN_SECRET"));
    let connection = Connection::open(&backup.join("messages.db")).unwrap();
    assert_eq!(connection.pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0)).unwrap(), "delete");
    assert_eq!(connection.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name='session_credentials'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    connection.close().unwrap();
    assert!(!backup.join("messages.db-wal").exists());
    assert!(!backup.join("messages.db-shm").exists());

    let export = root.join("conversation");
    let exported = store.export_conversation("test@s", &export, &media).unwrap();
    assert_eq!(exported.messages, 1002);
    let json: serde_json::Value = read_json(&export.join("conversation.json"), 16 * 1024 * 1024).unwrap();
    let pages = json["pages"].as_array().unwrap();
    assert_eq!(pages.iter().map(|p| p["messages"].as_array().unwrap().len()).collect::<Vec<_>>(), [500, 500, 2]);
    assert!(pages.iter().flat_map(|p| p["messages"].as_array().unwrap()).all(|m| m["chat"] == "test@s"));
    assert_eq!(pages[0]["marks"]["starred"][0], "m0000");
    assert_eq!(pages[0]["marks"]["polls"][0]["name"], "Question");

    let account = root.join("restored-account");
    let restored_media = root.join("restored-media");
    let moved = root.join("moved-backup");
    fs::rename(backup, &moved).unwrap();
    let backup = moved;
    restore_backup(&backup, &account, &restored_media).unwrap();
    let restored = MessageStore::open(&account.join("messages.db")).unwrap();
    assert_eq!(restored.count().unwrap(), 1003);
    assert_eq!(restored.message("test@s", "m0000").unwrap().local.sort_order,
        store.message("test@s", "m0000").unwrap().local.sort_order);
    assert_eq!(restored.name_for("sender@s").unwrap().as_deref(), Some("Synthetic sender"));
    assert_eq!(restored.poll_secret("test@s", "m0001").unwrap().unwrap().secret, vec![7; 32]);
    assert_eq!(restored.marks("test@s").unwrap().starred, ["m0000"]);
    let path = restored.message("test@s", "m0000").unwrap().media.path.unwrap();
    assert!(Path::new(&path).starts_with(restored_media.canonicalize().unwrap()));
    assert_eq!(fs::read(path).unwrap(), b"synthetic attachment");
    assert!(restored.message("test@s", "m0002").unwrap().media.path.is_none());
    let aliases = AliasStore::open(&account.join("aliases.db")).unwrap();
    assert_eq!(aliases.all().unwrap(), [("sender@s".into(), "friend".into())]);
    assert!(!account.join("session.db").exists());
    for (table, column, expected) in [("stickers", "path", b"synthetic sticker".as_slice()),
        ("sticker_packs", "tray_path", b"synthetic attachment".as_slice())] {
        let path: String = restored.conn.lock().unwrap().query_row(&format!("SELECT {column} FROM {table}"), [], |r| r.get(0)).unwrap();
        assert!(Path::new(&path).starts_with(restored_media.canonicalize().unwrap()));
        assert_eq!(fs::read(path).unwrap(), expected);
    }
    drop(aliases);
    drop(restored);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn restore_rejects_existing_targets_unsafe_paths_and_bad_manifests() {
    let root = root();
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    let backup = root.join("backup");
    store.export_backup(&backup, &root.join("no-media-yet"), &[]).unwrap();
    let existing = root.join("existing");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("sentinel"), b"preserve").unwrap();
    assert!(restore_backup(&backup, &existing, &root.join("unused-media")).is_err());
    assert_eq!(fs::read(existing.join("sentinel")).unwrap(), b"preserve");
    assert!(!root.join("unused-media").exists());
    let malformed = Connection::open(backup.join("messages.db")).unwrap();
    malformed.execute("INSERT INTO messages (chat,id,sender,timestamp,from_me,text,media_path) VALUES ('test@s','escape','sender@s',1,0,'x','../session.db')", []).unwrap();
    drop(malformed);
    fs::write(backup.join("manifest.json"), br#"{"format":"postal-local-backup","version":1,"messages":1,"attachments":1,"missing_attachments":0}"#).unwrap();
    assert!(restore_backup(&backup, &root.join("rejected"), &root.join("rejected-media")).unwrap_err().to_string().contains("attachment path"));
    assert!(!root.join("rejected").exists());
    assert!(!root.join("rejected-media").exists());
    fs::write(backup.join("manifest.json"), br#"{"format":"unknown","version":99,"messages":0,"attachments":0,"missing_attachments":0}"#).unwrap();
    assert!(restore_backup(&backup, &root.join("bad-format"), &root.join("bad-format-media")).is_err());
    assert!(!root.join("bad-format").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn older_backup_schema_migrates_without_importing_unknown_tables() {
    let root = root();
    let backup = root.join("legacy");
    fs::create_dir(&backup).unwrap();
    let source = Connection::open(backup.join("messages.db")).unwrap();
    super::super::schema::migrate_to(&source, 5).unwrap();
    source.execute("INSERT INTO chat_retention (jid,max_age_hours,max_messages,on_demand) VALUES ('test@s',0,0,1)", []).unwrap();
    drop(source);
    write_json(&backup.join("aliases.json"), &Vec::<(String, String)>::new()).unwrap();
    write_json(&backup.join("manifest.json"), &Manifest { format: "postal-local-backup".into(), version: 1,
        messages: 0, attachments: 0, missing_attachments: 0 }).unwrap();
    let account = root.join("restored");
    restore_backup(&backup, &account, &root.join("media")).unwrap();
    let restored = MessageStore::open(&account.join("messages.db")).unwrap();
    let policy = restored.chat_retention("test@s").unwrap();
    assert_eq!(policy.max_age_hours, RetentionLimit::Unlimited);
    assert_eq!(policy.max_messages, RetentionLimit::Unlimited);
    drop(restored);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_export_does_not_leave_partial_files_or_read_outside_media_root() {
    let root = root();
    let media = root.join("media");
    fs::create_dir(&media).unwrap();
    let outside = root.join("private.jpg");
    fs::write(&outside, b"must not be exported").unwrap();
    let store = MessageStore::open(Path::new(":memory:")).unwrap();
    store.insert_message(&StoredMessage { header: MessageHeader { chat: "test@s".into(), id: "escape".into(), ..Default::default() },
        media: Media { path: Some(outside.to_string_lossy().into()), ..Default::default() }, ..Default::default() }).unwrap();
    for (name, result) in [
        ("backup", store.export_backup(&root.join("backup"), &media, &[])),
        ("conversation", store.export_conversation("test@s", &root.join("conversation"), &media)),
    ] {
        assert!(result.unwrap_err().to_string().contains("outside the media folder"));
        assert!(!root.join(name).exists());
    }
    assert_eq!(fs::read(outside).unwrap(), b"must not be exported");
    fs::remove_dir_all(root).unwrap();
}
