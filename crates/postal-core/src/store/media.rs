//! Media: locators, downloaded files, view-once state and relocation.

use super::*;

impl MessageStore {
    /// The stored media reference for a message.
    pub fn media_ref_for(&self, chat: &str, id: &str) -> Result<Option<Vec<u8>>> {
        let conn = self.conn.lock().unwrap();
        let value = conn
            .query_row(
                "SELECT media_ref FROM messages WHERE chat = ?1 AND id = ?2",
                params![chat, id],
                |r| r.get::<_, Option<Vec<u8>>>(0),
            )
            .ok()
            .flatten();
        Ok(value)
    }

    /// Records where a downloaded file was written.
    pub fn set_media_path(&self, chat: &str, id: &str, path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE messages SET media_path = ?3 WHERE chat = ?1 AND id = ?2",
            params![chat, id, path],
        )?;
        Ok(())
    }

    /// Forgets every stored media path, returning how many rows changed.
    pub fn clear_media_paths(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute(
            "UPDATE messages
             SET media_path = NULL,
                 media_thumb = CASE WHEN media_thumb LIKE 'data:%' THEN media_thumb END
             WHERE media_path IS NOT NULL OR media_thumb NOT LIKE 'data:%'",
            [],
        )?)
    }

    /// Records a view-once message; `opened` only ever moves from false to true.
    pub fn set_view_once(&self, chat: &str, id: &str, opened: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO view_once (chat, id, opened) VALUES (?1, ?2, ?3)
             ON CONFLICT(chat, id) DO UPDATE SET opened = MAX(opened, excluded.opened)",
            params![chat, id, opened as i32],
        )?;
        Ok(())
    }

    /// Opens a view-once message: marks it and forgets its file, returning the path to delete.
    pub fn open_view_once(&self, chat: &str, id: &str) -> Result<Option<String>> {
        self.set_view_once(chat, id, true)?;
        let conn = self.conn.lock().unwrap();
        let path: Option<String> = conn
            .query_row(
                "SELECT media_path FROM messages WHERE chat = ?1 AND id = ?2",
                params![chat, id],
                |r| r.get(0),
            )
            .ok()
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
        const COLUMNS: [&str; 4] = ["media_path", "media_thumb", "reply_to_thumb", "preview_thumb"];
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
                        let _ = std::fs::remove_file(&source);
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
