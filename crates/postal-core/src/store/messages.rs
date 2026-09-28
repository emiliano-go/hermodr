//! Message rows: writing, reading, editing, revoking and deleting them.

use super::*;

impl MessageStore {
    /// Records a message. A repeat of a stored one (a replayed or duplicate
    /// event) refreshes its content but never moves local state backwards:
    /// delivery status only advances, read/mentioned stay set, a known media
    /// file or edited text is kept, and a revoked message is left as it is.
    /// State changes have their own methods (`set_delivery_state`, `mark_read`,
    /// `revoke_message`, `update_message_content`, `set_media_path`).
    pub fn insert_message(&self, message: &StoredMessage) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, &message.header.chat)?;
        let mut canonical;
        let message = if chat != message.header.chat {
            canonical = message.clone();
            canonical.header.chat = chat.into_owned();
            &canonical
        } else { message };
        Self::insert_row(&conn, message)?;
        self.revive_chat(&conn, &message.header.chat)?;
        Ok(())
    }

    pub(super) fn insert_row(conn: &Connection, message: &StoredMessage) -> Result<()> {
        conn.execute(
            "INSERT INTO messages
                 (chat, id, sender, timestamp, from_me, text,
                  media_kind, media_path, reply_to_id, reply_to_text, reply_to_sender,
                  read, revoked, mentioned, status,
                  preview_url, preview_title, preview_desc, preview_thumb,
                  reply_to_kind, reply_to_thumb, media_thumb, media_ref, reply_to_chat,
                  preview_site, preview_color, media_duration, system_kind, system_params,
                  reply_to_view_once, reply_to_recoverable, reply_to_path, reply_to_locator,
                  media_once_kind, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                     ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?29, ?30,
                     ?31, ?32, ?33, ?34, ?35, ?36)
             ON CONFLICT(chat, id) DO UPDATE SET
                 sort_order = CASE WHEN excluded.sort_order > 0 THEN MIN(messages.sort_order, excluded.sort_order) ELSE messages.sort_order END,
                 sender = excluded.sender,
                 timestamp = excluded.timestamp,
                 from_me = excluded.from_me,
                 text = CASE WHEN EXISTS (SELECT 1 FROM edited e
                                          WHERE e.chat = messages.chat AND e.id = messages.id)
                        THEN text ELSE excluded.text END,
                  media_kind = CASE WHEN excluded.media_kind = 'view_once' AND media_path IS NOT NULL
                                    THEN media_kind ELSE excluded.media_kind END,
                  media_path = COALESCE(media_path, excluded.media_path),
                  reply_to_id = COALESCE(excluded.reply_to_id, reply_to_id),
                  reply_to_text = CASE WHEN excluded.reply_to_text IS NULL OR excluded.reply_to_text = ''
                                       THEN reply_to_text ELSE excluded.reply_to_text END,
                  reply_to_sender = COALESCE(excluded.reply_to_sender, reply_to_sender),
                 read = MAX(read, excluded.read),
                 revoked = excluded.revoked,
                 mentioned = MAX(mentioned, excluded.mentioned),
                 status = CASE WHEN ?28 >(CASE status WHEN 'pending' THEN 0 WHEN 'sent' THEN 1
                                           WHEN 'delivered' THEN 2 WHEN 'read' THEN 3 ELSE -1 END)
                          THEN excluded.status ELSE status END,
                 preview_url = excluded.preview_url,
                 preview_title = excluded.preview_title,
                 preview_desc = excluded.preview_desc,
                 preview_thumb = excluded.preview_thumb,
                  reply_to_kind = CASE WHEN excluded.reply_to_kind IS NULL OR excluded.reply_to_kind = ''
                                        THEN reply_to_kind ELSE excluded.reply_to_kind END,
                  reply_to_thumb = COALESCE(excluded.reply_to_thumb, reply_to_thumb),
                 media_thumb = COALESCE(media_thumb, excluded.media_thumb),
                 media_ref = COALESCE(excluded.media_ref, media_ref),
                 media_duration = COALESCE(excluded.media_duration, media_duration),
                  reply_to_chat = COALESCE(excluded.reply_to_chat, reply_to_chat),
                 preview_site = excluded.preview_site,
                 preview_color = excluded.preview_color,
                  system_kind = excluded.system_kind,
                  system_params = excluded.system_params,
                  reply_to_view_once = MAX(reply_to_view_once, excluded.reply_to_view_once),
                  reply_to_recoverable = MAX(reply_to_recoverable, excluded.reply_to_recoverable),
                  reply_to_path = COALESCE(reply_to_path, excluded.reply_to_path),
                  reply_to_locator = COALESCE(excluded.reply_to_locator, reply_to_locator),
                  media_once_kind = COALESCE(excluded.media_once_kind, media_once_kind)
              WHERE revoked = 0",
            params![
                message.header.chat,
                message.header.id,
                message.header.sender,
                message.header.timestamp,
                message.header.from_me as i32,
                message.text,
                message.media.kind,
                message.media.path,
                message.quote.id,
                message.quote.text,
                message.quote.sender,
                message.local.read as i32,
                message.local.revoked as i32,
                message.local.mentioned as i32,
                message.local.status,
                message.link.url,
                message.link.title,
                message.link.desc,
                message.link.thumb,
                message.quote.kind,
                message.quote.thumb,
                message.media.thumb,
                message.media.locator,
                message.quote.chat,
                message.link.site,
                message.link.color,
                message.media.duration,
                message.local.status.as_deref().map(status_rank).unwrap_or(-1),
                message.system.kind,
                (!message.system.params.is_empty()).then(|| serde_json::to_string(&message.system.params)).transpose()?,
                message.quote.view_once as i32,
                message.quote.recoverable as i32,
                message.quote.path,
                message.quote.locator,
                message.media.once_kind,
                message.local.sort_order,
            ],
        )?;
        Ok(())
    }

    /// Messages in a chat, newest first.
    pub fn messages_for(&self, chat: &str, limit: u32) -> Result<Vec<StoredMessage>> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS}
             FROM messages m
             LEFT JOIN names n ON n.jid = m.sender
             WHERE m.chat = ?1
             ORDER BY m.timestamp DESC, m.sort_order DESC, m.id DESC LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![chat, limit], message_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Messages that mention us, in one chat or all of them, newest first.
    pub fn pings(&self, chat: Option<&str>, limit: u32) -> Result<Vec<StoredMessage>> {
        let conn = self.conn.lock().unwrap();
        let chat = chat.map(|jid| names::canonical_chat(&conn, jid)).transpose()?;
        let chat = chat.as_deref();
        let mut stmt = conn.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS}
             FROM messages m
             LEFT JOIN names n ON n.jid = m.sender
             WHERE m.mentioned = 1 AND m.from_me = 0 AND m.deleted = 0 AND (?1 IS NULL OR m.chat = ?1)
             ORDER BY m.timestamp DESC, m.sort_order DESC, m.id DESC LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![chat, limit], message_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Messages in a chat whose text contains `query`, ignoring case, newest first.
    /// Scans only that chat's rows; tested to stay under a second at 50 000.
    pub fn search_messages(&self, chat: &str, query: &str, limit: u32) -> Result<Vec<StoredMessage>> {
        let escaped = query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        let pattern = format!("%{}%", escaped.to_lowercase());
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS}
             FROM messages m
             LEFT JOIN names n ON n.jid = m.sender
             WHERE m.chat = ?1 AND lower(m.text) LIKE ?2 ESCAPE '\\' AND m.deleted = 0
             ORDER BY m.timestamp DESC, m.sort_order DESC, m.id DESC LIMIT ?3"
        ))?;
        let rows = stmt.query_map(params![chat, pattern, limit], message_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Starred messages across every chat, newest first.
    pub fn starred_messages(&self) -> Result<Vec<StoredMessage>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS}
             FROM stars s
             JOIN messages m ON m.chat = s.chat AND m.id = s.id
             LEFT JOIN names n ON n.jid = m.sender
             ORDER BY m.timestamp DESC, m.sort_order DESC, m.id DESC"
        ))?;
        let rows = stmt.query_map([], message_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// The oldest stored message in a chat, as (id, from_me, timestamp).
    pub fn oldest_message(&self, chat: &str) -> Result<Option<(String, bool, i64)>> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let row = conn
            .query_row(
                "SELECT id, from_me, timestamp FROM messages
                 WHERE chat = ?1 ORDER BY timestamp ASC, sort_order ASC, id ASC LIMIT 1",
                params![chat],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i32>(1)? != 0,
                        r.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;
        Ok(row)
    }

    /// Whether a system line of `kind` already sits within a few seconds of
    /// `timestamp`: the live notification and the history stub for one change
    /// carry different ids but the same server time.
    // Params are not compared, so two changes of one kind within those seconds collapse into one line.
    pub fn has_system_near(&self, chat: &str, kind: &str, timestamp: i64) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let found = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM messages
             WHERE chat = ?1 AND system_kind = ?2 AND ABS(timestamp - ?3) <= 5)",
            params![chat, kind, timestamp],
            |r| r.get::<_, bool>(0),
        )?;
        Ok(found)
    }

    /// The chat a stored message id belongs to, when it is known locally. A
    /// quoted message in another chat can be located with this.
    pub fn chat_of_message(&self, id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let chat = conn
            .query_row(
                "SELECT chat FROM messages WHERE id = ?1 LIMIT 1",
                params![id],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(chat)
    }

    /// Replaces a message's text (its caption, for media) after its sender edited it.
    pub fn update_message_content(&self, chat: &str, id: &str, text: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let changed = conn.execute(
            "UPDATE messages SET text = ?3 WHERE chat = ?1 AND id = ?2",
            params![chat, id, text],
        )?;
        if changed > 0 {
            conn.execute("INSERT OR IGNORE INTO edited (chat, id) VALUES (?1, ?2)", params![chat, id])?;
        }
        Ok(changed > 0)
    }

    /// Marks a message deleted on this device only. The row and its marks are
    /// kept, so the chat can still show it greyed out and nothing on WhatsApp
    /// changes.
    pub fn set_message_deleted(&self, chat: &str, id: &str, deleted: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        conn.execute(
            "UPDATE messages SET deleted = ?3 WHERE chat = ?1 AND id = ?2",
            params![chat, id, deleted as i32],
        )?;
        Ok(())
    }

    /// Unread messages in `chat` that mention us, oldest first.
    pub fn unread_mentions(&self, chat: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let mut stmt = conn.prepare(
            "SELECT id FROM messages
             WHERE chat = ?1 AND read = 0 AND from_me = 0 AND mentioned = 1 AND deleted = 0
             ORDER BY timestamp ASC, sort_order ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![chat], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// A single stored message.
    pub fn message(&self, chat: &str, id: &str) -> Result<StoredMessage> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let message = conn.query_row(
            &format!(
                "SELECT {MESSAGE_COLUMNS}
                 FROM messages m
                 LEFT JOIN names n ON n.jid = m.sender
                 WHERE m.chat = ?1 AND m.id = ?2"
            ),
            params![chat, id],
            message_row,
        )?;
        Ok(message)
    }

    /// Marks a message as deleted by its sender, clearing its content.
    ///
    /// The row is kept so the chat shows that something was removed rather than
    /// silently losing a message. A recovered view-once copy it quoted is
    /// dropped with the quote, so it stops holding a file open too. Returns
    /// whether a row was updated.
    pub fn revoke_message(&self, chat: &str, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let changed = conn.execute(
              "UPDATE messages
               SET revoked = 1, text = '', media_kind = NULL, media_path = NULL,
                   media_once_kind = NULL,
                   reply_to_id = NULL, reply_to_text = NULL, reply_to_sender = NULL,
                   reply_to_kind = NULL, reply_to_thumb = NULL, reply_to_chat = NULL,
                   reply_to_view_once = 0, reply_to_recoverable = 0,
                   reply_to_path = NULL, reply_to_locator = NULL
              WHERE chat = ?1 AND id = ?2 AND revoked = 0",
            params![chat, id],
        )?;
        Ok(changed > 0)
    }
}

impl StoreWorker {
    pub(crate) async fn insert_message(&self, message: &StoredMessage) -> Result<()> {
        let message = message.clone();
        self.run(move |store| store.insert_message(&message)).await
    }

    pub(crate) async fn messages_for(&self, chat: &str, limit: u32) -> Result<Vec<StoredMessage>> {
        let chat = chat.to_owned();
        self.run(move |store| store.messages_for(&chat, limit)).await
    }

    pub(crate) async fn pings(&self, chat: Option<&str>, limit: u32) -> Result<Vec<StoredMessage>> {
        let chat = chat.map(str::to_owned);
        self.run(move |store| store.pings(chat.as_deref(), limit)).await
    }

    pub(crate) async fn search_messages(&self, chat: &str, query: &str, limit: u32) -> Result<Vec<StoredMessage>> {
        let chat = chat.to_owned();
        let query = query.to_owned();
        self.run(move |store| store.search_messages(&chat, &query, limit)).await
    }

    pub(crate) async fn starred_messages(&self) -> Result<Vec<StoredMessage>> {
        self.run(move |store| store.starred_messages()).await
    }

    pub(crate) async fn oldest_message(&self, chat: &str) -> Result<Option<(String, bool, i64)>> {
        let chat = chat.to_owned();
        self.run(move |store| store.oldest_message(&chat)).await
    }

    pub(crate) async fn has_system_near(&self, chat: &str, kind: &str, timestamp: i64) -> Result<bool> {
        let chat = chat.to_owned();
        let kind = kind.to_owned();
        self.run(move |store| store.has_system_near(&chat, &kind, timestamp)).await
    }

    pub(crate) async fn chat_of_message(&self, id: &str) -> Result<Option<String>> {
        let id = id.to_owned();
        self.run(move |store| store.chat_of_message(&id)).await
    }

    pub(crate) async fn update_message_content(&self, chat: &str, id: &str, text: &str) -> Result<bool> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        let text = text.to_owned();
        self.run(move |store| store.update_message_content(&chat, &id, &text)).await
    }

    pub(crate) async fn set_message_deleted(&self, chat: &str, id: &str, deleted: bool) -> Result<()> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        self.run(move |store| store.set_message_deleted(&chat, &id, deleted)).await
    }

    pub(crate) async fn unread_mentions(&self, chat: &str) -> Result<Vec<String>> {
        let chat = chat.to_owned();
        self.run(move |store| store.unread_mentions(&chat)).await
    }

    pub(crate) async fn message(&self, chat: &str, id: &str) -> Result<StoredMessage> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        self.run(move |store| store.message(&chat, &id)).await
    }

    pub(crate) async fn revoke_message(&self, chat: &str, id: &str) -> Result<bool> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        self.run(move |store| store.revoke_message(&chat, &id)).await
    }
}
