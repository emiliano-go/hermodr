use super::*;

fn has_column(conn: &Connection) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('chat_settings') WHERE name='sound_muted')",
        [], |row| row.get(0),
    )?)
}

pub(crate) fn migrate(conn: &Connection) -> Result<()> {
    if !has_column(conn)? {
        conn.execute_batch("ALTER TABLE chat_settings ADD COLUMN sound_muted INTEGER CHECK(sound_muted IN (0,1));")?;
    }
    Ok(())
}

pub(super) fn merge(conn: &Connection, from: &str, to: &str) -> Result<()> {
    if from != to && has_column(conn)? {
        conn.execute(
            "UPDATE chat_settings SET sound_muted=COALESCE(sound_muted,
                (SELECT sound_muted FROM chat_settings WHERE jid=?2)) WHERE jid=?1",
            params![to, from],
        )?;
    }
    Ok(())
}

impl MessageStore {
    pub fn chat_sound_muted(&self, chat: &str) -> Result<Option<bool>> {
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?;
        Ok(conn.query_row("SELECT sound_muted FROM chat_settings WHERE jid=?1",
            [chat.as_ref()], |row| row.get::<_, Option<bool>>(0)).optional()?.flatten())
    }

    pub fn set_chat_sound_muted(&self, chat: &str, muted: Option<bool>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?;
        conn.execute(
            "INSERT INTO chat_settings(jid,auto_download,sound_muted) VALUES (?1,1,?2)
             ON CONFLICT(jid) DO UPDATE SET sound_muted=excluded.sound_muted",
            params![chat.as_ref(), muted],
        )?;
        Ok(())
    }
}

impl StoreWorker {
    pub(crate) async fn chat_sound_muted(&self, chat: &str) -> Result<Option<bool>> {
        let chat = chat.to_owned();
        self.run(move |store| store.chat_sound_muted(&chat)).await
    }

    pub(crate) async fn set_chat_sound_muted(&self, chat: &str, muted: Option<bool>) -> Result<()> {
        let chat = chat.to_owned();
        self.run(move |store| store.set_chat_sound_muted(&chat, muted)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> MessageStore {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE chat_settings(jid TEXT PRIMARY KEY,auto_download INTEGER NOT NULL,auto_transcribe INTEGER);
            CREATE TABLE lid_pn(lid TEXT,pn TEXT);
            INSERT INTO chat_settings VALUES ('100@s.whatsapp.net',0,1);
            INSERT INTO lid_pn VALUES ('300','100');").unwrap();
        migrate(&conn).unwrap();
        MessageStore { conn: Mutex::new(conn) }
    }

    #[test]
    fn migration_and_updates_preserve_other_chats_and_settings() {
        let store = fixture();
        assert_eq!(store.chat_sound_muted("100@s.whatsapp.net").unwrap(), None);
        assert_eq!(store.chat_sound_muted("200@s.whatsapp.net").unwrap(), None);
        store.set_chat_sound_muted("300@lid", Some(true)).unwrap();
        migrate(&store.conn.lock().unwrap()).unwrap();
        assert_eq!(store.chat_sound_muted("100@s.whatsapp.net").unwrap(), Some(true));
        assert_eq!(store.chat_sound_muted("200@s.whatsapp.net").unwrap(), None);
        assert_eq!(store.conn.lock().unwrap().query_row(
            "SELECT auto_download,auto_transcribe FROM chat_settings WHERE jid='100@s.whatsapp.net'",
            [], |row| Ok((row.get::<_, bool>(0)?, row.get::<_, bool>(1)?))).unwrap(), (false, true));
        store.set_chat_sound_muted("100@s.whatsapp.net", Some(false)).unwrap();
        assert_eq!(store.chat_sound_muted("300@lid").unwrap(), Some(false));
        store.set_chat_sound_muted("100@s.whatsapp.net", None).unwrap();
        assert_eq!(store.chat_sound_muted("300@lid").unwrap(), None);
        let other = fixture();
        store.set_chat_sound_muted("200@s.whatsapp.net", Some(true)).unwrap();
        assert_eq!(other.chat_sound_muted("200@s.whatsapp.net").unwrap(), None);
        store.conn.lock().unwrap().execute_batch("DROP TABLE chat_settings").unwrap();
        assert!(store.chat_sound_muted("200@s.whatsapp.net").is_err());
    }

    #[test]
    fn alias_merge_keeps_explicit_choice_and_other_columns() {
        let store = fixture();
        let conn = store.conn.lock().unwrap();
        conn.execute_batch("INSERT INTO chat_settings VALUES ('300@lid',1,0,1);").unwrap();
        merge(&conn, "300@lid", "100@s.whatsapp.net").unwrap();
        assert_eq!(conn.query_row("SELECT sound_muted FROM chat_settings WHERE jid='100@s.whatsapp.net'",
            [], |row| row.get::<_, bool>(0)).unwrap(), true);
        conn.execute_batch("UPDATE chat_settings SET sound_muted=0 WHERE jid='100@s.whatsapp.net';").unwrap();
        merge(&conn, "300@lid", "100@s.whatsapp.net").unwrap();
        assert_eq!(conn.query_row("SELECT auto_download,auto_transcribe,sound_muted FROM chat_settings WHERE jid='100@s.whatsapp.net'",
            [], |row| Ok((row.get::<_, bool>(0)?, row.get::<_, bool>(1)?, row.get::<_, bool>(2)?))).unwrap(), (false, true, false));
        merge(&conn, "300@lid", "400@s.whatsapp.net").unwrap();
        conn.execute("UPDATE chat_settings SET jid='400@s.whatsapp.net' WHERE jid='300@lid'", []).unwrap();
        assert_eq!(conn.query_row("SELECT auto_download,auto_transcribe,sound_muted FROM chat_settings WHERE jid='400@s.whatsapp.net'",
            [], |row| Ok((row.get::<_, bool>(0)?, row.get::<_, bool>(1)?, row.get::<_, bool>(2)?))).unwrap(), (true, false, true));
    }
}
