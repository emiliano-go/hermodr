use super::*;

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "postal-migration-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn database(&self) -> PathBuf {
        self.0.join("synthetic.db")
    }

    fn names(&self) -> Vec<String> {
        fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn key() -> DatabaseKey {
    DatabaseKey::from_bytes([0x31; 32])
}

fn legacy(path: &Path) -> Connection {
    let connection = Connection::open(path).unwrap();
    connection.execute_batch(
        "CREATE TABLE secrets(id INTEGER PRIMARY KEY AUTOINCREMENT, label TEXT NOT NULL, material BLOB NOT NULL);
         CREATE INDEX secret_labels ON secrets(label);
         CREATE TABLE audit(label TEXT NOT NULL);
         CREATE TRIGGER inserted_secret AFTER INSERT ON secrets BEGIN INSERT INTO audit(label) VALUES(new.label); END;
         CREATE VIEW secret_names AS SELECT id,label FROM secrets;
         PRAGMA user_version=17; PRAGMA application_id=12567;"
    ).unwrap();
    connection
        .execute(
            "INSERT INTO secrets(label,material) VALUES (?1,?2)",
            rusqlite::params!["synthetic session key", &[0u8, 1, 2, 0xff][..]],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO secrets(label,material) VALUES (?1,?2)",
            rusqlite::params!["deleted sequence entry", &[9u8][..]],
        )
        .unwrap();
    connection
        .execute("DELETE FROM secrets WHERE id=2", [])
        .unwrap();
    connection
}

fn assert_preserved(path: &Path) {
    let connection = open_keyed(path, &key()).unwrap();
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    let application: i64 = connection
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .unwrap();
    assert_eq!((version, application), (17, 12567));
    let material: Vec<u8> = connection
        .query_row("SELECT material FROM secrets WHERE id=1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(material, [0, 1, 2, 0xff]);
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM audit", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        connection
            .query_row("SELECT label FROM secret_names WHERE id=1", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
        "synthetic session key"
    );
    connection
        .execute(
            "INSERT INTO secrets(label,material) VALUES ('new entry', X'01')",
            [],
        )
        .unwrap();
    assert_eq!(connection.last_insert_rowid(), 3);
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM audit", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        3
    );
    connection.close().unwrap();
}

fn staged(path: &Path) -> Migration {
    let source = plaintext_source(path).unwrap();
    let mut migration = Migration::create(path, &key()).unwrap();
    migration.export(source, &key()).unwrap();
    migration
}

#[test]
fn missing_database_preparation_never_creates_files() {
    let directory = Directory::new();
    let path = directory.database();
    assert!(!needs_key(&path).unwrap());
    prepare_database(&path, &key()).unwrap();
    assert!(directory.names().is_empty());
}

#[test]
fn recovery_ledger_requires_existing_key_even_when_primary_is_missing() {
    let directory = Directory::new();
    let path = directory.database();
    legacy(&path).close().unwrap();
    let migration = staged(&path);
    assert!(needs_key(&path).unwrap());
    assert!(open_database(&path, None, OpenFlags::SQLITE_OPEN_READ_WRITE).is_err());
    fs::hard_link(&path, &migration.backup).unwrap();
    fs::remove_file(&path).unwrap();
    assert!(needs_key(&path).unwrap());
    assert!(open_database(&path, Some(&key()), OpenFlags::default()).is_err());
    assert!(!path.exists());
    prepare_database(&path, &key()).unwrap();
    assert_preserved(&path);
}

#[test]
fn orphan_artifacts_and_journals_prevent_fresh_database_creation() {
    for suffix in [
        ".cipher-1-2-3.stage",
        ".cipher-1-2-3.original",
        "-wal",
        "-shm",
        "-journal",
        ".postal-encryption",
    ] {
        let directory = Directory::new();
        let path = directory.database();
        let artifact = sibling(&path, suffix).unwrap();
        fs::write(&artifact, b"foreign artifact").unwrap();
        assert!(needs_key(&path).is_err(), "accepted {suffix}");
        assert!(open_database(&path, Some(&key()), OpenFlags::default()).is_err(), "keyed creation accepted {suffix}");
        assert!(prepare_database(&path, &key()).is_err());
        assert!(!path.exists());
        assert_eq!(fs::read(artifact).unwrap(), b"foreign artifact");
    }
}

#[test]
fn offline_migration_preserves_schema_rows_blobs_triggers_and_sequence() {
    let directory = Directory::new();
    let path = directory.database();
    legacy(&path).close().unwrap();
    prepare_database(&path, &key()).unwrap();
    assert!(needs_key(&path).unwrap());
    assert_eq!(directory.names(), ["synthetic.db"]);
    assert_preserved(&path);
    prepare_database(&path, &key()).unwrap();
}

#[test]
fn migration_checkpoints_committed_wal_rows_before_export() {
    let directory = Directory::new();
    let path = directory.database();
    let connection = legacy(&path);
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .unwrap();
    connection
        .pragma_update(None, "wal_autocheckpoint", 0)
        .unwrap();
    connection
        .execute("UPDATE secrets SET material=X'000102ff' WHERE id=1", [])
        .unwrap();
    let result = unsafe {
        rusqlite::ffi::sqlite3_db_config(
            connection.handle(),
            rusqlite::ffi::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
            1,
            std::ptr::null_mut::<i32>(),
        )
    };
    assert_eq!(result, rusqlite::ffi::SQLITE_OK);
    connection.close().unwrap();
    assert!(sibling(&path, "-wal").unwrap().exists());
    prepare_database(&path, &key()).unwrap();
    assert_preserved(&path);
    assert_eq!(directory.names(), ["synthetic.db"]);
}

#[test]
fn busy_wal_checkpoint_preserves_original_without_creating_stage() {
    let directory = Directory::new();
    let path = directory.database();
    let writer = legacy(&path);
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    let reader = Connection::open(&path).unwrap();
    reader.execute_batch("BEGIN").unwrap();
    reader
        .query_row("SELECT COUNT(*) FROM secrets", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap();
    writer
        .execute(
            "INSERT INTO secrets(label,material) VALUES ('pending WAL row',X'42')",
            [],
        )
        .unwrap();
    assert!(prepare_database(&path, &key()).is_err());
    assert!(!sibling(&path, ".postal-encryption").unwrap().exists());
    assert_eq!(
        writer
            .query_row("SELECT COUNT(*) FROM secrets", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    reader.execute_batch("ROLLBACK").unwrap();
    reader.close().unwrap();
    writer.close().unwrap();
    prepare_database(&path, &key()).unwrap();
    let connection = open_keyed(&path, &key()).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM secrets", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn migration_resumes_each_verified_replacement_interruption() {
    for step in 0..5 {
        let directory = Directory::new();
        let path = directory.database();
        legacy(&path).close().unwrap();
        let migration = staged(&path);
        if step >= 1 {
            fs::hard_link(&path, &migration.backup).unwrap();
        }
        if step >= 2 {
            fs::remove_file(&path).unwrap();
        }
        if step >= 3 {
            fs::hard_link(&migration.stage, &path).unwrap();
        }
        if step >= 4 {
            fs::remove_file(&migration.stage).unwrap();
            fs::remove_file(&migration.backup).unwrap();
        }
        prepare_database(&path, &key()).unwrap();
        assert_eq!(directory.names(), ["synthetic.db"]);
        assert_preserved(&path);
    }
}

#[test]
fn wrong_key_preserves_interrupted_migration_and_all_original_artifacts() {
    let directory = Directory::new();
    let path = directory.database();
    legacy(&path).close().unwrap();
    let migration = staged(&path);
    fs::hard_link(&path, &migration.backup).unwrap();
    fs::remove_file(&path).unwrap();
    let stage = fs::read(&migration.stage).unwrap();
    let original = fs::read(&migration.backup).unwrap();
    assert!(prepare_database(&path, &DatabaseKey::from_bytes([0x32; 32])).is_err());
    assert!(!path.exists());
    assert_eq!(fs::read(&migration.stage).unwrap(), stage);
    assert_eq!(fs::read(&migration.backup).unwrap(), original);
    prepare_database(&path, &key()).unwrap();
    assert_preserved(&path);
}

#[test]
fn foreign_migration_marker_is_never_overwritten() {
    let directory = Directory::new();
    let path = directory.database();
    legacy(&path).close().unwrap();
    let before = fs::read(&path).unwrap();
    let marker = sibling(&path, ".postal-encryption").unwrap();
    fs::write(&marker, b"foreign marker").unwrap();
    assert!(prepare_database(&path, &key()).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read(marker).unwrap(), b"foreign marker");
}

#[test]
fn foreign_backup_and_mismatched_stage_leave_plaintext_original_intact() {
    let directory = Directory::new();
    let path = directory.database();
    legacy(&path).close().unwrap();
    let migration = staged(&path);
    let before = fs::read(&path).unwrap();
    fs::write(&migration.backup, b"foreign backup").unwrap();
    assert!(prepare_database(&path, &key()).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read(&migration.backup).unwrap(), b"foreign backup");
    fs::remove_file(&migration.backup).unwrap();
    let changed = open_keyed(&migration.stage, &key()).unwrap();
    changed
        .execute(
            "UPDATE secrets SET label='different payload' WHERE id=1",
            [],
        )
        .unwrap();
    changed.close().unwrap();
    assert!(prepare_database(&path, &key()).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(migration.stage.exists());
}

#[test]
fn unknown_pending_stage_is_preserved_without_replacing_original() {
    let directory = Directory::new();
    let path = directory.database();
    legacy(&path).close().unwrap();
    let source = plaintext_source(&path).unwrap();
    let migration = Migration::create(&path, &key()).unwrap();
    source.close().unwrap();
    fs::write(&migration.stage, b"foreign stage").unwrap();
    let original = fs::read(&path).unwrap();
    assert!(prepare_database(&path, &key()).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(fs::read(&migration.stage).unwrap(), b"foreign stage");
}

#[test]
fn wrong_key_and_unkeyed_flags_cannot_open_or_replace_encrypted_database() {
    let directory = Directory::new();
    let path = directory.database();
    let connection = open_keyed(&path, &key()).unwrap();
    connection.execute_batch("CREATE TABLE protected(value TEXT); INSERT INTO protected VALUES('synthetic protected value')").unwrap();
    connection.close().unwrap();
    let before = fs::read(&path).unwrap();
    assert!(prepare_database(&path, &DatabaseKey::from_bytes([0x32; 32])).is_err());
    assert!(open_database(&path, None, OpenFlags::SQLITE_OPEN_READ_ONLY).is_err());
    let connection = open_database(&path, Some(&key()), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert!(connection.execute("DELETE FROM protected", []).is_err());
    connection.close().unwrap();
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn ffi_keying_never_sends_key_material_to_sql_trace() {
    let directory = Directory::new();
    let path = directory.database();
    let connection = Connection::open(&path).unwrap();
    let queries = std::sync::Mutex::new(Vec::<String>::new());
    let result = unsafe {
        rusqlite::ffi::sqlite3_trace_v2(
            connection.handle(),
            rusqlite::ffi::SQLITE_TRACE_STMT as u32,
            Some(trace_statement),
            (&queries as *const std::sync::Mutex<Vec<String>>)
                .cast_mut()
                .cast(),
        )
    };
    assert_eq!(result, rusqlite::ffi::SQLITE_OK);
    key_connection(&connection, &key()).unwrap();
    connection
        .execute_batch("CREATE TABLE protected(value BLOB)")
        .unwrap();
    let result = unsafe {
        rusqlite::ffi::sqlite3_trace_v2(connection.handle(), 0, None, std::ptr::null_mut())
    };
    assert_eq!(result, rusqlite::ffi::SQLITE_OK);
    connection.close().unwrap();
    let queries = queries.into_inner().unwrap();
    assert!(queries.iter().any(|query| query.contains("cipher_version")));
    assert!(queries
        .iter()
        .all(|query| !query.to_ascii_lowercase().contains("pragma key")
            && !query.contains(key().pragma_value().as_str())));
    assert!(needs_key(&path).unwrap());
    assert!(open_keyed(&path, &key()).is_ok());
}

unsafe extern "C" fn trace_statement(
    _: u32,
    context: *mut std::ffi::c_void,
    statement: *mut std::ffi::c_void,
    _: *mut std::ffi::c_void,
) -> i32 {
    let sql = unsafe { rusqlite::ffi::sqlite3_sql(statement.cast()) };
    if !sql.is_null() {
        let queries = unsafe { &*context.cast::<std::sync::Mutex<Vec<String>>>() };
        if let Ok(mut queries) = queries.lock() {
            queries.push(
                unsafe { std::ffi::CStr::from_ptr(sql) }
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    0
}

#[test]
fn empty_owned_stage_after_interruption_can_restart_export() {
    let directory = Directory::new();
    let path = directory.database();
    legacy(&path).close().unwrap();
    let source = plaintext_source(&path).unwrap();
    let migration = Migration::create(&path, &key()).unwrap();
    source.close().unwrap();
    drop(create_file(&migration.stage).unwrap());
    prepare_database(&path, &key()).unwrap();
    assert_eq!(directory.names(), ["synthetic.db"]);
    assert_preserved(&path);
}

#[test]
fn session_factory_suppresses_key_statement_instrumentation() {
    use diesel::connection::SimpleConnection;
    use diesel::Connection as _;
    let mut connection = diesel::SqliteConnection::establish(":memory:").unwrap();
    let events = Arc::new(AtomicU64::new(0));
    let observed = Arc::clone(&events);
    connection.set_instrumentation(move |_: diesel::connection::InstrumentationEvent<'_>| {
        observed.fetch_add(1, Ordering::Relaxed);
    });
    connection.batch_execute("SELECT 1").unwrap();
    let before = events.load(Ordering::Relaxed);
    assert!(before > 0);
    key().session_init()(&mut connection).unwrap();
    assert_eq!(events.load(Ordering::Relaxed), before);
    connection
        .batch_execute("CREATE TABLE protected(value BLOB)")
        .unwrap();
}

#[cfg(windows)]
#[test]
fn plaintext_backup_cleanup_failure_never_reports_completed_migration() {
    use std::os::windows::fs::OpenOptionsExt;
    let directory = Directory::new();
    let path = directory.database();
    legacy(&path).close().unwrap();
    let migration = staged(&path);
    fs::hard_link(&path, &migration.backup).unwrap();
    fs::remove_file(&path).unwrap();
    fs::hard_link(&migration.stage, &path).unwrap();
    let held = OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(&migration.backup)
        .unwrap();
    let result = prepare_database(&path, &key());
    assert!(result.is_err());
    assert!(migration.backup.exists() && migration.marker.exists());
    drop(held);
    prepare_database(&path, &key()).unwrap();
    assert_eq!(directory.names(), ["synthetic.db"]);
}

#[cfg(unix)]
#[test]
fn symlink_database_is_rejected_without_touching_target() {
    let directory = Directory::new();
    let path = directory.database();
    legacy(&path).close().unwrap();
    let link = directory.0.join("linked.db");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    let before = fs::read(&path).unwrap();
    assert!(prepare_database(&link, &key()).is_err());
    assert_eq!(fs::read(path).unwrap(), before);
}
