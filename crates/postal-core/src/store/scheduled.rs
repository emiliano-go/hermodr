use super::*;
use crate::message_ref::{MessageFailure, MessageRef};

const FAILURE_FORMAT: &str = "postal_scheduled_failure_v1";

#[derive(Serialize, Deserialize)]
struct StoredFailure {
    format: String,
    failure: MessageFailure,
}

fn encode_failure(failure: &MessageFailure) -> Result<String> {
    Ok(serde_json::to_string(&StoredFailure { format: FAILURE_FORMAT.into(), failure: failure.clone() })?)
}

pub fn decode_scheduled_failure(raw: &str, status: &str) -> MessageFailure {
    let value = serde_json::from_str::<serde_json::Value>(raw).ok();
    let marked = value.as_ref().is_some_and(|value| value.get("format").is_some());
    if value.as_ref().is_some_and(|value| value.get("format").and_then(|value| value.as_str()) == Some(FAILURE_FORMAT)) {
        if let Some(stored) = value.and_then(|value| serde_json::from_value::<StoredFailure>(value).ok()) {
            let code = &stored.failure.message.code;
            if code.starts_with("error.") && code.split('.').skip(1).all(|part| !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')) {
                return stored.failure;
            }
        }
    }
    let code = match (marked, status == "uncertain") {
        (true, true) => "error.scheduled_failure_unrecognized_uncertain",
        (true, false) => "error.scheduled_failure_unrecognized",
        (false, true) => "error.scheduled_failure_legacy_uncertain",
        (false, false) => "error.scheduled_failure_legacy",
    };
    MessageFailure { message: MessageRef::new(code), diagnostic: Some(raw.to_owned()) }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct ScheduledMessage {
    pub id: String,
    pub chat: String,
    pub text: String,
    pub mentions: Vec<String>,
    pub due_at: i64,
    pub status: String,
    pub error: Option<String>,
    pub attempted: bool,
}

pub(crate) struct ScheduledOutbox {
    conn: Mutex<Connection>,
}

pub(crate) type ScheduledWorker = super::worker::Worker<ScheduledOutbox>;

impl ScheduledWorker {
    pub(crate) async fn open_with_key(path: &Path, key: Option<crate::database_crypto::DatabaseKey>) -> Result<Self> {
        let path = path.to_owned();
        Ok(Self::new(tokio::task::spawn_blocking(move || ScheduledOutbox::open_with_key(&path, key.as_ref())).await??))
    }
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS scheduled_messages (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            id TEXT NOT NULL UNIQUE, chat TEXT NOT NULL, text TEXT NOT NULL,
            mentions TEXT NOT NULL, due_at INTEGER NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending'
                CHECK(status IN ('pending', 'sending', 'failed', 'uncertain')),
            error TEXT, attempted INTEGER NOT NULL DEFAULT 0);
         CREATE INDEX IF NOT EXISTS scheduled_due ON scheduled_messages(status, due_at, sequence);",
    )?;
    Ok(())
}

fn recover(conn: &Connection) -> Result<()> {
    let failure = encode_failure(&MessageFailure { message: MessageRef::new("error.scheduled_interrupted"), diagnostic: None })?;
    conn.execute(
        "UPDATE scheduled_messages SET status = 'uncertain',
            error = ?1
         WHERE status = 'sending'", [failure],
    )?;
    Ok(())
}

const COLUMNS: &str = "id, chat, text, mentions, due_at, status, error, attempted";

fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScheduledMessage> {
    let mentions: String = row.get(3)?;
    Ok(ScheduledMessage {
        id: row.get(0)?, chat: row.get(1)?, text: row.get(2)?,
        mentions: serde_json::from_str(&mentions).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(error))
        })?,
        due_at: row.get(4)?, status: row.get(5)?, error: row.get(6)?, attempted: row.get(7)?,
    })
}

impl ScheduledOutbox {
    #[cfg(test)]
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with_key(path, None)
    }

    pub fn open_with_key(path: &Path, key: Option<&crate::database_crypto::DatabaseKey>) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let conn = crate::database_crypto::open_database(path, key, rusqlite::OpenFlags::default())
            .with_context(|| format!("opening scheduled outbox at {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        migrate(&conn)?;
        recover(&conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn schedule_message(&self, id: &str, chat: &str, text: &str, mentions: &[String], due_at: i64) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "INSERT INTO scheduled_messages (id, chat, text, mentions, due_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, chat, text, serde_json::to_string(mentions)?, due_at],
        )?;
        Ok(())
    }

    pub fn scheduled_messages(&self) -> Result<Vec<ScheduledMessage>> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn.prepare(&format!("SELECT {COLUMNS} FROM scheduled_messages ORDER BY due_at, sequence"))?;
        let result = statement.query_map([], row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(result)
    }

    pub fn update_scheduled_message(&self, id: &str, text: &str, due_at: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let mut mentions: Vec<String> = serde_json::from_str(&conn.query_row(
            "SELECT mentions FROM scheduled_messages WHERE id = ?1", [id], |row| row.get::<_, String>(0),
        )?)?;
        mentions.retain(|jid| {
            let token = if jid == "@all" { jid.clone() } else { format!("@{}", jid.split('@').next().unwrap_or("").split(':').next().unwrap_or("")) };
            text.contains(&token)
        });
        let changed = conn.execute(
            "UPDATE scheduled_messages SET text = ?2, due_at = ?3, mentions = ?4
             WHERE id = ?1 AND status = 'pending' AND attempted = 0", params![id, text, due_at, serde_json::to_string(&mentions)?],
        )?;
        anyhow::ensure!(changed == 1, MessageRef::new("error.scheduled_edit_forbidden"));
        Ok(())
    }

    pub fn cancel_scheduled_message(&self, id: &str) -> Result<()> {
        let changed = self.conn.lock().unwrap().execute(
            "DELETE FROM scheduled_messages WHERE id = ?1 AND status != 'sending'", [id],
        )?;
        anyhow::ensure!(changed == 1, MessageRef::new("error.scheduled_cancel_forbidden"));
        Ok(())
    }

    pub fn retry_scheduled_message(&self, id: &str) -> Result<()> {
        let changed = self.conn.lock().unwrap().execute(
            "UPDATE scheduled_messages SET status = 'pending', error = NULL
             WHERE id = ?1 AND status IN ('failed', 'uncertain')", [id],
        )?;
        anyhow::ensure!(changed == 1, MessageRef::new("error.scheduled_retry_forbidden"));
        Ok(())
    }

    pub fn claim_scheduled_message(&self, id: &str, now: i64) -> Result<Option<ScheduledMessage>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row(&format!(
            "UPDATE scheduled_messages SET status = 'sending', attempted = 1
             WHERE id = ?1 AND status = 'pending' AND due_at <= ?2
               AND NOT EXISTS (SELECT 1 FROM scheduled_messages WHERE status = 'sending')
               AND sequence = (SELECT sequence FROM scheduled_messages
                   WHERE status = 'pending' AND due_at <= ?2 ORDER BY due_at, sequence LIMIT 1)
             RETURNING {COLUMNS}"), params![id, now], row).optional()?)
    }

    pub fn fail_scheduled_message(&self, id: &str, failure: &MessageFailure, uncertain: bool) -> Result<()> {
        let error = encode_failure(failure)?;
        self.conn.lock().unwrap().execute(
            "UPDATE scheduled_messages SET status = ?2, error = ?3 WHERE id = ?1 AND status = 'sending'",
            params![id, if uncertain { "uncertain" } else { "failed" }, error],
        )?;
        Ok(())
    }

    pub fn complete_scheduled_message(&self, id: &str) -> Result<()> {
        let changed = self.conn.lock().unwrap().execute(
            "DELETE FROM scheduled_messages WHERE id = ?1 AND status = 'sending'", [id],
        )?;
        anyhow::ensure!(changed == 1, MessageRef::new("error.scheduled_claim_lost"));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("postal-scheduled-failure-{}-{}.db", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()))
    }

    fn cleanup(path: &Path) {
        for suffix in ["", "-wal", "-shm"] { let _ = std::fs::remove_file(format!("{}{suffix}", path.display())); }
    }

    #[test]
    fn scheduled_failure_v1_survives_restart_with_code_params_diagnostic_and_state() {
        let path = temporary_path();
        let failure = MessageFailure::from(anyhow::Error::new(MessageRef::new("error.scheduled_delivery_uncertain")
            .with_param("status", "503").with_param("retry_after_seconds", serde_json::Number::from(30)))
            .context("synthetic transport acknowledgement failure"));
        let stored;
        {
            let store = ScheduledOutbox::open(&path).unwrap();
            store.schedule_message("one", "1@lid", "synthetic", &[], 1).unwrap();
            store.claim_scheduled_message("one", 1).unwrap().unwrap();
            store.fail_scheduled_message("one", &failure, true).unwrap();
            stored = store.scheduled_messages().unwrap()[0].error.clone().unwrap();
            assert_eq!(serde_json::from_str::<serde_json::Value>(&stored).unwrap()["format"], FAILURE_FORMAT);
        }
        let store = ScheduledOutbox::open(&path).unwrap();
        let row = store.scheduled_messages().unwrap().remove(0);
        assert_eq!(row.error.as_deref(), Some(stored.as_str()));
        assert_eq!(decode_scheduled_failure(row.error.as_ref().unwrap(), &row.status), failure);
        assert_eq!(row.status, "uncertain");
        assert!(row.attempted);
        assert!(store.claim_scheduled_message("one", 2).unwrap().is_none());
        drop(store);
        cleanup(&path);
    }

    #[test]
    fn scheduled_failure_legacy_malformed_and_future_records_remain_unchanged_on_restart() {
        let path = temporary_path();
        let records = ["legacy untranslated failure",
            r#"{"format":"postal_scheduled_failure_v1","failure":{"code":"error.example","params":{"nested":{}}}}"#,
            r#"{"format":"postal_scheduled_failure_v2","failure":{"code":"error.future","params":{}}}"#,
            r#"{"format":"postal_scheduled_failure_v1","failure":"#];
        {
            let store = ScheduledOutbox::open(&path).unwrap();
            for (index, raw) in records.iter().enumerate() {
                let id = index.to_string();
                store.schedule_message(&id, "1@lid", "synthetic", &[], index as i64).unwrap();
                store.conn.lock().unwrap().execute("UPDATE scheduled_messages SET status = 'failed', attempted = 1, error = ?2 WHERE id = ?1",
                    params![id, raw]).unwrap();
            }
        }
        let store = ScheduledOutbox::open(&path).unwrap();
        let rows = store.scheduled_messages().unwrap();
        assert_eq!(rows.len(), records.len());
        for (row, raw) in rows.iter().zip(records) {
            assert_eq!(row.error.as_deref(), Some(raw));
            let failure = decode_scheduled_failure(raw, &row.status);
            assert_eq!(failure.diagnostic.as_deref(), Some(raw));
            assert!(matches!(failure.message.code.as_str(), "error.scheduled_failure_legacy" | "error.scheduled_failure_unrecognized"));
            assert_eq!(row.status, "failed");
            assert!(row.attempted);
        }
        drop(store);
        cleanup(&path);
    }

    #[test]
    fn scheduled_failure_unknown_formats_keep_uncertainty_and_invalid_codes_keep_raw_detail() {
        for raw in [r#"{"format":"postal_scheduled_failure_v3"}"#,
            r#"{"format":"postal_scheduled_failure_v1","failure":{"code":"bad key","params":{}}}"#,
            r#"{"format":"postal_scheduled_failure_v1","failure":{"code":"error.","params":{}}}"#] {
            let failure = decode_scheduled_failure(raw, "uncertain");
            assert_eq!(failure.message.code, "error.scheduled_failure_unrecognized_uncertain");
            assert_eq!(failure.diagnostic.as_deref(), Some(raw));
        }
        let failure = decode_scheduled_failure("legacy cause", "uncertain");
        assert_eq!(failure.message.code, "error.scheduled_failure_legacy_uncertain");
        assert_eq!(failure.diagnostic.as_deref(), Some("legacy cause"));
    }

    #[test]
    fn scheduled_outbox_never_sends_early_and_retries_keep_the_id() {
        let store = ScheduledOutbox::open(Path::new(":memory:")).unwrap();
        store.schedule_message("first", "1@s.whatsapp.net", "first", &[], 100).unwrap();
        store.schedule_message("second", "1@s.whatsapp.net", "@2 second", &["2@lid".into()], 100).unwrap();
        assert!(store.claim_scheduled_message("first", 99).unwrap().is_none());
        assert!(store.claim_scheduled_message("second", 100).unwrap().is_none());
        assert_eq!(store.claim_scheduled_message("first", 100).unwrap().unwrap().id, "first");
        assert!(store.claim_scheduled_message("first", 100).unwrap().is_none());
        assert!(store.claim_scheduled_message("second", 100).unwrap().is_none());
        assert!(store.cancel_scheduled_message("first").is_err());
        assert!(store.update_scheduled_message("first", "changed", 200).is_err());
        store.fail_scheduled_message("first", &MessageFailure::from(anyhow::anyhow!("synthetic failure")), true).unwrap();
        assert!(store.claim_scheduled_message("first", 101).unwrap().is_none());
        store.retry_scheduled_message("first").unwrap();
        assert!(store.update_scheduled_message("first", "changed", 200).is_err());
        let row = store.claim_scheduled_message("first", 101).unwrap().unwrap();
        assert_eq!((row.id.as_str(), row.text.as_str()), ("first", "first"));
        store.conn.lock().unwrap().execute_batch(
            "CREATE TRIGGER fail_completion BEFORE DELETE ON scheduled_messages
             BEGIN SELECT RAISE(ABORT, 'synthetic completion failure'); END;",
        ).unwrap();
        assert!(store.complete_scheduled_message(&row.id).is_err());
        assert_eq!(store.scheduled_messages().unwrap()[0].status, "sending");
        store.fail_scheduled_message(&row.id, &MessageFailure { message: MessageRef::new("error.scheduled_delivery_uncertain"),
            diagnostic: Some("sent; local confirmation failed".into()) }, true).unwrap();
        assert_eq!(store.scheduled_messages().unwrap()[0].status, "uncertain");
        store.retry_scheduled_message(&row.id).unwrap();
        store.claim_scheduled_message(&row.id, 101).unwrap().unwrap();
        store.conn.lock().unwrap().execute_batch("DROP TRIGGER fail_completion").unwrap();
        store.complete_scheduled_message(&row.id).unwrap();
        assert_eq!(store.scheduled_messages().unwrap().len(), 1);
        store.update_scheduled_message("second", "edited", 200).unwrap();
        assert!(store.scheduled_messages().unwrap()[0].mentions.is_empty());
        assert!(store.claim_scheduled_message("second", 199).unwrap().is_none());
        store.cancel_scheduled_message("second").unwrap();
        assert!(store.scheduled_messages().unwrap().is_empty());
    }

    #[test]
    fn scheduled_outbox_restart_keeps_missed_sends_and_requires_manual_interrupted_retry() {
        let path = std::env::temp_dir().join(format!("postal-scheduled-{}-{}.db", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        {
            let store = ScheduledOutbox::open(&path).unwrap();
            store.schedule_message("missed", "1@s.whatsapp.net", "synthetic", &["2@lid".into()], 10).unwrap();
            store.schedule_message("interrupted", "1@s.whatsapp.net", "synthetic", &[], 5).unwrap();
            store.claim_scheduled_message("interrupted", 5).unwrap().unwrap();
        }
        let store = ScheduledOutbox::open(&path).unwrap();
        let rows = store.scheduled_messages().unwrap();
        assert_eq!(rows[0].status, "uncertain");
        let failure = decode_scheduled_failure(rows[0].error.as_ref().unwrap(), &rows[0].status);
        assert_eq!(failure.message.code, "error.scheduled_interrupted");
        assert!(failure.diagnostic.is_none());
        assert_eq!(rows[1].mentions, ["2@lid"]);
        assert_eq!(store.claim_scheduled_message("missed", 20).unwrap().unwrap().id, "missed");
        assert!(store.claim_scheduled_message("interrupted", 20).unwrap().is_none());
        drop(store);
        for suffix in ["", "-wal", "-shm"] { let _ = std::fs::remove_file(format!("{}{suffix}", path.display())); }
    }
}
