use super::*;
use rusqlite::{params_from_iter, types::Value};

pub const MAX_GALLERY_PAGE: u32 = 100;
const VISIBLE: &str = "m.deleted = 0 AND m.revoked = 0 AND m.system_kind IS NULL AND COALESCE(m.media_kind, '') != 'view_once'";
const MEDIA: &str = "m.media_kind IN ('image', 'video', 'round_video', 'audio', 'document', 'sticker', 'gif')";
const KIND: &str = "CASE m.media_kind WHEN 'round_video' THEN 'video' ELSE m.media_kind END";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub enum GalleryKind { Image, Video, Audio, Document, Sticker, Gif, Link }

impl GalleryKind {
    fn as_str(self) -> &'static str {
        match self { Self::Image => "image", Self::Video => "video", Self::Audio => "audio", Self::Document => "document",
            Self::Sticker => "sticker", Self::Gif => "gif", Self::Link => "link" }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct GalleryFilter {
    pub chat: Option<String>,
    pub kind: Option<GalleryKind>,
    pub from_me: Option<bool>,
    pub since: Option<i64>,
    pub until: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct GalleryCursor { pub timestamp: i64, pub sort_order: i64, pub chat: String, pub id: String }

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct GalleryItem { pub message: StoredMessage, pub urls: Vec<String> }

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct GalleryPage { pub items: Vec<GalleryItem>, pub next_cursor: Option<GalleryCursor> }

pub(super) fn create_indexes(conn: &Connection) -> Result<()> {
    let visible = VISIBLE.replace("m.", "");
    let media = MEDIA.replace("m.", "");
    let kind = KIND.replace("m.", "");
    let kind_prefix = format!("{kind},");
    let chat_kind_prefix = format!("chat,{kind},");
    for (name, prefix, predicate) in [
        ("media_all", "", media.as_str()), ("media_chat", "chat,", media.as_str()),
        ("media_kind", kind_prefix.as_str(), media.as_str()), ("media_chat_kind", chat_kind_prefix.as_str(), media.as_str()),
        ("links_all", "", "link_urls != '[]'"), ("links_chat", "chat,", "link_urls != '[]'"),
    ] {
        let order = if name.contains("chat") { "timestamp DESC, sort_order DESC, id DESC" } else { "timestamp DESC, sort_order DESC, chat DESC, id DESC" };
        conn.execute_batch(&format!("CREATE INDEX idx_gallery_{name} ON messages ({prefix}{order}) WHERE {visible} AND {predicate};"))?;
    }
    Ok(())
}

fn query(filter: &GalleryFilter, cursor: Option<&GalleryCursor>, limit: usize) -> Result<(String, Vec<Value>)> {
    anyhow::ensure!(!matches!((filter.since, filter.until), (Some(start), Some(end)) if start >= end), "date range must end after it starts");
    let links = matches!(filter.kind, Some(GalleryKind::Link));
    let suffix = if filter.chat.is_some() { "chat" } else { "all" };
    let index = if links { format!("links_{suffix}") } else if filter.kind.is_some() {
        if filter.chat.is_some() { "media_chat_kind".into() } else { "media_kind".into() }
    } else { format!("media_{suffix}") };
    let predicate = if links { "m.link_urls != '[]'" } else { MEDIA };
    let mut sql = format!("SELECT {MESSAGE_COLUMNS}, m.link_urls FROM messages m INDEXED BY idx_gallery_{index}
        LEFT JOIN names n ON n.jid = m.sender WHERE {VISIBLE} AND {predicate}");
    let mut values = Vec::new();
    for (column, value) in [("m.chat", filter.chat.clone().map(Value::Text)),
        ("m.from_me", filter.from_me.map(|v| Value::Integer(v as i64)))] {
        if let Some(value) = value { sql.push_str(&format!(" AND {column} = ?")); values.push(value); }
    }
    if let Some(kind) = filter.kind.filter(|_| !links) { sql.push_str(&format!(" AND {KIND} = ?")); values.push(Value::Text(kind.as_str().into())); }
    for (comparison, value) in [(">=", filter.since), ("<", filter.until)] {
        if let Some(value) = value { sql.push_str(&format!(" AND m.timestamp {comparison} ?")); values.push(Value::Integer(value)); }
    }
    if let Some(cursor) = cursor {
        sql.push_str(" AND (m.timestamp, m.sort_order, m.chat, m.id) < (?, ?, ?, ?)");
        values.extend([Value::Integer(cursor.timestamp), Value::Integer(cursor.sort_order), Value::Text(cursor.chat.clone()), Value::Text(cursor.id.clone())]);
    }
    sql.push_str(if filter.chat.is_some() { " ORDER BY m.timestamp DESC, m.sort_order DESC, m.id DESC LIMIT ?" }
        else { " ORDER BY m.timestamp DESC, m.sort_order DESC, m.chat DESC, m.id DESC LIMIT ?" });
    values.push(Value::Integer((limit + 1) as i64));
    Ok((sql, values))
}

impl MessageStore {
    pub fn gallery_page(&self, filter: &GalleryFilter, cursor: Option<&GalleryCursor>, limit: u32) -> Result<GalleryPage> {
        let conn = self.conn.lock().unwrap();
        let mut filter = filter.clone();
        if let Some(chat) = &filter.chat { filter.chat = Some(names::canonical_chat(&conn, chat)?.into_owned()); }
        let limit = limit.clamp(1, MAX_GALLERY_PAGE) as usize;
        let (sql, values) = query(&filter, cursor, limit)?;
        let mut items = conn.prepare(&sql)?.query_map(params_from_iter(values), |row| {
            let json: String = row.get("link_urls")?;
            let column = row.as_ref().column_index("link_urls")?;
            let urls = serde_json::from_str(&json).map_err(|error| rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Text, Box::new(error)))?;
            Ok(GalleryItem { message: message_row(row)?, urls })
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        let has_more = items.len() > limit;
        items.truncate(limit);
        let next_cursor = if has_more { items.last().map(|item| GalleryCursor {
            timestamp: item.message.header.timestamp, sort_order: item.message.local.sort_order,
            chat: item.message.header.chat.clone(), id: item.message.header.id.clone(),
        }) } else { None };
        Ok(GalleryPage { items, next_cursor })
    }
}

#[cfg(test)]
#[path = "gallery_tests.rs"]
mod tests;
