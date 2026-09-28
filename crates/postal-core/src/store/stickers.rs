//! Sticker packs and stickers, kept the way WhatsApp's app-state sync addresses
//! them: a pack id and the base64 SHA-256 of the sticker file. Upsert-only.

use super::*;

/// A sticker pack, received in a message or fetched by id.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
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

impl MessageStore {
    /// Inserts or updates a pack without ever clearing what is already known.
    pub fn upsert_sticker_pack(&self, pack: &StickerPack) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sticker_packs (pack_id, name, publisher, tray_path, origin, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(pack_id) DO UPDATE SET
                 name = COALESCE(excluded.name, name),
                 publisher = COALESCE(excluded.publisher, publisher),
                 tray_path = COALESCE(excluded.tray_path, tray_path),
                 origin = COALESCE(excluded.origin, origin),
                 updated_at = excluded.updated_at",
            params![pack.pack_id, pack.name, pack.publisher, pack.tray_path, pack.origin, pack.updated_at],
        )?;
        Ok(())
    }

    /// Inserts or updates a sticker, keeping the fields the caller did not send.
    pub fn upsert_sticker(&self, sticker: &Sticker) -> Result<()> {
        let emojis = (!sticker.emojis.is_empty())
            .then(|| serde_json::to_string(&sticker.emojis))
            .transpose()?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO stickers (filehash, pack_id, path, animated, lottie, emojis, favorite, recent_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(filehash) DO UPDATE SET
                 pack_id = COALESCE(excluded.pack_id, pack_id),
                 path = COALESCE(excluded.path, path),
                 animated = MAX(animated, excluded.animated),
                 lottie = MAX(lottie, excluded.lottie),
                 emojis = COALESCE(excluded.emojis, emojis),
                 favorite = excluded.favorite,
                 recent_at = COALESCE(excluded.recent_at, recent_at),
                 updated_at = excluded.updated_at",
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
        Ok(())
    }

    /// Sets the favorite flag for a sticker, creating a bare row if unknown.
    pub fn set_sticker_favorite(&self, filehash: &str, favorite: bool, updated_at: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO stickers (filehash, favorite, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(filehash) DO UPDATE SET favorite = excluded.favorite, updated_at = excluded.updated_at",
            params![filehash, favorite as i32, updated_at],
        )?;
        Ok(())
    }

    /// Sets (or clears) when a sticker was last sent.
    pub fn set_sticker_recent(&self, filehash: &str, recent_at: Option<i64>, updated_at: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO stickers (filehash, recent_at, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(filehash) DO UPDATE SET recent_at = excluded.recent_at, updated_at = excluded.updated_at",
            params![filehash, recent_at, updated_at],
        )?;
        Ok(())
    }

    /// Records where a downloaded sticker file lives.
    pub fn set_sticker_path(&self, filehash: &str, path: &str, updated_at: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO stickers (filehash, path, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(filehash) DO UPDATE SET path = excluded.path, updated_at = excluded.updated_at",
            params![filehash, path, updated_at],
        )?;
        Ok(())
    }

    /// Records the message locator a favorite push can rebuild its media fields from.
    pub fn set_sticker_locator(&self, filehash: &str, locator: &[u8]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO stickers (filehash, media_ref, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(filehash) DO UPDATE SET media_ref = excluded.media_ref",
            params![filehash, locator, unix_now()],
        )?;
        Ok(())
    }

    pub fn sticker_locator(&self, filehash: &str) -> Result<Option<Vec<u8>>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row("SELECT media_ref FROM stickers WHERE filehash = ?1", params![filehash], |r| r.get(0))
            .optional()?
            .flatten())
    }

    pub fn sticker(&self, filehash: &str) -> Result<Option<Sticker>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(&format!("SELECT {STICKER_COLUMNS} FROM stickers WHERE filehash = ?1"), params![filehash], sticker_row)
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

    fn stickers_where(&self, predicate: &str, params: impl rusqlite::Params) -> Result<Vec<Sticker>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {STICKER_COLUMNS} FROM stickers WHERE {predicate}
             ORDER BY favorite DESC, COALESCE(recent_at, updated_at) DESC LIMIT 500"
        ))?;
        let rows = stmt.query_map(params, sticker_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

impl StoreWorker {
    pub(crate) async fn upsert_sticker_pack(&self, pack: StickerPack) -> Result<()> {
        self.run(move |store| store.upsert_sticker_pack(&pack)).await
    }

    pub(crate) async fn upsert_sticker(&self, sticker: Sticker) -> Result<()> {
        self.run(move |store| store.upsert_sticker(&sticker)).await
    }

    pub(crate) async fn set_sticker_favorite(&self, filehash: String, favorite: bool, updated_at: i64) -> Result<()> {
        self.run(move |store| store.set_sticker_favorite(&filehash, favorite, updated_at)).await
    }

    pub(crate) async fn set_sticker_recent(&self, filehash: String, recent_at: Option<i64>, updated_at: i64) -> Result<()> {
        self.run(move |store| store.set_sticker_recent(&filehash, recent_at, updated_at)).await
    }

    pub(crate) async fn set_sticker_path(&self, filehash: String, path: String, updated_at: i64) -> Result<()> {
        self.run(move |store| store.set_sticker_path(&filehash, &path, updated_at)).await
    }

    pub(crate) async fn sticker(&self, filehash: String) -> Result<Option<Sticker>> {
        self.run(move |store| store.sticker(&filehash)).await
    }

    pub(crate) async fn set_sticker_locator(&self, filehash: String, locator: Vec<u8>) -> Result<()> {
        self.run(move |store| store.set_sticker_locator(&filehash, &locator)).await
    }

    pub(crate) async fn sticker_locator(&self, filehash: String) -> Result<Option<Vec<u8>>> {
        self.run(move |store| store.sticker_locator(&filehash)).await
    }

    pub(crate) async fn sticker_packs(&self) -> Result<Vec<StickerPack>> {
        self.run(move |store| store.sticker_packs()).await
    }

    pub(crate) async fn stickers_in_pack(&self, pack_id: String) -> Result<Vec<Sticker>> {
        self.run(move |store| store.stickers_in_pack(&pack_id)).await
    }

    pub(crate) async fn favorite_stickers(&self) -> Result<Vec<Sticker>> {
        self.run(move |store| store.favorite_stickers()).await
    }

    pub(crate) async fn recent_stickers(&self) -> Result<Vec<Sticker>> {
        self.run(move |store| store.recent_stickers()).await
    }
}
