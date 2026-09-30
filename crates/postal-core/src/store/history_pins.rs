use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MessagePinUpdate {
    pub target: String,
    pub remote: Option<String>,
    pub pinned: bool,
    pub timestamp: i64,
    pub expires_at: Option<i64>,
}

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS message_pin_sync (
        chat TEXT PRIMARY KEY, target TEXT NOT NULL, pinned INTEGER NOT NULL,
        timestamp INTEGER NOT NULL, expires_at INTEGER);
        INSERT OR IGNORE INTO message_pin_sync SELECT chat, id, 1, 0, NULL FROM message_pins;")?;
    Ok(())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()
        .min(i64::MAX as u128) as i64
}

pub(super) fn pinned(conn: &Connection, chat: &str) -> Result<Option<String>> {
    Ok(conn.query_row("SELECT p.id FROM message_pins p LEFT JOIN message_pin_sync s ON s.chat = p.chat
        WHERE p.chat = ?1 AND (s.expires_at IS NULL OR s.expires_at > ?2)",
        params![chat, now_ms()], |row| row.get(0)).optional()?)
}

pub(super) fn merge(conn: &Connection, from: &str, to: &str) -> Result<()> {
    conn.execute("INSERT INTO message_pin_sync (chat, target, pinned, timestamp, expires_at)
        SELECT ?1, target, pinned, timestamp, expires_at FROM message_pin_sync WHERE chat = ?2
        ON CONFLICT(chat) DO UPDATE SET target=excluded.target, pinned=excluded.pinned,
        timestamp=excluded.timestamp, expires_at=excluded.expires_at
        WHERE excluded.timestamp > message_pin_sync.timestamp
           OR (excluded.timestamp = message_pin_sync.timestamp AND excluded.pinned < message_pin_sync.pinned)", params![to, from])?;
    conn.execute("DELETE FROM message_pin_sync WHERE chat = ?1", params![from])?;
    conn.execute("DELETE FROM message_pins WHERE chat = ?1 AND EXISTS (SELECT 1 FROM message_pin_sync WHERE chat = ?1)", params![to])?;
    conn.execute("INSERT INTO message_pins (chat, id) SELECT chat, target FROM message_pin_sync WHERE chat = ?1 AND pinned = 1", params![to])?;
    Ok(())
}

impl MessageStore {
    pub(crate) fn mirror_message_pin(&self, chat: &str, id: Option<&str>) -> Result<()> {
        let target = match id {
            Some(id) => Some(id.to_owned()),
            None => {
                let conn = self.conn.lock().unwrap();
                let chat = names::canonical_chat(&conn, chat)?;
                conn.query_row("SELECT target FROM message_pin_sync WHERE chat = ?1", params![chat], |row| row.get::<_, String>(0)).optional()?
            }
        };
        let Some(target) = target else { return Ok(()) };
        self.apply_message_pin_update(chat, &MessagePinUpdate {
            target, remote: None, pinned: id.is_some(), timestamp: now_ms(), expires_at: None,
        }, false).map(|_| ())
    }

    pub(crate) fn apply_message_pin_update(&self, chat: &str, pin: &MessagePinUpdate, history: bool) -> Result<bool> {
        anyhow::ensure!(!pin.target.is_empty() && pin.timestamp >= 0, "invalid message pin metadata");
        let mut conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?.into_owned();
        if let Some(remote) = &pin.remote {
            anyhow::ensure!(names::canonical_chat(&conn, remote)? == chat, "message pin names another chat");
        }
        let tx = conn.savepoint()?;
        let previous = tx.query_row("SELECT target, pinned, timestamp FROM message_pin_sync WHERE chat = ?1",
            params![chat], |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?, row.get::<_, i64>(2)?))).optional()?;
        if let Some((target, pinned, timestamp)) = previous {
            if history && (pin.timestamp < timestamp || (pin.timestamp == timestamp && (!pinned || target != pin.target))) { return Ok(false) }
            if !pin.pinned && pinned && target != pin.target { return Ok(false) }
            if pin.expires_at.is_some_and(|expiry| expiry <= now_ms()) && target != pin.target { return Ok(false) }
        }
        tx.execute("INSERT INTO message_pin_sync (chat, target, pinned, timestamp, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(chat) DO UPDATE SET target=excluded.target, pinned=excluded.pinned,
            timestamp=excluded.timestamp, expires_at=excluded.expires_at",
            params![chat, pin.target, pin.pinned, pin.timestamp, pin.expires_at])?;
        if pin.pinned {
            tx.execute("INSERT INTO message_pins (chat, id) VALUES (?1, ?2) ON CONFLICT(chat) DO UPDATE SET id=excluded.id",
                params![chat, pin.target])?;
        } else {
            tx.execute("DELETE FROM message_pins WHERE chat = ?1", params![chat])?;
        }
        tx.commit()?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(target: &str, pinned: bool, timestamp: i64) -> MessagePinUpdate {
        MessagePinUpdate { target: target.into(), remote: None, pinned, timestamp, expires_at: None }
    }

    #[test]
    fn history_message_pins_keep_newer_pin_and_unpin_tombstone() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        assert!(store.apply_message_pin_update("1@g.us", &update("new", true, 20), true).unwrap());
        assert!(!store.apply_message_pin_update("1@g.us", &update("old", true, 10), true).unwrap());
        assert_eq!(store.marks("1@g.us").unwrap().pinned.as_deref(), Some("new"));
        assert!(!store.apply_message_pin_update("1@g.us", &update("old", false, 30), false).unwrap());
        assert!(store.apply_message_pin_update("1@g.us", &update("new", false, 30), false).unwrap());
        assert!(!store.apply_message_pin_update("1@g.us", &update("new", true, 20), true).unwrap());
        assert!(store.marks("1@g.us").unwrap().pinned.is_none());
    }

    #[test]
    fn history_message_pins_expiry_and_cross_chat_validation() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        let mut pin = update("message", true, 1);
        pin.expires_at = Some(2);
        store.apply_message_pin_update("1@g.us", &pin, true).unwrap();
        assert!(store.marks("1@g.us").unwrap().pinned.is_none());
        pin.remote = Some("2@g.us".into());
        assert!(store.apply_message_pin_update("1@g.us", &pin, true).is_err());
    }

    #[test]
    fn history_message_pin_positive_replay_restores_retained_state_after_pruning() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        let pin = update("message", true, 20);
        store.apply_message_pin_update("1@g.us", &pin, true).unwrap();
        store.conn.lock().unwrap().execute("DELETE FROM message_pins", []).unwrap();
        assert!(store.apply_message_pin_update("1@g.us", &pin, true).unwrap());
        assert_eq!(store.marks("1@g.us").unwrap().pinned.as_deref(), Some("message"));
        store.apply_message_pin_update("1@g.us", &update("message", false, 20), false).unwrap();
        assert!(!store.apply_message_pin_update("1@g.us", &pin, true).unwrap());
        assert!(store.marks("1@g.us").unwrap().pinned.is_none());
    }
}
