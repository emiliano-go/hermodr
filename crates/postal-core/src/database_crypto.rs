use anyhow::{anyhow, ensure, Result};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct DatabaseKey(Zeroizing<[u8; 32]>);

impl DatabaseKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    fn pragma_value(&self) -> Zeroizing<String> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut value = Zeroizing::new(String::with_capacity(67));
        value.push_str("x'");
        for byte in self.0.iter() {
            value.push(HEX[(byte >> 4) as usize] as char);
            value.push(HEX[(byte & 15) as usize] as char);
        }
        value.push('\'');
        value
    }

    fn fingerprint(&self) -> String {
        Sha256::digest(self.0.as_slice()).iter().map(|byte| format!("{byte:02x}")).collect()
    }

    pub fn session_init(&self) -> whatsapp_rust::store::ConnectionInitHook {
        use diesel::connection::{get_default_instrumentation, SimpleConnection};
        use diesel::Connection as _;
        use diesel::RunQueryDsl;
        let key = self.clone();
        Arc::new(move |connection| {
            connection.set_instrumentation(None::<Box<dyn diesel::connection::Instrumentation>>);
            let statement =
                Zeroizing::new(format!("PRAGMA key = \"{}\";", key.pragma_value().as_str()));
            let keyed = connection.batch_execute(&statement);
            drop(statement);
            connection.set_instrumentation(get_default_instrumentation());
            keyed.map_err(|_| std::io::Error::other("could not key session database"))?;
            let version = diesel::sql_query("PRAGMA cipher_version")
                .get_result::<SessionCipherVersion>(connection)?;
            if version.cipher_version.trim().is_empty() {
                return Err(std::io::Error::other("SQLCipher provider unavailable").into());
            }
            connection
                .batch_execute("SELECT count(*) FROM sqlite_master;")
                .map_err(|_| std::io::Error::other("session database key verification failed"))?;
            Ok(())
        })
    }
}

#[derive(diesel::QueryableByName)]
struct SessionCipherVersion {
    #[diesel(sql_type = diesel::sql_types::Text)]
    cipher_version: String,
}

impl std::fmt::Debug for DatabaseKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DatabaseKey([REDACTED])")
    }
}

pub fn open_keyed(path: &Path, key: &DatabaseKey) -> Result<Connection> {
    open_database(path, Some(key), OpenFlags::default())
}

pub fn key_connection(connection: &Connection, key: &DatabaseKey) -> Result<()> {
    let value = key.pragma_value();
    // SQLCipher copies the key while both handles remain alive.
    let result = unsafe {
        rusqlite::ffi::sqlite3_key(
            connection.handle(),
            value.as_ptr().cast(),
            value.len() as i32,
        )
    };
    ensure!(result == rusqlite::ffi::SQLITE_OK, "could not key database");
    provider_version(connection)?;
    connection
        .query_row("SELECT count(*) FROM sqlite_master", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|_| anyhow!("database key verification failed"))?;
    Ok(())
}

fn provider_version(connection: &Connection) -> Result<()> {
    let version: String = connection
        .query_row("PRAGMA cipher_version", [], |row| row.get(0))
        .map_err(|_| anyhow!("SQLCipher provider unavailable"))?;
    ensure!(!version.trim().is_empty(), "SQLCipher provider unavailable");
    Ok(())
}

pub(crate) fn check_database_path(path: &Path, key: Option<&DatabaseKey>) -> Result<()> {
    if key.is_none() {
        ensure!(!needs_key(path)?, "encrypted database requires its stored key");
    } else if path != Path::new(":memory:") {
        regular_file(path)?;
        ensure!(Migration::read(path)?.is_none(), "database migration must complete before opening");
        reject_orphan_artifacts(path)?;
        if regular_file(path)?.is_none() { no_sidecars(path)?; }
    }
    Ok(())
}

pub fn open_database(
    path: &Path,
    key: Option<&DatabaseKey>,
    flags: OpenFlags,
) -> Result<Connection> {
    check_database_path(path, key)?;
    let connection = Connection::open_with_flags(path, flags)?;
    if let Some(key) = key {
        key_connection(&connection, key)?;
    }
    Ok(connection)
}

pub fn needs_key(path: &Path) -> Result<bool> {
    if path == Path::new(":memory:") {
        return Ok(false);
    }
    if regular_file(path)?.is_none() {
        no_sidecars(path)?;
    }
    if let Some(migration) = Migration::read(path)? {
        migration.check_recovery_files()?;
        return Ok(true);
    }
    reject_orphan_artifacts(path)?;
    header_needs_key(path)
}

fn header_needs_key(path: &Path) -> Result<bool> {
    if path == Path::new(":memory:") {
        return Ok(false);
    }
    let Some(metadata) = regular_file(path)? else {
        return Ok(false);
    };
    if metadata.len() == 0 {
        return Ok(false);
    }
    ensure!(metadata.len() >= 16, "database header is truncated");
    let mut header = [0u8; 16];
    File::open(path)?.read_exact(&mut header)?;
    Ok(&header != b"SQLite format 3\0")
}

fn regular_file(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            ensure!(
                metadata.file_type().is_file(),
                "database or migration artifact is not a regular file"
            );
            Ok(Some(metadata))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

const MIGRATION_MAGIC: &str = "postal-db-encryption-v1\n";

struct Migration {
    path: PathBuf,
    marker: PathBuf,
    stage: PathBuf,
    backup: PathBuf,
    token: String,
    source_hash: String,
    key_hash: String,
    ready: bool,
}

fn sibling(path: &Path, suffix: &str) -> Result<PathBuf> {
    let mut name = path
        .file_name()
        .ok_or_else(|| anyhow!("invalid database path"))?
        .to_os_string();
    name.push(suffix);
    Ok(path.with_file_name(name))
}

fn hash_file(path: &Path) -> Result<String> {
    ensure!(regular_file(path)?.is_some(), "migration source is missing");
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let length = file.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        hash.update(&buffer[..length]);
    }
    Ok(hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

fn create_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

impl Migration {
    fn paths(
        path: &Path,
        token: String,
        source_hash: String,
        key_hash: String,
        ready: bool,
    ) -> Result<Self> {
        Ok(Self {
            path: path.into(),
            marker: sibling(path, ".postal-encryption")?,
            stage: sibling(path, &format!(".cipher-{token}.stage"))?,
            backup: sibling(path, &format!(".cipher-{token}.original"))?,
            token,
            source_hash,
            key_hash,
            ready,
        })
    }

    fn create(path: &Path, key: &DatabaseKey) -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let token = format!(
            "{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let record = Self::paths(path, token, hash_file(path)?, key.fingerprint(), false)?;
        ensure!(
            regular_file(&record.stage)?.is_none() && regular_file(&record.backup)?.is_none(),
            "migration artifact already exists"
        );
        let mut marker = create_file(&record.marker)?;
        write!(
            marker,
            "{MIGRATION_MAGIC}P\n{}\n{}\n{}\n",
            record.token, record.source_hash, record.key_hash
        )?;
        marker.sync_all()?;
        Ok(record)
    }

    fn read(path: &Path) -> Result<Option<Self>> {
        let marker = sibling(path, ".postal-encryption")?;
        let Some(metadata) = regular_file(&marker)? else {
            return Ok(None);
        };
        ensure!(metadata.len() <= 256, "unrecognized migration marker");
        let mut text = String::new();
        File::open(marker)?.take(257).read_to_string(&mut text)?;
        let fields: Vec<_> = text.lines().collect();
        ensure!(
            fields.len() == 5 && fields[0] == MIGRATION_MAGIC.trim_end(),
            "unrecognized migration marker"
        );
        ensure!(matches!(fields[1], "P" | "R"), "invalid migration state");
        ensure!(
            fields[2].len() <= 96
                && fields[2].split('-').count() == 3
                && fields[2]
                    .split('-')
                    .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())),
            "invalid migration artifact name"
        );
        ensure!(
            fields[3..]
                .iter()
                .all(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())),
            "invalid migration fingerprint"
        );
        Ok(Some(Self::paths(
            path,
            fields[2].into(),
            fields[3].into(),
            fields[4].into(),
            fields[1] == "R",
        )?))
    }

    fn load(path: &Path, key: &DatabaseKey) -> Result<Option<Self>> {
        let record = Self::read(path)?;
        if let Some(record) = &record {
            ensure!(
                record.key_hash == key.fingerprint(),
                "migration requires its original stored key"
            );
        }
        Ok(record)
    }

    fn check_recovery_files(&self) -> Result<()> {
        let target = regular_file(&self.path)?;
        let stage = regular_file(&self.stage)?;
        let backup = regular_file(&self.backup)?;
        ensure!(
            target.is_some() || backup.is_some(),
            "migration original is missing"
        );
        if target.is_some() && !header_needs_key(&self.path)? {
            self.check_original(&self.path)?;
        }
        if backup.is_some() {
            ensure!(
                self.ready && !header_needs_key(&self.backup)?,
                "unrecognized migration backup"
            );
            self.check_original(&self.backup)?;
        }
        if let Some(stage) = stage {
            ensure!(
                (!self.ready && stage.len() == 0) || header_needs_key(&self.stage)?,
                "unrecognized migration stage"
            );
        }
        Ok(())
    }

    fn mark_ready(&mut self) -> Result<()> {
        regular_file(&self.marker)?;
        let mut marker = OpenOptions::new().write(true).open(&self.marker)?;
        marker.seek(SeekFrom::Start(MIGRATION_MAGIC.len() as u64))?;
        marker.write_all(b"R")?;
        marker.sync_all()?;
        self.ready = true;
        Ok(())
    }

    fn check_original(&self, path: &Path) -> Result<()> {
        ensure!(
            hash_file(path)? == self.source_hash,
            "migration original changed; all copies preserved"
        );
        Ok(())
    }

    fn export(&mut self, source: Connection, key: &DatabaseKey) -> Result<()> {
        self.check_original(&self.path)?;
        if regular_file(&self.stage)?.is_some_and(|metadata| metadata.len() == 0) {
            fs::remove_file(&self.stage)?;
        }
        if regular_file(&self.stage)?.is_some() {
            source.close().map_err(|(_, error)| error)?;
            validate_copy(&self.path, &self.stage, key)?;
        } else {
            drop(create_file(&self.stage)?);
            if let Err(error) = export_encrypted(source, &self.stage, key) {
                if regular_file(&self.stage).ok().flatten().is_some() {
                    let _ = fs::remove_file(&self.stage);
                }
                return Err(error);
            }
            validate_copy(&self.path, &self.stage, key)?;
        }
        self.check_original(&self.path)?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.stage)?
            .sync_all()?;
        self.mark_ready()
    }

    fn finish(&self, key: &DatabaseKey) -> Result<()> {
        ensure!(self.ready, "migration stage is not verified");
        let backup_exists = regular_file(&self.backup)?.is_some();
        let target_exists = regular_file(&self.path)?.is_some();
        if !backup_exists {
            ensure!(target_exists, "migration original is missing");
            if header_needs_key(&self.path)? {
                validate_encrypted(&self.path, key)?;
                ensure!(
                    regular_file(&self.stage)?.is_none(),
                    "unexpected migration stage after cleanup"
                );
                fs::remove_file(&self.marker)?;
                return Ok(());
            }
            self.check_original(&self.path)?;
            validate_copy(&self.path, &self.stage, key)?;
            no_sidecars(&self.path)?;
            fs::hard_link(&self.path, &self.backup)?;
        }
        self.check_original(&self.backup)?;
        if regular_file(&self.path)?.is_some() && !header_needs_key(&self.path)? {
            self.check_original(&self.path)?;
            no_sidecars(&self.path)?;
            fs::remove_file(&self.path)?;
        }
        if regular_file(&self.path)?.is_none() {
            validate_copy(&self.backup, &self.stage, key)?;
            no_sidecars(&self.stage)?;
            fs::hard_link(&self.stage, &self.path)?;
        }
        validate_copy(&self.backup, &self.path, key)?;
        if regular_file(&self.stage)?.is_some() {
            validate_copy(&self.backup, &self.stage, key)?;
            fs::remove_file(&self.stage)?;
        }
        fs::remove_file(&self.backup)?;
        fs::remove_file(&self.marker)?;
        Ok(())
    }
}

fn no_sidecars(path: &Path) -> Result<()> {
    for suffix in ["-wal", "-shm", "-journal"] {
        ensure!(
            regular_file(&sibling(path, suffix)?)?.is_none(),
            "database is still open or has pending journal data"
        );
    }
    Ok(())
}

fn reject_orphan_artifacts(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !parent.exists() {
        return Ok(());
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("database path is not UTF-8"))?;
    let prefix = format!("{name}.cipher-");
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        if let Some(name) = entry.file_name().to_str() {
            ensure!(
                !(name.starts_with(&prefix)
                    && (name.ends_with(".stage") || name.ends_with(".original"))),
                "orphan database migration artifact requires recovery"
            );
        }
    }
    Ok(())
}

fn plaintext_source(path: &Path) -> Result<Connection> {
    ensure!(
        !header_needs_key(path)?,
        "migration source is not plaintext"
    );
    let source = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    provider_version(&source)?;
    source.busy_timeout(std::time::Duration::ZERO)?;
    let (busy, _, _): (i64, i64, i64) =
        source.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
    ensure!(
        busy == 0,
        "database checkpoint is busy; close every database user before migration"
    );
    let mode: String = source.query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))?;
    ensure!(
        mode.eq_ignore_ascii_case("delete"),
        "database journal mode cannot be quiesced"
    );
    source.execute_batch("PRAGMA temp_store=MEMORY; PRAGMA trusted_schema=OFF; BEGIN EXCLUSIVE")?;
    Ok(source)
}

fn export_encrypted(source: Connection, stage: &Path, key: &DatabaseKey) -> Result<()> {
    let version: i64 = source.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let application: i64 = source.pragma_query_value(None, "application_id", |row| row.get(0))?;
    let path = stage
        .to_str()
        .ok_or_else(|| anyhow!("database path is not UTF-8"))?;
    source
        .execute(
            "ATTACH DATABASE ?1 AS encrypted KEY ?2",
            rusqlite::params![path, key.pragma_value().as_str()],
        )
        .map_err(|_| anyhow!("could not attach encrypted migration stage"))?;
    source.query_row("SELECT sqlcipher_export('encrypted')", [], |_| Ok(()))?;
    source.pragma_update(
        Some("encrypted"),
        "user_version",
        version,
    )?;
    source.pragma_update(
        Some("encrypted"),
        "application_id",
        application,
    )?;
    source.execute_batch("COMMIT; DETACH DATABASE encrypted")?;
    source.close().map_err(|(_, error)| error)?;
    Ok(())
}

fn validate_encrypted(path: &Path, key: &DatabaseKey) -> Result<Connection> {
    ensure!(header_needs_key(path)?, "migration stage is not encrypted");
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    key_connection(&connection, key)?;
    connection.execute_batch(
        "PRAGMA temp_store=MEMORY; PRAGMA trusted_schema=OFF; PRAGMA query_only=ON",
    )?;
    let integrity: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    ensure!(
        integrity == "ok",
        "encrypted database integrity check failed"
    );
    Ok(connection)
}

type SchemaRow = (String, String, String, Option<String>);

fn schema_rows(connection: &Connection) -> Result<Vec<SchemaRow>> {
    let mut statement = connection.prepare("SELECT type,name,tbl_name,sql FROM sqlite_master ORDER BY type COLLATE BINARY,name COLLATE BINARY")?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

fn quoted_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn compare_table(source: &Connection, target: &Connection, table: &str) -> Result<()> {
    let base = format!("SELECT * FROM {}", quoted_identifier(table));
    let statement = source.prepare(&base)?;
    let columns = statement.column_count();
    let ordering = statement
        .column_names()
        .iter()
        .map(|name| format!("{} COLLATE BINARY", quoted_identifier(name)))
        .collect::<Vec<_>>()
        .join(",");
    drop(statement);
    let sql = format!("{base} ORDER BY {ordering}");
    let mut left_statement = source.prepare(&sql)?;
    let mut right_statement = target.prepare(&sql)?;
    ensure!(
        columns == right_statement.column_count(),
        "migration column count changed"
    );
    let mut left = left_statement.query([])?;
    let mut right = right_statement.query([])?;
    loop {
        match (left.next()?, right.next()?) {
            (None, None) => return Ok(()),
            (Some(left), Some(right)) => {
                for index in 0..columns {
                    ensure!(
                        left.get_ref(index)? == right.get_ref(index)?,
                        "migration row contents changed"
                    );
                }
            }
            _ => return Err(anyhow!("migration row count changed")),
        }
    }
}

fn validate_copy(source: &Path, target: &Path, key: &DatabaseKey) -> Result<()> {
    ensure!(
        !header_needs_key(source)?,
        "migration original is not plaintext"
    );
    let original = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    original.execute_batch(
        "PRAGMA temp_store=MEMORY; PRAGMA trusted_schema=OFF; PRAGMA query_only=ON",
    )?;
    let encrypted = validate_encrypted(target, key)?;
    for pragma in ["user_version", "application_id"] {
        let left: i64 = original.pragma_query_value(None, pragma, |row| row.get(0))?;
        let right: i64 = encrypted.pragma_query_value(None, pragma, |row| row.get(0))?;
        ensure!(left == right, "migration database version changed");
    }
    let schema = schema_rows(&original)?;
    ensure!(
        schema == schema_rows(&encrypted)?,
        "migration schema changed"
    );
    for (kind, name, _, _) in schema {
        if kind == "table" {
            compare_table(&original, &encrypted, &name)?;
        }
    }
    Ok(())
}

pub fn prepare_database(path: &Path, key: &DatabaseKey) -> Result<()> {
    if path == Path::new(":memory:") {
        return Ok(());
    }
    needs_key(path)?;
    let marker = sibling(path, ".postal-encryption")?;
    if regular_file(path)?.is_none() && regular_file(&marker)?.is_none() {
        return Ok(());
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let path = parent.canonicalize()?.join(
        path.file_name()
            .ok_or_else(|| anyhow!("invalid database path"))?,
    );
    if let Some(mut migration) = Migration::load(&path, key)? {
        if !migration.ready {
            migration.check_original(&path)?;
            migration.export(plaintext_source(&path)?, key)?;
        }
        return migration.finish(key);
    }
    if header_needs_key(&path)? {
        validate_encrypted(&path, key)?;
        return Ok(());
    }
    if fs::metadata(&path)?.len() == 0 {
        return Ok(());
    }
    let source = plaintext_source(&path)?;
    let mut migration = Migration::create(&path, key)?;
    migration.export(source, key)?;
    migration.finish(key)
}

#[cfg(test)]
#[path = "database_crypto_tests.rs"]
mod migration_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    const MARKER: &str = "Postal synthetic database plaintext marker 93c0b3f689fc4739";

    struct Database(PathBuf);

    impl Database {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let directory = std::env::temp_dir().join(format!(
                "postal-cipher-{}-{nonce}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&directory).unwrap();
            Self(directory.join("synthetic.db"))
        }

        fn encrypted(&self) {
            let connection = open_keyed(&self.0, &DatabaseKey::from_bytes([0x11; 32])).unwrap();
            connection
                .pragma_update(None, "journal_mode", "WAL")
                .unwrap();
            insert_marker(&connection);
            connection
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
                .unwrap();
            connection.close().unwrap();
        }
    }

    impl Drop for Database {
        fn drop(&mut self) {
            std::fs::remove_dir_all(self.0.parent().unwrap()).unwrap();
        }
    }

    fn insert_marker(connection: &Connection) {
        connection
            .execute_batch("CREATE TABLE payload (id INTEGER PRIMARY KEY, body TEXT NOT NULL)")
            .unwrap();
        connection
            .execute("INSERT INTO payload(body) VALUES (?1)", [MARKER])
            .unwrap();
    }

    fn read_marker(connection: &Connection) -> rusqlite::Result<String> {
        connection.query_row("SELECT body FROM payload WHERE id = 1", [], |row| {
            row.get(0)
        })
    }

    #[test]
    fn sqlcipher_provider_reports_nonempty_cipher_version() {
        let connection =
            open_keyed(Path::new(":memory:"), &DatabaseKey::from_bytes([0x11; 32])).unwrap();
        let version: String = connection
            .query_row("PRAGMA cipher_version", [], |row| row.get(0))
            .unwrap();
        assert!(!version.trim().is_empty());
    }

    #[test]
    fn encrypted_database_header_and_contents_hide_plaintext_after_checkpoint() {
        let database = Database::new();
        database.encrypted();
        let bytes = std::fs::read(&database.0).unwrap();
        assert!(bytes.len() >= 16);
        assert_ne!(&bytes[..16], b"SQLite format 3\0");
        assert!(!bytes
            .windows(MARKER.len())
            .any(|window| window == MARKER.as_bytes()));
        for suffix in ["-wal", "-journal"] {
            let path = PathBuf::from(format!("{}{suffix}", database.0.display()));
            if path.exists() {
                let bytes = std::fs::read(path).unwrap();
                assert!(!bytes
                    .windows(MARKER.len())
                    .any(|window| window == MARKER.as_bytes()));
            }
        }
    }

    #[test]
    fn correct_database_key_reopens_persisted_payload() {
        let database = Database::new();
        database.encrypted();
        let connection = open_keyed(&database.0, &DatabaseKey::from_bytes([0x11; 32])).unwrap();
        assert_eq!(read_marker(&connection).unwrap(), MARKER);
        connection.close().unwrap();
    }

    #[test]
    fn wrong_and_missing_database_keys_reject_encrypted_payload() {
        let database = Database::new();
        database.encrypted();
        assert!(open_keyed(&database.0, &DatabaseKey::from_bytes([0x22; 32])).is_err());
        let unkeyed = Connection::open(&database.0).unwrap();
        assert!(read_marker(&unkeyed).is_err());
        unkeyed.close().unwrap();
        let correct = open_keyed(&database.0, &DatabaseKey::from_bytes([0x11; 32])).unwrap();
        assert_eq!(read_marker(&correct).unwrap(), MARKER);
        correct.close().unwrap();
    }

    #[test]
    fn legacy_plaintext_database_still_opens_without_key() {
        let database = Database::new();
        let connection = Connection::open(&database.0).unwrap();
        insert_marker(&connection);
        connection.close().unwrap();
        let bytes = std::fs::read(&database.0).unwrap();
        assert_eq!(&bytes[..16], b"SQLite format 3\0");
        assert!(open_keyed(&database.0, &DatabaseKey::from_bytes([0x11; 32])).is_err());
        let connection = Connection::open(&database.0).unwrap();
        assert_eq!(read_marker(&connection).unwrap(), MARKER);
        connection.close().unwrap();
    }

    #[test]
    fn database_key_debug_redacts_key_material() {
        assert_eq!(
            format!("{:?}", DatabaseKey::from_bytes([0x11; 32])),
            "DatabaseKey([REDACTED])"
        );
    }

    #[derive(diesel::QueryableByName)]
    struct CipherVersion {
        #[diesel(sql_type = diesel::sql_types::Text)]
        cipher_version: String,
    }

    #[tokio::test]
    async fn sdk_session_init_hook_keys_writer_and_reader_connections() {
        use diesel::connection::SimpleConnection;
        use diesel::RunQueryDsl;
        use std::sync::Arc;
        use whatsapp_rust::store::{SqliteStore, SqliteStoreConfig};

        let database = Database::new();
        let acquired = Arc::new(AtomicU64::new(0));
        let calls = Arc::clone(&acquired);
        let key = DatabaseKey::from_bytes([0x11; 32]);
        let config = SqliteStoreConfig::default()
            .with_read_pool_size(1)
            .with_connection_init(move |connection| {
                let statement =
                    Zeroizing::new(format!("PRAGMA key = \"{}\";", key.pragma_value().as_str()));
                connection.batch_execute(&statement)?;
                let version = diesel::sql_query("PRAGMA cipher_version")
                    .get_result::<CipherVersion>(connection)?;
                if version.cipher_version.trim().is_empty() {
                    return Err(std::io::Error::other("SQLCipher provider unavailable").into());
                }
                connection.batch_execute("SELECT count(*) FROM sqlite_master;")?;
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(())
            });
        let store = SqliteStore::with_config(database.0.to_str().unwrap(), config)
            .await
            .unwrap();
        let shared = store.shared();
        shared
            .run(|connection| {
                connection
                    .batch_execute(
                        "CREATE TABLE payload (id INTEGER PRIMARY KEY, body TEXT NOT NULL)",
                    )
                    .unwrap();
                diesel::sql_query("INSERT INTO payload(body) VALUES (?)")
                    .bind::<diesel::sql_types::Text, _>(MARKER)
                    .execute(connection)
                    .unwrap();
                connection
                    .batch_execute("PRAGMA wal_checkpoint(TRUNCATE)")
                    .unwrap();
                Ok(())
            })
            .await
            .unwrap();
        shared
            .read(|connection| {
                let version = diesel::sql_query("PRAGMA cipher_version")
                    .get_result::<CipherVersion>(connection)
                    .unwrap();
                assert!(!version.cipher_version.trim().is_empty());
                connection
                    .batch_execute("SELECT body FROM payload;")
                    .unwrap();
                Ok(())
            })
            .await
            .unwrap();
        assert!(acquired.load(Ordering::Relaxed) >= 2);
        drop(shared);
        drop(store);
        let bytes = std::fs::read(&database.0).unwrap();
        assert_ne!(&bytes[..16], b"SQLite format 3\0");
        assert!(!bytes
            .windows(MARKER.len())
            .any(|window| window == MARKER.as_bytes()));
        let connection = open_keyed(&database.0, &DatabaseKey::from_bytes([0x11; 32])).unwrap();
        assert_eq!(read_marker(&connection).unwrap(), MARKER);
        connection.close().unwrap();
    }
}
