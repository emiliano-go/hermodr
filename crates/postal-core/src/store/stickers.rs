//! Sticker packs and stickers, kept the way WhatsApp's app-state sync addresses
//! them: a pack id and the base64 SHA-256 of the sticker file. Upsert-only.

use super::*;

/// A sticker pack, received in a message or fetched by id.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct StickerPack {
    #[serde(rename = "pack_id")]
    pub pack_id: String,
    pub name: Option<String>,
    pub publisher: Option<String>,
    #[serde(rename = "tray_path")]
    pub tray_path: Option<String>,
    pub origin: Option<String>,
    #[serde(rename = "updated_at")]
    pub updated_at: i64,
}

/// One sticker in a pack, or one kept from a message.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct Sticker {
    /// Base64 SHA-256 of the decrypted file: the app-state index key.
    pub filehash: String,
    #[serde(rename = "pack_id")]
    pub pack_id: Option<String>,
    pub path: Option<String>,
    pub animated: bool,
    pub lottie: bool,
    pub emojis: Vec<String>,
    pub favorite: bool,
    #[serde(rename = "recent_at")]
    pub recent_at: Option<i64>,
    #[serde(rename = "updated_at")]
    pub updated_at: i64,
}

fn sticker_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Sticker> {
    Ok(Sticker {
        filehash: row.get(0)?,
        pack_id: row.get(1)?,
        path: row.get(2)?,
        animated: row.get::<_, i32>(3)? != 0,
        lottie: row.get::<_, i32>(4)? != 0,
        emojis: row
            .get::<_, Option<String>>(5)?
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default(),
        favorite: row.get::<_, i32>(6)? != 0,
        recent_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

const STICKER_COLUMNS: &str =
    "filehash, pack_id, path, animated, lottie, emojis, favorite, recent_at, updated_at";

fn pack_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StickerPack> {
    Ok(StickerPack {
        pack_id: row.get(0)?,
        name: row.get(1)?,
        publisher: row.get(2)?,
        tray_path: row.get(3)?,
        origin: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

impl MessageStore {
    /// Inserts or updates a pack without ever clearing what is already known.
    pub fn upsert_sticker_pack(&self, pack: &StickerPack) -> Result<()> {
        self.upsert_sticker_pack_changed(pack).map(|_| ())
    }

    pub fn upsert_sticker_pack_changed(&self, pack: &StickerPack) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let previous:Option<StickerPack>=conn.query_row("SELECT pack_id,name,publisher,tray_path,origin,updated_at FROM sticker_packs WHERE pack_id=?1",
            [&pack.pack_id],pack_row).optional()?;
        conn.execute(
            "INSERT INTO sticker_packs (pack_id, name, publisher, tray_path, origin, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(pack_id) DO UPDATE SET
                 name = COALESCE(excluded.name, name),
                 publisher = COALESCE(excluded.publisher, publisher),
                 tray_path = COALESCE(excluded.tray_path, tray_path),
                 origin = COALESCE(excluded.origin, origin),
                 updated_at = MAX(updated_at, excluded.updated_at)",
            params![
                pack.pack_id,
                pack.name,
                pack.publisher,
                pack.tray_path,
                pack.origin,
                pack.updated_at
            ],
        )?;
        let current:StickerPack=conn.query_row("SELECT pack_id,name,publisher,tray_path,origin,updated_at FROM sticker_packs WHERE pack_id=?1",[&pack.pack_id],pack_row)?;
        let previous = previous.map(|mut value| {
            value.updated_at = current.updated_at;
            value
        });
        Ok(previous.as_ref() != Some(&current))
    }

    /// Inserts or updates a sticker, keeping the fields the caller did not send.
    pub fn upsert_sticker(&self, sticker: &Sticker) -> Result<()> {
        self.upsert_sticker_changed(sticker).map(|_| ())
    }

    pub fn upsert_sticker_changed(&self, sticker: &Sticker) -> Result<bool> {
        let emojis = (!sticker.emojis.is_empty())
            .then(|| serde_json::to_string(&sticker.emojis))
            .transpose()?;
        let conn = self.conn.lock().unwrap();
        let previous: Option<Sticker> = conn
            .query_row(
                &format!("SELECT {STICKER_COLUMNS} FROM stickers WHERE filehash=?1"),
                [&sticker.filehash],
                sticker_row,
            )
            .optional()?;
        conn.execute(
            "INSERT INTO stickers (filehash, pack_id, path, animated, lottie, emojis, favorite, recent_at, updated_at,recent_sent_ms,recent_updated_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,COALESCE(?8*1000,0),COALESCE(?8*1000,0))
             ON CONFLICT(filehash) DO UPDATE SET
                 pack_id = COALESCE(excluded.pack_id, pack_id),
                 path = COALESCE(excluded.path, path),
                 animated = MAX(animated, excluded.animated),
                 lottie = MAX(lottie, excluded.lottie),
                 emojis = COALESCE(excluded.emojis, emojis),
                 favorite = CASE WHEN favorite_updated_ms=0 THEN MAX(favorite, excluded.favorite) ELSE favorite END,
                 recent_at = CASE WHEN excluded.recent_sent_ms>recent_sent_ms AND excluded.recent_sent_ms>recent_removed_ms THEN excluded.recent_at ELSE recent_at END,
                 recent_updated_ms = MAX(recent_updated_ms, excluded.recent_updated_ms),
                 recent_sent_ms = MAX(recent_sent_ms, excluded.recent_sent_ms),
                 updated_at = MAX(updated_at, excluded.updated_at)",
            params![
                sticker.filehash,
                sticker.pack_id,
                sticker.path,
                sticker.animated as i32,
                sticker.lottie as i32,
                emojis,
                sticker.favorite as i32,
                sticker.recent_at,
                sticker.updated_at
            ],
        )?;
        let current: Sticker = conn.query_row(
            &format!("SELECT {STICKER_COLUMNS} FROM stickers WHERE filehash=?1"),
            [&sticker.filehash],
            sticker_row,
        )?;
        let previous = previous.map(|mut value| {
            value.updated_at = current.updated_at;
            value
        });
        Ok(previous.as_ref() != Some(&current))
    }

    /// Sets the favorite flag for a sticker, creating a bare row if unknown.
    pub fn set_sticker_favorite(
        &self,
        filehash: &str,
        favorite: bool,
        updated_at: i64,
    ) -> Result<()> {
        self.observe_sticker_favorite(filehash, favorite, updated_at.saturating_mul(1000))
            .map(|_| ())
    }

    pub fn observe_sticker_favorite(
        &self,
        filehash: &str,
        favorite: bool,
        at_ms: i64,
    ) -> Result<bool> {
        anyhow::ensure!(at_ms >= 0, "Invalid sticker favorite timestamp.");
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute(
            "INSERT INTO stickers (filehash, favorite, updated_at,favorite_updated_ms) VALUES (?1, ?2, ?3/1000,?3)
             ON CONFLICT(filehash) DO UPDATE SET favorite = excluded.favorite,
                 favorite_updated_ms=excluded.favorite_updated_ms,updated_at=MAX(updated_at,excluded.updated_at)
             WHERE excluded.favorite_updated_ms>favorite_updated_ms OR (excluded.favorite_updated_ms=favorite_updated_ms AND excluded.favorite<favorite)",
            params![filehash, favorite as i32, at_ms],
        )?>0)
    }

    /// Sets (or clears) when a sticker was last sent.
    pub fn set_sticker_recent(
        &self,
        filehash: &str,
        recent_at: Option<i64>,
        updated_at: i64,
    ) -> Result<()> {
        match recent_at {
            Some(at) => self.observe_sticker_recent(filehash, at.saturating_mul(1000)),
            None => self.remove_sticker_recent(filehash, updated_at.saturating_mul(1000), None),
        }
        .map(|_| ())
    }

    pub fn observe_sticker_recent(&self, filehash: &str, sent_ms: i64) -> Result<bool> {
        anyhow::ensure!(sent_ms > 0, "Invalid sticker send timestamp.");
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute("INSERT INTO stickers(filehash,recent_at,updated_at,recent_sent_ms,recent_updated_ms)
            VALUES(?1,?2/1000,?2/1000,?2,?2) ON CONFLICT(filehash) DO UPDATE SET recent_at=excluded.recent_at,
                recent_sent_ms=excluded.recent_sent_ms,recent_updated_ms=MAX(recent_updated_ms,excluded.recent_updated_ms),updated_at=MAX(updated_at,excluded.updated_at)
            WHERE excluded.recent_sent_ms>recent_sent_ms AND excluded.recent_sent_ms>recent_removed_ms",params![filehash,sent_ms])?>0)
    }

    pub fn remove_sticker_recent(
        &self,
        filehash: &str,
        at_ms: i64,
        threshold_ms: Option<i64>,
    ) -> Result<bool> {
        anyhow::ensure!(
            at_ms >= 0 && threshold_ms.is_none_or(|at| at >= 0),
            "Invalid sticker removal timestamp."
        );
        let conn = self.conn.lock().unwrap();
        let previous: Option<(Option<i64>, i64, i64)> = conn
            .query_row(
                "SELECT recent_at,recent_updated_ms,recent_sent_ms FROM stickers WHERE filehash=?1",
                [filehash],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        if previous
            .as_ref()
            .is_some_and(|(_, clock, _)| *clock > at_ms)
        {
            return Ok(false);
        }
        let cutoff =
            threshold_ms.unwrap_or(at_ms.max(previous.as_ref().map_or(0, |(_, _, sent)| *sent)));
        conn.execute("INSERT INTO stickers(filehash,recent_at,updated_at,recent_updated_ms,recent_removed_ms) VALUES(?1,NULL,?2/1000,?2,?3)
            ON CONFLICT(filehash) DO UPDATE SET recent_at=CASE WHEN ?4 OR recent_sent_ms<=MAX(recent_removed_ms,excluded.recent_removed_ms) THEN NULL ELSE recent_at END,
                recent_updated_ms=excluded.recent_updated_ms,recent_removed_ms=MAX(recent_removed_ms,excluded.recent_removed_ms),updated_at=MAX(updated_at,excluded.updated_at)",
            params![filehash,at_ms,cutoff,threshold_ms.is_none()])?;
        Ok(previous.is_some_and(|(recent, _, sent)| {
            recent.is_some() && (threshold_ms.is_none() || sent <= cutoff)
        }))
    }

    pub fn sticker_recent_sent_ms(&self, filehash: &str) -> Result<Option<i64>> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT recent_sent_ms FROM stickers WHERE filehash=?1 AND recent_at IS NOT NULL",
                [filehash],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    /// Records where a downloaded sticker file lives.
    pub fn set_sticker_path(&self, filehash: &str, path: &str, updated_at: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO stickers (filehash, path, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(filehash) DO UPDATE SET path = excluded.path, updated_at = MAX(updated_at,excluded.updated_at)",
            params![filehash, path, updated_at],
        )?;
        Ok(())
    }

    /// Records the message locator a favorite push can rebuild its media fields from.
    pub fn set_sticker_locator(&self, filehash: &str, locator: &[u8]) -> Result<()> {
        self.set_sticker_locator_changed(filehash, locator)
            .map(|_| ())
    }

    pub fn set_sticker_locator_changed(&self, filehash: &str, locator: &[u8]) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute(
            "INSERT INTO stickers (filehash, media_ref, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(filehash) DO UPDATE SET media_ref = excluded.media_ref WHERE media_ref IS NOT excluded.media_ref",
            params![filehash, locator, unix_now()],
        )?>0)
    }

    pub fn sticker_locator(&self, filehash: &str) -> Result<Option<Vec<u8>>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT media_ref FROM stickers WHERE filehash = ?1",
                params![filehash],
                |r| r.get(0),
            )
            .optional()?
            .flatten())
    }

    pub fn sticker(&self, filehash: &str) -> Result<Option<Sticker>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                &format!("SELECT {STICKER_COLUMNS} FROM stickers WHERE filehash = ?1"),
                params![filehash],
                sticker_row,
            )
            .optional()?)
    }

    pub fn sticker_packs(&self) -> Result<Vec<StickerPack>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT pack_id, name, publisher, tray_path, origin, updated_at
             FROM sticker_packs ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(StickerPack {
                pack_id: row.get(0)?,
                name: row.get(1)?,
                publisher: row.get(2)?,
                tray_path: row.get(3)?,
                origin: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The stickers of one pack, favourites first then by recency.
    pub fn stickers_in_pack(&self, pack_id: &str) -> Result<Vec<Sticker>> {
        self.stickers_where("pack_id = ?1", params![pack_id])
    }

    /// Favourited stickers, newest favorited first.
    pub fn favorite_stickers(&self) -> Result<Vec<Sticker>> {
        self.stickers_where("favorite = 1", params![])
    }

    /// Recently sent stickers, newest first.
    pub fn recent_stickers(&self) -> Result<Vec<Sticker>> {
        self.stickers_where("recent_at IS NOT NULL", params![])
    }

    fn stickers_where(
        &self,
        predicate: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<Sticker>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {STICKER_COLUMNS} FROM stickers WHERE {predicate}
             ORDER BY favorite DESC, CASE WHEN favorite=1 THEN COALESCE(NULLIF(favorite_updated_ms,0),updated_at*1000)
                 ELSE COALESCE(NULLIF(recent_sent_ms,0),updated_at*1000) END DESC LIMIT 500"
        ))?;
        let rows = stmt.query_map(params, sticker_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

impl StoreWorker {
    pub(crate) async fn set_sticker_locator_changed(
        &self,
        hash: String,
        locator: Vec<u8>,
    ) -> Result<bool> {
        self.run(move |store| store.set_sticker_locator_changed(&hash, &locator))
            .await
    }
    pub(crate) async fn observe_sticker_favorite(
        &self,
        hash: String,
        favorite: bool,
        at_ms: i64,
    ) -> Result<bool> {
        self.run(move |store| store.observe_sticker_favorite(&hash, favorite, at_ms))
            .await
    }
    pub(crate) async fn observe_sticker_recent(&self, hash: String, sent_ms: i64) -> Result<bool> {
        self.run(move |store| store.observe_sticker_recent(&hash, sent_ms))
            .await
    }
    pub(crate) async fn remove_sticker_recent(
        &self,
        hash: String,
        at_ms: i64,
        threshold_ms: Option<i64>,
    ) -> Result<bool> {
        self.run(move |store| store.remove_sticker_recent(&hash, at_ms, threshold_ms))
            .await
    }
    pub(crate) async fn sticker_recent_sent_ms(&self, hash: String) -> Result<Option<i64>> {
        self.run(move |store| store.sticker_recent_sent_ms(&hash))
            .await
    }
    pub(crate) async fn upsert_sticker_changed(&self, sticker: Sticker) -> Result<bool> {
        self.run(move |store| store.upsert_sticker_changed(&sticker))
            .await
    }
    pub(crate) async fn upsert_sticker_pack_changed(&self, pack: StickerPack) -> Result<bool> {
        self.run(move |store| store.upsert_sticker_pack_changed(&pack))
            .await
    }
    pub(crate) async fn upsert_sticker(&self, sticker: Sticker) -> Result<()> {
        self.run(move |store| store.upsert_sticker(&sticker)).await
    }

    pub(crate) async fn set_sticker_path(
        &self,
        filehash: String,
        path: String,
        updated_at: i64,
    ) -> Result<()> {
        self.run(move |store| store.set_sticker_path(&filehash, &path, updated_at))
            .await
    }

    pub(crate) async fn sticker(&self, filehash: String) -> Result<Option<Sticker>> {
        self.run(move |store| store.sticker(&filehash)).await
    }

    pub(crate) async fn set_sticker_locator(
        &self,
        filehash: String,
        locator: Vec<u8>,
    ) -> Result<()> {
        self.run(move |store| store.set_sticker_locator(&filehash, &locator))
            .await
    }

    pub(crate) async fn sticker_locator(&self, filehash: String) -> Result<Option<Vec<u8>>> {
        self.run(move |store| store.sticker_locator(&filehash))
            .await
    }

    pub(crate) async fn sticker_packs(&self) -> Result<Vec<StickerPack>> {
        self.run(move |store| store.sticker_packs()).await
    }

    pub(crate) async fn stickers_in_pack(&self, pack_id: String) -> Result<Vec<Sticker>> {
        self.run(move |store| store.stickers_in_pack(&pack_id))
            .await
    }

    pub(crate) async fn favorite_stickers(&self) -> Result<Vec<Sticker>> {
        self.run(move |store| store.favorite_stickers()).await
    }

    pub(crate) async fn recent_stickers(&self) -> Result<Vec<Sticker>> {
        self.run(move |store| store.recent_stickers()).await
    }
}

#[cfg(test)]
#[path = "stickers_tests.rs"]
mod tests;
