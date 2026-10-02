use super::*;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct Album {
    pub parent_id: Option<String>,
    pub expected_images: Option<u32>,
    pub expected_videos: Option<u32>,
    pub index: Option<u32>,
}

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    for (name, declaration) in [("album", "TEXT"), ("album_request_written", "INTEGER NOT NULL DEFAULT 0 CHECK(album_request_written IN (0,1))")] {
        let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('messages') WHERE name=?1)", [name], |row| row.get(0))?;
        if !exists { conn.execute_batch(&format!("ALTER TABLE messages ADD COLUMN {name} {declaration};"))?; }
    }
    Ok(())
}

impl MessageStore {
    pub(crate) fn discard_unsent_album_attempt(&self, expected: &StoredMessage) -> Result<Option<String>> {
        if !expected.header.from_me || expected.local.status.as_deref() != Some("pending") || expected.album.is_none()
            || !matches!(expected.media.kind.as_deref(), Some("album" | "image" | "video")) { return Ok(None); }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        let chat = names::canonical_chat(&tx, &expected.header.chat)?;
        let existing = tx.query_row(&format!("SELECT {MESSAGE_COLUMNS} FROM messages m LEFT JOIN names n ON n.jid=m.sender WHERE m.chat=?1 AND m.id=?2"),
            params![chat.as_ref(), expected.header.id], message_row).optional()?;
        if let Some(row) = existing {
            let written: bool = tx.query_row("SELECT album_request_written FROM messages WHERE chat=?1 AND id=?2",
                params![chat.as_ref(), expected.header.id], |row| row.get(0))?;
            if written || row.local.status.as_deref() != Some("pending") || !row.header.from_me || row.local.revoked || row.local.deleted
                || row.header.sender != expected.header.sender || row.header.timestamp != expected.header.timestamp
                || row.text != expected.text || row.album != expected.album || row.media != expected.media
                || row.spoiler != expected.spoiler || row.quote != expected.quote || row.system != expected.system {
                return Ok(None);
            }
            tx.execute("DELETE FROM messages WHERE chat=?1 AND id=?2", params![chat.as_ref(), expected.header.id])?;
        }
        let path = if let Some(path) = expected.media.path.as_deref() {
            let used: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM messages WHERE media_path=?1 OR reply_to_path=?1
                UNION ALL SELECT 1 FROM stickers WHERE path=?1 UNION ALL SELECT 1 FROM sticker_packs WHERE tray_path=?1)", [path], |row| row.get(0))?;
            (!used).then(|| path.to_owned())
        } else { None };
        tx.commit()?;
        Ok(path)
    }
    pub(crate) fn mark_album_request_written(&self, chat: &str, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?;
        let changed = conn.execute("UPDATE messages SET album_request_written=1 WHERE chat=?1 AND id=?2 AND from_me=1
            AND media_kind='album' AND album IS NOT NULL AND json_extract(album,'$.parent_id') IS NULL
            AND revoked=0 AND deleted=0 AND spoiler=0 AND media_once_kind IS NULL AND system_kind IS NULL",
            params![chat.as_ref(), id])?;
        anyhow::ensure!(changed == 1, "Album parent was removed before recording its request.");
        Ok(())
    }
    pub(crate) fn album_parent_for_send(&self, chat: &str, id: &str) -> Result<StoredMessage> {
        let conn = self.conn.lock().unwrap();
        parent_for_send(&conn, chat, id)
    }

    pub(crate) fn insert_album_child(&self, parent_id: &str, message: &StoredMessage) -> Result<StoredMessage> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        let parent = parent_for_send(&tx, &message.header.chat, parent_id)?;
        anyhow::ensure!(message.header.from_me && message.album.as_ref().and_then(|album| album.parent_id.as_deref()) == Some(parent_id)
            && matches!(message.media.kind.as_deref(), Some("image" | "video")) && !message.local.revoked && !message.local.deleted
            && !message.spoiler && message.media.once_kind.is_none(), "Invalid album child.");
        let mut message = message.clone();
        message.header.chat = parent.header.chat;
        Self::insert_row(&tx, &message)?;
        self.revive_chat(&tx, &message.header.chat)?;
        let row = tx.query_row(&format!("SELECT {MESSAGE_COLUMNS} FROM messages m LEFT JOIN names n ON n.jid=m.sender WHERE m.chat=?1 AND m.id=?2"),
            params![message.header.chat, message.header.id], message_row)?;
        tx.commit()?;
        Ok(row)
    }
}

fn parent_for_send(conn: &Connection, chat: &str, id: &str) -> Result<StoredMessage> {
        let chat = names::canonical_chat(conn, chat)?;
        let hidden: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM hidden_chats WHERE jid = ?1)", [chat.as_ref()], |row| row.get(0))?;
        anyhow::ensure!(!hidden, "Album conversation was removed.");
        let row = conn.query_row(&format!("SELECT {MESSAGE_COLUMNS} FROM messages m LEFT JOIN names n ON n.jid=m.sender WHERE m.chat=?1 AND m.id=?2"),
            params![chat.as_ref(), id], message_row)?;
        let written: bool = conn.query_row("SELECT album_request_written FROM messages WHERE chat=?1 AND id=?2", params![chat.as_ref(), id], |row| row.get(0))?;
        anyhow::ensure!(row.header.from_me && row.media.kind.as_deref() == Some("album")
            && row.album.as_ref().is_some_and(|album| album.parent_id.is_none())
            && !row.local.revoked && !row.local.deleted && !row.spoiler && row.media.once_kind.is_none()
            && row.system.kind.is_none() && (written || matches!(row.local.status.as_deref(), Some("sent" | "delivered" | "read"))),
            "Album parent is private, missing, unconfirmed or no longer available.");
        Ok(row)
}

pub(super) fn merge_written(conn: &Connection, from: &str, to: &str) -> Result<()> {
    conn.execute("UPDATE messages AS target SET album_request_written=1 WHERE target.chat=?2 AND target.from_me=1
        AND EXISTS(SELECT 1 FROM messages source WHERE source.chat=?1 AND source.id=target.id AND source.from_me=1 AND source.album_request_written=1)", params![from, to])?;
    Ok(())
}

impl StoreWorker {
    pub(crate) async fn discard_unsent_album_attempt(&self, expected: &StoredMessage) -> Result<Option<String>> {
        let expected = expected.clone();
        self.run(move |store| store.discard_unsent_album_attempt(&expected)).await
    }
    pub(crate) async fn mark_album_request_written(&self, chat: &str, id: &str) -> Result<()> {
        let (chat, id) = (chat.to_owned(), id.to_owned());
        self.run(move |store| store.mark_album_request_written(&chat, &id)).await
    }
    pub(crate) async fn album_parent_for_send(&self, chat: &str, id: &str) -> Result<StoredMessage> {
        let (chat, id) = (chat.to_owned(), id.to_owned());
        self.run(move |store| store.album_parent_for_send(&chat, &id)).await
    }
    pub(crate) async fn insert_album_child(&self, parent_id: &str, message: &StoredMessage) -> Result<StoredMessage> {
        let (parent_id, message) = (parent_id.to_owned(), message.clone());
        self.run(move |store| store.insert_album_child(&parent_id, &message)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_metadata_survives_store_replay_paging_and_alias_fold() {
        let store = MessageStore::open(std::path::Path::new(":memory:")).unwrap();
        let mut message = StoredMessage {
            header: MessageHeader { chat: "1@lid".into(), id: "child".into(), sender: "1@lid".into(), timestamp: 1, from_me: false },
            text: "caption".into(), media: Media { kind: Some("image".into()), ..Default::default() },
            album: Some(Album { parent_id: Some("parent".into()), index: Some(2), ..Default::default() }),
            ..Default::default()
        };
        store.insert_message(&message).unwrap();
        let original = store.message("1@lid", "child").unwrap();
        assert_eq!(original.album, message.album);
        message.album = None;
        store.insert_message(&message).unwrap();
        assert_eq!(store.message("1@lid", "child").unwrap().album, original.album);
        let conn = store.conn.lock().unwrap();
        super::super::chats::fold_chat(&conn, "1@lid", "2@s.whatsapp.net").unwrap();
        drop(conn);
        let rows = store.messages_for("2@s.whatsapp.net", 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].album, original.album);
        assert_eq!(rows[0].text, "caption");
        let page = store.message_page("2@s.whatsapp.net", 10, None, MessagePageDirection::Before).unwrap();
        assert_eq!(page.messages[0].album, original.album);
        store.clear_chat("2@s.whatsapp.net").unwrap();
        assert!(store.messages_for("2@s.whatsapp.net", 10).unwrap().is_empty());
    }

    #[test]
    fn album_column_migration_preserves_previous_rows_and_is_versioned() {
        let conn = Connection::open_in_memory().unwrap();
        let previous = 29;
        super::super::schema::migrate_to(&conn, previous).unwrap();
        conn.execute("INSERT INTO messages(chat,id,sender,timestamp,from_me,text) VALUES ('a@s','old','a@s',1,0,'kept')", []).unwrap();
        super::super::schema::migrate(&conn).unwrap();
        super::super::schema::migrate(&conn).unwrap();
        let (text, album): (String, Option<String>) = conn.query_row("SELECT text,album FROM messages", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(text, "kept");
        assert_eq!(album, None);
    }

    #[test]
    fn album_continuation_requires_available_sent_own_parent() {
        let store = MessageStore::open(std::path::Path::new(":memory:")).unwrap();
        let parent = StoredMessage { header: MessageHeader { chat: "1@s.whatsapp.net".into(), id: "parent".into(),
            sender: "2@s.whatsapp.net".into(), from_me: true, ..Default::default() },
            media: Media { kind: Some("album".into()), ..Default::default() }, album: Some(Album::default()),
            local: LocalState { status: Some("pending".into()), ..Default::default() }, ..Default::default() };
        store.insert_message(&parent).unwrap();
        assert!(store.album_parent_for_send("1@s.whatsapp.net", "parent").is_err());
        store.mark_album_request_written("1@s.whatsapp.net", "parent").unwrap();
        assert!(store.album_parent_for_send("1@s.whatsapp.net", "parent").is_ok());
        assert_eq!(store.message("1@s.whatsapp.net", "parent").unwrap().local.status.as_deref(), Some("pending"));
        let child = StoredMessage { header: MessageHeader { id: "child".into(), ..parent.header.clone() },
            album: Some(Album { parent_id: Some("parent".into()), ..Default::default() }),
            media: Media { kind: Some("image".into()), ..Default::default() }, ..Default::default() };
        assert!(store.insert_album_child("parent", &child).is_ok());
        assert!(store.album_parent_for_send("3@s.whatsapp.net", "parent").is_err());
        store.conn.lock().unwrap().execute("INSERT INTO hidden_chats(jid) VALUES ('1@s.whatsapp.net')", []).unwrap();
        assert!(store.album_parent_for_send("1@s.whatsapp.net", "parent").is_err());
        assert!(store.insert_album_child("parent", &child).is_err());
        store.clear_chat("1@s.whatsapp.net").unwrap();
        assert!(store.album_parent_for_send("1@s.whatsapp.net", "parent").is_err());
        assert!(store.insert_album_child("parent", &child).is_err());
        assert!(store.messages_for("1@s.whatsapp.net", 10).unwrap().is_empty());
    }
}
