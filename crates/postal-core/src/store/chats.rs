//! Chats: summaries, pins and per-chat settings.

use super::*;

pub(super) fn reconcile_addresses(conn: &Connection) -> Result<()> {
    // One direct chat can be stored under both its LID and phone-number
    // forms, which shows the same contact twice. Fold the LID copy onto the
    // phone-number one; the write path now keys direct chats by number.
    let lid_chats: Vec<String> = {
        let mut found = std::collections::BTreeSet::new();
        for (table, column) in [
            ("messages", "chat"),
            ("message_pin_sync", "chat"),
            ("chats", "jid"),
            ("chat_state", "jid"),
            ("pins", "jid"),
            ("pin_state", "jid"),
            ("cleared_chats", "jid"),
            ("hidden_chats", "jid"),
        ] {
            let mut stmt = conn.prepare(&format!(
                "SELECT DISTINCT {column} FROM {table} WHERE {column} LIKE '%@lid'"
            ))?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            for row in rows {
                found.insert(row?);
            }
        }
        found.into_iter().collect()
    };
    if !lid_chats.is_empty() {
        let tx = conn.unchecked_transaction()?;
        for lid_chat in &lid_chats {
            let Some(user) = lid_chat.split('@').next().and_then(|u| u.split(':').next()) else {
                continue;
            };
            let pn: Option<String> = tx
                .query_row("SELECT pn FROM lid_pn WHERE lid = ?1", params![user], |r| {
                    r.get(0)
                })
                .optional()?;
            if let Some(pn) = pn {
                fold_chat(&tx, lid_chat, &format!("{pn}@s.whatsapp.net"))?;
            }
        }
        tx.commit()?;
    }
    Ok(())
}

impl MessageStore {
    /// The per chat auto download override, if one is set.
    pub fn chat_auto_download(&self, jid: &str) -> Result<Option<bool>> {
        let conn = self.conn.lock().unwrap();
        let jid = &*names::canonical_chat(&conn, jid)?;
        let value = conn
            .query_row(
                "SELECT auto_download FROM chat_settings WHERE jid = ?1",
                params![jid],
                |r| r.get::<_, i32>(0),
            )
            .optional()?;
        Ok(value.map(|v| v != 0))
    }

    /// Sets the per chat auto download override.
    pub fn set_chat_auto_download(&self, jid: &str, enabled: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let jid = &*names::canonical_chat(&conn, jid)?;
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
        let jid = &*names::canonical_chat(&conn, jid)?;
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
        let jid = &*names::canonical_chat(&conn, jid)?;
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
        self.mirror_pin(jid, pinned)
    }

    /// Mirrors a chat's archive state from the account.
    pub fn set_archived(&self, jid: &str, archived: bool) -> Result<()> {
        self.set_chat_state(jid, "archived", archived as i64)
    }

    /// Whether a chat is archived; false when it has no state row.
    pub fn is_archived(&self, jid: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let jid = &*names::canonical_chat(&conn, jid)?;
        Ok(conn
            .query_row(
                "SELECT archived FROM chat_state WHERE jid = ?1",
                params![jid],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .is_some_and(|v| v != 0))
    }

    /// Mirrors a chat's mute end (seconds; -1 indefinitely, 0 unmuted).
    pub fn set_muted_until(&self, jid: &str, until: i64) -> Result<()> {
        self.set_chat_state(jid, "muted_until", until)
    }

    /// Mirrors a chat's manual unread mark from the account.
    pub fn set_marked_unread(&self, jid: &str, unread: bool) -> Result<()> {
        self.set_chat_state(jid, "marked_unread", unread as i64)
    }

    /// Lifts a manual unread mark; true if one was set.
    pub fn clear_marked_unread(&self, jid: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let jid = &*names::canonical_chat(&conn, jid)?;
        let changed =
            conn.execute("UPDATE chat_state SET marked_unread = 0 WHERE jid = ?1 AND marked_unread = 1", params![jid])?;
        Ok(changed > 0)
    }

    fn set_chat_state(&self, jid: &str, column: &str, value: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let jid = &*names::canonical_chat(&conn, jid)?;
        conn.execute(
            &format!(
                "INSERT INTO chat_state (jid, {column}) VALUES (?1, ?2)
                 ON CONFLICT(jid) DO UPDATE SET {column} = excluded.{column}"
            ),
            params![jid, value],
        )?;
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
            "SELECT c.jid, COALESCE(g.last_message_at, c.last_message_at), COALESCE(g.message_count, 0),
                    n.name, COALESCE(g.unread_count, 0), COALESCE(g.mention_count, 0), p.jid IS NOT NULL AS pinned,
                    COALESCE(CASE WHEN m.spoiler = 1 THEN '[Spoiler]' ELSE m.text END, ''), COALESCE(m.from_me, 0), s.name, COALESCE(m.sender, ''),
                    CASE WHEN m.system_kind IS NOT NULL THEN 'missed_call' ELSE m.media_kind END,
                    COALESCE(cs.archived, 0), COALESCE(cs.muted_until, 0), COALESCE(cs.marked_unread, 0)
             FROM chats c
             LEFT JOIN (SELECT chat,
                          MAX(timestamp) AS last_message_at,
                          COUNT(*) AS message_count,
                          SUM(read = 0 AND from_me = 0 AND deleted = 0) AS unread_count,
                          SUM(read = 0 AND from_me = 0 AND mentioned = 1 AND deleted = 0) AS mention_count
                   FROM messages GROUP BY chat) g ON g.chat = c.jid
             LEFT JOIN messages m ON m.rowid =
                  (SELECT rowid FROM messages WHERE chat = c.jid
                     AND deleted = 0
                     AND (system_kind IS NULL OR system_kind LIKE 'CALL_MISSED%' OR system_kind LIKE 'SILENCED_UNKNOWN_CALLER%')
                   ORDER BY timestamp DESC, sort_order DESC, id DESC LIMIT 1)
             LEFT JOIN names n ON n.jid = c.jid
             LEFT JOIN names s ON s.jid = m.sender
             LEFT JOIN pins p ON p.jid = c.jid
             LEFT JOIN chat_state cs ON cs.jid = c.jid
             WHERE c.jid NOT IN (SELECT jid FROM hidden_chats)
             ORDER BY pinned DESC, (SELECT timestamp FROM pin_state WHERE jid=c.jid) DESC, COALESCE(g.last_message_at, c.last_message_at) DESC, m.sort_order DESC, c.jid",
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
                    archived: row.get::<_, i64>(12)? != 0,
                    muted_until: row.get(13)?,
                    marked_unread: row.get::<_, i64>(14)? != 0,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(summaries)
    }

    /// Drops every stored row for one chat, keeping names, pins and settings.
    /// Returns how many messages went. Local-only: the phone keeps its copy.
    fn drop_chat_messages(&self, conn: &rusqlite::Connection, jid: &str) -> Result<usize> {
        conn.execute("DELETE FROM message_pin_sync WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM receipts WHERE id IN (SELECT id FROM messages WHERE chat = ?1)", params![jid])?;
        conn.execute("DELETE FROM reactions WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM stars WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM message_pins WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM polls WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM poll_votes WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM poll_option_hashes WHERE chat = ?1", params![jid])?;
        conn.execute("DELETE FROM secret_edit_revisions WHERE chat = ?1", params![jid])?;
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
            if let Err(error) = std::fs::remove_file(path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    log::error!("could not remove deleted chat media: {error}");
                }
            }
        }
        Ok(removed)
    }

    /// Clears one chat: messages go, the empty chat stays in the list.
    pub fn clear_chat(&self, jid: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let jid = &*names::canonical_chat(&conn, jid)?;
        let removed = self.drop_chat_messages(&conn, jid)?;
        conn.execute("DELETE FROM hidden_chats WHERE jid = ?1", params![jid])?;
        conn.execute("INSERT OR IGNORE INTO cleared_chats (jid) VALUES (?1)", params![jid])?;
        conn.execute("INSERT OR IGNORE INTO chats (jid) VALUES (?1)", params![jid])?;
        reclaim(&conn, 0)?;
        Ok(removed)
    }

    /// Deletes one chat: messages go and the chat leaves the list until a new
    /// message arrives. Local-only: the phone keeps its copy.
    pub fn delete_chat(&self, jid: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let jid = &*names::canonical_chat(&conn, jid)?;
        let removed = self.drop_chat_messages(&conn, jid)?;
        conn.execute("DELETE FROM cleared_chats WHERE jid = ?1", params![jid])?;
        conn.execute("DELETE FROM pins WHERE jid = ?1", params![jid])?;
        conn.execute("DELETE FROM chats WHERE jid = ?1", params![jid])?;
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

/// Whether any table keeps rows for a chat: messages or any of its list state.
/// Moves every row from one chat to `to`, keeping whatever state either side
/// had. A chat that just gained messages is never left hidden or kept-empty.
pub(crate) fn fold_chat(conn: &Connection, from: &str, to: &str) -> Result<()> {
    if from == to {
        return Ok(());
    }
    copy_shadowed_messages(conn, from, to)?;
    merge_chat_row(conn, from, to)?;
    super::secret_edits::merge(conn, from, to)?;
    move_chat_keyed_tables(conn, from, to, MESSAGE_STATE_TABLES, "chat")?;
    super::history_pins::merge(conn, from, to)?;
    // Messages last, so `to` knows it has history before the state below.
    conn.execute("UPDATE OR IGNORE messages SET chat = ?1 WHERE chat = ?2", params![to, from])?;
    conn.execute("DELETE FROM messages WHERE chat = ?1", params![from])?;
    conn.execute(
        "UPDATE OR IGNORE messages SET reply_to_chat = ?1 WHERE reply_to_chat = ?2",
        params![to, from],
    )?;
    // Archive, mute and unread marks: keep whichever side had them set.
    merge_chat_state(conn, from, to)?;
    merge_pin(conn, from, to)?;
    move_chat_keyed_tables(conn, from, to, CHAT_SETTING_TABLES, "jid")?;
    move_list_flags(conn, from, to)?;
    adopt_name(conn, from, to)?;
    Ok(())
}

/// Tables whose rows are keyed by (chat, message) and travel with messages.
const MESSAGE_STATE_TABLES: &[&str] = &[
    "reactions",
    "stars",
    "message_pins",
    "polls",
    "poll_votes",
    "transcripts",
    "events",
    "event_responses",
    "view_once",
    "forwarded",
    "edited",
];

/// Tables with one row per chat, carrying the chat's own settings.
const CHAT_SETTING_TABLES: &[&str] = &["chat_privacy", "chat_settings", "chat_retention"];

/// Rows a message id exists under in both chats: `INSERT OR IGNORE` would drop
/// the incoming copy, so they are re-inserted under the surviving chat first.
fn copy_shadowed_messages(conn: &Connection, from: &str, to: &str) -> Result<()> {
    let mut duplicates = conn.prepare(&format!(
        "SELECT {MESSAGE_COLUMNS} FROM messages m LEFT JOIN names n ON n.jid = m.sender
         WHERE m.chat = ?1 AND EXISTS (SELECT 1 FROM messages t WHERE t.chat = ?2 AND t.id = m.id)"
    ))?;
    for row in duplicates.query_map(params![from, to], message_row)? {
        let mut row = row?;
        row.header.chat = to.to_string();
        MessageStore::insert_row(conn, &row)?;
    }
    Ok(())
}

fn merge_chat_row(conn: &Connection, from: &str, to: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO chats (jid, last_message_at)
         SELECT ?1, last_message_at FROM chats WHERE jid = ?2
         ON CONFLICT(jid) DO UPDATE SET last_message_at = MAX(chats.last_message_at, excluded.last_message_at)",
        params![to, from],
    )?;
    conn.execute("DELETE FROM chats WHERE jid = ?1", params![from])?;
    Ok(())
}

/// Moves rows in the given chat-keyed tables, keeping the survivor's rows when
/// both sides have one (the first UPDATE wins under `OR IGNORE`).
fn move_chat_keyed_tables(conn: &Connection, from: &str, to: &str, tables: &[&str], column: &str) -> Result<()> {
    for table in tables {
        conn.execute(
            &format!("UPDATE OR IGNORE {table} SET {column} = ?1 WHERE {column} = ?2"),
            params![to, from],
        )?;
        conn.execute(&format!("DELETE FROM {table} WHERE {column} = ?1"), params![from])?;
    }
    Ok(())
}

fn merge_chat_state(conn: &Connection, from: &str, to: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO chat_state (jid, archived, muted_until, marked_unread)
         SELECT ?1, archived, muted_until, marked_unread FROM chat_state WHERE jid = ?2
         ON CONFLICT(jid) DO UPDATE SET
             archived = MAX(chat_state.archived, excluded.archived),
             muted_until = MAX(chat_state.muted_until, excluded.muted_until),
             marked_unread = MAX(chat_state.marked_unread, excluded.marked_unread)",
        params![to, from],
    )?;
    conn.execute("DELETE FROM chat_state WHERE jid = ?1", params![from])?;
    Ok(())
}

fn merge_pin(conn: &Connection, from: &str, to: &str) -> Result<()> {
    if conn
        .query_row("SELECT 1 FROM pins WHERE jid = ?1", params![from], |r| r.get::<_, i64>(0))
        .optional()?
        .is_some()
    {
        conn.execute("INSERT OR IGNORE INTO pins (jid) VALUES (?1)", params![to])?;
    }
    conn.execute("DELETE FROM pins WHERE jid = ?1", params![from])?;
    super::pins::merge(conn, from, to)?;
    Ok(())
}

/// A cleared or deleted chat keeps its empty row or stays hidden only while it
/// has no history; the merged chat must not be hidden.
fn move_list_flags(conn: &Connection, from: &str, to: &str) -> Result<()> {
    let has_messages: Option<i64> = conn
        .query_row("SELECT 1 FROM messages WHERE chat = ?1 LIMIT 1", params![to], |r| r.get(0))
        .optional()?;
    if has_messages.is_none() {
        conn.execute(
            "INSERT OR IGNORE INTO cleared_chats (jid) SELECT ?1 FROM cleared_chats WHERE jid = ?2",
            params![to, from],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO hidden_chats (jid) SELECT ?1 FROM hidden_chats WHERE jid = ?2",
            params![to, from],
        )?;
    }
    conn.execute("DELETE FROM cleared_chats WHERE jid = ?1", params![from])?;
    conn.execute("DELETE FROM hidden_chats WHERE jid = ?1", params![from])?;
    Ok(())
}

/// The phone-number row keeps its name; adopt the other only when it has none.
fn adopt_name(conn: &Connection, from: &str, to: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO names (jid, name, saved)
         SELECT ?1, name, saved FROM names WHERE jid = ?2
           AND NOT EXISTS (SELECT 1 FROM names WHERE jid = ?1)",
        params![to, from],
    )?;
    Ok(())
}

impl StoreWorker {
    pub(crate) async fn chat_auto_download(&self, jid: &str) -> Result<Option<bool>> {
        let jid = jid.to_owned();
        self.run(move |store| store.chat_auto_download(&jid)).await
    }

    pub(crate) async fn set_chat_auto_download(&self, jid: &str, enabled: bool) -> Result<()> {
        let jid = jid.to_owned();
        self.run(move |store| store.set_chat_auto_download(&jid, enabled)).await
    }

    pub(crate) async fn chat_privacy(&self, jid: &str) -> Result<(Option<bool>, Option<bool>)> {
        let jid = jid.to_owned();
        self.run(move |store| store.chat_privacy(&jid)).await
    }

    pub(crate) async fn set_chat_privacy(&self, jid: &str, typing: Option<bool>, receipts: Option<bool>) -> Result<()> {
        let jid = jid.to_owned();
        self.run(move |store| store.set_chat_privacy(&jid, typing, receipts)).await
    }

    pub(crate) async fn set_archived(&self, jid: &str, archived: bool) -> Result<()> {
        let jid = jid.to_owned();
        self.run(move |store| store.set_archived(&jid, archived)).await
    }

    pub(crate) async fn is_archived(&self, jid: &str) -> Result<bool> {
        let jid = jid.to_owned();
        self.run(move |store| store.is_archived(&jid)).await
    }

    pub(crate) async fn set_muted_until(&self, jid: &str, until: i64) -> Result<()> {
        let jid = jid.to_owned();
        self.run(move |store| store.set_muted_until(&jid, until)).await
    }

    pub(crate) async fn set_marked_unread(&self, jid: &str, unread: bool) -> Result<()> {
        let jid = jid.to_owned();
        self.run(move |store| store.set_marked_unread(&jid, unread)).await
    }

    pub(crate) async fn clear_marked_unread(&self, jid: &str) -> Result<bool> {
        let jid = jid.to_owned();
        self.run(move |store| store.clear_marked_unread(&jid)).await
    }

    pub(crate) async fn chats(&self) -> Result<Vec<ChatSummary>> {
        self.run(move |store| store.chats()).await
    }

    pub(crate) async fn clear_chat(&self, jid: &str) -> Result<usize> {
        let jid = jid.to_owned();
        self.run(move |store| store.clear_chat(&jid)).await
    }

    pub(crate) async fn delete_chat(&self, jid: &str) -> Result<usize> {
        let jid = jid.to_owned();
        self.run(move |store| store.delete_chat(&jid)).await
    }
}
