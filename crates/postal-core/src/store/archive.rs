use super::*;
use crate::aliases::AliasStore;
use std::{collections::HashMap, fs::{self, File}, io::{BufReader, BufWriter, Write}, path::{Component, PathBuf}};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct ArchiveReport {
    pub directory: String,
    pub messages: u64,
    pub attachments: usize,
    pub missing_attachments: usize,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
#[cfg_attr(feature = "wire-types", ts(rename = "ArchiveManifest"))]
struct Manifest {
    format: String,
    version: u32,
    messages: u64,
    attachments: usize,
    missing_attachments: usize,
}

#[cfg(feature = "wire-types")]
pub fn visit_wire_types(visitor: &mut impl ts_rs::TypeVisitor) {
    visitor.visit::<Manifest>();
}

#[derive(Default)]
struct NewDirectories(Vec<PathBuf>);

impl NewDirectories {
    fn create(&mut self, path: &Path) -> Result<()> {
        let parent = path.parent().unwrap_or_else(|| Path::new(".")).canonicalize()?;
        fs::create_dir(path).context("destination must be a new folder")?;
        let created = path.canonicalize()?;
        anyhow::ensure!(created.parent() == Some(parent.as_path()) && created.file_name() == path.file_name(), "created folder escaped its destination");
        self.0.push(created);
        Ok(())
    }

    fn complete(mut self) { self.0.clear(); }
}

impl Drop for NewDirectories {
    fn drop(&mut self) {
        for path in self.0.iter().rev() {
            if let Err(error) = fs::remove_dir_all(path) { log::warn!("could not remove incomplete archive {}: {error}", path.display()); }
        }
    }
}

struct Attachments {
    root: PathBuf,
    destination: PathBuf,
    paths: HashMap<String, Option<String>>,
    missing: usize,
}

impl Attachments {
    fn new(root: &Path, destination: &Path) -> Result<Self> {
        Ok(Self { root: root.into(), destination: destination.into(), paths: HashMap::new(), missing: 0 })
    }

    fn copy(&mut self, source: &str) -> Result<Option<String>> {
        if let Some(path) = self.paths.get(source) { return Ok(path.clone()); }
        let source_path = match Path::new(source).canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.missing += 1;
                self.paths.insert(source.into(), None);
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        };
        anyhow::ensure!(source_path.starts_with(self.root.canonicalize()?) && source_path.is_file(), "attachment is outside the media folder");
        let extension = source_path.extension().and_then(|v| v.to_str())
            .filter(|v| v.len() <= 8 && v.chars().all(|c| c.is_ascii_alphanumeric())).unwrap_or("bin");
        let relative = format!("media/{}.{}", self.paths.len(), extension);
        fs::create_dir_all(self.destination.join("media"))?;
        fs::copy(&source_path, self.destination.join(&relative))?;
        self.paths.insert(source.into(), Some(relative.clone()));
        Ok(Some(relative))
    }

    fn count(&self) -> usize { self.paths.len() - self.missing }
}

fn names(conn: &Connection) -> Result<Vec<String>> {
    Ok(conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?
        .query_map([], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?)
}

fn quoted(identifier: &str) -> String { format!("\"{}\"", identifier.replace('"', "\"\"")) }

fn attachment_columns(conn: &Connection) -> Result<Vec<(&'static str, &'static str)>> {
    let tables = names(conn)?;
    Ok([("messages", "media_path"), ("messages", "reply_to_path"),
        ("stickers", "path"), ("sticker_packs", "tray_path")].into_iter()
        .filter(|(table, _)| tables.iter().any(|name| name == table)).collect())
}

fn attachment_paths(conn: &Connection) -> Result<Vec<String>> {
    let query = attachment_columns(conn)?.iter().map(|(table, column)|
        format!("SELECT {column} FROM {table} WHERE {column} IS NOT NULL AND {column} NOT LIKE 'data:%'"))
        .collect::<Vec<_>>().join(" UNION ");
    Ok(conn.prepare(&query)?.query_map([], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?)
}

fn rewrite_attachment(conn: &Connection, from: &str, to: Option<&str>) -> Result<()> {
    for (table, column) in attachment_columns(conn)? {
        conn.execute(&format!("UPDATE {table} SET {column} = ?1 WHERE {column} = ?2"), params![to, from])?;
    }
    Ok(())
}

fn copy_tables(source: &Connection, destination: &mut Connection) -> Result<()> {
    let source_version: i64 = source.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let destination_version: i64 = destination.pragma_query_value(None, "user_version", |row| row.get(0))?;
    anyhow::ensure!(source_version == destination_version, "backup schema {source_version} differs from supported schema {destination_version}");
    let transaction = destination.transaction()?;
    // Only the fresh application's schema defines what can enter a backup or restore.
    for table in names(&transaction)? {
        let table = quoted(&table);
        let columns = transaction.prepare(&format!("PRAGMA table_info({table})"))?
            .query_map([], |row| row.get::<_, String>(1))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let fields = columns.iter().map(|column| quoted(column)).collect::<Vec<_>>().join(",");
        let placeholders = vec!["?"; columns.len()].join(",");
        let mut source_rows = source.prepare(&format!("SELECT {fields} FROM {table}"))?;
        let mut rows = source_rows.query([])?;
        let mut insert = transaction.prepare(&format!("INSERT OR REPLACE INTO {table} ({fields}) VALUES ({placeholders})"))?;
        while let Some(row) = rows.next()? {
            let values = (0..columns.len()).map(|i| row.get::<_, rusqlite::types::Value>(i)).collect::<rusqlite::Result<Vec<_>>>()?;
            insert.execute(rusqlite::params_from_iter(values))?;
        }
    }
    transaction.commit()?;
    Ok(())
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut writer = BufWriter::new(File::create_new(path)?);
    serde_json::to_writer_pretty(&mut writer, value)?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(())
}

impl MessageStore {
    pub fn export_backup(&self, directory: &Path, media_root: &Path, aliases: &[(String, String)]) -> Result<ArchiveReport> {
        let mut created = NewDirectories::default();
        created.create(directory)?;
        let backup = MessageStore::open(&directory.join("messages.db"))?;
        {
            let source = self.conn.lock().unwrap();
            let snapshot = source.unchecked_transaction()?;
            copy_tables(&snapshot, &mut backup.conn.lock().unwrap())?;
        }
        let mut files = Attachments::new(media_root, directory)?;
        let paths = attachment_paths(&backup.conn.lock().unwrap())?;
        for path in paths {
            let copied = files.copy(&path)?;
            let conn = backup.conn.lock().unwrap();
            rewrite_attachment(&conn, &path, copied.as_deref())?;
        }
        let messages = backup.conn.lock().unwrap().query_row("SELECT COUNT(*) FROM messages", [], |r| r.get::<_, i64>(0))? as u64;
        let connection = backup.conn.into_inner().unwrap();
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")?;
        connection.close().map_err(|(_, error)| error)?;
        write_json(&directory.join("aliases.json"), &aliases)?;
        write_json(&directory.join("manifest.json"), &Manifest { format: "postal-local-backup".into(), version: 1,
            messages, attachments: files.count(), missing_attachments: files.missing })?;
        created.complete();
        Ok(ArchiveReport { directory: directory.to_string_lossy().into(), messages,
            attachments: files.count(), missing_attachments: files.missing })
    }

    pub fn export_conversation(&self, chat: &str, directory: &Path, media_root: &Path) -> Result<ArchiveReport> {
        let mut created = NewDirectories::default();
        created.create(directory)?;
        let mut files = Attachments::new(media_root, directory)?;
        let mut writer = BufWriter::new(File::create_new(directory.join("conversation.json"))?);
        let source = self.conn.lock().unwrap();
        let snapshot = source.unchecked_transaction()?;
        let chat = &*super::names::canonical_chat(&snapshot, chat)?;
        write!(writer, "{{\"format\":\"postal-conversation\",\"version\":1,\"chat\":")?;
        serde_json::to_writer(&mut writer, chat)?;
        write!(writer, ",\"pages\":[")?;
        let mut statement = snapshot.prepare(&format!("SELECT {MESSAGE_COLUMNS} FROM messages m
            LEFT JOIN names n ON n.jid = m.sender WHERE m.chat = ?1 ORDER BY m.timestamp, m.sort_order, m.id"))?;
        let mut rows = statement.query([chat])?;
        let mut count = 0;
        loop {
            let mut messages = Vec::new();
            for _ in 0..500 {
                let Some(row) = rows.next()? else { break };
                let mut message = message_row(row)?;
                message.media.path = message.media.path.as_deref().map(|path| files.copy(path)).transpose()?.flatten();
                message.quote.path = message.quote.path.as_deref().map(|path| files.copy(path)).transpose()?.flatten();
                messages.push(message);
            }
            if messages.is_empty() { break; }
            let ids = messages.iter().map(|m| m.header.id.clone()).collect::<Vec<_>>();
            let marks = Self::marks_on(&snapshot, chat, Some(&ids))?;
            if count > 0 { write!(writer, ",")?; }
            count += messages.len() as u64;
            serde_json::to_writer(&mut writer, &serde_json::json!({"messages": messages, "marks": marks}))?;
        }
        write!(writer, "]}}")?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        created.complete();
        Ok(ArchiveReport { directory: directory.to_string_lossy().into(), messages: count,
            attachments: files.count(), missing_attachments: files.missing })
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, limit: u64) -> Result<T> {
    let file = File::open(path)?;
    anyhow::ensure!(file.metadata()?.len() <= limit, "backup metadata is too large");
    Ok(serde_json::from_reader(BufReader::new(file))?)
}

fn backup_file(root: &Path, name: &str) -> Result<PathBuf> {
    let path = root.join(name).canonicalize()?;
    anyhow::ensure!(path.starts_with(root) && path.is_file(), "backup file escapes its folder");
    Ok(path)
}

pub fn restore_backup(source: &Path, account: &Path, media: &Path) -> Result<ArchiveReport> {
    let source = source.canonicalize()?;
    let manifest: Manifest = read_json(&backup_file(&source, "manifest.json")?, 64 * 1024)?;
    anyhow::ensure!(manifest.format == "postal-local-backup" && manifest.version == 1, "unsupported backup format");
    let aliases: Vec<(String, String)> = read_json(&backup_file(&source, "aliases.json")?, 16 * 1024 * 1024)?;
    let db = backup_file(&source, "messages.db")?;
    let original = Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    original.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON")?;
    anyhow::ensure!(original.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))? == "ok", "backup database is corrupt");
    let version = original.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))?;
    anyhow::ensure!(version > 0, "backup has no supported message schema");
    let source_messages = original.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get::<_, i64>(0))? as u64;
    let source_files = attachment_paths(&original)?.len();
    anyhow::ensure!(source_messages == manifest.messages && source_files == manifest.attachments, "backup counts do not match its manifest");
    let mut created = NewDirectories::default();
    created.create(account)?;
    created.create(media)?;
    let database = account.join("messages.db");
    let mut imported = Connection::open(&database)?;
    super::schema::migrate_to(&imported, version.try_into()?)?;
    copy_tables(&original, &mut imported)?;
    drop(imported);
    let restored = MessageStore::open(&database)?;
    let mut count = 0;
    let paths = attachment_paths(&restored.conn.lock().unwrap())?;
    for path in paths {
        let parts = Path::new(&path).components().collect::<Vec<_>>();
        anyhow::ensure!(matches!(parts.as_slice(), [Component::Normal(folder), Component::Normal(_)] if *folder == "media"), "invalid backup attachment path");
        let input = source.join(&path).canonicalize()?;
        anyhow::ensure!(input.starts_with(&source) && input.is_file(), "backup attachment escapes its folder");
        let target = media.join(Path::new(&path).file_name().ok_or_else(|| anyhow::anyhow!("attachment has no name"))?);
        fs::copy(input, &target)?;
        let destination = target.canonicalize()?.to_string_lossy().into_owned();
        let conn = restored.conn.lock().unwrap();
        rewrite_attachment(&conn, &path, Some(&destination))?;
        count += 1;
    }
    let alias_store = AliasStore::open(&account.join("aliases.db"))?;
    for (jid, alias) in aliases { alias_store.add(&[jid], &alias)?; }
    let messages = restored.conn.lock().unwrap().query_row("SELECT COUNT(*) FROM messages", [], |r| r.get::<_, i64>(0))? as u64;
    created.complete();
    Ok(ArchiveReport { directory: account.to_string_lossy().into(), messages, attachments: count,
        missing_attachments: manifest.missing_attachments })
}

#[cfg(test)]
#[path = "archive_tests.rs"]
mod tests;
