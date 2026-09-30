use super::*;

impl MessageStore {
    pub(crate) fn save_created_group(&self, jid: &str, subject: &str, timestamp: i64) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        tx.execute("INSERT INTO chats (jid, last_message_at) VALUES (?1, ?2)
            ON CONFLICT(jid) DO UPDATE SET last_message_at = MAX(chats.last_message_at, excluded.last_message_at)", params![jid, timestamp])?;
        if is_placeholder_name(subject) {
            tx.execute("INSERT OR IGNORE INTO names (jid, name, saved) VALUES (?1, ?2, 0)", params![jid, subject])?;
        } else {
            tx.execute("INSERT INTO names (jid, name, saved) VALUES (?1, ?2, 0)
                ON CONFLICT(jid) DO UPDATE SET name = excluded.name, saved = 0 WHERE saved = 0", params![jid, subject])?;
        }
        tx.execute("DELETE FROM hidden_chats WHERE jid = ?1", params![jid])?;
        tx.commit()?;
        Ok(())
    }
}

impl StoreWorker {
    pub(crate) async fn save_created_group(&self, jid: &str, subject: &str, timestamp: i64) -> Result<()> {
        let (jid, subject) = (jid.to_string(), subject.to_string());
        self.run(move |store| store.save_created_group(&jid, &subject, timestamp)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmed_empty_group_is_listed_and_repeat_does_not_regress_it() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        let jid = "12345-678@g.us";
        store.save_created_group(jid, "Synthetic group", 100).unwrap();
        let chats = store.chats().unwrap();
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].chat, jid);
        assert_eq!(chats[0].display_name.as_deref(), Some("Synthetic group"));
        assert_eq!(chats[0].message_count, 0);
        assert_eq!(chats[0].last_message_at, 100);
        store.set_saved_name(jid, "Local name").unwrap();
        store.save_created_group(jid, "Server subject", 50).unwrap();
        let chats = store.chats().unwrap();
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].last_message_at, 100);
        assert_eq!(chats[0].display_name.as_deref(), Some("Local name"));
    }

    #[test]
    fn confirmed_group_save_rolls_back_chat_when_name_write_fails() {
        let store = MessageStore::open(Path::new(":memory:")).unwrap();
        store.conn.lock().unwrap().execute_batch("CREATE TRIGGER fail_created_group_name BEFORE INSERT ON names
            BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END;").unwrap();
        assert!(store.save_created_group("12345-678@g.us", "Synthetic group", 100).is_err());
        assert!(store.chats().unwrap().is_empty());
    }
}
