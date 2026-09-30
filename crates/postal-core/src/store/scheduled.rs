use super::*;

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
    pub(crate) async fn open(path: &Path) -> Result<Self> {
        let path = path.to_owned();
        Ok(Self::new(tokio::task::spawn_blocking(move || ScheduledOutbox::open(&path)).await??))
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
    conn.execute(
        "UPDATE scheduled_messages SET status = 'uncertain',
            error = 'Sending was interrupted. Delivery may have succeeded; retry manually.'
         WHERE status = 'sending'", [],
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
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening scheduled outbox at {}", path.display()))?;
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
        anyhow::ensure!(changed == 1, "only unattempted pending messages can be edited");
        Ok(())
    }

    pub fn cancel_scheduled_message(&self, id: &str) -> Result<()> {
        let changed = self.conn.lock().unwrap().execute(
            "DELETE FROM scheduled_messages WHERE id = ?1 AND status != 'sending'", [id],
        )?;
        anyhow::ensure!(changed == 1, "message is already sending or no longer scheduled");
        Ok(())
    }

    pub fn retry_scheduled_message(&self, id: &str) -> Result<()> {
        let changed = self.conn.lock().unwrap().execute(
            "UPDATE scheduled_messages SET status = 'pending', error = NULL
             WHERE id = ?1 AND status IN ('failed', 'uncertain')", [id],
        )?;
        anyhow::ensure!(changed == 1, "only failed or interrupted messages can be retried");
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

    pub fn fail_scheduled_message(&self, id: &str, error: &str, uncertain: bool) -> Result<()> {
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
        anyhow::ensure!(changed == 1, "scheduled send lost its claim");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        store.fail_scheduled_message("first", "synthetic failure", true).unwrap();
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
        store.fail_scheduled_message(&row.id, "sent; local confirmation failed", true).unwrap();
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
        assert!(rows[0].error.as_ref().unwrap().contains("Delivery may have succeeded"));
        assert_eq!(rows[1].mentions, ["2@lid"]);
        assert_eq!(store.claim_scheduled_message("missed", 20).unwrap().unwrap().id, "missed");
        assert!(store.claim_scheduled_message("interrupted", 20).unwrap().is_none());
        drop(store);
        for suffix in ["", "-wal", "-shm"] { let _ = std::fs::remove_file(format!("{}{suffix}", path.display())); }
    }
}
