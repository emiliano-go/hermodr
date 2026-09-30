use super::*;

pub(crate) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS chat_unarchive (
        jid TEXT PRIMARY KEY, enabled INTEGER NOT NULL CHECK(enabled IN (0,1)));",
    )?;
    Ok(())
}

pub(super) fn merge(conn: &Connection, from: &str, to: &str) -> Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='chat_unarchive')",
        [],
        |row| row.get(0),
    )?;
    if !exists || from == to {
        return Ok(());
    }
    conn.execute(
        "INSERT OR IGNORE INTO chat_unarchive(jid,enabled)
        SELECT ?1,enabled FROM chat_unarchive WHERE jid=?2",
        params![to, from],
    )?;
    conn.execute("DELETE FROM chat_unarchive WHERE jid=?1", [from])?;
    Ok(())
}

impl MessageStore {
    pub fn chat_unarchive(&self, chat: &str) -> Result<Option<bool>> {
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?;
        Ok(conn
            .query_row(
                "SELECT enabled FROM chat_unarchive WHERE jid=?1",
                [chat.as_ref()],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn set_chat_unarchive(&self, chat: &str, enabled: Option<bool>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?;
        match enabled {
            Some(enabled) => {
                conn.execute(
                    "INSERT INTO chat_unarchive(jid,enabled) VALUES (?1,?2)
                ON CONFLICT(jid) DO UPDATE SET enabled=excluded.enabled",
                    params![chat.as_ref(), enabled],
                )?;
            }
            None => {
                conn.execute("DELETE FROM chat_unarchive WHERE jid=?1", [chat.as_ref()])?;
            }
        }
        Ok(())
    }
}

impl StoreWorker {
    pub(crate) async fn chat_unarchive(&self, chat: &str) -> Result<Option<bool>> {
        let chat = chat.to_owned();
        self.run(move |store| store.chat_unarchive(&chat)).await
    }

    pub(crate) async fn set_chat_unarchive(&self, chat: &str, enabled: Option<bool>) -> Result<()> {
        let chat = chat.to_owned();
        self.run(move |store| store.set_chat_unarchive(&chat, enabled))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_inherit_reset_and_follow_aliases_across_merge_and_reopen() {
        let path = std::env::temp_dir().join(format!(
            "postal-unarchive-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = MessageStore::open(&path).unwrap();
        assert_eq!(store.chat_unarchive("100@s.whatsapp.net").unwrap(), None);
        store
            .set_chat_unarchive("100@s.whatsapp.net", Some(false))
            .unwrap();
        store.set_chat_unarchive("300@lid", Some(true)).unwrap();
        merge(&Connection::open_in_memory().unwrap(), "a", "b").unwrap();
        store.set_lid_pn("300", "100").unwrap();
        {
            let conn = store.conn.lock().unwrap();
            assert_eq!(
                conn.query_row(
                    "SELECT COUNT(*) FROM chat_unarchive WHERE jid='300@lid'",
                    [],
                    |row| row.get::<_, u32>(0)
                )
                .unwrap(),
                0
            );
        }
        assert_eq!(store.chat_unarchive("300@lid").unwrap(), Some(false));
        store.set_chat_unarchive("300@lid", None).unwrap();
        assert_eq!(store.chat_unarchive("100@s.whatsapp.net").unwrap(), None);
        store.set_chat_unarchive("300@lid", Some(true)).unwrap();
        store.set_chat_unarchive("400@lid", Some(false)).unwrap();
        store.set_lid_pn("400", "200").unwrap();
        drop(store);
        let store = MessageStore::open(&path).unwrap();
        assert_eq!(store.chat_unarchive("300@lid").unwrap(), Some(true));
        assert_eq!(
            store.chat_unarchive("200@s.whatsapp.net").unwrap(),
            Some(false)
        );
        assert_eq!(store.chat_unarchive("400@lid").unwrap(), Some(false));
        assert_eq!(store.chat_unarchive("unconfigured").unwrap(), None);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }
}
