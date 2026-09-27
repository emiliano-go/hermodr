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
                   FROM messages GROUP BY chat) g
             JOIN messages m ON m.rowid =
                  (SELECT rowid FROM messages WHERE chat = g.chat ORDER BY timestamp DESC LIMIT 1)
             LEFT JOIN names n ON n.jid = g.chat
             LEFT JOIN names s ON s.jid = m.sender
             LEFT JOIN pins p ON p.jid = g.chat
             ORDER BY pinned DESC, g.last_message_at DESC",
        )?;
        let summaries = stmt
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
        Ok(summaries)
    }
}
