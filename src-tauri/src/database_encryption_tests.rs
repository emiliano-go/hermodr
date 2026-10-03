use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!("postal-boot-crypto-{}-{}-{}", std::process::id(),
            crate::account_store::now_millis(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&path).unwrap(); Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) { std::fs::remove_dir_all(&self.0).unwrap(); }
}

#[test]
fn database_inventory_includes_disabled_history_both_links_and_old_session_files() {
    let directory = Directory::new();
    std::fs::write(directory.0.join("session_name"), "session-current.db").unwrap();
    std::fs::write(directory.0.join("session-old.db"), b"").unwrap();
    std::fs::write(directory.0.join("unrelated.db"), b"").unwrap();
    let history = directory.0.join("cold");
    let previous = directory.0.join("previous");
    let paths = files(&directory.0, &history, Some(&previous)).unwrap();
    for path in [directory.0.join("messages.db"), directory.0.join("scheduled.db"), directory.0.join("favorites.db"),
        directory.0.join("aliases.db"), directory.0.join("session-current.db"), directory.0.join("session-android.db"),
        directory.0.join("session-old.db"), history.join("messages.db"), previous.join("messages.db")] {
        assert!(paths.contains(&path));
    }
    assert!(!paths.contains(&directory.0.join("unrelated.db")));
    std::fs::write(directory.0.join("session_name"), "../foreign.db").unwrap();
    assert!(files(&directory.0, &history, None).is_err());
}

#[test]
fn plaintext_off_mode_never_calls_key_provider_or_creates_databases() {
    let directory = Directory::new(); let path = directory.0.join("missing.db");
    assert!(prepare_files(&[path.clone()], false, |_| panic!("key provider accessed")).unwrap().is_none());
    assert!(!path.exists());
}

#[test]
fn absent_old_session_primary_keeps_its_migration_artifacts_in_startup_inventory() {
    let directory = Directory::new();
    let path = directory.0.join("session-old.db");
    let marker = directory.0.join("session-old.db.postal-encryption");
    let backup = directory.0.join("session-old.db.cipher-synthetic.original");
    let stage = directory.0.join("session-old.db.cipher-synthetic.stage");
    std::fs::write(&marker, b"unrecognized interrupted ledger").unwrap();
    std::fs::write(&backup, b"synthetic plaintext session secret").unwrap();
    std::fs::write(&stage, b"synthetic stage").unwrap();
    let paths = files(&directory.0, &directory.0, None).unwrap();
    assert!(paths.contains(&path));
    assert!(prepare_files(&paths, true, |_| panic!("unverified artifacts must fail before key access")).is_err());
    assert!(!path.exists());
    assert_eq!(std::fs::read(&backup).unwrap(), b"synthetic plaintext session secret");
    std::fs::remove_file(&marker).unwrap();
    let paths = files(&directory.0, &directory.0, None).unwrap();
    assert!(paths.contains(&path));
    assert!(prepare_files(&paths, false, |_| panic!("orphan artifacts must not create keys")).is_err());
    assert!(backup.exists() && stage.exists());
}

#[test]
fn encrypted_files_require_existing_key_even_when_setting_is_off() {
    let directory = Directory::new(); let path = directory.0.join("encrypted.db");
    let key = DatabaseKey::from_bytes([17; 32]);
    let conn = postal_core::database_crypto::open_keyed(&path, &key).unwrap();
    conn.execute_batch("CREATE TABLE payload(value TEXT); INSERT INTO payload VALUES ('synthetic');").unwrap();
    conn.close().unwrap();
    let result = prepare_files(&[path.clone()], false, |may_create| { assert!(!may_create); Err("missing key".into()) });
    assert_eq!(result.unwrap_err(), "missing key");
    let conn = postal_core::database_crypto::open_keyed(&path, &key).unwrap();
    assert_eq!(conn.query_row("SELECT value FROM payload", [], |row| row.get::<_, String>(0)).unwrap(), "synthetic");
}

#[test]
fn bootstrap_migrates_plaintext_and_retains_account_key_scope() {
    let directory = Directory::new(); let path = directory.0.join("plaintext.db");
    let conn = postal_core::database_crypto::open_database(&path, None, Default::default()).unwrap();
    conn.execute_batch("CREATE TABLE payload(value TEXT); INSERT INTO payload VALUES ('synthetic');").unwrap();
    conn.close().unwrap();
    let loaded = prepare_files(&[path.clone()], true, |may_create| {
        assert!(may_create); Ok(DatabaseKey::from_bytes([17; 32]))
    }).unwrap();
    assert!(needs_key(&path).unwrap());
    let state = DatabaseEncryption::new(true);
    state.accounts.lock().unwrap().insert("one".into(), Ok(loaded));
    assert!(state.key("one").unwrap().is_some()); assert!(state.key("two").unwrap().is_none());
    state.accounts.lock().unwrap().insert("failed".into(), Err("locked".into()));
    assert_eq!(state.key("failed").unwrap_err(), "locked");
}
