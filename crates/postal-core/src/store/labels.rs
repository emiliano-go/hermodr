use super::*;
use crate::message_ref::MessageRef;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct Label {
    pub id: String,
    pub name: String,
    pub color: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct ChatLabelAssociation {
    pub label_id: String,
    pub chat: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct MessageLabelAssociation {
    pub label_id: String,
    pub chat: String,
    pub message_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "wire-types", derive(ts_rs::TS))]
pub struct LabelsView {
    pub complete: bool,
    pub labels: Vec<Label>,
    pub chats: Vec<ChatLabelAssociation>,
    pub messages: Vec<MessageLabelAssociation>,
}

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS labels_catalog (
            id TEXT PRIMARY KEY CHECK(length(id) > 0),
            name TEXT NOT NULL DEFAULT '', color INTEGER NOT NULL DEFAULT 0,
            deleted INTEGER NOT NULL DEFAULT 0, updated_at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS chat_labels (
            label_id TEXT NOT NULL, chat TEXT NOT NULL,
            labeled INTEGER NOT NULL, updated_at INTEGER NOT NULL,
            PRIMARY KEY(label_id, chat));
         CREATE TABLE IF NOT EXISTS message_labels (
            label_id TEXT NOT NULL, chat TEXT NOT NULL, message_id TEXT NOT NULL,
            labeled INTEGER NOT NULL, updated_at INTEGER NOT NULL,
            PRIMARY KEY(label_id, chat, message_id));",
    )?;
    Ok(())
}

impl MessageStore {
    pub fn labels_view(&self) -> Result<LabelsView> {
        let conn = self.conn.lock().unwrap();
        let mut labels = conn.prepare(
            "SELECT id, name, color FROM labels_catalog WHERE deleted = 0 AND name <> '' ORDER BY name, id",
        )?;
        let labels = labels.query_map([], |row| Ok(Label {
            id: row.get(0)?, name: row.get(1)?, color: row.get(2)?,
        }))?.collect::<rusqlite::Result<_>>()?;
        let mut chats = conn.prepare(
            "SELECT a.label_id, a.chat FROM chat_labels a JOIN labels_catalog l ON l.id = a.label_id
             WHERE a.labeled = 1 AND l.deleted = 0 AND l.name <> ''
               AND NOT EXISTS (SELECT 1 FROM hidden_chats h WHERE h.jid = a.chat)
             ORDER BY a.chat, a.label_id",
        )?;
        let chats = chats.query_map([], |row| Ok(ChatLabelAssociation {
            label_id: row.get(0)?, chat: row.get(1)?,
        }))?.collect::<rusqlite::Result<_>>()?;
        let mut messages = conn.prepare(
            "SELECT a.label_id, a.chat, a.message_id FROM message_labels a
             JOIN labels_catalog l ON l.id = a.label_id
             WHERE a.labeled = 1 AND l.deleted = 0 AND l.name <> ''
               AND NOT EXISTS (SELECT 1 FROM hidden_chats h WHERE h.jid = a.chat)
             ORDER BY a.chat, a.message_id, a.label_id",
        )?;
        let messages = messages.query_map([], |row| Ok(MessageLabelAssociation {
            label_id: row.get(0)?, chat: row.get(1)?, message_id: row.get(2)?,
        }))?.collect::<rusqlite::Result<_>>()?;
        Ok(LabelsView { complete: false, labels, chats, messages })
    }

    pub fn labelled_messages(&self, label_ids: &[String], chat: Option<&str>, query: &str, limit: u32) -> Result<Vec<StoredMessage>> {
        anyhow::ensure!(!label_ids.is_empty() && label_ids.len() <= 50 && label_ids.iter().all(|id| !id.is_empty()),
            MessageRef::new("error.label_selection_invalid").with_param("min", serde_json::Number::from(1))
                .with_param("max", serde_json::Number::from(50)).with_param("actual", serde_json::Number::from(label_ids.len() as u64)));
        let ids = serde_json::to_string(label_ids)?;
        let escaped = query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_").to_lowercase();
        let pattern = format!("%{escaped}%");
        let conn = self.conn.lock().unwrap();
        let chat = chat.map(|chat| names::canonical_chat(&conn, chat).map(|chat| chat.to_string())).transpose()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages m LEFT JOIN names n ON n.jid = m.sender
             WHERE {VISIBLE_MESSAGE_SQL} AND m.deleted = 0 AND m.revoked = 0 AND m.spoiler = 0
               AND m.system_kind IS NULL AND m.media_once_kind IS NULL
               AND COALESCE(m.media_kind, '') NOT IN ('view_once', 'unknown')
               AND NOT EXISTS (SELECT 1 FROM view_once v WHERE v.chat = m.chat AND v.id = m.id)
               AND NOT EXISTS (SELECT 1 FROM hidden_chats h WHERE h.jid = m.chat)
               AND EXISTS (SELECT 1 FROM message_labels a JOIN labels_catalog l ON l.id = a.label_id
                   WHERE a.chat = m.chat AND a.message_id = m.id AND a.labeled = 1
                     AND l.deleted = 0 AND l.name <> '' AND a.label_id IN (SELECT value FROM json_each(?1)))
               AND (?2 IS NULL OR m.chat = ?2)
               AND (lower(m.text) LIKE ?3 ESCAPE '\\' OR lower(m.link_urls) LIKE ?3 ESCAPE '\\')
             ORDER BY m.timestamp DESC, m.sort_order DESC, m.id DESC, m.chat ASC LIMIT ?4"
        ))?;
        let rows = stmt.query_map(params![ids, chat, pattern, limit.clamp(1, 500)], message_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub(crate) fn label_exists(&self, id: &str) -> Result<bool> {
        Ok(self.conn.lock().unwrap().query_row(
            "SELECT EXISTS(SELECT 1 FROM labels_catalog WHERE id = ?1 AND deleted = 0 AND name <> '')",
            [id], |row| row.get(0),
        )?)
    }

    pub(crate) fn label_id_known(&self, id: &str) -> Result<bool> {
        Ok(self.conn.lock().unwrap().query_row(
            "SELECT EXISTS(SELECT 1 FROM labels_catalog WHERE id = ?1)",
            [id], |row| row.get(0),
        )?)
    }

    pub(crate) fn set_label(
        &self, id: &str, name: Option<&str>, color: Option<i32>, deleted: Option<bool>, timestamp: i64,
    ) -> Result<bool> {
        anyhow::ensure!(!id.is_empty() && timestamp >= 0, MessageRef::new("error.label_update_invalid"));
        if name.is_none() && color.is_none() && deleted.is_none() { return Ok(false); }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        let changed = tx.execute(
            "INSERT INTO labels_catalog (id, name, color, deleted, updated_at)
             VALUES (?1, COALESCE(?2, ''), COALESCE(?3, 0), COALESCE(?4, 0), ?5)
             ON CONFLICT(id) DO UPDATE SET
               name = COALESCE(?2, labels_catalog.name), color = COALESCE(?3, labels_catalog.color),
               deleted = COALESCE(?4, labels_catalog.deleted), updated_at = ?5
             WHERE ?5 > labels_catalog.updated_at OR (?5 = labels_catalog.updated_at AND
               (COALESCE(?2, labels_catalog.name) <> labels_catalog.name OR
                COALESCE(?3, labels_catalog.color) <> labels_catalog.color OR
                COALESCE(?4, labels_catalog.deleted) <> labels_catalog.deleted))",
            params![id, name, color, deleted, timestamp],
        )? > 0;
        if changed && deleted == Some(true) {
            for table in ["chat_labels", "message_labels"] {
                tx.execute(&format!(
                    "UPDATE {table} SET labeled = 0, updated_at = ?2 WHERE label_id = ?1 AND updated_at <= ?2"
                ), params![id, timestamp])?;
            }
        }
        tx.commit()?;
        Ok(changed)
    }

    pub(crate) fn set_chat_label(&self, label_id: &str, chat: &str, labeled: bool, timestamp: i64) -> Result<bool> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.savepoint()?;
        let chat = names::canonical_chat(&tx, chat)?;
        let changed = association(&tx, label_id, &chat, None, labeled, timestamp)?;
        tx.commit()?;
        Ok(changed)
    }

    pub(crate) fn set_message_label(
        &self, label_id: &str, chat: &str, message_id: &str, labeled: bool, timestamp: i64,
    ) -> Result<bool> {
        anyhow::ensure!(!message_id.is_empty(), MessageRef::new("error.message_required"));
        let conn = self.conn.lock().unwrap();
        let chat = names::canonical_chat(&conn, chat)?;
        association(&conn, label_id, &chat, Some(message_id), labeled, timestamp)
    }
}

fn association(conn: &Connection, label: &str, chat: &str, message: Option<&str>, labeled: bool, timestamp: i64) -> Result<bool> {
    anyhow::ensure!(!label.is_empty() && !chat.is_empty() && timestamp >= 0, MessageRef::new("error.label_association_invalid"));
    let deletion: Option<i64> = conn.query_row(
        "SELECT updated_at FROM labels_catalog WHERE id = ?1 AND deleted = 1", [label], |row| row.get(0),
    ).optional()?;
    let labeled = labeled && deletion.is_none_or(|deleted| timestamp > deleted);
    let timestamp = deletion.map_or(timestamp, |deleted| timestamp.max(deleted));
    let changed = if let Some(message) = message {
        conn.execute(
            "INSERT INTO message_labels (label_id, chat, message_id, labeled, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(label_id, chat, message_id) DO UPDATE SET labeled = ?4, updated_at = ?5
             WHERE ?5 > message_labels.updated_at OR (?5 = message_labels.updated_at AND ?4 <> message_labels.labeled)",
            params![label, chat, message, labeled, timestamp],
        )?
    } else {
        let changed = conn.execute(
            "INSERT INTO chat_labels (label_id, chat, labeled, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(label_id, chat) DO UPDATE SET labeled = ?3, updated_at = ?4
             WHERE ?4 > chat_labels.updated_at OR (?4 = chat_labels.updated_at AND ?3 <> chat_labels.labeled)",
            params![label, chat, labeled, timestamp],
        )?;
        if changed > 0 && labeled {
            conn.execute("INSERT OR IGNORE INTO chats (jid) VALUES (?1)", [chat])?;
        }
        changed
    };
    Ok(changed > 0)
}

pub(super) fn merge(conn: &Connection, from: &str, to: &str) -> Result<()> {
    for (table, keys, columns) in [
        ("chat_labels", "label_id, chat", "label_id, ?1, labeled, updated_at"),
        ("message_labels", "label_id, chat, message_id", "label_id, ?1, message_id, labeled, updated_at"),
    ] {
        conn.execute(&format!(
            "INSERT INTO {table} SELECT {columns} FROM {table} WHERE chat = ?2
             ON CONFLICT({keys}) DO UPDATE SET labeled = excluded.labeled, updated_at = excluded.updated_at
             WHERE excluded.updated_at > {table}.updated_at OR
               (excluded.updated_at = {table}.updated_at AND excluded.labeled < {table}.labeled)"
        ), params![to, from])?;
        conn.execute(&format!("DELETE FROM {table} WHERE chat = ?1"), [from])?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "labels_tests.rs"]
mod tests;
