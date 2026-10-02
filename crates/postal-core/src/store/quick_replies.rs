use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct QuickReply {
    pub id: String,
    pub shortcut: String,
    pub message: String,
    pub keywords: Vec<String>,
    pub count: i32,
    pub associated_label_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct QuickRepliesView {
    pub complete: bool,
    pub replies: Vec<QuickReply>,
}

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS quick_replies (
            id TEXT PRIMARY KEY CHECK(length(id) > 0),
            shortcut TEXT NOT NULL DEFAULT '', message TEXT NOT NULL DEFAULT '',
            keywords TEXT NOT NULL DEFAULT '[]', count INTEGER NOT NULL DEFAULT 0,
            associated_label_ids TEXT NOT NULL DEFAULT '[]',
            deleted INTEGER NOT NULL DEFAULT 0, updated_at INTEGER NOT NULL);",
    )?;
    Ok(())
}

impl MessageStore {
    pub fn quick_replies(&self) -> Result<Vec<QuickReply>> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn.prepare(
            "SELECT id, shortcut, message, keywords, count, associated_label_ids
             FROM quick_replies WHERE deleted = 0 ORDER BY lower(shortcut), shortcut, id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?, row.get::<_, String>(1)?,
                row.get::<_, String>(2)?, row.get::<_, String>(3)?,
                row.get::<_, i32>(4)?, row.get::<_, String>(5)?,
            ))
        })?;
        rows.map(|row| {
            let (id, shortcut, message, keywords, count, associated_label_ids) = row?;
            Ok(QuickReply {
                id, shortcut, message, count,
                keywords: serde_json::from_str(&keywords)?,
                associated_label_ids: serde_json::from_str(&associated_label_ids)?,
            })
        }).collect()
    }

    pub(crate) fn set_quick_reply(
        &self, id: &str, reply: Option<QuickReply>, timestamp: i64,
    ) -> Result<bool> {
        anyhow::ensure!(!id.is_empty() && timestamp >= 0, "invalid quick-reply update");
        let deleted = reply.is_none();
        let reply = reply.unwrap_or_else(|| QuickReply {
            id: id.to_owned(), shortcut: String::new(), message: String::new(),
            keywords: Vec::new(), count: 0, associated_label_ids: Vec::new(),
        });
        anyhow::ensure!(reply.id == id, "quick-reply id changed during update");
        anyhow::ensure!(deleted || (!reply.shortcut.is_empty() && !reply.message.is_empty() && reply.count >= 0),
            "quick reply requires a shortcut, message and nonnegative count");
        let keywords = serde_json::to_string(&reply.keywords)?;
        let labels = serde_json::to_string(&reply.associated_label_ids)?;
        Ok(self.conn.lock().unwrap().execute(
            "INSERT INTO quick_replies (id, shortcut, message, keywords, count, associated_label_ids, deleted, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET shortcut = excluded.shortcut, message = excluded.message,
               keywords = excluded.keywords, count = excluded.count,
               associated_label_ids = excluded.associated_label_ids, deleted = excluded.deleted,
               updated_at = excluded.updated_at
             WHERE excluded.updated_at > quick_replies.updated_at OR
               (excluded.updated_at = quick_replies.updated_at AND excluded.deleted > quick_replies.deleted)",
            params![id, reply.shortcut, reply.message, keywords, reply.count, labels, deleted, timestamp],
        )? > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> MessageStore {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        migrate(&store.conn.lock().unwrap()).unwrap();
        store
    }

    fn reply(id: &str, shortcut: &str, message: &str) -> QuickReply {
        QuickReply { id: id.into(), shortcut: shortcut.into(), message: message.into(),
            keywords: vec!["order".into(), "送貨".into()], count: 7, associated_label_ids: vec!["label-1".into()] }
    }

    #[test]
    fn metadata_roundtrips_and_account_stores_stay_separate() {
        let first = store();
        let second = store();
        let value = reply("opaque", "shipping", "Shipping in two days.\n送貨");
        assert!(first.set_quick_reply("opaque", Some(value.clone()), 10).unwrap());
        assert_eq!(first.quick_replies().unwrap(), vec![value]);
        assert!(second.quick_replies().unwrap().is_empty());
        first.set_quick_reply("earlier", Some(reply("earlier", "billing", "Invoice")), 11).unwrap();
        assert_eq!(first.quick_replies().unwrap()[0].id, "earlier");
    }

    #[test]
    fn replay_is_idempotent_and_tombstones_block_stale_resurrection() {
        let store = store();
        let value = reply("id", "hello", "Hello");
        assert!(store.set_quick_reply("id", Some(value.clone()), 10).unwrap());
        assert!(!store.set_quick_reply("id", Some(value.clone()), 10).unwrap());
        assert!(store.set_quick_reply("id", None, 20).unwrap());
        for timestamp in [0, 10, 19, 20] {
            assert!(!store.set_quick_reply("id", Some(value.clone()), timestamp).unwrap());
        }
        assert!(store.quick_replies().unwrap().is_empty());
        assert!(!store.set_quick_reply("id", None, 20).unwrap());
        assert!(store.set_quick_reply("id", Some(value), 21).unwrap());
        assert_eq!(store.quick_replies().unwrap().len(), 1);
        assert!(store.set_quick_reply("id", None, 21).unwrap());
        assert!(store.set_quick_reply("unseen", None, 30).unwrap());
        assert!(!store.set_quick_reply("unseen", Some(reply("unseen", "late", "Old")), 29).unwrap());
        assert!(store.quick_replies().unwrap().is_empty());
    }

    #[test]
    fn malformed_updates_preserve_stored_reply() {
        let store = store();
        let value = reply("id", "hello", "Hello");
        store.set_quick_reply("id", Some(value.clone()), 10).unwrap();
        for invalid in [reply("id", "", "Hello"), reply("id", "hello", ""), reply("other", "hello", "Hello")] {
            assert!(store.set_quick_reply("id", Some(invalid), 20).is_err());
        }
        let mut negative = value.clone();
        negative.count = -1;
        assert!(store.set_quick_reply("id", Some(negative), 20).is_err());
        assert!(store.set_quick_reply("", None, 20).is_err());
        assert!(store.set_quick_reply("id", None, -1).is_err());
        assert_eq!(store.quick_replies().unwrap(), vec![value]);
    }
}
