//! Media: locators, downloaded files, view-once state and relocation.

use super::*;

/// A reply that quotes a view-once, with whatever copy of it was kept.
#[derive(Debug, Clone)]
pub struct QuoteCopy {
    pub chat: String,
    pub id: String,
    pub from_me: bool,
    /// Message the reply quotes, as the wire id.
    pub quoted: Option<String>,
    /// Who wrote it, `@me` for this account.
    pub sender: Option<String>,
    /// Whether the gate lets this account take it.
    pub allowed: bool,
    /// The copy carried inside the reply, when one was recorded.
    pub locator: Option<Vec<u8>>,
    /// Media kind of the quoted message, as stored.
    pub quoted_kind: Option<String>,
    /// Whether the quoted message kept a locator of its own.
    pub quoted_stored: bool,
}

/// A reply carrying a copy of some message, found by the message it quotes.
#[derive(Debug, Clone)]
pub struct QuoteSource {
    /// Chat the reply is in.
    pub chat: String,
    /// The reply's own id.
    pub id: String,
    /// The copy of the quoted message, as it arrived inside the reply.
    pub locator: Vec<u8>,
}

impl MessageStore {
    /// Every downloaded media file the stored messages point at, including the
    /// recovered view-once copies a reply quotes.
    pub fn media_paths(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT media_path FROM messages WHERE media_path IS NOT NULL
             UNION SELECT reply_to_path FROM messages WHERE reply_to_path IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Where a reply's recovered view-once copy was written.
    pub fn quote_media_path(&self, chat: &str, id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let value = conn
            .query_row(
                "SELECT reply_to_path FROM messages WHERE chat = ?1 AND id = ?2",
                params![chat, id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        Ok(value)
    }

    /// Every recovered view-once file a stored message still points at.
    pub fn quote_media_paths(&self) -> Result<std::collections::HashSet<String>> {
        let conn = self.conn.lock().unwrap();
        let rows = conn
            .prepare("SELECT DISTINCT reply_to_path FROM messages WHERE reply_to_path IS NOT NULL")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows.into_iter().collect())
    }

    /// Records where a reply's recovered view-once copy was written, on the row
    /// and on every other reply quoting the same message, so they all show it.
    pub fn set_quote_media_path(&self, chat: &str, id: &str, path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let quoted: Option<String> = conn
            .query_row(
                "SELECT reply_to_id FROM messages WHERE chat = ?1 AND id = ?2",
                params![chat, id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let Some(quoted) = quoted.filter(|q| !q.is_empty()) else {
            conn.execute(
                "UPDATE messages SET reply_to_path = ?3 WHERE chat = ?1 AND id = ?2",
                params![chat, id, path],
            )?;
            return Ok(());
        };
        // Matched on the quoted id alone, since it is unique across chats and
        // the copy is the same one wherever it is quoted from.
        conn.execute(
            "UPDATE messages SET reply_to_path = ?2
             WHERE reply_to_id = ?3 AND (reply_to_path IS NULL OR reply_to_path = '')",
            params![chat, path, quoted],
        )?;
        Ok(())
    }

    /// The stored media reference for a message.
    pub fn media_ref_for(&self, chat: &str, id: &str) -> Result<Option<Vec<u8>>> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let value = conn
            .query_row(
                "SELECT media_ref FROM messages WHERE chat = ?1 AND id = ?2",
                params![chat, id],
                |r| r.get::<_, Option<Vec<u8>>>(0),
            )
            .optional()?
            .flatten();
        Ok(value)
    }

    /// Replaces a message's media reference, after the sender uploaded it again.
    pub fn set_media_ref(&self, chat: &str, id: &str, media_ref: &[u8]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        conn.execute(
            "UPDATE messages SET media_ref = ?3 WHERE chat = ?1 AND id = ?2",
            params![chat, id, media_ref],
        )?;
        Ok(())
    }

    /// Records where a downloaded file was written.
    pub fn set_media_path(&self, chat: &str, id: &str, path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        conn.execute(
            "UPDATE messages SET media_path = ?3 WHERE chat = ?1 AND id = ?2",
            params![chat, id, path],
        )?;
        Ok(())
    }

    /// Records a preview for downloaded media whose sender sent none.
    pub fn set_media_thumb(&self, chat: &str, id: &str, thumb: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        conn.execute(
            "UPDATE messages SET media_thumb = ?3 WHERE chat = ?1 AND id = ?2",
            params![chat, id, thumb],
        )?;
        Ok(())
    }

    /// Records what a view-once turned out to be, once a copy of it is taken.
    pub fn set_once_kind(&self, chat: &str, id: &str, kind: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        conn.execute(
            "UPDATE messages SET media_once_kind = ?3 WHERE chat = ?1 AND id = ?2",
            params![chat, id, kind],
        )?;
        Ok(())
    }

    /// Incoming one-time messages newer than `within` that still have no media
    /// file, as `(chat, id)`. The Android companion wakes on these and goes
    /// dormant again once they are fetched; the window bounds rows the phone
    /// probably spent already. Sourced from the small view-once mark table, so
    /// it does not scan the message history.
    pub fn pending_view_once(&self, within: std::time::Duration) -> Result<Vec<(String, String)>> {
        let since = unix_now() - within.as_secs() as i64;
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT v.chat, v.id FROM view_once v
             JOIN messages m ON m.chat = v.chat AND m.id = v.id
             WHERE v.opened = 0 AND m.from_me = 0 AND m.media_path IS NULL
               AND m.timestamp >= ?1",
        )?;
        let rows = stmt
            .query_map(params![since], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Turns a view-once whose media was kept into an ordinary attachment: the
    /// original kind comes back and the one-time mark is dropped, so it renders
    /// and opens like any other media instead of being deleted unseen.
    /// `media_once_kind` is kept as the marker the UI uses to tell a kept
    /// one-time attachment apart from an ordinary one.
    pub fn keep_view_once(&self, chat: &str, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        conn.execute(
            "UPDATE messages
             SET media_kind = COALESCE(media_once_kind, media_kind)
             WHERE chat = ?1 AND id = ?2",
            params![chat, id],
        )?;
        conn.execute("DELETE FROM view_once WHERE chat = ?1 AND id = ?2", params![chat, id])?;
        Ok(())
    }

    /// Forgets every stored media path, returning how many rows changed.
    pub fn clear_media_paths(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute(
            "UPDATE messages
             SET media_path = NULL,
                 media_thumb = CASE WHEN media_thumb LIKE 'data:%' THEN media_thumb END,
                 reply_to_path = NULL
             WHERE media_path IS NOT NULL OR reply_to_path IS NOT NULL
                OR media_thumb NOT LIKE 'data:%'",
            [],
        )?)
    }

    /// Records a view-once message; `opened` only ever moves from false to true.
    pub fn set_view_once(&self, chat: &str, id: &str, opened: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        conn.execute(
            "INSERT INTO view_once (chat, id, opened) VALUES (?1, ?2, ?3)
             ON CONFLICT(chat, id) DO UPDATE SET opened = MAX(opened, excluded.opened)",
            params![chat, id, opened as i32],
        )?;
        Ok(())
    }

    /// The copy of a view-once some stored reply in the chat carried in its quote.
    pub fn view_once_copy(&self, chat: &str, quoted: &str) -> Result<Option<Vec<u8>>> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let found = conn
            .query_row(
                "SELECT reply_to_locator FROM messages
                 WHERE chat = ?1 AND reply_to_id = ?2 AND reply_to_locator IS NOT NULL LIMIT 1",
                params![chat, quoted],
                |r| r.get::<_, Vec<u8>>(0),
            )
            .optional()?;
        Ok(found)
    }

    /// Replies that quote a view-once, with the copy each one carries, so the
    /// platform's behaviour can be measured rather than assumed.
    pub fn view_once_quotes(&self) -> Result<Vec<QuoteCopy>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT m.chat, m.id, m.from_me, m.reply_to_id, m.reply_to_sender,
                    m.reply_to_recoverable, m.reply_to_locator,
                    q.media_kind, q.media_ref IS NOT NULL
             FROM messages m
             LEFT JOIN messages q ON q.id = m.reply_to_id AND q.chat = m.chat
             WHERE m.reply_to_view_once = 1 ORDER BY m.rowid DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(QuoteCopy {
                chat: r.get(0)?,
                id: r.get(1)?,
                from_me: r.get::<_, i32>(2)? != 0,
                quoted: r.get(3)?,
                sender: r.get(4)?,
                allowed: r.get::<_, i32>(5)? != 0,
                locator: r.get(6)?,
                quoted_kind: r.get(7)?,
                quoted_stored: r.get::<_, Option<i32>>(8)?.unwrap_or(0) != 0,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The newest reply quoting `id` that carries a copy of it.
    pub fn quote_source_for(&self, id: &str) -> Result<Option<QuoteSource>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT chat, id, reply_to_locator FROM messages
                  WHERE reply_to_id = ?1 AND reply_to_locator IS NOT NULL AND reply_to_locator != ''
                  ORDER BY rowid DESC LIMIT 1",
                params![id],
                |row| Ok(QuoteSource { chat: row.get(0)?, id: row.get(1)?, locator: row.get(2)? }),
            )
            .optional()?)
    }

    /// Whether a message is marked view-once, whether or not it has been opened.
    pub fn is_view_once(&self, chat: &str, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        Ok(conn
            .query_row(
                "SELECT 1 FROM view_once WHERE chat = ?1 AND id = ?2",
                params![chat, id],
                |r| r.get::<_, i32>(0),
            )
            .optional()?.is_some())
    }

    /// Opens a view-once message: marks it and forgets its file, returning the path to delete.
    pub fn open_view_once(&self, chat: &str, id: &str) -> Result<Option<String>> {
        self.set_view_once(chat, id, true)?;
        let conn = self.conn.lock().unwrap();
        let chat = &*names::canonical_chat(&conn, chat)?;
        let path: Option<String> = conn
            .query_row(
                "SELECT media_path FROM messages WHERE chat = ?1 AND id = ?2",
                params![chat, id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        conn.execute(
            "UPDATE messages SET media_path = NULL, media_thumb = NULL, media_ref = NULL
             WHERE chat = ?1 AND id = ?2",
            params![chat, id],
        )?;
        Ok(path)
    }

    /// Downloaded files of one media kind, newest first, each file once.
    pub fn recent_media(&self, kind: &str, limit: u32) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let rows = conn
            .prepare(
                "SELECT media_path FROM messages
                 WHERE media_kind = ?1 AND media_path IS NOT NULL AND revoked = 0
                 GROUP BY media_path ORDER BY MAX(timestamp) DESC LIMIT ?2",
            )?
            .query_map(params![kind, limit], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Moves the media files this store references out of `from` into `to`,
    /// rewriting the stored paths, and returns how many paths were rewritten.
    ///
    /// Only referenced files move, so `from` may be a folder shared with other
    /// programs. A reference whose file is gone is also relinked when a file of
    /// the same name is found in `from` or `to`.
    pub fn relocate_media(&self, from: &[&Path], to: &Path) -> Result<usize> {
        const COLUMNS: [&str; 5] = ["media_path", "media_thumb", "reply_to_path", "reply_to_thumb", "preview_thumb"];
        let conn = self.conn.lock().unwrap();
        let mut rewritten = 0;
        for column in COLUMNS {
            let paths: Vec<String> = conn
                .prepare(&format!("SELECT DISTINCT {column} FROM messages WHERE {column} IS NOT NULL AND {column} NOT LIKE 'data:%'"))?
                .query_map([], |row| row.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            for old in paths {
                let old_path = Path::new(&old);
                let Some(name) = old_path.file_name() else { continue };
                let in_from = from.iter().any(|dir| old_path.starts_with(dir));
                if old_path.starts_with(to) || (!in_from && old_path.exists()) {
                    continue;
                }
                let dest = to.join(name);
                if !dest.exists() {
                    let source = std::iter::once(old_path.to_path_buf())
                        .filter(|_| in_from)
                        .chain(from.iter().map(|dir| dir.join(name)))
                        .find(|p| p.is_file());
                    let Some(source) = source else { continue };
                    std::fs::create_dir_all(to)?;
                    if std::fs::rename(&source, &dest).is_err() {
                        // A different filesystem cannot be renamed across.
                        std::fs::copy(&source, &dest)?;
                        if let Err(error) = std::fs::remove_file(&source) {
                            log::error!("media copied, but old file could not be removed: {error}");
                        }
                    }
                }
                rewritten += conn.execute(
                    &format!("UPDATE messages SET {column} = ?2 WHERE {column} = ?1"),
                    params![old, dest.to_string_lossy()],
                )?;
            }
        }
        Ok(rewritten)
    }
}

impl StoreWorker {
    pub(crate) async fn media_paths(&self) -> Result<Vec<String>> {
        self.run(move |store| store.media_paths()).await
    }

    pub(crate) async fn set_quote_media_path(&self, chat: &str, id: &str, path: &str) -> Result<()> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        let path = path.to_owned();
        self.run(move |store| store.set_quote_media_path(&chat, &id, &path)).await
    }

    pub(crate) async fn media_ref_for(&self, chat: &str, id: &str) -> Result<Option<Vec<u8>>> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        self.run(move |store| store.media_ref_for(&chat, &id)).await
    }

    pub(crate) async fn set_media_ref(&self, chat: &str, id: &str, media_ref: &[u8]) -> Result<()> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        let media_ref = media_ref.to_vec();
        self.run(move |store| store.set_media_ref(&chat, &id, &media_ref)).await
    }

    pub(crate) async fn set_media_path(&self, chat: &str, id: &str, path: &str) -> Result<()> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        let path = path.to_owned();
        self.run(move |store| store.set_media_path(&chat, &id, &path)).await
    }

    pub(crate) async fn set_media_thumb(&self, chat: &str, id: &str, thumb: &str) -> Result<()> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        let thumb = thumb.to_owned();
        self.run(move |store| store.set_media_thumb(&chat, &id, &thumb)).await
    }

    pub(crate) async fn set_once_kind(&self, chat: &str, id: &str, kind: &str) -> Result<()> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        let kind = kind.to_owned();
        self.run(move |store| store.set_once_kind(&chat, &id, &kind)).await
    }

    pub(crate) async fn pending_view_once(&self, within: std::time::Duration) -> Result<Vec<(String, String)>> {
        self.run(move |store| store.pending_view_once(within)).await
    }

    pub(crate) async fn keep_view_once(&self, chat: &str, id: &str) -> Result<()> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        self.run(move |store| store.keep_view_once(&chat, &id)).await
    }

    pub(crate) async fn set_view_once(&self, chat: &str, id: &str, opened: bool) -> Result<()> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        self.run(move |store| store.set_view_once(&chat, &id, opened)).await
    }

    pub(crate) async fn view_once_copy(&self, chat: &str, quoted: &str) -> Result<Option<Vec<u8>>> {
        let chat = chat.to_owned();
        let quoted = quoted.to_owned();
        self.run(move |store| store.view_once_copy(&chat, &quoted)).await
    }

    pub(crate) async fn quote_source_for(&self, id: &str) -> Result<Option<QuoteSource>> {
        let id = id.to_owned();
        self.run(move |store| store.quote_source_for(&id)).await
    }

    pub(crate) async fn is_view_once(&self, chat: &str, id: &str) -> Result<bool> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        self.run(move |store| store.is_view_once(&chat, &id)).await
    }

    pub(crate) async fn open_view_once(&self, chat: &str, id: &str) -> Result<Option<String>> {
        let chat = chat.to_owned();
        let id = id.to_owned();
        self.run(move |store| store.open_view_once(&chat, &id)).await
    }

    pub(crate) async fn recent_media(&self, kind: &str, limit: u32) -> Result<Vec<String>> {
        let kind = kind.to_owned();
        self.run(move |store| store.recent_media(&kind, limit)).await
    }
}
