//! Chats: summaries, pins and per-chat settings.

use super::*;

impl MessageStore {
    /// The per chat auto download override, if one is set.
    pub fn chat_auto_download(&self, jid: &str) -> Result<Option<bool>> {
        let conn = self.conn.lock().unwrap();
        let value = conn
            .query_row(
                "SELECT auto_download FROM chat_settings WHERE jid = ?1",
                params![jid],
                |r| r.get::<_, i32>(0),
            )
            .ok();
        Ok(value.map(|v| v != 0))
    }

    /// Sets the per chat auto download override.
    pub fn set_chat_auto_download(&self, jid: &str, enabled: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO chat_settings (jid, auto_download) VALUES (?1, ?2)
             ON CONFLICT(jid) DO UPDATE SET auto_download = excluded.auto_download",
            params![jid, enabled as i32],
        )?;
        Ok(())
    }

    /// The chat's typing and read receipt overrides; `None` follows the global setting.
    pub fn chat_privacy(&self, jid: &str) -> Result<(Option<bool>, Option<bool>)> {
        let conn = self.conn.lock().unwrap();
        let value = conn
            .query_row(
                "SELECT send_typing, send_receipts FROM chat_privacy WHERE jid = ?1",
                params![jid],
                |r| Ok((r.get::<_, Option<bool>>(0)?, r.get::<_, Option<bool>>(1)?)),
            )
            .optional()?;
        Ok(value.unwrap_or_default())
    }

    /// Sets the chat's typing and read receipt overrides; both `None` removes them.
    pub fn set_chat_privacy(&self, jid: &str, typing: Option<bool>, receipts: Option<bool>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        if typing.is_none() && receipts.is_none() {
            conn.execute("DELETE FROM chat_privacy WHERE jid = ?1", params![jid])?;
        } else {
            conn.execute(
                "INSERT INTO chat_privacy (jid, send_typing, send_receipts) VALUES (?1, ?2, ?3)
                 ON CONFLICT(jid) DO UPDATE SET send_typing = excluded.send_typing,
                                                send_receipts = excluded.send_receipts",
                params![jid, typing, receipts],
            )?;
        }
        Ok(())
    }

    /// Mirrors a chat's pin state from the account.
    pub fn set_pinned(&self, jid: &str, pinned: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        if pinned {
            conn.execute("INSERT OR IGNORE INTO pins (jid) VALUES (?1)", params![jid])?;
        } else {
            conn.execute("DELETE FROM pins WHERE jid = ?1", params![jid])?;
        }
        Ok(())
    }

    /// The pinned chats.
    pub fn pinned_chats(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT jid FROM pins")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// One summary per chat, most recently active first.
    ///
    /// Deleted chats stay hidden until a new message arrives; cleared chats
    /// stay as empty rows so the conversation keeps its place in the list.
    pub fn chats(&self) -> Result<Vec<ChatSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            // The preview row is one index seek per chat; a window over every
            // message would copy the whole table into a temporary sort.
            "SELECT g.chat, g.last_message_at, g.message_count, n.name, g.unread_count,
                    g.mention_count, p.jid IS NOT NULL AS pinned,
                    m.text, m.from_me, s.name, m.sender, m.media_kind
             FROM (SELECT chat,
                          MAX(timestamp) AS last_message_at,
                          COUNT(*) AS message_count,
                          SUM(read = 0 AND from_me = 0) AS unread_count,
                          SUM(read = 0 AND from_me = 0 AND mentioned = 1) AS mention_count
                   FROM messages WHERE chat NOT IN (SELECT jid FROM hidden_chats) GROUP BY chat) g
             JOIN messages m ON m.rowid =
                  (SELECT rowid FROM messages WHERE chat = g.chat ORDER BY timestamp DESC LIMIT 1)
             LEFT JOIN names n ON n.jid = g.chat
             LEFT JOIN names s ON s.jid = m.sender
             LEFT JOIN pins p ON p.jid = g.chat
             ORDER BY pinned DESC, g.last_message_at DESC",
        )?;
        let mut summaries = stmt
            .query_map([], |row| {
                Ok(ChatSummary {
                    chat: row.get(0)?,
                    last_message_at: row.get(1)?,
                    message_count: row.get(2)?,
                    display_name: row.get(3)?,
                    unread_count: row.get(4)?,
                    mention_count: row.get(5)?,
                    pinned: row.get::<_, i64>(6)? != 0,
                    last_text: row.get(7)?,
                    last_from_me: row.get::<_, i64>(8)? != 0,
                    last_sender_name: row.get(9)?,
                    last_sender: row.get(10)?,
                    last_media_kind: row.get(11)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        // Cleared chats with no messages left: empty rows that keep the chat
        // in the list. Chats with real messages are already above.
        let mut empty = conn.prepare(
            "SELECT c.jid, n.name, p.jid IS NOT NULL AS pinned
             FROM cleared_chats c
             LEFT JOIN names n ON n.jid = c.jid
             LEFT JOIN pins p ON p.jid = c.jid
             WHERE c.jid NOT IN (SELECT jid FROM hidden_chats)
               AND NOT EXISTS (SELECT 1 FROM messages WHERE chat = c.jid)",
        )?;
        let empties = empty
            .query_map([], |row| {
                Ok(ChatSummary {
                    chat: row.get::<_, String>(0)?,
                    last_message_at: 0,
                    message_count: 0,
                    display_name: row.get(1)?,
                    unread_count: 0,
                    mention_count: 0,
                    pinned: row.get::<_, i64>(2)? != 0,
                    last_text: String::new(),
                    last_from_me: false,
                    last_sender_name: None,
                    last_sender: String::new(),
                    last_media_kind: None,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        summaries.extend(empties);
        summaries.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.last_message_at.cmp(&a.last_message_at)));
        Ok(summaries)
    }

    /// Drops every stored row for one chat, keeping names, pins and settings.
    /// Returns how many messages went. Local-only: the phone keeps its copy.
    fn drop_chat_messages(&self, conn: &rusqlite::Connection, jid: &str) -> Result<usize> {
        conn.execute("DELETE FROM receipts WHERE id IN (SELECT id FROM messages WHERE chat = ?1)", params![jid])?;
        conn.execute("DELETE FROM reactions WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM stars WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM message_pins WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM polls WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM poll_votes WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM events WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM event_responses WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM view_once WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM forwarded WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM edited WHERE chat = ?1", params![jid])?;
        // Media files left without a referent are removed from disk.
        let paths: Vec<String> = {
            let mut stmt = conn.prepare("SELECT media_path FROM messages WHERE chat = ?1 AND media_path IS NOT NULL")?;
            let paths = stmt
                .query_map(params![jid], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?;
            paths
        };
        let removed = conn.execute("DELETE FROM messages WHERE chat = ?1", params![jid])?;
        for path in paths {
            let _ = std::fs::remove_file(path);
        }
        Ok(removed)
    }

    /// Clears one chat: messages go, the empty chat stays in the list.
    pub fn clear_chat(&self, jid: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let removed = self.drop_chat_messages(&conn, jid)?;
        conn.execute("DELETE FROM hidden_chats WHERE jid = ?1", params![jid])?;
        conn.execute("INSERT OR IGNORE INTO cleared_chats (jid) VALUES (?1)", params![jid])?;
        reclaim(&conn, 0)?;
        Ok(removed)
    }

    /// Deletes one chat: messages go and the chat leaves the list until a new
    /// message arrives. Local-only: the phone keeps its copy.
    pub fn delete_chat(&self, jid: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let removed = self.drop_chat_messages(&conn, jid)?;
        conn.execute("DELETE FROM cleared_chats WHERE jid = ?1", params![jid])?;
        conn.execute("DELETE FROM pins WHERE jid = ?1", params![jid])?;
        conn.execute("INSERT OR IGNORE INTO hidden_chats (jid) VALUES (?1)", params![jid])?;
        reclaim(&conn, 0)?;
        Ok(removed)
    }

    /// A new message unhides its chat and retires its kept-empty row.
    pub(crate) fn revive_chat(&self, conn: &rusqlite::Connection, jid: &str) -> Result<()> {
        conn.execute("DELETE FROM hidden_chats WHERE jid = ?1", params![jid])?;
        conn.execute("DELETE FROM cleared_chats WHERE jid = ?1", params![jid])?;
        Ok(())
    }
}
