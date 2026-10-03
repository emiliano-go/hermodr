use crate::{AliasStore, MessageStore, StoredMessage};
use crate::database_crypto::{DatabaseKey, open_database, prepare_database};
use crate::store::{MessageHeader, favorites::FavoriteWorker, scheduled::ScheduledOutbox};
use std::{fs, path::{Path, PathBuf}};

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("postal-encrypted-stores-{}-{nonce}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Directory {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
}

fn message() -> StoredMessage {
    StoredMessage { header: MessageHeader { chat: "synthetic@s".into(), id: "encrypted-row".into(),
        sender: "synthetic-sender@s".into(), timestamp: 123, ..Default::default() },
        text: "SYNTHETIC_PRIVATE_ARCHIVE_TEXT".into(), ..Default::default() }
}

fn assert_encrypted(path: &Path, key: &DatabaseKey) {
    let bytes = fs::read(path).unwrap();
    assert!(!bytes.starts_with(b"SQLite format 3\0"));
    let marker = b"SYNTHETIC_PRIVATE_ARCHIVE_TEXT";
    assert!(!bytes.windows(marker.len()).any(|window| window == marker));
    assert!(open_database(path, None, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).is_err());
    let wrong = DatabaseKey::from_bytes([0x11; 32]);
    assert!(open_database(path, Some(&wrong), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).is_err());
    open_database(path, Some(key), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
}

#[tokio::test]
async fn database_encryption_migrates_every_postal_store_and_reopens_data() {
    let directory = Directory::new();
    let messages = directory.0.join("messages.db");
    let aliases = directory.0.join("aliases.db");
    let scheduled = directory.0.join("scheduled.db");
    let favorites = directory.0.join("favorites.db");
    {
        let store = MessageStore::open(&messages).unwrap();
        store.insert_message(&message()).unwrap();
        store.set_saved_name("synthetic-sender@s", "Synthetic sender").unwrap();
        store.save_poll("synthetic@s", "encrypted-row", "synthetic-sender@s", "Private poll", &["Yes".into()], false, Some(&[0x42; 32])).unwrap();
        let alias = AliasStore::open(&aliases).unwrap();
        alias.add(&["synthetic-sender@s".into()], "synthetic").unwrap();
        let outbox = ScheduledOutbox::open(&scheduled).unwrap();
        outbox.schedule_message("queued", "synthetic@s", "Private scheduled text", &[], 456).unwrap();
        let favorite = FavoriteWorker::open_with_key(&favorites, None).await.unwrap();
        favorite.run(|db| db.replace(1, &["synthetic@s".into()])).await.unwrap();
    }
    let key = DatabaseKey::from_bytes([0x73; 32]);
    for path in [&messages, &aliases, &scheduled, &favorites] { prepare_database(path, &key).unwrap(); }
    let store = MessageStore::open_with_key(&messages, Some(&key)).unwrap();
    assert_eq!(store.message("synthetic@s", "encrypted-row").unwrap().text, message().text);
    assert_eq!(store.name_for("synthetic-sender@s").unwrap().as_deref(), Some("Synthetic sender"));
    assert_eq!(store.poll_secret("synthetic@s", "encrypted-row").unwrap().unwrap().secret, vec![0x42; 32]);
    let alias = AliasStore::open_with_key(&aliases, Some(&key)).unwrap();
    assert_eq!(alias.all().unwrap(), [("synthetic-sender@s".into(), "synthetic".into())]);
    let outbox = ScheduledOutbox::open_with_key(&scheduled, Some(&key)).unwrap();
    assert_eq!(outbox.scheduled_messages().unwrap()[0].text, "Private scheduled text");
    let favorite = FavoriteWorker::open_with_key(&favorites, Some(key.clone())).await.unwrap();
    assert_eq!(favorite.run(|db| db.list()).await.unwrap(), ["synthetic@s"]);
    drop((store, alias, outbox, favorite));
    for path in [&messages, &aliases, &scheduled, &favorites] { assert_encrypted(path, &key); }
}

#[test]
fn database_encryption_export_is_plaintext_and_restore_encrypts_from_first_write() {
    let directory = Directory::new();
    let key = DatabaseKey::from_bytes([0x72; 32]);
    let media = directory.0.join("media");
    fs::create_dir(&media).unwrap();
    let source = MessageStore::open_with_key(&directory.0.join("messages.db"), Some(&key)).unwrap();
    source.insert_message(&message()).unwrap();
    let backup = directory.0.join("backup");
    source.export_backup(&backup, &media, &[("synthetic-sender@s".into(), "synthetic".into())]).unwrap();
    assert!(fs::read(backup.join("messages.db")).unwrap().starts_with(b"SQLite format 3\0"));
    assert!(!backup.join("session.db").exists());
    let restored = directory.0.join("restored");
    let restored_media = directory.0.join("restored-media");
    crate::store::archive::restore_backup_with_key(&backup, &restored, &restored_media, Some(&key)).unwrap();
    let store = MessageStore::open_with_key(&restored.join("messages.db"), Some(&key)).unwrap();
    assert_eq!(store.message("synthetic@s", "encrypted-row").unwrap().text, message().text);
    drop(store);
    for name in ["messages.db", "aliases.db"] { assert_encrypted(&restored.join(name), &key); }
}

#[tokio::test]
async fn database_encryption_rejects_pending_session_before_any_store_or_network_start() {
    for keyed in [false, true] {
        for suffix in [".postal-encryption", ".cipher-synthetic.original", "-wal"] {
            let directory = Directory::new();
            let mut config = crate::ServiceConfig::under(&directory.0);
            config.database_key = keyed.then(|| DatabaseKey::from_bytes([0x52; 32]));
            config.messages_path = directory.0.join("store-open-sentinel");
            fs::create_dir(&config.messages_path).unwrap();
            let artifact = directory.0.join(format!("session.db{suffix}"));
            fs::write(&artifact, b"synthetic recovery artifact").unwrap();
            let error = match crate::WhatsAppService::start(config).await {
                Err(error) => error.to_string(),
                Ok(_) => panic!("pending session unexpectedly started"),
            };
            assert!(error.contains("migration") || error.contains("journal"), "wrong startup phase: {error}");
            assert!(!error.contains("regular file") && !error.contains("store-open-sentinel"));
            for name in ["session.db", "scheduled.db", "aliases.db", "favorites.db"] {
                assert!(!directory.0.join(name).exists());
            }
            assert_eq!(fs::read(&artifact).unwrap(), b"synthetic recovery artifact");
        }
    }
}
