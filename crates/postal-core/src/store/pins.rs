use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PinState {
    pub chat: String,
    pub pinned: bool,
    pub timestamp: i64,
    // Local delivery order survives device clock skew.
    pub sequence: i64,
}

impl PinState {
    pub(crate) fn supersedes(&self, previous: &Self) -> bool {
        self.sequence > previous.sequence
            || (self.sequence == previous.sequence && !self.pinned && previous.pinned)
    }
}

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS pin_state (jid TEXT PRIMARY KEY, pinned INTEGER NOT NULL, timestamp INTEGER NOT NULL, sequence INTEGER NOT NULL);
         INSERT OR IGNORE INTO pin_state SELECT jid, 1, 0, 0 FROM pins;
         INSERT OR IGNORE INTO chats (jid, last_message_at) SELECT jid, 0 FROM pins;",
    )?;
    Ok(())
}

pub(super) fn merge(conn: &Connection, from: &str, to: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO pin_state (jid, pinned, timestamp, sequence) SELECT ?1, pinned, timestamp, sequence FROM pin_state WHERE jid = ?2
         ON CONFLICT(jid) DO UPDATE SET pinned = excluded.pinned, timestamp = excluded.timestamp, sequence = excluded.sequence
         WHERE excluded.sequence > pin_state.sequence
            OR (excluded.sequence = pin_state.sequence AND excluded.pinned < pin_state.pinned)",
        params![to, from],
    )?;
    conn.execute("DELETE FROM pin_state WHERE jid = ?1", params![from])?;
    conn.execute("DELETE FROM pins WHERE jid = ?1 AND EXISTS (SELECT 1 FROM pin_state WHERE jid = ?1 AND pinned = 0)", params![to])?;
    conn.execute("INSERT OR IGNORE INTO pins SELECT jid FROM pin_state WHERE jid = ?1 AND pinned = 1", params![to])?;
    Ok(())
}

fn canonical(conn: &Connection, pins: &[PinState]) -> Result<Vec<PinState>> {
    let mut canonical = BTreeMap::<String, PinState>::new();
    for pin in pins {
        anyhow::ensure!(!pin.chat.is_empty() && pin.timestamp >= 0 && pin.sequence >= 0, "invalid chat pin state");
        let chat = names::canonical_chat(conn, &pin.chat)?.into_owned();
        let pin = PinState { chat: chat.clone(), ..pin.clone() };
        if canonical.get(&chat).map_or(true, |previous| pin.supersedes(previous)) {
            canonical.insert(chat, pin);
        }
    }
    Ok(canonical.into_values().collect())
}

impl MessageStore {
    pub(crate) fn mirror_pin(&self, jid: &str, pinned: bool) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let jid = names::canonical_chat(&conn, jid)?.into_owned();
        let tx = conn.savepoint()?;
        let sequence = tx.query_row("SELECT COALESCE(MAX(sequence), 0) FROM pin_state", [], |row| row.get::<_, i64>(0))?
            .checked_add(1).ok_or_else(|| anyhow::anyhow!("chat pin sequence exhausted"))?;
        tx.execute("INSERT INTO pin_state (jid, pinned, timestamp, sequence) VALUES (?1, ?2, 0, ?3)
            ON CONFLICT(jid) DO UPDATE SET pinned = excluded.pinned, sequence = excluded.sequence", params![jid, pinned, sequence])?;
        if pinned {
            tx.execute("INSERT OR IGNORE INTO pins (jid) VALUES (?1)", params![jid])?;
            tx.execute("INSERT OR IGNORE INTO chats (jid, last_message_at) VALUES (?1, 0)", params![jid])?;
        } else {
            tx.execute("DELETE FROM pins WHERE jid = ?1", params![jid])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn pin_state(&self) -> Result<Vec<PinState>> {
        let conn = self.conn.lock().unwrap();
        let mut statement = conn.prepare("SELECT jid, pinned, timestamp, sequence FROM pin_state ORDER BY timestamp DESC, jid")?;
        let rows = statement.query_map([], |row| Ok(PinState {
            chat: row.get(0)?, pinned: row.get(1)?, timestamp: row.get(2)?, sequence: row.get(3)?,
        }))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub(crate) fn replace_pin_state(&self, pins: &[PinState]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let canonical = canonical(&conn, pins)?;
        let tx = conn.savepoint()?;
        tx.execute_batch("DELETE FROM pins; DELETE FROM pin_state;")?;
        for pin in canonical {
            tx.execute("INSERT INTO pin_state (jid, pinned, timestamp, sequence) VALUES (?1, ?2, ?3, ?4)",
                params![pin.chat, pin.pinned, pin.timestamp, pin.sequence])?;
            if pin.pinned {
                tx.execute("INSERT INTO pins (jid) VALUES (?1)", params![pin.chat])?;
                tx.execute("INSERT OR IGNORE INTO chats (jid, last_message_at) VALUES (?1, 0)", params![pin.chat])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn canonical_pin_state(&self, pins: &[PinState]) -> Result<Vec<PinState>> {
        canonical(&self.conn.lock().unwrap(), pins)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(chat: &str, pinned: bool, timestamp: i64) -> PinState {
        PinState { chat: chat.into(), pinned, timestamp, sequence: timestamp }
    }

    #[test]
    fn pin_state_replaces_absent_pins_and_creates_quiet_chats() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        store.replace_pin_state(&[pin("1@g.us", true, 1), pin("2@g.us", true, 2)]).unwrap();
        assert_eq!(store.chats().unwrap().iter().map(|chat| chat.chat.as_str()).collect::<Vec<_>>(), ["2@g.us", "1@g.us"]);
        store.replace_pin_state(&[pin("2@g.us", true, 2), pin("3@g.us", true, 3)]).unwrap();
        let chats = store.chats().unwrap();
        assert_eq!(chats.iter().filter(|chat| chat.pinned).map(|chat| chat.chat.as_str()).collect::<Vec<_>>(), ["3@g.us", "2@g.us"]);
        assert!(!chats.iter().find(|chat| chat.chat == "1@g.us").unwrap().pinned);
        store.replace_pin_state(&[]).unwrap();
        assert!(store.pinned_chats().unwrap().is_empty());
    }

    #[test]
    fn pin_state_write_is_atomic_and_account_local() {
        let first = MessageStore::open(Path::new(":memory:")).unwrap();
        let second = MessageStore::open(Path::new(":memory:")).unwrap();
        first.replace_pin_state(&[pin("1@g.us", true, 1)]).unwrap();
        first.conn.lock().unwrap().execute_batch("CREATE TRIGGER fail_pin BEFORE INSERT ON pins BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END;").unwrap();
        assert!(first.replace_pin_state(&[pin("2@g.us", true, 2)]).is_err());
        assert_eq!(first.pin_state().unwrap(), [pin("1@g.us", true, 1)]);
        assert_eq!(first.pinned_chats().unwrap(), ["1@g.us"]);
        assert!(second.pin_state().unwrap().is_empty());
    }

    #[test]
    fn pin_state_aliases_keep_latest_unpin_and_do_not_limit_accounts() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        store.set_lid_pn("123", "598123").unwrap();
        store.replace_pin_state(&[
            pin("123@lid", true, 1), pin("598123@s.whatsapp.net", false, 2),
        ]).unwrap();
        assert!(store.pinned_chats().unwrap().is_empty());
        let pins: Vec<_> = (1..=10).map(|n| pin(&format!("{n}@g.us"), true, n)).collect();
        store.replace_pin_state(&pins).unwrap();
        assert_eq!(store.pinned_chats().unwrap().len(), 10);
        assert_eq!(store.pin_state().unwrap().first().unwrap().chat, "10@g.us");
    }

    #[test]
    fn pin_state_mapping_preserves_the_latest_tombstone() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        store.replace_pin_state(&[pin("123@lid", true, 1), pin("598123@s.whatsapp.net", false, 2)]).unwrap();
        store.set_lid_pn("123", "598123").unwrap();
        assert!(store.pinned_chats().unwrap().is_empty());
        assert_eq!(store.pin_state().unwrap(), [pin("598123@s.whatsapp.net", false, 2)]);
    }

    #[test]
    fn pin_state_legacy_mirror_updates_the_projection_and_quiet_row() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        store.set_pinned("1@g.us", true).unwrap();
        assert_eq!(store.pin_state().unwrap(), [PinState { chat: "1@g.us".into(), pinned: true, timestamp: 0, sequence: 1 }]);
        assert!(store.chats().unwrap()[0].pinned);
        store.set_pinned("1@g.us", false).unwrap();
        assert!(!store.pin_state().unwrap()[0].pinned);
        assert!(!store.chats().unwrap()[0].pinned);
    }
}
